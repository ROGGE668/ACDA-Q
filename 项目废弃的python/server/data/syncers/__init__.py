"""
数据同步器。
"""
from server.data.syncers.tushare_syncer import TushareSyncer
from server.data.syncers.akshare_syncer import AKShareSyncer

__all__ = ["TushareSyncer", "AKShareSyncer"]
