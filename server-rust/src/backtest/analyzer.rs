//! 绩效分析器 — 从回测结果计算风险收益指标

use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use crate::backtest::broker::Broker;
use crate::backtest::types::{AccountSnapshot, MonthlyReturn, Performance};

/// 计算绩效指标
pub fn calculate_performance(
    broker: &Broker,
    initial_cash: Decimal,
    risk_free_rate: Decimal,
) -> Performance {
    let curve = &broker.equity_curve;
    if curve.is_empty() {
        return Performance {
            total_return: dec!(0),
            annual_return: dec!(0),
            max_drawdown: dec!(0),
            sharpe_ratio: dec!(0),
            sortino_ratio: dec!(0),
            calmar_ratio: dec!(0),
            win_rate: dec!(0),
            profit_ratio: dec!(0),
            total_trades: 0,
            total_commission: dec!(0),
            final_value: initial_cash,
            duration_days: 0,
            trading_days: 0,
            monthly_returns: vec![],
        };
    }

    let final_val = curve.last().unwrap().total_value;
    let total_return = (final_val - initial_cash) / initial_cash;

    let trading_days = curve.len() as u64;
    let duration = if trading_days > 1 {
        (curve.last().unwrap().timestamp - curve.first().unwrap().timestamp).num_days() as u64
    } else {
        0
    };

    // 最大回撤
    let mut running_max = dec!(0);
    let mut max_dd = dec!(0);
    for snap in curve.iter() {
        if snap.total_value > running_max {
            running_max = snap.total_value;
        }
        let dd = (snap.total_value - running_max) / running_max;
        if dd < max_dd {
            max_dd = dd;
        }
    }

    // 收益率序列
    let mut returns: Vec<Decimal> = Vec::with_capacity(curve.len().saturating_sub(1));
    for i in 1..curve.len() {
        let r = (curve[i].total_value - curve[i - 1].total_value) / curve[i - 1].total_value;
        returns.push(r);
    }

    // 夏普 / 索提诺
    let (sharpe, sortino) = if !returns.is_empty() {
        let mean = returns.iter().sum::<Decimal>() / Decimal::from(returns.len() as u64);
        let rf_daily = risk_free_rate / Decimal::from(252u64);
        let excess = mean - rf_daily;

        let variance = returns
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

    // FIFO 盈亏配对计算 win_rate / profit_ratio
    let (win_rate, profit_ratio, _total_paired_trades, total_commission) =
        calculate_fifo_stats(broker);

    // Calmar
    let calmar = if max_dd != dec!(0) {
        let annual = calculate_annual_return(total_return, trading_days);
        annual / max_dd.abs()
    } else {
        dec!(0)
    };

    // 月度收益
    let monthly_returns = calculate_monthly_returns(curve);

    Performance {
        total_return,
        annual_return: calculate_annual_return(total_return, trading_days),
        max_drawdown: max_dd,
        sharpe_ratio: sharpe,
        sortino_ratio: sortino,
        calmar_ratio: calmar,
        win_rate,
        profit_ratio,
        total_trades: broker.trades.len() as u64,
        total_commission,
        final_value: final_val,
        duration_days: duration.max(0) as u64,
        trading_days,
        monthly_returns,
    }
}

/// 计算年化收益（复利）
fn calculate_annual_return(total_return: Decimal, trading_days: u64) -> Decimal {
    if trading_days == 0 {
        return dec!(0);
    }
    let years = Decimal::from(trading_days) / Decimal::from(252u64);
    if years <= dec!(0) {
        return dec!(0);
    }

    let one_plus_r = Decimal::ONE + total_return;
    if one_plus_r <= dec!(0) {
        return dec!(-1);
    }

    // 使用 Decimal 的 ln/exp 避免 f64 精度损失
    let ln_val = one_plus_r.ln();
    if ln_val.is_zero() && one_plus_r != Decimal::ONE {
        return dec!(0);
    }
    let annual_ln = ln_val / years;
    let exp_val = annual_ln.exp();
    exp_val - Decimal::ONE
}

/// FIFO 盈亏配对统计
fn calculate_fifo_stats(broker: &Broker) -> (Decimal, Decimal, u64, Decimal) {
    use std::collections::{HashMap, VecDeque};

    #[derive(Debug, Clone)]
    struct BuyLot {
        price: Decimal,
        amount: u64,
        commission: Decimal,
    }

    let mut pending_buys: HashMap<String, VecDeque<BuyLot>> = HashMap::new();
    let mut paired_pnls: Vec<Decimal> = Vec::new();
    let mut total_commission = dec!(0);

    for trade in &broker.trades {
        total_commission += trade.commission + trade.stamp_duty + trade.transfer_fee;

        match trade.order_type {
            crate::backtest::types::OrderType::Buy => {
                pending_buys
                    .entry(trade.symbol.clone())
                    .or_default()
                    .push_back(BuyLot {
                        price: trade.price,
                        amount: trade.amount,
                        commission: trade.commission + trade.transfer_fee,
                    });
            }
            crate::backtest::types::OrderType::Sell => {
                let mut remaining = trade.amount;
                let sell_price = trade.price;
                let sell_commission = trade.commission + trade.stamp_duty + trade.transfer_fee;

                if let Some(queue) = pending_buys.get_mut(&trade.symbol) {
                    while remaining > 0 && !queue.is_empty() {
                        let buy = queue.front().unwrap();
                        let use_amount = remaining.min(buy.amount);

                        let buy_comm_ratio = if buy.amount > 0 {
                            Decimal::from(use_amount) / Decimal::from(buy.amount)
                        } else {
                            dec!(0)
                        };
                        let buy_comm = buy.commission * buy_comm_ratio;

                        let sell_comm_ratio = if trade.amount > 0 {
                            Decimal::from(use_amount) / Decimal::from(trade.amount)
                        } else {
                            dec!(0)
                        };
                        let sell_comm = sell_commission * sell_comm_ratio;

                        let pnl = (sell_price - buy.price) * Decimal::from(use_amount) - buy_comm - sell_comm;
                        paired_pnls.push(pnl);

                        remaining -= use_amount;
                        let front = queue.front_mut().unwrap();
                        front.amount -= use_amount;
                        if front.amount == 0 {
                            queue.pop_front();
                        }
                    }
                }
            }
        }
    }

    if paired_pnls.is_empty() {
        return (dec!(0), dec!(0), 0, total_commission);
    }

    let wins: Vec<_> = paired_pnls.iter().filter(|&&p| p > dec!(0)).collect();
    let losses: Vec<_> = paired_pnls.iter().filter(|&&p| p < dec!(0)).collect();

    let win_rate = Decimal::from(wins.len() as u64) / Decimal::from(paired_pnls.len() as u64);

    let avg_win = if !wins.is_empty() {
        wins.iter().map(|&&v| v).sum::<Decimal>() / Decimal::from(wins.len() as u64)
    } else {
        dec!(0)
    };

    let avg_loss = if !losses.is_empty() {
        losses.iter().map(|&&v| v.abs()).sum::<Decimal>() / Decimal::from(losses.len() as u64)
    } else {
        dec!(1) // avoid div by zero
    };

    let profit_ratio = if avg_loss > dec!(0) {
        avg_win / avg_loss
    } else {
        dec!(0)
    };

    (win_rate, profit_ratio, paired_pnls.len() as u64, total_commission)
}

/// 计算月度收益
fn calculate_monthly_returns(curve: &[AccountSnapshot]) -> Vec<MonthlyReturn> {
    use std::collections::BTreeMap;

    let mut monthly: BTreeMap<String, Vec<Decimal>> = BTreeMap::new();

    for snap in curve {
        let key = snap.timestamp.format("%Y-%m").to_string();
        monthly.entry(key).or_default().push(snap.total_value);
    }

    let mut result = Vec::new();
    let mut prev_value: Option<Decimal> = None;

    for (month, values) in monthly {
        if let Some(last) = values.last() {
            if let Some(prev) = prev_value {
                let ret = (*last - prev) / prev;
                result.push(MonthlyReturn {
                    month,
                    return_pct: ret,
                });
            }
            prev_value = Some(*last);
        }
    }

    result
}
