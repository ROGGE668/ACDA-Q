from datetime import datetime
from fastapi import Depends, HTTPException, status, Request
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select

from server.api.core.database import get_db
from server.api.core.security import decode_token
from server.api.models.models import User, UserDevice

security = HTTPBearer(auto_error=False)


def _extract_token(request: Request, credentials: HTTPAuthorizationCredentials | None) -> str | None:
    """优先从 Header 读取 token，其次从 Cookie 读取。"""
    if credentials:
        return credentials.credentials
    return request.cookies.get("access_token")


async def get_current_user(
    request: Request,
    credentials: HTTPAuthorizationCredentials = Depends(security),
    db: AsyncSession = Depends(get_db),
) -> User:
    token = _extract_token(request, credentials)
    if not token:
        raise HTTPException(status_code=401, detail="Not authenticated")

    payload = decode_token(token)
    if not payload or payload.get("type") != "access":
        raise HTTPException(status_code=401, detail="Invalid or expired token")

    user_id = payload.get("sub")
    if not user_id:
        raise HTTPException(status_code=401, detail="Invalid token payload")

    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(status_code=401, detail="User not found")

    return user


async def get_current_device(
    request: Request,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> UserDevice:
    """验证请求头中的设备指纹，确保设备已注册且未被吊销。"""
    fingerprint = request.headers.get("X-Device-Fingerprint")
    if not fingerprint:
        raise HTTPException(status_code=401, detail="Device fingerprint required")

    result = await db.execute(
        select(UserDevice).where(
            UserDevice.user_id == current_user.id,
            UserDevice.device_fingerprint == fingerprint,
        )
    )
    device = result.scalar_one_or_none()
    if not device:
        raise HTTPException(status_code=403, detail="Device not registered")

    if device.revoked_at:
        raise HTTPException(status_code=403, detail="Device has been revoked")

    if not device.is_active:
        raise HTTPException(status_code=403, detail="Device is inactive")

    # 隐式更新心跳时间（不阻塞请求）
    device.last_heartbeat_at = datetime.utcnow()
    await db.commit()

    return device


# ========== Quota Checks ==========

import redis.asyncio as redis
from server.api.core.config import get_settings

settings = get_settings()


def _get_redis() -> redis.Redis:
    return redis.from_url(settings.REDIS_URL, decode_responses=True)


def _quota_key(user_id: str, quota_type: str, date_str: str) -> str:
    return f"quota:{quota_type}:{user_id}:{date_str}"


async def check_backtest_quota(
    current_user: User = Depends(get_current_user),
) -> User:
    """检查用户每日回测配额。"""
    today = datetime.utcnow().strftime("%Y%m%d")
    r = _get_redis()
    try:
        used = int(await r.get(_quota_key(str(current_user.id), "backtest", today)) or 0)
        if used >= current_user.quota_backtest_daily:
            raise HTTPException(status_code=429, detail="Daily backtest quota exceeded")
    finally:
        await r.close()
    return current_user


async def increment_backtest_quota(user_id: str) -> None:
    """增加用户回测配额计数。"""
    today = datetime.utcnow().strftime("%Y%m%d")
    r = _get_redis()
    try:
        key = _quota_key(user_id, "backtest", today)
        await r.incr(key)
        await r.expire(key, 86400)
    finally:
        await r.close()


async def check_ai_quota(
    current_user: User = Depends(get_current_user),
) -> User:
    """检查用户每日AI生成配额。"""
    today = datetime.utcnow().strftime("%Y%m%d")
    r = _get_redis()
    try:
        used = int(await r.get(_quota_key(str(current_user.id), "ai", today)) or 0)
        if used >= current_user.quota_ai_daily:
            raise HTTPException(status_code=429, detail="Daily AI generation quota exceeded")
    finally:
        await r.close()
    return current_user


async def increment_ai_quota(user_id: str) -> None:
    """增加用户AI生成配额计数。"""
    today = datetime.utcnow().strftime("%Y%m%d")
    r = _get_redis()
    try:
        key = _quota_key(user_id, "ai", today)
        await r.incr(key)
        await r.expire(key, 86400)
    finally:
        await r.close()


async def get_current_admin(
    current_user: User = Depends(get_current_user),
) -> User:
    """验证当前用户是否为管理员。"""
    if not current_user.is_admin:
        raise HTTPException(status_code=403, detail="Admin access required")
    return current_user
