"""
数据加载器：支持 TimescaleDB、CSV、内存模拟数据。
兼容 Python 3.11.8
"""
from typing import List, Optional
import pandas as pd
import numpy as np
import os


class DataFeed:
    def __init__(self, data_dir: Optional[str] = None, use_db: bool = True):
        self.data_dir = data_dir or os.path.join(
            os.path.dirname(__file__), "../../../data/samples"
        )
        self.use_db = use_db

    def load_bars(
        self,
        symbols: List[str],
        start_date: str,
        end_date: str,
        freq: str = "1d",
    ) -> pd.DataFrame:
        """
        加载历史K线数据。优先级：
        1. TimescaleDB（生产环境，use_db=True）
        2. 本地CSV缓存
        3. 内存模拟数据（MVP测试用）
        """
        combined = pd.DataFrame()
        if self.use_db:
            try:
                from server.api.core.timescale import get_adj_price
                # 默认使用前复权价格回测
                df = get_adj_price(symbols, start_date, end_date, adj_type="qfq")
                if not df.empty:
                    combined = df
            except ImportError:
                pass
            except Exception as e:
                import logging
                logging.getLogger(__name__).warning(
                    "Database query failed, falling back to CSV/mock data: %s", e
                )

        if combined.empty:
            frames = []
            for symbol in symbols:
                df = self._load_symbol_bars(symbol, start_date, end_date, freq)
                if not df.empty:
                    df["symbol"] = symbol
                    frames.append(df)

            if not frames:
                return pd.DataFrame(
                    columns=["symbol", "datetime", "open", "high", "low", "close", "volume"]
                )

            combined = pd.concat(frames, ignore_index=True)
            combined["datetime"] = pd.to_datetime(combined["datetime"])
            combined = combined.sort_values(["datetime", "symbol"]).reset_index(drop=True)

        # 合并 ST 状态信息（从 stock_basic 查询当前状态作为近似）
        if self.use_db:
            try:
                from server.api.core.timescale import get_ts_engine
                from sqlalchemy import text
                ts_engine = get_ts_engine()
                sym_list = combined["symbol"].unique().tolist()
                with ts_engine.connect() as conn:
                    result = conn.execute(
                        text("SELECT symbol, is_st FROM stock_basic WHERE symbol = ANY(:symbols)"),
                        {"symbols": sym_list},
                    )
                    st_map = {row[0]: row[1] for row in result}
                combined["is_st"] = combined["symbol"].map(st_map).fillna(False)
            except Exception:
                combined["is_st"] = False
        else:
            combined["is_st"] = False

        # 停牌检测：基于全量交易日历计算各 symbol 的缺失天数并记录日志
        if not combined.empty and len(symbols) > 0:
            trading_dates = combined["datetime"].dt.date.unique()
            for sym in symbols:
                sym_dates = combined.loc[combined["symbol"] == sym, "datetime"].dt.date.unique()
                missing = len(trading_dates) - len(sym_dates)
                if missing > len(trading_dates) * 0.5:
                    import logging
                    logging.getLogger(__name__).warning(
                        "[Suspension] %s is missing %d/%d trading days (possible long-term suspension)",
                        sym, missing, len(trading_dates),
                    )

        return combined

    def _load_symbol_bars(
        self, symbol: str, start_date: str, end_date: str, freq: str
    ) -> pd.DataFrame:
        # 尝试本地CSV
        csv_path = os.path.join(self.data_dir, f"{symbol}.csv")
        if os.path.exists(csv_path):
            df = pd.read_csv(csv_path, parse_dates=["datetime"])
            mask = (df["datetime"] >= start_date) & (df["datetime"] <= end_date)
            return df.loc[mask].copy()

        # 回退到模拟数据
        return self._generate_mock_bars(symbol, start_date, end_date)

    def _generate_mock_bars(
        self, symbol: str, start_date: str, end_date: str, seed: Optional[int] = None
    ) -> pd.DataFrame:
        """
        生成带趋势的随机漫步模拟K线数据，用于MVP阶段无外部数据源时测试。
        seed: 随机种子，None 则使用基于 symbol 的确定性种子 + 时间偏移，保证不同运行结果不同。
        """
        start = pd.to_datetime(start_date)
        end = pd.to_datetime(end_date)
        dates = pd.date_range(start=start, end=end, freq="B")  # 仅工作日

        if seed is None:
            # 使用 symbol hash + 纳秒级时间偏移，保证不同运行结果不同
            import time
            seed = (hash(symbol) + int(time.time_ns())) % 2**31
        rng = np.random.default_rng(seed)
        n = len(dates)

        returns = rng.normal(loc=0.0005, scale=0.02, size=n)
        prices = 100 * np.exp(np.cumsum(returns))

        # 生成 OHLC
        noise = rng.uniform(0.005, 0.02, size=n)
        high = prices * (1 + noise)
        low = prices * (1 - noise)
        open_price = np.roll(prices, 1)
        open_price[0] = prices[0] * (1 + rng.normal(0, 0.01))
        pre_close = np.roll(prices, 1)
        pre_close[0] = prices[0]
        volume = rng.integers(1_000_000, 10_000_000, size=n)

        df = pd.DataFrame({
            "datetime": dates,
            "open": np.round(open_price, 2),
            "high": np.round(high, 2),
            "low": np.round(low, 2),
            "close": np.round(prices, 2),
            "volume": volume,
            "pre_close": np.round(pre_close, 2),
        })
        return df

