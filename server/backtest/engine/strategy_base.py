"""
策略基类定义，独立模块以避免循环导入。
兼容 Python 3.11.8
"""
from typing import Dict, Any, Optional


class BaseStrategy:
    """策略基类，用户策略应继承此类"""

    def __init__(self, params: Optional[Dict[str, Any]] = None):
        self.params = params or {}
        self.broker: Optional[Any] = None

    def set_broker(self, broker: Any):
        self.broker = broker

    def on_init(self):
        """回测开始前调用一次"""
        pass

    def on_bar(self, context: Any, bar_group: Any):
        """每个时间周期调用一次"""
        raise NotImplementedError

    def on_exit(self):
        """回测结束后调用一次"""
        pass
