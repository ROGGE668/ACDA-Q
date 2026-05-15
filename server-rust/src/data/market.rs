//! 市场数据模块
//!
//! 支持 A股、港股、美股的数据查询和 Symbol 解析

use serde::{Deserialize, Serialize};
use std::fmt;

/// 市场类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Market {
    /// A股 (SH/SZ/BJ)
    AShare,
    /// 港股 (HKEX)
    HK,
    /// 美股 (NYSE/NASDAQ)
    US,
}

impl Default for Market {
    fn default() -> Self {
        Market::AShare
    }
}

impl fmt::Display for Market {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Market::AShare => write!(f, "A"),
            Market::HK => write!(f, "HK"),
            Market::US => write!(f, "US"),
        }
    }
}

/// 频率周期
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    /// 分钟线 (1/2/3/5/10/30/60 分钟)
    Minute(u32),
    /// 日线 (1/2/3/5/10/30/60 日)
    Daily(u32),
}

impl Default for Period {
    fn default() -> Self {
        Period::Daily(1)
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Period::Minute(n) => write!(f, "{}m", n),
            Period::Daily(n) => write!(f, "{}d", n),
        }
    }
}

impl Period {
    /// 解析 period 字符串，如 "1m", "5m", "1d", "60d"
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_lowercase();
        if s.ends_with('m') {
            let n: u32 = s[..s.len() - 1].parse().ok()?;
            if [1, 2, 3, 5, 10, 30, 60].contains(&n) {
                Some(Period::Minute(n))
            } else {
                None
            }
        } else if s.ends_with('d') {
            let n: u32 = s[..s.len() - 1].parse().ok()?;
            if [1, 2, 3, 5, 10, 30, 60].contains(&n) {
                Some(Period::Daily(n))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 返回对应的数据库表名
    pub fn table_name(&self) -> &'static str {
        match self {
            Period::Minute(_) => "minute_bars",
            Period::Daily(_) => "daily_bars",
        }
    }
}

/// 交易所代码
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exchange {
    /// 上海交易所
    SH,
    /// 深圳交易所
    SZ,
    /// 北京交易所
    BJ,
    /// 香港交易所
    HK,
    /// 纽约交易所
    #[allow(non_upper_case_globals)]
    Nyse,
    /// 纳斯达克
    #[allow(non_upper_case_globals)]
    Nasdaq,
}

impl Exchange {
    /// 从字符串解析交易所代码
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "SH" | "SSE" => Some(Exchange::SH),
            "SZ" | "SZSE" => Some(Exchange::SZ),
            "BJ" | "BSE" => Some(Exchange::BJ),
            "HK" | "HKEX" => Some(Exchange::HK),
            "NYSE" | "nyse" => Some(Exchange::Nyse),
            "NASDAQ" | "nasdaq" | "US" => Some(Exchange::Nasdaq),
            _ => None,
        }
    }

    /// 返回交易所代码的小写字符串
    pub fn code(&self) -> &'static str {
        match self {
            Exchange::SH => "SH",
            Exchange::SZ => "SZ",
            Exchange::BJ => "BJ",
            Exchange::HK => "HK",
            Exchange::Nyse => "Nyse",
            Exchange::Nasdaq => "Nasdaq",
        }
    }
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// 解析 Symbol，返回 (交易所, 股票代码)
/// 格式: 000001.SZ, 600000.SH, 00700.HK, AAPL.US
pub fn parse_symbol(symbol: &str) -> Option<(Exchange, String)> {
    let parts: Vec<&str> = symbol.rsplitn(2, '.').collect();
    if parts.len() != 2 {
        return None;
    }
    let code = parts[1].to_uppercase();
    let suffix = parts[0].to_uppercase();
    let exchange = Exchange::parse(&suffix)?;
    Some((exchange, code))
}

/// 构建 Symbol 字符串
pub fn build_symbol(exchange: &Exchange, code: &str) -> String {
    format!("{}.{}", code, exchange.code())
}

/// 获取市场类型
pub fn get_market(exchange: &Exchange) -> Market {
    match exchange {
        Exchange::SH | Exchange::SZ | Exchange::BJ => Market::AShare,
        Exchange::HK => Market::HK,
        Exchange::Nyse | Exchange::Nasdaq => Market::US,
    }
}

/// 检查是否支持该周期
pub fn is_valid_period(period: &Period) -> bool {
    match period {
        Period::Minute(n) => [1, 2, 3, 5, 10, 30, 60].contains(n),
        Period::Daily(n) => [1, 2, 3, 5, 10, 30, 60].contains(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_symbol() {
        assert_eq!(parse_symbol("000001.SZ"), Some((Exchange::SZ, "000001".to_string())));
        assert_eq!(parse_symbol("600000.SH"), Some((Exchange::SH, "600000".to_string())));
        assert_eq!(parse_symbol("00700.HK"), Some((Exchange::HK, "00700".to_string())));
        assert_eq!(parse_symbol("AAPL.US"), Some((Exchange::Nasdaq, "AAPL".to_string())));
    }

    #[test]
    fn test_period_parse() {
        assert_eq!(Period::parse("1m"), Some(Period::Minute(1)));
        assert_eq!(Period::parse("5m"), Some(Period::Minute(5)));
        assert_eq!(Period::parse("60m"), Some(Period::Minute(60)));
        assert_eq!(Period::parse("1d"), Some(Period::Daily(1)));
        assert_eq!(Period::parse("60d"), Some(Period::Daily(60)));
        assert_eq!(Period::parse("invalid"), None);
    }

    #[test]
    fn test_get_market() {
        assert_eq!(get_market(&Exchange::SZ), Market::AShare);
        assert_eq!(get_market(&Exchange::SH), Market::AShare);
        assert_eq!(get_market(&Exchange::BJ), Market::AShare);
        assert_eq!(get_market(&Exchange::HK), Market::HK);
        assert_eq!(get_market(&Exchange::Nyse), Market::US);
        assert_eq!(get_market(&Exchange::Nasdaq), Market::US);
    }

    #[test]
    fn test_period_table_name() {
        assert_eq!(Period::Minute(1).table_name(), "minute_bars");
        assert_eq!(Period::Minute(5).table_name(), "minute_bars");
        assert_eq!(Period::Minute(60).table_name(), "minute_bars");
        assert_eq!(Period::Daily(1).table_name(), "daily_bars");
        assert_eq!(Period::Daily(60).table_name(), "daily_bars");
    }
}