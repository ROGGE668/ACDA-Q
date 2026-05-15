//! 回测引擎 — 事件驱动主循环 + 绩效分析

use chrono::{Datelike};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use super::analyzer::calculate_performance;
use super::broker::Broker;
use super::context::Context;
use super::types::{Bar, Performance};

/// 策略 Trait：用户实现 on_bar 方法
pub trait Strategy {
    fn on_init(&mut self) {}
    fn on_bar(&mut self, ctx: &mut Context);
    fn on_exit(&mut self) {}
}

pub struct Engine {
    pub broker: Broker,
    initial_cash: Decimal,
    risk_free_rate: Decimal,
}

impl Engine {
    pub fn new(initial_cash: Decimal) -> Self {
        Self {
            broker: Broker::new(
                initial_cash,
                dec!(0.0003),
                dec!(0.001),
                dec!(0.0005),
                dec!(0.00001),
            ),
            initial_cash,
            risk_free_rate: dec!(0.02),
        }
    }

    pub fn with_options(
        mut self,
        commission: Decimal,
        slippage: Decimal,
        stamp_duty: Decimal,
        transfer_fee: Decimal,
    ) -> Self {
        self.broker = Broker::new(
            self.initial_cash,
            commission,
            slippage,
            stamp_duty,
            transfer_fee,
        );
        self
    }

    pub fn with_risk_free_rate(mut self, rate: Decimal) -> Self {
        self.risk_free_rate = rate;
        self
    }

    /// 执行回测
    pub fn run<S: Strategy>(
        &mut self,
        strategy: &mut S,
        bars: &[Bar],
    ) -> Performance {
        strategy.on_init();

        // 按日期分组（使用 NaiveDate 作为 key 避免时间精度问题）
        let mut groups: HashMap<chrono::NaiveDate, Vec<&Bar>> = HashMap::new();
        for bar in bars {
            groups.entry(bar.timestamp.date()).or_default().push(bar);
        }

        let mut dates: Vec<_> = groups.keys().copied().collect();
        dates.sort();

        for date in dates {
            let group = groups.get(&date).unwrap();
            let group_bars: Vec<Bar> = group.iter().map(|b| (*b).clone()).collect();

            let mut ctx = Context::new(date.and_hms_opt(0, 0, 0).unwrap(), &mut self.broker, &group_bars, bars);
            strategy.on_bar(&mut ctx);

            // 执行待处理订单
            self.broker.execute_pending_orders(&group_bars);

            // 记录净值
            tracing::debug!("After execute: trades={}, equity={}", self.broker.trades.len(), self.broker.equity_curve.len());
            self.broker.record_snapshot(date.and_hms_opt(0, 0, 0).unwrap());
            tracing::debug!("After snapshot: equity={}", self.broker.equity_curve.len());
        }

        strategy.on_exit();
        tracing::debug!("Before calculate_performance: trades={}", self.broker.trades.len());
        calculate_performance(&self.broker, self.initial_cash, self.risk_free_rate)
    }
}

// ========== Mock 数据生成 ==========

pub fn generate_mock_bars(symbol: &str, n_days: usize, seed: u64) -> Vec<Bar> {
    use chrono::{Duration, NaiveDate};
    use rand::prelude::*;

    let mut rng = StdRng::seed_from_u64(seed);
    let start = NaiveDate::from_ymd_opt(2023, 1, 3).unwrap();

    let mut bars = Vec::with_capacity(n_days);
    let mut price = dec!(100);

    for i in 0..n_days {
        let date = start + Duration::days(i as i64);
        // 跳过周末
        if date.weekday().number_from_monday() > 5 {
            continue;
        }

        let ret = Decimal::from_f64(rng.gen_range(-0.02..0.022)).unwrap_or(dec!(0));
        let new_price = price * (Decimal::ONE + ret);
        let noise = Decimal::from_f64(rng.gen_range(0.005..0.02)).unwrap_or(dec!(0));

        let high = new_price.max(price) * (Decimal::ONE + noise);
        let low = new_price.min(price) * (Decimal::ONE - noise);
        let open = price * (Decimal::ONE + Decimal::from_f64(rng.gen_range(-0.01..0.01)).unwrap_or(dec!(0)));

        bars.push(Bar {
            symbol: symbol.to_string(),
            timestamp: date.and_hms_opt(0, 0, 0).unwrap(),
            open: open.max(dec!(0.01)),
            high: high.max(open),
            low: low.max(dec!(0.01)).min(open),
            close: new_price.max(dec!(0.01)),
            volume: rng.gen_range(1_000_000..10_000_000),
            pre_close: price,
            is_st: false,
        });

        price = new_price.max(dec!(0.01));
    }

    bars
}

#[cfg(test)]
mod tests {
    use super::*;

    // 双均线策略
    struct DualMA {
        short_window: usize,
        long_window: usize,
        holding: bool,
    }

    impl Strategy for DualMA {
        fn on_bar(&mut self, ctx: &mut Context) {
            let symbols: Vec<String> = ctx
                .bar_group
                .iter()
                .map(|b| b.symbol.clone())
                .collect();

            for sym in symbols {
                let hist = ctx.history(&sym, self.long_window + 2);
                if hist.len() < self.long_window {
                    continue;
                }

                let short_ma: Decimal = hist.iter().rev().take(self.short_window).sum::<Decimal>()
                    / Decimal::from(self.short_window as u64);
                let long_ma: Decimal = hist.iter().rev().take(self.long_window).sum::<Decimal>()
                    / Decimal::from(self.long_window as u64);
                let prev_short: Decimal = hist[..hist.len() - 1]
                    .iter()
                    .rev()
                    .take(self.short_window)
                    .sum::<Decimal>()
                    / Decimal::from(self.short_window as u64);
                let prev_long: Decimal = hist[..hist.len() - 1]
                    .iter()
                    .rev()
                    .take(self.long_window)
                    .sum::<Decimal>()
                    / Decimal::from(self.long_window as u64);

                if prev_short <= prev_long && short_ma > long_ma && !self.holding {
                    ctx.buy(&sym, dec!(0.5));
                    self.holding = true;
                } else if prev_short >= prev_long && short_ma < long_ma && self.holding {
                    ctx.sell(&sym, Decimal::ONE);
                    self.holding = false;
                }
            }
        }
    }

    #[test]
    fn test_dual_ma_backtest() {
        let bars = generate_mock_bars("000001.SZ", 120, 42);
        assert!(!bars.is_empty());

        let mut engine = Engine::new(dec!(1_000_000));
        let mut strategy = DualMA {
            short_window: 5,
            long_window: 20,
            holding: false,
        };

        let perf = engine.run(&mut strategy, &bars);

        println!("总收益率: {}", perf.total_return);
        println!("年化收益率: {}", perf.annual_return);
        println!("最大回撤: {}", perf.max_drawdown);
        println!("夏普比率: {}", perf.sharpe_ratio);
        println!("总交易次数: {}", perf.total_trades);
        println!("最终资产: {}", perf.final_value);

        assert!(perf.trading_days > 0);
        assert!(perf.final_value > dec!(0));
    }

    #[test]
    fn test_buy_hold_backtest() {
        struct BuyHold;
        impl Strategy for BuyHold {
            fn on_bar(&mut self, ctx: &mut Context) {
                if !ctx.broker.has_positions() {
                    for bar in ctx.bar_group {
                        ctx.buy(&bar.symbol, dec!(0.9) / Decimal::from(ctx.bar_group.len() as u64));
                    }
                }
            }
        }

        let bars = generate_mock_bars("000001.SZ", 60, 123);
        let mut engine = Engine::new(dec!(1_000_000));
        let perf = engine.run(&mut BuyHold, &bars);

        assert!(perf.total_trades >= 1, "BuyHold should have at least one trade, actual trades: {}", perf.total_trades);
        assert!(perf.final_value > dec!(0));
    }
}
