"""
TimescaleDB 连接与行情数据查询。
兼容 Python 3.11.8
"""
from typing import List, Optional
import pandas as pd
from sqlalchemy import create_engine, text
from server.api.core.config import get_settings

settings = get_settings()

# TimescaleDB 使用同步连接（数据查询以同步为主）
_ts_engine = None


def get_ts_engine():
    global _ts_engine
    if _ts_engine is None:
        base_url = getattr(settings, "TIMESCALE_DATABASE_URL", None)
        if not base_url:
            # 本地开发回退推断
            base_url = settings.SYNC_DATABASE_URL.replace("quant_db", "quant_market")
        # 同步引擎必须使用 psycopg2 驱动，而非 asyncpg
        sync_url = base_url.replace("postgresql+asyncpg://", "postgresql+psycopg2://")
        if sync_url == base_url and "+" not in sync_url.split("://")[0]:
            sync_url = sync_url.replace("postgresql://", "postgresql+psycopg2://")
        _ts_engine = create_engine(
            sync_url,
            pool_size=5,
            max_overflow=10,
            pool_pre_ping=True,
            pool_recycle=3600,
        )
    return _ts_engine


def query_daily_bars(
    symbols: List[str],
    start_date: str,
    end_date: str,
) -> pd.DataFrame:
    """从 TimescaleDB 查询日K线数据"""
    if not symbols:
        return pd.DataFrame(columns=["symbol", "datetime", "open", "high", "low", "close", "volume"])

    engine = get_ts_engine()
    sql = text("""
        SELECT symbol, datetime, open, high, low, close, volume, pre_close
        FROM daily_bars
        WHERE symbol = ANY(:symbols)
          AND datetime BETWEEN :start AND :end
        ORDER BY datetime, symbol
    """)

    with engine.connect() as conn:
        df = pd.read_sql(sql, conn, params={"symbols": symbols, "start": start_date, "end": end_date})

    if not df.empty:
        df["datetime"] = pd.to_datetime(df["datetime"])
    return df


def query_stock_list(exchange: Optional[str] = None, search: Optional[str] = None, limit: int = 5000) -> pd.DataFrame:
    """查询股票基础列表"""
    engine = get_ts_engine()
    sql = "SELECT symbol, name, exchange, industry, list_date, is_active FROM stock_basic"
    params = {}
    conditions = []
    if exchange:
        conditions.append("exchange = :exchange")
        params["exchange"] = exchange
    if search:
        conditions.append("(symbol ILIKE :search OR name ILIKE :search)")
        params["search"] = f"%{search}%"
    if conditions:
        sql += " WHERE " + " AND ".join(conditions)
    params["limit"] = limit
    sql += " ORDER BY symbol LIMIT :limit"

    with engine.connect() as conn:
        return pd.read_sql(text(sql), conn, params=params)


def get_adj_price(
    symbols: List[str],
    start_date: str,
    end_date: str,
    adj_type: str = "qfq",
) -> pd.DataFrame:
    """
    查询复权价格。
    adj_type: "qfq" (前复权) / "hfq" (后复权) / "none" (不复权)
    前复权 = close * adj_factor / 最新adj_factor
    后复权 = close * adj_factor / 上市首日adj_factor (简化为 close * adj_factor)
    """
    if adj_type == "none":
        return query_daily_bars(symbols, start_date, end_date)

    engine = get_ts_engine()
    # 拉取原始价格和复权因子
    sql = text("""
        SELECT
            d.symbol,
            d.datetime,
            d.open,
            d.high,
            d.low,
            d.close,
            d.volume,
            d.pre_close,
            a.adj_factor
        FROM daily_bars d
        LEFT JOIN adj_factors a
            ON d.symbol = a.symbol AND DATE(d.datetime) = a.trade_date
        WHERE d.symbol = ANY(:symbols)
          AND d.datetime BETWEEN :start AND :end
        ORDER BY d.datetime, d.symbol
    """)

    with engine.connect() as conn:
        df = pd.read_sql(sql, conn, params={"symbols": symbols, "start": start_date, "end": end_date})

    if df.empty:
        return df

    df["datetime"] = pd.to_datetime(df["datetime"])

    if adj_type == "qfq":
        # 前复权：用最新复权因子作为基准
        latest_factors = (
            df.groupby("symbol")["adj_factor"].last().reset_index()
            .rename(columns={"adj_factor": "latest_factor"})
        )
        df = df.merge(latest_factors, on="symbol", how="left")
        factor = df["adj_factor"] / df["latest_factor"]
        df["open"] = df["open"] * factor
        df["high"] = df["high"] * factor
        df["low"] = df["low"] * factor
        df["close"] = df["close"] * factor
        df = df.drop(columns=["adj_factor", "latest_factor"])
    elif adj_type == "hfq":
        # 后复权：直接乘以复权因子
        factor = df["adj_factor"].fillna(1.0)
        df["open"] = df["open"] * factor
        df["high"] = df["high"] * factor
        df["low"] = df["low"] * factor
        df["close"] = df["close"] * factor
        df = df.drop(columns=["adj_factor"])

    return df
