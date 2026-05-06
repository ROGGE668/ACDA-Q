//! 策略运行时上下文
//!
//! 封装 history、sma、ema、buy、sell 等接口。

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
    pub fn sma(&mut self, symbol: &str, period: usize) -> Decimal {
        let hist = self.history(symbol, period);
        if hist.len() < period {
            return dec!(0);
        }
        let sum: Decimal = hist.iter().sum();
        sum / Decimal::from(period as u64)
    }

    /// 指数移动平均
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
        let amount_u64: u64 = amount.to_u64().unwrap_or(0);

        if amount_u64 > 0 {
            self.broker.execute(
                &[Order {
                    symbol: symbol.to_string(),
                    amount: amount_u64,
                    order_type: OrderType::Buy,
                    timestamp: self.timestamp,
                }],
                self.bar_group,
            );
        }
    }

    /// 卖出（按持仓百分比）
    pub fn sell(&mut self, symbol: &str, percent: Decimal) {
        let qty = self.broker.positions.get(symbol).copied().unwrap_or(dec!(0));
        let raw_amount = (qty * percent).floor();
        let amount = (raw_amount / dec!(100)) * dec!(100);
        let amount_u64: u64 = amount.to_u64().unwrap_or(0);

        if amount_u64 > 0 {
            self.broker.execute(
                &[Order {
                    symbol: symbol.to_string(),
                    amount: amount_u64,
                    order_type: OrderType::Sell,
                    timestamp: self.timestamp,
                }],
                self.bar_group,
            );
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
