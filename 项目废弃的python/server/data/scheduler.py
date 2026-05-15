"""
数据同步调度器（APScheduler）
每日收盘后自动执行增量数据同步。
兼容 Python 3.11.8
"""
import os
import sys
import logging
import argparse

# 将项目根目录加入路径
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../.."))

from server.data.syncers.tushare_syncer import TushareSyncer
from server.data.syncers.akshare_syncer import AKShareSyncer

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
)
logger = logging.getLogger(__name__)


def run_sync_primary():
    """主数据源同步（Tushare）"""
    logger.info("[Scheduler] Running primary sync (Tushare)")
    syncer = TushareSyncer()
    try:
        syncer.sync_stock_list()
        syncer.sync_daily_incremental()
        syncer.sync_adj_factors()
        logger.info("[Scheduler] Primary sync completed")
    except Exception as exc:
        logger.error("[Scheduler] Primary sync failed: %s", exc)
        raise


def run_sync_fallback():
    """兜底数据源同步（AKShare）"""
    logger.info("[Scheduler] Running fallback sync (AKShare)")
    syncer = AKShareSyncer()
    try:
        syncer.sync_stock_list()
        syncer.sync_daily_incremental()
        logger.info("[Scheduler] Fallback sync completed")
    except Exception as exc:
        logger.error("[Scheduler] Fallback sync failed: %s", exc)
        raise


def run_full_sync():
    """
    全量同步：先尝试 Tushare，失败则回退到 AKShare。
    """
    try:
        run_sync_primary()
    except Exception:
        logger.warning("[Scheduler] Primary failed, switching to fallback")
        run_sync_fallback()


def main():
    parser = argparse.ArgumentParser(description="Quant data sync scheduler")
    parser.add_argument("--once", action="store_true", help="Run once and exit")
    parser.add_argument("--mode", choices=["primary", "fallback", "full"], default="full",
                        help="Sync mode")
    args = parser.parse_args()

    if args.mode == "primary":
        sync_fn = run_sync_primary
    elif args.mode == "fallback":
        sync_fn = run_sync_fallback
    else:
        sync_fn = run_full_sync

    if args.once:
        sync_fn()
        return

    # 定时模式：每天 15:30 执行（A股收盘后）
    try:
        from apscheduler.schedulers.blocking import BlockingScheduler
    except ImportError:
        logger.error("apscheduler not installed, run: pip install apscheduler")
        sys.exit(1)

    scheduler = BlockingScheduler()
    scheduler.add_job(sync_fn, "cron", hour=15, minute=30, id="daily_sync")
    logger.info("[Scheduler] Started. Next run at 15:30 daily.")
    scheduler.start()


if __name__ == "__main__":
    main()
