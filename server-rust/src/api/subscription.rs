use std::sync::Arc;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;
use sqlx::query_as;
use uuid::Uuid;

use crate::api::AppState;
use crate::error::AppError;
use crate::middleware::auth::CurrentUser;
use crate::models::{PaymentOrder, Subscription, UserDevice};

#[derive(Deserialize)]
pub struct DeviceRegisterPayload {
    device_fingerprint: String,
    device_name: Option<String>,
    os_type: Option<String>,
}

#[derive(Deserialize)]
pub struct DeviceHeartbeatPayload {
    device_fingerprint: String,
}

#[derive(Deserialize)]
pub struct PaymentCreatePayload {
    channel: String,
    tier: String,
    duration_months: i32,
}

pub async fn get_subscription(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let sub: Option<Subscription> = sqlx::query_as(
        "SELECT * FROM subscriptions WHERE user_id = $1"
    )
    .bind(current_user.id)
    .fetch_optional(&state.db)
    .await?;

    let devices_active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_devices WHERE user_id = $1 AND is_active = true AND revoked_at IS NULL"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // 查询今日 AI 和回测使用量
    let ai_used_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_generations WHERE user_id = $1 AND created_at >= CURRENT_DATE"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let backtest_used_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM backtest_jobs WHERE user_id = $1 AND created_at >= CURRENT_DATE"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let sub = sub.unwrap_or_else(|| Subscription {
        id: Uuid::new_v4(),
        user_id: current_user.id,
        tier: "free".to_string(),
        status: "active".to_string(),
        expires_at: None,
        max_devices: 1,
        ai_quota_daily: 3,
        backtest_quota_daily: 5,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });

    Ok(Json(serde_json::json!({
        "tier": sub.tier,
        "status": sub.status,
        "expires_at": sub.expires_at,
        "max_devices": sub.max_devices,
        "ai_quota_daily": sub.ai_quota_daily,
        "backtest_quota_daily": sub.backtest_quota_daily,
        "devices_active": devices_active,
        "ai_used_today": ai_used_today,
        "backtest_used_today": backtest_used_today,
    })))
}

pub async fn register_device(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Json(payload): Json<DeviceRegisterPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 检查设备数量限制
    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_devices WHERE user_id = $1 AND is_active = true AND revoked_at IS NULL"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let max_devices: i32 = sqlx::query_scalar(
        "SELECT COALESCE(max_devices, 1) FROM subscriptions WHERE user_id = $1"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(1);

    if active_count >= max_devices as i64 {
        return Err(AppError::BadRequest(
            format!("Device limit reached: {}/{} devices", active_count, max_devices)
        ));
    }

    let device: UserDevice = sqlx::query_as(
        "INSERT INTO user_devices (user_id, device_fingerprint, device_name, os_type, last_heartbeat_at, is_active)
         VALUES ($1, $2, $3, $4, NOW(), true)
         ON CONFLICT (user_id, device_fingerprint) DO UPDATE SET
            is_active = true,
            last_heartbeat_at = NOW(),
            device_name = COALESCE(EXCLUDED.device_name, user_devices.device_name),
            os_type = COALESCE(EXCLUDED.os_type, user_devices.os_type)
         RETURNING *"
    )
    .bind(current_user.id)
    .bind(&payload.device_fingerprint)
    .bind(&payload.device_name)
    .bind(&payload.os_type)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(serde_json::json!({"status": "ok", "device_id": device.id})))
}

pub async fn device_heartbeat(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Json(payload): Json<DeviceHeartbeatPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query(
        "UPDATE user_devices SET last_heartbeat_at = NOW(), is_active = true
         WHERE user_id = $1 AND device_fingerprint = $2 AND revoked_at IS NULL"
    )
    .bind(current_user.id)
    .bind(&payload.device_fingerprint)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Device not registered or revoked".to_string()));
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn list_devices(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
) -> Result<Json<Vec<UserDevice>>, AppError> {
    let devices: Vec<UserDevice> = sqlx::query_as(
        "SELECT * FROM user_devices WHERE user_id = $1 ORDER BY created_at DESC"
    )
    .bind(current_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(devices))
}

pub async fn revoke_device(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let device_id = Uuid::parse_str(&id)
        .map_err(|_| AppError::BadRequest("Invalid device ID".to_string()))?;

    let result = sqlx::query(
        "UPDATE user_devices SET is_active = false, revoked_at = NOW() WHERE id = $1 AND user_id = $2"
    )
    .bind(device_id)
    .bind(current_user.id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Device not found".to_string()));
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn create_payment(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Json(payload): Json<PaymentCreatePayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let order_no = format!("ORD{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));

    let pricing: std::collections::HashMap<&str, i32> = [
        ("basic", 990),
        ("pro", 1990),
        ("max", 9900),
    ]
    .into_iter()
    .collect();

    let price_per_month = pricing.get(payload.tier.as_str()).copied()
        .ok_or_else(|| AppError::BadRequest("Invalid tier".to_string()))?;
    let amount_cents = price_per_month * payload.duration_months;

    let order: PaymentOrder = sqlx::query_as(
        "INSERT INTO payment_orders (user_id, order_no, channel, amount_cents, tier, duration_months, status)
         VALUES ($1, $2, $3, $4, $5, $6, 'pending') RETURNING *"
    )
    .bind(current_user.id)
    .bind(&order_no)
    .bind(&payload.channel)
    .bind(amount_cents)
    .bind(&payload.tier)
    .bind(payload.duration_months)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "order_no": order.order_no,
        "qr_code_url": format!("mock://pay/{}?order={}&amount={}", payload.channel, order_no, amount_cents),
        "amount_cents": amount_cents,
        "expires_at": (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
    })))
}

pub async fn get_payments(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
) -> Result<Json<Vec<PaymentOrder>>, AppError> {
    let orders: Vec<PaymentOrder> = sqlx::query_as(
        "SELECT * FROM payment_orders WHERE user_id = $1 ORDER BY created_at DESC"
    )
    .bind(current_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(orders))
}

pub async fn get_payment_status(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(order_no): Path<String>,
) -> Result<Json<PaymentOrder>, AppError> {
    let order: PaymentOrder = sqlx::query_as(
        "SELECT * FROM payment_orders WHERE order_no = $1 AND user_id = $2"
    )
    .bind(&order_no)
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(order))
}

pub async fn cancel_payment(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(order_no): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query(
        "UPDATE payment_orders SET status = 'cancelled' WHERE order_no = $1 AND user_id = $2 AND status = 'pending'"
    )
    .bind(&order_no)
    .bind(current_user.id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::BadRequest("Order not found or already processed".to_string()));
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}
