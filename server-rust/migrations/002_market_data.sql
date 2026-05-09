-- 002_market_data.sql — TimescaleDB 行情数据表

-- 加载 TimescaleDB 扩展
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- 股票基础信息表
CREATE TABLE IF NOT EXISTS stock_basic (
    symbol VARCHAR(20) PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    exchange VARCHAR(10),
    industry VARCHAR(50),
    list_date DATE,
    total_shares BIGINT,
    float_shares BIGINT,
    is_st BOOLEAN DEFAULT FALSE,
    is_active BOOLEAN DEFAULT TRUE
);

-- 日K线数据 hypertable
CREATE TABLE IF NOT EXISTS daily_bars (
    symbol VARCHAR(20) NOT NULL,
    datetime TIMESTAMPTZ NOT NULL,
    open DECIMAL(12, 4),
    high DECIMAL(12, 4),
    low DECIMAL(12, 4),
    close DECIMAL(12, 4),
    volume BIGINT,
    amount DECIMAL(20, 4),
    pre_close DECIMAL(12, 4),
    change_pct DECIMAL(8, 4),
    PRIMARY KEY (symbol, datetime)
);

SELECT create_hypertable('daily_bars', 'datetime', if_not_exists => TRUE);

-- 分钟K线数据 hypertable
CREATE TABLE IF NOT EXISTS minute_bars (
    symbol VARCHAR(20) NOT NULL,
    datetime TIMESTAMPTZ NOT NULL,
    open DECIMAL(12, 4),
    high DECIMAL(12, 4),
    low DECIMAL(12, 4),
    close DECIMAL(12, 4),
    volume BIGINT,
    amount DECIMAL(20, 4),
    PRIMARY KEY (symbol, datetime)
);

SELECT create_hypertable('minute_bars', 'datetime', if_not_exists => TRUE);

-- 复权因子表
CREATE TABLE IF NOT EXISTS adj_factors (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    adj_factor DECIMAL(16, 10) NOT NULL,
    PRIMARY KEY (symbol, trade_date)
);

-- 回测缓存表
CREATE TABLE IF NOT EXISTS backtest_cache (
    cache_hash VARCHAR(64) PRIMARY KEY,
    strategy_id UUID REFERENCES strategies(id) ON DELETE CASCADE NOT NULL,
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

-- Indexes
CREATE INDEX IF NOT EXISTS idx_daily_bars_symbol ON daily_bars(symbol, datetime DESC);
CREATE INDEX IF NOT EXISTS idx_minute_bars_symbol ON minute_bars(symbol, datetime DESC);
CREATE INDEX IF NOT EXISTS idx_stock_basic_industry ON stock_basic(industry);
CREATE INDEX IF NOT EXISTS idx_backtest_cache_expires ON backtest_cache(expires_at);
