from typing import List, Optional
from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.ext.asyncio import AsyncSession
from starlette.concurrency import run_in_threadpool

from server.api.core.database import get_db
from server.api.dependencies import get_current_user
from server.api.models.models import User
from server.api.core.timescale import query_daily_bars, query_stock_list

router = APIRouter()


@router.get("/stocks")
async def list_stocks(
    exchange: Optional[str] = None,
    search: Optional[str] = None,
    limit: int = 50,
    current_user: User = Depends(get_current_user),
):
    """获取股票列表，支持搜索"""
    try:
        df = await run_in_threadpool(query_stock_list, exchange=exchange, search=search, limit=limit)
        return {
            "total": len(df),
            "items": df.to_dict(orient="records"),
        }
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Query failed: {str(exc)}")


@router.get("/history/{symbol}")
async def get_history(
    symbol: str,
    start_date: str,
    end_date: str,
    current_user: User = Depends(get_current_user),
):
    """获取单标的历史K线"""
    try:
        df = await run_in_threadpool(query_daily_bars, [symbol], start_date, end_date)
        if df.empty:
            return {"symbol": symbol, "data": []}
        df["datetime"] = df["datetime"].astype(str)
        return {
            "symbol": symbol,
            "data": df.to_dict(orient="records"),
        }
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Query failed: {str(exc)}")
