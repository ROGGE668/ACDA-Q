-- 002_market_data.sql — TimescaleDB 行情数据表

-- 加载 TimescaleDB 扩展（仅在扩展可用时执行）
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'timescaledb') THEN
    CREATE EXTENSION IF NOT EXISTS timescaledb;
  END IF;
END
$$;

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

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'timescaledb') THEN
    PERFORM create_hypertable('daily_bars', 'datetime', if_not_exists => TRUE);
  END IF;
END $$;

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

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'timescaledb') THEN
    PERFORM create_hypertable('minute_bars', 'datetime', if_not_exists => TRUE);
  END IF;
END $$;

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

-- TimescaleDB 压缩策略：7 天后自动压缩
DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'timescaledb') THEN
    ALTER TABLE daily_bars SET (timescaledb.compress, timescaledb.compress_segmentby = 'symbol');
    PERFORM add_compression_policy('daily_bars', INTERVAL '7 days', if_not_exists => TRUE);
  END IF;
END $$;

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'timescaledb') THEN
    ALTER TABLE minute_bars SET (timescaledb.compress, timescaledb.compress_segmentby = 'symbol');
    PERFORM add_compression_policy('minute_bars', INTERVAL '7 days', if_not_exists => TRUE);
  END IF;
END $$;
