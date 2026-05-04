# ACDA-Q

A 股量化投资平台 — Tauri 桌面客户端 + Python 后端。

## 简介

ACDA-Q 面向个人量化交易者,提供:

- **AI 辅助策略生成**:基于 DeepSeek 等大模型,自然语言描述生成可执行的 Python 策略代码
- **历史回测**:支持沪深 A 股日线 / 分钟线,内置常用指标和性能分析
- **实时行情**:K 线图表、净值曲线
- **多设备订阅**:基础版 / PRO / MAX 三档,按月 / 季度 / 半年 / 年订阅
- **桌面原生体验**:Tauri 打包,启动快、占用低,跨平台(Windows / macOS / Linux)

## 技术栈

**客户端** `client/`
- Tauri 2.x(Rust shell)+ React 18 + TypeScript
- Vite 5、Zustand、React Router、Lightweight Charts

**后端** `server/`
- Python 3.11 + FastAPI
- SQLAlchemy + asyncpg
- PostgreSQL 16(业务)+ TimescaleDB(行情)
- Redis 7(缓存 / 队列)+ MinIO(对象存储)
- Celery(异步任务)+ APScheduler(定时同步)
- Alembic(数据库迁移)

**部署** 根目录
- Docker Compose(`docker-compose.yml` 开发,`docker-compose.prod.yml` 生产)
- 系统级 nginx 反向代理

## 目录结构

```
ACDA-Q/
├─ client/                  # Tauri 桌面客户端
│  ├─ src/                  # React + TypeScript 源码
│  └─ src-tauri/            # Rust 主进程 + 配置 + icons
├─ server/                  # Python 后端
│  ├─ api/                  # FastAPI 路由 / 模型 / schema
│  ├─ ai/                   # AI 策略生成器 + 验证器
│  ├─ backtest/             # 回测引擎 / 指标 / broker
│  ├─ data/                 # 数据同步(scheduler)
│  ├─ worker/               # Celery 异步任务
│  └─ alembic/              # 数据库迁移
├─ database/migrations/     # SQL 初始化脚本
├─ docs/                    # 项目文档
│  └─ legal/                # 隐私政策 + 用户服务协议
├─ tests/                   # 后端 pytest
├─ nginx/                   # nginx 配置
├─ docker-compose.yml       # 开发用
├─ docker-compose.prod.yml  # 生产用
└─ logo.png                 # 1024×1024 应用图标源
```

## 快速开始

### 后端(本地)

```bash
cp client/.env.example .env  # 复制并按需修改
docker-compose up -d --build
docker exec quant_api bash -c "cd /app/server && alembic upgrade head"
curl http://localhost:8000/health
```

### 客户端(开发模式)

```bash
cd client
npm install
npm run tauri dev
```

应用窗口会自动打开,代码改动热重载,Inspect 可以打开。

### 客户端(打包 dmg / msi)

```bash
cd client
npm run tauri build
# 输出位于 src-tauri/target/release/bundle/{dmg,msi,...}/
```

### 服务器部署

参考 `deploy-server.sh`(在服务器上执行),或:

```bash
# 服务器上
cd ~/ACDA-Q
git pull
docker-compose -f docker-compose.prod.yml up -d --build
docker exec quant_api bash -c "cd /app/server && alembic upgrade head"
```

## 订阅档位

| 档位 | 价格 | AI / 回测调用 |
|------|------|---------------|
| 免费版 | 免费 | 各 3 次 / 日 |
| 基础版 | 9.9 元 / 月 | 各 30 次 / 月 |
| PRO | 19.9 元 / 月 | 各 80 次 / 月 |
| MAX | 99 元 / 月 | 各 500 次 / 月 |

按月 / 季度 / 半年 / 年购买,纯乘法不打折。

## 文档

- [`STRATEGY_GUIDE.md`](STRATEGY_GUIDE.md) — 策略编写规范与示例
- [`TECHNICAL_REVIEW_REPORT.md`](TECHNICAL_REVIEW_REPORT.md) — 技术评审
- [`TECHNICAL_REVIEW_CURRENT.md`](TECHNICAL_REVIEW_CURRENT.md) — 当前架构状态
- [`ITERATION_PLAN.md`](ITERATION_PLAN.md) — 迭代规划
- [`NEXT_STEPS.md`](NEXT_STEPS.md) — 下一步计划
- [`docs/legal/`](docs/legal) — 隐私政策 / 用户服务协议

## 开发约定

参考 [`CLAUDE.md`](CLAUDE.md):

- 不假设、不藏疑问、把权衡说清
- 简洁优先,只写解决问题最少的代码
- 改动只动该动的,不顺手"改进"邻近代码
- 目标驱动:把任务变成可验证的 success criteria

## License

[MIT](LICENSE)
