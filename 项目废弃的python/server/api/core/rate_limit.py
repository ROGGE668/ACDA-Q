"""
基于 Redis 的滑动窗口速率限制器。
单实例模式下支持内存回退。
"""
import time
import asyncio
from collections import OrderedDict
from typing import Optional
from fastapi import Request, HTTPException
from fastapi.security import HTTPBearer

from server.api.core.config import get_settings

settings = get_settings()

# 内存回退：用于无 Redis 或 Redis 不可用时
_MAX_MEMORY_KEYS = 5000
_memory_windows: OrderedDict[str, list] = OrderedDict()
_memory_lock = asyncio.Lock()


class RateLimiter:
    """滑动窗口速率限制器"""

    def __init__(
        self,
        max_requests: int = 10,
        window_seconds: int = 60,
        key_prefix: str = "rl",
    ):
        self.max_requests = max_requests
        self.window_seconds = window_seconds
        self.key_prefix = key_prefix
        self._redis = None

    async def _get_redis(self):
        if self._redis is not None:
            return self._redis
        try:
            from redis.asyncio import from_url
            self._redis = from_url(
                settings.REDIS_URL,
                encoding="utf-8",
                decode_responses=True,
            )
            await self._redis.ping()
            return self._redis
        except Exception:
            self._redis = None
            return None

    async def is_allowed(self, key: str) -> bool:
        """检查 key 在当前窗口内是否允许继续请求"""
        redis = await self._get_redis()
        now = time.time()
        window_start = now - self.window_seconds

        if redis:
            rkey = f"{self.key_prefix}:{key}"
            pipe = redis.pipeline()
            # 移除窗口外的旧记录
            pipe.zremrangebyscore(rkey, 0, window_start)
            # 统计窗口内记录数
            pipe.zcard(rkey)
            # 添加当前请求
            pipe.zadd(rkey, {str(now): now})
            # 设置过期时间（窗口 + 1秒缓冲）
            pipe.expire(rkey, self.window_seconds + 1)
            results = await pipe.execute()
            current_count = results[1]
            return current_count < self.max_requests

        # 内存回退（仅适用于单实例）
        async with _memory_lock:
            # 清理所有 key 的过期窗口记录，并移除空列表的 key
            empty_keys = [
                k for k, w in _memory_windows.items()
                if not [t for t in w if t > window_start]
            ]
            for k in empty_keys:
                del _memory_windows[k]

            window = _memory_windows.setdefault(key, [])
            # 清理窗口外记录
            window[:] = [t for t in window if t > window_start]
            if len(window) >= self.max_requests:
                return False
            window.append(now)
            _memory_windows.move_to_end(key)

            # LRU 淘汰：限制总 key 数量
            while len(_memory_windows) > _MAX_MEMORY_KEYS:
                _memory_windows.popitem(last=False)
            return True


# 预设的限流规则
backtest_limiter = RateLimiter(max_requests=10, window_seconds=60, key_prefix="rl:backtest")
ai_limiter = RateLimiter(max_requests=5, window_seconds=60, key_prefix="rl:ai")
auth_limiter = RateLimiter(max_requests=5, window_seconds=60, key_prefix="rl:auth")
device_limiter = RateLimiter(max_requests=20, window_seconds=60, key_prefix="rl:device")


async def rate_limit_backtest(request: Request):
    """回测提交端点限流依赖"""
    user = getattr(request.state, "user", None)
    key = str(user.id) if user else request.client.host if request.client else "anonymous"
    if not await backtest_limiter.is_allowed(key):
        raise HTTPException(status_code=429, detail="Rate limit exceeded: max 10 backtests per minute")


async def rate_limit_ai(request: Request):
    """AI生成端点限流依赖"""
    user = getattr(request.state, "user", None)
    key = str(user.id) if user else request.client.host if request.client else "anonymous"
    if not await ai_limiter.is_allowed(key):
        raise HTTPException(status_code=429, detail="Rate limit exceeded: max 5 AI generations per minute")


async def rate_limit_auth(request: Request):
    """认证端点限流依赖（防暴力破解）"""
    key = request.client.host if request.client else "anonymous"
    if not await auth_limiter.is_allowed(key):
        raise HTTPException(status_code=429, detail="Too many attempts, please try again later")


async def rate_limit_device(request: Request):
    """设备端点限流依赖"""
    key = request.client.host if request.client else "anonymous"
    if not await device_limiter.is_allowed(key):
        raise HTTPException(status_code=429, detail="Too many device operations")
