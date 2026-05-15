"""
双均线策略示例
买入条件：短期均线上穿长期均线（金叉）
卖出条件：短期均线下穿长期均线（死叉）
"""


class Strategy(BaseStrategy):
    def on_init(self):
        self.short_window = self.params.get("short_window", 10)
        self.long_window = self.params.get("long_window", 30)
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

            # 金叉买入
            if prev_short <= prev_long and short_ma > long_ma and symbol not in self.holding:
                context.buy(symbol, percent=0.2)
                self.holding.add(symbol)

            # 死叉卖出
            elif prev_short >= prev_long and short_ma < long_ma and symbol in self.holding:
                context.sell(symbol, percent=1.0)
                self.holding.discard(symbol)
