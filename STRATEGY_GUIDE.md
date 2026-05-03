# ACDA-Q 策略编写指南

本文档面向需要在 ACDA-Q 量化平台上编写交易策略的用户，涵盖策略结构、API 说明、完整示例及安全限制。

---

## 1. 快速入门

一个最简策略只需满足两个条件：**类名为 `Strategy`**，**继承 `BaseStrategy`**，并实现 `on_bar` 方法。

```python
class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        pass
```

运行后引擎会按时间顺序逐条推送行情，你在 `on_bar` 里决定买或卖即可。

---

## 2. 策略结构

### 2.1 类定义

```python
class Strategy(BaseStrategy):
    ...
```

- 类名**必须**是 `Strategy`，不可更改。
- `BaseStrategy` 由平台自动注入，**无需 `import`**。

### 2.2 生命周期方法

| 方法 | 调用时机 | 说明 |
|------|---------|------|
| `on_init(self)` | 回测开始前，调用 **1 次** | 初始化参数、状态变量 |
| `on_bar(self, context, bar_group)` | 每个时间周期（默认日线） | 核心交易逻辑 |
| `on_exit(self)` | 回测结束后，调用 **1 次** | 清理资源、输出统计 |

三个方法均为可选，但 `on_bar` 不实现则策略不会有任何操作。

### 2.3 参数读取

策略参数通过 `self.params` 字典传入，建议在 `on_init` 中读取并设置默认值：

```python
def on_init(self):
    self.fast = self.params.get("fast", 10)
    self.slow = self.params.get("slow", 30)
```

前端参数面板会根据代码自动提取这些定义。

---

## 3. Context 接口（交易与查询）

`on_bar` 的第一个参数 `context` 是策略与回测引擎交互的唯一入口。

### 3.1 下单方法

#### `context.buy(symbol, amount=None, percent=None)`

买入指定标的。

- `amount`：买入股数（整数）。
- `percent`：占总资产的比例（0~1），与 `amount` 二选一。

```python
# 买入 1000 股
ccontext.buy("600519", amount=1000)

# 用总资产的 20% 买入
context.buy("600519", percent=0.2)
```

#### `context.sell(symbol, amount=None, percent=None)`

卖出指定标的。

- `amount`：卖出股数。
- `percent`：占当前持仓的比例（0~1）。

```python
# 卖出全部持仓
context.sell("600519", percent=1.0)

# 卖出 500 股
context.sell("600519", amount=500)
```

#### `context.target_percent(symbol, target)`

将某标的仓位调整至占总资产的 `target` 比例（0~1）。不足则买入，超出则卖出。

```python
# 将茅台仓位调整到总资产的 30%
context.target_percent("600519", 0.3)
```

### 3.2 账户信息

| 属性 | 类型 | 说明 |
|------|------|------|
| `context.cash` | `float` | 当前可用现金 |
| `context.total_value` | `float` | 总资产 = 现金 + 持仓市值 |
| `context.positions` | `dict` | 当前持仓，`{symbol: qty}` |

```python
# 查看当前现金和总资产
cash = context.cash
total = context.total_value

# 检查是否持有某股票
has_position = context.positions.get("600519", 0) > 0
```

### 3.3 历史数据查询

#### `context.history(symbol, field="close", lookback=20)`

获取某标的最近 `lookback` 条历史数据，返回 `pandas.Series`。

- `field`：可选 `"open"`、`"high"`、`"low"`、`"close"`、`"volume"`。
- `lookback`：回查条数，默认 20。

```python
# 获取最近 30 日收盘价
closes = context.history("600519", field="close", lookback=30)

# 计算 10 日均线
ma10 = closes.rolling(10).mean().iloc[-1]
```

**注意**：返回的数据不包含当前 `bar`，仅包含历史数据。如果历史不足 `lookback` 条，返回实际可用的全部数据。

---

## 4. bar_group 数据结构

`on_bar` 的第二个参数 `bar_group` 是一个 `pandas.DataFrame`，包含当前时间周期内所有标的的行情数据。

### 4.1 列说明

| 列名 | 类型 | 说明 |
|------|------|------|
| `symbol` | `str` | 股票代码 |
| `datetime` | `datetime` | 当前时间戳 |
| `open` | `float` | 开盘价 |
| `high` | `float` | 最高价 |
| `low` | `float` | 最低价 |
| `close` | `float` | 收盘价 |
| `volume` | `float` | 成交量 |
| `pre_close` | `float` | 昨收价（用于涨跌停判断） |

### 4.2 遍历方式

单股回测时 `bar_group` 只有一行，组合/全市场回测时有多行。推荐按行遍历：

```python
def on_bar(self, context, bar_group):
    for _, row in bar_group.iterrows():
        symbol = row["symbol"]
        close = float(row["close"])
        volume = float(row["volume"])
        # ... 交易逻辑
```

或用 `symbol` 列去重获取所有标的：

```python
symbols = bar_group["symbol"].unique()
```

---

## 5. 交易规则说明

回测引擎已内置 A 股核心交易规则，策略代码中**不需要手动处理**以下内容：

- **T+1 限制**：当日买入的股票当日不可卖出。
- **涨跌停限制**：涨停时无法买入，跌停时无法卖出（主板 10%，科创板/创业板 20%）。
- **滑点**：买入按 `close * (1 + slippage)` 成交，卖出按 `close * (1 - slippage)` 成交。
- **佣金**：按成交金额乘以佣金率扣除（默认 0.03%）。
- **停牌处理**：停牌日无行情数据，策略不会收到该标的的 bar。
- **复权**：回测全程使用前复权价格计算，成交记录记录实际价格。

---

## 6. 完整示例

### 6.1 买入持有（Buy & Hold）

回测开始时按等权买入所有标的并持有至结束。

```python
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
```

### 6.2 双均线策略（Dual MA）

**买入**：短期均线上穿长期均线（金叉）
**卖出**：短期均线下穿长期均线（死叉）

```python
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
```

### 6.3 RSI 策略

**买入**：RSI < 30（超卖）
**卖出**：RSI > 70（超买）

```python
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
        return 100 - (100 / (1 + rs))

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
```

### 6.4 MA 趋势跟踪策略（高级示例）

结合多头排列、金叉/恢复信号、成交量过滤、动态止损止盈的复合策略。

```python
class Strategy(BaseStrategy):
    def __init__(self, params=None):
        super().__init__(params)
        self._closes = {}
        self._highs = {}
        self._volumes = {}
        self._entries = {}

    def on_init(self):
        self._closes.clear()
        self._highs.clear()
        self._volumes.clear()
        self._entries.clear()

    @staticmethod
    def _sma(values, period):
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

    def _should_open(self, sym, idx):
        closes = self._closes[sym]
        volumes = self._volumes[sym]

        ma5 = self._sma(closes, 5)
        ma10 = self._sma(closes, 10)
        ma20 = self._sma(closes, 20)

        m5, m10, m20 = ma5[idx], ma10[idx], ma20[idx]
        pm5, pm10, pm20 = ma5[idx - 1], ma10[idx - 1], ma20[idx - 1]

        if any(v is None for v in [m5, m10, m20, pm5, pm10, pm20]):
            return False, 0, ""

        # MA20 上升且价格在 MA20 上方
        if m20 <= pm20 or closes[idx] <= m20:
            return False, 0, ""

        # 多头排列 + 金叉/恢复
        signal_a = (pm5 <= pm10 and m5 > m10) and (m5 > m10 > m20)
        signal_b = (pm10 > pm20 and pm5 <= pm10) and (m5 > m10 > m20)
        if not signal_a and not signal_b:
            return False, 0, ""

        # lambda 范围检查
        lam = abs(m5 - m20) / m20 if m20 != 0 else 0
        if lam > 0.15:
            return False, 0, ""

        # 成交量过滤
        if lam > 0.08:
            v5 = sum(volumes[idx - 4:idx + 1]) / 5.0
            if volumes[idx] / v5 < 1.2:
                return False, 0, ""

        reason = f"金叉 λ={lam:.3f}" if signal_a else f"恢复 λ={lam:.3f}"
        return True, closes[idx], reason

    def _should_close(self, sym, idx):
        closes = self._closes[sym]
        highs = self._highs[sym]
        entry = self._entries.get(sym)
        if entry is None:
            return False, ""

        ep, ei = entry["price"], entry["idx"]
        ma5 = self._sma(closes, 5)
        ma10 = self._sma(closes, 10)
        ma20 = self._sma(closes, 20)
        m5, m10, m20 = ma5[idx], ma10[idx], ma20[idx]
        pm5, pm10 = ma5[idx - 1], ma10[idx - 1]

        if any(v is None for v in [m5, m10, m20, pm5, pm10]):
            return False, ""

        lam = abs(m5 - m20) / m20 if m20 != 0 else 0
        pct = (closes[idx] - ep) / ep * 100

        if lam > 0.15:
            return True, "熔断"
        if pct <= (-2.0 if lam > 0.08 else -5.0):
            return True, "止损"
        if pct >= (24.0 if lam > 0.08 else 38.0):
            return True, "止盈"

        mh = max(highs[ei:idx + 1])
        dd = (closes[idx] - mh) / mh * 100
        if dd <= (-2.0 if lam > 0.08 else -4.0):
            return True, "回撤"

        if pm5 >= pm10 and m5 < m10:
            return True, "死叉"
        if closes[idx] < m20:
            return True, "破MA20"

        return False, ""

    def on_bar(self, context, bar_group):
        for _, bar in bar_group.iterrows():
            sym = bar["symbol"]
            close = float(bar["close"])
            high = float(bar["high"])
            vol = float(bar["volume"])

            self._closes.setdefault(sym, []).append(close)
            self._highs.setdefault(sym, []).append(high)
            self._volumes.setdefault(sym, []).append(vol)

            idx = len(self._closes[sym]) - 1
            if idx < 60:
                continue

            has_pos = context.positions.get(sym, 0) > 0
            has_entry = sym in self._entries

            if not has_pos and has_entry:
                self._entries.pop(sym, None)
                has_entry = False

            if not has_pos and not has_entry:
                should_open, price, reason = self._should_open(sym, idx)
                if should_open:
                    context.buy(sym, percent=0.95)
                    self._entries[sym] = {"price": price, "idx": idx}

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
```

---

## 7. 安全限制

为保护平台安全，策略代码在受限环境中运行，以下操作**被禁止**：

### 7.1 禁止使用的库

`os`、`sys`、`subprocess`、`socket`、`requests`、`urllib`、`http`、`pathlib`、`shutil`、`pickle`、`threading`、`multiprocessing` 等。

### 7.2 禁止的操作

- 文件读写（`open`、`read`、`write`）
- 网络请求
- `eval`、`exec`、`compile`
- `__import__`

### 7.3 允许使用的库

`numpy`（别名为 `np`）、`pandas`（别名为 `pd`）、`math`、`random`、`statistics`、`datetime`、`time`、`typing`、`collections`、`itertools`、`functools`。

无需在代码中 `import`，常用库已预加载到命名空间。

---

## 8. 常见问题

**Q：为什么 `BaseStrategy` 不需要 import？**

A：平台在运行策略前会自动将 `BaseStrategy` 注入到代码环境中，因此直接继承即可。

**Q：`context.history` 返回的数据包含当前 bar 吗？**

A：不包含。返回的是严格在当前时间戳之前的历史数据，确保不会出现未来信息泄露。

**Q：可以同时买卖多只股票吗？**

A：可以。`bar_group` 在组合回测和全市场扫描模式下会包含多行数据，遍历处理即可。单股回测时只有一行。

**Q：策略出错如何排查？**

A：平台会在提交策略时进行语法校验和安全扫描。回测失败时可在回测详情页查看错误信息。常见原因包括：
- 类名不是 `Strategy`
- 缺少 `on_bar` 方法
- 使用了禁止的库或函数
- `context.history` 返回空 Series 导致计算报错

**Q：如何调试策略逻辑？**

A：建议先在本地用少量数据验证算法正确性，再粘贴到平台。平台暂不支持策略内打印日志，可通过控制持仓标记（如 `self.holding`）和参数调整来验证逻辑路径。

---

*文档版本：v1.0*  
*适用平台：ACDA-Q*
