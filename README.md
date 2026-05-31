# ACDA-Quant

[English](README_EN.md) | 中文

A 股量化投资平台 — Rust 高性能回测引擎 + React Web 前端。

## 功能特性

- **AI 策略生成**：接入 DeepSeek 大模型，自然语言描述即可生成可回测的 Python 策略代码
- **历史回测**：支持 A 股 / 港股 / 美股，日线及 1/5/15/30/60 分钟线，内置常用技术指标
- **全市场扫描**：一键扫描全市场标的，按评分排序输出回测结果
- **净值曲线与 K 线图表**：实时渲染净值走势、K 线及交易标记
- **策略管理**：创建、编辑、复制策略，参数可调并自动同步到代码
- **用户与订阅**：JWT 认证，多设备管理，分档订阅（免费 / 基础 / PRO / MAX）

## 技术栈

| 层级 | 技术 |
|------|------|
| **后端** | Rust (Axum + Tokio + SQLx + Redis) |
| **前端** | React 18 + TypeScript + Vite + Zustand |
| **图表** | Lightweight Charts (K 线 / 净值曲线) |
| **数据库** | PostgreSQL 16 (业务) + TimescaleDB (行情时序) |
| **缓存 / 队列** | Redis 7 (Streams 回测任务队列) |
| **沙箱执行** | Python 3 (sandbox_runner.py 隔离执行策略) |
| **AI** | DeepSeek API (策略代码生成) |
| **部署** | Docker Compose / systemd / Nginx 反向代理 |

## 目录结构

```
ACDA-Q/
├── client/                  # React 前端
│   ├── src/
│   │   ├── components/      # 图表、布局等组件
│   │   ├── pages/           # 页面：仪表盘、策略、回测、设置
│   │   ├── stores/          # Zustand 状态管理
│   │   └── services/        # API 请求、AI 生成
│   └── package.json
├── server-rust/             # Rust 后端
│   ├── src/
│   │   ├── api/             # HTTP 路由 (auth, backtest, strategies, ai...)
│   │   ├── backtest/        # 回测引擎 (engine, broker, datafeed, scanner)
│   │   ├── ai/              # DeepSeek 策略生成
│   │   ├── sandbox/         # 策略沙箱隔离执行
│   │   ├── queue.rs         # Redis Streams 任务队列
│   │   └── config.rs        # 环境变量配置
│   ├── migrations/          # SQLx 数据库迁移
│   └── Cargo.toml
├── scripts/                 # 数据同步脚本 (Python)
│   ├── fetch_data.py        # A 股日线 (AkShare)
│   ├── fetch_minute_data.py # 分钟线数据
│   └── fetch_hk_us.py       # 港股 / 美股
├── nginx/                   # Nginx 配置
├── docs/                    # 文档 (开发、法律)
├── sandbox_runner.py        # 策略沙箱执行器
├── docker-compose.rust-only.yml
├── docker-compose.db-only.yml
└── Dockerfile.local
```

## 依赖要求

### 服务端

| 依赖 | 版本 | 说明 |
|------|------|------|
| Rust | ≥ 1.75 | 编译后端 (cargo build --release) |
| Python | 3.10+ | 沙箱执行策略代码、数据同步脚本 |
| PostgreSQL | 16 | 业务数据库 |
| TimescaleDB | latest-pg16 | 行情时序数据 |
| Redis | 7 | 任务队列 + 缓存 |
| Node.js | 18+ | 构建前端 |
| Docker | (可选) | 容器化部署 |

### Python 依赖 (数据同步 + 沙箱)

```bash
pip install pandas sqlalchemy psycopg2-binary akshare requests baostock
```

## 快速开始

### 方式一：Docker Compose (推荐)

```bash
# 1. 克隆仓库
git clone git@github.com:ROGGE668/ACDA-Q.git
cd ACDA-Q

# 2. 配置环境变量
cp .env.example .env
# 编辑 .env，填入 SECRET_KEY、DEEPSEEK_API_KEY 等

# 3. 启动全部服务
docker-compose -f docker-compose.rust-only.yml up -d

# 4. 访问
# Web 前端: http://localhost (Nginx)
# API 直连: http://localhost:8000
```

### 方式二：手动部署

```bash
# --- 后端 ---
cd server-rust

# 安装 Rust (如未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 编译
cargo build --release
# 产物: target/release/acda-q-server

# --- 数据库 ---
# 确保 PostgreSQL + TimescaleDB + Redis 已运行
# 创建数据库
psql -U quant -h 127.0.0.1 -c "CREATE DATABASE quant_db;"
psql -U quant -h 127.0.0.1 -p 5433 -c "CREATE DATABASE quant_market;"

# --- 启动服务 ---
export ACDA_Q__DATABASE_URL=postgresql+asyncpg://quant:quant123@127.0.0.1:5432/quant_db
export ACDA_Q__SYNC_DATABASE_URL=postgresql://quant:quant123@127.0.0.1:5432/quant_db
export ACDA_Q__TIMESCALE_DATABASE_URL=postgresql://quant:quant123@127.0.0.1:5433/quant_market
export ACDA_Q__REDIS_URL=redis://127.0.0.1:6379/0
export ACDA_Q__SECRET_KEY=<至少32字符的随机密钥>

# API 服务
./target/release/acda-q-server

# Worker (回测任务消费)
./target/release/acda-q-server --worker

# --- 前端 ---
cd ../client
npm install
npm run build
# 产物在 client/dist/，由 Rust 服务静态托管
```

### 方式三：仅启动数据库 (本地开发)

```bash
docker-compose -f docker-compose.db-only.yml up -d
# PostgreSQL: localhost:5432
# TimescaleDB: localhost:5433
# Redis: localhost:6379
```

## 环境变量

| 变量 | 必填 | 说明 |
|------|------|------|
| `ACDA_Q__DATABASE_URL` | ✅ | PostgreSQL 连接串 (asyncpg) |
| `ACDA_Q__SYNC_DATABASE_URL` | ✅ | PostgreSQL 同步连接串 |
| `ACDA_Q__TIMESCALE_DATABASE_URL` | ✅ | TimescaleDB 连接串 |
| `ACDA_Q__REDIS_URL` | ✅ | Redis 连接串 |
| `ACDA_Q__SECRET_KEY` | ✅ | JWT 签名密钥 (≥32 字符) |
| `ACDA_Q__DEEPSEEK_API_KEY` | ❌ | AI 策略生成 (不填则功能禁用) |
| `ACDA_Q__CORS_ORIGINS` | ❌ | CORS 允许源 (默认 `*`) |

## 数据同步

```bash
# A 股日线 (每交易日)
python3 scripts/fetch_data.py --sync-today

# 分钟线 (1/5/15/30/60 分钟)
python3 scripts/fetch_minute_data.py --periods 1 5 15 30 60

# 港股 / 美股
python3 scripts/fetch_hk_us.py
```

## systemd 部署 (生产)

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

## 订阅档位

| 档位 | 价格 | AI / 回测调用 |
|------|------|---------------|
| 免费版 | 免费 | 各 3 次 / 日 |
| 基础版 | 9.9 元 / 月 | 各 30 次 / 月 |
| PRO | 19.9 元 / 月 | 各 80 次 / 月 |
| MAX | 99 元 / 月 | 各 500 次 / 月 |

## License

[MIT](LICENSE)
