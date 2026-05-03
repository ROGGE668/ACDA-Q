"""
事件驱动回测引擎核心。
兼容 Python 3.11.8
"""
from typing import List, Dict, Any, Optional
from datetime import datetime
from dataclasses import dataclass, field
import pandas as pd
from server.backtest.engine.broker import Broker, Order, OrderType
from server.backtest.engine.datafeed import DataFeed
from server.backtest.engine.strategy_base import BaseStrategy
from server.backtest.sandbox.executor import compile_strategy_code, load_strategy_class


@dataclass
class Context:
    """策略运行时上下文，封装了当前时间、持仓、下单接口和历史数据访问"""
    timestamp: datetime
    broker: Broker
    bar_group: pd.DataFrame
    _all_bars: pd.DataFrame = field(default_factory=pd.DataFrame, repr=False)
    _history_cache: Dict[str, pd.DataFrame] = field(default_factory=dict, repr=False)

    LOT_SIZE: int = 100  # A股最小交易单位

    # 快捷属性
    @property
    def cash(self) -> float:
        return self.broker.cash

    @property
    def total_value(self) -> float:
        return self.broker.total_value

    @property
    def positions(self) -> Dict[str, float]:
        return dict(self.broker.positions)

    def _to_lot(self, amount: float) -> int:
        """将股数向下取整为最小交易单位的整数倍"""
        return max(0, (int(amount) // self.LOT_SIZE) * self.LOT_SIZE)

    def history(self, symbol: str, field: str = "close", lookback: int = 20) -> pd.Series:
        """
        获取某标的最近 lookback 期的历史数据（懒加载，首次查询时从完整数据中过滤缓存）。
        用于计算均线、MACD 等技术指标。
        """
        df = self._history_cache.get(symbol)
        if df is None:
            if self._all_bars.empty:
                return pd.Series(dtype=float)
            df = self._all_bars[self._all_bars["symbol"] == symbol].copy()
            self._history_cache[symbol] = df
        if df.empty:
            return pd.Series(dtype=float)
        # 只取当前时间之前的数据
        mask = df["datetime"] < self.timestamp
        hist = df.loc[mask, ["datetime", field]]
        if hist.empty:
            return pd.Series(dtype=float)
        # 使用 datetime 作为索引，确保多标截面数据对齐
        return pd.Series(hist[field].values, index=pd.to_datetime(hist["datetime"])).iloc[-lookback:]

    def buy(self, symbol: str, amount: Optional[float] = None, percent: Optional[float] = None):
        """买入。amount: 股数；percent: 占总资产比例（0~1）"""
        bar = self.bar_group[self.bar_group['symbol'] == symbol]
        if bar.empty:
            return
        price = float(bar['close'].values[0])
        if percent is not None:
            amount = int((self.broker.total_value * percent) / price)
        amount = self._to_lot(amount or 0)
        if amount > 0:
            self.broker.submit_order(
                Order(
                    symbol=symbol,
                    amount=amount,
                    price=price,
                    order_type=OrderType.BUY,
                    timestamp=self.timestamp,
                )
            )

    def sell(self, symbol: str, amount: Optional[float] = None, percent: Optional[float] = None):
        """卖出。amount: 股数；percent: 占持仓比例（0~1）"""
        bar = self.bar_group[self.bar_group['symbol'] == symbol]
        if bar.empty:
            return
        price = float(bar['close'].values[0])
        if percent is not None:
            position = self.broker.positions.get(symbol, 0)
            amount = int(position * percent)
        amount = self._to_lot(amount or 0)
        if amount > 0:
            self.broker.submit_order(
                Order(
                    symbol=symbol,
                    amount=amount,
                    price=price,
                    order_type=OrderType.SELL,
                    timestamp=self.timestamp,
                )
            )

    def target_percent(self, symbol: str, target: float):
        """将某标的仓位调整到占总资产的 target 比例"""
        bar = self.bar_group[self.bar_group['symbol'] == symbol]
        if bar.empty:
            return
        price = float(bar['close'].values[0])
        current_value = self.broker.positions.get(symbol, 0) * price
        target_value = self.broker.total_value * target
        diff_value = target_value - current_value
        diff_amount = self._to_lot(abs(int(diff_value / price)))
        if diff_amount <= 0:
            return
        if diff_value > 0:
            self.buy(symbol, amount=diff_amount)
        else:
            self.sell(symbol, amount=diff_amount)

    # ---------- 向量化技术指标（常用热路径） ----------

    def sma(self, symbol: str, period: int = 20, field: str = "close") -> float:
        """简单移动平均。数据不足时返回 0.0。"""
        hist = self.history(symbol, field=field, lookback=period)
        return float(hist.mean()) if len(hist) >= period else 0.0

    def ema(self, symbol: str, period: int = 20, field: str = "close") -> float:
        """指数移动平均。数据不足时返回 0.0。"""
        hist = self.history(symbol, field=field, lookback=period * 2)
        if len(hist) < period:
            return 0.0
        return float(hist.ewm(span=period, adjust=False).mean().iloc[-1])

    def history_batch(
        self, symbols: List[str], field: str = "close", lookback: int = 20
    ) -> pd.DataFrame:
        """
        批量获取多只标的的历史数据，返回 DataFrame（index=日期, columns=symbols）。
        比多次调用 history() 更高效，适合多因子/截面策略。
        """
        rows = []
        for sym in symbols:
            hist = self.history(sym, field=field, lookback=lookback)
            if not hist.empty:
                rows.append(hist.rename(sym))
        if not rows:
            return pd.DataFrame()
        df = pd.concat(rows, axis=1)
        return df.sort_index()


class BacktestEngine:
    def __init__(
        self,
        initial_cash: float = 1_000_000.0,
        commission: float = 0.0003,
        slippage: float = 0.001,
        stamp_duty: float = 0.0005,
        transfer_fee: float = 0.00001,
    ):
        self.initial_cash = initial_cash
        self.commission = commission
        self.slippage = slippage
        self.broker = Broker(
            initial_cash, commission, slippage,
            stamp_duty=stamp_duty, transfer_fee=transfer_fee,
        )
        self.data_feed = DataFeed()
        self.strategy: Optional[BaseStrategy] = None
        self.equity_curve: List[Dict[str, Any]] = []
        self._symbols: List[str] = []

    def set_strategy(self, strategy_cls: type, params: Optional[Dict[str, Any]] = None):
        """设置策略类实例"""
        self.strategy = strategy_cls(params=params)
        self.strategy.set_broker(self.broker)

    def run(
        self,
        symbols: List[str],
        start_date: str,
        end_date: str,
        freq: str = "1d",
    ) -> Dict[str, Any]:
        """
        执行回测主循环。
        """
        self._symbols = symbols
        bars = self.data_feed.load_bars(symbols, start_date, end_date, freq)
        if bars.empty:
            return {"error": "No data loaded for the given symbols and date range"}

        bars["datetime"] = pd.to_datetime(bars["datetime"])
        bars = bars.sort_values("datetime").reset_index(drop=True)

        self.strategy.on_init()

        # 按时间分组，逐条推送
        grouped = bars.groupby("datetime")
        for timestamp, bar_group in grouped:
            context = Context(
                timestamp=timestamp,
                broker=self.broker,
                bar_group=bar_group,
                _all_bars=bars,
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

    def _build_result(self) -> Dict[str, Any]:
        from server.backtest.analyzers.performance import PerformanceAnalyzer

        analyzer = PerformanceAnalyzer()
        return analyzer.calculate(
            self.broker.trades, self.equity_curve, self.initial_cash
        )


def run_strategy_code(
    code: str,
    symbols: List[str],
    start_date: str,
    end_date: str,
    initial_cash: float = 1_000_000.0,
    params: Optional[Dict[str, Any]] = None,
    commission: float = 0.0003,
    slippage: float = 0.001,
    stamp_duty: float = 0.0005,
    transfer_fee: float = 0.00001,
) -> Dict[str, Any]:
    """
    高层封装：从策略代码字符串直接运行回测。
    包含安全编译、策略加载、回测执行、结果计算全流程。
    """
    # 1. 安全编译代码
    module = compile_strategy_code(code)

    # 2. 提取 Strategy 类
    strategy_cls = load_strategy_class(module)

    # 3. 初始化引擎并运行
    engine = BacktestEngine(
        initial_cash=initial_cash,
        commission=commission,
        slippage=slippage,
        stamp_duty=stamp_duty,
        transfer_fee=transfer_fee,
    )
    engine.set_strategy(strategy_cls, params=params)
    result = engine.run(symbols, start_date, end_date)

    # 4. 附加交易记录和净值曲线到结果中
    result["trades"] = [
        {
            "symbol": t.symbol,
            "amount": t.amount,
            "price": round(t.price, 2),
            "type": t.order_type.value,
            "timestamp": t.timestamp.isoformat(),
            "commission": round(t.commission, 4),
            "pnl": round(t.pnl, 2),
        }
        for t in engine.broker.trades
    ]
    result["equity_curve"] = engine.equity_curve
    result["positions"] = {
        sym: {"qty": qty, "last_price": round(engine.broker._last_price.get(sym, 0), 2)}
        for sym, qty in engine.broker.positions.items()
    }

    return result
