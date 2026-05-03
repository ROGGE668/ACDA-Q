"""
MA 趋势跟踪策略 - ACDA-Q 原生实现

基于 jzhu-trading 风格重写。核心逻辑与原版一致，去掉冗余包装类，
直接使用 ACDA-Q 引擎 API。

开仓条件（满足以下全部）：
  1. MA20 上升且价格在 MA20 上方
  2. MA5/MA10/MA20 多头排列（MA5 > MA10 > MA20）
  3. 金叉：MA5 上穿 MA10；或 恢复：prevMA10 > prevMA20 且 MA5 上穿 MA10
  4. λ = |MA5 - MA20| / MA20 在合理范围（≤15%）
  5. 成交量过滤（λ > 8% 时要求放量）

平仓条件（按优先级）：
  1. λ > 15%：熔断
  2. 达到止损线
  3. 达到止盈线
  4. 从最高点回撤超限
  5. MA5 下穿 MA10（死叉）
  6. 价格跌破 MA20
"""

# BaseStrategy 由沙箱环境自动注入，无需 import

class Strategy(BaseStrategy):
    def __init__(self, params=None):
        super().__init__(params)
        self._closes = {}    # symbol -> list[float]
        self._highs = {}     # symbol -> list[float]
        self._volumes = {}   # symbol -> list[float]
        self._entries = {}   # symbol -> { 'price': float, 'idx': int }

    def on_init(self):
        self._closes.clear()
        self._highs.clear()
        self._volumes.clear()
        self._entries.clear()

    # ─── helpers ─────────────────────────────────────────────

    @staticmethod
    def _sma(values: list, period: int) -> list:
        """简单移动平均。前 period-1 项为 None。"""
        result = [None] * len(values)
        n = len(values)
        if n < period:
            return result
        window_sum = sum(values[:period])
        result[period - 1] = window_sum / period
        for i in range(period, n):
            window_sum += values[i] - values[i - period]
            result[i] = window_sum / period
        return result

    @staticmethod
    def _pct(a: float, b: float) -> float:
        """(a - b) / b * 100"""
        return (a - b) / b * 100.0 if b != 0 else 0.0

    # ─── 开仓信号 ─────────────────────────────────────────

    def _should_open(self, sym: str, idx: int) -> tuple[bool, float, str]:
        """
        检查是否满足开仓条件。
        返回 (should_open, entry_price, reason)
        """
        closes = self._closes[sym]
        highs = self._highs[sym]
        volumes = self._volumes[sym]

        ma5_arr = self._sma(closes, 5)
        ma10_arr = self._sma(closes, 10)
        ma20_arr = self._sma(closes, 20)

        m5 = ma5_arr[idx]
        m10 = ma10_arr[idx]
        m20 = ma20_arr[idx]
        pm5 = ma5_arr[idx - 1]
        pm10 = ma10_arr[idx - 1]
        pm20 = ma20_arr[idx - 1]

        if any(v is None for v in [m5, m10, m20, pm5, pm10, pm20]):
            return False, 0, ""

        # 条件 1：MA20 必须上升，价格在 MA20 上方
        if m20 <= pm20 or closes[idx] <= m20:
            return False, 0, ""

        # 条件 2：多头排列检查
        signal_a = (pm5 <= pm10 and m5 > m10) and (m5 > m10 > m20)
        signal_b = (pm10 > pm20 and pm5 <= pm10) and (m5 > m10 > m20)

        if not signal_a and not signal_b:
            return False, 0, ""

        # 条件 3：λ 范围检查
        lam = abs(m5 - m20) / m20 if m20 != 0 else 0
        if lam > 0.15:
            return False, 0, ""

        if lam < 0.002 and signal_b:
            return False, 0, ""

        # 条件 4：成交量过滤
        if lam > 0.08:
            v5 = sum(volumes[idx - 4:idx + 1])
            avg_vol = v5 / 5.0
            vol_ratio = volumes[idx] / avg_vol if avg_vol > 0 else 0
            if vol_ratio < 1.2:
                return False, 0, ""

        # 条件 5：高 λ 时不允许 signal_b
        if lam > 0.12 and signal_b:
            return False, 0, ""

        sig_type = "\u91d1\u53c9" if signal_a else "\u6062\u590d"  # 金叉 / 恢复
        reason = f"{sig_type} \u03bb={lam:.3f}"
        return True, closes[idx], reason

    # ─── 平仓信号 ─────────────────────────────────────────

    def _should_close(self, sym: str, idx: int) -> tuple[bool, str]:
        """
        检查是否满足平仓条件。
        返回 (should_close, reason)
        """
        closes = self._closes[sym]
        highs = self._highs[sym]

        entry = self._entries.get(sym)
        if entry is None:
            return False, ""
        ep = entry["price"]
        ei = entry["idx"]

        ma5_arr = self._sma(closes, 5)
        ma10_arr = self._sma(closes, 10)
        ma20_arr = self._sma(closes, 20)

        m5 = ma5_arr[idx]
        m10 = ma10_arr[idx]
        m20 = ma20_arr[idx]
        pm5 = ma5_arr[idx - 1]
        pm10 = ma10_arr[idx - 1]

        if any(v is None for v in [m5, m10, m20, pm5, pm10]):
            return False, ""

        lam = abs(m5 - m20) / m20 if m20 != 0 else 0

        # 1. 熔断
        if lam > 0.15:
            return True, "\u7194\u65ad"  # 熔断

        pct = self._pct(closes[idx], ep)

        # 2. 止损
        sl = -2.0 if lam > 0.08 else (-3.0 if lam > 0.05 else -5.0)
        if pct <= sl:
            return True, "\u6b62\u635f"  # 止损

        # 3. 止盈
        if pct > 0:
            tp = 24.0 if lam > 0.08 else 38.0
            if pct >= tp:
                return True, "\u6b62\u76c8"  # 止盈

        # 4. 回撤
        mh = max(highs[ei:idx + 1])
        dd = self._pct(closes[idx], mh)
        dt = -2.0 if lam > 0.08 else -4.0
        if dd <= dt:
            return True, "\u56de\u64a4"  # 回撤

        # 5. 死叉
        if pm5 >= pm10 and m5 < m10:
            return True, "\u6b7b\u53c9"  # 死叉

        # 6. 破 MA20
        if closes[idx] < m20:
            return True, "\u7834MA20"

        return False, ""

    # ─── 主循环 ──────────────────────────────────────────

    def on_bar(self, context, bar_group):
        for _, bar in bar_group.iterrows():
            sym = bar["symbol"]
            close = float(bar["close"])
            high = float(bar["high"])
            vol = float(bar["volume"])

            # 累积数据
            self._closes.setdefault(sym, []).append(close)
            self._highs.setdefault(sym, []).append(high)
            self._volumes.setdefault(sym, []).append(vol)

            idx = len(self._closes[sym]) - 1
            if idx < 60:
                continue

            has_pos = context.positions.get(sym, 0) > 0
            has_entry = sym in self._entries

            # 修复状态不一致
            if not has_pos and has_entry:
                self._entries.pop(sym, None)
                has_entry = False
            if has_pos and not has_entry:
                # 有仓位但没有进场记录（外部来源），本策略不管理
                has_pos = False

            # 开仓
            if not has_pos and not has_entry:
                should_open, price, reason = self._should_open(sym, idx)
                if should_open:
                    context.buy(sym, percent=0.95)
                    self._entries[sym] = {"price": price, "idx": idx}

            # 平仓
            elif has_pos and has_entry:
                should_close, reason = self._should_close(sym, idx)
                if should_close:
                    context.sell(sym, percent=1.0)
                    self._entries.pop(sym, None)

    def on_exit(self):
        self._closes.clear()
        self._highs.clear()
        self._volumes.clear()
        self._entries.clear()
