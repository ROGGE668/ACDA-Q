import uuid
from datetime import datetime
from sqlalchemy import Column, String, DateTime, Boolean, Integer, Text, DECIMAL, ForeignKey, ARRAY, UniqueConstraint
from sqlalchemy.dialects.postgresql import UUID, JSONB
from server.api.core.database import Base


class User(Base):
    __tablename__ = "users"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    email = Column(String(255), unique=True, nullable=False)
    password_hash = Column(String(255), nullable=False)
    nickname = Column(String(50), nullable=True)
    is_admin = Column(Boolean, default=False, nullable=False)
    tier = Column(String(20), default="free")
    quota_ai_daily = Column(Integer, default=5)
    quota_backtest_daily = Column(Integer, default=10)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class Strategy(Base):
    __tablename__ = "strategies"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey("users.id"), nullable=False)
    name = Column(String(255), nullable=False)
    description = Column(Text, nullable=True)
    type = Column(String(50), nullable=True)  # single_stock / multi_stock / market_scan
    code = Column(Text, nullable=False)
    params = Column(JSONB, default={})
    version = Column(Integer, default=1)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class BacktestJob(Base):
    __tablename__ = "backtest_jobs"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey("users.id"), nullable=False)
    strategy_id = Column(UUID(as_uuid=True), ForeignKey("strategies.id"), nullable=True)
    status = Column(String(20), default="pending")  # pending/running/success/failed
    scope = Column(String(20), nullable=True)  # single/multi/scan
    symbols = Column(ARRAY(String))
    start_date = Column(DateTime, nullable=True)
    end_date = Column(DateTime, nullable=True)
    initial_cash = Column(DECIMAL(15, 2), default=1000000)
    params = Column(JSONB, default={})
    result_summary = Column(JSONB, nullable=True)
    result_report_path = Column(String(255), nullable=True)
    error_message = Column(Text, nullable=True)
    started_at = Column(DateTime, nullable=True)
    completed_at = Column(DateTime, nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)


class BacktestCache(Base):
    __tablename__ = "backtest_cache"

    cache_hash = Column(String(64), primary_key=True)
    strategy_id = Column(UUID(as_uuid=True), ForeignKey("strategies.id", ondelete="CASCADE"), nullable=False)
    scope = Column(String(20), nullable=True)
    symbols = Column(ARRAY(String), nullable=True)
    start_date = Column(DateTime, nullable=True)
    end_date = Column(DateTime, nullable=True)
    initial_cash = Column(DECIMAL(15, 2), nullable=True)
    params = Column(JSONB, default={})
    result_summary = Column(JSONB, nullable=True)
    result_report_path = Column(String(255), nullable=True)
    created_at = Column(DateTime(timezone=True), default=datetime.utcnow)
    expires_at = Column(DateTime(timezone=True), nullable=True)


class RefreshToken(Base):
    __tablename__ = "refresh_tokens"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey("users.id", ondelete="CASCADE"), nullable=False)
    token_jti = Column(String(36), unique=True, nullable=False, index=True)
    family_id = Column(String(36), nullable=False, index=True)
    revoked = Column(Boolean, default=False)
    expires_at = Column(DateTime, nullable=False)
    created_at = Column(DateTime, default=datetime.utcnow)


class AIGeneration(Base):
    __tablename__ = "ai_generations"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey("users.id"), nullable=False)
    prompt = Column(Text, nullable=False)
    generated_code = Column(Text, nullable=True)
    model = Column(String(50), nullable=True)
    tokens_used = Column(Integer, nullable=True)
    status = Column(String(20), nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)


class UserDevice(Base):
    __tablename__ = "user_devices"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey("users.id", ondelete="CASCADE"), nullable=False)
    device_fingerprint = Column(String(64), nullable=False, index=True)
    device_name = Column(String(255), nullable=True)
    os_type = Column(String(50), nullable=True)
    last_heartbeat_at = Column(DateTime, nullable=True)
    is_active = Column(Boolean, default=True)
    revoked_at = Column(DateTime, nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)

    __table_args__ = (UniqueConstraint("user_id", "device_fingerprint", name="uq_user_device"),)


class Subscription(Base):
    __tablename__ = "subscriptions"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey("users.id", ondelete="CASCADE"), nullable=False, unique=True)
    tier = Column(String(20), default="free")  # free / pro / enterprise
    status = Column(String(20), default="active")  # active / expired / cancelled / suspended
    expires_at = Column(DateTime, nullable=True)
    max_devices = Column(Integer, default=1)
    ai_quota_daily = Column(Integer, default=5)
    backtest_quota_daily = Column(Integer, default=10)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class PaymentOrder(Base):
    __tablename__ = "payment_orders"

    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey("users.id", ondelete="CASCADE"), nullable=False)
    order_no = Column(String(64), unique=True, nullable=False, index=True)
    channel = Column(String(20), nullable=False)  # alipay / wechat
    amount_cents = Column(Integer, nullable=False)
    tier = Column(String(20), nullable=False)
    duration_months = Column(Integer, nullable=False)
    status = Column(String(20), default="pending")  # pending / paid / failed / refunded
    paid_at = Column(DateTime, nullable=True)
    channel_transaction_id = Column(String(128), nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)
