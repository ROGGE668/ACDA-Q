"""
Add admin flag, subscription/device/payment tables, and fix strategy_id nullable

Revision ID: 002
Revises: 001
Create Date: 2026-05-02

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

revision: str = "002"
down_revision: Union[str, None] = "001"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # 1. Add is_admin to users
    op.add_column(
        "users",
        sa.Column("is_admin", sa.Boolean(), server_default="false", nullable=False),
    )

    # 2. Create user_devices table
    op.create_table(
        "user_devices",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("user_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("users.id", ondelete="CASCADE"), nullable=False),
        sa.Column("device_fingerprint", sa.String(64), nullable=False),
        sa.Column("device_name", sa.String(255), nullable=True),
        sa.Column("os_type", sa.String(50), nullable=True),
        sa.Column("last_heartbeat_at", sa.DateTime, nullable=True),
        sa.Column("is_active", sa.Boolean, server_default="true"),
        sa.Column("revoked_at", sa.DateTime, nullable=True),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
    )
    op.create_index("idx_user_devices_fingerprint", "user_devices", ["device_fingerprint"])
    op.create_index("idx_user_devices_user", "user_devices", ["user_id"])
    op.create_unique_constraint("uq_user_device", "user_devices", ["user_id", "device_fingerprint"])

    # 3. Create subscriptions table
    op.create_table(
        "subscriptions",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("user_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("users.id", ondelete="CASCADE"), nullable=False, unique=True),
        sa.Column("tier", sa.String(20), server_default="free"),
        sa.Column("status", sa.String(20), server_default="active"),
        sa.Column("expires_at", sa.DateTime, nullable=True),
        sa.Column("max_devices", sa.Integer, server_default="1"),
        sa.Column("ai_quota_daily", sa.Integer, server_default="5"),
        sa.Column("backtest_quota_daily", sa.Integer, server_default="10"),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
        sa.Column("updated_at", sa.DateTime, server_default=sa.text("NOW()"), onupdate=sa.text("NOW()")),
    )

    # 4. Create payment_orders table
    op.create_table(
        "payment_orders",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("user_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("users.id", ondelete="CASCADE"), nullable=False),
        sa.Column("order_no", sa.String(64), unique=True, nullable=False),
        sa.Column("channel", sa.String(20), nullable=False),
        sa.Column("amount_cents", sa.Integer, nullable=False),
        sa.Column("tier", sa.String(20), nullable=False),
        sa.Column("duration_months", sa.Integer, nullable=False),
        sa.Column("status", sa.String(20), server_default="pending"),
        sa.Column("paid_at", sa.DateTime, nullable=True),
        sa.Column("channel_transaction_id", sa.String(128), nullable=True),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
    )
    op.create_index("idx_payment_orders_order_no", "payment_orders", ["order_no"])
    op.create_index("idx_payment_orders_user", "payment_orders", ["user_id"])

    # 5. Make backtest_jobs.strategy_id nullable
    op.alter_column("backtest_jobs", "strategy_id",
                     existing_type=postgresql.UUID(as_uuid=True),
                     nullable=True)


def downgrade() -> None:
    op.alter_column("backtest_jobs", "strategy_id",
                     existing_type=postgresql.UUID(as_uuid=True),
                     nullable=False)
    op.drop_table("payment_orders")
    op.drop_table("subscriptions")
    op.drop_table("user_devices")
    op.drop_column("users", "is_admin")
