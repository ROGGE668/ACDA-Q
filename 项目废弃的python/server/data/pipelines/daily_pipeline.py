"""
每日数据管道：收盘后自动执行增量同步
可接入 Airflow / Celery Beat 定时调度
兼容 Python 3.11.8
"""
import os
import sys
import logging
from datetime import datetime, timedelta
from typing import Optional

# 将 server 加入路径
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../../"))

from server.data.syncers.tushare_syncer import TushareSyncer
from server.data.syncers.akshare_syncer import AKShareSyncer

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
logger = logging.getLogger(__name__)


def run_daily_pipeline(trade_date: Optional[str] = None, source: str = "tushare"):
    """
    每日收盘后增量同步：
    1. 同步当日日K
    2. 同步复权因子（Tushare）
    3. 更新 Redis 缓存热点数据
    """
    if trade_date is None:
        # 默认取最近一个工作日
        today = datetime.now()
        if today.weekday() >= 5:
            today = today - timedelta(days=today.weekday() - 4)
        trade_date = today.strftime("%Y%m%d")

    logger.info("[Pipeline] Starting daily sync for %s via %s", trade_date, source)

    if source == "tushare":
        syncer = TushareSyncer()
        try:
            syncer.sync_daily_incremental(trade_date)
        except Exception as exc:
            logger.error("[Pipeline] Tushare failed: %s, falling back to AKShare", exc)
            syncer = AKShareSyncer()
            syncer.sync_daily_incremental(trade_date)
    else:
        syncer = AKShareSyncer()
        syncer.sync_daily_incremental(trade_date)

    logger.info("[Pipeline] Daily sync completed for %s", trade_date)


def run_full_historical_sync(symbols: Optional[list] = None, years: int = 5):
    """
    全量历史数据同步（首次部署时使用）。
    """
    end_date = datetime.now().strftime("%Y%m%d")
    start_date = (datetime.now() - timedelta(days=365 * years)).strftime("%Y%m%d")

    logger.info("[Pipeline] Starting full historical sync: %s ~ %s", start_date, end_date)

    syncer = TushareSyncer()
    try:
        syncer.sync_stock_list()
        syncer.sync_daily_bars(symbols=symbols, start_date=start_date, end_date=end_date)
        syncer.sync_adj_factors(symbols=symbols, start_date=start_date, end_date=end_date)
    except Exception as exc:
        logger.error("[Pipeline] Tushare full sync failed: %s", exc)
        syncer = AKShareSyncer()
        syncer.sync_stock_list()
        syncer.sync_daily_bars(symbols=symbols, start_date=start_date, end_date=end_date)

    logger.info("[Pipeline] Full historical sync completed")


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Quant data pipeline")
    parser.add_argument("--mode", choices=["daily", "full"], default="daily", help="Sync mode")
    parser.add_argument("--date", help="Trade date (YYYYMMDD)")
    parser.add_argument("--source", choices=["tushare", "akshare"], default="tushare", help="Data source")
    parser.add_argument("--years", type=int, default=5, help="Years for full sync")
    args = parser.parse_args()

    if args.mode == "daily":
        run_daily_pipeline(trade_date=args.date, source=args.source)
    else:
        run_full_historical_sync(years=args.years)
