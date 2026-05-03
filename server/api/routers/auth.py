from datetime import datetime, timezone
from fastapi import APIRouter, Depends, HTTPException, status, Response, Request
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, update, func

from server.api.core.database import get_db
from server.api.core.security import (
    verify_password, get_password_hash, create_access_token, create_refresh_token, decode_token
)
from server.api.core.config import get_settings
from server.api.core.rate_limit import rate_limit_auth
from server.api.models.models import User, RefreshToken, AIGeneration, Subscription
from server.api.schemas.schemas import UserRegister, UserLogin, UserOut, StatusResponse
from server.api.dependencies import get_current_user

router = APIRouter()
settings = get_settings()


def _set_auth_cookies(response: Response, access_token: str, refresh_token: str) -> None:
    """设置 HttpOnly Cookie，不再返回 token 到 JSON body。"""
    response.set_cookie(
        key="access_token",
        value=access_token,
        httponly=True,
        secure=settings.COOKIE_SECURE,
        samesite="lax",
        max_age=settings.ACCESS_TOKEN_EXPIRE_MINUTES * 60,
    )
    response.set_cookie(
        key="refresh_token",
        value=refresh_token,
        httponly=True,
        secure=settings.COOKIE_SECURE,
        samesite="lax",
        max_age=settings.REFRESH_TOKEN_EXPIRE_DAYS * 24 * 60 * 60,
    )


def _extract_refresh_meta(token: str):
    """从刚生成的 refresh token 中提取 jti/family/exp"""
    payload = decode_token(token)
    if not payload:
        raise RuntimeError("Failed to decode freshly created refresh token")
    return (
        payload["jti"],
        payload["family"],
        datetime.fromtimestamp(payload["exp"], tz=timezone.utc).replace(tzinfo=None),
    )


@router.post("/register", response_model=StatusResponse)
async def register(
    payload: UserRegister,
    response: Response,
    request: Request,
    db: AsyncSession = Depends(get_db),
):
    await rate_limit_auth(request)

    # 密码强度校验
    if len(payload.password) < 8:
        raise HTTPException(status_code=400, detail="Password must be at least 8 characters")
    if not any(c.isupper() for c in payload.password):
        raise HTTPException(status_code=400, detail="Password must contain at least one uppercase letter")
    if not any(c.islower() for c in payload.password):
        raise HTTPException(status_code=400, detail="Password must contain at least one lowercase letter")
    if not any(c.isdigit() for c in payload.password):
        raise HTTPException(status_code=400, detail="Password must contain at least one digit")

    result = await db.execute(select(User).where(User.email == payload.email))
    existing = result.scalar_one_or_none()
    if existing:
        raise HTTPException(status_code=400, detail="Email already registered")

    user = User(
        email=payload.email,
        password_hash=get_password_hash(payload.password),
        nickname=payload.nickname or payload.email.split("@")[0],
    )
    db.add(user)
    await db.commit()
    await db.refresh(user)

    token_data = {"sub": str(user.id)}
    refresh_tok = create_refresh_token(token_data)
    jti, family, exp = _extract_refresh_meta(refresh_tok)
    db.add(RefreshToken(user_id=user.id, token_jti=jti, family_id=family, expires_at=exp))
    await db.commit()

    access_tok = create_access_token(token_data)
    _set_auth_cookies(response, access_tok, refresh_tok)
    return {"status": "ok", "access_token": access_tok, "refresh_token": refresh_tok}


@router.post("/login", response_model=StatusResponse)
async def login(
    payload: UserLogin,
    response: Response,
    request: Request,
    db: AsyncSession = Depends(get_db),
):
    await rate_limit_auth(request)

    result = await db.execute(select(User).where(User.email == payload.email))
    user = result.scalar_one_or_none()
    if not user or not verify_password(payload.password, user.password_hash):
        raise HTTPException(status_code=401, detail="Invalid credentials")

    token_data = {"sub": str(user.id)}
    refresh_tok = create_refresh_token(token_data)
    jti, family, exp = _extract_refresh_meta(refresh_tok)
    db.add(RefreshToken(user_id=user.id, token_jti=jti, family_id=family, expires_at=exp))
    await db.commit()

    access_tok = create_access_token(token_data)
    _set_auth_cookies(response, access_tok, refresh_tok)
    return {"status": "ok", "access_token": access_tok, "refresh_token": refresh_tok}


@router.post("/refresh", response_model=StatusResponse)
async def refresh(request: Request, response: Response, db: AsyncSession = Depends(get_db)):
    # 优先从 Cookie 读取，其次从 Authorization Header 读取
    refresh_token = request.cookies.get("refresh_token")
    if not refresh_token:
        auth_header = request.headers.get("Authorization", "")
        if auth_header.startswith("Bearer "):
            refresh_token = auth_header[7:]
    if not refresh_token:
        raise HTTPException(status_code=401, detail="Missing refresh token")

    token_data = decode_token(refresh_token)
    if not token_data or token_data.get("type") != "refresh":
        raise HTTPException(status_code=401, detail="Invalid refresh token")

    user_id = token_data.get("sub")
    old_jti = token_data.get("jti")
    old_family = token_data.get("family")
    if not user_id or not old_jti or not old_family:
        raise HTTPException(status_code=401, detail="Invalid token payload")

    # 校验 refresh token 是否存在于数据库且未撤销
    result = await db.execute(
        select(RefreshToken).where(RefreshToken.token_jti == old_jti)
    )
    rt = result.scalar_one_or_none()
    if not rt or rt.revoked:
        # Token 重用检测：如果已撤销，撤销整个 family
        if rt and rt.revoked:
            await db.execute(
                update(RefreshToken)
                .where(RefreshToken.family_id == old_family)
                .values(revoked=True)
            )
            await db.commit()
        raise HTTPException(status_code=401, detail="Refresh token has been revoked")

    # 标记旧 token 为已撤销
    rt.revoked = True
    await db.commit()

    # 确认用户仍然有效
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(status_code=401, detail="User not found")

    # 生成新的 token pair（保持同一 family）
    new_token_data = {"sub": str(user.id)}
    new_refresh_tok = create_refresh_token(new_token_data, family=old_family)
    jti, family, exp = _extract_refresh_meta(new_refresh_tok)
    db.add(RefreshToken(user_id=user.id, token_jti=jti, family_id=family, expires_at=exp))
    await db.commit()

    access_tok = create_access_token(new_token_data)
    _set_auth_cookies(response, access_tok, new_refresh_tok)
    return {"status": "ok", "access_token": access_tok, "refresh_token": new_refresh_tok}


@router.post("/logout", response_model=StatusResponse)
async def logout(request: Request, response: Response, db: AsyncSession = Depends(get_db)):
    refresh_token = request.cookies.get("refresh_token")
    if refresh_token:
        token_data = decode_token(refresh_token)
        if token_data:
            jti = token_data.get("jti")
            family = token_data.get("family")
            if jti:
                result = await db.execute(
                    select(RefreshToken).where(RefreshToken.token_jti == jti)
                )
                rt = result.scalar_one_or_none()
                if rt:
                    if family:
                        await db.execute(
                            update(RefreshToken)
                            .where(RefreshToken.family_id == family)
                            .values(revoked=True)
                        )
                    else:
                        rt.revoked = True
                    await db.commit()

    response.delete_cookie(
        key="access_token",
        httponly=True,
        secure=settings.COOKIE_SECURE,
        samesite="lax",
    )
    response.delete_cookie(
        key="refresh_token",
        httponly=True,
        secure=settings.COOKIE_SECURE,
        samesite="lax",
    )
    return {"status": "ok"}


@router.get("/me", response_model=UserOut)
async def get_me(current_user: User = Depends(get_current_user), db: AsyncSession = Depends(get_db)):
    today_start = datetime.utcnow().replace(hour=0, minute=0, second=0, microsecond=0)
    result = await db.execute(
        select(func.count(AIGeneration.id)).where(
            AIGeneration.user_id == current_user.id,
            AIGeneration.created_at >= today_start,
        )
    )
    ai_used_today = result.scalar() or 0

    # 优先从订阅表获取配额
    sub_result = await db.execute(
        select(Subscription).where(Subscription.user_id == current_user.id)
    )
    sub = sub_result.scalar_one_or_none()

    return {
        "id": current_user.id,
        "email": current_user.email,
        "nickname": current_user.nickname,
        "tier": sub.tier if sub else current_user.tier,
        "is_admin": current_user.is_admin,
        "quota_ai_daily": sub.ai_quota_daily if sub else current_user.quota_ai_daily,
        "ai_used_today": ai_used_today,
        "created_at": current_user.created_at,
    }
