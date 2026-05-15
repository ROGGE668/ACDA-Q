//! Tushare Pro API 数据同步器
//!
//! 负责从 Tushare Pro API 拉取 A 股基础信息和历史行情，写入 TimescaleDB。
//! Rate limit: ~400 req/min (每次请求间隔 150ms)

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

const TUSHARE_API_URL: &str = "http://api.tushare.pro";
const REQUEST_INTERVAL_MS: u64 = 150;

/// Tushare API 客户端
pub struct TushareClient {
    token: String,
    client: reqwest::Client,
}

impl TushareClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            client: reqwest::Client::new(),
        }
    }

    /// 通用 API 调用
    async fn call(
        &self,
        api_name: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<TushareResponse> {
        let body = json!({
            "api_name": api_name,
            "token": self.token,
            "params": params,
            "fields": "",
        });

        let resp = self
            .client
            .post(TUSHARE_API_URL)
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(anyhow::anyhow!("Tushare HTTP {}: {}", status, text));
        }

        let parsed: TushareResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Parse error: {} | raw: {}", e, text))?;

        if parsed.code != 0 {
            return Err(anyhow::anyhow!(
                "Tushare API error: code={}, msg={:?}",
                parsed.code,
                parsed.msg
            ));
        }

        Ok(parsed)
    }

    // ========== 股票基础信息 ==========

    /// 同步全市场股票列表到 stock_basic 表
    pub async fn sync_stock_list(&self, db: &PgPool) -> anyhow::Result<usize> {
        info!("Syncing stock list from Tushare...");

        let resp = self
            .call("stock_basic", json!({"exchange": "", "list_status": "L"}))
            .await?;

        let data = resp.data.ok_or_else(|| anyhow::anyhow!("No data"))?;
        let idx = StockBasicIdx::new(&data.fields)?;
        let mut count = 0;

        for item in &data.items {
            let ts_code = item.get_str(idx.ts_code)?;
            let symbol = parse_ts_code(&ts_code);
            let name = item.get_str(idx.name)?;
            let exchange = item.get_opt_str(idx.exchange);
            let industry = item.get_opt_str(idx.industry);
            let list_date = item.get_opt_date(idx.list_date)?;
            let total_shares = item.get_opt_i64(idx.total_share);
            let float_shares = item.get_opt_i64(idx.float_share);
            let is_st = name.contains("ST") || name.contains("*ST");

            sqlx::query(
                "INSERT INTO stock_basic (symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, TRUE)
                 ON CONFLICT (symbol) DO UPDATE SET
                     name = EXCLUDED.name,
                     exchange = EXCLUDED.exchange,
                     industry = EXCLUDED.industry,
                     list_date = EXCLUDED.list_date,
                     total_shares = EXCLUDED.total_shares,
                     float_shares = EXCLUDED.float_shares,
                     is_st = EXCLUDED.is_st,
                     is_active = TRUE"
            )
            .bind(&symbol)
            .bind(&name)
            .bind(&exchange)
            .bind(&industry)
            .bind(list_date)
            .bind(total_shares)
            .bind(float_shares)
            .bind(is_st)
            .execute(db)
            .await?;

            count += 1;
        }

        info!("Synced {} stocks", count);
        Ok(count)
    }

    // ========== 日线数据 ==========

    /// 同步单只股票的日线数据
    pub async fn sync_daily_bars(
        &self,
        db: &PgPool,
        symbol: &str,
        start_date: &str,
        end_date: &str,
    ) -> anyhow::Result<usize> {
        let ts_code = to_ts_code(symbol);
        info!("Syncing daily bars for {} ({} ~ {})", symbol, start_date, end_date);

        let resp = self
            .call(
                "daily",
                json!({
                    "ts_code": ts_code,
                    "start_date": start_date,
                    "end_date": end_date,
                }),
            )
            .await?;

        let data = resp.data.ok_or_else(|| anyhow::anyhow!("No data"))?;
        if data.items.is_empty() {
            info!("No daily bars for {} in range", symbol);
            return Ok(0);
        }

        let idx = DailyBarIdx::new(&data.fields)?;

        // 删除重叠数据
        let start_dt = parse_date(start_date)?.and_hms_opt(0, 0, 0).unwrap();
        let end_dt = parse_date(end_date)?.and_hms_opt(23, 59, 59).unwrap();

        sqlx::query(
            "DELETE FROM daily_bars WHERE symbol = $1 AND datetime BETWEEN $2 AND $3"
        )
        .bind(symbol)
        .bind(start_dt)
        .bind(end_dt)
        .execute(db)
        .await?;

        let mut count = 0;
        for item in &data.items {
            let trade_date = item.get_str(idx.trade_date)?;
            let datetime = parse_datetime(&trade_date)?;
            let open = item.get_decimal(idx.open)?;
            let high = item.get_decimal(idx.high)?;
            let low = item.get_decimal(idx.low)?;
            let close = item.get_decimal(idx.close)?;
            let pre_close = item.get_opt_decimal(idx.pre_close);
            let change_pct = item.get_opt_decimal(idx.pct_chg);
            let vol = item.get_opt_i64(idx.vol).unwrap_or(0);
            let amount = item.get_opt_decimal(idx.amount);

            sqlx::query(
                "INSERT INTO daily_bars (symbol, datetime, open, high, low, close, volume, amount, pre_close, change_pct)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
            )
            .bind(symbol)
            .bind(datetime)
            .bind(open)
            .bind(high)
            .bind(low)
            .bind(close)
            .bind(vol)
            .bind(amount)
            .bind(pre_close)
            .bind(change_pct)
            .execute(db)
            .await?;

            count += 1;
        }

        info!("Synced {} daily bars for {}", count, symbol);
        sleep(Duration::from_millis(REQUEST_INTERVAL_MS)).await;
        Ok(count)
    }

    /// 批量同步多只股票日线数据
    pub async fn sync_daily_bars_batch(
        &self,
        db: &PgPool,
        symbols: &[String],
        start_date: &str,
        end_date: &str,
    ) -> anyhow::Result<usize> {
        let mut total = 0;
        for symbol in symbols {
            match self.sync_daily_bars(db, symbol, start_date, end_date).await {
                Ok(n) => total += n,
                Err(e) => warn!("Failed to sync {}: {}", symbol, e),
            }
        }
        info!("Batch sync: {} bars for {} symbols", total, symbols.len());
        Ok(total)
    }

    /// 按交易日批量同步（高效模式：一次请求获取全市场）
    pub async fn sync_daily_bars_by_trade_date(
        &self,
        db: &PgPool,
        trade_date: &str,
    ) -> anyhow::Result<usize> {
        info!("Syncing daily bars for trade date {}", trade_date);

        let resp = self.call("daily", json!({"trade_date": trade_date})).await?;
        let data = resp.data.ok_or_else(|| anyhow::anyhow!("No data"))?;
        if data.items.is_empty() {
            return Ok(0);
        }

        let idx = DailyBarIdx::new(&data.fields)?;
        let mut count = 0;

        for item in &data.items {
            let ts_code = item.get_str(idx.ts_code)?;
            let symbol = parse_ts_code(&ts_code);
            let trade_date_str = item.get_str(idx.trade_date)?;
            let datetime = parse_datetime(&trade_date_str)?;

            let open = item.get_decimal(idx.open)?;
            let high = item.get_decimal(idx.high)?;
            let low = item.get_decimal(idx.low)?;
            let close = item.get_decimal(idx.close)?;
            let pre_close = item.get_opt_decimal(idx.pre_close);
            let change_pct = item.get_opt_decimal(idx.pct_chg);
            let vol = item.get_opt_i64(idx.vol).unwrap_or(0);
            let amount = item.get_opt_decimal(idx.amount);

            sqlx::query(
                "INSERT INTO daily_bars (symbol, datetime, open, high, low, close, volume, amount, pre_close, change_pct)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (symbol, datetime) DO UPDATE SET
                     open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low,
                     close = EXCLUDED.close, volume = EXCLUDED.volume, amount = EXCLUDED.amount,
                     pre_close = EXCLUDED.pre_close, change_pct = EXCLUDED.change_pct"
            )
            .bind(&symbol)
            .bind(datetime)
            .bind(open)
            .bind(high)
            .bind(low)
            .bind(close)
            .bind(vol)
            .bind(amount)
            .bind(pre_close)
            .bind(change_pct)
            .execute(db)
            .await?;

            count += 1;
        }

        info!("Synced {} bars for {}", count, trade_date);
        Ok(count)
    }
}

// ========== 响应结构 ==========

#[derive(Debug, Deserialize)]
struct TushareResponse {
    code: i32,
    msg: Option<String>,
    data: Option<TushareData>,
}

#[derive(Debug, Deserialize)]
struct TushareData {
    fields: Vec<String>,
    #[serde(default)]
    items: Vec<Vec<serde_json::Value>>,
}

// ========== 字段索引 ==========

struct StockBasicIdx {
    ts_code: usize,
    name: usize,
    exchange: usize,
    industry: usize,
    list_date: usize,
    total_share: usize,
    float_share: usize,
}

impl StockBasicIdx {
    fn new(fields: &[String]) -> anyhow::Result<Self> {
        let f = |name: &str| {
            fields.iter().position(|f| f == name)
                .ok_or_else(|| anyhow::anyhow!("Missing field: {}", name))
        };
        Ok(Self {
            ts_code: f("ts_code")?,
            name: f("name")?,
            exchange: f("exchange")?,
            industry: f("industry")?,
            list_date: f("list_date")?,
            total_share: f("total_share")?,
            float_share: f("float_share")?,
        })
    }
}

struct DailyBarIdx {
    ts_code: usize,
    trade_date: usize,
    open: usize,
    high: usize,
    low: usize,
    close: usize,
    pre_close: usize,
    pct_chg: usize,
    vol: usize,
    amount: usize,
}

impl DailyBarIdx {
    fn new(fields: &[String]) -> anyhow::Result<Self> {
        let f = |name: &str| {
            fields.iter().position(|f| f == name)
                .ok_or_else(|| anyhow::anyhow!("Missing field: {}", name))
        };
        Ok(Self {
            ts_code: f("ts_code")?,
            trade_date: f("trade_date")?,
            open: f("open")?,
            high: f("high")?,
            low: f("low")?,
            close: f("close")?,
            pre_close: f("pre_close")?,
            pct_chg: f("pct_chg")?,
            vol: f("vol")?,
            amount: f("amount")?,
        })
    }
}

// ========== 工具函数 ==========

fn parse_ts_code(ts_code: &str) -> String {
    ts_code.split('.').next().unwrap_or(ts_code).to_string()
}

fn to_ts_code(symbol: &str) -> String {
    if symbol.ends_with(".SH") || symbol.ends_with(".SZ") || symbol.ends_with(".BJ") {
        symbol.to_string()
    } else if symbol.starts_with('6') {
        format!("{}.SH", symbol)
    } else {
        format!("{}.SZ", symbol)
    }
}

fn parse_date(s: &str) -> anyhow::Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y%m%d")
        .map_err(|e| anyhow::anyhow!("Invalid date {}: {}", s, e))
}

fn parse_datetime(s: &str) -> anyhow::Result<DateTime<Utc>> {
    let date = parse_date(s)?;
    let naive_dt = date.and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("Invalid time at {}", s))?;
    Ok(naive_dt.and_local_timezone(Utc).single()
        .ok_or_else(|| anyhow::anyhow!("Invalid timezone at {}", s))?)
}

// ========== 行解析 trait ==========

trait RowExt {
    fn get_str(&self, idx: usize) -> anyhow::Result<String>;
    fn get_opt_str(&self, idx: usize) -> Option<String>;
    fn get_opt_i64(&self, idx: usize) -> Option<i64>;
    fn get_decimal(&self, idx: usize) -> anyhow::Result<Decimal>;
    fn get_opt_decimal(&self, idx: usize) -> Option<Decimal>;
    fn get_opt_date(&self, idx: usize) -> anyhow::Result<Option<NaiveDate>>;
}

impl RowExt for Vec<serde_json::Value> {
    fn get_str(&self, idx: usize) -> anyhow::Result<String> {
        self.get(idx)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| self.get(idx).and_then(|v| v.as_f64()).map(|f| f.to_string()))
            .ok_or_else(|| anyhow::anyhow!("Not a string at {}", idx))
    }

    fn get_opt_str(&self, idx: usize) -> Option<String> {
        self.get(idx).and_then(|v| {
            v.as_str().filter(|s| !s.is_empty() && *s != "None").map(|s| s.to_string())
        })
    }

    fn get_opt_i64(&self, idx: usize) -> Option<i64> {
        self.get(idx).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
    }

    fn get_decimal(&self, idx: usize) -> anyhow::Result<Decimal> {
        let val = self.get(idx).ok_or_else(|| anyhow::anyhow!("Missing at {}", idx))?;
        if let Some(s) = val.as_str() {
            s.parse().map_err(|e| anyhow::anyhow!("Bad decimal at {}: {}", idx, e))
        } else if let Some(f) = val.as_f64() {
            Decimal::from_f64_retain(f)
                .ok_or_else(|| anyhow::anyhow!("Invalid decimal float at {}", idx))
        } else {
            Err(anyhow::anyhow!("Not a number at {}", idx))
        }
    }

    fn get_opt_decimal(&self, idx: usize) -> Option<Decimal> {
        self.get(idx).and_then(|v| {
            if let Some(s) = v.as_str() {
                if s.is_empty() { None } else { s.parse().ok() }
            } else {
                v.as_f64().and_then(Decimal::from_f64_retain)
            }
        })
    }

    fn get_opt_date(&self, idx: usize) -> anyhow::Result<Option<NaiveDate>> {
        match self.get_opt_str(idx) {
            Some(s) => parse_date(&s).map(Some),
            None => Ok(None),
        }
    }
}
