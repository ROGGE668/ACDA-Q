"""
RSI 策略示例
买入条件：RSI < 30（超卖）
卖出条件：RSI > 70（超买）
"""


class Strategy(BaseStrategy):
    def on_init(self):
        self.period = self.params.get("period", 14)
        self.oversold = self.params.get("oversold", 30)
        self.overbought = self.params.get("overbought", 70)
        self.holding = set()

    def _rsi(self, series, period):
        delta = series.diff()
        gain = delta.where(delta > 0, 0.0)
        loss = (-delta).where(delta < 0, 0.0)
        avg_gain = gain.rolling(window=period).mean()
        avg_loss = loss.rolling(window=period).mean()
        rs = avg_gain / avg_loss
        rsi = 100 - (100 / (1 + rs))
        return rsi

    def on_bar(self, context, bar_group):
        for _, row in bar_group.iterrows():
            symbol = row["symbol"]
            hist = context.history(symbol, field="close", lookback=self.period + 10)
            if len(hist) < self.period + 1:
                continue

            rsi = self._rsi(hist, self.period)
            current_rsi = rsi.iloc[-1]

            if current_rsi < self.oversold and symbol not in self.holding:
                context.buy(symbol, percent=0.25)
                self.holding.add(symbol)

            elif current_rsi > self.overbought and symbol in self.holding:
                context.sell(symbol, percent=1.0)
                self.holding.discard(symbol)
