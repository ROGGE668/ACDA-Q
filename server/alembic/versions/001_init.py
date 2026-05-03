"""
Initial migration: create core tables

Revision ID: 001
Revises:
Create Date: 2026-05-02

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

# revision identifiers, used by Alembic.
revision: str = "001"
down_revision: Union[str, None] = None
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    op.execute("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\"")

    op.create_table(
        "users",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("email", sa.String(255), unique=True, nullable=False),
        sa.Column("phone", sa.String(20), nullable=True),
        sa.Column("password_hash", sa.String(255), nullable=False),
        sa.Column("nickname", sa.String(50), nullable=True),
        sa.Column("tier", sa.String(20), server_default="free"),
        sa.Column("quota_backtest_daily", sa.Integer, server_default="10"),
        sa.Column("quota_ai_daily", sa.Integer, server_default="5"),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
        sa.Column("updated_at", sa.DateTime, server_default=sa.text("NOW()"), onupdate=sa.text("NOW()")),
    )

    op.create_table(
        "strategies",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("user_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("users.id", ondelete="CASCADE"), nullable=False),
        sa.Column("name", sa.String(255), nullable=False),
        sa.Column("description", sa.Text, nullable=True),
        sa.Column("type", sa.String(50), nullable=True),
        sa.Column("code", sa.Text, nullable=False),
        sa.Column("params", postgresql.JSONB, server_default="{}"),
        sa.Column("is_public", sa.Boolean, server_default="false"),
        sa.Column("version", sa.Integer, server_default="1"),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
        sa.Column("updated_at", sa.DateTime, server_default=sa.text("NOW()"), onupdate=sa.text("NOW()")),
    )
    op.create_index("idx_strategies_user", "strategies", ["user_id"])

    op.create_table(
        "backtest_jobs",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("user_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("users.id", ondelete="CASCADE"), nullable=False),
        sa.Column("strategy_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("strategies.id", ondelete="CASCADE"), nullable=False),
        sa.Column("status", sa.String(20), server_default="pending"),
        sa.Column("scope", sa.String(20), nullable=True),
        sa.Column("symbols", sa.ARRAY(sa.String)),
        sa.Column("start_date", sa.Date, nullable=True),
        sa.Column("end_date", sa.Date, nullable=True),
        sa.Column("initial_cash", sa.DECIMAL(15, 2), server_default="1000000"),
        sa.Column("params", postgresql.JSONB, server_default="{}"),
        sa.Column("result_summary", postgresql.JSONB, nullable=True),
        sa.Column("result_report_path", sa.String(255), nullable=True),
        sa.Column("error_message", sa.Text, nullable=True),
        sa.Column("started_at", sa.DateTime, nullable=True),
        sa.Column("completed_at", sa.DateTime, nullable=True),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
    )
    op.create_index("idx_backtest_jobs_user", "backtest_jobs", ["user_id"])
    op.create_index("idx_backtest_jobs_status", "backtest_jobs", ["status"])

    op.create_table(
        "ai_generations",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("user_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("users.id", ondelete="CASCADE"), nullable=False),
        sa.Column("prompt", sa.Text, nullable=False),
        sa.Column("generated_code", sa.Text, nullable=True),
        sa.Column("model", sa.String(50), nullable=True),
        sa.Column("tokens_used", sa.Integer, nullable=True),
        sa.Column("status", sa.String(20), nullable=True),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
    )

    op.create_table(
        "backtest_cache",
        sa.Column("cache_hash", sa.String(64), primary_key=True),
        sa.Column("strategy_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("strategies.id", ondelete="CASCADE"), nullable=False),
        sa.Column("scope", sa.String(20), nullable=True),
        sa.Column("symbols", sa.ARRAY(sa.String), nullable=True),
        sa.Column("start_date", sa.Date, nullable=True),
        sa.Column("end_date", sa.Date, nullable=True),
        sa.Column("initial_cash", sa.DECIMAL(15, 2), nullable=True),
        sa.Column("params", postgresql.JSONB, server_default="{}"),
        sa.Column("result_summary", postgresql.JSONB, nullable=True),
        sa.Column("result_report_path", sa.String(255), nullable=True),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
        sa.Column("expires_at", sa.DateTime, nullable=True),
    )
    op.create_index("idx_backtest_cache_expires", "backtest_cache", ["expires_at"])

    op.create_table(
        "refresh_tokens",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True, server_default=sa.text("uuid_generate_v4()")),
        sa.Column("user_id", postgresql.UUID(as_uuid=True), sa.ForeignKey("users.id", ondelete="CASCADE"), nullable=False),
        sa.Column("token_jti", sa.String(36), unique=True, nullable=False),
        sa.Column("family_id", sa.String(36), nullable=False),
        sa.Column("revoked", sa.Boolean, server_default="false"),
        sa.Column("expires_at", sa.DateTime, nullable=False),
        sa.Column("created_at", sa.DateTime, server_default=sa.text("NOW()")),
    )
    op.create_index("idx_refresh_tokens_jti", "refresh_tokens", ["token_jti"])
    op.create_index("idx_refresh_tokens_family", "refresh_tokens", ["family_id"])


def downgrade() -> None:
    op.drop_table("refresh_tokens")
    op.drop_table("backtest_cache")
    op.drop_table("ai_generations")
    op.drop_table("backtest_jobs")
    op.drop_table("strategies")
    op.drop_table("users")
