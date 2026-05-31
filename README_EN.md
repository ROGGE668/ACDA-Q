# ACDA-Quant

English | [中文](README.md)

A-share quantitative investment platform — high-performance Rust backtest engine + React web frontend.

## Features

- **AI Strategy Generation**: Generate testable Python strategy code from natural language via DeepSeek LLM
- **Historical Backtesting**: Supports A-shares / HK / US stocks, daily and 1/5/15/30/60-minute bars with built-in technical indicators
- **Full Market Scan**: One-click scan of entire market, ranked by performance score
- **Equity Curve & K-Line Charts**: Real-time rendering of equity curves, candlestick charts, and trade markers
- **Strategy Management**: Create, edit, clone strategies with auto-synced parameters
- **Auth & Subscriptions**: JWT authentication, multi-device management, tiered subscriptions (Free / Basic / PRO / MAX)

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **Backend** | Rust (Axum + Tokio + SQLx + Redis) |
| **Frontend** | React 18 + TypeScript + Vite + Zustand |
| **Charts** | Lightweight Charts (K-line / equity curve) |
| **Database** | PostgreSQL 16 (business) + TimescaleDB (market time-series) |
| **Cache / Queue** | Redis 7 (Streams for backtest task queue) |
| **Sandbox** | Python 3 (sandbox_runner.py for isolated strategy execution) |
| **AI** | DeepSeek API (strategy code generation) |
| **Deploy** | Docker Compose / systemd / Nginx reverse proxy |

## Project Structure

```
ACDA-Q/
├── client/                  # React frontend
│   ├── src/
│   │   ├── components/      # Charts, layout, etc.
│   │   ├── pages/           # Dashboard, strategies, backtest, settings
│   │   ├── stores/          # Zustand state management
│   │   └── services/        # API requests, AI generation
│   └── package.json
├── server-rust/             # Rust backend
│   ├── src/
│   │   ├── api/             # HTTP routes (auth, backtest, strategies, ai...)
│   │   ├── backtest/        # Backtest engine (engine, broker, datafeed, scanner)
│   │   ├── ai/              # DeepSeek strategy generation
│   │   ├── sandbox/         # Strategy sandbox execution
│   │   ├── queue.rs         # Redis Streams task queue
│   │   └── config.rs        # Environment config
│   ├── migrations/          # SQLx database migrations
│   └── Cargo.toml
├── scripts/                 # Data sync scripts (Python)
│   ├── fetch_data.py        # A-share daily bars (AkShare)
│   ├── fetch_minute_data.py # Minute-level data
│   └── fetch_hk_us.py       # HK / US stocks
├── nginx/                   # Nginx configs
├── docs/                    # Documentation (dev, legal)
├── sandbox_runner.py        # Strategy sandbox executor
├── docker-compose.rust-only.yml
├── docker-compose.db-only.yml
└── Dockerfile.local
```

## Prerequisites

### Server-side

| Dependency | Version | Purpose |
|-----------|---------|---------|
| Rust | ≥ 1.75 | Compile backend (cargo build --release) |
| Python | 3.10+ | Strategy sandbox execution, data sync scripts |
| PostgreSQL | 16 | Business database |
| TimescaleDB | latest-pg16 | Market time-series data |
| Redis | 7 | Task queue + cache |
| Node.js | 18+ | Build frontend |
| Docker | (optional) | Containerized deployment |

### Python Dependencies (data sync + sandbox)

```bash
pip install pandas sqlalchemy psycopg2-binary akshare requests baostock
```

## Quick Start

### Option 1: Docker Compose (Recommended)

```bash
# 1. Clone the repository
git clone git@github.com:ROGGE668/ACDA-Q.git
cd ACDA-Q

# 2. Configure environment variables
cp .env.example .env
# Edit .env — set SECRET_KEY, DEEPSEEK_API_KEY, etc.

# 3. Start all services
docker-compose -f docker-compose.rust-only.yml up -d

# 4. Access
# Web frontend: http://localhost (via Nginx)
# API direct:   http://localhost:8000
```

### Option 2: Manual Deployment

```bash
# --- Backend ---
cd server-rust

# Install Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release
# Binary: target/release/acda-q-server

# --- Database ---
# Ensure PostgreSQL + TimescaleDB + Redis are running
psql -U quant -h 127.0.0.1 -c "CREATE DATABASE quant_db;"
psql -U quant -h 127.0.0.1 -p 5433 -c "CREATE DATABASE quant_market;"

# --- Start Services ---
export ACDA_Q__DATABASE_URL=postgresql+asyncpg://quant:quant123@127.0.0.1:5432/quant_db
export ACDA_Q__SYNC_DATABASE_URL=postgresql://quant:quant123@127.0.0.1:5432/quant_db
export ACDA_Q__TIMESCALE_DATABASE_URL=postgresql://quant:quant123@127.0.0.1:5433/quant_market
export ACDA_Q__REDIS_URL=redis://127.0.0.1:6379/0
export ACDA_Q__SECRET_KEY=<random-key-at-least-32-chars>

# API server
./target/release/acda-q-server

# Worker (consumes backtest tasks)
./target/release/acda-q-server --worker

# --- Frontend ---
cd ../client
npm install
npm run build
# Output in client/dist/, served by Rust as static files
```

### Option 3: Database Only (Local Dev)

```bash
docker-compose -f docker-compose.db-only.yml up -d
# PostgreSQL:  localhost:5432
# TimescaleDB: localhost:5433
# Redis:       localhost:6379
```

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `ACDA_Q__DATABASE_URL` | ✅ | PostgreSQL connection string (asyncpg) |
| `ACDA_Q__SYNC_DATABASE_URL` | ✅ | PostgreSQL synchronous connection |
| `ACDA_Q__TIMESCALE_DATABASE_URL` | ✅ | TimescaleDB connection string |
| `ACDA_Q__REDIS_URL` | ✅ | Redis connection string |
| `ACDA_Q__SECRET_KEY` | ✅ | JWT signing key (≥32 chars) |
| `ACDA_Q__DEEPSEEK_API_KEY` | ❌ | AI strategy generation (disabled if empty) |
| `ACDA_Q__CORS_ORIGINS` | ❌ | CORS allowed origins (default `*`) |

## Data Sync

```bash
# A-share daily bars (each trading day)
python3 scripts/fetch_data.py --sync-today

# Minute-level data (1/5/15/30/60 min)
python3 scripts/fetch_minute_data.py --periods 1 5 15 30 60

# HK / US stocks
python3 scripts/fetch_hk_us.py
```

## systemd Deployment (Production)

```ini
# ~/.config/systemd/user/acda-q.service
[Unit]
Description=ACDA-Q API Server
After=network.target

[Service]
Type=simple
WorkingDirectory=/home/hong/ACDA-Q-RUST-v1
ExecStart=/home/hong/ACDA-Q-RUST-v1/start-server.sh
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

## Subscription Tiers

| Tier | Price | AI / Backtest Calls |
|------|-------|---------------------|
| Free | Free | 3 / day each |
| Basic | ¥9.9 / month | 30 / month each |
| PRO | ¥19.9 / month | 80 / month each |
| MAX | ¥99 / month | 500 / month each |

## License

[MIT](LICENSE)
