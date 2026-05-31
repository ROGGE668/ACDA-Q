# ACDA-Quant 全链路修复记录

**日期**: 2026-05-19  
**概要**: 后端 Rust API + Worker + Python 沙箱 + 前端 App 系统性修复

---

## 1. 后端 API 修复

### 1.1 ConnectInfo 注入缺失
**文件**: `server-rust/src/main.rs`  
**问题**: 注册接口依赖 `ConnectInfo<SocketAddr>` 扩展，但 `axum::serve` 未注入。  
**修复**: `axum::serve(listener, app)` → `app.into_make_service_with_connect_info::<SocketAddr>()`  
**影响**: `POST /api/v1/auth/register` 从 500 → 正常。

### 1.2 策略保存字段名变更
**文件**: `strategies.rs`, `models.rs`  
**问题**: 列名 `type` 是 PG 保留字，sqlx 映射失败。  
**修复**: 模型 + DB 列 `type` → `strategy_type`（`ALTER TABLE ... RENAME COLUMN`）  
**影响**: 策略保存从 500 → 正常。

### 1.3 股票搜索实现
**文件**: `market.rs`  
**问题**: 返回空数组。  
**修复**: 真实 SQL 查询 + `search`/`market` 映射 + 分页 + `{ items: [...] }` 包装。  
**影响**: 搜索从空 → 真实数据。

### 1.4 回测记录删除
**文件**: `backtest.rs`  
**新增**: `DELETE /api/v1/backtests/:job_id`（仅删当前用户记录）

---

## 2. 后端 Worker 修复

### 2.1 失败时更新 DB 状态
**文件**: `worker.rs`  
**问题**: `process()` 用 `?` 传播错误，不写 `backtest_jobs` → 永久 `pending`。  
**修复**: 主逻辑包在 `async { }` 闭包，`match` 错误分支写 `status='failed', error_message=$1`。

### 2.2 内置策略误判
**文件**: `worker.rs`  
**修复**: `is_builtin_strategy()` 增加 `&& payload.code.trim().is_empty()` 检查。

### 2.3 Stderr 死锁
**文件**: `executor.rs`  
**问题**: Python stderr 满 64KB 阻塞 → `child.wait()` 永久挂起。  
**修复**: `.stderr(Stdio::null())`  
**影响**: Worker 不再卡死。

### 2.4 阻塞 IO 修复
**文件**: `executor.rs`  
**问题**: `child.wait()` 在 async 运行时中阻塞线程。  
**修复**: `tokio::task::block_in_place(|| child.wait())`

---

## 3. Redis 队列修复

### 3.1 消费组起始消息 ID
**文件**: `queue.rs`  
**问题**: `XGROUP CREATE` 用 `$`（仅新消息），已有消息永不被消费。  
**修复**: `$` → `0`（从头开始）

### 3.2 CPU 空转
**文件**: `queue.rs`  
**问题**: `XREADGROUP` 无 `BLOCK` 参数 → CPU 100%。  
**修复**: `BLOCK 1000`（1 秒阻塞）

### 3.3 响应解析
**文件**: `queue.rs`  
**问题**: 嵌套 `Bulk` 结构因 `items.len() >= 2` 被跳过。  
**修复**: 增加 `items.len() == 1` 分支。

### 3.4 回收任务执行
**文件**: `queue.rs`  
**问题**: `reclaim_pending` 仅日志不执行。  
**修复**: `reclaim_interval` 中反序列化并调用 `worker.process()`。

---

## 4. Python 沙箱修复

### 4.1 Runner 实现
**文件**: `sandbox_runner.py`  
**修复**: 174 行完整实现：TimescaleDB 加载、策略 exec、K 线历史缓存、实算买卖、绩效计算。

### 4.2 BaseStrategy 冲突
**文件**: `sandbox_runner.py`  
**问题**: 用户 `BaseStrategy.__init__` 与沙箱冲突。  
**修复**: 
- 基类改 `_SandboxBase` 
- 检测排除名称 `BaseStrategy`
- 实例化失败时 `try/except TypeError` + `inspect.signature` 推测

### 4.3 CAST 语法修复
**文件**: `sandbox_runner.py`  
**问题**: SQLAlchemy `CAST(:end AS date)` 解析出错。  
**修复**: 使用显式 CAST 语法。

---

## 5. 前端 App 修复

### 5.1 错误信息格式
**波及**: `LoginPage`, `StrategyEditorPage`, `StrategyBacktestPage`, `BacktestListPage`, `DashboardPage`, `SubscriptionPage`, `strategyStore`  
**修复**: `.detail` → `.error || .detail`

### 5.2 StrategyStore 重构
**文件**: `strategyStore.ts`  
**问题**: 原 Store 用 Tauri 本地文件，新建策略报错。  
**修复**: 改用后端 API `strategyAPI.create/update/list/delete`。

### 5.3 API URL 条件 Bug
**文件**: `api.ts`  
**问题**: `!API_BASE && API_BASE !== "" && ...` 永不为真。  
**修复**: `!API_BASE && ...` + `!fullUrl` 兜底。

### 5.4 K 线数据解析
**文件**: `BacktestResultPage.tsx`  
**修复**: `Array.isArray(res.data) ? res.data : res.data?.data || []`

### 5.5 删除按钮
**文件**: `BacktestListPage.tsx`（删除回测）、`StrategyListPage.tsx`（删除策略）  
**新增**: 调用 `DELETE /api/v1/backtests/:id` 和 `DELETE /api/v1/strategies/:id`

---

## 6. 当前服务状态

| 组件 | 端口 | 状态 |
|------|------|------|
| PostgreSQL | 5432 | ✅ |
| TimescaleDB | 5433 | ✅ |
| Redis | 6379 | ✅ |
| API Server | 8000 | ✅ |
| Worker | — | ✅ |

**全链路延时**: App → API(2ms) → Redis → Worker(1s BLOCK) → Sandbox(595ms) → DB → 结果返回 ≈ 4-5 秒
