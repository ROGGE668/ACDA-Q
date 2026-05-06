-- Migration 004: refresh_tokens 表 + backtest_cache 表（如果尚不存在）
-- 日期: 2026-05-05

-- refresh_tokens: 支持 Refresh Token 轮换和重用检测
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    token_jti VARCHAR(36) UNIQUE NOT NULL,
    family_id VARCHAR(36) NOT NULL,
    revoked BOOLEAN DEFAULT FALSE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_jti ON refresh_tokens(token_jti);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_family ON refresh_tokens(family_id);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens(user_id);

-- backtest_cache: 回测结果缓存（如果 001_init.sql 未创建）
CREATE TABLE IF NOT EXISTS backtest_cache (
    cache_hash VARCHAR(64) PRIMARY KEY,
    strategy_id UUID REFERENCES strategies(id) ON DELETE CASCADE,
    scope VARCHAR(20),
    symbols VARCHAR(20)[],
    start_date TIMESTAMP,
    end_date TIMESTAMP,
    initial_cash DECIMAL(15, 2),
    params JSONB DEFAULT '{}',
    result_summary JSONB,
    result_report_path VARCHAR(255),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_backtest_cache_strategy ON backtest_cache(strategy_id);
CREATE INDEX IF NOT EXISTS idx_backtest_cache_expires ON backtest_cache(expires_at);
