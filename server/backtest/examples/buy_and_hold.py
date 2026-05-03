"""
买入持有策略示例
回测开始时满仓买入，持有到结束
"""


class Strategy(BaseStrategy):
    def on_init(self):
        self.inited = False

    def on_bar(self, context, bar_group):
        if self.inited:
            return
        for _, row in bar_group.iterrows():
            symbol = row["symbol"]
            context.buy(symbol, percent=1.0 / len(bar_group))
        self.inited = True
