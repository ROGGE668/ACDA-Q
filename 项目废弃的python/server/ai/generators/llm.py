import os
import re
import ast
from typing import Optional, Tuple
import httpx
from server.api.core.config import get_settings
from server.backtest.sandbox.executor import ALLOWED_MODULES, BLACKLISTED_NAMES

settings = get_settings()

_MOCK_TEMPLATE_PATH = os.path.join(os.path.dirname(__file__), "../../backtest/examples/dual_ma.py")
try:
    with open(_MOCK_TEMPLATE_PATH, "r", encoding="utf-8") as _f:
        _FALLBACK_CODE = _f.read().strip()
except FileNotFoundError:
    _FALLBACK_CODE = "class Strategy(BaseStrategy):\n    def on_bar(self, context, bar_group):\n        pass\n"

FEW_SHOT_EXAMPLES = """
示例1 - 双均线策略：
```python
class Strategy(BaseStrategy):
    def on_init(self):
        self.fast = self.params.get("fast", 10)
        self.slow = self.params.get("slow", 30)

    def on_bar(self, context, bar_group):
        for symbol in bar_group["symbol"].unique():
            hist = context.history(symbol, field="close", lookback=self.slow + 5)
            if len(hist) < self.slow:
                continue
            ma_fast = hist.rolling(self.fast).mean().iloc[-1]
            ma_slow = hist.rolling(self.slow).mean().iloc[-1]
            if ma_fast > ma_slow and context.positions.get(symbol, 0) == 0:
                context.buy(symbol, percent=0.2)
            elif ma_fast < ma_slow and context.positions.get(symbol, 0) > 0:
                context.sell(symbol, percent=1.0)
```

示例2 - RSI策略：
```python
import numpy as np

class Strategy(BaseStrategy):
    def on_init(self):
        self.period = self.params.get("period", 14)
        self.overbought = self.params.get("overbought", 70)
        self.oversold = self.params.get("oversold", 30)

    def on_bar(self, context, bar_group):
        for symbol in bar_group["symbol"].unique():
            hist = context.history(symbol, field="close", lookback=self.period + 5)
            if len(hist) < self.period:
                continue
            delta = hist.diff()
            gain = delta.where(delta > 0, 0).rolling(self.period).mean().iloc[-1]
            loss = (-delta.where(delta < 0, 0)).rolling(self.period).mean().iloc[-1]
            rs = gain / loss if loss != 0 else 0
            rsi = 100 - (100 / (1 + rs))
            if rsi < self.oversold and context.positions.get(symbol, 0) == 0:
                context.buy(symbol, percent=0.2)
            elif rsi > self.overbought and context.positions.get(symbol, 0) > 0:
                context.sell(symbol, percent=1.0)
```
"""

SYSTEM_PROMPT = f"""你是一位专业的A股量化策略工程师。请根据用户需求，编写一个符合平台API的Python策略类。

平台约束：
- 类名必须为 Strategy，继承自 BaseStrategy
- 必须实现 on_bar(self, context, bar_group) 方法
- 可选实现 on_init(self) 用于初始化参数（在 self.params 中读取）
- 使用 context.buy(symbol, amount) / context.sell(symbol, amount) 下单，或用 percent 参数如 context.buy(symbol, percent=0.5)
- 使用 context.history(symbol, field="close", lookback=20) 获取历史数据，返回 pandas Series
- 可用库：pandas, numpy
- 策略参数通过 self.params 字典访问，务必在 on_init 中定义默认值
- bar_group 是 pandas DataFrame，包含 columns: symbol, open, high, low, close, volume

安全限制：
- 禁止 import os, sys, subprocess, socket, requests, urllib
- 禁止文件读写（open/read/write）、网络请求、eval/exec
- 仅输出纯Python代码，不要包含任何解释或markdown代码块标记

{FEW_SHOT_EXAMPLES}
"""

async def generate_strategy_code(prompt: str, model: Optional[str] = None) -> Tuple[str, str, int]:
    model = model or settings.DEEPSEEK_MODEL
    api_key = settings.DEEPSEEK_API_KEY
    base_url = settings.DEEPSEEK_BASE_URL.rstrip("/")

    if not api_key:
        # Fallback for development without API key
        return _FALLBACK_CODE, "mock", 0

    async with httpx.AsyncClient(timeout=settings.AI_CODE_TIMEOUT) as client:
        response = await client.post(
            f"{base_url}/v1/chat/completions",
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
            },
            json={
                "model": model,
                "messages": [
                    {"role": "system", "content": SYSTEM_PROMPT},
                    {"role": "user", "content": f"用户需求：\n{prompt}\n\n请输出完整可运行的Python代码，不要包含任何解释文字。"},
                ],
                "max_tokens": settings.AI_MAX_TOKENS,
                "temperature": 0.2,
            },
        )
        response.raise_for_status()
        data = response.json()
        raw = data["choices"][0]["message"]["content"]
        tokens = data.get("usage", {}).get("total_tokens", 0)

    code = _extract_code(raw)
    _validate_code(code)
    return code, model, tokens

def _extract_code(text: str) -> str:
    # Remove markdown code fences
    text = re.sub(r"```python\n?|```\n?", "", text)
    return text.strip()


def _validate_code(code: str):
    try:
        tree = ast.parse(code)
    except SyntaxError as e:
        raise ValueError(f"Syntax error in generated code: {e}")

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name.split(".")[0] not in ALLOWED_MODULES:
                    raise ValueError(f"Disallowed import: {alias.name}")
        elif isinstance(node, ast.ImportFrom):
            if node.module and node.module.split(".")[0] not in ALLOWED_MODULES:
                raise ValueError(f"Disallowed import from: {node.module}")
        elif isinstance(node, ast.Name):
            if node.id in BLACKLISTED_NAMES:
                raise ValueError(f"Disallowed name usage: {node.id}")
        elif isinstance(node, ast.Call):
            if isinstance(node.func, ast.Name) and node.func.id in BLACKLISTED_NAMES:
                raise ValueError(f"Disallowed function call: {node.func.id}")
        elif isinstance(node, ast.With):
            # 禁止 with open(...) as f: 等文件IO
            for item in node.items:
                ctx = item.context_expr
                if isinstance(ctx, ast.Call):
                    func = ctx.func
                    if isinstance(func, ast.Name) and func.id == "open":
                        raise ValueError("Disallowed file operation: open() in with statement")
                    if isinstance(func, ast.Attribute) and func.attr in ("read", "write", "readlines"):
                        raise ValueError(f"Disallowed file operation: {func.attr}")
        elif isinstance(node, ast.Attribute):
            # 禁止 .read() / .write() 等文件方法调用
            if node.attr in ("read", "write", "readlines", "writelines"):
                raise ValueError(f"Disallowed file operation method: {node.attr}")
