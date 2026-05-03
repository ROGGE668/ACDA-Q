"""
经纪商模拟器：模拟A股交易环境，支持滑点、佣金、印花税、持仓管理。
兼容 Python 3.11.8
核心资金计算使用 Decimal 保证精度。
"""
from dataclasses import dataclass, field
from datetime import datetime
from typing import List, Dict
from decimal import Decimal, ROUND_HALF_UP
from enum import Enum
import pandas as pd


class OrderType(Enum):
    BUY = "buy"
    SELL = "sell"


@dataclass
class Order:
    symbol: str
    amount: float
    price: float
    order_type: OrderType
    timestamp: datetime
    filled: bool = False
    fill_price: float = 0.0


@dataclass
class Trade:
    symbol: str
    amount: float
    price: float
    order_type: OrderType
    timestamp: datetime
    commission: float = 0.0
    pnl: float = 0.0


class Broker:
    def __init__(
        self,
        cash: float,
        commission: float = 0.0003,
        slippage: float = 0.001,
        stamp_duty: float = 0.0005,
        transfer_fee: float = 0.00001,
        enable_t1: bool = True,
        enable_limit: bool = True,
    ):
        self._initial_cash = Decimal(str(cash))
        self._cash = Decimal(str(cash))
        self._commission_rate = Decimal(str(commission))
        self._slippage = Decimal(str(slippage))
        self._stamp_duty_rate = Decimal(str(stamp_duty))      # 印花税：仅卖出收取
        self._transfer_fee_rate = Decimal(str(transfer_fee))  # 过户费：双向收取
        self.enable_t1 = enable_t1
        self.enable_limit = enable_limit
        self._positions: Dict[str, Decimal] = {}
        self.pending_orders: List[Order] = []
        self.trades: List[Trade] = []
        self._last_price: Dict[str, Decimal] = {}
        self._cost_basis: Dict[str, Decimal] = {}
        self._today_buy_amounts: Dict[str, Decimal] = {}  # symbol -> 当日净买入量（T+1用）
        self._last_trade_date = None

    @property
    def initial_cash(self) -> float:
        return float(self._initial_cash)

    @property
    def cash(self) -> float:
        return float(self._cash)

    @property
    def total_value(self) -> float:
        """总资产 = 现金 + 所有持仓按最新价市值"""
        position_value = sum(
            qty * self._last_price.get(sym, Decimal("0"))
            for sym, qty in self._positions.items()
        )
        return float(self._cash + position_value)

    @property
    def position_value(self) -> float:
        return float(sum(
            qty * self._last_price.get(sym, Decimal("0"))
            for sym, qty in self._positions.items()
        ))

    @property
    def positions(self) -> Dict[str, float]:
        return {sym: float(qty) for sym, qty in self._positions.items()}

    @staticmethod
    def _get_limit_pct(bar: pd.DataFrame, symbol: str) -> Decimal:
        """
        获取单只股票的涨跌幅限制。
        优先级：
        1. bar 数据中显式指定的 limit_pct 列
        2. ST 股（含 *ST）→ 5%
        3. 基于代码前缀推断板块
        """
        if "limit_pct" in bar.columns:
            raw = bar["limit_pct"].values[0]
            if raw is not None and raw == raw:  # 排除 NaN
                return Decimal(str(raw))
        # ST 股（含 *ST）：无论板块统一 5%
        if "is_st" in bar.columns:
            if bool(bar["is_st"].values[0]):
                return Decimal("0.05")
        # 北交所: 8xxxxx / 4xxxxx → 30%
        if symbol.startswith("8") or symbol.startswith("4") or symbol.startswith("83") or symbol.startswith("43") or symbol.startswith("87"):
            return Decimal("0.3")
        # 科创板: 688xxx → 20%
        if symbol.startswith("688"):
            return Decimal("0.2")
        # 创业板: 300xxx / 301xxx → 20%
        if symbol.startswith("30"):
            return Decimal("0.2")
        # 科创板/创业板注册制新股前5日无涨跌幅限制，但回测中简化处理
        # 主板默认 10%
        return Decimal("0.1")

    def submit_order(self, order: Order) -> None:
        """提交订单到待执行队列"""
        self.pending_orders.append(order)

    def execute_orders(self, bar_group: pd.DataFrame) -> None:
        """根据当前Bar数据执行所有待处理订单（含A股T+1、涨跌停限制）"""
        # 统一处理交易日切换：新交易日开始时清空当日买入量记录
        if self.enable_t1 and not bar_group.empty:
            current_date = pd.to_datetime(bar_group["datetime"].iloc[0]).date()
            if self._last_trade_date != current_date:
                self._today_buy_amounts.clear()
                self._last_trade_date = current_date

        for order in list(self.pending_orders):
            bar = bar_group[bar_group['symbol'] == order.symbol]
            if bar.empty:
                continue

            close = Decimal(str(bar['close'].values[0]))
            pre_close = Decimal(str(bar['pre_close'].values[0])) if 'pre_close' in bar.columns else close
            self._last_price[order.symbol] = close

            # A股涨跌停限制
            if self.enable_limit and pre_close > 0:
                limit_pct = self._get_limit_pct(bar, order.symbol)
                limit_up = pre_close * (Decimal("1") + limit_pct)
                limit_down = pre_close * (Decimal("1") - limit_pct)

                if order.order_type == OrderType.BUY and close >= limit_up * Decimal("0.999"):
                    # 涨停时无法买入（留出微小容差）
                    continue
                if order.order_type == OrderType.SELL and close <= limit_down * Decimal("1.001"):
                    # 跌停时无法卖出
                    continue

            # 模拟滑点
            if order.order_type == OrderType.BUY:
                fill_price = close * (Decimal("1") + self._slippage)
            else:
                fill_price = close * (Decimal("1") - self._slippage)

            amount = Decimal(str(order.amount))
            cost = fill_price * amount
            commission = (cost * self._commission_rate).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
            transfer_fee = (cost * self._transfer_fee_rate).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)

            # 卖出前记录成本价（在更新仓位前，因为全仓卖出会删除 _cost_basis）
            avg_cost_before = self._cost_basis.get(order.symbol)

            if order.order_type == OrderType.BUY:
                total_cost = cost + commission + transfer_fee
                if self._cash < total_cost:
                    continue
                self._cash -= total_cost
                old_qty = self._positions.get(order.symbol, Decimal("0"))
                old_cost = self._cost_basis.get(order.symbol, Decimal("0")) * old_qty
                new_qty = old_qty + amount
                if new_qty > 0:
                    self._cost_basis[order.symbol] = (old_cost + cost) / new_qty
                else:
                    self._cost_basis[order.symbol] = Decimal("0")
                self._positions[order.symbol] = new_qty
                self._today_buy_amounts[order.symbol] = self._today_buy_amounts.get(order.symbol, Decimal("0")) + amount
            else:
                current_qty = self._positions.get(order.symbol, Decimal("0"))
                if current_qty < amount:
                    continue

                # T+1 限制：当日买入的部分不可卖出，旧持仓可以卖出
                if self.enable_t1:
                    today_bought = self._today_buy_amounts.get(order.symbol, Decimal("0"))
                    max_sellable = current_qty - today_bought
                    if amount > max_sellable:
                        continue

                stamp_duty = (cost * self._stamp_duty_rate).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
                self._cash += cost - commission - stamp_duty - transfer_fee
                new_qty = current_qty - amount
                if new_qty <= 0:
                    del self._positions[order.symbol]
                    del self._cost_basis[order.symbol]
                else:
                    self._positions[order.symbol] = new_qty

            # 计算单笔PNL（扣除全部交易成本）
            pnl = Decimal("0")
            total_fees = commission + transfer_fee
            if order.order_type == OrderType.SELL:
                stamp_duty = (cost * self._stamp_duty_rate).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
                total_fees += stamp_duty
                if avg_cost_before is not None:
                    pnl = (fill_price - avg_cost_before) * amount - total_fees

            order.filled = True
            order.fill_price = float(fill_price)
            self.trades.append(Trade(
                symbol=order.symbol,
                amount=order.amount,
                price=float(fill_price),
                order_type=order.order_type,
                timestamp=order.timestamp,
                commission=float(total_fees),
                pnl=float(pnl),
            ))

        # 清除已成交订单
        self.pending_orders = [o for o in self.pending_orders if not o.filled]
