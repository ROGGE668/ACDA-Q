"""
后台管理面板 API
提供系统数据统计和用户/设备/订单管理功能
"""
from datetime import datetime, timedelta, timezone
from typing import Optional, List
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, func, desc, and_

from server.api.core.database import get_db
from server.api.dependencies import get_current_admin
from server.api.models.models import (
    User, UserDevice, Subscription, PaymentOrder, BacktestJob, AIGeneration, Strategy
)

router = APIRouter()


# ========== Dashboard Stats ==========

@router.get("/stats")
async def get_dashboard_stats(
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """获取仪表盘核心统计数据"""
    # 用户统计
    user_count_result = await db.execute(select(func.count(User.id)))
    total_users = user_count_result.scalar() or 0

    # 今日新增用户
    today_start = datetime.now(timezone.utc).replace(hour=0, minute=0, second=0, microsecond=0)
    new_today_result = await db.execute(
        select(func.count(User.id)).where(User.created_at >= today_start)
    )
    new_users_today = new_today_result.scalar() or 0

    # 活跃设备
    active_devices_result = await db.execute(
        select(func.count(UserDevice.id)).where(
            UserDevice.is_active == True,
            UserDevice.revoked_at.is_(None),
        )
    )
    active_devices = active_devices_result.scalar() or 0

    # 今日回测
    backtests_today_result = await db.execute(
        select(func.count(BacktestJob.id)).where(BacktestJob.created_at >= today_start)
    )
    backtests_today = backtests_today_result.scalar() or 0

    # 回测成功率
    backtests_success_result = await db.execute(
        select(func.count(BacktestJob.id)).where(
            BacktestJob.created_at >= today_start,
            BacktestJob.status == "success",
        )
    )
    backtests_success = backtests_success_result.scalar() or 0
    backtest_success_rate = round(backtests_success / backtests_today * 100, 1) if backtests_today > 0 else 0

    # 订阅统计
    sub_result = await db.execute(
        select(Subscription.tier, func.count(Subscription.id))
        .group_by(Subscription.tier)
    )
    sub_distribution = {tier: count for tier, count in sub_result.all()}

    # 收入统计
    revenue_result = await db.execute(
        select(func.coalesce(func.sum(PaymentOrder.amount_cents), 0)).where(
            PaymentOrder.status == "paid"
        )
    )
    total_revenue = (revenue_result.scalar() or 0) / 100

    revenue_this_month_result = await db.execute(
        select(func.coalesce(func.sum(PaymentOrder.amount_cents), 0)).where(
            PaymentOrder.status == "paid",
            PaymentOrder.created_at >= today_start.replace(day=1),
        )
    )
    revenue_this_month = (revenue_this_month_result.scalar() or 0) / 100

    # AI 生成统计
    ai_today_result = await db.execute(
        select(func.count(AIGeneration.id)).where(AIGeneration.created_at >= today_start)
    )
    ai_today = ai_today_result.scalar() or 0

    return {
        "users": {
            "total": total_users,
            "new_today": new_users_today,
        },
        "devices": {
            "active": active_devices,
        },
        "backtests": {
            "today": backtests_today,
            "success_rate": backtest_success_rate,
        },
        "subscriptions": sub_distribution,
        "revenue": {
            "total_cny": total_revenue,
            "this_month_cny": revenue_this_month,
        },
        "ai_generations": {
            "today": ai_today,
        },
    }


# ========== User Management ==========

@router.get("/users")
async def list_users(
    search: Optional[str] = None,
    tier: Optional[str] = None,
    is_admin: Optional[bool] = None,
    skip: int = Query(0, ge=0),
    limit: int = Query(20, ge=1, le=100),
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """分页查询用户列表"""
    query = select(User)

    filters = []
    if search:
        filters.append(
            User.email.ilike(f"%{search}%") | User.nickname.ilike(f"%{search}%")
        )
    if tier:
        filters.append(User.tier == tier)
    if is_admin is not None:
        filters.append(User.is_admin == is_admin)

    if filters:
        query = query.where(and_(*filters))

    count_result = await db.execute(select(func.count()).select_from(query.subquery()))
    total = count_result.scalar() or 0

    query = query.order_by(desc(User.created_at)).offset(skip).limit(limit)
    result = await db.execute(query)
    users = result.scalars().all()

    return {
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": [
            {
                "id": str(u.id),
                "email": u.email,
                "nickname": u.nickname,
                "tier": u.tier,
                "is_admin": u.is_admin,
                "quota_ai_daily": u.quota_ai_daily,
                "quota_backtest_daily": u.quota_backtest_daily,
                "created_at": u.created_at.isoformat() if u.created_at else None,
            }
            for u in users
        ],
    }


@router.put("/users/{user_id}/admin")
async def toggle_admin(
    user_id: UUID,
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """切换用户管理员权限"""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    user.is_admin = not user.is_admin
    await db.commit()
    return {"id": str(user.id), "is_admin": user.is_admin}


@router.delete("/users/{user_id}")
async def delete_user(
    user_id: UUID,
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """删除用户（级联删除关联数据）"""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    await db.delete(user)
    await db.commit()
    return {"status": "deleted"}


# ========== Device Management ==========

@router.get("/devices")
async def list_all_devices(
    user_id: Optional[str] = None,
    is_active: Optional[bool] = None,
    skip: int = Query(0, ge=0),
    limit: int = Query(20, ge=1, le=100),
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """分页查询所有设备"""
    query = select(UserDevice)

    filters = []
    if user_id:
        filters.append(UserDevice.user_id == user_id)
    if is_active is not None:
        filters.append(UserDevice.is_active == is_active)

    if filters:
        query = query.where(and_(*filters))

    count_result = await db.execute(select(func.count()).select_from(query.subquery()))
    total = count_result.scalar() or 0

    query = query.order_by(desc(UserDevice.created_at)).offset(skip).limit(limit)
    result = await db.execute(query)
    devices = result.scalars().all()

    # 关联查询用户信息
    user_ids = [d.user_id for d in devices]
    user_result = await db.execute(select(User).where(User.id.in_(user_ids)))
    users = {u.id: u for u in user_result.scalars().all()}

    return {
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": [
            {
                "id": str(d.id),
                "user_id": str(d.user_id),
                "user_email": users.get(d.user_id, User(email="")).email,
                "device_fingerprint": d.device_fingerprint,
                "device_name": d.device_name,
                "os_type": d.os_type,
                "is_active": d.is_active,
                "revoked_at": d.revoked_at.isoformat() if d.revoked_at else None,
                "last_heartbeat_at": d.last_heartbeat_at.isoformat() if d.last_heartbeat_at else None,
                "created_at": d.created_at.isoformat() if d.created_at else None,
            }
            for d in devices
        ],
    }


@router.post("/devices/{device_id}/revoke")
async def admin_revoke_device(
    device_id: UUID,
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """管理员吊销设备"""
    result = await db.execute(select(UserDevice).where(UserDevice.id == device_id))
    device = result.scalar_one_or_none()
    if not device:
        raise HTTPException(status_code=404, detail="Device not found")

    device.is_active = False
    device.revoked_at = datetime.utcnow()
    await db.commit()
    return {"status": "revoked", "id": str(device.id)}


# ========== Subscription Management ==========

@router.get("/subscriptions")
async def list_subscriptions(
    tier: Optional[str] = None,
    status: Optional[str] = None,
    skip: int = Query(0, ge=0),
    limit: int = Query(20, ge=1, le=100),
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """分页查询所有订阅"""
    query = select(Subscription)

    filters = []
    if tier:
        filters.append(Subscription.tier == tier)
    if status:
        filters.append(Subscription.status == status)

    if filters:
        query = query.where(and_(*filters))

    count_result = await db.execute(select(func.count()).select_from(query.subquery()))
    total = count_result.scalar() or 0

    query = query.order_by(desc(Subscription.created_at)).offset(skip).limit(limit)
    result = await db.execute(query)
    subs = result.scalars().all()

    user_ids = [s.user_id for s in subs]
    user_result = await db.execute(select(User).where(User.id.in_(user_ids)))
    users = {u.id: u for u in user_result.scalars().all()}

    return {
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": [
            {
                "id": str(s.id),
                "user_id": str(s.user_id),
                "user_email": users.get(s.user_id, User(email="")).email,
                "tier": s.tier,
                "status": s.status,
                "expires_at": s.expires_at.isoformat() if s.expires_at else None,
                "max_devices": s.max_devices,
                "ai_quota_daily": s.ai_quota_daily,
                "backtest_quota_daily": s.backtest_quota_daily,
                "created_at": s.created_at.isoformat() if s.created_at else None,
            }
            for s in subs
        ],
    }


@router.put("/subscriptions/{sub_id}")
async def update_subscription(
    sub_id: UUID,
    tier: Optional[str] = None,
    status: Optional[str] = None,
    expires_at: Optional[datetime] = None,
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """更新订阅信息"""
    result = await db.execute(select(Subscription).where(Subscription.id == sub_id))
    sub = result.scalar_one_or_none()
    if not sub:
        raise HTTPException(status_code=404, detail="Subscription not found")

    if tier:
        sub.tier = tier
    if status:
        sub.status = status
    if expires_at:
        sub.expires_at = expires_at

    await db.commit()
    await db.refresh(sub)
    return {"status": "updated", "id": str(sub.id)}


# ========== Payment Management ==========

@router.get("/payments")
async def list_payments(
    status: Optional[str] = None,
    channel: Optional[str] = None,
    skip: int = Query(0, ge=0),
    limit: int = Query(20, ge=1, le=100),
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """分页查询所有支付订单"""
    query = select(PaymentOrder)

    filters = []
    if status:
        filters.append(PaymentOrder.status == status)
    if channel:
        filters.append(PaymentOrder.channel == channel)

    if filters:
        query = query.where(and_(*filters))

    count_result = await db.execute(select(func.count()).select_from(query.subquery()))
    total = count_result.scalar() or 0

    query = query.order_by(desc(PaymentOrder.created_at)).offset(skip).limit(limit)
    result = await db.execute(query)
    orders = result.scalars().all()

    user_ids = [o.user_id for o in orders]
    user_result = await db.execute(select(User).where(User.id.in_(user_ids)))
    users = {u.id: u for u in user_result.scalars().all()}

    return {
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": [
            {
                "id": str(o.id),
                "order_no": o.order_no,
                "user_id": str(o.user_id),
                "user_email": users.get(o.user_id, User(email="")).email,
                "channel": o.channel,
                "amount_cents": o.amount_cents,
                "tier": o.tier,
                "duration_months": o.duration_months,
                "status": o.status,
                "paid_at": o.paid_at.isoformat() if o.paid_at else None,
                "created_at": o.created_at.isoformat() if o.created_at else None,
            }
            for o in orders
        ],
    }


@router.put("/payments/{order_id}/status")
async def update_payment_status(
    order_id: UUID,
    status: str,
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """手动更新支付订单状态（用于人工审核或退款）"""
    result = await db.execute(select(PaymentOrder).where(PaymentOrder.id == order_id))
    order = result.scalar_one_or_none()
    if not order:
        raise HTTPException(status_code=404, detail="Order not found")

    order.status = status
    if status == "paid":
        order.paid_at = datetime.utcnow()
    elif status == "refunded":
        order.paid_at = None

    await db.commit()
    return {"status": "updated", "order_status": order.status}


# ========== Backtest Job Monitoring ==========

@router.get("/backtests")
async def list_all_backtests(
    status: Optional[str] = None,
    user_id: Optional[str] = None,
    skip: int = Query(0, ge=0),
    limit: int = Query(20, ge=1, le=100),
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """分页查询所有回测任务"""
    query = select(BacktestJob)

    filters = []
    if status:
        filters.append(BacktestJob.status == status)
    if user_id:
        filters.append(BacktestJob.user_id == user_id)

    if filters:
        query = query.where(and_(*filters))

    count_result = await db.execute(select(func.count()).select_from(query.subquery()))
    total = count_result.scalar() or 0

    query = query.order_by(desc(BacktestJob.created_at)).offset(skip).limit(limit)
    result = await db.execute(query)
    jobs = result.scalars().all()

    return {
        "total": total,
        "skip": skip,
        "limit": limit,
        "items": [
            {
                "id": str(j.id),
                "user_id": str(j.user_id),
                "status": j.status,
                "scope": j.scope,
                "symbols": j.symbols,
                "initial_cash": float(j.initial_cash) if j.initial_cash else None,
                "started_at": j.started_at.isoformat() if j.started_at else None,
                "completed_at": j.completed_at.isoformat() if j.completed_at else None,
                "created_at": j.created_at.isoformat() if j.created_at else None,
                "error_message": j.error_message,
            }
            for j in jobs
        ],
    }


@router.delete("/backtests/{job_id}")
async def delete_backtest_job(
    job_id: UUID,
    _: User = Depends(get_current_admin),
    db: AsyncSession = Depends(get_db),
):
    """删除回测任务"""
    result = await db.execute(select(BacktestJob).where(BacktestJob.id == job_id))
    job = result.scalar_one_or_none()
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")

    await db.delete(job)
    await db.commit()
    return {"status": "deleted"}
