//! 回测任务处理器 — Worker 核心逻辑
//!
//! 消费 Redis Streams 任务，执行内置策略回测，保存结果。
//! Phase 1 支持内置策略（BuyAndHold / DualMA），自定义策略待 Phase 2。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};
use crate::backtest::context::Context;
use crate::backtest::datafeed;
use crate::backtest::engine::{Engine, Strategy};
use crate::backtest::scanner;
use crate::backtest::types::{Bar, Performance};
use crate::config::Settings;
use crate::models::BacktestJob;
use crate::queue::{BacktestPayload, Queue, Task, TaskStatus, Worker};
use sha2::{Digest, Sha256};

/// 报告文件根目录
fn report_dir() -> PathBuf {
    std::env::var("REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./reports"))
}

/// 保存报告到 JSON 文件
fn save_json_report(job_id: &str, data: &serde_json::Value) -> Result<String, std::io::Error> {
    let dir = report_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", job_id));
    std::fs::write(&path, serde_json::to_string_pretty(data)?)?;
    Ok(path.to_string_lossy().to_string())
}

fn save_report(job_id: &str, perf: &Performance) -> Result<String, std::io::Error> {
    let dir = report_dir();
    std::fs::create_dir_all(&dir)?;

    let safe_name = format!("{}.json", job_id);
    let path = dir.join(&safe_name);

    let report = json!({
        "job_id": job_id,
        "total_return": perf.total_return.to_string(),
        "annual_return": perf.annual_return.to_string(),
        "max_drawdown": perf.max_drawdown.to_string(),
        "sharpe_ratio": perf.sharpe_ratio.to_string(),
        "sortino_ratio": perf.sortino_ratio.to_string(),
        "calmar_ratio": perf.calmar_ratio.to_string(),
        "win_rate": perf.win_rate.to_string(),
        "profit_ratio": perf.profit_ratio.to_string(),
        "total_trades": perf.total_trades,
        "total_commission": perf.total_commission.to_string(),
        "final_value": perf.final_value.to_string(),
        "duration_days": perf.duration_days,
        "trading_days": perf.trading_days,
        "monthly_returns": perf.monthly_returns,
    });

    std::fs::write(&path, serde_json::to_string_pretty(&report)?)?;
    Ok(path.to_string_lossy().to_string())
}

// ========== 内置策略 ==========

/// 买入持有策略：第一天买入，持有到最后
pub struct BuyAndHold {
    has_bought: bool,
}

impl BuyAndHold {
    pub fn new() -> Self {
        Self { has_bought: false }
    }
}

impl Strategy for BuyAndHold {
    fn on_bar(&mut self, ctx: &mut Context) {
        if self.has_bought {
            return;
        }
        let n = ctx.bar_group.len();
        if n == 0 {
            return;
        }
        // 等权重分配
        let weight = Decimal::ONE / Decimal::from(n as u64);
        for bar in ctx.bar_group {
            ctx.buy(&bar.symbol, weight);
        }
        self.has_bought = true;
    }
}

/// 双均线策略
pub struct DualMA {
    short_window: usize,
    long_window: usize,
    holding: bool,
}

impl DualMA {
    pub fn new(short_window: usize, long_window: usize) -> Self {
        Self {
            short_window,
            long_window,
            holding: false,
        }
    }
}

impl Strategy for DualMA {
    fn on_bar(&mut self, ctx: &mut Context) {
        for bar in ctx.bar_group {
            let sym = &bar.symbol;
            let hist = ctx.history(sym, self.long_window + 2);
            if hist.len() < self.long_window {
                continue;
            }

            let short_ma: Decimal = hist.iter().rev().take(self.short_window).sum::<Decimal>()
                / Decimal::from(self.short_window as u64);
            let long_ma: Decimal = hist.iter().rev().take(self.long_window).sum::<Decimal>()
                / Decimal::from(self.long_window as u64);

            let prev_short: Decimal = hist[..hist.len() - 1]
                .iter()
                .rev()
                .take(self.short_window)
                .sum::<Decimal>()
                / Decimal::from(self.short_window as u64);
            let prev_long: Decimal = hist[..hist.len() - 1]
                .iter()
                .rev()
                .take(self.long_window)
                .sum::<Decimal>()
                / Decimal::from(self.long_window as u64);

            if prev_short <= prev_long && short_ma > long_ma && !self.holding {
                ctx.buy(sym, dec!(0.5));
                self.holding = true;
            } else if prev_short >= prev_long && short_ma < long_ma && self.holding {
                ctx.sell(sym, Decimal::ONE);
                self.holding = false;
            }
        }
    }
}

// ========== BacktestWorker ==========

/// 为多个 symbol 生成 mock 数据（fallback 用）
fn generate_mock_bars_for_symbols(symbols: &[String]) -> Vec<Bar> {
    let mut all_bars = Vec::new();
    for (i, symbol) in symbols.iter().enumerate() {
        let seed = 42 + i as u64;
        let bars = crate::backtest::engine::generate_mock_bars(symbol, 252, seed);
        all_bars.extend(bars);
    }
    all_bars
}

pub struct BacktestWorker {
    pub db: PgPool,
    pub ts_db: PgPool,
    pub queue: Queue,
    pub settings: Arc<Settings>,
}

impl BacktestWorker {
    pub fn new(db: PgPool, ts_db: PgPool, queue: Queue, settings: Arc<Settings>) -> Self {
        Self {
            db,
            ts_db,
            queue,
            settings,
        }
    }
}

#[async_trait]
impl Worker<BacktestPayload> for BacktestWorker {
    async fn process(&self, task: &Task<BacktestPayload>) -> Result<(), String> {
        let payload = &task.payload;
        let job_id_str = payload.job_id.to_string();

        info!("Processing backtest job: {}", job_id_str);

        // 1. 更新状态为 running
        self.queue
            .update_task_status(&task.id, TaskStatus::Running, None)
            .await
            .map_err(|e| format!("Failed to update status: {}", e))?;
        self.queue
            .publish_progress(&job_id_str, 0.1, "开始执行回测", "running")
            .await
            .ok();

        // 2. 解析参数
        let initial_cash = payload
            .initial_cash
            .parse::<Decimal>()
            .map_err(|e| format!("Invalid initial_cash: {}", e))?;

        let strategy_type = payload
            .params
            .get("strategy_type")
            .and_then(|v| v.as_str())
            .unwrap_or("buy_and_hold");

        let symbols = payload.symbols.clone();
        if symbols.is_empty() {
            return Err("No symbols provided".to_string());
        }

        // 3. 加载历史数据：优先从 TimescaleDB 加载真实数据，失败则 fallback 到 mock
        let mut all_bars = match datafeed::load_daily_bars(
            &self.ts_db,
            &symbols,
            &payload.start_date,
            &payload.end_date,
        )
        .await
        {
            Ok(bars) if !bars.is_empty() => {
                info!("Loaded {} real bars from TimescaleDB", bars.len());
                bars
            }
            Ok(_) => {
                warn!("No real data found for {:?}, falling back to mock bars", symbols);
                generate_mock_bars_for_symbols(&symbols)
            }
            Err(e) => {
                warn!("Failed to load real data ({}), falling back to mock bars", e);
                generate_mock_bars_for_symbols(&symbols)
            }
        };
        all_bars.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        if all_bars.is_empty() {
            return Err("No bars loaded".to_string());
        }

        self.queue
            .publish_progress(&job_id_str, 0.3, "数据加载完成", "running")
            .await
            .ok();

        // 4. 创建 Engine（使用 Settings 中的费率）
        let mut engine = Engine::new(initial_cash).with_options(
            self.settings.backtest_commission,
            self.settings.backtest_slippage,
            self.settings.backtest_stamp_duty,
            self.settings.backtest_transfer_fee,
        );

        self.queue
            .publish_progress(&job_id_str, 0.5, "开始回测计算", "running")
            .await
            .ok();

        // 5. 执行回测或扫描
        let (perf, scan_results) = if payload.scope == "scan" {
            let top_n = payload
                .params
                .get("top_n")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            let score_threshold = Decimal::from_f64_retain(
                payload
                    .params
                    .get("score_threshold")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(60.0),
            )
            .unwrap_or(dec!(60));

            self.queue
                .publish_progress(&job_id_str, 0.5, "开始全市场扫描", "running")
                .await
                .ok();

            let results = scanner::scan_market(
                &self.db,
                &self.ts_db,
                top_n,
                score_threshold,
                &payload.start_date,
                &payload.end_date,
            )
            .await
            .map_err(|e| format!("Scan failed: {}", e))?;

            let suitable_count = results.len();
            let avg_return = if suitable_count > 0 {
                results.iter().map(|r| r.total_return).sum::<Decimal>() / Decimal::from(suitable_count as u64)
            } else {
                dec!(0)
            };

            let perf = Performance {
                total_return: avg_return,
                annual_return: dec!(0),
                max_drawdown: dec!(0),
                sharpe_ratio: dec!(0),
                sortino_ratio: dec!(0),
                calmar_ratio: dec!(0),
                win_rate: dec!(0),
                profit_ratio: dec!(0),
                total_trades: suitable_count as u64,
                total_commission: dec!(0),
                final_value: dec!(0),
                duration_days: 0,
                trading_days: 0,
                monthly_returns: Vec::new(),
            };

            (perf, Some(results))
        } else {
            let perf = match strategy_type {
                "buy_and_hold" => {
                    let mut strategy = BuyAndHold::new();
                    engine.run(&mut strategy, &all_bars)
                }
                "dual_ma" => {
                    let short = payload
                        .params
                        .get("short_window")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(5) as usize;
                    let long = payload
                        .params
                        .get("long_window")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(20) as usize;
                    let mut strategy = DualMA::new(short, long);
                    engine.run(&mut strategy, &all_bars)
                }
                _ => {
                    return Err(format!(
                        "Unsupported strategy type: {}. Supported: buy_and_hold, dual_ma",
                        strategy_type
                    ));
                }
            };
            (perf, None)
        };

        self.queue
            .publish_progress(&job_id_str, 0.8, "回测计算完成", "running")
            .await
            .ok();

        // 6. 保存报告
        let report_path = if let Some(results) = scan_results {
            let report = json!({
                "scope": "scan",
                "suitable_count": results.len(),
                "results": results,
            });
            save_json_report(&job_id_str, &report)
                .map_err(|e| format!("Failed to save scan report: {}", e))?
        } else {
            save_report(&job_id_str, &perf)
                .map_err(|e| format!("Failed to save report: {}", e))?
        };

        info!("Report saved: {}", report_path);

        // 7. 更新数据库
        let summary = json!({
            "total_return": perf.total_return.to_string(),
            "annual_return": perf.annual_return.to_string(),
            "max_drawdown": perf.max_drawdown.to_string(),
            "sharpe_ratio": perf.sharpe_ratio.to_string(),
            "final_value": perf.final_value.to_string(),
            "total_trades": perf.total_trades,
        });

        sqlx::query(
            "UPDATE backtest_jobs SET status = 'success', result_summary = $1, result_report_path = $2, completed_at = NOW() WHERE id = $3"
        )
        .bind(&summary)
        .bind(&report_path)
        .bind(payload.job_id)
        .execute(&self.db)
        .await
        .map_err(|e| format!("Failed to update job: {}", e))?;

        // 8. 写入结果缓存（仅限有 strategy_id 的任务）
        if let Ok(Some(job)) = sqlx::query_as::<_, BacktestJob>(
            "SELECT * FROM backtest_jobs WHERE id = $1"
        )
        .bind(payload.job_id)
        .fetch_optional(&self.db)
        .await
        {
            if let Some(strategy_id) = job.strategy_id {
                let cache_hash = make_cache_hash(
                    &strategy_id,
                    &payload.code,
                    &symbols,
                    &payload.start_date,
                    &payload.end_date,
                    &initial_cash,
                    &payload.params,
                    &payload.scope,
                );

                sqlx::query(
                    "INSERT INTO backtest_cache (cache_hash, strategy_id, scope, symbols, start_date, end_date, initial_cash, params, result_summary, result_report_path)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT (cache_hash) DO UPDATE SET
                         result_summary = EXCLUDED.result_summary,
                         result_report_path = EXCLUDED.result_report_path,
                         created_at = NOW(),
                         expires_at = NOW() + INTERVAL '7 days'"
                )
                .bind(&cache_hash)
                .bind(strategy_id)
                .bind(&payload.scope)
                .bind(&symbols)
                .bind(payload.start_date.parse::<chrono::NaiveDate>().ok())
                .bind(payload.end_date.parse::<chrono::NaiveDate>().ok())
                .bind(initial_cash)
                .bind(&payload.params)
                .bind(&summary)
                .bind(&report_path)
                .execute(&self.db)
                .await
                .ok();
            }
        }

        // 8. 推送完成
        self.queue
            .publish_progress(&job_id_str, 1.0, "回测完成", "success")
            .await
            .ok();

        info!("Backtest completed: {}", job_id_str);
        Ok(())
    }
}

/// 生成缓存哈希键
fn make_cache_hash(
    strategy_id: &uuid::Uuid,
    code: &str,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
    initial_cash: &Decimal,
    params: &serde_json::Value,
    scope: &str,
) -> String {
    let input = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        strategy_id,
        code,
        symbols.join(","),
        start_date,
        end_date,
        initial_cash,
        params.to_string(),
        scope
    );
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(hash)
}
