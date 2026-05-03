# ACDA-Q 量化投资平台 —— 迭代计划 v1.0

**制定日期**: 2026-05-02  
**目标**: 从 Alpha+ 推进到生产就绪 Beta  
**周期**: 4 个 Sprint，约 3-4 周  

---

## 当前状态诊断

### 服务端

| 模块 | 状态 | 核心问题 |
|------|------|---------|
| 回测引擎 | 可用 | 13 个测试通过，核心链路稳定 |
| 安全沙箱 | 可用 | AST 检查 + getattr 封堵，但无进程级隔离 |
| Auth | 可用 | Refresh Token 轮换已落地，但无 httpOnly cookie 支持 |
| API | 可用 | 限流、路径防护、复权数据均已接入 |
| 数据同步 | 可用 | 分区剪枝已修复，但逐只拉取效率低 |
| 基础设施 | **薄弱** | 无 requirements.txt、无 Dockerfile、无生产配置、CORS 只配了 dev |

### App 端（Tauri + React）

| 模块 | 状态 | 核心问题 |
|------|------|---------|
| API 通信 | 可用 | 硬编码公网 IP，localStorage 存 JWT（XSS 风险） |
| 回测结果 | 可用 | 轮询 2 秒一次，无 WebSocket |
| 策略编辑器 | 可用 | 无防抖，频繁触发参数提取 |
| AI 配额 | **显示异常** | 后端不再递减 quota_ai_daily，前端显示永远是 5/5 |
| 设置存储 | 可用 | Tauri secure store 只存了 settings，没存 token |

### 生产环境

| 项 | 状态 |
|----|------|
| Docker / 容器化 | 无 |
| 健康检查 | 仅返回 `{"status":"ok"}`，不检查 DB/Redis/Celery |
| 日志 | 标准 logging，无结构化、无 request_id |
| 监控 | 无 Metrics、无告警 |
| CORS | 只配置了 dev URL |
| 环境变量 | 无 `.env.example` |
| 数据库迁移 | 只有 SQL 文件，无 Alembic 管理 |

---

## Sprint 1：生产基础设施（服务端为主，5 天）

**目标**：让项目可以被干净部署、被 CI 自动测试、被监控。

### 服务端任务

| 编号 | 任务 | 文件 | 验收标准 |
|------|------|------|---------|
| S1-S1 | 补充 `requirements.txt` | 根目录 | `pip install -r requirements.txt` 后 `pytest tests/` 通过 |
| S1-S2 | 补充 `.env.example` | 根目录 | 包含所有必填环境变量及说明 |
| S1-S3 | 补充 `alembic.ini` + `alembic/` 目录 | 新增 | `alembic upgrade head` 可创建所有表（含 refresh_tokens、backtest_cache） |
| S1-S4 | 真实健康检查 | `api/main.py` | `/health` 检查 DB、Redis、Celery 连接状态，任一失败返回 503 |
| S1-S5 | CORS 生产配置 | `api/main.py` | `CORS_ORIGINS` 从环境变量读取，dev 默认 `["*"]`，生产必须显式配置 |
| S1-S6 | 结构化日志 | `api/core/logging.py` | 统一 JSON 格式，每条日志含 request_id、timestamp、level、message |

### App 端任务

| 编号 | 任务 | 文件 | 验收标准 |
|------|------|------|---------|
| S1-A1 | API Base 从环境变量读取 | `api.ts` | 生产构建时通过 `VITE_API_BASE` 注入，不再硬编码公网 IP |
| S1-A2 | 移除 settingsStore 中的默认公网 IP | `settingsStore.ts` | 默认值改为空字符串或 `http://localhost:8000/api/v1`，首次启动提示用户配置 |

### 生产环境任务

| 编号 | 任务 | 说明 |
|------|------|------|
| S1-P1 | `Dockerfile` | Python 3.11 slim，多阶段构建，最终镜像 < 500MB |
| S1-P2 | `docker-compose.yml` | 包含 api、worker、postgres、redis、timescaledb |
| S1-P3 | `docker-compose.prod.yml` | 生产编排：非 root 用户、只读文件系统、资源限制、健康检查探针 |

### Sprint 1 交付物

- 新开发者 `git clone && docker-compose up` 即可跑通全套系统
- CI 可执行 `pytest` + `docker build`

---

## Sprint 2：认证安全 + AI 配额修复（服务端 + App 各半，5 天）

**目标**：消除 JWT XSS 风险，修复 AI 配额显示，支持多 Tab 登录。

### 服务端任务

| 编号 | 任务 | 文件 | 验收标准 |
|------|------|------|---------|
| S2-S1 | Token 改为 Cookie 传输 | `api/routers/auth.py` | Login/Refresh 响应 `Set-Cookie: access_token=...; HttpOnly; Secure; SameSite=Lax`，同时保留 JSON body 中的 token 供过渡期使用 |
| S2-S2 | `/auth/me` 增加 `ai_used_today` | `api/routers/auth.py` | 返回用户今日 AI 调用次数，前端无需自己计算 |
| S2-S3 | 补充 `004_refresh_tokens.sql` | `database/migrations/` | 创建 refresh_tokens 表（如果不用 Alembic 的话） |
| S2-S4 | Prometheus metrics 端点 | `api/main.py` | `/metrics` 暴露：回测队列长度、回测执行耗时、AI 调用次数、HTTP 状态码分布 |
| S2-S5 | 请求追踪 middleware | `api/main.py` | 每个请求注入 `X-Request-ID`，传递到 Celery task 和日志中 |

### App 端任务

| 编号 | 任务 | 文件 | 验收标准 |
|------|------|------|---------|
| S2-A1 | Token 存储迁移到 Tauri secure store | `api.ts` + `authStore.ts` | `localStorage` 不再读写 token，改用 `@tauri-apps/plugin-store` 或 `@tauri-apps/plugin-stronghold` |
| S2-A2 | 多 Tab token 同步 | `api.ts` | 任一 Tab 刷新 token 后，其他 Tab 通过 `storage` 事件或 `BroadcastChannel` 同步新 token，避免 401 踢出 |
| S2-A3 | 修复 AI 配额显示 | `authStore.ts` + 相关页面 | 显示 `ai_used_today / quota_ai_daily` 代替直接显示 `quota_ai_daily` |
| S2-A4 | API Base 配置界面 | `SettingsPage.tsx` | 允许用户在设置页修改 API Base，保存到 Tauri store |
| S2-A5 | 请求头携带 `X-Request-ID` | `api.ts` | 每次请求生成 uuid v4 作为 request-id，便于服务端追踪 |

### 注意事项

- **S2-S1 和 S2-A1 必须同时上线**：服务端改 Cookie 后，如果客户端还读 localStorage，认证会断。建议分两步：
  1. 服务端同时返回 Cookie + JSON body（兼容期）
  2. 客户端迁移到 secure store
  3. 服务端关闭 JSON body 中的 token（Sprint 3）

---

## Sprint 3：回测体验 + 数据优化（服务端为主，App 配合，7 天）

**目标**：回测进度实时可见、数据同步秒级完成、策略编辑器不卡顿。

### 服务端任务

| 编号 | 任务 | 文件 | 验收标准 |
|------|------|------|---------|
| S3-S1 | WebSocket 回测进度推送 | `api/routers/backtest.py` + `ws.py` | `ws://host/ws/backtest/{job_id}` 推送 `pending/running/success/failed` 及进度百分比 |
| S3-S2 | 全市场批量同步接口 | `data/syncers/tushare_syncer.py` | 利用 Tushare `daily` 不传 `ts_code` 的批量接口，500 只 < 5 秒 |
| S3-S3 | 停牌检测 | `datafeed.py` | 停牌日该 symbol 不出现在 bar_group 中，策略无需手动判断 |
| S3-S4 | 数据质量告警 | `data/syncers/tushare_syncer.py` | 同步时检测单日涨跌幅 > 30%（非新股）记录 warning 日志 |
| S3-S5 | equity_curve 降采样接口 | `api/routers/backtest.py` | `/backtests/{id}/chart` 返回按周/月聚合的净值点，减少传输量 |
| S3-S6 | 交易记录分页 | `api/routers/backtest.py` | `/backtests/{id}/trades?page=1&page_size=50` 分页返回 |

### App 端任务

| 编号 | 任务 | 文件 | 验收标准 |
|------|------|------|---------|
| S3-A1 | WebSocket 替换轮询 | `BacktestResultPage.tsx` | 连接 WebSocket 接收实时进度，任务完成后自动拉取结果 |
| S3-A2 | 策略编辑器防抖 | `StrategyEditorPage.tsx` | 停止输入 500ms 后再触发 `extractParams` |
| S3-A3 | 支持降采样图表 | `EquityCurveChart.tsx` | 数据点 > 1000 时自动按周聚合显示，不卡顿 |
| S3-A4 | 交易记录分页加载 | `BacktestResultPage.tsx` | 滚动加载更多交易记录，不一次性渲染全部 |

---

## Sprint 4：安全加固 + 沙箱隔离（服务端，7 天）

**目标**：策略代码在独立进程中运行，系统可观测性完整。

### 服务端任务

| 编号 | 任务 | 文件 | 验收标准 |
|------|------|------|---------|
| S4-S1 | 策略代码子进程执行 | `sandbox/executor.py` | `multiprocessing.Process` 执行策略，`Queue` 返回信号，超时 300 秒自动 kill |
| S4-S2 | 子进程资源限制 | `sandbox/executor.py` | 内存限制 512MB（通过 `resource.setrlimit` 或 cgroup），CPU 时间 60 秒 |
| S4-S3 | 子进程网络隔离 | `sandbox/executor.py` | 子进程关闭所有 socket（`socket.socket = lambda *a, **k: None`），禁止任何网络访问 |
| S4-S4 | 关闭 JSON body token（完成迁移） | `api/routers/auth.py` | Login/Refresh 不再在 JSON body 中返回 token，仅通过 Cookie 传输 |
| S4-S5 | 告警规则（可选） | 新增 | Celery 任务失败 > 5 次/分钟时发送告警（日志或 Webhook） |

---

## 跨 Sprint 的基础设施清单

### 新增文件（预计）

```
/
├── requirements.txt                    # S1
├── .env.example                        # S1
├── pytest.ini                          # S1
├── Dockerfile                          # S1
├── docker-compose.yml                  # S1
├── docker-compose.prod.yml             # S1
├── alembic.ini                         # S1
├── alembic/
│   ├── env.py
│   ├── versions/
│   │   ├── 001_init.py                 # 现有 SQL 转 Alembic
│   │   ├── 002_market_data.py
│   │   ├── 003_st_stock.py
│   │   └── 004_refresh_tokens.py       # S2
│   └── script.py.mako
└── server/
    ├── api/
    │   ├── core/
    │   │   └── logging.py              # S1
    │   └── routers/
    │       └── ws.py                   # S3
    └── sandbox/
        └── subprocess_runner.py        # S4
```

### 必须修改的文件清单

#### 服务端

| 文件 | Sprint | 修改内容 |
|------|--------|---------|
| `server/api/main.py` | S1, S2, S4 | CORS 配置、健康检查、metrics、request_id middleware |
| `server/api/routers/auth.py` | S2, S4 | Cookie 传输、ai_used_today、关闭 JSON token |
| `server/api/routers/backtest.py` | S3 | WebSocket 集成、降采样、分页 |
| `server/api/core/security.py` | S2 | token 生成增加 jti/family（已完成） |
| `server/api/models/models.py` | S2 | RefreshToken 模型（已完成） |
| `server/backtest/sandbox/executor.py` | S4 | 子进程隔离 |
| `server/data/syncers/tushare_syncer.py` | S3 | 批量接口、数据质量校验 |

#### App 端

| 文件 | Sprint | 修改内容 |
|------|--------|---------|
| `client/src/services/api.ts` | S1, S2 | API Base 环境变量、secure store 读写 token、request_id |
| `client/src/stores/authStore.ts` | S2 | secure store、多 Tab 同步、AI 配额显示 |
| `client/src/stores/settingsStore.ts` | S1 | 移除默认公网 IP |
| `client/src/pages/LoginPage.tsx` | S2 | 登录后写入 secure store |
| `client/src/pages/BacktestResultPage.tsx` | S3 | WebSocket、分页、降采样 |
| `client/src/pages/StrategyEditorPage.tsx` | S3 | 防抖 |
| `client/src/pages/SettingsPage.tsx` | S2 | API Base 配置界面 |

---

## 依赖关系与阻塞点

```
S1（基础设施）
  ├── 阻塞 S2-A1: S1-P1 Dockerfile 完成后才能部署测试 Cookie 传输
  └── 阻塞 S3-S1: S1-S6 结构化日志完成后 WebSocket 才能复用 request_id

S2（认证修复）
  ├── 阻塞 S3-A1: WebSocket 也需要认证（Cookie 或 token）
  ├── 阻塞 S4-S4: 必须先完成 S2-A1 的 secure store 迁移
  └── 注意: S2-S1 和 S2-A1 必须配对上线，不能单独发布

S3（体验优化）
  └── 阻塞: 无，可独立发布

S4（安全隔离）
  └── 阻塞: 无，可独立发布
```

---

## 风险与应对

| 风险 | 影响 | Sprint | 应对措施 |
|------|------|--------|---------|
| Tauri secure store API 变动 | S2-A1 延期 | S2 | 备选方案：用原生 `localStorage` 但加密 token（ interim 方案） |
| Tushare 批量接口限频 | S3-S2 效果不达预期 | S3 | 备选 AKShare 批量接口，或增加同步间隔 |
| WebSocket 在生产环境穿透 | S3-A1 连接失败 | S3 | Nginx 配置 `proxy_pass` + `proxy_http_version 1.1` + `Upgrade` header |
| 子进程隔离跨平台差异 | S4-S1 Mac 开发机无法测试 | S4 | 开发机用 mock 模式，生产只在 Linux 容器验证 |
| 前端资源不足 | S2/S3 前端任务堆积 | S2-S3 | 优先保证服务端任务不阻塞，前端可延后 1 周 |

---

## 上线检查清单（每个 Sprint 发布前）

- [ ] 所有服务端测试通过（`pytest`）
- [ ] Docker 镜像构建成功且 `docker-compose up` 可跑通
- [ ] 前端 `tauri build` 成功（Mac 端）
- [ ] 生产环境 `.env` 已配置且密钥已更换（非默认值）
- [ ] 数据库 migration 已执行（`alembic upgrade head`）
- [ ] 健康检查 `/health` 返回全部 green
- [ ] 核心功能手动验证：注册 -> 登录 -> 创建策略 -> 回测 -> 查看结果
- [ ] 回测结果缓存命中验证（同一策略重复提交应秒级返回）
- [ ] 沙箱安全验证：`import os` 和 `getattr` 绕过被拦截

---

## 建议的首次外部测试版本

**版本号**: v0.3.0-beta  
**范围**: Sprint 1 + Sprint 2  
**发布时间**: 约 2 周后  
**目标用户**: 内部团队 + 少量种子用户  

**必须包含**：
- Docker 一键部署
- 结构化日志 + 健康检查
- Token 安全存储（secure store）
- AI 配额正确显示
- 硬编码 API 地址修复

**可延后到 v0.4.0**：
- WebSocket 进度推送
- 子进程沙箱隔离
- 全市场批量同步优化

---

**计划制定**: Claude  
**建议 Review 节点**: Sprint 1 结束、Sprint 2 结束时各进行一次代码 Review 和手动验收
