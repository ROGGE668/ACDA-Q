//! 数据库模型

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ========== Auth ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

// ========== User ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub nickname: Option<String>,
    pub is_admin: bool,
    pub tier: String,
    pub quota_ai_daily: i32,
    pub quota_backtest_daily: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRegister {
    pub email: String,
    pub password: String,
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLogin {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserOut {
    pub id: Uuid,
    pub email: String,
    pub nickname: Option<String>,
    pub tier: String,
    pub is_admin: bool,
    pub quota_ai_daily: i32,
    pub ai_used_today: i32,
    pub created_at: DateTime<Utc>,
}

// ========== Strategy ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Strategy {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub strategy_type: Option<String>,
    pub code: String,
    pub params: serde_json::Value,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyCreate {
    pub name: String,
    pub description: Option<String>,
    pub strategy_type: Option<String>,
    pub code: Option<String>,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub code: Option<String>,
    pub params: Option<serde_json::Value>,
}

// ========== Backtest Job ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct BacktestJob {
    pub id: Uuid,
    pub user_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub status: String, // pending/running/success/failed
    pub scope: Option<String>, // single/multi/scan
    pub symbols: Option<Vec<String>>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub initial_cash: Decimal,
    pub params: Option<serde_json::Value>,
    pub result_summary: Option<serde_json::Value>,
    pub result_report_path: Option<String>,
    pub error_message: Option<String>,
    pub period: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestSubmit {
    pub strategy_id: Option<Uuid>,
    pub strategy_code: Option<String>,
    pub symbols: Vec<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub initial_cash: Option<Decimal>,
    pub scope: Option<String>,
    pub period: Option<String>,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJobOut {
    pub id: Uuid,
    pub status: String,
    pub scope: Option<String>,
    pub symbols: Option<Vec<String>>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub initial_cash: Decimal,
    pub result_summary: Option<serde_json::Value>,
    pub result_report_path: Option<String>,
    pub error_message: Option<String>,
    pub period: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub job_id: Uuid,
    pub summary: serde_json::Value,
    pub trades: Vec<serde_json::Value>,
    pub equity_curve: Vec<serde_json::Value>,
    pub signals: Vec<serde_json::Value>,
    pub suitable_stocks: Vec<serde_json::Value>,
    pub unsuitable_stocks: Vec<serde_json::Value>,
    pub report_path: Option<String>,
}

// ========== Refresh Token ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_jti: String,
    pub family_id: String,
    pub revoked: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// ========== Subscription ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Uuid,
    pub user_id: Uuid,
    pub tier: String,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_devices: i32,
    pub ai_quota_daily: i32,
    pub backtest_quota_daily: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ========== AI Generation ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AIGeneration {
    pub id: Uuid,
    pub user_id: Uuid,
    pub prompt: String,
    pub generated_code: Option<String>,
    pub model: Option<String>,
    pub tokens_used: Option<i32>,
    pub status: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ========== Payment ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PaymentOrder {
    pub id: Uuid,
    pub user_id: Uuid,
    pub order_no: String,
    pub channel: String,
    pub amount_cents: i32,
    pub tier: String,
    pub duration_months: i32,
    pub status: String,
    pub paid_at: Option<DateTime<Utc>>,
    pub channel_transaction_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ========== Device ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct UserDevice {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_fingerprint: String,
    pub device_name: Option<String>,
    pub os_type: Option<String>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ========== Market Data ==========

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StockBasic {
    pub symbol: String,
    pub name: String,
    pub exchange: Option<String>,
    pub industry: Option<String>,
    pub list_date: Option<chrono::NaiveDate>,
    pub total_shares: Option<i64>,
    pub float_shares: Option<i64>,
    pub is_st: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DailyBar {
    pub symbol: String,
    pub datetime: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: i64,
    pub amount: Option<Decimal>,
    pub pre_close: Option<Decimal>,
    pub change_pct: Option<Decimal>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct MinuteBar {
    pub symbol: String,
    pub datetime: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: i64,
    pub amount: Option<Decimal>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct AdjFactor {
    pub symbol: String,
    pub trade_date: chrono::NaiveDate,
    pub adj_factor: Decimal,
}


#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct BacktestCache {
    pub cache_hash: String,
    pub strategy_id: Uuid,
    pub scope: Option<String>,
    pub symbols: Option<Vec<String>>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub initial_cash: Option<Decimal>,
    pub params: Option<serde_json::Value>,
    pub result_summary: Option<serde_json::Value>,
    pub result_report_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
