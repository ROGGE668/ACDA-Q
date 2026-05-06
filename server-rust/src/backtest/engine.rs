//! 回测引擎 — 事件驱动主循环 + 绩效分析

use chrono::{Datelike, NaiveDateTime};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

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
        }
    }

    pub fn with_options(mut self, commission: Decimal, slippage: Decimal) -> Self {
        self.broker = Broker::new(
            self.initial_cash,
            commission,
            slippage,
            dec!(0.0005),
            dec!(0.00001),
        );
        self
    }

    /// 执行回测
    pub fn run<S: Strategy>(
        &mut self,
        strategy: &mut S,
        bars: &[Bar],
    ) -> Performance {
        strategy.on_init();

        // 按日期分组
        let mut groups: HashMap<NaiveDateTime, Vec<&Bar>> = HashMap::new();
        for bar in bars {
            groups.entry(bar.timestamp).or_default().push(bar);
        }

        let mut timestamps: Vec<_> = groups.keys().copied().collect();
        timestamps.sort();

        for ts in timestamps {
            let group = groups.get(&ts).unwrap();
            let group_bars: Vec<Bar> = group.iter().map(|b| (*b).clone()).collect();

            let mut ctx = Context::new(ts, &mut self.broker, &group_bars, bars);
            strategy.on_bar(&mut ctx);

            // 记录净值
            self.broker.record_snapshot(ts);
        }

        strategy.on_exit();
        self.calculate_performance()
    }

    fn calculate_performance(&self) -> Performance {
        let curve = &self.broker.equity_curve;
        if curve.is_empty() {
            return Performance {
                total_return: dec!(0),
                annual_return: dec!(0),
                max_drawdown: dec!(0),
                sharpe_ratio: dec!(0),
                sortino_ratio: dec!(0),
                win_rate: dec!(0),
                total_trades: 0,
                total_commission: dec!(0),
                final_value: self.initial_cash,
                duration_days: 0,
                trading_days: 0,
            };
        }

        let initial = self.initial_cash;
        let final_val = curve.last().unwrap().total_value;
        let total_return = (final_val - initial) / initial;

        let trading_days = curve.len() as u64;
        let duration = if trading_days > 0 {
            (curve.last().unwrap().timestamp - curve.first().unwrap().timestamp).num_days() as u64
        } else {
            0
        };

        // 最大回撤
        let mut cummax = dec!(0);
        let mut max_dd = dec!(0);
        for snap in curve.iter() {
            if snap.total_value > cummax {
                cummax = snap.total_value;
            }
            let dd = (snap.total_value - cummax) / cummax;
            if dd < max_dd {
                max_dd = dd;
            }
        }

        // 收益率序列
        let mut returns: Vec<Decimal> = Vec::new();
        for i in 1..curve.len() {
            let r = (curve[i].total_value - curve[i - 1].total_value) / curve[i - 1].total_value;
            returns.push(r);
        }

        // 夏普比率（简化：无风险利率 2%）
        let rf_daily = dec!(0.02) / Decimal::from(252u64);
        let (sharpe, sortino) = if !returns.is_empty() {
            let mean: Decimal = returns.iter().sum::<Decimal>() / Decimal::from(returns.len() as u64);
            let excess = mean - rf_daily;

            let variance: Decimal = returns
                .iter()
                .map(|r| {
                    let diff = *r - mean;
                    diff * diff
                })
                .sum::<Decimal>()
                / Decimal::from(returns.len() as u64);
            let std_dev = variance.sqrt().unwrap_or(dec!(0));

            let sharpe = if std_dev > dec!(0) {
                let sqrt_252 = Decimal::from(252u64).sqrt().unwrap_or(dec!(0));
                excess * sqrt_252 / std_dev
            } else {
                dec!(0)
            };

            let downside: Vec<Decimal> = returns.iter().filter(|r| **r < dec!(0)).copied().collect();
            let downside_std = if !downside.is_empty() {
                let d_mean = downside.iter().sum::<Decimal>() / Decimal::from(downside.len() as u64);
                let d_var = downside
                    .iter()
                    .map(|r| {
                        let diff = *r - d_mean;
                        diff * diff
                    })
                    .sum::<Decimal>()
                    / Decimal::from(downside.len() as u64);
                d_var.sqrt().unwrap_or(dec!(0))
            } else {
                dec!(0)
            };

            let sortino = if downside_std > dec!(0) {
                let sqrt_252 = Decimal::from(252u64).sqrt().unwrap_or(dec!(0));
                excess * sqrt_252 / downside_std
            } else {
                dec!(0)
            };

            (sharpe, sortino)
        } else {
            (dec!(0), dec!(0))
        };

        // 胜率
        let mut wins = 0u64;
        let mut total_pnl = dec!(0);
        let mut total_commission = dec!(0);
        for trade in &self.broker.trades {
            total_commission += trade.commission + trade.stamp_duty + trade.transfer_fee;
            if trade.pnl > dec!(0) {
                wins += 1;
            }
            total_pnl += trade.pnl;
        }

        let win_rate = if !self.broker.trades.is_empty() {
            Decimal::from(wins) / Decimal::from(self.broker.trades.len() as u64)
        } else {
            dec!(0)
        };

        // 年化收益（按交易日 252）
        let annual_return = if trading_days > 0 {
            let years = Decimal::from(trading_days) / Decimal::from(252u64);
            if years > dec!(0) {
                let years_f = years.to_f64().unwrap_or(0.0);
                let total_f = total_return.to_f64().unwrap_or(0.0);
                if years_f > 0.0 {
                    let annual_f = (1.0 + total_f).powf(1.0 / years_f) - 1.0;
                    Decimal::from_f64(annual_f).unwrap_or(dec!(0))
                } else {
                    dec!(0)
                }
            } else {
                dec!(0)
            }
        } else {
            dec!(0)
        };

        Performance {
            total_return,
            annual_return,
            max_drawdown: max_dd,
            sharpe_ratio: sharpe,
            sortino_ratio: sortino,
            win_rate,
            total_trades: self.broker.trades.len() as u64,
            total_commission,
            final_value: final_val,
            duration_days: duration.max(0) as u64,
            trading_days,
        }
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
                        ctx.buy(&bar.symbol, Decimal::ONE / Decimal::from(ctx.bar_group.len() as u64));
                    }
                }
            }
        }

        let bars = generate_mock_bars("000001.SZ", 60, 123);
        let mut engine = Engine::new(dec!(1_000_000));
        let perf = engine.run(&mut BuyHold, &bars);

        assert!(perf.total_trades >= 1);
    }
}
