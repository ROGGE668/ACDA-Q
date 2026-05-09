//! 数据供给层 — 从 TimescaleDB 加载历史 K 线数据

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::FromRow;

use crate::backtest::types::Bar;
use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, FromRow)]
struct DailyBarRow {
    symbol: String,
    datetime: chrono::DateTime<chrono::Utc>,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: i64,
    pre_close: Option<Decimal>,
}

/// 从 TimescaleDB 加载指定标的的日线数据
pub async fn load_daily_bars(
    pool: &DbPool,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
) -> Result<Vec<Bar>, AppError> {
    let rows: Vec<DailyBarRow> = sqlx::query_as(
        "SELECT symbol, datetime, open, high, low, close, volume, pre_close
         FROM daily_bars
         WHERE symbol = ANY($1) AND datetime BETWEEN $2 AND $3
         ORDER BY datetime, symbol"
    )
    .bind(symbols)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await?;

    let bars = rows
        .into_iter()
        .map(|r| Bar {
            symbol: r.symbol,
            timestamp: r.datetime.naive_utc(),
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume as u64,
            pre_close: r.pre_close.unwrap_or(r.close),
            is_st: false, // TODO: join with stock_basic to get is_st
        })
        .collect();

    Ok(bars)
}

/// 加载单标的日线数据（同步版本，用于 Worker 内部）
pub async fn load_symbol_bars(
    pool: &DbPool,
    symbol: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<Bar>, AppError> {
    load_daily_bars(pool, &[symbol.to_string()], start_date, end_date).await
}

/// 批量加载多标的数据，按 symbol 分组
pub async fn load_bars_grouped(
    pool: &DbPool,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
) -> Result<std::collections::HashMap<String, Vec<Bar>>, AppError> {
    let bars = load_daily_bars(pool, symbols, start_date, end_date).await?;
    let mut grouped: std::collections::HashMap<String, Vec<Bar>> = std::collections::HashMap::new();
    for bar in bars {
        grouped.entry(bar.symbol.clone()).or_default().push(bar);
    }
    Ok(grouped)
}
