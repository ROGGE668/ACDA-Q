from datetime import datetime
from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import func, update
from sqlalchemy.ext.asyncio import AsyncSession
from prometheus_client import Counter

from server.api.core.database import get_db
from server.api.core.rate_limit import ai_limiter
from server.api.models.models import User, AIGeneration
from server.api.schemas.schemas import AIGenerateRequest, AIGenerateResponse
from server.api.dependencies import get_current_user
from server.ai.generators.llm import generate_strategy_code
from server.ai.generators.params_extractor import extract_params

router = APIRouter()

AI_GENERATION_TOTAL = Counter("ai_generation_total", "Total AI generations", ["status"])


async def _check_ai_rate_limit(
    current_user: User = Depends(get_current_user),
):
    key = str(current_user.id)
    if not await ai_limiter.is_allowed(key):
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail="Rate limit exceeded: max 5 AI generations per minute",
        )
    return current_user


@router.post("/generate", response_model=AIGenerateResponse)
async def generate_strategy(
    payload: AIGenerateRequest,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(_check_ai_rate_limit),
):
    # 基于实际用量检查每日配额（每日自动重置）
    today_start = datetime.utcnow().replace(hour=0, minute=0, second=0, microsecond=0)
    result = await db.execute(
        select(func.count(AIGeneration.id)).where(
            AIGeneration.user_id == current_user.id,
            AIGeneration.created_at >= today_start,
        )
    )
    used_today = result.scalar() or 0
    if used_today >= current_user.quota_ai_daily:
        raise HTTPException(status_code=429, detail="Daily AI generation quota exceeded")

    try:
        code, model_used, tokens = await generate_strategy_code(
            prompt=payload.prompt,
            model=payload.model,
        )
    except Exception as e:
        AI_GENERATION_TOTAL.labels(status="error").inc()
        raise HTTPException(status_code=500, detail=f"AI generation failed: {str(e)}")

    record = AIGeneration(
        user_id=current_user.id,
        prompt=payload.prompt,
        generated_code=code,
        model=model_used,
        tokens_used=tokens,
        status="success",
    )
    db.add(record)
    await db.commit()

    AI_GENERATION_TOTAL.labels(status="success").inc()
    return AIGenerateResponse(
        generated_code=code,
        model=model_used,
        tokens_used=tokens,
    )


@router.post("/extract-params")
async def extract_strategy_params(payload: dict):
    """从策略代码中提取参数定义，供前端动态渲染参数面板"""
    code = payload.get("code", "")
    if not code.strip():
        return {"params": []}
    params = extract_params(code)
    return {"params": params}
