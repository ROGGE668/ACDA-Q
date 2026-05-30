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
use crate::backtest::types::{Bar, Performance, AccountSnapshot};
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
    let safe_name = format!("{}.json", job_id);
    let path = dir.join(&safe_name);
    std::fs::write(&path, serde_json::to_string_pretty(data)?)?;
    Ok(safe_name)
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
    Ok(safe_name)
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

        info!("Processing backtest job: {} scope={} symbols={}", job_id_str, payload.scope, payload.symbols.len());

        // 1. 更新状态为 running
        self.queue
            .update_task_status(&task.id, TaskStatus::Running, None)
            .await
            .map_err(|e| format!("Failed to update status: {}", e))?;
        self.queue
            .publish_progress(&job_id_str, 0.1, "开始执行回测", "running")
            .await
            .ok();

        // 用闭包捕获 ? 错误，失败时更新 DB 状态避免卡 pending
        let job_id = payload.job_id;
        let process_result: Result<(), String> = async {
        let initial_cash = payload
            .initial_cash
            .parse::<Decimal>()
            .map_err(|e| format!("Invalid initial_cash: {}", e))?;

        let (strategy_type, strategy_params) = parse_strategy_params(&payload.params);

        let symbols = payload.symbols.clone();
        if symbols.is_empty() && payload.scope != "scan" {
            return Err("No symbols provided".to_string());
        }

        // 3. 加载历史数据：仅内置策略需要，沙箱策略自行加载
        let period = &payload.period;
        let is_sandbox = !(is_builtin_strategy(&strategy_type) && payload.code.trim().is_empty());

        let mut all_bars = if is_sandbox {
            // 沙箱策略自行从 DB 加载数据，跳过此处加载节省 ~1.5s
            info!("Sandbox strategy detected, skipping data preload");
            Vec::new()
        } else if period == "1d" {
            match datafeed::load_daily_bars(
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
            }
        } else {
            // 分钟线：从 minute_bars 表加载
            let db_period = period.replace("min", "");
            match datafeed::load_minute_bars(
                &self.ts_db,
                &symbols,
                &payload.start_date,
                &payload.end_date,
                &db_period,
            )
            .await
            {
                Ok(bars) if !bars.is_empty() => {
                    info!("Loaded {} minute bars (period={})", bars.len(), db_period);
                    bars
                }
                Ok(_) => {
                    warn!("No minute data for {:?}, falling back to daily bars", symbols);
                    match datafeed::load_daily_bars(
                        &self.ts_db,
                        &symbols,
                        &payload.start_date,
                        &payload.end_date,
                    )
                    .await
                    {
                        Ok(bars) if !bars.is_empty() => bars,
                        _ => generate_mock_bars_for_symbols(&symbols),
                    }
                }
                Err(e) => {
                    warn!("Failed to load minute bars ({}), falling back to daily", e);
                    match datafeed::load_daily_bars(
                        &self.ts_db,
                        &symbols,
                        &payload.start_date,
                        &payload.end_date,
                    )
                    .await
                    {
                        Ok(bars) if !bars.is_empty() => bars,
                        _ => generate_mock_bars_for_symbols(&symbols),
                    }
                }
            }
        };

        self.queue
            .publish_progress(&job_id_str, 0.3, "数据加载完成", "running")
            .await
            .ok();

        // 4. 判断执行路径：内置策略 vs 自定义策略（沙箱）
        let mut scan_trades: Option<Vec<serde_json::Value>> = None;
        let mut equity_snapshots: Vec<AccountSnapshot> = vec![];
        let mut sandbox_kline: Vec<serde_json::Value> = vec![];
        let mut scan_summary: Option<serde_json::Value> = None;
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

            let exchange = payload.params.get("exchange").and_then(|v| v.as_str()).unwrap_or("cn");
            let results = scanner::scan_market(
                &self.db,
                &self.ts_db,
                &payload.code,
                &symbols,
                exchange,
                top_n,
                score_threshold,
                &payload.start_date,
                &payload.end_date,
                initial_cash.to_string().parse().unwrap_or(1_000_000.0),
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

            let n = results.len().max(1) as u64;
            scan_summary = Some(json!({
                "scanned_count": symbols.len(),
                "suitable_count": results.len(),
                "avg_return": results.iter().map(|r| r.total_return).sum::<Decimal>() / Decimal::from(n),
                "avg_annual_return": results.iter().map(|r| r.annual_return).sum::<Decimal>() / Decimal::from(n),
                "avg_sharpe": results.iter().map(|r| r.sharpe_ratio).sum::<Decimal>() / Decimal::from(n),
                "avg_drawdown": results.iter().map(|r| r.max_drawdown).sum::<Decimal>() / Decimal::from(n),
                "total_trades": results.iter().map(|r| r.total_trades).sum::<u64>(),
            }));
            (perf, Some(results))
        } else if is_builtin_strategy(&strategy_type) && payload.code.trim().is_empty() {
            // 内置策略：在当前进程执行（Rust 原生，仅当无自定义代码时）
            let (perf, snaps) = self.run_builtin_strategy(
                &strategy_type,
                &strategy_params,
                &all_bars,
                initial_cash,
            )?;
            equity_snapshots = snaps;
            (perf, None)
        } else {
            // 自定义策略：通过沙箱执行 Python 代码
            let (perf, sandbox_trades, sandbox_snaps, sandbox_kline_in) = self.run_sandbox_strategy(
                &payload.code,
                &symbols,
                &payload.start_date,
                &payload.end_date,
                initial_cash,
                &strategy_params,
                &job_id_str,
                &payload.period,
            )
            .await?;
            scan_trades = sandbox_trades;
            equity_snapshots = sandbox_snaps;
            sandbox_kline = sandbox_kline_in;
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
                "suitable_stocks": results,
            });
            save_json_report(&job_id_str, &report)
                .map_err(|e| format!("Failed to save scan report: {}", e))?
        } else {
            save_report(&job_id_str, &perf)
                .map_err(|e| format!("Failed to save report: {}", e))?
        };

        info!("Report saved: {}", report_path);

        // 6. 追加交易记录到报告文件（仅沙箱执行路径有）
        if let Some(ref trades) = scan_trades {
            if !trades.is_empty() {
                let report_full_path = report_dir().join(&report_path);
                if let Ok(content) = tokio::fs::read_to_string(&report_full_path).await {
                    if let Ok(mut report) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(obj) = report.as_object_mut() {
                            obj.insert("trades".to_string(), serde_json::Value::Array(trades.clone()));
                            let _ = std::fs::write(&report_full_path, serde_json::to_string_pretty(&report).unwrap_or_default());
                        }
                    }
                }
            }
        }

        // 7. 追加净值曲线到报告文件
        tracing::info!("Equity snapshots count: {}", equity_snapshots.len());
        if !equity_snapshots.is_empty() {
            let report_full_path = report_dir().join(&report_path);
            if let Ok(content) = tokio::fs::read_to_string(&report_full_path).await {
                if let Ok(mut report) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(obj) = report.as_object_mut() {
                        // 分钟级数据保留完整时间戳，日线级按日期去重
                        let job_period = if payload.period.is_empty() { "1d" } else { &payload.period };
                        let is_minute_job = job_period != "1d";
                        if is_minute_job {
                            // 分钟级：保留每条快照的完整时间戳
                            let equity_data: Vec<serde_json::Value> = equity_snapshots.iter().map(|s| {
                                let dt = s.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
                                serde_json::json!({
                                    "datetime": dt,
                                    "total_value": s.total_value.to_string(),
                                    "cash": s.cash.to_string(),
                                    "position_value": s.position_value.to_string(),
                                })
                            }).collect();
                            obj.insert("equity_curve".to_string(), serde_json::Value::Array(equity_data));
                        } else {
                            // 日线级：按日期去重，同一天保留最后一条
                            let mut seen: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
                            for s in &equity_snapshots {
                                let dt = s.timestamp.format("%Y-%m-%d").to_string();
                                seen.insert(dt.clone(), serde_json::json!({
                                    "datetime": dt,
                                    "total_value": s.total_value.to_string(),
                                    "cash": s.cash.to_string(),
                                    "position_value": s.position_value.to_string(),
                                }));
                            }
                            let mut equity_data: Vec<serde_json::Value> = seen.into_iter().map(|(_, v)| v).collect();
                            equity_data.sort_by(|a, b| {
                                a.get("datetime").and_then(|v| v.as_str()).unwrap_or("")
                                    .cmp(b.get("datetime").and_then(|v| v.as_str()).unwrap_or(""))
                            });
                            obj.insert("equity_curve".to_string(), serde_json::Value::Array(equity_data));
                        }
                        let _ = std::fs::write(&report_full_path, serde_json::to_string_pretty(&report).unwrap_or_default());
                    }
                }
            }
        }

        // 7b. 追加 K 线数据到报告文件
        {
            // 对于内置策略，从 all_bars 生成 kline_bars
            if sandbox_kline.is_empty() && !all_bars.is_empty() {
                let kline: Vec<serde_json::Value> = all_bars.iter().map(|bar| {
                    serde_json::json!({
                        "symbol": bar.symbol,
                        "datetime": bar.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                        "open": bar.open.to_string(),
                        "high": bar.high.to_string(),
                        "low": bar.low.to_string(),
                        "close": bar.close.to_string(),
                        "volume": bar.volume,
                        "amount": serde_json::Value::Null,
                    })
                }).collect();
                let report_full_path = report_dir().join(&report_path);
                if let Ok(content) = tokio::fs::read_to_string(&report_full_path).await {
                    if let Ok(mut report) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(obj) = report.as_object_mut() {
                            obj.insert("kline_bars".to_string(), serde_json::Value::Array(kline));
                            let _ = std::fs::write(&report_full_path, serde_json::to_string_pretty(&report).unwrap_or_default());
                            info!("K-line bars from all_bars saved to report");
                        }
                    }
                }
            } else if !sandbox_kline.is_empty() {
                let report_full_path = report_dir().join(&report_path);
                if let Ok(content) = tokio::fs::read_to_string(&report_full_path).await {
                    if let Ok(mut report) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(obj) = report.as_object_mut() {
                            obj.insert("kline_bars".to_string(), serde_json::Value::Array(sandbox_kline.clone()));
                            let _ = std::fs::write(&report_full_path, serde_json::to_string_pretty(&report).unwrap_or_default());
                            info!("K-line bars ({} rows) saved to report", sandbox_kline.len());
                        }
                    }
                }
            }
        }

        // 8. 更新数据库
        // Decimal → f64 后再序列化，确保前端能直接调用 .toFixed()
        use rust_decimal::prelude::ToPrimitive;
        fn d2f(d: &Decimal) -> f64 { ToPrimitive::to_f64(d).unwrap_or(0.0) }
        let base_summary = json!({
            "total_return": d2f(&perf.total_return),
            "annual_return": d2f(&perf.annual_return),
            "max_drawdown": d2f(&perf.max_drawdown),
            "sharpe_ratio": d2f(&perf.sharpe_ratio),
            "sortino_ratio": d2f(&perf.sortino_ratio),
            "calmar_ratio": d2f(&perf.calmar_ratio),
            "win_rate": d2f(&perf.win_rate),
            "profit_ratio": d2f(&perf.profit_ratio),
            "total_commission": d2f(&perf.total_commission),
            "final_value": d2f(&perf.final_value),
            "total_trades": perf.total_trades,
            "duration_days": perf.duration_days,
            "trading_days": perf.trading_days,
        });
        let summary = if let Some(ref ss) = scan_summary {
            let mut merged = base_summary.as_object().unwrap().clone();
            if let Some(ss_obj) = ss.as_object() {
                for (k, v) in ss_obj { merged.insert(k.clone(), v.clone()); }
            }
            serde_json::Value::Object(merged)
        } else {
            base_summary
        };

        sqlx::query(
            "UPDATE backtest_jobs SET status = 'success', result_summary = $1, result_report_path = $2, period = $3, completed_at = NOW() WHERE id = $4"
        )
        .bind(&summary)
        .bind(&report_path)
        .bind(&payload.period)
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
        }.await;  // 结束 async 闭包

        // 处理闭包结果：失败时写 DB 状态，避免卡 pending
        match process_result {
            Ok(()) => Ok(()),
            Err(e) => {
                sqlx::query(
                    "UPDATE backtest_jobs SET status = 'failed', error_message = $1, completed_at = NOW() WHERE id = $2"
                )
                .bind(&e)
                .bind(job_id)
                .execute(&self.db)
                .await
                .ok();
                error!("Backtest failed: {} - {}", job_id_str, e);
                Err(e)
            }
        }
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
    ) -> Result<(Performance, Vec<AccountSnapshot>), String> {
        let mut engine = Engine::new(initial_cash).with_options(
            self.settings.backtest_commission,
            self.settings.backtest_slippage,
            self.settings.backtest_stamp_duty,
            self.settings.backtest_transfer_fee,
        );

        match strategy_type {
            "buy_and_hold" => {
                let mut strategy = BuyAndHold::new();
                let perf = engine.run(&mut strategy, bars);
                Ok((perf, engine.broker.equity_curve))
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
                let perf = engine.run(&mut strategy, bars);
                Ok((perf, engine.broker.equity_curve))
            }
            _ => Err(format!(
                "Unsupported builtin strategy: {}. Supported: buy_and_hold, dual_ma",
                strategy_type
            )),
        }
    }

    /// 通过沙箱执行自定义策略（独立子进程）
    /// 返回 (Performance, Option<trades>)
    async fn run_sandbox_strategy(
        &self,
        code: &str,
        symbols: &[String],
        start_date: &str,
        end_date: &str,
        initial_cash: Decimal,
        params: &serde_json::Value,
        job_id: &str,
        period: &str,
    ) -> Result<(Performance, Option<Vec<serde_json::Value>>, Vec<AccountSnapshot>, Vec<serde_json::Value>), String> {
        info!("Running custom strategy in sandbox for job: {}", job_id);
// 尝试从缓存读取结果
        let cache_hash = make_cache_hash(&CacheKey {
            strategy_id: &uuid::Uuid::nil(), // 缓存读取时不验证 strategy_id
            code,
            symbols,
            start_date,
            end_date,
            initial_cash: &initial_cash,
            params,
            scope: "",
        });

        if let Ok(Some(cached)) = sqlx::query_as::<_, (serde_json::Value, String)>(
            "SELECT result_summary, result_report_path FROM backtest_cache WHERE cache_hash = $1 AND expires_at > NOW()"
        )
        .bind(&cache_hash)
        .fetch_optional(&self.db)
        .await
        {
            info!("Cache hit for job: {}", job_id);
            let cached_summary = cached.0;
            return Ok((
                Performance {
                    total_return: cached_summary.get("total_return").and_then(|v| v.as_f64()).map(|v| rust_decimal::Decimal::from_f64_retain(v).unwrap_or(dec!(0))).unwrap_or(dec!(0)),
                    annual_return: cached_summary.get("annual_return").and_then(|v| v.as_f64()).map(|v| rust_decimal::Decimal::from_f64_retain(v).unwrap_or(dec!(0))).unwrap_or(dec!(0)),
                    max_drawdown: cached_summary.get("max_drawdown").and_then(|v| v.as_f64()).map(|v| rust_decimal::Decimal::from_f64_retain(v).unwrap_or(dec!(0))).unwrap_or(dec!(0)),
                    sharpe_ratio: cached_summary.get("sharpe_ratio").and_then(|v| v.as_f64()).map(|v| rust_decimal::Decimal::from_f64_retain(v).unwrap_or(dec!(0))).unwrap_or(dec!(0)),
                    sortino_ratio: dec!(0),
                    calmar_ratio: dec!(0),
                    win_rate: cached_summary.get("win_rate").and_then(|v| v.as_f64()).map(|v| rust_decimal::Decimal::from_f64_retain(v).unwrap_or(dec!(0))).unwrap_or(dec!(0)),
                    profit_ratio: dec!(0),
                    total_trades: cached_summary.get("total_trades").and_then(|v| v.as_u64()).unwrap_or(0),
                    total_commission: dec!(0),
                    final_value: cached_summary.get("final_value").and_then(|v| v.as_f64()).map(|v| rust_decimal::Decimal::from_f64_retain(v).unwrap_or(dec!(0))).unwrap_or(dec!(0)),
                    duration_days: 0,
                    trading_days: 0,
                    monthly_returns: Vec::new(),
                },
                None,
                vec![],
                vec![],
            ));
        }

        // 构建沙箱配置
        let sandbox_config = SandboxConfig {
            timeout_secs: 300,
            memory_limit_bytes: 512 * 1024 * 1024, // 512MB
            cpu_limit_secs: 60,
        };

        // Rust 端预加载行情数据（比 Python + SQLAlchemy 快 5-10 倍）
        let bars_json = {
            let clean_symbols: Vec<String> = symbols.iter().map(|s| s.split('.').next().unwrap_or(s).to_string()).collect();
            if period == "1d" || period.is_empty() {
                match datafeed::load_daily_bars(&self.ts_db, &clean_symbols, start_date, end_date).await {
                    Ok(bars) => {
                        let rows: Vec<serde_json::Value> = bars.iter().map(|b| {
                            serde_json::json!({
                                "symbol": &b.symbol,
                                "datetime": b.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                                "open": b.open,
                                "high": b.high,
                                "low": b.low,
                                "close": b.close,
                                "volume": b.volume as i64,
                                "amount": 0.0,
                            })
                        }).collect();
                        info!("Loaded {} bars in Rust for job: {}", rows.len(), job_id);
                        Some(serde_json::Value::Array(rows))
                    }
                    Err(e) => {
                        warn!("Rust bar loading failed, falling back to Python: {}", e);
                        None
                    }
                }
            } else {
                // 分钟级数据
                let minute_period = period.replace("min", "");
                match datafeed::load_minute_bars(&self.ts_db, &clean_symbols, start_date, end_date, &minute_period).await {
                    Ok(bars) => {
                        let mut rows: Vec<serde_json::Value> = bars.iter().map(|b| {
                            serde_json::json!({
                                "symbol": &b.symbol,
                                "datetime": b.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                                "open": b.open,
                                "high": b.high,
                                "low": b.low,
                                "close": b.close,
                                "volume": b.volume as i64,
                                "amount": 0.0,
                            })
                        }).collect();
                        // 分钟数据量大时采样
                        if rows.len() > 5000 {
                            let step = rows.len() / 5000;
                            rows = rows.into_iter().enumerate().filter_map(|(i, r)| if i % step == 0 { Some(r) } else { None }).collect();
                        }
                        info!("Loaded {} minute bars in Rust for job: {}", rows.len(), job_id);
                        Some(serde_json::Value::Array(rows))
                    }
                    Err(e) => {
                        warn!("Rust minute bar loading failed, falling back to Python: {}", e);
                        None
                    }
                }
            }
        };

        // 通过沙箱执行策略（数据已由 Rust 预加载）
        let result = run_backtest_sandbox(
            code,
            symbols,
            start_date,
            end_date,
            initial_cash.to_string().parse().unwrap_or(100000.0),
            &sandbox_config,
            period,
            bars_json.as_ref(),
        ).map_err(|e| {
            error!("Sandbox execution failed: {}", e);
            format!("Sandbox execution failed: {}", e)
        })?;

        // 解析沙箱返回结果
        let (perf, sandbox_result_data) = match result.status.as_str() {
            "success" => {
                let result_data = result.result.ok_or("No result data from sandbox")?;
                let p = self.parse_sandbox_result(&result_data)?;
                (p, Some(result_data))
            }
            "security_error" => {
                return Err(format!("Security error: {}", result.error.unwrap_or_else(|| "Unknown error".to_string())))
            }
            _ => {
                return Err(format!(
                    "Strategy execution failed: {}",
                    result.error.unwrap_or_else(|| "Unknown error".to_string())
                ))
            }
        };

        // 提取交易记录
        let trades = sandbox_result_data
            .as_ref()
            .and_then(|data| data.get("trades").and_then(|v| v.as_array()).cloned());

        // 提取沙箱返回的净值曲线
        let mut sandbox_equity: Vec<AccountSnapshot> = vec![];
        tracing::info!("Sandbox result has equity_curve: {}", sandbox_result_data.as_ref().map(|d| d.get("equity_curve").map(|v| v.as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0)).unwrap_or(0));
        if let Some(ref data) = sandbox_result_data {
            if let Some(ec) = data.get("equity_curve").and_then(|v| v.as_array()) {
                for point in ec {
                    if let (Some(dt), Some(tv)) = (
                        point.get("datetime").and_then(|v| v.as_str()),
                        point.get("total_value").and_then(|v| v.as_str()),
                    ) {
                        let ts = chrono::NaiveDateTime::parse_from_str(dt, "%Y-%m-%d %H:%M:%S")
                            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&dt.replace("+00:00", ""), "%Y-%m-%d %H:%M:%S"))
                            .or_else(|_| chrono::NaiveDateTime::parse_from_str(dt, "%Y-%m-%dT%H:%M:%S"))
                            .or_else(|_| chrono::NaiveDate::parse_from_str(dt, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap()))
                            .ok();
                        if let Some(ts) = ts {
                            if let Ok(val) = tv.parse::<Decimal>() {
                                sandbox_equity.push(AccountSnapshot {
                                    timestamp: ts,
                                    total_value: val,
                                    cash: point.get("cash").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(dec!(0)),
                                    position_value: point.get("position_value").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(dec!(0)),
                                });
                            }
                        }
                    }
                }
            }
        }

        // 提取 K 线数据（sandbox 已从 DB 加载，直接返回避免重复查询）
        let mut kline_bars: Vec<serde_json::Value> = vec![];
        if let Some(ref data) = sandbox_result_data {
            if let Some(kb) = data.get("kline_bars").and_then(|v| v.as_array()) {
                kline_bars = kb.clone();
            }
        }

        Ok((perf, trades, sandbox_equity, kline_bars))
    }

    /// 解析沙箱返回的 JSON 结果转换为 Performance
    fn parse_sandbox_result(&self, data: &serde_json::Value) -> Result<Performance, String> {
        // 辅助：从 JSON 中提取 f64，支持数字和字符串两种类型
        fn get_f64(v: &serde_json::Value, key: &str) -> Option<f64> {
            v.get(key).and_then(|val| {
                val.as_f64()
                    .or_else(|| val.as_str().and_then(|s| s.parse::<f64>().ok()))
            })
        }

        let total_return = get_f64(data, "total_return")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let annual_return = get_f64(data, "annual_return")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let max_drawdown = get_f64(data, "max_drawdown")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let sharpe_ratio = get_f64(data, "sharpe_ratio")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let sortino_ratio = get_f64(data, "sortino_ratio")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let calmar_ratio = get_f64(data, "calmar_ratio")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let win_rate = get_f64(data, "win_rate")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let profit_ratio = get_f64(data, "profit_ratio")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let total_trades = data
            .get("total_trades")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let total_commission = get_f64(data, "total_commission")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
            .unwrap_or(dec!(0));

        let final_value = get_f64(data, "final_value")
            .and_then(|v| rust_decimal::Decimal::from_f64_retain(v))
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
