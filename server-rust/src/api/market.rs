use std::sync::Arc;
use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use sqlx::query_as;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{DailyBar, StockBasic};

#[derive(Deserialize)]
pub struct StockListParams {
    exchange: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
}

pub async fn list_stocks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<StockListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = params.limit.unwrap_or(50).min(5000);

    let stocks: Vec<StockBasic> = sqlx::query_as(
        "SELECT symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active
         FROM stock_basic
         WHERE ($1::text IS NULL OR exchange = $1)
           AND ($2::text IS NULL OR symbol ILIKE $2 OR name ILIKE $2)
         ORDER BY symbol
         LIMIT $3"
    )
    .bind(&params.exchange)
    .bind(params.search.as_ref().map(|s| format!("%{}%", s)))
    .bind(limit)
    .fetch_all(&state.ts_db)
    .await?;

    Ok(Json(serde_json::json!({
        "total": stocks.len(),
        "items": stocks,
    })))
}

pub async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let start_date = params.get("start_date").cloned().unwrap_or_else(|| "2023-01-01".to_string());
    let end_date = params.get("end_date").cloned().unwrap_or_else(|| "2023-12-31".to_string());

    let bars: Vec<DailyBar> = sqlx::query_as(
        "SELECT symbol, datetime, open, high, low, close, volume, amount, pre_close, change_pct
         FROM daily_bars
         WHERE symbol = $1 AND datetime BETWEEN $2 AND $3
         ORDER BY datetime"
    )
    .bind(&symbol)
    .bind(&start_date)
    .bind(&end_date)
    .fetch_all(&state.ts_db)
    .await?;

    Ok(Json(serde_json::json!({
        "symbol": symbol,
        "data": bars,
    })))
}
