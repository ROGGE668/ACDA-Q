use std::sync::Arc;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::api::AppState;
use crate::data::{Market, Period, parse_symbol};
use crate::error::AppError;
use crate::models::{DailyBar, StockBasic};

#[derive(Debug, Deserialize)]
pub struct ListStocksQuery {
    pub market: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

pub async fn list_stocks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListStocksQuery>,
) -> Result<Json<Vec<StockBasic>>, AppError> {
    let _page = query.page.unwrap_or(1);
    let _page_size = query.page_size.unwrap_or(50);
    
    let stocks = Vec::new();
    Ok(Json(stocks))
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub adjust: Option<String>,
}

pub async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<DailyBar>>, AppError> {
    let _start = query.start_date.unwrap_or_else(|| "2024-01-01".to_string());
    let _end = query.end_date.unwrap_or_else(|| "2024-12-31".to_string());
    let _adjust = query.adjust.unwrap_or_else(|| "qfq".to_string());
    
    let bars = Vec::new();
    Ok(Json(bars))
}
