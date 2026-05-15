//! 回测任务处理器 — Worker 核心逻辑
//!
//! 消费 Redis Streams 任务，执行内置策略回测，保存结果。
//! Phase 1 支持内置策略（BuyAndHold / DualMA），自定义策略通过沙箱执行。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn, error};
use crate::backtest::context::Context;
use crate::backtest::datafeed;
use crate::backtest::engine::{Engine, Strategy};
use crate::backtest::scanner;
use crate::backtest::types::{Bar, Performance};
use crate::config::Settings;
use crate::models::BacktestJob;
use crate::queue::{BacktestPayload, Queue, Task, TaskStatus, Worker};
use crate::sandbox::{run_backtest_sandbox, SandboxConfig};
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

/// 检查是否为内置策略类型
fn is_builtin_strategy(strategy_type: &str) -> bool {
    matches!(strategy_type, "buy_and_hold" | "dual_ma")
}

/// 从 payload 解析策略参数
fn parse_strategy_params(params: &serde_json::Value) -> (String, serde_json::Value) {
    let strategy_type = params
        .get("strategy_type")
        .and_then(|v| v.as_str())
        .unwrap_or("buy_and_hold")
        .to_string();
    
    let strategy_params = params.clone();
    (strategy_type, strategy_params)
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

        let (strategy_type, strategy_params) = parse_strategy_params(&payload.params);

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
        all_bars.sort_by_key(|x| x.timestamp);

        if all_bars.is_empty() {
            return Err("No bars loaded".to_string());
        }

        self.queue
            .publish_progress(&job_id_str, 0.3, "数据加载完成", "running")
            .await
            .ok();

        // 4. 判断执行路径：内置策略 vs 自定义策略（沙箱）
        let (perf, scan_results) = if payload.scope == "scan" {
            // Scan 模式：全市场扫描
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
        } else if is_builtin_strategy(&strategy_type) {
            // 内置策略：在当前进程执行（Rust 原生）
            let perf = self.run_builtin_strategy(
                &strategy_type,
                &strategy_params,
                &all_bars,
                initial_cash,
            )?;
            (perf, None)
        } else {
            // 自定义策略：通过沙箱执行 Python 代码
            let perf = self.run_sandbox_strategy(
                &payload.code,
                &symbols,
                &payload.start_date,
                &payload.end_date,
                initial_cash,
                &strategy_params,
                &job_id_str,
            )
            .await?;
            (perf, None)
        };

        self.queue
            .publish_progress(&job_id_str, 0.8, "回测计算完成", "running")
            .await
            .ok();

        // 5. 保存报告
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

        // 6. 更新数据库
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

        // 7. 写入结果缓存（仅限有 strategy_id 的任务）
        if let Ok(Some(job)) = sqlx::query_as::<_, BacktestJob>(
            "SELECT * FROM backtest_jobs WHERE id = $1"
        )
        .bind(payload.job_id)
        .fetch_optional(&self.db)
        .await
        {
            if let Some(strategy_id) = job.strategy_id {
                let cache_key = CacheKey {
                    strategy_id: &strategy_id,
                    code: &payload.code,
                    symbols: &symbols,
                    start_date: &payload.start_date,
                    end_date: &payload.end_date,
                    initial_cash: &initial_cash,
                    params: &payload.params,
                    scope: &payload.scope,
                };
                let cache_hash = make_cache_hash(&cache_key);

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

impl BacktestWorker {
    /// 执行内置策略（在当前进程）
    fn run_builtin_strategy(
        &self,
        strategy_type: &str,
        params: &serde_json::Value,
        bars: &[Bar],
        initial_cash: Decimal,
    ) -> Result<Performance, String> {
        let mut engine = Engine::new(initial_cash).with_options(
            self.settings.backtest_commission,
            self.settings.backtest_slippage,
            self.settings.backtest_stamp_duty,
            self.settings.backtest_transfer_fee,
        );

        match strategy_type {
            "buy_and_hold" => {
                let mut strategy = BuyAndHold::new();
                Ok(engine.run(&mut strategy, bars))
            }
            "dual_ma" => {
                let short = params
                    .get("short_window")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;
                let long = params
                    .get("long_window")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as usize;
                let mut strategy = DualMA::new(short, long);
                Ok(engine.run(&mut strategy, bars))
            }
            _ => Err(format!(
                "Unsupported builtin strategy: {}. Supported: buy_and_hold, dual_ma",
                strategy_type
            )),
        }
    }

    /// 通过沙箱执行自定义策略（独立子进程）
    async fn run_sandbox_strategy(
        &self,
        code: &str,
        symbols: &[String],
        start_date: &str,
        end_date: &str,
        initial_cash: Decimal,
        params: &serde_json::Value,
        job_id: &str,
    ) -> Result<Performance, String> {
        info!("Running custom strategy in sandbox for job: {}", job_id);

        // 构建沙箱配置
        let sandbox_config = SandboxConfig {
            timeout_secs: 300,
            memory_limit_bytes: 512 * 1024 * 1024, // 512MB
            cpu_limit_secs: 60,
        };

        // 通过沙箱执行策略
        let result = run_backtest_sandbox(
            code,
            symbols,
            start_date,
            end_date,
            initial_cash.to_string().parse().unwrap_or(100000.0),
            &sandbox_config,
        ).map_err(|e| {
            error!("Sandbox execution failed: {}", e);
            format!("Sandbox execution failed: {}", e)
        })?;

        // 解析沙箱返回结果
        match result.status.as_str() {
            "success" => {
                let result_data = result.result.ok_or("No result data from sandbox")?;
                self.parse_sandbox_result(&result_data)
            }
            "security_error" => {
                Err(format!("Security error: {}", result.error.unwrap_or_else(|| "Unknown error".to_string())))
            }
            _ => {
                Err(format!(
                    "Strategy execution failed: {}",
                    result.error.unwrap_or_else(|| "Unknown error".to_string())
                ))
            }
        }
    }

    /// 解析沙箱返回的 JSON 结果转换为 Performance
    fn parse_sandbox_result(&self, data: &serde_json::Value) -> Result<Performance, String> {
        // 从 JSON 解析性能指标
        let total_return = data
            .get("total_return")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let annual_return = data
            .get("annual_return")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let max_drawdown = data
            .get("max_drawdown")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let sharpe_ratio = data
            .get("sharpe_ratio")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let sortino_ratio = data
            .get("sortino_ratio")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let calmar_ratio = data
            .get("calmar_ratio")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let win_rate = data
            .get("win_rate")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let profit_ratio = data
            .get("profit_ratio")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let total_trades = data
            .get("total_trades")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let total_commission = data
            .get("total_commission")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let final_value = data
            .get("final_value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(Decimal::from_f64_retain)
            .flatten()
            .unwrap_or(dec!(0));

        let duration_days = data
            .get("duration_days")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let trading_days = data
            .get("trading_days")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        Ok(Performance {
            total_return,
            annual_return,
            max_drawdown,
            sharpe_ratio,
            sortino_ratio,
            calmar_ratio,
            win_rate,
            profit_ratio,
            total_trades,
            total_commission,
            final_value,
            duration_days,
            trading_days,
            monthly_returns: Vec::new(),
        })
    }
}

/// 生成缓存哈希键
struct CacheKey<'a> {
    strategy_id: &'a uuid::Uuid,
    code: &'a str,
    symbols: &'a [String],
    start_date: &'a str,
    end_date: &'a str,
    initial_cash: &'a Decimal,
    params: &'a serde_json::Value,
    scope: &'a str,
}

fn make_cache_hash(key: &CacheKey) -> String {
    let input = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        key.strategy_id,
        key.code,
        key.symbols.join(","),
        key.start_date,
        key.end_date,
        key.initial_cash,
        key.params.to_string(),
        key.scope
    );
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(hash)
}
