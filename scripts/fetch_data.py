#!/usr/bin/env python3
"""ACDA-Q 数据拉取脚本 - 使用 AkShare"""

import argparse
from datetime import datetime, timedelta
import sys

try:
    import akshare as ak
    import pandas as pd
    from psycopg2 import connect
    from psycopg2.extras import execute_values
except ImportError as e:
    print(f"缺少依赖: {e}")
    print("请运行: pip install akshare pandas psycopg2-binary")
    sys.exit(1)


DB_CONFIG = {
    "host": "localhost",
    "port": 5432,
    "database": "quant_db",
    "user": "quant",
    "password": "quant123"
}

def get_db_connection():
    return connect(**DB_CONFIG)


def get_a_stock_code(symbol: str) -> str:
    """将 6 位代码转换为 ts_code 格式"""
    if symbol.startswith(("0", "3")):
        return f"{symbol}.SZ"
    else:
        return f"{symbol}.SH"


def sync_stock_basic(conn) -> int:
    """同步股票基础信息"""
    print("正在拉取股票列表...")

    cursor = conn.cursor()

    # A股
    try:
        print("  拉取 A股 股票列表...")
        df_a = ak.stock_info_a_code_name()
        print(f"    获得 {len(df_a)} 只 A股")

        data_a = []
        for _, row in df_a.iterrows():
            symbol = str(row.get("code", "")).zfill(6)
            ts_code = get_a_stock_code(symbol)
            data_a.append((
                ts_code, row.get("name", ""), "A",
                None, None, None, None, False, True
            ))

        execute_values(cursor, """
            INSERT INTO stock_basic (symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active)
            VALUES %s
            ON CONFLICT (symbol) DO UPDATE SET
                name = EXCLUDED.name, is_active = EXCLUDED.is_active
        """, data_a)
        print(f"    A股 已写入 {len(data_a)} 条")
    except Exception as e:
        print(f"    A股 拉取失败: {e}")

    # 港股
    try:
        print("  拉取 港股 股票列表...")
        df_hk = ak.stock_hk_spot_em()
        print(f"    获得 {len(df_hk)} 只 港股")

        data_hk = []
        for _, row in df_hk.iterrows():
            symbol = f"HK{str(row.get('代码', '')).zfill(5)}"
            data_hk.append((
                symbol, row.get("名称", ""),
                "HK", None, None, None, True, True
            ))

        execute_values(cursor, """
            INSERT INTO stock_basic (symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active)
            VALUES %s
            ON CONFLICT (symbol) DO UPDATE SET
                name = EXCLUDED.name, is_active = EXCLUDED.is_active
        """, data_hk)
        print(f"    港股 已写入 {len(data_hk)} 条")
    except Exception as e:
        print(f"    港股 拉取失败: {e}")

    # 美股
    try:
        print("  拉取 美股 股票列表...")
        df_us = ak.stock_us_spot_em()
        print(f"    获得 {len(df_us)} 只 美股")

        data_us = []
        for _, row in df_us.iterrows():
            symbol = f"US{str(row.get('代码', ''))}"
            data_us.append((
                symbol, row.get("名称", ""),
                "US", None, None, None, False, True
            ))

        execute_values(cursor, """
            INSERT INTO stock_basic (symbol, name, exchange, industry, list_date, total_shares, float_shares, is_st, is_active)
            VALUES %s
            ON CONFLICT (symbol) DO UPDATE SET
                name = EXCLUDED.name, is_active = EXCLUDED.is_active
        """, data_us)
        print(f"    美股 已写入 {len(data_us)} 条")
    except Exception as e:
        print(f"    美股 拉取失败: {e}")

    conn.commit()
    cursor.close()
    return 1


def sync_daily_a_stock(conn, start_date: str = None, end_date: str = None) -> int:
    """同步 A股 日线数据"""
    print(f"正在拉取 A股 日线数据...")

    if not start_date:
        start_date = (datetime.now() - timedelta(days=3*365)).strftime("%Y%m%d")
    if not end_date:
        end_date = datetime.now().strftime("%Y%m%d")

    try:
        cursor = conn.cursor()
        cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'A'")
        codes = [row[0] for row in cursor.fetchall()]
        print(f"  共 {len(codes)} 只 A股")

        total_count = 0
        for i, symbol in enumerate(codes):
            try:
                df = ak.stock_zh_a_hist(symbol=symbol, start_date=start_date, end_date=end_date, adjust="qfq")

                if df is None or df.empty:
                    continue

                data = []
                for _, row in df.iterrows():
                    dt = pd.to_datetime(row["日期"])
                    close_price = float(row["close"])
                    change_pct = float(row.get("涨跌幅", 0))
                    pre_close = close_price / (1 + change_pct / 100) if change_pct != 0 else close_price
                    data.append((
                        symbol, dt,
                        float(row["开盘"]), float(row["收盘"]),
                        float(row["最高"]), float(row["最低"]),
                        int(row["成交量"]),
                        float(row.get("成交额", 0)),
                        pre_close,
                        float(row.get("涨跌幅", 0))
                    ))

                if data:
                    execute_values(cursor, """
                        INSERT INTO daily_bars (symbol, datetime, open, close, high, low, volume, amount, pre_close, change_pct)
                        VALUES %s
                        ON CONFLICT (symbol, datetime) DO UPDATE SET
                            open = EXCLUDED.open, close = EXCLUDED.close,
                            high = EXCLUDED.high, low = EXCLUDED.low,
                            volume = EXCLUDED.volume, amount = EXCLUDED.amount,
                            pre_close = EXCLUDED.pre_close, change_pct = EXCLUDED.change_pct
                    """, data)
                    total_count += len(data)

                if (i + 1) % 100 == 0:
                    conn.commit()
                    print(f"  进度: {i+1}/{len(codes)}, 已插入 {total_count} 条")

            except Exception as e:
                continue

        conn.commit()
        cursor.close()
        print(f"  A股 日线数据完成: {total_count} 条")
        return total_count

    except Exception as e:
        print(f"  A股 日线数据失败: {e}")
        return 0


def sync_daily_hk(conn, start_date: str = None, end_date: str = None) -> int:
    """同步 港股 日线数据"""
    print(f"正在拉取 港股 日线数据...")

    if not start_date:
        start_date = (datetime.now() - timedelta(days=3*365)).strftime("%Y%m%d")
    if not end_date:
        end_date = datetime.now().strftime("%Y%m%d")

    try:
        cursor = conn.cursor()
        cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'HK'")
        codes = [row[0] for row in cursor.fetchall()]
        print(f"  共 {len(codes)} 只 港股")

        total_count = 0
        for i, symbol in enumerate(codes):
            try:
                # 去掉 HK 前缀
                code = symbol[2:]
                df = ak.stock_hk_daily(symbol=code, start_date=start_date, end_date=end_date, adjust="qfq")

                if df is None or df.empty:
                    continue

                data = []
                for _, row in df.iterrows():
                    dt = pd.to_datetime(row["date"])
                    pre_close = float(row.get("close", row["open"]))
                    change = (float(row["close"]) - pre_close) / pre_close * 100 if pre_close else 0
                    data.append((
                        symbol, dt,
                        float(row["open"]), float(row["close"]),
                        float(row["high"]), float(row["low"]),
                        int(row["volume"]), 0.0,
                        pre_close, change
                    ))

                if data:
                    execute_values(cursor, """
                        INSERT INTO daily_bars (symbol, datetime, open, close, high, low, volume, amount, pre_close, change_pct)
                        VALUES %s
                        ON CONFLICT (symbol, datetime) DO UPDATE SET
                            open = EXCLUDED.open, close = EXCLUDED.close,
                            high = EXCLUDED.high, low = EXCLUDED.low,
                            volume = EXCLUDED.volume
                    """, data)
                    total_count += len(data)

                if (i + 1) % 100 == 0:
                    conn.commit()
                    print(f"  进度: {i+1}/{len(codes)}, 已插入 {total_count} 条")

            except Exception as e:
                continue

        conn.commit()
        cursor.close()
        print(f"  港股 日线数据完成: {total_count} 条")
        return total_count

    except Exception as e:
        print(f"  港股 日线数据失败: {e}")
        return 0


def sync_daily_us(conn, start_date: str = None, end_date: str = None) -> int:
    """同步 美股 日线数据"""
    print(f"正在拉取 美股 日线数据...")

    if not start_date:
        start_date = (datetime.now() - timedelta(days=3*365)).strftime("%Y%m%d")
    if not end_date:
        end_date = datetime.now().strftime("%Y%m%d")

    try:
        cursor = conn.cursor()
        cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'US'")
        codes = [row[0] for row in cursor.fetchall()]
        print(f"  共 {len(codes)} 只 美股")

        total_count = 0
        for i, symbol in enumerate(codes):
            try:
                # 去掉 US 前缀
                code = symbol[2:]
                df = ak.stock_us_hist(symbol=code, start_date=start_date, end_date=end_date, adjust="qfq")

                if df is None or df.empty:
                    continue

                data = []
                for _, row in df.iterrows():
                    dt = pd.to_datetime(row["日期"])
                    pre_close = float(row.get("收盘", row["开盘"])) / (1 + float(row.get("涨跌幅", 0)) / 100)
                    data.append((
                        symbol, dt,
                        float(row["开盘"]), float(row["收盘"]),
                        float(row["最高"]), float(row["最低"]),
                        int(row["成交量"]),
                        float(row.get("成交额", 0)),
                        pre_close,
                        float(row.get("涨跌幅", 0))
                    ))

                if data:
                    execute_values(cursor, """
                        INSERT INTO daily_bars (symbol, datetime, open, close, high, low, volume, amount, pre_close, change_pct)
                        VALUES %s
                        ON CONFLICT (symbol, datetime) DO UPDATE SET
                            open = EXCLUDED.open, close = EXCLUDED.close,
                            high = EXCLUDED.high, low = EXCLUDED.low,
                            volume = EXCLUDED.volume, amount = EXCLUDED.amount,
                            pre_close = EXCLUDED.pre_close, change_pct = EXCLUDED.change_pct
                    """, data)
                    total_count += len(data)

                if (i + 1) % 100 == 0:
                    conn.commit()
                    print(f"  进度: {i+1}/{len(codes)}, 已插入 {total_count} 条")

            except Exception as e:
                continue

        conn.commit()
        cursor.close()
        print(f"  美股 日线数据完成: {total_count} 条")
        return total_count

    except Exception as e:
        print(f"  美股 日线数据失败: {e}")
        return 0


def sync_minute_a_stock(conn, period: str = "5") -> int:
    """同步 A股 分钟级数据"""
    print(f"正在拉取 A股 {period}分钟级数据...")

    try:
        cursor = conn.cursor()
        cursor.execute("SELECT symbol FROM stock_basic WHERE exchange = 'A'")
        codes = [row[0] for row in cursor.fetchall()]
        print(f"  共 {len(codes)} 只 A股")

        total_count = 0
        end_date = datetime.now().strftime("%Y%m%d")
        start_date = (datetime.now() - timedelta(days=5)).strftime("%Y%m%d")

        for i, symbol in enumerate(codes):
            try:
                df = ak.stock_zh_a_min(symbol=symbol, start_date=start_date, end_date=end_date, period=f"{period}m", adjust="qfq")

                if df is None or df.empty:
                    continue

                data = []
                for _, row in df.iterrows():
                    dt = pd.to_datetime(row["时间"])
                    data.append((
                        symbol, dt,
                        float(row["开盘"]), float(row["收盘"]),
                        float(row["最高"]), float(row["最低"]),
                        int(row["成交量"]), 0.0
                    ))

                if data:
                    execute_values(cursor, """
                        INSERT INTO minute_bars (symbol, datetime, open, high, low, close, volume, amount)
                        VALUES %s
                        ON CONFLICT (symbol, datetime) DO UPDATE SET
                            open = EXCLUDED.open, close = EXCLUDED.close,
                            high = EXCLUDED.high, low = EXCLUDED.low,
                            volume = EXCLUDED.volume, amount = EXCLUDED.amount
                    """, data)
                    total_count += len(data)

                if (i + 1) % 50 == 0:
                    conn.commit()
                    print(f"  进度: {i+1}/{len(codes)}, 已插入 {total_count} 条")

            except Exception as e:
                continue

        conn.commit()
        cursor.close()
        print(f"  A股 {period}分钟数据完成: {total_count} 条")
        return total_count

    except Exception as e:
        print(f"  A股 分钟数据失败: {e}")
        return 0


def main():
    parser = argparse.ArgumentParser(description="ACDA-Q 数据拉取脚本")
    parser.add_argument("--market", choices=["a", "hk", "us", "all"], default="all",
                        help="市场: a(A股), hk(港股), us(美股), all(全部)")
    parser.add_argument("--data-type", choices=["basic", "daily", "minute", "all"], default="all",
                        help="数据类型: basic(股票列表), daily(日线), minute(分钟), all(全部)")
    parser.add_argument("--period", default="5", help="分钟周期: 1, 5, 15, 30, 60 (默认5)")
    parser.add_argument("--start-date", default=None, help="开始日期 YYYYMMDD")
    parser.add_argument("--end-date", default=None, help="结束日期 YYYYMMDD")

    args = parser.parse_args()

    print("=" * 60)
    print("ACDA-Q 数据拉取脚本 (AkShare)")
    print("=" * 60)

    conn = get_db_connection()

    if args.data_type in ["basic", "all"]:
        sync_stock_basic(conn)

    if args.data_type in ["daily", "all"]:
        if args.market in ["a", "all"]:
            sync_daily_a_stock(conn, args.start_date, args.end_date)
        if args.market in ["hk", "all"]:
            sync_daily_hk(conn, args.start_date, args.end_date)
        if args.market in ["us", "all"]:
            sync_daily_us(conn, args.start_date, args.end_date)

    if args.data_type in ["minute", "all"]:
        if args.market in ["a", "all"]:
            sync_minute_a_stock(conn, args.period)

    conn.close()
    print("=" * 60)
    print("数据拉取完成!")
    print("=" * 60)


if __name__ == "__main__":
    main()
