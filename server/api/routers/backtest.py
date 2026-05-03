import hashlib
import json
import logging
import os
from typing import List
from uuid import UUID
from fastapi import APIRouter, Depends, HTTPException, status, Request
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from datetime import datetime, timezone

from server.api.core.database import get_db
from server.api.core.rate_limit import backtest_limiter
from server.api.models.models import User, BacktestJob, BacktestCache, Subscription
from server.api.schemas.schemas import BacktestSubmit, BacktestJobOut, BacktestResult
from server.api.dependencies import get_current_user, increment_backtest_quota
from server.worker.tasks import run_backtest_task
import redis.asyncio as redis
from server.api.core.config import get_settings

settings = get_settings()


def _get_redis() -> redis.Redis:
    return redis.from_url(settings.REDIS_URL, decode_responses=True)


def _quota_key(user_id: str, quota_type: str, date_str: str) -> str:
    return f"quota:{quota_type}:{user_id}:{date_str}"

logger = logging.getLogger(__name__)

router = APIRouter()


async def _check_backtest_rate_limit(
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    # 速率限制
    key = str(current_user.id)
    if not await backtest_limiter.is_allowed(key):
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail="Rate limit exceeded: max 10 backtests per minute",
        )

    # 配额检查
    sub_result = await db.execute(
        select(Subscription).where(Subscription.user_id == current_user.id)
    )
    sub = sub_result.scalar_one_or_none()
    quota = sub.backtest_quota_daily if sub else current_user.quota_backtest_daily

    today = datetime.now(timezone.utc).strftime("%Y%m%d")
    r = _get_redis()
    used = int(await r.get(_quota_key(str(current_user.id), "backtest", today)) or 0)
    await r.close()

    if used >= quota:
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail="Daily backtest quota exceeded",
        )

    return current_user

# 报告文件根目录，所有报告文件必须位于此目录下
_REPORT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../reports"))


def _safe_report_path(path: str | None) -> str | None:
    """校验报告路径，防止路径遍历攻击。"""
    if not path:
        return None
    abs_path = os.path.abspath(path)
    # 确保目标路径在报告根目录下
    if os.path.commonpath([_REPORT_DIR, abs_path]) != _REPORT_DIR:
        logger.warning("Blocked path traversal attempt: %s", path)
        return None
    return abs_path


def _make_cache_hash(payload: BacktestSubmit) -> str:
    """基于策略+参数+范围生成缓存哈希"""
    cache_key = {
        "strategy_id": str(payload.strategy_id) if payload.strategy_id else "",
        "strategy_code": payload.strategy_code or "",
        "symbols": sorted(payload.symbols or []),
        "start_date": payload.start_date.isoformat() if payload.start_date else "",
        "end_date": payload.end_date.isoformat() if payload.end_date else "",
        "initial_cash": str(payload.initial_cash),
        "scope": payload.scope or "single",
        "params": payload.params or {},
    }
    return hashlib.sha256(json.dumps(cache_key, sort_keys=True).encode()).hexdigest()


@router.post("", response_model=BacktestJobOut, status_code=status.HTTP_202_ACCEPTED)
async def submit_backtest(
    payload: BacktestSubmit,
    request: Request,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(_check_backtest_rate_limit),
):
    # 验证：必须提供 strategy_id 或 strategy_code
    if not payload.strategy_id and not payload.strategy_code:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Either strategy_id or strategy_code must be provided",
        )

    # 检查缓存：相同策略+参数+时间范围的结果直接复用
    cache_hash = _make_cache_hash(payload)
    cache_result = await db.execute(
        select(BacktestCache).where(
            BacktestCache.cache_hash == cache_hash,
            BacktestCache.expires_at > datetime.now(timezone.utc),
        )
    )
    cached = cache_result.scalar_one_or_none()
    if cached:
        # 缓存命中：创建新 job 但直接标记为成功，复用缓存结果
        job = BacktestJob(
            user_id=current_user.id,
            strategy_id=payload.strategy_id,
            status="success",
            scope=payload.scope or ("single" if len(payload.symbols) == 1 else "multi"),
            symbols=payload.symbols,
            start_date=payload.start_date,
            end_date=payload.end_date,
            initial_cash=payload.initial_cash,
            params=payload.params,
            result_summary=cached.result_summary,
            result_report_path=cached.result_report_path,
            completed_at=datetime.now(timezone.utc),
        )
        db.add(job)
        await db.commit()
        await db.refresh(job)
        return job

    job = BacktestJob(
        user_id=current_user.id,
        strategy_id=payload.strategy_id,
        status="pending",
        scope=payload.scope or ("single" if len(payload.symbols) == 1 else "multi"),
        symbols=payload.symbols,
        start_date=payload.start_date,
        end_date=payload.end_date,
        initial_cash=payload.initial_cash,
        params=payload.params,
    )
    db.add(job)
    await db.commit()
    await db.refresh(job)

    # 增加配额计数
    today = datetime.now(timezone.utc).strftime("%Y%m%d")
    r = _get_redis()
    quota_key = _quota_key(str(current_user.id), "backtest", today)
    await r.incr(quota_key)
    await r.expire(quota_key, 86400)
    await r.close()

    # Dispatch Celery task（额外传递 cache_hash 和 strategy_code 用于结果写入缓存）
    request_id = getattr(request.state, "request_id", None)
    task_kwargs = {"cache_hash": cache_hash}
    if payload.strategy_code:
        task_kwargs["strategy_code"] = payload.strategy_code
    run_backtest_task.apply_async(
        args=[str(job.id)],
        kwargs=task_kwargs,
        headers={"request_id": request_id} if request_id else None,
    )

    return job


@router.get("", response_model=List[BacktestJobOut])
async def list_backtests(
    skip: int = 0,
    limit: int = 20,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    result = await db.execute(
        select(BacktestJob)
        .where(BacktestJob.user_id == current_user.id)
        .offset(skip)
        .limit(limit)
        .order_by(BacktestJob.created_at.desc())
    )
    return result.scalars().all()


@router.get("/{job_id}", response_model=BacktestJobOut)
async def get_backtest(
    job_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    result = await db.execute(
        select(BacktestJob).where(BacktestJob.id == job_id, BacktestJob.user_id == current_user.id)
    )
    job = result.scalar_one_or_none()
    if not job:
        raise HTTPException(status_code=404, detail="Backtest job not found")
    return job


@router.get("/{job_id}/result", response_model=BacktestResult)
async def get_backtest_result(
    job_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    result = await db.execute(
        select(BacktestJob).where(BacktestJob.id == job_id, BacktestJob.user_id == current_user.id)
    )
    job = result.scalar_one_or_none()
    if not job:
        raise HTTPException(status_code=404, detail="Backtest job not found")
    if job.status == "failed":
        raise HTTPException(status_code=400, detail=f"Backtest failed: {job.error_message or 'Unknown error'}")
    if job.status != "success":
        raise HTTPException(status_code=400, detail="Backtest not completed yet")

    summary = job.result_summary or {}
    trades: List[dict] = []
    equity_curve: List[dict] = []
    signals: List[dict] = []
    suitable_stocks: List[dict] = []
    unsuitable_stocks: List[dict] = []

    # 从报告文件读取完整数据
    safe_path = _safe_report_path(job.result_report_path)
    if safe_path and os.path.exists(safe_path):
        try:
            with open(safe_path, "r", encoding="utf-8") as f:
                report = json.load(f)
            trades = report.get("trades", [])
            equity_curve = report.get("equity_curve", [])
            signals = report.get("signals", [])
            suitable_stocks = report.get("suitable_stocks", [])
            unsuitable_stocks = report.get("unsuitable_stocks", [])
            # 如果 summary 为空，从报告补全
            if not summary:
                summary = {k: v for k, v in report.items() if k not in ("trades", "equity_curve", "positions", "signals", "suitable_stocks", "unsuitable_stocks")}
        except Exception as exc:
            logger.warning("Failed to read backtest report %s: %s", safe_path, exc)

    return {
        "job_id": job.id,
        "summary": summary,
        "trades": trades,
        "equity_curve": equity_curve,
        "signals": signals,
        "suitable_stocks": suitable_stocks,
        "unsuitable_stocks": unsuitable_stocks,
        "report_path": job.result_report_path,
    }


@router.get("/{job_id}/chart")
async def get_backtest_chart(
    job_id: UUID,
    agg: str = "auto",  # auto | week | month
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    """返回按周/月聚合的净值曲线，减少传输量。"""
    result = await db.execute(
        select(BacktestJob).where(BacktestJob.id == job_id, BacktestJob.user_id == current_user.id)
    )
    job = result.scalar_one_or_none()
    if not job:
        raise HTTPException(status_code=404, detail="Backtest job not found")
    if job.status != "success":
        raise HTTPException(status_code=400, detail="Backtest not completed yet")

    safe_path = _safe_report_path(job.result_report_path)
    equity_curve = []
    if safe_path and os.path.exists(safe_path):
        try:
            with open(safe_path, "r", encoding="utf-8") as f:
                report = json.load(f)
            equity_curve = report.get("equity_curve", [])
        except Exception:
            pass

    if not equity_curve:
        return {"points": [], "agg": agg}

    import pandas as pd
    df = pd.DataFrame(equity_curve)
    df["datetime"] = pd.to_datetime(df["datetime"])
    df = df.set_index("datetime").sort_index()

    # Auto: if > 1000 points, aggregate by week; if > 3000, by month
    if agg == "auto":
        if len(df) > 3000:
            agg = "month"
        elif len(df) > 1000:
            agg = "week"
        else:
            agg = "day"

    if agg == "week":
        df_agg = df.resample("W").last()
    elif agg == "month":
        df_agg = df.resample("ME").last()
    else:
        df_agg = df

    df_agg = df_agg.reset_index()
    points = [
        {
            "datetime": row["datetime"].isoformat() if hasattr(row["datetime"], "isoformat") else str(row["datetime"]),
            "total_value": round(row.get("total_value", 0), 2),
        }
        for _, row in df_agg.iterrows()
    ]
    return {"points": points, "agg": agg}


@router.get("/{job_id}/trades")
async def get_backtest_trades(
    job_id: UUID,
    page: int = 1,
    page_size: int = 50,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    """分页返回交易记录。"""
    result = await db.execute(
        select(BacktestJob).where(BacktestJob.id == job_id, BacktestJob.user_id == current_user.id)
    )
    job = result.scalar_one_or_none()
    if not job:
        raise HTTPException(status_code=404, detail="Backtest job not found")
    if job.status != "success":
        raise HTTPException(status_code=400, detail="Backtest not completed yet")

    safe_path = _safe_report_path(job.result_report_path)
    trades = []
    if safe_path and os.path.exists(safe_path):
        try:
            with open(safe_path, "r", encoding="utf-8") as f:
                report = json.load(f)
            trades = report.get("trades", [])
        except Exception:
            pass

    total = len(trades)
    # Sort by timestamp desc (latest first)
    trades_sorted = sorted(trades, key=lambda t: t.get("timestamp", ""), reverse=True)
    start = (page - 1) * page_size
    end = start + page_size
    return {
        "total": total,
        "page": page,
        "page_size": page_size,
        "items": trades_sorted[start:end],
    }
