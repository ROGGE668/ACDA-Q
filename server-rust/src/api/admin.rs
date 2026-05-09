use std::sync::Arc;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;
use sqlx::query_as;
use uuid::Uuid;

use crate::api::AppState;
use crate::data::tushare::TushareClient;
use crate::error::AppError;
use crate::models::{BacktestJob, PaymentOrder, Subscription, User, UserDevice};

#[derive(Deserialize)]
pub struct ListParams {
    skip: Option<i64>,
    limit: Option<i64>,
}

pub async fn get_dashboard_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE created_at >= CURRENT_DATE"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let active_devices: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_devices WHERE is_active = true AND revoked_at IS NULL"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let backtests_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM backtest_jobs WHERE created_at >= CURRENT_DATE"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let backtests_success: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM backtest_jobs WHERE created_at >= CURRENT_DATE AND status = 'success'"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let success_rate = if backtests_today > 0 {
        (backtests_success as f64 / backtests_today as f64 * 100.0).round()
    } else {
        0.0
    };

    let total_revenue: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_cents), 0) FROM payment_orders WHERE status = 'paid'"
    )
    .fetch_one(&state.db)
    .await?;

    let revenue_this_month: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_cents), 0) FROM payment_orders WHERE status = 'paid' AND created_at >= DATE_TRUNC('month', CURRENT_DATE)"
    )
    .fetch_one(&state.db)
    .await?;

    let ai_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_generations WHERE created_at >= CURRENT_DATE"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "users": { "total": total_users, "new_today": new_today },
        "devices": { "active": active_devices },
        "backtests": { "today": backtests_today, "success_rate": success_rate },
        "subscriptions": {},
        "revenue": { "total_cny": (total_revenue.unwrap_or(0) as f64) / 100.0, "this_month_cny": (revenue_this_month.unwrap_or(0) as f64) / 100.0 },
        "ai_generations": { "today": ai_today },
    })))
}

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let skip = params.skip.unwrap_or(0);
    let limit = params.limit.unwrap_or(20);

    let users: Vec<User> = sqlx::query_as(
        "SELECT * FROM users ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(limit)
    .bind(skip)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": users,
    })))
}

pub async fn toggle_admin(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user: User = sqlx::query_as(
        "UPDATE users SET is_admin = NOT is_admin WHERE id = $1 RETURNING *"
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "id": user.id,
        "is_admin": user.is_admin,
    })))
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

pub async fn list_all_devices(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let skip = params.skip.unwrap_or(0);
    let limit = params.limit.unwrap_or(20);

    let devices: Vec<UserDevice> = sqlx::query_as(
        "SELECT * FROM user_devices ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(limit)
    .bind(skip)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_devices")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": devices,
    })))
}

pub async fn revoke_device(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    sqlx::query(
        "UPDATE user_devices SET is_active = false, revoked_at = NOW() WHERE id = $1"
    )
    .bind(id)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({"status": "revoked"})))
}

pub async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let skip = params.skip.unwrap_or(0);
    let limit = params.limit.unwrap_or(20);

    let subs: Vec<Subscription> = sqlx::query_as(
        "SELECT * FROM subscriptions ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(limit)
    .bind(skip)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscriptions")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": subs,
    })))
}

pub async fn update_subscription(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tier = payload.get("tier").and_then(|v| v.as_str());
    let status = payload.get("status").and_then(|v| v.as_str());

    sqlx::query(
        "UPDATE subscriptions SET tier = COALESCE($2, tier), status = COALESCE($3, status), updated_at = NOW() WHERE id = $1"
    )
    .bind(id)
    .bind(tier)
    .bind(status)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({"status": "updated", "id": id})))
}

pub async fn list_payments(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let skip = params.skip.unwrap_or(0);
    let limit = params.limit.unwrap_or(20);

    let orders: Vec<PaymentOrder> = sqlx::query_as(
        "SELECT * FROM payment_orders ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(limit)
    .bind(skip)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payment_orders")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": orders,
    })))
}

pub async fn update_payment_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let status = payload.get("status")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("Missing status".to_string()))?;

    let paid_at = if status == "paid" {
        Some(chrono::Utc::now())
    } else {
        None
    };

    let order: PaymentOrder = sqlx::query_as(
        "UPDATE payment_orders SET status = $2, paid_at = $3 WHERE id = $1 RETURNING *"
    )
    .bind(id)
    .bind(status)
    .bind(paid_at)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "status": "updated",
        "order_status": order.status,
    })))
}

pub async fn list_all_backtests(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let skip = params.skip.unwrap_or(0);
    let limit = params.limit.unwrap_or(20);

    let jobs: Vec<BacktestJob> = sqlx::query_as(
        "SELECT * FROM backtest_jobs ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(limit)
    .bind(skip)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backtest_jobs")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": jobs,
    })))
}

pub async fn delete_backtest_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    sqlx::query("DELETE FROM backtest_jobs WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

// ========== 数据同步端点 ==========

#[derive(Deserialize)]
pub struct SyncDailyBarsRequest {
    pub symbols: Vec<String>,
    pub start_date: String,
    pub end_date: String,
}

pub async fn sync_stock_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = &state.settings.tushare_token;
    if token.is_empty() {
        return Err(AppError::BadRequest("Tushare token not configured".to_string()));
    }

    let client = TushareClient::new(token);
    let count = client.sync_stock_list(&state.db).await
        .map_err(|e| AppError::Internal(format!("Sync failed: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "synced": count,
        "type": "stock_list",
    })))
}

pub async fn sync_daily_bars(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SyncDailyBarsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = &state.settings.tushare_token;
    if token.is_empty() {
        return Err(AppError::BadRequest("Tushare token not configured".to_string()));
    }

    let client = TushareClient::new(token);
    let count = client.sync_daily_bars_batch(&state.db, &req.symbols, &req.start_date, &req.end_date).await
        .map_err(|e| AppError::Internal(format!("Sync failed: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "synced": count,
        "type": "daily_bars",
        "symbols": req.symbols.len(),
    })))
}
