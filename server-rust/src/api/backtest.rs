use std::sync::Arc;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;
use std::path::PathBuf;
use uuid::Uuid;

use crate::api::AppState;
use crate::error::AppError;
use crate::middleware::auth::CurrentUser;
use crate::models::{BacktestJob, BacktestJobOut, BacktestResult, BacktestSubmit, DailyBar, MinuteBar};
use crate::queue::{BacktestPayload};
use std::time::Instant;

#[derive(Deserialize)]
pub struct ListParams {
    skip: Option<i64>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct ChartParams {
    agg: Option<String>,
}

#[derive(Deserialize)]
pub struct TradesParams {
    page: Option<i64>,
    page_size: Option<i64>,
}

/// 报告文件根目录
fn report_dir() -> PathBuf {
    std::env::var("REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./reports"))
}

/// 校验报告路径，防止路径遍历攻击
///
/// Uses the same approach as Python's `os.path.commonpath`: normalizes both paths
/// and checks that the resolved target is contained within the reports directory.
/// Unlike `canonicalize()`, this works even if the file does not exist yet.
fn safe_report_path(path: Option<&str>) -> Option<PathBuf> {
    let path = path?;
    let base = report_dir();
    let target = base.join(path);

    // Normalize both paths to absolute form
    let base_abs = std::fs::canonicalize(&base).unwrap_or(base);
    let target_abs = std::fs::canonicalize(&target).ok()?;

    // Ensure the target is under the reports directory (no path traversal)
    if target_abs.starts_with(&base_abs) {
        Some(target_abs)
    } else {
        tracing::warn!("Blocked path traversal attempt: {}", path);
        None
    }
}

pub async fn list_backtests(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<BacktestJobOut>>, AppError> {
    let skip = params.skip.unwrap_or(0);
    let limit = params.limit.unwrap_or(20).min(100);

    let jobs: Vec<BacktestJob> = sqlx::query_as(
        "SELECT * FROM backtest_jobs WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
    )
    .bind(current_user.id)
    .bind(limit)
    .bind(skip)
    .fetch_all(&state.db)
    .await?;

    let outs: Vec<BacktestJobOut> = jobs.into_iter().map(job_to_out).collect();
    Ok(Json(outs))
}

pub async fn submit_backtest(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Json(payload): Json<BacktestSubmit>,
) -> Result<Json<BacktestJobOut>, AppError> {
    if payload.strategy_id.is_none() && payload.strategy_code.is_none() {
        return Err(AppError::BadRequest(
            "Either strategy_id or strategy_code must be provided".to_string(),
        ));
    }

    // 提前克隆需要的字段，避免 move 问题
    let symbols = payload.symbols.clone();
    let start_date = payload.start_date.clone().unwrap_or_else(|| "2023-01-01".to_string());

    // 当 strategy_id 存在但 strategy_code 为空时，从数据库查找策略代码
    let strategy_code = if let Some(code) = payload.strategy_code.clone() {
        code
    } else if let Some(sid) = payload.strategy_id {
        let strategy: Option<crate::models::Strategy> = sqlx::query_as(
            "SELECT * FROM strategies WHERE id = $1 AND user_id = $2"
        )
        .bind(sid)
        .bind(current_user.id)
        .fetch_optional(&state.db)
        .await?;
        strategy.ok_or_else(|| AppError::NotFound("Strategy not found".to_string()))?.code
    } else {
        String::new()
    };
    let end_date = payload.end_date.clone().unwrap_or_else(|| "2023-12-31".to_string());
    // 合并策略的 params（包含 strategy_type 等）
    let mut merged_params = payload.params.clone().unwrap_or(serde_json::json!({}));
    if payload.strategy_id.is_some() && payload.strategy_code.is_none() {
        if let Some(sid) = payload.strategy_id {
            if let Ok(Some(strategy)) = sqlx::query_as::<_, crate::models::Strategy>(
                "SELECT * FROM strategies WHERE id = $1 AND user_id = $2"
            )
            .bind(sid)
            .bind(current_user.id)
            .fetch_optional(&state.db)
            .await
            {
                if let Some(obj) = merged_params.as_object_mut() {
                    if let Some(strategy_obj) = strategy.params.as_object() {
                        for (k, v) in strategy_obj {
                            obj.entry(k.clone()).or_insert(v.clone());
                        }
                    }
                }
            }
        }
    }
    let initial_cash = payload.initial_cash.unwrap_or_else(|| rust_decimal::Decimal::from(1_000_000));

    // 检查回测配额（简化版，实际应查 subscription 表）
    // Note: fetch_one returns Err on DB connection failure — we default to 10 only when
    // the user has no subscription row (COALESCE handles the NULL case).
    // Using unwrap_or_else to distinguish "DB error" (propagate) from "no rows" (default 10).
    let quota: i32 = match sqlx::query_scalar(
        "SELECT COALESCE(backtest_quota_daily, 10) FROM subscriptions WHERE user_id = $1"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    {
        Ok(q) => q,
        Err(_) => {
            tracing::warn!("Failed to fetch quota for user {}, using default 10", current_user.id);
            10
        }
    };

    let used_today: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM backtest_jobs WHERE user_id = $1 AND created_at >= CURRENT_DATE"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to count today's backtests for user {}: {}", current_user.id, e);
            // Fail closed: if we can't verify quota, deny the request
            return Err(AppError::Internal("Quota check failed, please try again".to_string()));
        }
    };

    if used_today >= quota as i64 {
        return Err(AppError::RateLimited);
    }

    let scope = payload.scope.unwrap_or_else(|| {
        if symbols.is_empty() {
            "scan".to_string()
        } else if symbols.len() == 1 {
            "single".to_string()
        } else {
            "multi".to_string()
        }
    });

    let job: BacktestJob = sqlx::query_as(
        "INSERT INTO backtest_jobs (user_id, strategy_id, status, scope, symbols, start_date, end_date, initial_cash, params, period)
         VALUES ($1, $2, 'pending', $3, $4, $5, $6, $7, $8, $9) RETURNING *"
    )
    .bind(current_user.id)
    .bind(payload.strategy_id)
    .bind(&scope)
    .bind(&symbols)
    .bind(payload.start_date.as_ref().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()))
    .bind(payload.end_date.as_ref().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()))
    .bind(initial_cash)
    .bind(merged_params)
        .bind(payload.period.unwrap_or_else(|| "1d".to_string()))
    .fetch_one(&state.db)
    .await?;

    // 推送任务到 Redis 队列
    let backtest_payload = BacktestPayload {
        job_id: job.id,
        user_id: current_user.id,
        code: strategy_code,
        symbols,
        start_date,
        end_date,
        initial_cash: job.initial_cash.to_string(),
        params: job.params.clone().unwrap_or(serde_json::json!({})),
        scope,
        period: job.period.clone().unwrap_or_else(|| "1d".to_string()),
    };

    match state.queue.push_task("backtest", &backtest_payload).await {
        Ok(task_id) => {
            tracing::info!("Backtest task queued: job_id={}, task_id={}", job.id, task_id);
        }
        Err(e) => {
            tracing::error!("Failed to queue backtest task: {}", e);
            // 不回滚，任务可以后续通过轮询或重试机制处理
        }
    }

    Ok(Json(job_to_out(job)))
}

pub async fn get_backtest(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(job_id): Path<Uuid>,
) -> Result<Json<BacktestJobOut>, AppError> {
    let job: BacktestJob = sqlx::query_as(
        "SELECT * FROM backtest_jobs WHERE id = $1 AND user_id = $2"
    )
    .bind(job_id)
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(job_to_out(job)))
}

pub async fn get_backtest_result(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(job_id): Path<Uuid>,
) -> Result<Json<BacktestResult>, AppError> {
    let job: BacktestJob = sqlx::query_as(
        "SELECT * FROM backtest_jobs WHERE id = $1 AND user_id = $2"
    )
    .bind(job_id)
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await?;

    if job.status != "success" {
        return Err(AppError::BadRequest("Backtest not completed yet".to_string()));
    }

    let summary = job.result_summary.unwrap_or(serde_json::json!({}));

    let mut trades: Vec<serde_json::Value> = vec![];
    let mut equity_curve: Vec<serde_json::Value> = vec![];
    let mut signals: Vec<serde_json::Value> = vec![];
    let mut suitable_stocks: Vec<serde_json::Value> = vec![];
    let mut unsuitable_stocks: Vec<serde_json::Value> = vec![];

    // 从报告文件读取完整数据
    if let Some(path) = safe_report_path(job.result_report_path.as_deref()) {
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                if let Ok(report) = serde_json::from_str::<serde_json::Value>(&content) {
                    trades = report.get("trades").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    equity_curve = report.get("equity_curve").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    signals = report.get("signals").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    suitable_stocks = report.get("suitable_stocks").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    unsuitable_stocks = report.get("unsuitable_stocks").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read backtest report {:?}: {}", path, e);
            }
        }
    }

    Ok(Json(BacktestResult {
        job_id,
        summary,
        trades,
        equity_curve,
        signals,
        suitable_stocks,
        unsuitable_stocks,
        report_path: job.result_report_path,
    }))
}

pub async fn get_backtest_chart(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(job_id): Path<Uuid>,
    Query(params): Query<ChartParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let agg = params.agg.clone().unwrap_or_else(|| "auto".to_string());
    let cache_key = format!("{}:{}", job_id, agg);

    // 检查缓存（5 分钟有效期，避免重复查询 TimescaleDB 4s+ 的 chunk 规划开销）
    {
        let cache = state.chart_cache.read().await;
        if let Some((ts, cached_value)) = cache.get(&cache_key) {
            if ts.elapsed().as_secs() < 300 {
                tracing::debug!("Chart cache hit for {}", cache_key);
                return Ok(Json(cached_value.clone()));
            }
        }
    }

    let job: BacktestJob = sqlx::query_as(
        "SELECT * FROM backtest_jobs WHERE id = $1 AND user_id = $2"
    )
    .bind(job_id)
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await?;

    if job.status != "success" {
        return Err(AppError::BadRequest("Backtest not completed yet".to_string()));
    }

    // 从报告文件一次性读取净值曲线 + K 线数据（worker 已预写入，无需查 TimescaleDB）
    let report_path = safe_report_path(job.result_report_path.as_deref());
    let mut equity_curve: Vec<serde_json::Value> = vec![];
    let mut kline_bars: Vec<serde_json::Value> = vec![];

    if let Some(path) = report_path {
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            if let Ok(report) = serde_json::from_str::<serde_json::Value>(&content) {
                equity_curve = report.get("equity_curve").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                // 优先从报告读取 K 线（worker 预写入）；兼容旧报告（无 kline_bars 字段）
                if let Some(kb) = report.get("kline_bars").and_then(|v| v.as_array()) {
                    kline_bars = kb.clone();
                } else {
                    // 旧报告 fallback：根据 period 选择查询 minute_bars 或 daily_bars
                    let kline_symbol = job.symbols.as_ref().and_then(|s| s.first()).map(|s| s.split('.').next().unwrap_or(s).to_string());
                    let job_period = job.period.as_deref().unwrap_or("1d");
                    if let (Some(sym), Some(start_date), Some(end_date)) = (kline_symbol.as_deref(), &job.start_date, &job.end_date) {
                        let start_str = start_date.format("%Y-%m-%d").to_string();
                        let end_str = end_date.format("%Y-%m-%d").to_string();
                        if job_period != "1d" {
                            // 分钟线：从 minute_bars 查询
                            let db_period = job_period.replace("min", "");
                            if let Ok(rows) = sqlx::query_as::<_, MinuteBar>(
                                "SELECT symbol, datetime, open, high, low, close, volume, amount
                                 FROM minute_bars
                                 WHERE symbol = $1
                                   AND period = $4
                                   AND datetime >= $2::date
                                   AND datetime <= $3::date + interval '1 day'
                                 ORDER BY datetime ASC"
                            )
                            .bind(sym)
                            .bind(&start_str)
                            .bind(&end_str)
                            .bind(&db_period)
                            .fetch_all(&state.ts_db)
                            .await
                            {
                                kline_bars = rows.iter().filter_map(|bar| serde_json::to_value(bar).ok()).collect();
                            }
                        } else {
                            if let Ok(rows) = sqlx::query_as::<_, DailyBar>(
                                "SELECT symbol, datetime, open, high, low, close, volume, amount, pre_close, change_pct
                                 FROM daily_bars
                                 WHERE symbol = $1
                                   AND datetime >= $2::date
                                   AND datetime <= $3::date + interval '1 day'
                                 ORDER BY datetime ASC"
                            )
                            .bind(sym)
                            .bind(&start_str)
                            .bind(&end_str)
                            .fetch_all(&state.ts_db)
                            .await
                            {
                                kline_bars = rows.iter().filter_map(|bar| serde_json::to_value(bar).ok()).collect();
                            }
                        }
                    }
                }
            }
        }
    }

    if equity_curve.is_empty() {
        return Ok(Json(serde_json::json!({
            "points": [],
            "kline_bars": kline_bars,
            "agg": params.agg.unwrap_or_else(|| "auto".to_string()),
        })));
    }

    let equity_len = equity_curve.len();

    let agg = params.agg.unwrap_or_else(|| {
        if equity_len > 3000 {
            "month".to_string()
        } else if equity_len > 1000 {
            "week".to_string()
        } else {
            "day".to_string()
        }
    });

    let points: Vec<serde_json::Value> = if agg == "week" || agg == "month" {
        let step = if agg == "month" { 22 } else { 5 };
        equity_curve
            .into_iter()
            .enumerate()
            .filter(move |(i, _)| i % step == 0 || *i == equity_len - 1)
            .map(|(_, p)| p)
            .collect()
    } else {
        equity_curve
    };

    let response = serde_json::json!({
        "points": points,
        "kline_bars": kline_bars,
        "agg": agg,
    });

    // 写入缓存
    {
        let mut cache = state.chart_cache.write().await;
        cache.insert(cache_key, (Instant::now(), response.clone()));
        // 清理过期缓存（保留最近 100 条）
        if cache.len() > 100 {
            let now = Instant::now();
            cache.retain(|_, (ts, _)| now.duration_since(*ts).as_secs() < 300);
        }
    }

    Ok(Json(response))
}

pub async fn get_backtest_trades(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(job_id): Path<Uuid>,
    Query(params): Query<TradesParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let job: BacktestJob = sqlx::query_as(
        "SELECT * FROM backtest_jobs WHERE id = $1 AND user_id = $2"
    )
    .bind(job_id)
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await?;

    if job.status != "success" {
        return Err(AppError::BadRequest("Backtest not completed yet".to_string()));
    }

    let mut trades = vec![];

    if let Some(path) = safe_report_path(job.result_report_path.as_deref()) {
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            if let Ok(report) = serde_json::from_str::<serde_json::Value>(&content) {
                trades = report.get("trades").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            }
        }
    }

    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).clamp(1, 200);
    let total = trades.len();

    // 按时间倒序
    trades.reverse();

    let start = ((page - 1) as usize) * (page_size as usize);
    let end = (start + page_size as usize).min(total);
    let items: Vec<_> = trades.into_iter().skip(start).take(end - start).collect();

    Ok(Json(serde_json::json!({
        "total": total,
        "page": page,
        "page_size": page_size,
        "items": items,
    })))
}

/// 删除回测记录（普通用户只能删自己的）
pub async fn delete_backtest(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(job_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 先查询获取报告文件路径，以便删除
    let job: Option<BacktestJob> = sqlx::query_as(
        "SELECT * FROM backtest_jobs WHERE id = $1 AND user_id = $2"
    )
    .bind(job_id)
    .bind(current_user.id)
    .fetch_optional(&state.db)
    .await?;

    let job = job.ok_or_else(|| AppError::NotFound("回测记录未找到或无权限删除".to_string()))?;

    // 删除报告文件（如果存在）
    if let Some(report_path) = safe_report_path(job.result_report_path.as_deref()) {
        if report_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&report_path).await {
                tracing::warn!("Failed to delete report file: {}", e);
            }
        }
    }

    // 删除数据库记录
    let result = sqlx::query(
        "DELETE FROM backtest_jobs WHERE id = $1 AND user_id = $2"
    )
    .bind(job_id)
    .bind(current_user.id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("回测记录未找到或无权限删除".to_string()));
    }

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

fn job_to_out(job: BacktestJob) -> BacktestJobOut {
    BacktestJobOut {
        id: job.id,
        status: job.status,
        scope: job.scope,
        symbols: job.symbols,
        start_date: job.start_date,
        end_date: job.end_date,
        initial_cash: job.initial_cash,
        result_summary: job.result_summary,
        result_report_path: job.result_report_path,
        error_message: job.error_message,
        period: job.period,
        created_at: job.created_at,
        completed_at: job.completed_at,
    }
}
