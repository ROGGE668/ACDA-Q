//! 策略运行时上下文
//!
//! 封装 history、sma、ema、buy、sell 等接口。

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::MathematicalOps;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use super::broker::Broker;
use super::types::{Bar, Order, OrderType};

pub struct Context<'a> {
    pub timestamp: chrono::NaiveDateTime,
    pub broker: &'a mut Broker,
    pub bar_group: &'a [Bar],
    all_bars: &'a [Bar],
    history_cache: HashMap<String, Vec<Decimal>>,
}

impl<'a> Context<'a> {
    pub fn new(
        timestamp: chrono::NaiveDateTime,
        broker: &'a mut Broker,
        bar_group: &'a [Bar],
        all_bars: &'a [Bar],
    ) -> Self {
        Self {
            timestamp,
            broker,
            bar_group,
            all_bars,
            history_cache: HashMap::new(),
        }
    }

    /// 获取某标的最近 lookback 条收盘价历史
    pub fn history(&mut self, symbol: &str, lookback: usize) -> Vec<Decimal> {
        if let Some(cached) = self.history_cache.get(symbol) {
            return cached.clone();
        }

        let hist: Vec<Decimal> = self
            .all_bars
            .iter()
            .filter(|b| b.symbol == symbol && b.timestamp < self.timestamp)
            .map(|b| b.close)
            .collect();

        let result = hist.into_iter().rev().take(lookback).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>();
        self.history_cache.insert(symbol.to_string(), result.clone());
        result
    }

    /// 简单移动平均
    #[allow(dead_code)]
    pub fn sma(&mut self, symbol: &str, period: usize) -> Decimal {
        let hist = self.history(symbol, period);
        if hist.len() < period {
            return dec!(0);
        }
        let sum: Decimal = hist.iter().sum();
        sum / Decimal::from(period as u64)
    }

    /// 指数移动平均
    #[allow(dead_code)]
    pub fn ema(&mut self, symbol: &str, period: usize) -> Decimal {
        let lookback = period * 2;
        let hist = self.history(symbol, lookback);
        if hist.len() < period {
            return dec!(0);
        }

        let multiplier = dec!(2) / (Decimal::from(period as u64) + Decimal::ONE);
        let mut ema = hist[0];
        for price in hist.iter().skip(1) {
            ema = (*price - ema) * multiplier + ema;
        }
        ema
    }

    /// RSI (相对强弱指标)
    #[allow(dead_code)]
    pub fn rsi(&mut self, symbol: &str, period: usize) -> Decimal {
        let hist = self.history(symbol, period + 1);
        if hist.len() < period + 1 {
            return dec!(0);
        }

        let mut gains = dec!(0);
        let mut losses = dec!(0);
        let start = hist.len() - period;

        for i in (start + 1)..hist.len() {
            let change = hist[i] - hist[i - 1];
            if change > dec!(0) {
                gains += change;
            } else {
                losses += change.abs();
            }
        }

        let avg_gain = gains / Decimal::from(period as u64);
        let avg_loss = losses / Decimal::from(period as u64);

        if avg_loss == dec!(0) {
            return dec!(100);
        }

        let rs = avg_gain / avg_loss;
        dec!(100) - (dec!(100) / (Decimal::ONE + rs))
    }

    /// Bollinger Bands (布林带)
    /// 返回 (上轨, 中轨, 下轨)
    #[allow(dead_code)]
    pub fn bollinger_bands(&mut self, symbol: &str, period: usize) -> (Decimal, Decimal, Decimal) {
        let hist = self.history(symbol, period);
        if hist.len() < period {
            return (dec!(0), dec!(0), dec!(0));
        }

        let sum: Decimal = hist.iter().sum();
        let middle = sum / Decimal::from(period as u64);

        let variance_sum: Decimal = hist
            .iter()
            .map(|price| {
                let diff = *price - middle;
                diff * diff
            })
            .sum();

        let variance = variance_sum / Decimal::from(period as u64);
        let std_dev = variance.sqrt().unwrap_or(dec!(0));

        let upper = middle + std_dev * dec!(2);
        let lower = middle - std_dev * dec!(2);

        (upper, middle, lower)
    }

    /// MACD (指数平滑异同平均线)
    /// 返回 (macd_line, signal_line, histogram)
    #[allow(dead_code)]
    pub fn macd(&mut self, symbol: &str, fast: usize, slow: usize, signal: usize) -> (Decimal, Decimal, Decimal) {
        let total = slow + signal;
        let hist = self.history(symbol, total * 2);
        if hist.len() < total {
            return (dec!(0), dec!(0), dec!(0));
        }

        let fast_mul = dec!(2) / (Decimal::from(fast as u64) + Decimal::ONE);
        let slow_mul = dec!(2) / (Decimal::from(slow as u64) + Decimal::ONE);
        let signal_mul = dec!(2) / (Decimal::from(signal as u64) + Decimal::ONE);

        // 计算 fast_ema 和 slow_ema 序列
        let mut fast_ema = hist[0];
        let mut slow_ema = hist[0];
        let mut macd_values = Vec::new();

        for price in hist.iter().skip(1) {
            fast_ema = (*price - fast_ema) * fast_mul + fast_ema;
            slow_ema = (*price - slow_ema) * slow_mul + slow_ema;
            macd_values.push(fast_ema - slow_ema);
        }

        if macd_values.len() < signal {
            return (dec!(0), dec!(0), dec!(0));
        }

        // 计算 signal_line (macd_values 的 EMA)
        let mut signal_ema = macd_values[0];
        for val in macd_values.iter().skip(1) {
            signal_ema = (*val - signal_ema) * signal_mul + signal_ema;
        }

        let macd_line = *macd_values.last().unwrap_or(&dec!(0));
        let histogram = macd_line - signal_ema;

        (macd_line, signal_ema, histogram)
    }

    /// 买入（按金额百分比）
    pub fn buy(&mut self, symbol: &str, percent: Decimal) {
        let total = self.broker.total_value();
        let target_value = total * percent;

        let price = self.current_price(symbol);
        if price == dec!(0) {
            return;
        }

        let raw_amount = (target_value / price).floor();
        let amount = (raw_amount / dec!(100)) * dec!(100); // 向下取整到 100 的倍数
        let amount_u64: u64 = amount.trunc().to_u64().unwrap_or(0);

        if amount_u64 > 0 {
            self.broker.submit_order(Order {
                symbol: symbol.to_string(),
                amount: amount_u64,
                order_type: OrderType::Buy,
                timestamp: self.timestamp,
            });
        }
    }

    /// 卖出（按持仓百分比）
    pub fn sell(&mut self, symbol: &str, percent: Decimal) {
        let qty = self.broker.position_qty(symbol);
        let raw_amount = (qty * percent).floor();
        let amount = (raw_amount / dec!(100)) * dec!(100);
        let amount_u64: u64 = amount.trunc().to_u64().unwrap_or(0);

        if amount_u64 > 0 {
            self.broker.submit_order(Order {
                symbol: symbol.to_string(),
                amount: amount_u64,
                order_type: OrderType::Sell,
                timestamp: self.timestamp,
            });
        }
    }

    fn current_price(&self, symbol: &str) -> Decimal {
        self.bar_group
            .iter()
            .find(|b| b.symbol == symbol)
            .map(|b| b.close)
            .unwrap_or(dec!(0))
    }
}
