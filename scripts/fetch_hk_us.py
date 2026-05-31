#!/usr/bin/env python3
"""拉取港股和美股数据"""

import requests
from psycopg2 import connect
from psycopg2.extras import execute_values
import time

DB_CONFIG = {
    "host": "localhost",
    "port": 5432,
    "database": "quant_db",
    "user": "quant",
    "password": "quant123"
}

def get_db_connection():
    return connect(**DB_CONFIG)

def sync_hk_basic(conn):
    """同步港股列表"""
    print("正在获取港股列表...")
    try:
        # 使用腾讯港股列表API
        url = "http://sqt.gtimg.cn/q=hkAll"
        resp = requests.get(url, timeout=30)
        lines = resp.text.strip().split('\n')

        cursor = conn.cursor()
        data = []
        for line in lines:
            parts = line.split('~')
            if len(parts) > 10:
                code = parts[1]  # 如 00700
                name = parts[2]  # 如 腾讯控股
                symbol = f"HK{code.zfill(5)}"
                data.append((symbol, name, "HK"))

        if data:
            execute_values(cursor, """
                INSERT INTO stock_basic (symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active)
                VALUES %s
                ON CONFLICT (symbol) DO UPDATE SET name = EXCLUDED.name
            """, data)
            conn.commit()
        cursor.close()
        print(f"  港股已写入 {len(data)} 条")
        return len(data)
    except Exception as e:
        print(f"  港股列表失败: {e}")
        return 0

def sync_us_basic(conn):
    """同步美股列表"""
    print("正在获取美股列表...")
    try:
        # 使用腾讯美股列表API
        url = "http://sqt.gtimg.cn/q=usAll"
        resp = requests.get(url, timeout=30)
        lines = resp.text.strip().split('\n')

        cursor = conn.cursor()
        data = []
        for line in lines:
            parts = line.split('~')
            if len(parts) > 10:
                code = parts[1]  # 如 AAPL
                name = parts[2]  # 如 Apple Inc.
                symbol = f"US{code.upper()}"
                data.append((symbol, name, "US"))

        if data:
            execute_values(cursor, """
                INSERT INTO stock_basic (symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active)
                VALUES %s
                ON CONFLICT (symbol) DO UPDATE SET name = EXCLUDED.name
            """, data)
            conn.commit()
        cursor.close()
        print(f"  美股已写入 {len(data)} 条")
        return len(data)
    except Exception as e:
        print(f"  美股列表失败: {e}")
        return 0

def fetch_tencent_hist(code, exchange, days=365):
    """获取腾讯历史K线"""
    if exchange == "HK":
        tc_code = f"hk{code.zfill(5)}"
    elif exchange == "US":
        tc_code = f"us{code.upper()}"
    else:
        return []

    url = f"http://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={tc_code},day,,,{days},qfq"

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
    except:
        return []

def sync_hk_daily(conn, days=365):
    """同步港股日线"""
    print(f"正在拉取港股日线 (近{days}天)...")

    cursor = conn.cursor()
    cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'HK'")
    codes = [row[0] for row in cursor.fetchall()]
    print(f"  共 {len(codes)} 只港股")

    total = 0
    for i, symbol in enumerate(codes):
        code = symbol[2:]  # 去掉 HK 前缀
        data = fetch_tencent_hist(code, "HK", days)

        if not data:
            continue

        rows = [(symbol, d["date"], d["open"], d["close"], d["high"], d["low"],
                 d["volume"], 0.0, 0.0, 0.0) for d in data]

        execute_values(cursor, """
            INSERT INTO daily_bars (symbol, datetime, open, close, high, low, volume, amount, pre_close, change_pct)
            VALUES %s
            ON CONFLICT (symbol, datetime) DO UPDATE SET close = EXCLUDED.close
        """, rows)
        total += len(rows)

        if (i + 1) % 50 == 0:
            conn.commit()
            print(f"  进度: {i+1}/{len(codes)}, 已插入 {total} 条")

        time.sleep(0.1)  # 避免请求过快

    conn.commit()
    cursor.close()
    print(f"  港股日线完成: {total} 条")
    return total

def sync_us_daily(conn, days=365):
    """同步美股日线"""
    print(f"正在拉取美股日线 (近{days}天)...")

    cursor = conn.cursor()
    cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'US'")
    codes = [row[0] for row in cursor.fetchall()]
    print(f"  共 {len(codes)} 只美股")

    total = 0
    for i, symbol in enumerate(codes):
        code = symbol[2:]  # 去掉 US 前缀
        data = fetch_tencent_hist(code, "US", days)

        if not data:
            continue

        rows = [(symbol, d["date"], d["open"], d["close"], d["high"], d["low"],
                 d["volume"], 0.0, 0.0, 0.0) for d in data]

        execute_values(cursor, """
            INSERT INTO daily_bars (symbol, datetime, open, close, high, low, volume, amount, pre_close, change_pct)
            VALUES %s
            ON CONFLICT (symbol, datetime) DO UPDATE SET close = EXCLUDED.close
        """, rows)
        total += len(rows)

        if (i + 1) % 50 == 0:
            conn.commit()
            print(f"  进度: {i+1}/{len(codes)}, 已插入 {total} 条")

        time.sleep(0.1)

    conn.commit()
    cursor.close()
    print(f"  美股日线完成: {total} 条")
    return total

def main():
    print("=" * 60)
    print("港股/美股数据拉取")
    print("=" * 60)

    conn = get_db_connection()

    # 同步股票列表
    sync_hk_basic(conn)
    sync_us_basic(conn)

    # 同步日线数据
    sync_hk_daily(conn)
    sync_us_daily(conn)

    conn.close()
    print("=" * 60)
    print("完成!")
    print("=" * 60)

if __name__ == "__main__":
    main()
