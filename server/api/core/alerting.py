"""
简单告警管理器：基于内存的滑动窗口失败计数。
生产环境建议替换为 Prometheus Alertmanager 或 Sentry。
"""
import json
import os
import time
import urllib.request
from collections import deque
from typing import Optional


class SimpleAlertManager:
    """
    在滑动时间窗口内统计失败次数，超过阈值时触发告警。
    默认：60 秒内失败 >= 5 次触发一次告警，冷却期 5 分钟。
    """

    def __init__(self, threshold: int = 5, window_seconds: int = 60, cooldown_seconds: int = 300):
        self.threshold = threshold
        self.window = window_seconds
        self.cooldown = cooldown_seconds
        self._events: deque = deque()
        self._last_alert = 0.0

    def record_failure(self, task_name: str, error: Optional[str] = None) -> None:
        now = time.time()
        self._events.append((now, task_name, error))
        # 清理窗口外的过期事件
        while self._events and self._events[0][0] < now - self.window:
            self._events.popleft()

        if len(self._events) >= self.threshold and now - self._last_alert > self.cooldown:
            self._trigger_alert()
            self._last_alert = now

    def _trigger_alert(self) -> None:
        count = len(self._events)
        recent = [e[1] for e in list(self._events)[-self.threshold:]]
        message = f"[ALERT] Celery task failures: {count} in {self.window}s. Recent tasks: {recent}"

        # 优先尝试 Webhook
        webhook_url = os.getenv("ALERT_WEBHOOK_URL")
        if webhook_url:
            try:
                payload = json.dumps({"text": message}).encode("utf-8")
                req = urllib.request.Request(
                    webhook_url,
                    data=payload,
                    headers={"Content-Type": "application/json"},
                    method="POST",
                )
                with urllib.request.urlopen(req, timeout=5) as resp:
                    if resp.status >= 300:
                        print(f"[ALERT] Webhook returned {resp.status}")
            except Exception as exc:
                print(f"[ALERT] Webhook failed: {exc}")

        # fallback 到 stderr / 日志
        print(message, flush=True)


# 全局实例
alert_manager = SimpleAlertManager()
