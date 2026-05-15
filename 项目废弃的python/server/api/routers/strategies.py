from typing import List
from uuid import UUID
from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select

from server.api.core.database import get_db
from server.api.models.models import User, Strategy
from server.api.schemas.schemas import StrategyCreate, StrategyUpdate, StrategyOut
from server.api.dependencies import get_current_user
from server.backtest.sandbox.executor import compile_strategy_code, load_strategy_class, StrategyLoadError, SecurityError

router = APIRouter()


MAX_CODE_LENGTH = 8000


@router.post("/validate")
async def validate_strategy_code(payload: dict):
    """验证策略代码语法、安全性和 Strategy 类完整性"""
    code = payload.get("code", "")
    if not code.strip():
        return {"valid": False, "error": "代码为空"}
    if len(code) > MAX_CODE_LENGTH:
        return {"valid": False, "error": f"策略代码超出长度限制（{MAX_CODE_LENGTH} 字符）"}
    try:
        module = compile_strategy_code(code)
        load_strategy_class(module)
        return {"valid": True, "error": None}
    except (StrategyLoadError, SecurityError) as e:
        return {"valid": False, "error": str(e)}
    except Exception as e:
        return {"valid": False, "error": f"验证异常: {e}"}


@router.get("", response_model=List[StrategyOut])
async def list_strategies(
    skip: int = 0,
    limit: int = 20,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    result = await db.execute(
        select(Strategy)
        .where(Strategy.user_id == current_user.id)
        .offset(skip)
        .limit(limit)
        .order_by(Strategy.created_at.desc())
    )
    return result.scalars().all()


@router.post("", response_model=StrategyOut, status_code=status.HTTP_201_CREATED)
async def create_strategy(
    payload: StrategyCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    strategy = Strategy(
        user_id=current_user.id,
        name=payload.name,
        description=payload.description,
        type=payload.type,
        code=payload.code or "",
        params=payload.params,
    )
    db.add(strategy)
    await db.commit()
    await db.refresh(strategy)
    return strategy


@router.get("/{strategy_id}", response_model=StrategyOut)
async def get_strategy(
    strategy_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    result = await db.execute(
        select(Strategy).where(Strategy.id == strategy_id, Strategy.user_id == current_user.id)
    )
    strategy = result.scalar_one_or_none()
    if not strategy:
        raise HTTPException(status_code=404, detail="Strategy not found")
    return strategy


@router.put("/{strategy_id}", response_model=StrategyOut)
async def update_strategy(
    strategy_id: UUID,
    payload: StrategyUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    result = await db.execute(
        select(Strategy).where(Strategy.id == strategy_id, Strategy.user_id == current_user.id)
    )
    strategy = result.scalar_one_or_none()
    if not strategy:
        raise HTTPException(status_code=404, detail="Strategy not found")

    if payload.name is not None:
        strategy.name = payload.name
    if payload.description is not None:
        strategy.description = payload.description
    if payload.code is not None:
        strategy.code = payload.code
        strategy.version += 1
    if payload.params is not None:
        strategy.params = payload.params

    await db.commit()
    await db.refresh(strategy)
    return strategy


@router.delete("/{strategy_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_strategy(
    strategy_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    result = await db.execute(
        select(Strategy).where(Strategy.id == strategy_id, Strategy.user_id == current_user.id)
    )
    strategy = result.scalar_one_or_none()
    if not strategy:
        raise HTTPException(status_code=404, detail="Strategy not found")
    await db.delete(strategy)
    await db.commit()
    return None
