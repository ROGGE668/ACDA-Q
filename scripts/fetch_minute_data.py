#!/usr/bin/env python3
"""
ACDA-Q 分钟线数据拉取脚本
========================
- 并行拉取 1/5/15/30/60 分钟线
- 指数退避 + 随机抖动防 IP 限制
- 每级别最多重试 7 分钟
- 从上次拉取的最新时间开始增量拉取
"""

import os
import sys
import time
import random
import signal
import logging
import argparse
from datetime import datetime, timedelta
from concurrent.futures import ThreadPoolExecutor, as_completed

import pandas as pd

try:
    from psycopg2 import connect, sql
    from psycopg2.extras import execute_values
except ImportError:
    print("请安装 psycopg2: pip install psycopg2-binary")
    sys.exit(1)

# ── 日志 ──────────────────────────────────────────
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("fetch_minute")

# ── 配置 ──────────────────────────────────────────
DB_CONFIG = {
    "host": os.environ.get("DB_HOST", "127.0.0.1"),
    "port": int(os.environ.get("DB_PORT", "5433")),
    "database": os.environ.get("DB_NAME", "quant_market"),
    "user": os.environ.get("DB_USER", "quant"),
    "password": os.environ.get("DB_PASS", "quant123"),
}

PERIODS = ["1", "5", "15", "30", "60"]
MAX_RETRY_SECONDS = 7 * 60  # 每级别最多重试 7 分钟
BASE_DELAY = 1.0            # 基础退避延迟(秒)
MAX_DELAY = 30.0            # 最大退避延迟(秒)
BATCH_SIZE = 50             # 每批拉取的标的数
RATE_LIMIT_DELAY = 0.3      # 请求间隔(秒)

# 全局速率限制器（线程安全，10线程共享）
import threading as _threading
class _RateLimiter:
    def __init__(self, max_per_sec=8):
        self._interval = 1.0 / max_per_sec
        self._lock = _threading.Lock()
        self._last = 0.0
    def acquire(self):
        with self._lock:
            now = time.time()
            wait = self._last + self._interval - now
            if wait > 0:
                time.sleep(wait)
            self._last = time.time()

_rate_limiter = _RateLimiter(max_per_sec=8)

_stop = False

def _signal_handler(sig, frame):
    global _stop
    log.warning("收到中断信号，等待当前批次完成...")
    _stop = True

signal.signal(signal.SIGINT, _signal_handler)
signal.signal(signal.SIGTERM, _signal_handler)


def get_db():
    return connect(**DB_CONFIG)


def get_latest_datetime(conn, period):
    """获取某周期的最新数据时间"""
    cur = conn.cursor()
    cur.execute("SELECT MAX(datetime) FROM minute_bars WHERE period = %s", (period,))
    row = cur.fetchone()
    return row[0] if row and row[0] else None


def get_all_symbols(conn):
    """获取所有活跃 A 股标的"""
    cur = conn.cursor()
    cur.execute("""
        SELECT symbol FROM stock_basic
        WHERE exchange IN ('A', '主板', '创业板', '科创板')
          AND is_active = true
        ORDER BY symbol
    """)
    return [row[0] for row in cur.fetchall()]


def decompress_overlapping_chunks(conn, period, start_date):
    """解压与数据范围重叠的压缩 chunk，避免 tuple decompression limit 错误"""
    cur = conn.cursor()
    try:
        cur.execute("""
            SELECT chunk_name, range_start, range_end
            FROM timescaledb_information.chunks
            WHERE hypertable_name = 'minute_bars'
              AND is_compressed = true
              AND range_end > %s::timestamptz
            ORDER BY range_start DESC
            LIMIT 2
        """, (start_date,))
        chunks = cur.fetchall()
        for chunk_name, rs, re in chunks:
            try:
                cur.execute(f"SELECT decompress_chunk('{chunk_name}')")
                log.info(f"[decompress] 解压 chunk {chunk_name} ({rs} ~ {re})")
            except Exception as e:
                log.debug(f"[decompress] {chunk_name} 已解压或失败: {e}")
        conn.commit()
    except Exception as e:
        log.debug(f"[decompress] 跳过: {e}")
        conn.rollback()

def recompress_chunks(conn, period):
    """同步完成后重新压缩已解压的 chunk"""
    cur = conn.cursor()
    try:
        cur.execute("""
            SELECT chunk_name FROM timescaledb_information.chunks
            WHERE hypertable_name = 'minute_bars'
              AND is_compressed = false
              AND range_end < NOW() - INTERVAL '3 days'
            ORDER BY range_start DESC LIMIT 10
        """)
        chunks = [row[0] for row in cur.fetchall()]
        for chunk_name in chunks:
            try:
                cur.execute(f"SELECT compress_chunk('{chunk_name}')")
                log.info(f"[compress] 重新压缩 chunk {chunk_name}")
            except Exception as e:
                log.debug(f"[compress] {chunk_name} 压缩跳过: {e}")
        conn.commit()
    except Exception as e:
        log.debug(f"[compress] 跳过: {e}")
        conn.rollback()

_decompressed_periods: set = set()

import requests as _requests
import json as _json
import re as _re

_HEADERS = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    "Accept": "*/*",
    "Referer": "https://finance.sina.com.cn/",
}

_SINA_SCALE_MAP = {"1": "1", "5": "5", "15": "15", "30": "30", "60": "240"}

def _sina_fetch_minute_kline(code, period, start_date, end_date):
    """通过新浪财经 API 拉取分钟线，返回 DataFrame"""
    prefix = "sh" if code.startswith("6") else "sz"
    symbol = f"{prefix}{code}"
    scale = _SINA_SCALE_MAP.get(period, "5")
    datalen = 1000
    url = f"https://quotes.sina.cn/cn/api/jsonp_v2.php/var/CN_MarketDataService.getKLineData?symbol={symbol}&scale={scale}&ma=no&datalen={datalen}"
    r = _requests.get(url, headers=_HEADERS, timeout=15)
    r.raise_for_status()
    text = r.text
    m = _re.search(r"\((\[.*?\])\)", text, _re.DOTALL)
    if not m:
        m = _re.search(r"\[(\{.*?\})\]", text, _re.DOTALL)
        if m:
            arr_str = "[" + m.group(1) + "]"
        else:
            return pd.DataFrame()
    else:
        arr_str = m.group(1)
    try:
        data = _json.loads(arr_str)
    except:
        return pd.DataFrame()
    if not data:
        return pd.DataFrame()
    start_dt = pd.to_datetime(start_date)
    end_dt = pd.to_datetime(end_date)
    rows = []
    for item in data:
        dt = pd.to_datetime(item.get("day", ""))
        if dt < start_dt or dt > end_dt:
            continue
        rows.append({
            "时间": item["day"],
            "开盘": float(item.get("open", 0)),
            "收盘": float(item.get("close", 0)),
            "最高": float(item.get("high", 0)),
            "最低": float(item.get("low", 0)),
            "成交量": float(item.get("volume", 0)),
            "成交额": float(item.get("amount", 0)),
        })
    return pd.DataFrame(rows)


def fetch_minute_data(symbol, period, start_date, end_date, retries=0, deadline=None):
    """
    拉取单只标的的分钟线数据，带指数退避重试。
    返回 DataFrame 或 None。
    """
    if _stop:
        return None
    if deadline and time.time() > deadline:
        log.warning(f"[{symbol}] 超过重试时限，跳过")
        return None

    try:
        code = symbol.split(".")[0] if "." in symbol else symbol
        df = _sina_fetch_minute_kline(code, period, start_date, end_date)
        _rate_limiter.acquire()
        return df
    except Exception as e:
        if _stop:
            return None
        delay = min(BASE_DELAY * (2 ** retries) + random.uniform(0, 1), MAX_DELAY)
        if deadline:
            remaining = deadline - time.time()
            if remaining <= 0:
                log.warning(f"[{symbol}] 超过重试时限，跳过")
                return None
            delay = min(delay, remaining)
        log.warning(f"[{symbol}] 拉取失败(周期={period}): {e}，{delay:.1f}s 后重试")
        time.sleep(delay)
        return fetch_minute_data(symbol, period, start_date, end_date, retries + 1, deadline)


def save_minute_bars(conn, symbol, period, df):
    """写入分钟线数据到 minute_bars 表"""
    if df is None or df.empty:
        return 0

    cur = conn.cursor()

    # 东方财富 API 返回的列名
    col_map = {
        "时间": "datetime",
        "开盘": "open",
        "收盘": "close",
        "最高": "high",
        "最低": "low",
        "成交量": "volume",
        "成交额": "amount",
    }

    rows = []
    for _, row in df.iterrows():
        dt_str = str(row.get("时间", ""))
        try:
            dt = pd.to_datetime(dt_str)
        except:
            continue
        rows.append((
            symbol,
            dt.to_pydatetime(),
            float(row.get("开盘", 0)),
            float(row.get("最高", 0)),
            float(row.get("最低", 0)),
            float(row.get("收盘", 0)),
            int(float(row.get("成交量", 0))),
            float(row.get("成交额", 0)) if pd.notna(row.get("成交额")) else 0.0,
        ))

    if not rows:
        return 0

    # 首次写入时解压目标 chunk
    global _decompressed_periods
    if period not in _decompressed_periods:
        decompress_overlapping_chunks(conn, period, rows[0][1])
        _decompressed_periods.add(period)

    execute_values(cur, """
        INSERT INTO minute_bars (symbol, datetime, open, high, low, close, volume, amount, period)
        VALUES %s
        ON CONFLICT (symbol, datetime, period) DO UPDATE SET
            open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low,
            close = EXCLUDED.close, volume = EXCLUDED.volume, amount = EXCLUDED.amount
    """, rows, template="(%s, %s, %s, %s, %s, %s, %s, %s, '" + period + "')")
    conn.commit()
    return len(rows)


def _fetch_and_save_one(sym, period, start_date, end_date, deadline):
    """拉取单只标的并写入 DB（线程安全）"""
    if _stop:
        return 0
    df = fetch_minute_data(sym, period, start_date, end_date, deadline=deadline)
    if df is None or df.empty:
        return 0
    conn = get_db()
    try:
        n = save_minute_bars(conn, sym, period, df)
        return n
    except Exception as e:
        log.warning(f"[{sym}] 写入失败: {e}")
        return 0
    finally:
        conn.close()


def fetch_period(conn, period, symbols, start_date, end_date):
    """拉取单个周期的所有数据（多线程并行）"""
    deadline = time.time() + MAX_RETRY_SECONDS
    total_rows = 0
    success_count = 0
    fail_count = 0
    worker_count = min(10, len(symbols))

    log.info(f"[周期={period}min] 开始拉取 {len(symbols)} 只标的，范围 {start_date} ~ {end_date}，线程数 {worker_count}")

    with ThreadPoolExecutor(max_workers=worker_count) as pool:
        futures = {}
        for sym in symbols:
            if _stop:
                break
            futures[sym] = pool.submit(_fetch_and_save_one, sym, period, start_date, end_date, deadline)

        done_count = 0
        for sym, fut in futures.items():
            try:
                n = fut.result()
                total_rows += n
                if n > 0:
                    success_count += 1
                else:
                    fail_count += 1
            except Exception as e:
                log.warning(f"[{sym}] 异常: {e}")
                fail_count += 1
            done_count += 1
            if done_count % 500 == 0:
                log.info(f"[周期={period}min] 进度 {done_count}/{len(symbols)}，累计 {total_rows} 行")

    log.info(f"[周期={period}min] 完成：成功 {success_count}，失败 {fail_count}，总行数 {total_rows}")
    # 同步完成后重新压缩已解压的 chunk
    try:
        recompress_chunks(conn, period)
    except Exception as e:
        log.debug(f"[compress] 后处理跳过: {e}")
    return total_rows


def main():
    parser = argparse.ArgumentParser(description="ACDA-Q 分钟线数据拉取")
    parser.add_argument("--periods", nargs="+", default=PERIODS, help="要拉取的周期 (1/5/15/30/60)")
    parser.add_argument("--start", help="开始日期 (YYYY-MM-DD)，默认从数据库最新时间开始")
    parser.add_argument("--end", help="结束日期 (YYYY-MM-DD)，默认今天")
    parser.add_argument("--symbols", nargs="+", help="指定标的列表 (留空=全部)")
    parser.add_argument("--parallel", type=int, default=3, help="并行拉取的周期数 (默认3)")
    parser.add_argument("--full", action="store_true", help="全量拉取 (从最早时间开始)")
    args = parser.parse_args()

    conn = get_db()
    today = args.end or datetime.now().strftime("%Y-%m-%d")
    symbols = args.symbols or get_all_symbols(conn)
    log.info(f"标的数: {len(symbols)}，周期: {args.periods}，结束: {today}")

    def run_period(period):
        if args.start:
            start = args.start
        elif args.full:
            start = "2024-01-01"
        else:
            latest = get_latest_datetime(conn, period)
            if latest:
                start = (latest - timedelta(days=1)).strftime("%Y-%m-%d %H:%M:%S")
            else:
                start = "2024-01-01"
        return fetch_period(conn, period, symbols, start, today)

    # 并行拉取各周期
    results = {}
    with ThreadPoolExecutor(max_workers=args.parallel) as pool:
        futures = {pool.submit(run_period, p): p for p in args.periods}
        for future in as_completed(futures):
            period = futures[future]
            try:
                count = future.result()
                results[period] = count
            except Exception as e:
                log.error(f"周期 {period} 拉取异常: {e}")
                results[period] = 0

    conn.close()

    log.info("=== 拉取汇总 ===")
    for p, count in sorted(results.items()):
        log.info(f"  {p}min: {count} 行")
    log.info("完成！")


if __name__ == "__main__":
    main()
