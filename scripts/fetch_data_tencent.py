#!/usr/bin/env python3
"""ACDA-Q 数据拉取脚本 - 腾讯数据源"""

import argparse
from datetime import datetime, timedelta
import sys

try:
    import pandas as pd
    import requests
    from psycopg2 import connect
    from psycopg2.extras import execute_values
except ImportError as e:
    print(f"缺少依赖: {e}")
    sys.exit(1)


DB_CONFIG = {
    "host": "localhost",
    "port": 5432,
    "database": "quant_db",
    "user": "quant",
    "password": "quant123"
}

# 腾讯行情API
TENCENT_HIST_API = "http://web.ifzq.gtimg.cn/appstock/app/fqkline/get"


def get_db_connection():
    return connect(**DB_CONFIG)


def get_tencent_code(symbol, exchange):
    """转换为腾讯股票代码"""
    if exchange == "A":
        code = symbol.split(".")[0]
        if code.startswith(("0", "3")):
            return f"sz{code}"
        else:
            return f"sh{code}"
    elif exchange == "HK":
        return f"hk{symbol[2:]}" if symbol.startswith("HK") else f"hk{symbol}"
    elif exchange == "US":
        return f"us{symbol[2:]}" if symbol.startswith("US") else f"us{symbol}"
    return symbol


def fetch_tencent_hist(symbol, exchange, days=365):
    """获取腾讯历史K线"""
    tc_code = get_tencent_code(symbol, exchange)
    url = f"{TENCENT_HIST_API}?param={tc_code},day,,,{days},qfq"

    try:
        resp = requests.get(url, timeout=10)
        data = resp.json()
        qfqday = data.get("data", {}).get(tc_code, {}).get("qfqday")
        if not qfqday:
            return []

        result = []
        for item in qfqday:
            result.append({
                "date": item[0],
                "open": float(item[1]),
                "close": float(item[2]),
                "high": float(item[3]),
                "low": float(item[4]),
                "volume": int(float(item[5]))
            })
        return result
    except Exception as e:
        return []


def sync_daily_a_stock(conn, days=365):
    """同步 A股 日线数据"""
    print(f"正在拉取 A股 日线数据 (近{days}天)...")

    cursor = conn.cursor()
    cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'A'")
    codes = [row[0] for row in cursor.fetchall()]
    print(f"  共 {len(codes)} 只 A股")

    total_count = 0
    for i, symbol in enumerate(codes):
        data = fetch_tencent_hist(symbol, "A", days)

        if not data:
            continue

        rows = []
        for d in data:
            rows.append((
                symbol, d["date"], d["open"], d["close"], d["high"], d["low"],
                d["volume"], 0.0, 0.0, 0.0
            ))

        if rows:
            execute_values(cursor, """
                INSERT INTO daily_bars (symbol, datetime, open, close, high, low, volume, amount, pre_close, change_pct)
                VALUES %s
                ON CONFLICT (symbol, datetime) DO UPDATE SET close = EXCLUDED.close
            """, rows)
            total_count += len(rows)

        if (i + 1) % 100 == 0:
            conn.commit()
            print(f"  进度: {i+1}/{len(codes)}, 已插入 {total_count} 条")

    conn.commit()
    cursor.close()
    print(f"  A股 日线数据完成: {total_count} 条")
    return total_count


def sync_daily_hk(conn, days=365):
    """同步 港股 日线数据"""
    print(f"正在拉取 港股 日线数据 (近{days}天)...")

    cursor = conn.cursor()
    cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'HK'")
    codes = [row[0] for row in cursor.fetchall()]
    print(f"  共 {len(codes)} 只 港股")

    total_count = 0
    for i, symbol in enumerate(codes):
        data = fetch_tencent_hist(symbol, "HK", days)

        if not data:
            continue

        rows = []
        for d in data:
            rows.append((
                symbol, d["date"], d["open"], d["close"], d["high"], d["low"],
                d["volume"], 0.0, 0.0, 0.0
            ))

        if rows:
            execute_values(cursor, """
                INSERT INTO daily_bars (symbol, datetime, open, close, high, low, volume, amount, pre_close, change_pct)
                VALUES %s
                ON CONFLICT (symbol, datetime) DO UPDATE SET close = EXCLUDED.close
            """, rows)
            total_count += len(rows)

        if (i + 1) % 100 == 0:
            conn.commit()
            print(f"  进度: {i+1}/{len(codes)}, 已插入 {total_count} 条")

    conn.commit()
    cursor.close()
    print(f"  港股 日线数据完成: {total_count} 条")
    return total_count


def sync_minute_a_stock(conn, period="5"):
    """同步 A股 分钟级数据"""
    print(f"正在拉取 A股 {period}分钟级数据...")

    period_map = {"1": "1min", "5": "5min", "15": "15min", "30": "30min", "60": "60min"}
    tc_period = period_map.get(period, "5min")

    cursor = conn.cursor()
    cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'A'")
    codes = [row[0] for row in cursor.fetchall()]
    print(f"  共 {len(codes)} 只 A股")

    total_count = 0
    for i, symbol in enumerate(codes):
        tc_code = get_tencent_code(symbol, "A")
        url = f"{TENCENT_HIST_API}?param={tc_code},{tc_period},,,10,qfq"

        try:
            resp = requests.get(url, timeout=10)
            data = resp.json()
            min_data = data.get("data", {}).get(tc_code, {}).get(tc_period)

            if not min_data:
                continue

            rows = []
            for item in min_data:
                rows.append((
                    symbol, item[0], float(item[1]), float(item[2]),
                    float(item[3]), float(item[4]), int(float(item[5])), 0.0
                ))

            if rows:
                execute_values(cursor, """
                    INSERT INTO minute_bars (symbol, datetime, open, high, low, close, volume, amount)
                    VALUES %s
                    ON CONFLICT (symbol, datetime) DO UPDATE SET close = EXCLUDED.close
                """, rows)
                total_count += len(rows)

        except Exception:
            continue

        if (i + 1) % 100 == 0:
            conn.commit()
            print(f"  进度: {i+1}/{len(codes)}, 已插入 {total_count} 条")

    conn.commit()
    cursor.close()
    print(f"  A股 {period}分钟数据完成: {total_count} 条")
    return total_count


def main():
    parser = argparse.ArgumentParser(description="ACDA-Q 数据拉取脚本 (腾讯数据源)")
    parser.add_argument("--market", choices=["a", "hk", "us", "all"], default="all")
    parser.add_argument("--data-type", choices=["basic", "daily", "minute", "all"], default="all")
    parser.add_argument("--period", default="5", help="分钟周期: 1, 5, 15, 30, 60")
    parser.add_argument("--days", type=int, default=365, help="获取近多少天数据")

    args = parser.parse_args()

    print("=" * 60)
    print("ACDA-Q 数据拉取脚本 (腾讯数据源)")
    print("=" * 60)

    conn = get_db_connection()

    if args.data_type in ["daily", "all"]:
        if args.market in ["a", "all"]:
            sync_daily_a_stock(conn, args.days)
        if args.market in ["hk", "all"]:
            sync_daily_hk(conn, args.days)

    if args.data_type in ["minute", "all"]:
        if args.market in ["a", "all"]:
            sync_minute_a_stock(conn, args.period)

    conn.close()
    print("=" * 60)
    print("数据拉取完成!")
    print("=" * 60)


if __name__ == "__main__":
    main()
