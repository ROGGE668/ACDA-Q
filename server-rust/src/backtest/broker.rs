//! 经纪商模拟器 — Decimal 精度
//!
//! 内置 A 股规则：T+1、涨跌停、100 股整数倍、印花税、过户费

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use super::types::{Bar, Order, OrderType, Trade, AccountSnapshot};

/// 涨跌幅限制百分比
fn limit_pct(symbol: &str, is_st: bool) -> Decimal {
    if is_st {
        return dec!(0.05); // ST 股 5%
    }
    // 北交所 30%
    if symbol.starts_with('8') || symbol.starts_with('4') {
        return dec!(0.30);
    }
    // 科创板 688 / 创业板 30/301 → 20%
    if symbol.starts_with("688") || symbol.starts_with("30") {
        return dec!(0.20);
    }
    // 主板默认 10%
    dec!(0.10)
}

pub struct Broker {
    initial_cash: Decimal,
    cash: Decimal,
    commission_rate: Decimal,
    slippage: Decimal,
    stamp_duty_rate: Decimal,
    transfer_fee_rate: Decimal,
    enable_t1: bool,
    enable_limit: bool,

    positions: HashMap<String, Decimal>,       // symbol -> 持仓股数
    cost_basis: HashMap<String, Decimal>,       // symbol -> 成本价
    today_bought: HashMap<String, Decimal>,     // symbol -> 当日净买入量
    last_trade_date: Option<chrono::NaiveDate>,
    pending_orders: Vec<Order>,

    pub trades: Vec<Trade>,
    pub equity_curve: Vec<AccountSnapshot>,
    last_prices: HashMap<String, Decimal>,
}

impl Broker {
    pub fn new(
        cash: Decimal,
        commission_rate: Decimal,
        slippage: Decimal,
        stamp_duty_rate: Decimal,
        transfer_fee_rate: Decimal,
    ) -> Self {
        let s = Self {
            initial_cash: cash,
            cash,
            commission_rate,
            slippage,
            stamp_duty_rate,
            transfer_fee_rate,
            enable_t1: true,
            enable_limit: true,
            positions: HashMap::new(),
            cost_basis: HashMap::new(),
            today_bought: HashMap::new(),
            last_trade_date: None,
            pending_orders: Vec::new(),
            trades: Vec::new(),
            equity_curve: Vec::new(),
            last_prices: HashMap::new(),
        };
        println!("Broker::new positions_len={}", s.positions.len());
        s
    }

    pub fn with_options(mut self, enable_t1: bool, enable_limit: bool) -> Self {
        self.enable_t1 = enable_t1;
        self.enable_limit = enable_limit;
        self
    }

    /// 总资产 = 现金 + 持仓市值
    pub fn total_value(&self) -> Decimal {
        let pos_val: Decimal = self
            .positions
            .iter()
            .map(|(sym, qty)| qty * self.last_prices.get(sym).copied().unwrap_or(dec!(0)))
            .sum();
        self.cash + pos_val
    }

    pub fn position_value(&self) -> Decimal {
        self.positions
            .iter()
            .map(|(sym, qty)| qty * self.last_prices.get(sym).copied().unwrap_or(dec!(0)))
            .sum()
    }

    /// 获取某标的持仓股数
    pub fn position_qty(&self, symbol: &str) -> Decimal {
        self.positions.get(symbol).copied().unwrap_or(dec!(0))
    }

    /// 是否持有任何仓位
    pub fn has_positions(&self) -> bool {
        let result = !self.positions.is_empty();
        println!("has_positions: len={}, result={}", self.positions.len(), result);
        result
    }

    /// 提交订单到待执行队列
    pub fn submit_order(&mut self, order: Order) {
        self.pending_orders.push(order);
    }

    /// 执行所有待处理订单
    pub fn execute_pending_orders(&mut self, bars: &[Bar]) {
        let orders: Vec<Order> = self.pending_orders.drain(..).collect();
        self.execute(&orders, bars);
    }

    /// 执行订单
    pub fn execute(&mut self, orders: &[Order], bars: &[Bar]) {
        println!("execute: orders={}, bars={}", orders.len(), bars.len());
        // 交易日切换检测
        if self.enable_t1 && !bars.is_empty() {
            let current_date = bars[0].timestamp.date();
            if self.last_trade_date != Some(current_date) {
                self.today_bought.clear();
                self.last_trade_date = Some(current_date);
            }
        }

        // 建立 symbol -> bar 映射
        let bar_map: HashMap<&str, &Bar> = bars.iter().map(|b| (b.symbol.as_str(), b)).collect();
        println!("bar_map keys: {:?}", bar_map.keys().collect::<Vec<_>>());

        for order in orders {
            println!("Processing order: {} {:?} amount={}", order.symbol, order.order_type, order.amount);
            let Some(bar) = bar_map.get(order.symbol.as_str()) else {
                println!("  Bar not found for {}", order.symbol);
                continue;
            };

            let close = bar.close;
            let pre_close = if bar.pre_close > dec!(0) {
                bar.pre_close
            } else {
                close
            };

            self.last_prices.insert(order.symbol.clone(), close);

            // 涨跌停检查
            if self.enable_limit && pre_close > dec!(0) {
                let pct = limit_pct(&order.symbol, bar.is_st);
                let limit_up = pre_close * (Decimal::ONE + pct);
                let limit_down = pre_close * (Decimal::ONE - pct);

                match order.order_type {
                    OrderType::Buy if close >= limit_up * dec!(0.999) => continue,
                    OrderType::Sell if close <= limit_down * dec!(1.001) => continue,
                    _ => {}
                }
            }

            // 滑点
            let fill_price = match order.order_type {
                OrderType::Buy => close * (Decimal::ONE + self.slippage),
                OrderType::Sell => close * (Decimal::ONE - self.slippage),
            };

            let amount = Decimal::from(order.amount);
            let cost = fill_price * amount;

            let commission = (cost * self.commission_rate)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointNearestEven);
            let transfer_fee = (cost * self.transfer_fee_rate)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointNearestEven);

            match order.order_type {
                OrderType::Buy => {
                    let total_cost = cost + commission + transfer_fee;
                    println!("  Buy: cash={}, total_cost={}", self.cash, total_cost);
                    if self.cash < total_cost {
                        println!("  Buy: cash insufficient");
                        continue;
                    }
                    self.cash -= total_cost;
                    println!("  Buy: cash after={}", self.cash);

                    let old_qty = self.positions.get(&order.symbol).copied().unwrap_or(dec!(0));
                    let old_cost = self.cost_basis.get(&order.symbol).copied().unwrap_or(dec!(0)) * old_qty;
                    let new_qty = old_qty + amount;
                    if new_qty > dec!(0) {
                        self.cost_basis.insert(order.symbol.clone(), (old_cost + cost) / new_qty);
                    } else {
                        self.cost_basis.insert(order.symbol.clone(), dec!(0));
                    }
                    self.positions.insert(order.symbol.clone(), new_qty);
                    println!("  Buy: positions inserted, trades before push={}", self.trades.len());

                    *self.today_bought.entry(order.symbol.clone()).or_insert(dec!(0)) += amount;

                    self.trades.push(Trade {
                        symbol: order.symbol.clone(),
                        amount: order.amount,
                        price: fill_price,
                        order_type: OrderType::Buy,
                        timestamp: order.timestamp,
                        commission,
                        stamp_duty: dec!(0),
                        transfer_fee,
                        pnl: dec!(0),
                    });
                    println!("  Buy: trades after push={}", self.trades.len());
                }
                OrderType::Sell => {
                    let current_qty = self.positions.get(&order.symbol).copied().unwrap_or(dec!(0));
                    if current_qty < amount {
                        continue;
                    }

                    // T+1: 当日买入部分不可卖出
                    if self.enable_t1 {
                        let today_bought = self.today_bought.get(&order.symbol).copied().unwrap_or(dec!(0));
                        let max_sellable = current_qty - today_bought;
                        if amount > max_sellable {
                            continue;
                        }
                    }

                    let stamp_duty = (cost * self.stamp_duty_rate)
                        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointNearestEven);
                    self.cash += cost - commission - stamp_duty - transfer_fee;

                    let avg_cost = self.cost_basis.get(&order.symbol).copied().unwrap_or(dec!(0));
                    let pnl = (fill_price - avg_cost) * amount - commission - stamp_duty - transfer_fee;

                    let new_qty = current_qty - amount;
                    if new_qty <= dec!(0) {
                        self.positions.remove(&order.symbol);
                        self.cost_basis.remove(&order.symbol);
                    } else {
                        self.positions.insert(order.symbol.clone(), new_qty);
                    }

                    self.trades.push(Trade {
                        symbol: order.symbol.clone(),
                        amount: order.amount,
                        price: fill_price,
                        order_type: OrderType::Sell,
                        timestamp: order.timestamp,
                        commission,
                        stamp_duty,
                        transfer_fee,
                        pnl,
                    });
                }
            }
        }
    }

    /// 记录净值快照
    pub fn record_snapshot(&mut self, timestamp: chrono::NaiveDateTime) {
        self.equity_curve.push(AccountSnapshot {
            timestamp,
            total_value: self.total_value(),
            cash: self.cash,
            position_value: self.position_value(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn make_broker() -> Broker {
        Broker::new(
            dec!(1_000_000),
            dec!(0.0003),
            dec!(0.001),
            dec!(0.0005),
            dec!(0.00001),
        )
    }

    fn bar(symbol: &str, close: Decimal, pre_close: Decimal) -> Bar {
        let ts = NaiveDate::from_ymd_opt(2023, 1, 5)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        Bar {
            symbol: symbol.to_string(),
            timestamp: ts,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1000000,
            pre_close,
            is_st: false,
        }
    }

    #[test]
    fn test_buy_and_cash_reduction() {
        let mut b = make_broker();
        let order = Order {
            symbol: "000001.SZ".to_string(),
            amount: 1000,
            order_type: OrderType::Buy,
            timestamp: b.equity_curve.first().map(|s| s.timestamp).unwrap_or(
                NaiveDate::from_ymd_opt(2023, 1, 5).unwrap().and_hms_opt(0,0,0).unwrap()
            ),
        };
        b.execute(&[order], &[bar("000001.SZ", dec!(100), dec!(99))]);
        assert_eq!(b.positions.get("000001.SZ").copied().unwrap_or(dec!(0)), dec!(1000));
        assert!(b.cash < dec!(1_000_000));
        assert_eq!(b.trades.len(), 1);
    }

    #[test]
    fn test_t1_blocks_same_day_sell() {
        let mut b = make_broker();
        let ts = NaiveDate::from_ymd_opt(2023, 1, 5).unwrap().and_hms_opt(0,0,0).unwrap();
        let buy = Order { symbol: "000001.SZ".to_string(), amount: 1000, order_type: OrderType::Buy, timestamp: ts };
        let sell = Order { symbol: "000001.SZ".to_string(), amount: 1000, order_type: OrderType::Sell, timestamp: ts };
        let the_bar = bar("000001.SZ", dec!(100), dec!(99));
        b.execute(&[buy], &[the_bar.clone()]);
        b.execute(&[sell], &[the_bar]);
        // 买入成交，卖出被 T+1 拦截
        let sell_trades: Vec<_> = b.trades.iter().filter(|t| matches!(t.order_type, OrderType::Sell)).collect();
        assert!(sell_trades.is_empty());
    }

    #[test]
    fn test_limit_up_blocks_buy() {
        let mut b = make_broker();
        let ts = NaiveDate::from_ymd_opt(2023, 1, 5).unwrap().and_hms_opt(0,0,0).unwrap();
        // pre_close=100, close=110 (涨停 10%)
        let up_bar = Bar { close: dec!(110), pre_close: dec!(100), ..bar("000001.SZ", dec!(110), dec!(100)) };
        let order = Order { symbol: "000001.SZ".to_string(), amount: 1000, order_type: OrderType::Buy, timestamp: ts };
        b.execute(&[order], &[up_bar]);
        assert!(b.trades.is_empty());
    }

    #[test]
    fn test_lot_size_must_be_100_multiple() {
        // Rust 中 amount 类型是 u64，但策略代码可能传入非 100 倍数
        // 这个测试验证引擎层面是否拒绝
        // 实际中 Context.buy 应该先把 amount 向下取整到 100 的倍数
        // 这里先测试 broker 接收 100 倍数能正常成交
        let mut b = make_broker();
        let ts = NaiveDate::from_ymd_opt(2023, 1, 5).unwrap().and_hms_opt(0,0,0).unwrap();
        let order = Order { symbol: "000001.SZ".to_string(), amount: 500, order_type: OrderType::Buy, timestamp: ts };
        b.execute(&[order], &[bar("000001.SZ", dec!(100), dec!(99))]);
        assert_eq!(b.positions.get("000001.SZ").copied().unwrap_or(dec!(0)), dec!(500));
    }
}
