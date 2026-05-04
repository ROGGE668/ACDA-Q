import uuid
from datetime import datetime, timedelta
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Request, status
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, func
import redis.asyncio as redis

from server.api.core.database import get_db
from server.api.core.config import get_settings
from server.api.dependencies import get_current_user
from server.api.models.models import User, UserDevice, Subscription, PaymentOrder
from server.api.schemas.schemas import (
    DeviceRegister,
    DeviceHeartbeat,
    DeviceOut,
    SubscriptionStatus,
    PaymentCreate,
    PaymentOut,
    PaymentQRPayload,
    StatusResponse,
)

router = APIRouter()
settings = get_settings()

# Pricing: cents per month
TIER_PRICING = {
    "basic": 990,         # 9.9 RMB / 月  (基础版,30 次)
    "pro":   1990,        # 19.9 RMB / 月 (PRO,80 次)
    "max":   9900,        # 99 RMB / 月   (MAX,500 次)
}

# 配额含义: AI 与回测各 N 次
# 免费版按"日"计(每天 3 次);付费版按"月"计(每月 N 次)
# 字段名 ai_quota_daily / backtest_quota_daily 是历史遗留,数字含义视 tier 而定
TIER_CONFIG = {
    "free":  {"max_devices": 1, "ai_quota_daily": 3,   "backtest_quota_daily": 5},
    "basic": {"max_devices": 1, "ai_quota_daily": 30,  "backtest_quota_daily": 30},
    "pro":   {"max_devices": 2, "ai_quota_daily": 80,  "backtest_quota_daily": 80},
    "max":   {"max_devices": 5, "ai_quota_daily": 500, "backtest_quota_daily": 500},
}


def _get_redis() -> redis.Redis:
    return redis.from_url(settings.REDIS_URL, decode_responses=True)


def _quota_key(user_id: str, quota_type: str, date_str: str) -> str:
    return f"quota:{quota_type}:{user_id}:{date_str}"


def _generate_order_no() -> str:
    return f"ORD{datetime.utcnow().strftime('%Y%m%d%H%M%S')}{uuid.uuid4().hex[:8].upper()}"


# ========== Device ==========

@router.post("/devices/register", response_model=StatusResponse)
async def register_device(
    payload: DeviceRegister,
    request: Request,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """注册设备，检查订阅套餐的设备数量限制。"""
    # 检查订阅
    sub_result = await db.execute(
        select(Subscription).where(Subscription.user_id == current_user.id)
    )
    sub = sub_result.scalar_one_or_none()
    if not sub:
        # 自动创建免费订阅
        cfg = TIER_CONFIG["free"]
        sub = Subscription(
            user_id=current_user.id,
            tier="free",
            max_devices=cfg["max_devices"],
            ai_quota_daily=cfg["ai_quota_daily"],
            backtest_quota_daily=cfg["backtest_quota_daily"],
        )
        db.add(sub)
        await db.commit()
        await db.refresh(sub)

    # 检查设备数量限制
    active_count_result = await db.execute(
        select(func.count(UserDevice.id)).where(
            UserDevice.user_id == current_user.id,
            UserDevice.is_active == True,
            UserDevice.revoked_at.is_(None),
        )
    )
    active_count = active_count_result.scalar() or 0

    # 检查是否已存在此设备
    existing_result = await db.execute(
        select(UserDevice).where(
            UserDevice.user_id == current_user.id,
            UserDevice.device_fingerprint == payload.device_fingerprint,
        )
    )
    existing = existing_result.scalar_one_or_none()

    if existing:
        if existing.revoked_at:
            raise HTTPException(status_code=403, detail="Device has been revoked")
        existing.is_active = True
        existing.last_heartbeat_at = datetime.utcnow()
        existing.device_name = payload.device_name or existing.device_name
        existing.os_type = payload.os_type or existing.os_type
        await db.commit()
        return {"status": "ok"}

    if active_count >= sub.max_devices:
        raise HTTPException(
            status_code=403,
            detail=f"Device limit reached ({sub.max_devices}). Please upgrade your plan or revoke an existing device.",
        )

    device = UserDevice(
        user_id=current_user.id,
        device_fingerprint=payload.device_fingerprint,
        device_name=payload.device_name,
        os_type=payload.os_type,
        last_heartbeat_at=datetime.utcnow(),
        is_active=True,
    )
    db.add(device)
    await db.commit()
    return {"status": "ok"}


@router.post("/devices/heartbeat", response_model=StatusResponse)
async def device_heartbeat(
    payload: DeviceHeartbeat,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """设备心跳，更新最后活跃时间。"""
    result = await db.execute(
        select(UserDevice).where(
            UserDevice.user_id == current_user.id,
            UserDevice.device_fingerprint == payload.device_fingerprint,
        )
    )
    device = result.scalar_one_or_none()
    if not device:
        raise HTTPException(status_code=404, detail="Device not found")

    if device.revoked_at:
        raise HTTPException(status_code=403, detail="Device has been revoked")

    device.last_heartbeat_at = datetime.utcnow()
    device.is_active = True
    await db.commit()
    return {"status": "ok"}


@router.get("/devices", response_model=list[DeviceOut])
async def list_devices(
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """列出用户所有设备。"""
    result = await db.execute(
        select(UserDevice).where(UserDevice.user_id == current_user.id)
    )
    devices = result.scalars().all()
    return devices


@router.post("/devices/{device_id}/revoke", response_model=StatusResponse)
async def revoke_device(
    device_id: str,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """吊销指定设备。"""
    from uuid import UUID

    try:
        dev_uuid = UUID(device_id)
    except ValueError:
        raise HTTPException(status_code=400, detail="Invalid device ID")

    result = await db.execute(
        select(UserDevice).where(
            UserDevice.id == dev_uuid,
            UserDevice.user_id == current_user.id,
        )
    )
    device = result.scalar_one_or_none()
    if not device:
        raise HTTPException(status_code=404, detail="Device not found")

    device.is_active = False
    device.revoked_at = datetime.utcnow()
    await db.commit()
    return {"status": "ok"}


# ========== Subscription ==========

@router.get("/subscription", response_model=SubscriptionStatus)
async def get_subscription(
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """获取用户订阅状态和配额使用情况。"""
    result = await db.execute(
        select(Subscription).where(Subscription.user_id == current_user.id)
    )
    sub = result.scalar_one_or_none()
    if not sub:
        cfg = TIER_CONFIG["free"]
        sub = Subscription(
            user_id=current_user.id,
            tier="free",
            max_devices=cfg["max_devices"],
            ai_quota_daily=cfg["ai_quota_daily"],
            backtest_quota_daily=cfg["backtest_quota_daily"],
        )
        db.add(sub)
        await db.commit()
        await db.refresh(sub)

    # 活跃设备数
    active_result = await db.execute(
        select(func.count(UserDevice.id)).where(
            UserDevice.user_id == current_user.id,
            UserDevice.is_active == True,
            UserDevice.revoked_at.is_(None),
        )
    )
    devices_active = active_result.scalar() or 0

    # 今日配额使用情况（从 Redis）
    today = datetime.utcnow().strftime("%Y%m%d")
    r = _get_redis()
    ai_used = int(await r.get(_quota_key(str(current_user.id), "ai", today)) or 0)
    backtest_used = int(await r.get(_quota_key(str(current_user.id), "backtest", today)) or 0)
    await r.close()

    return {
        "tier": sub.tier,
        "status": sub.status,
        "expires_at": sub.expires_at,
        "max_devices": sub.max_devices,
        "ai_quota_daily": sub.ai_quota_daily,
        "backtest_quota_daily": sub.backtest_quota_daily,
        "devices_active": devices_active,
        "ai_used_today": ai_used,
        "backtest_used_today": backtest_used,
    }


# ========== Payment ==========

@router.post("/payments", response_model=PaymentQRPayload)
async def create_payment(
    payload: PaymentCreate,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """创建支付订单，返回二维码信息。"""
    if payload.tier not in TIER_PRICING:
        raise HTTPException(status_code=400, detail="Invalid tier")

    amount_cents = TIER_PRICING[payload.tier] * payload.duration_months
    order_no = _generate_order_no()

    order = PaymentOrder(
        user_id=current_user.id,
        order_no=order_no,
        channel=payload.channel,
        amount_cents=amount_cents,
        tier=payload.tier,
        duration_months=payload.duration_months,
        status="pending",
    )
    db.add(order)
    await db.commit()

    # 生成模拟二维码URL（实际集成时需调用支付宝/微信SDK）
    # 此处返回一个占位URL，前端可轮询 /payments/{order_no} 查询状态
    qr_url = f"mock://pay/{payload.channel}?order={order_no}&amount={amount_cents}"

    return {
        "order_no": order_no,
        "qr_code_url": qr_url,
        "amount_cents": amount_cents,
        "expires_at": datetime.utcnow() + timedelta(minutes=30),
    }


@router.get("/payments/{order_no}", response_model=PaymentOut)
async def get_payment_status(
    order_no: str,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """查询支付订单状态。"""
    result = await db.execute(
        select(PaymentOrder).where(
            PaymentOrder.order_no == order_no,
            PaymentOrder.user_id == current_user.id,
        )
    )
    order = result.scalar_one_or_none()
    if not order:
        raise HTTPException(status_code=404, detail="Order not found")
    return order


@router.post("/payments/{order_no}/cancel", response_model=StatusResponse)
async def cancel_payment(
    order_no: str,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """取消待支付订单。"""
    result = await db.execute(
        select(PaymentOrder).where(
            PaymentOrder.order_no == order_no,
            PaymentOrder.user_id == current_user.id,
        )
    )
    order = result.scalar_one_or_none()
    if not order:
        raise HTTPException(status_code=404, detail="Order not found")
    if order.status != "pending":
        raise HTTPException(status_code=400, detail="Only pending orders can be cancelled")

    order.status = "cancelled"
    await db.commit()
    return {"status": "ok"}
