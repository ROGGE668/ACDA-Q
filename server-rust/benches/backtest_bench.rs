use criterion::{black_box, criterion_group, criterion_main, Criterion};
use acda_q::backtest::engine::{generate_mock_bars, Engine};
use acda_q::backtest::context::Context;
use acda_q::backtest::types::{Bar, Order, OrderType};
use acda_q::backtest::engine::Strategy;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

struct DualMA {
    short_window: usize,
    long_window: usize,
    holding: bool,
}

impl Strategy for DualMA {
    fn on_bar(&mut self, ctx: &mut Context) {
        for bar in ctx.bar_group {
            let sym = &bar.symbol;
            let hist = ctx.history(sym, self.long_window + 2);
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
                ctx.buy(sym, dec!(0.5));
                self.holding = true;
            } else if prev_short >= prev_long && short_ma < long_ma && self.holding {
                ctx.sell(sym, Decimal::ONE);
                self.holding = false;
            }
        }
    }
}

fn bench_dual_ma(c: &mut Criterion) {
    let bars = generate_mock_bars("000001.SZ", 500, 42);

    c.bench_function("dual_ma_500_days", |b| {
        b.iter(|| {
            let mut engine = Engine::new(dec!(1_000_000));
            let mut strategy = DualMA {
                short_window: 5,
                long_window: 20,
                holding: false,
            };
            let _perf = engine.run(&mut strategy, black_box(&bars));
        });
    });
}

fn bench_buy_hold(c: &mut Criterion) {
    let bars = generate_mock_bars("000001.SZ", 1000, 99);

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

    c.bench_function("buy_hold_1000_days", |b| {
        b.iter(|| {
            let mut engine = Engine::new(dec!(1_000_000));
            let _perf = engine.run(&mut BuyHold, black_box(&bars));
        });
    });
}

criterion_group!(benches, bench_dual_ma, bench_buy_hold);
criterion_main!(benches);
