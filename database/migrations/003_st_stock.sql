-- ST 股涨跌停支持 & 回测缓存表

-- stock_basic 增加 ST 状态字段
ALTER TABLE stock_basic ADD COLUMN IF NOT EXISTS is_st BOOLEAN DEFAULT FALSE;

-- 回测结果缓存表（相同策略+参数+时间范围的结果去重）
CREATE TABLE IF NOT EXISTS backtest_cache (
    cache_hash VARCHAR(64) PRIMARY KEY,
    strategy_id UUID NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
    scope VARCHAR(20),
    symbols TEXT[],
    start_date DATE,
    end_date DATE,
    initial_cash DECIMAL(15, 2),
    params JSONB,
    result_summary JSONB,
    result_report_path VARCHAR(255),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '7 days')
);

CREATE INDEX IF NOT EXISTS idx_backtest_cache_expires ON backtest_cache(expires_at);
