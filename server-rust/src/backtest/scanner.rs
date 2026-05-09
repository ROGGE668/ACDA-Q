//! 市场扫描器 — 对全市场标的逐只运行策略，输出评分排序列表
//!
//! 简化版：用 Buy&Hold 策略对每只股票执行快速回测，按综合评分排序。

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::backtest::context::Context;
use crate::backtest::datafeed;
use crate::backtest::engine::{Engine, Strategy};
use crate::error::AppError;

#[derive(Debug, Serialize)]
pub struct ScanResultItem {
    pub symbol: String,
    pub name: String,
    pub score: Decimal,
    pub total_return: Decimal,
    pub sharpe_ratio: Decimal,
    pub max_drawdown: Decimal,
    pub total_trades: u64,
}

/// 扫描策略（内嵌，避免循环依赖）
struct ScanBuyHold {
    has_bought: bool,
}

impl ScanBuyHold {
    fn new() -> Self {
        Self { has_bought: false }
    }
}

impl Strategy for ScanBuyHold {
    fn on_bar(&mut self, ctx: &mut Context) {
        if self.has_bought {
            return;
        }
        let n = ctx.bar_group.len();
        if n == 0 {
            return;
        }
        let weight = Decimal::ONE / Decimal::from(n as u64);
        for bar in ctx.bar_group {
            ctx.buy(&bar.symbol, weight);
        }
        self.has_bought = true;
    }
}

/// 扫描全市场标的
pub async fn scan_market(
    db: &PgPool,
    ts_db: &PgPool,
    top_n: usize,
    score_threshold: Decimal,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<ScanResultItem>, AppError> {
    info!(
        "Starting market scan, top_n={}, threshold={}",
        top_n, score_threshold
    );

    let stocks: Vec<(String, String)> = sqlx::query_as(
        "SELECT symbol, name FROM stock_basic WHERE is_active = TRUE ORDER BY symbol LIMIT 100",
    )
    .fetch_all(db)
    .await?;

    let mut results = Vec::new();

    for (symbol, name) in stocks {
        let bars = match datafeed::load_symbol_bars(ts_db, &symbol, start_date, end_date).await {
            Ok(b) if b.len() >= 20 => b,
            _ => continue,
        };

        let perf = match tokio::task::spawn_blocking(move || {
            let mut engine = Engine::new(dec!(1_000_000));
            let mut strategy = ScanBuyHold::new();
            engine.run(&mut strategy, &bars)
        })
        .await
        {
            Ok(p) => p,
            Err(e) => {
                warn!("Scan task panicked for {}: {}", symbol, e);
                continue;
            }
        };

        let score = perf.sharpe_ratio * dec!(0.4)
            + perf.total_return * dec!(0.4)
            - perf.max_drawdown.abs() * dec!(0.2);

        if score >= score_threshold {
            results.push(ScanResultItem {
                symbol,
                name,
                score,
                total_return: perf.total_return,
                sharpe_ratio: perf.sharpe_ratio,
                max_drawdown: perf.max_drawdown,
                total_trades: perf.total_trades,
            });
        }
    }

    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(top_n);

    info!("Scan complete: {} suitable stocks", results.len());
    Ok(results)
}
