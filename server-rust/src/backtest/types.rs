//! 回测引擎核心类型定义

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 订单类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Buy,
    Sell,
}

/// 订单
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub symbol: String,
    pub amount: u64, // 股数，必须是 100 的整数倍
    pub order_type: OrderType,
    pub timestamp: NaiveDateTime,
}

/// 成交记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: String,
    pub amount: u64,
    pub price: Decimal,
    pub order_type: OrderType,
    pub timestamp: NaiveDateTime,
    pub commission: Decimal,
    pub stamp_duty: Decimal,
    pub transfer_fee: Decimal,
    pub pnl: Decimal,
}

/// K 线数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub symbol: String,
    pub timestamp: NaiveDateTime,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: u64,
    pub pre_close: Decimal,
    pub is_st: bool,
}

/// 账户快照（用于净值曲线）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub timestamp: NaiveDateTime,
    pub total_value: Decimal,
    pub cash: Decimal,
    pub position_value: Decimal,
}

/// 月度收益
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyReturn {
    pub month: String,
    #[serde(rename = "return")]
    pub return_pct: Decimal,
}

/// 绩效分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Performance {
    pub total_return: Decimal,
    pub annual_return: Decimal,
    pub max_drawdown: Decimal,
    pub sharpe_ratio: Decimal,
    pub sortino_ratio: Decimal,
    pub calmar_ratio: Decimal,
    pub win_rate: Decimal,
    pub profit_ratio: Decimal,
    pub total_trades: u64,
    pub total_commission: Decimal,
    pub final_value: Decimal,
    pub duration_days: u64,
    pub trading_days: u64,
    pub monthly_returns: Vec<MonthlyReturn>,
}
