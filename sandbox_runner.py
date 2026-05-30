#!/usr/bin/env python3
"""
ACDA-Q 沙箱回测 Runner
======================
从 stdin 接收 JSON，执行策略回测，结果写到 stdout。

协议:
  stdin:  {"code": "...", "symbols": ["000001"], "start_date": "...", "end_date": "...", "initial_cash": 1000000.0}
  stdout: {"status": "success", "result": {...}, "error": null, "traceback": null}
"""

import sys
import json
import os
import traceback
import textwrap
from datetime import datetime

import pandas as pd

# 沙箱安全：限制 Python builtins，防止用户代码执行危险操作
import builtins as _builtins
# 受限 pandas 代理：只暴露 DataFrame 和 Series，阻止通过 pandas 内部路径绕过沙箱
class _SafePandas:
    """受限的 pandas 代理，只暴露安全的 API"""
    DataFrame = pd.DataFrame
    Series = pd.Series
    Timestamp = pd.Timestamp
    NaT = pd.NaT
    concat = pd.concat
    merge = pd.merge
    to_datetime = pd.to_datetime
    # 不暴露 read_sql（防止直接数据库访问）
    # 不暴露 io.common, core, _libs 等内部模块

_SAFE_PANDAS = _SafePandas()

_SAFE_BUILTINS = {
    name: getattr(_builtins, name)
    for name in [
        'abs', 'all', 'any', 'bool', 'callable', 'chr', '__build_class__', 'dir', 'divmod',
        'enumerate', 'filter', 'float', 'format', 'getattr', 'hasattr',
        'hash', 'hex', 'id', 'int', 'isinstance', 'issubclass', 'iter',
        'len', 'list', 'map', 'max', 'min', 'next', 'oct', 'ord', 'pow',
        'print', 'range', 'repr', 'reversed', 'round', 'set', 'slice',
        'sorted', 'str', 'sum', 'super', 'tuple', 'type', 'zip',
    ]
    if hasattr(_builtins, name)
}

# 优先从 ACDA_Q__TIMESCALE_DATABASE_URL 解析，否则用单独的环境变量
_ts_url = os.environ.get("ACDA_Q__TIMESCALE_DATABASE_URL", "")
if _ts_url:
    # 格式: postgresql://user:pass@host:port/dbname
    from urllib.parse import urlparse
    _parsed = urlparse(_ts_url)
    DB_HOST = _parsed.hostname or "127.0.0.1"
    DB_PORT = _parsed.port or 5433
    DB_USER = _parsed.username or "quant"
    DB_PASSWORD = _parsed.password or ""
    DB_NAME = _parsed.path.lstrip("/") or "quant_market"
else:
    DB_HOST = os.environ.get("ACDA_Q__DB_HOST", "127.0.0.1")
    DB_PORT = int(os.environ.get("ACDA_Q__DB_PORT", "5433"))
    DB_USER = os.environ.get("ACDA_Q__DB_USER", "quant")
    DB_PASSWORD = os.environ.get("ACDA_Q__DB_PASS", "")
    DB_NAME = os.environ.get("ACDA_Q__DB_NAME", "quant_market")

# 全局 DB 引擎复用（进程池模式下避免每次重建连接）
_engine = None

def _get_engine():
    global _engine
    if _engine is None:
        from sqlalchemy import create_engine
        url = f"postgresql://{DB_USER}:{DB_PASSWORD}@{DB_HOST}:{DB_PORT}/{DB_NAME}"
        _engine = create_engine(url, pool_size=2, max_overflow=0, pool_pre_ping=True)
    return _engine


# K 线数据缓存（进程内，避免重复查询 TimescaleDB 4s+ 规划开销）
_bars_cache = {}
_bars_cache_max = 50

def _cache_key(symbols, start_date, end_date, period="1d"):
    return (tuple(sorted(symbols)), start_date, end_date, period)

# 进程启动时预热 DB 引擎 + prepared statement 缓存
def _warmup():
    try:
        engine = _get_engine()
        from sqlalchemy import text
        with engine.connect() as conn:
            # 预热 daily_bars 查询（匹配 load_daily_bars 的实际 SQL 模式）
            conn.execute(text(
                "SELECT symbol, datetime, open, high, low, close, volume, amount "
                "FROM daily_bars WHERE symbol = ANY(:symbols) "
                "AND datetime >= :start::date AND datetime < :end::date + interval '1 day' "
                "ORDER BY datetime, symbol"
            ), {"symbols": ["warmup"], "start": "2000-01-01", "end": "2000-01-02"})
    except Exception:
        pass

_warmup()


def load_daily_bars(symbols, start_date, end_date):
    key = _cache_key(symbols, start_date, end_date, "1d")
    if key in _bars_cache:
        return _bars_cache[key].copy()
    from sqlalchemy import text
    engine = _get_engine()
    placeholders = ",".join(f":s{i}" for i in range(len(symbols)))
    params = {f"s{i}": s for i, s in enumerate(symbols)}
    sql = f"""
        SELECT symbol, datetime, open, high, low, close, volume, amount
        FROM daily_bars
        WHERE symbol IN ({placeholders})
          AND datetime >= CAST(:start AS date) AND datetime <= CAST(:end AS date) + interval '1 day'
        ORDER BY symbol, datetime ASC
    """
    params["start"] = start_date
    params["end"] = end_date
    with engine.connect() as conn:
        df = pd.read_sql(text(sql), conn, params=params)
    if len(_bars_cache) >= _bars_cache_max:
        _bars_cache.clear()
    _bars_cache[key] = df.copy()
    return df


def load_minute_bars(symbols, start_date, end_date, period="5"):
    key = _cache_key(symbols, start_date, end_date, period)
    if key in _bars_cache:
        return _bars_cache[key].copy()
    from sqlalchemy import text
    engine = _get_engine()
    placeholders = ",".join(f":s{i}" for i in range(len(symbols)))
    params = {f"s{i}": s for i, s in enumerate(symbols)}
    sql = "SELECT symbol, datetime, open, high, low, close, volume, amount FROM minute_bars WHERE symbol IN (" + placeholders + ") AND period = :period AND datetime >= CAST(:start AS timestamp) AND datetime <= CAST(:end AS timestamp) + interval '1 day' ORDER BY symbol, datetime ASC"
    params["start"] = start_date
    params["end"] = end_date
    params["period"] = str(period)
    with engine.connect() as conn:
        df = pd.read_sql(text(sql), conn, params=params)
    if len(_bars_cache) >= _bars_cache_max:
        _bars_cache.clear()
    _bars_cache[key] = df.copy()
    return df


def compute_performance(trades, initial_cash, equity_curve=None):
    import math

    if not trades:
        return {"total_return": 0.0, "annual_return": 0.0, "max_drawdown": 0.0,
                "sharpe_ratio": 0.0, "sortino_ratio": 0.0, "calmar_ratio": 0.0,
                "final_value": float(initial_cash), "total_trades": 0,
                "win_rate": 0.0, "profit_ratio": 0.0, "total_commission": 0.0,
                "duration_days": 0, "trading_days": 0, "trades": []}

    total_pnl = sum(t.get("pnl", 0) or 0 for t in trades if t.get("side") == "sell")
    final_value = float(initial_cash) + total_pnl
    total_return = (final_value - float(initial_cash)) / float(initial_cash) if float(initial_cash) > 0 else 0.0
    wins = sum(1 for t in trades if t.get("side") == "sell" and (t.get("pnl") or 0) > 0)
    total_sells = sum(1 for t in trades if t.get("side") == "sell")
    win_rate = wins / total_sells if total_sells > 0 else 0.0

    # 盈亏比（profit_ratio）：平均盈利 / 平均亏损
    profits = [t.get("pnl", 0) or 0 for t in trades if t.get("side") == "sell" and (t.get("pnl") or 0) > 0]
    losses = [abs(t.get("pnl", 0) or 0) for t in trades if t.get("side") == "sell" and (t.get("pnl") or 0) < 0]
    avg_profit = sum(profits) / len(profits) if profits else 0.0
    avg_loss = sum(losses) / len(losses) if losses else 0.0
    profit_ratio = avg_profit / avg_loss if avg_loss > 0 else 0.0

    # 从净值曲线计算高级指标
    max_drawdown = 0.0
    sharpe_ratio = 0.0
    sortino_ratio = 0.0
    calmar_ratio = 0.0
    annual_return = total_return
    duration_days = 0
    trading_days = 0

    if equity_curve and len(equity_curve) >= 2:
        values = [float(p["total_value"]) for p in equity_curve]
        # 计算实际交易天数（按日期去重，分钟级数据每天有多条快照）
        dates_raw = [pt.get("datetime", "") for pt in equity_curve if pt.get("datetime")]
        unique_dates = set()
        for d in dates_raw:
            unique_dates.add(str(d)[:10])
        trading_days = len(unique_dates) if unique_dates else len(values)

        # 交易天数跨度（从净值曲线日期计算，支持日期和时间戳格式）
        dates = [pt.get("datetime", "") for pt in equity_curve if pt.get("datetime")]
        if len(dates) >= 2:
            try:
                from datetime import timedelta
                d0 = None
                d1 = None
                for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S%z", "%Y-%m-%dT%H:%M:%S%z", "%Y-%m-%d"]:
                    try:
                        d0 = datetime.strptime(str(dates[0]).replace("+00:00", ""), fmt.replace("%z", ""))
                        d1 = datetime.strptime(str(dates[-1]).replace("+00:00", ""), fmt.replace("%z", ""))
                        break
                    except ValueError:
                        continue
                if d0 and d1:
                    duration_days = max((d1 - d0).days, 1)
                else:
                    duration_days = trading_days
            except Exception:
                duration_days = trading_days
        else:
            duration_days = trading_days

        # 最大回撤
        peak = values[0]
        for v in values:
            if v > peak:
                peak = v
            dd = (peak - v) / peak if peak > 0 else 0
            if dd > max_drawdown:
                max_drawdown = dd

        # 年化收益（252 个自然交易日/年）
        if duration_days > 0:
            annual_return = (values[-1] / values[0]) ** (252 / duration_days) - 1 if values[0] > 0 else 0.0

        # 收益率序列
        returns = []
        for i in range(1, len(values)):
            if values[i-1] > 0:
                returns.append((values[i] - values[i-1]) / values[i-1])

        if len(returns) > 1:
            mean_ret = sum(returns) / len(returns)
            std_ret = math.sqrt(sum((r - mean_ret) ** 2 for r in returns) / (len(returns) - 1))

            # 年化因子：净值曲线通常是日级别快照
            annualize = math.sqrt(252)

            # 夏普比率（年化，无风险利率 2%）
            if std_ret > 0:
                sharpe_ratio = (mean_ret - 0.02 / 252) / std_ret * annualize

            # 索提诺比率
            downside_returns = [r for r in returns if r < 0]
            if downside_returns:
                downside_std = math.sqrt(sum(r ** 2 for r in downside_returns) / len(downside_returns))
                if downside_std > 0:
                    sortino_ratio = (mean_ret - 0.02 / 252) / downside_std * annualize

        # Calmar 比率
        if max_drawdown > 0:
            calmar_ratio = annual_return / max_drawdown

    return {
        "total_return": round(total_return, 6),
        "annual_return": round(annual_return, 6),
        "max_drawdown": round(max_drawdown, 6),
        "sharpe_ratio": round(sharpe_ratio, 4),
        "sortino_ratio": round(sortino_ratio, 4),
        "calmar_ratio": round(calmar_ratio, 4),
        "final_value": round(final_value, 2),
        "total_trades": len(trades),
        "win_rate": round(win_rate, 4),
        "profit_ratio": round(profit_ratio, 4),
        "total_commission": 0.0,
        "duration_days": duration_days,
        "trading_days": trading_days,
        "trades": trades,
    }


class MockContext:
    def __init__(self, cash):
        self.initial_cash = float(cash)
        self.cash = self.initial_cash
        self.trades = []
        self.positions = {}
        self._history = {}
        self._open_trades = {}
        self._tags = {}
        self.current_date = None
        self.current_bar = None
        self.equity_curve = []
    def set_tag(self, symbol, key, value):
        self._tags.setdefault(symbol, {})[key] = value
    def get_tag(self, symbol, key, default=None):
        return self._tags.get(symbol, {}).get(key, default)
    def _record(self, sym):
        if self.current_bar:
            self._history.setdefault(sym, []).append(dict(self.current_bar))
    def record_snapshot(self):
        pos_val = 0.0
        for sym, qty in self.positions.items():
            if qty > 0 and self.current_bar and self.current_bar.get("symbol") == sym:
                pos_val += float(self.current_bar["close"]) * qty
        bar_dt = (self.current_bar or {}).get("datetime") or str(self.current_date)
        self.equity_curve.append({
            "datetime": bar_dt,
            "total_value": str(round(self.cash + pos_val, 2)),
            "cash": str(round(self.cash, 2)),
            "position_value": str(round(pos_val, 2)),
        })
    def history(self, sym, n):
        h = self._history.get(sym, [])
        return h[-n:] if len(h) >= n else h
    def buy(self, symbol, percent=0.1):
        bar = self.current_bar
        if bar is None: return
        price = float(bar["close"])
        qty = int(self.cash * percent / price / 100) * 100
        if qty < 100: return
        self.cash -= round(price * qty, 4)
        self.positions[symbol] = self.positions.get(symbol, 0) + qty
        self._open_trades[symbol] = {"price": price, "index": len(self._history.get(symbol, []))}
        bar_dt = (self.current_bar or {}).get("datetime") or str(self.current_date)
        self.trades.append({"symbol": symbol, "side": "buy", "quantity": qty,
                            "price": price, "datetime": bar_dt})
    def sell(self, symbol, percent=1.0):
        bar = self.current_bar
        if bar is None: return
        qty = int(self.positions.get(symbol, 0) * percent / 100) * 100
        if qty <= 0: return
        price = float(bar["close"])
        self.cash += round(price * qty, 4)
        self.positions[symbol] = self.positions.get(symbol, 0) - qty
        ot = self._open_trades.pop(symbol, {"price": price})
        pnl = round((price - ot["price"]) * qty, 2)
        bar_dt = (self.current_bar or {}).get("datetime") or str(self.current_date)
        self.trades.append({"symbol": symbol, "side": "sell", "quantity": qty,
                            "price": price, "pnl": pnl, "datetime": bar_dt})
    def __getattr__(self, name):
        if name.startswith("_"):
            raise AttributeError(name)
        return lambda *a, **kw: None


def execute_strategy(code, bars, initial_cash, period="1d"):
    # 规范化代码缩进：移除公共前导空格 + 首尾空白
    # 防止用户保存代码时意外引入前导空格导致 SyntaxError
    code = textwrap.dedent(code).strip()
    local_ns = {"pd": _SAFE_PANDAS, "datetime": datetime, "print": lambda *a: None, "BaseStrategy": object, "__builtins__": _SAFE_BUILTINS, "__name__": "__main__"}
    exec(code, local_ns)
    sc = None
    for name, obj in local_ns.items():
        if isinstance(obj, type) and hasattr(obj, "on_bar") and name != "BaseStrategy":
            sc = obj
            break
    if sc is None:
        return [], []
    ctx = MockContext(initial_cash)
    # 使用 __new__ 而不是构造函数，绕过用户代码中可能存在的自定义 __init__
    # 用户策略可能定义带参数的 __init__(self, data, indicator)
    # 沙箱只关心 on_bar / on_init，不依赖构造函数
    s = object.__new__(sc)
    # 初始化策略参数容器（object.__new__ 跳过 __init__，需要手动初始化）
    if not hasattr(s, 'params'):
        s.params = {}
    if hasattr(s, "on_init"):
        s.on_init()
    is_minute = period != "1d"
    for dt, grp in bars.groupby(bars["datetime"].dt.date):
        ctx.current_date = dt
        for _, row in grp.iterrows():
            sym = row["symbol"]
            bar_dt = str(row["datetime"])
            ctx.current_bar = {"symbol": sym, "close": float(row["close"]),
                               "high": float(row["high"]), "low": float(row["low"]),
                               "open": float(row["open"]), "volume": float(row.get("volume", 0)),
                               "datetime": bar_dt}
            ctx._record(sym)
            day_bars = grp[grp["symbol"] == sym]
            s.on_bar(ctx, day_bars)
            # 分钟级：每根 bar 记录快照
            if is_minute:
                ctx.record_snapshot()
        # 日线级：每天结束时记录快照
        if not is_minute:
            ctx.record_snapshot()
    for sym, qty in list(ctx.positions.items()):
        if qty > 0 and ctx.current_bar:
            ctx.sell(sym, 1.0)
    if ctx.current_date:
        ctx.record_snapshot()
    return ctx.trades, ctx.equity_curve


def bars_from_rows(rows):
    """将 Rust 传入的行列表转为 DataFrame，转换 datetime 为时间类型"""
    if not rows:
        return pd.DataFrame()
    df = pd.DataFrame(rows)
    if "datetime" in df.columns:
        df["datetime"] = pd.to_datetime(df["datetime"])
    for col in ["open", "high", "low", "close", "amount"]:
        if col in df.columns:
            df[col] = pd.to_numeric(df[col], errors="coerce")
    if "volume" in df.columns:
        df["volume"] = pd.to_numeric(df["volume"], errors="coerce").fillna(0).astype(int)
    return df


def handle_batch_scan(params):
    """批量扫描：对多只标的运行策略，返回每只的绩效"""
    code = params.get("code", "")
    symbols = params.get("symbols", [])
    start_date = params.get("start_date", "2024-01-01")
    end_date = params.get("end_date", "2024-12-31")
    initial_cash = params.get("initial_cash", 1000000.0)
    period = params.get("period", "1d")

    if not symbols or not code.strip():
        return {"status": "error", "error": "Missing symbols or code"}

    results = []
    # 预加载所有标的的数据
    for symbol in symbols:
        try:
            if period == "1d" or period == "":
                bars = load_daily_bars([symbol], start_date, end_date)
            else:
                db_period = period.replace("min", "")
                bars = load_minute_bars([symbol], start_date, end_date, db_period)
                if bars.empty:
                    bars = load_daily_bars([symbol], start_date, end_date)

            if bars.empty or len(bars) < 20:
                continue

            trades, equity_curve = execute_strategy(code, bars, initial_cash, period)
            perf = compute_performance(trades, initial_cash, equity_curve)

            score = float(perf.get("sharpe_ratio", 0)) * 0.4                   + float(perf.get("total_return", 0)) * 0.4                   - abs(float(perf.get("max_drawdown", 0))) * 0.2

            results.append({
                "symbol": symbol,
                "score": round(score, 4),
                "total_return": round(float(perf.get("total_return", 0)), 6),
                "sharpe_ratio": round(float(perf.get("sharpe_ratio", 0)), 4),
                "max_drawdown": round(float(perf.get("max_drawdown", 0)), 6),
                "total_trades": int(perf.get("total_trades", 0)),
                "final_value": float(perf.get("final_value", 0)),
            })
        except Exception as e:
            # 跳过执行失败的标的
            continue

    # 按 score 降序排序
    results.sort(key=lambda x: x["score"], reverse=True)

    return {"status": "success", "results": results}


def handle_request(params):
    """处理单个回测请求，返回结果 dict"""
    code = params.get("code", "")
    symbols = params.get("symbols", [])
    start_date = params.get("start_date", "2024-01-01")
    end_date = params.get("end_date", "2024-12-31")
    initial_cash = params.get("initial_cash", 1000000.0)

    if not symbols:
        return {"status": "error", "result": None, "error": "No symbols", "traceback": None}
    if not code.strip():
        return {"status": "error", "result": None, "error": "Empty code", "traceback": None}

    try:
        period = params.get("period", "1d")
        bars_data = params.get("bars")
        if bars_data is not None:
            bars = bars_from_rows(bars_data)
        elif period and period != "1d":
            db_period = period.replace("min", "")
            try:
                bars = load_minute_bars(symbols, start_date, end_date, db_period)
            except Exception:
                bars = pd.DataFrame()
            if bars.empty:
                bars = load_daily_bars(symbols, start_date, end_date)
        else:
            bars = load_daily_bars(symbols, start_date, end_date)
        trades, equity_curve = execute_strategy(code, bars, initial_cash, period)
        perf = compute_performance(trades, initial_cash, equity_curve)
        perf["equity_curve"] = equity_curve
        is_minute = period and period != "1d"
        kline = []
        if not bars.empty:
            for _, row in bars.iterrows():
                kline.append({
                    "symbol": str(row["symbol"]),
                    "datetime": str(row["datetime"]),
                    "open": str(row["open"]),
                    "high": str(row["high"]),
                    "low": str(row["low"]),
                    "close": str(row["close"]),
                    "volume": int(row["volume"]) if pd.notna(row["volume"]) else 0,
                    "amount": str(row["amount"]) if pd.notna(row.get("amount")) else None,
                })
        if is_minute and len(kline) > 5000:
            step = len(kline) // 5000
            kline = kline[::step]
        perf["kline_bars"] = kline
        return {"status": "success", "result": perf, "error": None, "traceback": None}
    except Exception as e:
        return {"status": "error", "result": None, "error": str(e), "traceback": traceback.format_exc()}


def main():
    # 循环模式：每行一个 JSON 请求，进程持久化复用（pandas/DB 已预加载）
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            params = json.loads(line)
        except json.JSONDecodeError as e:
            result = {"status": "error", "result": None,
                      "error": f"Invalid JSON: {e}", "traceback": traceback.format_exc()}
            print(json.dumps(result), flush=True)
            continue

        if params.get("action") == "batch_scan":
            result = handle_batch_scan(params)
        else:
            result = handle_request(params)
        print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
