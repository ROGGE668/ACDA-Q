"""
AKShare 数据同步器（Tushare 的免费开源替代方案）
兼容 Python 3.11.8
"""
from typing import List, Optional
import os
import time
import logging

import pandas as pd
import numpy as np
from sqlalchemy import create_engine, text
from sqlalchemy.engine import Engine

logger = logging.getLogger(__name__)


class AKShareSyncer:
    """AKShare 数据同步器——功能对标 TushareSyncer"""

    def __init__(self, db_url: Optional[str] = None):
        if db_url is None:
            db_url = os.getenv(
                "TIMESCALE_DATABASE_URL", "postgresql://quant:quant123@localhost:5433/quant_market"
            )
        self.engine: Engine = create_engine(
            db_url,
            pool_size=3,
            max_overflow=5,
            pool_pre_ping=True,
        )

    def _ak(self):
        """懒加载 akshare"""
        try:
            import akshare as ak
            return ak
        except ImportError:
            raise ImportError("akshare not installed, run: pip install akshare")

    # ------------------------------------------------------------------
    # 股票基础信息
    # ------------------------------------------------------------------
    def sync_stock_list(self) -> int:
        """同步 A 股上市股票基础信息到 stock_basic 表"""
        logger.info("[AKShare] Syncing stock_basic ...")
        ak = self._ak()
        try:
            # 东方财富 A 股实时行情（含基础信息）
            df_spot = ak.stock_zh_a_spot_em()
            if df_spot is None or df_spot.empty:
                raise ValueError("Empty spot data")
        except Exception as exc:
            logger.error("[AKShare] stock_basic spot failed: %s", exc)
            # fallback: 使用 stock_info_a_code_name
            try:
                df_spot = ak.stock_info_a_code_name()
                if df_spot is None or df_spot.empty:
                    raise ValueError("Empty stock_info")
            except Exception as exc2:
                logger.error("[AKShare] stock_basic fallback also failed: %s", exc2)
                return 0

        # 字段标准化
        if "代码" in df_spot.columns:
            df_spot = df_spot.rename(columns={"代码": "symbol", "名称": "name"})
        elif "CODE" in df_spot.columns:
            df_spot = df_spot.rename(columns={"CODE": "symbol", "NAME": "name"})
        elif "code" in df_spot.columns:
            df_spot = df_spot.rename(columns={"code": "symbol", "name": "name"})

        df_spot = df_spot[["symbol", "name"]].copy()
        df_spot["exchange"] = df_spot["symbol"].apply(
            lambda x: "SSE" if str(x).startswith(("6", "5")) else "SZSE"
        )
        df_spot["industry"] = None
        df_spot["list_date"] = None
        df_spot["total_shares"] = None
        df_spot["float_shares"] = None
        df_spot["is_st"] = df_spot["name"].str.contains(r"\*?ST", case=False, na=False)
        df_spot["is_active"] = True

        # 尝试补充总股本和流通股本（需要额外接口）
        try:
            df_info = ak.stock_individual_info_em()
            if df_info is not None and not df_info.empty:
                info_map = df_info.set_index("股票代码").to_dict("index")
                df_spot["total_shares"] = df_spot["symbol"].map(
                    lambda s: _parse_shares(info_map.get(s, {}).get("总股本"))
                )
                df_spot["float_shares"] = df_spot["symbol"].map(
                    lambda s: _parse_shares(info_map.get(s, {}).get("流通股本"))
                )
        except Exception as exc:
            logger.warning("[AKShare] Failed to enrich shares info: %s", exc)

        # Upsert：先标记全部 inactive，再更新存在的为 active
        with self.engine.begin() as conn:
            conn.execute(text("UPDATE stock_basic SET is_active = FALSE"))
            for _, row in df_spot.iterrows():
                conn.execute(
                    text("""
                        INSERT INTO stock_basic (
                            symbol, name, exchange, industry, list_date,
                            total_shares, float_shares, is_st, is_active
                        )
                        VALUES (
                            :symbol, :name, :exchange, :industry, :list_date,
                            :total_shares, :float_shares, :is_st, TRUE
                        )
                        ON CONFLICT (symbol) DO UPDATE SET
                            name = EXCLUDED.name,
                            exchange = EXCLUDED.exchange,
                            industry = EXCLUDED.industry,
                            list_date = EXCLUDED.list_date,
                            total_shares = EXCLUDED.total_shares,
                            float_shares = EXCLUDED.float_shares,
                            is_st = EXCLUDED.is_st,
                            is_active = TRUE
                    """),
                    row.to_dict(),
                )

        logger.info("[AKShare] stock_basic synced: %d rows", len(df_spot))
        return len(df_spot)

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
        AKShare 的 stock_zh_a_hist 接口支持按 symbol 拉取历史数据。
        batch_mode 对 AKShare 无效（无全市场批量接口），仅保持 API 兼容。
        """
        if end_date is None:
            end_date = pd.Timestamp.now().strftime("%Y%m%d")
        if start_date is None:
            start_date = (pd.Timestamp.now() - pd.Timedelta(days=365)).strftime("%Y%m%d")

        # AKShare 日期格式为 YYYYMMDD
        start_fmt = start_date
        end_fmt = end_date

        if symbols is None:
            with self.engine.connect() as conn:
                res = conn.execute(text("SELECT symbol FROM stock_basic WHERE is_active = TRUE"))
                symbols = [row[0] for row in res.fetchall()]
            logger.info("[AKShare] Full market sync: %d symbols", len(symbols))

        ak = self._ak()
        total = 0
        failed_symbols = []

        for symbol in symbols:
            try:
                df = ak.stock_zh_a_hist(
                    symbol=symbol,
                    period="daily",
                    start_date=start_fmt,
                    end_date=end_fmt,
                    adjust="",  # 不复权，复权因子单独计算
                )
                if df is None or df.empty:
                    continue
                total += self._insert_daily_bars(df, symbol)
                time.sleep(0.15)  # AKShare 限频控制
            except Exception as exc:
                logger.error("[AKShare] daily %s failed: %s", symbol, exc)
                failed_symbols.append(symbol)

        if failed_symbols:
            logger.warning("[AKShare] %d symbols failed: %s", len(failed_symbols), failed_symbols[:10])

        logger.info("[AKShare] daily_bars synced: %d rows (%d symbols)", total, len(symbols))
        return total

    def _insert_daily_bars(self, df: pd.DataFrame, symbol: str) -> int:
        """将 AKShare 原始 DataFrame 标准化后写入数据库"""
        column_map = {
            "日期": "datetime",
            "开盘": "open",
            "收盘": "close",
            "最高": "high",
            "最低": "low",
            "成交量": "volume",
            "成交额": "amount",
        }
        # 只保留存在的列
        rename_map = {k: v for k, v in column_map.items() if k in df.columns}
        df = df.rename(columns=rename_map).copy()

        # 确保必要列存在
        for col in ["datetime", "open", "high", "low", "close", "volume"]:
            if col not in df.columns:
                df[col] = 0.0 if col != "datetime" else None

        df["symbol"] = symbol
        df["datetime"] = pd.to_datetime(df["datetime"])

        # 数值列清洗
        numeric_cols = ["open", "high", "low", "close", "volume"]
        for col in numeric_cols:
            df[col] = pd.to_numeric(df[col], errors="coerce")

        # 计算 pre_close 和 change_pct
        df = df.sort_values("datetime").reset_index(drop=True)
        df["pre_close"] = df["close"].shift(1)
        df.loc[0, "pre_close"] = df.loc[0, "open"] if pd.notna(df.loc[0, "open"]) else df.loc[0, "close"]
        df["change_pct"] = np.where(
            df["pre_close"] != 0,
            (df["close"] - df["pre_close"]) / df["pre_close"] * 100,
            0.0,
        )

        # 数据质量告警
        for _, row in df.iterrows():
            pre_close = pd.to_numeric(row.get("pre_close"), errors="coerce")
            change_pct = pd.to_numeric(row.get("change_pct"), errors="coerce")
            if pre_close and pre_close > 0 and change_pct and abs(change_pct) > 30:
                logger.warning(
                    "[DataQuality] Abnormal price change: %s on %s, pre_close=%s, change_pct=%s%%",
                    symbol, row["datetime"], pre_close, change_pct,
                )

        # amount 列如果不存在
        if "amount" not in df.columns:
            df["amount"] = 0.0

        df = df[[
            "symbol", "datetime", "open", "high", "low", "close",
            "volume", "amount", "pre_close", "change_pct",
        ]]

        # 删除旧数据后写入
        min_date = df["datetime"].min().date()
        max_date = df["datetime"].max().date()
        with self.engine.begin() as conn:
            conn.execute(
                text("""
                    DELETE FROM daily_bars
                    WHERE symbol = :symbol
                      AND datetime >= :min_date
                      AND datetime < :max_date + INTERVAL '1 day'
                """),
                {"symbol": symbol, "min_date": min_date, "max_date": max_date},
            )
            df.to_sql("daily_bars", conn, if_exists="append", index=False)

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
        """
        同步复权因子。AKShare 无独立复权因子接口，
        通过对比不复权 vs 前复权价格计算复权因子。
        """
        if end_date is None:
            end_date = pd.Timestamp.now().strftime("%Y%m%d")
        if start_date is None:
            start_date = "20000101"

        if symbols is None:
            with self.engine.connect() as conn:
                res = conn.execute(text("SELECT symbol FROM stock_basic WHERE is_active = TRUE"))
                symbols = [row[0] for row in res.fetchall()]

        ak = self._ak()
        total = 0
        failed_symbols = []

        for symbol in symbols:
            try:
                # 拉取不复权数据
                df_raw = ak.stock_zh_a_hist(
                    symbol=symbol,
                    period="daily",
                    start_date=start_date,
                    end_date=end_date,
                    adjust="",
                )
                # 拉取前复权数据
                df_adj = ak.stock_zh_a_hist(
                    symbol=symbol,
                    period="daily",
                    start_date=start_date,
                    end_date=end_date,
                    adjust="qfq",
                )
                if df_raw is None or df_raw.empty or df_adj is None or df_adj.empty:
                    continue

                # 合并计算复权因子
                df_raw = df_raw[["日期", "收盘"]].rename(columns={"日期": "datetime", "收盘": "close_raw"})
                df_adj = df_adj[["日期", "收盘"]].rename(columns={"日期": "datetime", "收盘": "close_adj"})
                df_raw["datetime"] = pd.to_datetime(df_raw["datetime"])
                df_adj["datetime"] = pd.to_datetime(df_adj["datetime"])
                merged = pd.merge(df_raw, df_adj, on="datetime", how="inner")

                merged["close_raw"] = pd.to_numeric(merged["close_raw"], errors="coerce")
                merged["close_adj"] = pd.to_numeric(merged["close_adj"], errors="coerce")
                merged = merged[merged["close_raw"] > 0]
                merged["adj_factor"] = merged["close_adj"] / merged["close_raw"]
                merged["trade_date"] = merged["datetime"].dt.date
                merged["symbol"] = symbol

                df_factors = merged[["symbol", "trade_date", "adj_factor"]].copy()

                # 写入数据库
                dates = df_factors["trade_date"].unique().tolist()
                with self.engine.begin() as conn:
                    conn.execute(
                        text("""
                            DELETE FROM adj_factors
                            WHERE symbol = :symbol AND trade_date = ANY(:dates)
                        """),
                        {"symbol": symbol, "dates": dates},
                    )
                    df_factors.to_sql("adj_factors", conn, if_exists="append", index=False)

                total += len(df_factors)
                time.sleep(0.2)  # 每个 symbol 请求两次，限频更严格
            except Exception as exc:
                logger.error("[AKShare] adj_factor %s failed: %s", symbol, exc)
                failed_symbols.append(symbol)

        if failed_symbols:
            logger.warning("[AKShare] %d symbols adj_factor failed", len(failed_symbols))

        logger.info("[AKShare] adj_factors synced: %d rows", total)
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
            trade_date = (pd.Timestamp.now() - pd.Timedelta(days=1)).strftime("%Y%m%d")

        logger.info("[AKShare] Daily incremental for %s", trade_date)
        self.sync_stock_list()
        count = self.sync_daily_bars(start_date=trade_date, end_date=trade_date)
        self.sync_adj_factors(start_date=trade_date, end_date=trade_date)
        return count


# ------------------------------------------------------------------
# 辅助函数
# ------------------------------------------------------------------
def _parse_shares(val) -> Optional[float]:
    """解析股本字符串（如 '12.34亿' -> 1234000000）"""
    if val is None or pd.isna(val):
        return None
    if isinstance(val, (int, float)):
        return float(val)
    val_str = str(val).strip()
    try:
        if "亿" in val_str:
            return float(val_str.replace("亿", "").replace(",", "")) * 1e8
        if "万" in val_str:
            return float(val_str.replace("万", "").replace(",", "")) * 1e4
        return float(val_str.replace(",", ""))
    except (ValueError, TypeError):
        return None
