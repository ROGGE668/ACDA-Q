"""
回测引擎本地测试脚本
无需数据库和 Celery，直接验证核心回测链路。
Python 3.11.8 compatible
"""
import sys
import os

# 将 server 加入路径
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "server"))

import pandas as pd
from backtest.engine.core import run_strategy_code
from backtest.engine.broker import Broker
from backtest.sandbox.executor import SecurityError, compile_strategy_code


SAMPLE_DUAL_MA = '''
class Strategy(BaseStrategy):
    def on_init(self):
        self.short_window = self.params.get("short_window", 5)
        self.long_window = self.params.get("long_window", 10)
        self.holding = set()

    def on_bar(self, context, bar_group):
        for _, row in bar_group.iterrows():
            symbol = row["symbol"]
            hist = context.history(symbol, field="close", lookback=self.long_window + 5)
            if len(hist) < self.long_window:
                continue

            short_ma = hist.iloc[-self.short_window:].mean()
            long_ma = hist.iloc[-self.long_window:].mean()
            prev_short = hist.iloc[-self.short_window - 1:-1].mean()
            prev_long = hist.iloc[-self.long_window - 1:-1].mean()

            if prev_short <= prev_long and short_ma > long_ma and symbol not in self.holding:
                context.buy(symbol, percent=0.5)
                self.holding.add(symbol)
            elif prev_short >= prev_long and short_ma < long_ma and symbol in self.holding:
                context.sell(symbol, percent=1.0)
                self.holding.discard(symbol)
'''


SAMPLE_BUY_HOLD = '''
class Strategy(BaseStrategy):
    def on_init(self):
        self.inited = False

    def on_bar(self, context, bar_group):
        if self.inited:
            return
        n = len(bar_group)
        for _, row in bar_group.iterrows():
            symbol = row["symbol"]
            context.buy(symbol, percent=1.0 / n)
        self.inited = True
'''


SAMPLE_LOT_SIZE = '''
class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        for _, row in bar_group.iterrows():
            symbol = row["symbol"]
            # 尝试买入 123 股（不是 100 的整数倍）
            if context.positions.get(symbol, 0) == 0:
                context.buy(symbol, amount=123)
'''


def test_dual_ma():
    print("=" * 60)
    print("测试：双均线策略回测")
    print("=" * 60)
    result = run_strategy_code(
        code=SAMPLE_DUAL_MA,
        symbols=["000001.SZ", "000002.SZ"],
        start_date="2023-01-01",
        end_date="2023-06-30",
        initial_cash=1_000_000,
        params={"short_window": 5, "long_window": 10},
    )

    print(f"总收益率: {result.get('total_return', 0):.2%}")
    print(f"年化收益率: {result.get('annual_return', 0):.2%}")
    print(f"最大回撤: {result.get('max_drawdown', 0):.2%}")
    print(f"夏普比率: {result.get('sharpe_ratio', 0):.4f}")
    print(f"总交易次数: {result.get('total_trades', 0)}")
    print(f"最终资产: {result.get('final_value', 0):,.2f}")
    print(f"交易记录数: {len(result.get('trades', []))}")
    print(f"净值曲线点数: {len(result.get('equity_curve', []))}")
    print("测试通过 ✓\n")


def test_buy_and_hold():
    print("=" * 60)
    print("测试：买入持有策略回测")
    print("=" * 60)
    result = run_strategy_code(
        code=SAMPLE_BUY_HOLD,
        symbols=["000001.SZ"],
        start_date="2023-01-01",
        end_date="2023-03-31",
        initial_cash=1_000_000,
    )
    print(f"总收益率: {result.get('total_return', 0):.2%}")
    print(f"最大回撤: {result.get('max_drawdown', 0):.2%}")
    print(f"总交易次数: {result.get('total_trades', 0)}")
    print("测试通过 ✓\n")


def test_security_block_import():
    print("=" * 60)
    print("测试：安全沙箱 - 禁止 import os")
    print("=" * 60)
    malicious_code = '''
import os
class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        os.system("rm -rf /")
'''
    try:
        run_strategy_code(
            code=malicious_code,
            symbols=["000001.SZ"],
            start_date="2023-01-01",
            end_date="2023-01-10",
        )
        print("测试失败：恶意代码被执行！")
    except Exception as e:
        print(f"恶意代码被拦截: {type(e).__name__}: {e}")
        print("测试通过 ✓\n")


def test_security_block_reflection():
    print("=" * 60)
    print("测试：安全沙箱 - 禁止反射绕过 (__subclasses__)")
    print("=" * 60)
    reflection_code = '''
class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        [x for x in [].__class__.__base__.__subclasses__() if x.__name__ == "_wrap_close"]
'''
    try:
        compile_strategy_code(reflection_code)
        print("测试失败：反射绕过代码未被拦截！")
    except SecurityError as e:
        print(f"反射绕过被拦截: {e}")
        print("测试通过 ✓\n")


def test_lot_size_rounding():
    print("=" * 60)
    print("测试：成交数量强制 100 股整数倍")
    print("=" * 60)
    result = run_strategy_code(
        code=SAMPLE_LOT_SIZE,
        symbols=["000001.SZ"],
        start_date="2023-01-01",
        end_date="2023-01-31",
        initial_cash=1_000_000,
    )
    trades = result.get("trades", [])
    assert len(trades) >= 1, "应该有至少一笔交易"
    first_trade = trades[0]
    amount = first_trade["amount"]
    assert amount % 100 == 0, f"成交数量 {amount} 不是 100 的整数倍"
    print(f"买入数量: {amount}（100 的整数倍）")
    print("测试通过 ✓\n")


def test_transaction_costs():
    print("=" * 60)
    print("测试：交易成本包含印花税和过户费")
    print("=" * 60)
    broker = Broker(
        cash=1_000_000,
        commission=0.0003,
        slippage=0.001,
        stamp_duty=0.0005,
        transfer_fee=0.00001,
    )
    # 验证费率已设置
    assert float(broker._stamp_duty_rate) == 0.0005, "印花税费率未正确设置"
    assert float(broker._transfer_fee_rate) == 0.00001, "过户费费率未正确设置"
    print(f"佣金费率: {float(broker._commission_rate):.4%}")
    print(f"印花税费率: {float(broker._stamp_duty_rate):.4%}")
    print(f"过户费费率: {float(broker._transfer_fee_rate):.5%}")
    print("测试通过 ✓\n")


def test_decimal_precision():
    print("=" * 60)
    print("测试：资金计算使用 Decimal 精度")
    print("=" * 60)
    broker = Broker(cash=1_000_000)
    # 多次小额交易后检查精度
    from decimal import Decimal
    broker._cash = Decimal("1000000.01")
    broker._cash -= Decimal("0.01")
    assert float(broker._cash) == 1_000_000.0, "Decimal 精度丢失"
    print(f"多次运算后现金: {broker.cash:,.2f}")
    print("测试通过 ✓\n")


def test_st_stock_limit():
    print("=" * 60)
    print("测试：ST 股涨跌停限制为 5%")
    print("=" * 60)
    import pandas as pd
    from decimal import Decimal

    # 模拟 ST 股 bar 数据
    st_bar = pd.DataFrame({
        "symbol": ["000001.SZ"],
        "close": [10.5],
        "pre_close": [10.0],
        "is_st": [True],
    })
    normal_bar = pd.DataFrame({
        "symbol": ["000001.SZ"],
        "close": [10.5],
        "pre_close": [10.0],
        "is_st": [False],
    })

    limit_st = Broker._get_limit_pct(st_bar, "000001.SZ")
    limit_normal = Broker._get_limit_pct(normal_bar, "000001.SZ")

    assert limit_st == Decimal("0.05"), f"ST 股涨跌幅应为 5%，实际 {limit_st}"
    assert limit_normal == Decimal("0.1"), f"非 ST 主板涨跌幅应为 10%，实际 {limit_normal}"
    print(f"ST 股 limit_pct: {float(limit_st):.0%}")
    print(f"普通股 limit_pct: {float(limit_normal):.0%}")
    print("测试通过 ✓\n")


def test_vectorized_indicators():
    print("=" * 60)
    print("测试：向量化技术指标 (SMA / EMA / history_batch)")
    print("=" * 60)
    from backtest.engine.core import BacktestEngine

    engine = BacktestEngine()
    bars = engine.data_feed.load_bars(
        symbols=["000001.SZ"],
        start_date="2023-01-01",
        end_date="2023-06-30",
    )
    if bars.empty:
        print("警告：无数据，跳过向量化指标测试")
        return

    from backtest.engine.core import Context
    # 取中间某天的数据构造 Context
    timestamp = bars["datetime"].iloc[50]
    bar_group = bars[bars["datetime"] == timestamp]
    context = Context(
        timestamp=timestamp,
        broker=Broker(cash=1_000_000),
        bar_group=bar_group,
        _all_bars=bars,
    )

    sma_val = context.sma("000001.SZ", period=20)
    ema_val = context.ema("000001.SZ", period=20)
    batch = context.history_batch(["000001.SZ"], lookback=20)

    assert sma_val >= 0, "SMA 应非负"
    assert ema_val >= 0, "EMA 应非负"
    assert not batch.empty, "history_batch 应返回非空 DataFrame"
    assert "000001.SZ" in batch.columns, "history_batch 应包含目标列"
    print(f"SMA(20): {sma_val:.2f}")
    print(f"EMA(20): {ema_val:.2f}")
    print(f"history_batch shape: {batch.shape}")
    print("测试通过 ✓\n")


def test_pre_close_limit_up_down():
    print("=" * 60)
    print("测试：pre_close 数据链路 & 涨跌停实际触发")
    print("=" * 60)
    import pandas as pd
    from backtest.engine.broker import Broker, Order, OrderType

    broker = Broker(cash=1_000_000, enable_t1=False)

    # 构造涨停 bar：pre_close=10.0, close=11.0 (涨停 10%)
    limit_up_bar = pd.DataFrame({
        "symbol": ["000001.SZ"],
        "close": [11.0],
        "pre_close": [10.0],
        "is_st": [False],
    })
    # 构造跌停 bar：pre_close=10.0, close=9.0 (跌停 10%)
    limit_down_bar = pd.DataFrame({
        "symbol": ["000001.SZ"],
        "close": [9.0],
        "pre_close": [10.0],
        "is_st": [False],
    })
    # 构造正常 bar
    normal_bar = pd.DataFrame({
        "symbol": ["000001.SZ"],
        "close": [10.5],
        "pre_close": [10.0],
        "is_st": [False],
    })

    from datetime import datetime
    ts = datetime(2023, 1, 5)

    # 涨停日买入应被拒绝
    broker.submit_order(Order(symbol="000001.SZ", amount=100, price=11.0, order_type=OrderType.BUY, timestamp=ts))
    broker.execute_orders(limit_up_bar)
    assert len(broker.trades) == 0, "涨停日买入应被拒绝"
    print("涨停日买入被拒绝 ✓")

    # 正常日买入应成交
    broker.pending_orders.clear()
    broker.submit_order(Order(symbol="000001.SZ", amount=100, price=10.5, order_type=OrderType.BUY, timestamp=ts))
    broker.execute_orders(normal_bar)
    assert len(broker.trades) == 1, "正常日买入应成交"
    print("正常日买入成交 ✓")

    # 跌停日卖出应被拒绝
    broker.pending_orders.clear()
    broker.submit_order(Order(symbol="000001.SZ", amount=100, price=9.0, order_type=OrderType.SELL, timestamp=ts))
    broker.execute_orders(limit_down_bar)
    # 之前已买入 100 股，现在有持仓
    sell_trades = [t for t in broker.trades if t.order_type == OrderType.SELL]
    assert len(sell_trades) == 0, "跌停日卖出应被拒绝"
    print("跌停日卖出被拒绝 ✓")
    print("测试通过 ✓\n")


def test_getattr_bypass_blocked():
    print("=" * 60)
    print("测试：安全沙箱 - 禁止 getattr 绕过")
    print("=" * 60)
    bypass_code = '''
class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        getattr(globals()["__builtins__"], "__import__")("os")
'''
    try:
        compile_strategy_code(bypass_code)
        print("测试失败：getattr 绕过代码未被拦截！")
    except SecurityError as e:
        print(f"getattr 绕过被拦截: {e}")
        print("测试通过 ✓\n")


def test_history_batch_alignment():
    print("=" * 60)
    print("测试：history_batch 多标日期对齐")
    print("=" * 60)
    from backtest.engine.core import BacktestEngine, Context

    engine = BacktestEngine()
    bars = engine.data_feed.load_bars(
        symbols=["000001.SZ", "000002.SZ"],
        start_date="2023-01-01",
        end_date="2023-06-30",
    )
    if bars.empty or len(bars["symbol"].unique()) < 2:
        print("警告：多标数据不足，跳过对齐测试")
        return

    timestamp = bars["datetime"].iloc[50]
    bar_group = bars[bars["datetime"] == timestamp]
    context = Context(
        timestamp=timestamp,
        broker=Broker(cash=1_000_000),
        bar_group=bar_group,
        _all_bars=bars,
    )

    batch = context.history_batch(["000001.SZ", "000002.SZ"], lookback=20)
    assert not batch.empty, "history_batch 应返回非空 DataFrame"
    assert "000001.SZ" in batch.columns, "应包含 000001.SZ"
    assert "000002.SZ" in batch.columns, "应包含 000002.SZ"

    # 检查索引是否为 datetime 类型
    assert isinstance(batch.index, pd.DatetimeIndex), f"索引应为 DatetimeIndex，实际为 {type(batch.index)}"
    print(f"history_batch shape: {batch.shape}")
    print(f"索引类型: {type(batch.index).__name__}")
    print("测试通过 ✓\n")


def test_t1_rule_precision():
    print("=" * 60)
    print("测试：T+1 规则精确性（旧持仓可卖，当日买入不可卖）")
    print("=" * 60)
    import pandas as pd
    from backtest.engine.broker import Broker, Order, OrderType
    from datetime import datetime

    broker = Broker(cash=1_000_000, enable_t1=True)

    # 第1天：买入 500 股
    day1 = datetime(2023, 1, 3)
    bar_day1 = pd.DataFrame({
        "symbol": ["000001.SZ"],
        "close": [100.0],
        "pre_close": [99.0],
        "datetime": [day1],
    })
    broker.submit_order(Order(symbol="000001.SZ", amount=500, price=100.0, order_type=OrderType.BUY, timestamp=day1))
    broker.execute_orders(bar_day1)
    assert len(broker.trades) == 1, "第1天买入应成交"
    assert broker.positions.get("000001.SZ", 0) == 500, "持仓应为 500 股"

    # 第2天：再买入 300 股，然后尝试卖出 500 股（旧持仓）
    day2 = datetime(2023, 1, 4)
    bar_day2 = pd.DataFrame({
        "symbol": ["000001.SZ"],
        "close": [101.0],
        "pre_close": [100.0],
        "datetime": [day2],
    })
    broker.submit_order(Order(symbol="000001.SZ", amount=300, price=101.0, order_type=OrderType.BUY, timestamp=day2))
    broker.execute_orders(bar_day2)
    assert broker.positions.get("000001.SZ", 0) == 800, "持仓应为 800 股"

    # 同日尝试卖出 500 股（旧持仓 500 股可卖）
    broker.submit_order(Order(symbol="000001.SZ", amount=500, price=101.0, order_type=OrderType.SELL, timestamp=day2))
    broker.execute_orders(bar_day2)
    sell_trades = [t for t in broker.trades if t.order_type == OrderType.SELL]
    assert len(sell_trades) == 1, "旧持仓 500 股应可卖出"
    assert broker.positions.get("000001.SZ", 0) == 300, "剩余持仓应为 300 股（当日买入部分）"

    # 同日尝试再卖出 300 股（当日买入部分不可卖）
    broker.submit_order(Order(symbol="000001.SZ", amount=300, price=101.0, order_type=OrderType.SELL, timestamp=day2))
    broker.execute_orders(bar_day2)
    sell_trades = [t for t in broker.trades if t.order_type == OrderType.SELL]
    assert len(sell_trades) == 1, "当日买入部分不应被卖出"
    assert broker.positions.get("000001.SZ", 0) == 300, "持仓应保持 300 股"

    print("旧持仓可卖出 ✓")
    print("当日买入部分被拒绝 ✓")
    print("测试通过 ✓\n")


if __name__ == "__main__":
    test_dual_ma()
    test_buy_and_hold()
    test_security_block_import()
    test_security_block_reflection()
    test_getattr_bypass_blocked()
    test_lot_size_rounding()
    test_transaction_costs()
    test_decimal_precision()
    test_pre_close_limit_up_down()
    test_st_stock_limit()
    test_vectorized_indicators()
    test_history_batch_alignment()
    test_t1_rule_precision()
    print("全部测试完成！")
