use std::sync::Arc;
use axum::extract::{Json, Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::api::AppState;
use crate::data::{Market, Period, parse_symbol};
use crate::error::AppError;
use crate::models::{DailyBar, StockBasic};

/// 前端期望的包装格式: { items: [...] }
#[derive(Serialize)]
pub struct StockSearchResponse {
    pub items: Vec<StockBasic>,
}

#[derive(Debug, Deserialize)]
pub struct ListStocksQuery {
    pub market: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// 将前端 exchange 参数映射为数据库中的 exchange 值
fn map_exchange(market: &str) -> Vec<String> {
    match market {
        "cn" => vec!["主板", "创业板", "科创板", "其他"],
        "hk" => vec!["港股"],
        "us" => vec!["美股"],
        other => vec![other],
    }.into_iter().map(String::from).collect()
}

/// 去除 symbol 后缀（如 000001.SZ → 000001）
/// 数据库中 symbol 存储为纯代码，不带交易所后缀
fn strip_symbol_suffix(symbol: &str) -> &str {
    if let Some(dot_pos) = symbol.rfind('.') {
        &symbol[..dot_pos]
    } else {
        symbol
    }
}

pub async fn list_stocks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListStocksQuery>,
) -> Result<Json<StockSearchResponse>, AppError> {
    let page = query.page.unwrap_or(1) as i64;
    let page_size = query.page_size.unwrap_or(50) as i64;
    let offset = (page - 1) * page_size;

    let stocks = if let Some(ref q) = query.search {
        let pattern = format!("%{}%", q);
        sqlx::query_as::<_, StockBasic>(
            "SELECT symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active
             FROM stock_basic
             WHERE is_active = TRUE
               AND (symbol ILIKE $1 OR name ILIKE $1)
             ORDER BY symbol
             LIMIT $2 OFFSET $3"
        )
        .bind(&pattern)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.ts_db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    } else if let Some(ref m) = query.market {
        let exchanges = map_exchange(m);
        sqlx::query_as::<_, StockBasic>(
            "SELECT symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active
             FROM stock_basic
             WHERE is_active = TRUE AND exchange = ANY($1)
             ORDER BY symbol
             LIMIT $2 OFFSET $3"
        )
        .bind(&exchanges)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.ts_db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    } else {
        sqlx::query_as::<_, StockBasic>(
            "SELECT symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active
             FROM stock_basic
             WHERE is_active = TRUE
             ORDER BY symbol
             LIMIT $1 OFFSET $2"
        )
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.ts_db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    };

    Ok(Json(StockSearchResponse { items: stocks }))
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub adjust: Option<String>,
    pub exchange: Option<String>,
    pub period: Option<String>,
    pub limit: Option<i64>,
}

pub async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<DailyBar>>, AppError> {
    let start = query.start_date.unwrap_or_else(|| "2000-01-01".to_string());
    let end = query.end_date.unwrap_or_else(|| "2099-12-31".to_string());
    let raw_period = query.period.as_deref().unwrap_or("1d");
    let limit = query.limit.unwrap_or(5000);

    // 去除 symbol 后缀以匹配数据库中的纯代码格式
    let clean_symbol = strip_symbol_suffix(&symbol);

    // 分钟线: period = "1" / "5" / "15" / "30" / "60"
    let minute_periods = ["1", "5", "15", "30", "60"];
    let is_minute = minute_periods.contains(&raw_period);

    let bars: Vec<DailyBar> = if is_minute {
        // 查询 minute_bars 表，按 period 字段过滤
        sqlx::query_as::<_, DailyBar>(
            "SELECT symbol, datetime, open, high, low, close, volume, amount,
                    NULL::numeric AS pre_close, NULL::numeric AS change_pct
             FROM minute_bars
             WHERE symbol = $1
               AND period = $2
               AND datetime >= $3::date
               AND datetime <= $4::date + interval '1 day'
             ORDER BY datetime ASC
             LIMIT $5"
        )
        .bind(clean_symbol)
        .bind(raw_period)
        .bind(&start)
        .bind(&end)
        .bind(limit)
        .fetch_all(&state.ts_db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    } else {
        // Determine table for daily/weekly/monthly
        let table = match raw_period {
            "1w" => "weekly_bars",
            "1m" => "monthly_bars",
            _ => "daily_bars",
        };

        let query_str = format!(
            "SELECT symbol, datetime, open, high, low, close, volume, amount, pre_close, change_pct
             FROM {}
             WHERE symbol = $1
               AND datetime >= $2::date
               AND datetime <= $3::date + interval '1 day'
             ORDER BY datetime ASC
             LIMIT $4",
            table
        );
        sqlx::query_as::<_, DailyBar>(&query_str)
            .bind(clean_symbol)
            .bind(&start)
            .bind(&end)
            .bind(limit)
            .fetch_all(&state.ts_db)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
    };

    Ok(Json(bars))
}
