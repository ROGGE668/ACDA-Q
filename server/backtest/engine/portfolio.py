"""
组合回测引擎：支持多标的等权/市值加权/自定义权重、定期再平衡。
兼容 Python 3.11.8
"""
from typing import List, Dict, Any, Optional
from datetime import datetime
import pandas as pd
from server.backtest.engine.core import BacktestEngine, Context
from server.backtest.engine.broker import Broker, Order, OrderType
from server.backtest.engine.strategy_base import BaseStrategy


class PortfolioEngine(BacktestEngine):
    """
    组合回测引擎，继承自 BacktestEngine。
    在单股回测基础上增加多标的权重分配与再平衡。
    """

    LOT_SIZE = 100

    def __init__(
        self,
        initial_cash: float = 1_000_000.0,
        commission: float = 0.0003,
        slippage: float = 0.001,
        stamp_duty: float = 0.0005,
        transfer_fee: float = 0.00001,
        weight_mode: str = "equal",  # equal / market_cap / custom
        custom_weights: Optional[Dict[str, float]] = None,
        rebalance_freq: str = "1M",  # 1W / 1M / 3M / none
    ):
        super().__init__(initial_cash, commission, slippage, stamp_duty, transfer_fee)
        self.weight_mode = weight_mode
        self.custom_weights = custom_weights or {}
        self.rebalance_freq = rebalance_freq
        self._target_weights: Dict[str, float] = {}
        self._last_rebalance: Optional[datetime] = None

    def run(
        self,
        symbols: List[str],
        start_date: str,
        end_date: str,
        freq: str = "1d",
    ) -> Dict[str, Any]:
        self._symbols = symbols
        bars = self.data_feed.load_bars(symbols, start_date, end_date, freq)
        if bars.empty:
            return {"error": "No data loaded for the given symbols and date range"}

        bars["datetime"] = pd.to_datetime(bars["datetime"])
        bars = bars.sort_values("datetime").reset_index(drop=True)

        # 为每个 symbol 缓存完整历史数据
        for sym in symbols:
            self._history_cache[sym] = bars[bars["symbol"] == sym].copy()

        # 初始化目标权重
        self._init_weights(bars, symbols)

        self.strategy.on_init()

        grouped = bars.groupby("datetime")
        for timestamp, bar_group in grouped:
            # 再平衡检查
            if self._should_rebalance(timestamp):
                self._rebalance(bar_group)
                self._last_rebalance = timestamp

            context = Context(
                timestamp=timestamp,
                broker=self.broker,
                bar_group=bar_group,
                _all_bars=bars,
                _history_cache=self._history_cache,
            )
            self.strategy.on_bar(context, bar_group)
            self.broker.execute_orders(bar_group)

            self.equity_curve.append({
                "datetime": timestamp.isoformat(),
                "total_value": round(self.broker.total_value, 2),
                "cash": round(self.broker.cash, 2),
                "position_value": round(self.broker.position_value, 2),
            })

        self.strategy.on_exit()
        return self._build_result()

    def _init_weights(self, bars: pd.DataFrame, symbols: List[str]) -> None:
        """根据权重模式初始化目标权重"""
        if self.weight_mode == "equal":
            n = len(symbols)
            self._target_weights = {s: 1.0 / n for s in symbols} if n > 0 else {}
        elif self.weight_mode == "market_cap":
            # 用首日的收盘价 × 流通股本计算市值
            first_day = bars.groupby("symbol").first().reset_index()
            # 从 stock_basic 查询流通股本
            try:
                from server.api.core.timescale import get_ts_engine
                from sqlalchemy import text
                ts_engine = get_ts_engine()
                placeholders = ", ".join([f":s{i}" for i in range(len(symbols))])
                params = {f"s{i}": s for i, s in enumerate(symbols)}
                sql = text(f"""
                    SELECT symbol, float_shares FROM stock_basic
                    WHERE symbol IN ({placeholders}) AND is_active = TRUE
                """)
                with ts_engine.connect() as conn:
                    shares_df = pd.read_sql(sql, conn, params=params)
                merged = first_day.merge(shares_df, on="symbol", how="left")
                merged["float_shares"] = pd.to_numeric(merged["float_shares"], errors="coerce").fillna(0)
                mkt_caps = merged["close"] * merged["float_shares"]
                cap_map = dict(zip(merged["symbol"], mkt_caps))
                total_cap = sum(cap_map.values())
                self._target_weights = {
                    s: cap_map.get(s, 0) / total_cap for s in symbols
                } if total_cap > 0 else {s: 1.0 / len(symbols) for s in symbols}
            except Exception:
                n = len(symbols)
                self._target_weights = {s: 1.0 / n for s in symbols} if n > 0 else {}
        elif self.weight_mode == "custom":
            total = sum(self.custom_weights.values())
            self._target_weights = {
                s: self.custom_weights.get(s, 0) / total for s in symbols
            } if total > 0 else {s: 1.0 / len(symbols) for s in symbols}
        else:
            n = len(symbols)
            self._target_weights = {s: 1.0 / n for s in symbols} if n > 0 else {}

    def _should_rebalance(self, timestamp: datetime) -> bool:
        """检查是否到达再平衡时间点"""
        if self.rebalance_freq == "none" or not self.rebalance_freq:
            return False
        if self._last_rebalance is None:
            return True

        delta = timestamp - self._last_rebalance
        if self.rebalance_freq == "1W":
            return delta.days >= 7
        elif self.rebalance_freq == "1M":
            return delta.days >= 30
        elif self.rebalance_freq == "3M":
            return delta.days >= 90
        return False

    @staticmethod
    def _to_lot(amount: int) -> int:
        return max(0, (amount // PortfolioEngine.LOT_SIZE) * PortfolioEngine.LOT_SIZE)

    def _rebalance(self, bar_group: pd.DataFrame) -> None:
        """执行再平衡：将持仓调整回目标权重"""
        total_value = self.broker.total_value
        for symbol, target_w in self._target_weights.items():
            bar = bar_group[bar_group["symbol"] == symbol]
            if bar.empty:
                continue
            price = float(bar["close"].values[0])
            current_value = self.broker.positions.get(symbol, 0) * price
            target_value = total_value * target_w
            diff_value = target_value - current_value
            diff_amount = self._to_lot(int(diff_value / price))
            if diff_amount <= 0:
                continue
            if diff_value > 0:
                self.broker.submit_order(
                    Order(
                        symbol=symbol,
                        amount=diff_amount,
                        price=price,
                        order_type=OrderType.BUY,
                        timestamp=bar["datetime"].iloc[0] if "datetime" in bar.columns else pd.Timestamp.now(),
                    )
                )
            else:
                self.broker.submit_order(
                    Order(
                        symbol=symbol,
                        amount=diff_amount,
                        price=price,
                        order_type=OrderType.SELL,
                        timestamp=bar["datetime"].iloc[0] if "datetime" in bar.columns else pd.Timestamp.now(),
                    )
                )
