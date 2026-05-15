from datetime import datetime, date
from typing import Optional, List, Dict, Any
from pydantic import BaseModel, EmailStr, Field
from uuid import UUID


# ========== Auth ==========
class UserRegister(BaseModel):
    email: EmailStr
    password: str
    nickname: Optional[str] = None


class UserLogin(BaseModel):
    email: EmailStr
    password: str


class TokenPair(BaseModel):
    access_token: str
    refresh_token: str
    token_type: str = "bearer"


class RefreshRequest(BaseModel):
    token: str


class StatusResponse(BaseModel):
    status: str = "ok"
    access_token: Optional[str] = None
    refresh_token: Optional[str] = None


class UserOut(BaseModel):
    id: UUID
    email: str
    nickname: Optional[str]
    tier: str
    is_admin: bool = False
    quota_ai_daily: int
    ai_used_today: int = 0
    created_at: datetime

    class Config:
        from_attributes = True


# ========== Strategy ==========
class StrategyCreate(BaseModel):
    name: str
    description: Optional[str] = None
    type: Optional[str] = "single_stock"
    code: Optional[str] = Field(default=None, max_length=8000)
    params: Optional[Dict[str, Any]] = {}


class StrategyUpdate(BaseModel):
    name: Optional[str] = None
    description: Optional[str] = None
    code: Optional[str] = Field(default=None, max_length=8000)
    params: Optional[Dict[str, Any]] = None


class StrategyOut(BaseModel):
    id: UUID
    user_id: UUID
    name: str
    description: Optional[str]
    type: Optional[str]
    code: Optional[str]
    params: Dict[str, Any]
    version: int
    created_at: datetime
    updated_at: datetime

    class Config:
        from_attributes = True


# ========== AI Generation ==========
class AIGenerateRequest(BaseModel):
    prompt: str
    model: Optional[str] = None


class AIGenerateResponse(BaseModel):
    generated_code: str
    model: str
    tokens_used: Optional[int] = None


# ========== Backtest ==========
class BacktestSubmit(BaseModel):
    strategy_id: Optional[UUID] = None
    strategy_code: Optional[str] = Field(default=None, max_length=16000)
    symbols: List[str]
    start_date: Optional[date] = None
    end_date: Optional[date] = None
    initial_cash: Optional[float] = 1_000_000.0
    scope: Optional[str] = "single"  # single / portfolio / scan
    params: Optional[Dict[str, Any]] = {}


class BacktestJobOut(BaseModel):
    id: UUID
    status: str
    scope: Optional[str]
    symbols: Optional[List[str]]
    start_date: Optional[datetime]
    end_date: Optional[datetime]
    initial_cash: float
    result_summary: Optional[Dict[str, Any]]
    result_report_path: Optional[str]
    error_message: Optional[str]
    created_at: datetime
    completed_at: Optional[datetime]

    class Config:
        from_attributes = True


class BacktestResult(BaseModel):
    job_id: UUID
    summary: Dict[str, Any]
    trades: List[Dict[str, Any]] = []
    equity_curve: List[Dict[str, Any]] = []
    signals: List[Dict[str, Any]] = []
    suitable_stocks: List[Dict[str, Any]] = []
    unsuitable_stocks: List[Dict[str, Any]] = []
    report_path: Optional[str] = None


# ========== Device & Subscription ==========
class DeviceRegister(BaseModel):
    device_fingerprint: str = Field(..., min_length=64, max_length=64)
    device_name: Optional[str] = None
    os_type: Optional[str] = None


class DeviceHeartbeat(BaseModel):
    device_fingerprint: str = Field(..., min_length=64, max_length=64)


class DeviceOut(BaseModel):
    id: UUID
    device_fingerprint: str
    device_name: Optional[str]
    os_type: Optional[str]
    last_heartbeat_at: Optional[datetime]
    is_active: bool
    revoked_at: Optional[datetime]
    created_at: datetime

    class Config:
        from_attributes = True


class SubscriptionOut(BaseModel):
    id: UUID
    user_id: UUID
    tier: str
    status: str
    expires_at: Optional[datetime]
    max_devices: int
    ai_quota_daily: int
    backtest_quota_daily: int
    created_at: datetime
    updated_at: datetime

    class Config:
        from_attributes = True


class SubscriptionStatus(BaseModel):
    tier: str
    status: str
    expires_at: Optional[datetime]
    max_devices: int
    ai_quota_daily: int
    backtest_quota_daily: int
    devices_active: int
    ai_used_today: int = 0
    backtest_used_today: int = 0


# ========== Payment ==========
class PaymentCreate(BaseModel):
    channel: str = Field(..., pattern="^(alipay|wechat)$")
    tier: str = Field(..., pattern="^(pro|enterprise)$")
    duration_months: int = Field(..., ge=1, le=36)


class PaymentOut(BaseModel):
    id: UUID
    order_no: str
    channel: str
    amount_cents: int
    tier: str
    duration_months: int
    status: str
    paid_at: Optional[datetime]
    created_at: datetime

    class Config:
        from_attributes = True


class PaymentQRPayload(BaseModel):
    order_no: str
    qr_code_url: str
    amount_cents: int
    expires_at: datetime
