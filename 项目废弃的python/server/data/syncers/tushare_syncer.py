"""
Tushare 数据同步器
负责从 Tushare Pro API 拉取 A 股历史行情、基础信息等数据，写入 TimescaleDB。
兼容 Python 3.11.8
"""
from typing import List, Optional
import os
import time
import logging

import pandas as pd
from sqlalchemy import create_engine, text
from sqlalchemy.engine import Engine

logger = logging.getLogger(__name__)


class TushareSyncer:
    """Tushare Pro 数据同步器"""

    def __init__(self, api_token: Optional[str] = None, db_url: Optional[str] = None):
        self.api_token = api_token or os.getenv("TUSHARE_TOKEN", "")
        self._pro = None

        if db_url is None:
            db_url = os.getenv(
                "TIMESCALE_DATABASE_URL", "postgresql://quant:quant123@localhost:5433/quant_market")
        self.engine: Engine = create_engine(db_url)

    @property
    def pro(self):
        """懒加载 Tushare Pro API"""
        if self._pro is None:
            try:
                import tushare as ts
                if not self.api_token:
                    raise ValueError("TUSHARE_TOKEN not set")
                self._pro = ts.pro_api(self.api_token)
            except ImportError:
                raise ImportError("tushare not installed, run: pip install tushare")
        return self._pro

    # ------------------------------------------------------------------
    # 股票基础信息
    # ------------------------------------------------------------------
    def sync_stock_list(self) -> int:
        """同步 A 股上市股票基础信息到 stock_basic 表"""
        logger.info("[Tushare] Syncing stock_basic ...")
        try:
            df = self.pro.stock_basic(exchange='', list_status='L')
        except Exception as exc:
            logger.error("[Tushare] stock_basic failed: %s", exc)
            return 0

        if df is None or df.empty:
            logger.warning("[Tushare] stock_basic returned empty")
            return 0

        df = df.rename(columns={
            "ts_code": "symbol",
            "name": "name",
            "exchange": "exchange",
            "industry": "industry",
            "list_date": "list_date",
            "total_share": "total_shares",
            "float_share": "float_shares",
        })[["symbol", "name", "exchange", "industry", "list_date", "total_shares", "float_shares", "is_st", "is_active"]]
        # Tushare 返回的是万股，转换为股
        for col in ["total_shares", "float_shares"]:
            if col in df.columns:
                df[col] = pd.to_numeric(df[col], errors="coerce") * 10000
        df["is_st"] = df["name"].str.contains(r"\*?ST", case=False, na=False)
        df["is_active"] = True

        # 写入数据库（upsert）
        with self.engine.begin() as conn:
            conn.execute(text("TRUNCATE TABLE stock_basic RESTART IDENTITY CASCADE"))
            df.to_sql("stock_basic", conn, if_exists="append", index=False)

        logger.info("[Tushare] stock_basic synced: %d rows", len(df))
        return len(df)

    # ------------------------------------------------------------------
    # 日K线
    # ------------------------------------------------------------------
    def sync_daily_bars(
        self,
        symbols: Optional[List[str]] = None,
        start_date: Optional[str] = None,
        end_date: Optional[str] = None,
        batch_mode: bool = True,
    ) -> int:
        """
        同步日K线数据到 daily_bars hypertable。
        如果不传 symbols 且 batch_mode=True，使用 Tushare 全市场批量接口（每日接口不传 ts_code）。
        """
        if end_date is None:
            end_date = pd.Timestamp.now().strftime("%Y%m%d")
        if start_date is None:
            start_date = (pd.Timestamp.now() - pd.Timedelta(days=365)).strftime("%Y%m%d")

        if symbols is None and batch_mode:
            # 批量模式：按交易日拉取全市场数据（效率更高）
            return self._sync_daily_bars_batch(start_date, end_date)

        if symbols is None:
            with self.engine.connect() as conn:
                res = conn.execute(text("SELECT symbol FROM stock_basic WHERE is_active = TRUE"))
                symbols = [row[0] for row in res.fetchall()]
            logger.info("[Tushare] Full market sync (per-symbol): %d symbols", len(symbols))

        total = 0
        for symbol in symbols:
            try:
                df = self.pro.daily(ts_code=symbol, start_date=start_date, end_date=end_date)
                if df is None or df.empty:
                    continue
                total += self._insert_daily_bars(df)
                time.sleep(0.15)  # Tushare 免费版限频 ~600/分钟
            except Exception as exc:
                logger.error("[Tushare] daily %s failed: %s", symbol, exc)

        logger.info("[Tushare] daily_bars synced: %d rows", total)
        return total

    def _sync_daily_bars_batch(self, start_date: str, end_date: str) -> int:
        """使用 Tushare daily 接口不传 ts_code 的批量模式，按交易日拉取。"""
        trade_dates = pd.bdate_range(start=pd.to_datetime(start_date), end=pd.to_datetime(end_date)).strftime("%Y%m%d").tolist()
        logger.info("[Tushare] Batch sync for %d trade dates", len(trade_dates))
        total = 0
        for trade_date in trade_dates:
            try:
                df = self.pro.daily(trade_date=trade_date)
                if df is None or df.empty:
                    continue
                total += self._insert_daily_bars(df)
                time.sleep(0.15)
            except Exception as exc:
                logger.error("[Tushare] batch daily %s failed: %s", trade_date, exc)
        logger.info("[Tushare] daily_bars batch synced: %d rows", total)
        return total

    def _insert_daily_bars(self, df: pd.DataFrame) -> int:
        """将 Tushare daily 原始 DataFrame 标准化后写入数据库"""
        df = df.rename(columns={
            "ts_code": "symbol",
            "trade_date": "datetime",
            "open": "open",
            "high": "high",
            "low": "low",
            "close": "close",
            "vol": "volume",
            "amount": "amount",
            "pre_close": "pre_close",
            "pct_chg": "change_pct",
        })
        df["datetime"] = pd.to_datetime(df["datetime"])
        df = df[["symbol", "datetime", "open", "high", "low", "close",
                 "volume", "amount", "pre_close", "change_pct"]]

        # 数据质量告警：非新股单日涨跌幅超过 30%
        for _, row in df.iterrows():
            pre_close = pd.to_numeric(row.get("pre_close"), errors="coerce")
            change_pct = pd.to_numeric(row.get("change_pct"), errors="coerce")
            if pre_close and pre_close > 0 and change_pct and abs(change_pct) > 30:
                logger.warning(
                    "[DataQuality] Abnormal price change: %s on %s, pre_close=%s, change_pct=%s%%",
                    row["symbol"], row["datetime"], pre_close, change_pct,
                )

        # 批量模式下 symbol 不唯一，按 symbol 分组删除旧数据
        symbols = df["symbol"].unique().tolist()
        min_date = df["datetime"].min().date()
        max_date = df["datetime"].max().date()
        with self.engine.connect() as conn:
            conn.execute(
                text("""
                    DELETE FROM daily_bars
                    WHERE symbol = ANY(:symbols)
                      AND datetime >= :min_date
                      AND datetime < :max_date + INTERVAL '1 day'
                """),
                {"symbols": symbols, "min_date": min_date, "max_date": max_date},
            )
            df.to_sql("daily_bars", conn, if_exists="append", index=False)
            conn.commit()

        return len(df)

    # ------------------------------------------------------------------
    # 复权因子
    # ------------------------------------------------------------------
    def sync_adj_factors(
        self,
        symbols: Optional[List[str]] = None,
        start_date: Optional[str] = None,
        end_date: Optional[str] = None,
    ) -> int:
        """同步复权因子"""
        if end_date is None:
            end_date = pd.Timestamp.now().strftime("%Y%m%d")
        if start_date is None:
            start_date = "20000101"

        if symbols is None:
            with self.engine.connect() as conn:
                res = conn.execute(text("SELECT symbol FROM stock_basic WHERE is_active = TRUE"))
                symbols = [row[0] for row in res.fetchall()]

        total = 0
        for symbol in symbols:
            try:
                df = self.pro.adj_factor(ts_code=symbol, start_date=start_date, end_date=end_date)
                if df is None or df.empty:
                    continue
                df = df.rename(columns={"ts_code": "symbol", "trade_date": "trade_date", "adj_factor": "adj_factor"})
                df["trade_date"] = pd.to_datetime(df["trade_date"]).dt.date
                with self.engine.connect() as conn:
                    conn.execute(
                        text("DELETE FROM adj_factors WHERE symbol = :symbol AND trade_date = ANY(:dates)"),
                        {"symbol": symbol, "dates": df["trade_date"].tolist()},
                    )
                    df.to_sql("adj_factors", conn, if_exists="append", index=False)
                    conn.commit()
                total += len(df)
                time.sleep(0.15)
            except Exception as exc:
                logger.error("[Tushare] adj_factor %s failed: %s", symbol, exc)

        logger.info("[Tushare] adj_factors synced: %d rows", total)
        return total

    # ------------------------------------------------------------------
    # 每日增量
    # ------------------------------------------------------------------
    def sync_daily_incremental(self, trade_date: Optional[str] = None) -> int:
        """
        每日收盘后增量同步：
        1. 更新 stock_basic（处理新股上市/退市）
        2. 同步当日 daily_bars
        3. 同步当日 adj_factors
        """
        if trade_date is None:
            # 取最近一个交易日
            trade_date = (pd.Timestamp.now() - pd.Timedelta(days=1)).strftime("%Y%m%d")

        logger.info("[Tushare] Daily incremental for %s", trade_date)
        self.sync_stock_list()
        count = self.sync_daily_bars(start_date=trade_date, end_date=trade_date)
        self.sync_adj_factors(start_date=trade_date, end_date=trade_date)
        return count
