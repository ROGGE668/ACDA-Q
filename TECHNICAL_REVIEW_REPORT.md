# ACDA-Q 量化投资平台 —— 技术评审报告

**评审日期**: 2026-05-02  
**评审范围**: 全栈代码（Client / API / Worker / Backtest Engine / Data Pipeline / Infra）  
**评审视角**: 架构设计、金融正确性、安全、性能、工程实践  

---

## 1. 执行摘要

ACDA-Q 是一个以 Python FastAPI + Tauri 为技术栈、面向 A 股的量化回测平台。整体架构选型合理（事件驱动回测、TimescaleDB 时序存储、Celery 异步任务），MVP 阶段的功能覆盖度较好。但作为金融类系统，当前代码在**金融计算正确性**、**安全沙箱**、**数据精度**和**工程规范**四个维度存在显著风险。若不加以修复，上线后可能在回测结果准确性、用户代码执行安全、资金计算精度等关键领域产生系统性问题。

**总体评级**: Beta / 预生产阶段，不建议在未经关键修复前承载实盘参考场景。

---

## 2. 架构层评审

### 2.1 设计合理性

| 模块 | 评价 | 说明 |
|------|------|------|
| 客户端选型 | 合理 | Tauri 2.0 + React + TypeScript 是跨平台桌面应用的成熟组合，体积小、性能好。 |
| API 框架 | 合理 | FastAPI + SQLAlchemy 2.0 async 模式，生态成熟，适合量化服务端。 |
| 时序数据库 | 合理 | TimescaleDB 对金融时序数据的压缩、分区能力优于原生 PostgreSQL。 |
| 任务队列 | 合理 | Celery + Redis 用于回测任务调度是行业标准方案。 |
| 部署架构 | 基本可用 | Docker Compose 支撑 MVP，但缺少 K8s/编排层的生产就绪配置。 |

### 2.2 架构缺陷

**A1. 缺乏 API 网关防护**

当前 Nginx 仅做反向代理，没有限流（Rate Limiting）、熔断、IP 黑名单或 WAF 能力。回测任务属于 CPU 密集型，恶意用户可通过高频提交大任务造成拒绝服务。

**A2. WebSocket 进度推送缺失**

开发文档提到 `WS /ws/v1/backtest/{job_id}` 用于回测进度实时推送，但代码中完全没有 WebSocket 实现。用户只能轮询任务状态，体验差且增加数据库压力。

**A3. 服务间通信过于简单**

API、Worker、Scheduler 共享同一数据库连接池和代码目录，耦合度高。随着回测引擎复杂度增加，应考虑将 Backtest Engine 拆分为独立 gRPC/HTTP 服务，实现语言级隔离。

**A4. 无事件总线**

回测完成、AI 生成完成等状态变更没有事件通知机制（如 Webhook、消息队列事件），不利于后续扩展（如发送邮件通知、触发后续分析任务）。

---

## 3. 回测引擎核心评审（金融正确性）

回测引擎是量化平台的核心资产，其正确性直接决定用户决策质量。以下问题按严重性排序。

### 3.1 P0 —— 复权价格缺失（致命）

**问题**: `DataFeed.load_bars` 默认查询 `daily_bars` 原始价格，**没有调用 `get_adj_price` 进行复权处理**。`server/backtest/engine/core.py` 和 `server/backtest/engine/datafeed.py` 中，回测引擎使用的是未复权的收盘价。

**影响**: 跨越除权除息日时，价格会出现跳变，均线交叉、收益率等所有指标都会失真。例如一只股票分红后从 100 元跌到 90 元，引擎会认为是 10% 的跌幅，可能触发错误的卖出信号。

**修复**: 在 `DataFeed.load_bars` 中默认使用前复权价格；`context.history` 返回的历史数据也应基于复权序列。

### 3.2 P0 —— 佣金模型不完整（严重）

**问题**: `broker.py` 仅模拟了佣金（默认 0.03%），但**未包含印花税（卖出 0.05%）和过户费**。

**影响**: A 股交易成本中印花税占比最高。以 100 万本金、月换手率 100% 的策略为例，忽略印花税会导致年化收益虚高约 0.6%~1.2%，严重误导用户对策略真实收益的预期。

**修复**: `Broker` 增加 `stamp_duty_rate`（仅卖出收取，默认 0.0005）和 `transfer_fee_rate`。

### 3.3 P0 —— 成交数量未强制 100 股整数倍（严重）

**问题**: `Context.buy/sell/target_percent` 中 `amount = int(...)` 直接截断小数，没有确保是 100 的整数倍。

**影响**: `context.buy("600519", percent=0.33)` 计算出的股数可能不是 100 的倍数，与 A 股交易规则不符。

**修复**: 所有下单数量应向下取整到 100 的整数倍：`amount = (amount // 100) * 100`。

### 3.4 P0 —— 浮点精度用于金融计算（严重）

**问题**: `Broker` 中 `cash`、`total_value`、`commission` 全部使用 `float`。Python float 遵循 IEEE 754 双精度，二进制浮点无法精确表示十进制小数（如 0.1）。在数千笔交易后，资金余额可能出现分位级累积误差。

**影响**: 可能导致净值曲线不平滑、最终资产计算出现可感知的偏差。

**修复**: 资金计算核心应使用 `decimal.Decimal`，仅在最终输出时转换为 float。

### 3.5 P1 —— 涨跌停规则过于简化

**问题**: 
1. `broker.py` 对科创板（688）、创业板（30）判断 20% 涨跌幅，但**遗漏了北交所（8/4 开头，30%）和 ST 股（5%）**。
2. 涨停时完全禁止买入、跌停时完全禁止卖出。实际 A 股涨停/跌停时仍可挂单排队，只是不一定成交。完全禁止会导致策略行为与实际交易环境差异过大。

**修复**: 
1. 扩展涨跌停判断逻辑，覆盖全市场板块。
2. 涨跌停时应允许挂单（加入 pending_orders），成交概率可通过队列模型模拟，而非直接拒绝。

### 3.6 P1 —— 滑点模型过于简单

**问题**: 滑点固定为 `close * slippage`，没有考虑成交量、流动性、价格档位。

**影响**: 小盘股和大盘股使用相同滑点，回测结果对实盘指导意义下降。

**修复**: 引入基于波动率或市值的弹性滑点模型，或至少区分大盘/小盘。

### 3.7 P1 —— 市值加权逻辑错误

**问题**: `PortfolioEngine._init_weights` 中市值加权使用 `close * volume` 作为市值 proxy。`volume` 是当日成交量，不是总股本或流通股本。

**影响**: 市值权重完全错误。成交量大的股票会被过度加权。

**修复**: `stock_basic` 表应补充 `total_shares` / `float_shares` 字段，市值 = 收盘价 × 总股本。

### 3.8 P1 —— 绩效指标计算缺陷

**问题**:
1. `annual_return = (1 + total_return) ** (1 / duration_years) - 1` 使用自然日（365.25）而非交易日（约 252 天）计算年化，会系统性低估年化收益。
2. 盈亏配对采用 FIFO，但如果卖出数量大于第一笔买入数量，PNL 计算错误（用卖出数量乘以单股盈亏，但只扣除了一笔买入的佣金）。
3. 夏普比率使用简单年化，未考虑无风险利率的复利效应。
4. Beta、Alpha、信息比率等开发文档承诺的指标均未实现。

**修复**: 按交易日年化；实现精确的部分盈亏配对；补全 Beta/Alpha 计算。

### 3.9 P2 —— Mock 数据种子固定

**问题**: `DataFeed._generate_mock_bars` 使用 `np.random.seed(hash(symbol) % 2**31)`，相同 symbol 每次回测产生完全相同的随机序列。

**影响**: 用户运行同一策略多次会得到完全相同的“随机”结果，失去统计意义。

**修复**: 添加 `seed` 参数，默认使用随机种子。

---

## 4. 安全评审

### 4.1 P0 —— 策略沙箱可被反射绕过（致命）

**问题**: `server/backtest/sandbox/executor.py` 的 AST 沙箱无法防御反射攻击。以下绕过在当前环境中**可直接执行**：

```python
# 通过子类枚举获取 os._wrap_close，进而执行任意系统命令
[x for x in [].__class__.__base__.__subclasses__() 
 if x.__name__ == '_wrap_close'][0].__init__.__globals__['system']('id')
```

另外，`builtins` 不在 `BLACKLISTED_NAMES` 中，恶意代码可通过 `import builtins; builtins.__import__('os')` 绕过导入限制。

**影响**: 攻击者可在 Worker 容器中执行任意系统命令，读取数据库凭证、篡改回测结果、横向渗透。

**修复**:
1. **短期**: 在 AST 检查中禁止 `__class__`、`__base__`、`__subclasses__`、`__globals__` 等 dunder 属性访问。
2. **中期**: 使用 RestrictedPython 替代手写 AST 沙箱。
3. **长期**: 将用户代码执行迁移到 Firecracker MicroVM / gVisor / 独立 Docker 容器（无网络、只读文件系统、CPU/内存限制）。

### 4.2 P0 —— 路径遍历风险

**问题**: `server/api/routers/backtest.py` 第 105 行直接使用数据库中的 `job.result_report_path` 打开文件：

```python
if job.result_report_path and os.path.exists(job.result_report_path):
    with open(job.result_report_path, "r") as f:
        report = json.load(f)
```

如果数据库被注入恶意 `report_path`（如 `/etc/passwd`），攻击者可读取服务器任意文件。

**修复**: 所有报告文件必须存放在固定根目录下，读取前校验路径：`os.path.commonpath([report_dir, target_path]) == report_dir`。

### 4.3 P1 —— JWT 配置缺少校验

**问题**: `Settings.SECRET_KEY` 为空字符串时，`jwt.encode` 仍可使用空 key 签发 token，安全性归零。且 `get_settings()` 在 import 时即被调用，缺少启动时必填项校验。

**修复**: 应用启动时断言 `SECRET_KEY` 长度 ≥ 32 字节；`ACCESS_TOKEN_EXPIRE_MINUTES` 必须有合理值。

### 4.4 P1 —— Refresh Token 未实现轮换

**问题**: `auth.py` 中 refresh token 可无限次使用生成新的 token pair，旧 token 不会被撤销。如果 refresh token 泄露，攻击者可永久保持访问权限。

**修复**: 实现 Refresh Token Rotation（每次刷新后使旧 refresh token 失效），并存储 token 家族（token family）以检测重用攻击。

### 4.5 P1 —— 前端 Token 存储不安全

**问题**: `client/src/services/api.ts` 将 JWT 存储在 `localStorage` 中，XSS 攻击可轻易窃取。

**修复**: 使用 httpOnly secure cookie，或 Tauri 的 secure store API（`@tauri-apps/plugin-store` 已引入但仅用于 settings）。

### 4.6 P1 —— 公网 IP 硬编码

**问题**: `api.ts` 和 `settingsStore.ts` 硬编码了公网 IP `http://124.220.70.210:8000`，且未强制 HTTPS。

**修复**: 生产环境必须 HTTPS；API Base 应通过构建时环境变量注入，而非硬编码。

### 4.7 P2 —— 缺少 API 速率限制

**问题**: 全站没有限流。AI 生成接口（调用 OpenAI API）、回测提交接口（触发 Celery 任务）均可能被滥用。

**修复**: 对 `/ai/generate` 和 `/backtests` 增加基于用户 ID 的速率限制（Redis + sliding window）。

---

## 5. 性能与可扩展性评审

### 5.1 P1 —— 全市场扫描性能极差

**问题**: `MarketScanner.run` 对每只股票单独调用 `run_strategy_code`，而 `run_strategy_code` 内部每次都会重新 `compile_strategy_code` + `load_strategy_class`。扫描 500 只股票 = 编译同一段策略 500 次。

**影响**: 全市场扫描耗时随股票数线性增长，且斜率很高。当前实现无法在合理时间内完成全市场扫描。

**修复**: 策略编译结果缓存（code hash -> compiled module），扫描时只编译一次。

### 5.2 P1 —— TimescaleDB 连接池配置不当

**问题**: `server/api/core/timescale.py` 使用 `NullPool`，每次查询都新建并关闭连接。时序数据查询频率高（回测引擎逐 bar 查询历史数据），连接开销巨大。

**修复**: 使用 `QueuePool` 并设置合理的 `pool_size` 和 `max_overflow`。

### 5.3 P2 —— 回测引擎内存占用高

**问题**: `BacktestEngine.run` 中 `_history_cache` 为每个 symbol 缓存完整历史数据。全市场组合回测时，内存中同时持有所有股票的全部历史 K 线。以 5000 只股票、10 年日K 估算，约需数 GB 内存。

**修复**: `context.history` 改为惰性查询（Lazy Query），仅在实际请求时才从数据库/缓存加载对应 symbol 和 lookback 的数据。或采用向量化回测替代事件驱动，大幅降低内存占用。

### 5.4 P2 —— SQL 占位符扩展问题

**问题**: `query_daily_bars` 通过字符串拼接为每个 symbol 生成占位符 `:s0, :s1, ...`。symbol 数量大时 SQL 语句超长，且不利于查询计划缓存。

**修复**: 使用 PostgreSQL `ANY` 数组语法：`WHERE symbol = ANY(:symbols)`。

### 5.5 P2 —— 回测报告直接返回全量数据

**问题**: `get_backtest_result` 将完整的 `trades` 和 `equity_curve` 从 JSON 文件读取后直接返回。长期回测的 equity_curve 可能有几万条记录，全市场扫描的 trades 可能数十万条。

**修复**: 
1. 分页返回交易记录。
2. equity_curve 按时间降采样（如合并为周度/月度点）后返回，或提供专门的图表数据端点。
3. 大报告接入 MinIO/S3，返回预签名 URL 而非内联数据。

---

## 6. 代码质量与工程实践

### 6.1 测试覆盖率严重不足

**问题**: `tests/test_backtest_engine.py` 仅 3 个测试用例，且以脚本形式编写，未使用 pytest 的标准 fixture、parametrize、assertion 机制。没有单元测试覆盖：
- Broker 的 T+1 / 涨跌停逻辑
- 绩效指标计算
- AI 生成参数提取
- API 路由的认证/授权
- 安全沙箱绕过场景（negative testing）

**修复**: 
1. 引入 pytest + pytest-asyncio + pytest-cov。
2. 为 Broker、PerformanceAnalyzer、executor 编写单元测试。
3. 为 API 路由编写集成测试（TestClient + 内存数据库）。
4. 添加 CI 流水线（GitHub Actions），要求 PR 合并前测试通过且覆盖率 ≥ 70%。

### 6.2 缺少静态分析

**问题**: 没有 Black/Ruff/mypy 配置，代码风格不一致（如 docstring 格式混用）。

**修复**: 引入 Ruff 作为 linter 和 formatter，mypy 进行类型检查，并在 CI 中强制执行。

### 6.3 错误处理不一致

**问题**: 
- `market.py` 用 `except Exception` 捕获所有异常并返回 500，暴露了内部错误。
- `strategies.py` 的 `validate_strategy_code` 返回 JSON `{"valid": False}` 而不是抛出 HTTPException，HTTP 状态码仍是 200，不符合 REST 错误语义。
- 部分路由没有输入校验（如 `market.py` 的 `limit` 参数无上限）。

**修复**: 统一错误响应格式（RFC 7807 Problem Details），区分客户端错误（4xx）和服务器错误（5xx）。

### 6.4 数据库迁移管理缺失

**问题**: 虽然安装了 Alembic，但实际使用 `Base.metadata.create_all` 建表。生产环境无法做 schema migration、rollback、数据迁移。

**修复**: 立即初始化 Alembic，将所有现有表结构转为 migration 脚本，后续 schema 变更必须通过 migration 完成。

### 6.5 日志未结构化

**问题**: 虽然安装了 `structlog`，但代码中仍大量使用标准 `logging`。Celery Worker 的日志没有 correlation ID，难以追踪一次回测请求在 API -> Worker -> DB 全链路的日志。

**修复**: 
1. 统一使用 structlog，配置 JSON 格式输出。
2. 通过 FastAPI middleware 注入 `request_id`，传递到 Celery task 和数据库查询中。

### 6.6 AI 配额机制缺陷

**问题**: `quota_ai_daily` 是一次性配额，不会每日重置。用户注册后只有 5 次 AI 调用机会，之后永久耗尽。

**修复**: 
1. 引入 `user_quota_usage` 表，记录每日用量，定时任务重置。
2. 或者使用时间窗口滑动计数（Redis + TTL）。

---

## 7. 数据层评审

### 7.1 UUID 生成方式不一致

**问题**: `001_init.sql` 使用 `uuid_generate_v4()`（需 `uuid-ossp`），而 `models.py` 使用 `uuid.uuid4()`。SQLAlchemy 的 `UUID` 类型在某些驱动下会生成 `gen_random_uuid()`（需 `pgcrypto`）。扩展依赖混乱。

**修复**: 统一使用 `pgcrypto` 的 `gen_random_uuid()`，弃用 `uuid-ossp`。

### 7.2 TimescaleDB 分区剪枝失效风险

**问题**: `tushare_syncer.py` 删除数据时使用 `DATE(datetime) = ANY(:dates)`。对 hypertable 的 time 列应用函数（如 `DATE()`）会阻止 TimescaleDB 的分区剪枝，导致扫描全部分区。

**修复**: 使用范围查询：`datetime >= :start AND datetime < :end`。

### 7.3 缺少数据保留策略

**问题**: TimescaleDB 数据无限增长，没有配置 retention policy 和 compression policy。

**修复**: 
1. 对 `daily_bars` 配置 5 年以上数据自动压缩。
2. 对 `minute_bars` 配置 2 年以上数据自动删除（或归档到 MinIO）。

### 7.4 数据同步单点故障

**问题**: `scheduler.py` 的 full_sync 模式在主数据源失败后回退到 AKShare，但没有重试机制、没有死信队列、没有同步状态持久化。

**修复**: 引入同步任务状态表，记录每次同步的起止时间、数据量、错误信息，支持断点续传。

---

## 8. 前端评审

### 8.1 安全问题
- 硬编码 HTTP 公网 IP（已在上文提及）。
- `localStorage` 存储 JWT（已在上文提及）。
- 没有 CSP（Content Security Policy）配置。

### 8.2 体验问题
- `StrategyEditorPage` 中 AI 生成参数提取调用 `aiAPI.extractParams?.(code)` 使用了可选链，但结果未被使用（状态管理依赖本地正则 `extractParamsLocal`）。前后端参数提取逻辑不一致时，用户看到的参数面板与后端解析结果可能不同。
- 回测提交后跳转到结果页，若任务未完成则显示错误。应显示进度/排队状态。
- 没有防抖：代码编辑器输入时频繁触发本地正则提取，可能影响性能。

### 8.3 工程问题
- TypeScript 类型安全：`api.ts` 大量使用 `any`。
- 没有前端测试（React Testing Library / Vitest）。

---

## 9. 修改建议汇总（按优先级排序）

### P0 —— 阻塞性问题（上线前必须修复）

| 编号 | 问题 | 修改位置 | 预期工作量 |
|------|------|---------|-----------|
| P0-1 | **复权价格缺失**：回测引擎默认使用前复权价格 | `datafeed.py`, `get_adj_price` 整合 | 1-2 天 |
| P0-2 | **佣金模型不完整**：增加印花税、过户费 | `broker.py` | 0.5 天 |
| P0-3 | **成交数量未强制 100 股整数倍** | `core.py` Context.buy/sell | 0.5 天 |
| P0-4 | **金融计算使用 float**：Broker 核心改用 Decimal | `broker.py`, `context.py` | 2-3 天 |
| P0-5 | **沙箱反射绕过**：禁止 dunder 属性访问，或引入 RestrictedPython | `executor.py` | 1-2 天 |
| P0-6 | **路径遍历风险**：校验报告文件路径 | `backtest.py` | 0.5 天 |
| P0-7 | **JWT 空密钥风险**：启动时校验 SECRET_KEY | `config.py`, `main.py` | 0.5 天 |
| P0-8 | **市值加权逻辑错误**：使用实际股本而非成交量 | `portfolio.py`, `stock_basic` schema | 1 天 |

### P1 —— 严重问题（建议 2 周内修复）

| 编号 | 问题 | 修改位置 |
|------|------|---------|
| P1-1 | **涨跌停规则不完整**：覆盖北交所、ST 股 | `broker.py` |
| P1-2 | **全市场扫描编译缓存**：策略只编译一次 | `scanner.py`, `executor.py` |
| P1-3 | **TimescaleDB 连接池**：NullPool -> QueuePool | `timescale.py` |
| P1-4 | **Refresh Token 轮换**：防止 token 重用 | `auth.py` |
| P1-5 | **前端 Token 存储迁移**：localStorage -> httpOnly cookie / Tauri secure store | `api.ts` |
| P1-6 | **API 速率限制**：Redis-based 限流 | `main.py` middleware |
| P1-7 | **绩效指标修正**：交易日年化、精确盈亏配对、补全 Beta/Alpha | `performance.py` |
| P1-8 | **数据库迁移管理**：启用 Alembic | 新增 `alembic/` |

### P2 —— 中等问题（建议 1 个月内修复）

| 编号 | 问题 | 修改位置 |
|------|------|---------|
| P2-1 | **回测引擎内存优化**：Lazy history 加载 | `core.py`, `datafeed.py` |
| P2-2 | **报告数据分页/降采样**：避免传输全量 trades | `backtest.py` |
| P2-3 | **SQL ANY 数组语法**：优化大 symbol 列表查询 | `timescale.py` |
| P2-4 | **WebSocket 进度推送**：实现实时状态通知 | 新增 WebSocket endpoint |
| P2-5 | **AI 配额每日重置**：引入用量表或 Redis 计数 | `ai.py`, `models.py` |
| P2-6 | **Mock 数据随机种子**：支持可变种子 | `datafeed.py` |
| P2-7 | **滑点弹性模型**：基于市值/波动率 | `broker.py` |

### P3 —— 改进项（长期优化）

| 编号 | 问题 | 说明 |
|------|------|------|
| P3-1 | **向量化回测引擎**：对简单策略提供 NumPy 向量化回测 | 性能提升 10-100x |
| P3-2 | **用户代码容器隔离**：gVisor / Firecracker 执行环境 | 安全终极方案 |
| P3-3 | **回测结果缓存**：相同策略+参数+时间范围的结果缓存 | 减少重复计算 |
| P3-4 | **数据质量校验**：停牌、价格异常、复权因子一致性检查 | 数据 Pipeline |
| P3-5 | **多因子模型支持**：Barra 风格因子接入 | 扩展性 |

---

## 10. 长期演进建议

1. **回测引擎双轨制**: 保留事件驱动引擎用于复杂策略，增加向量化引擎（类似 VectorBT）用于简单技术指标策略，实现亚秒级全市场扫描。

2. **策略代码版本控制**: 将策略代码接入 Git（每版本一个 commit），支持 diff、回滚、分支合并。

3. **实时行情接入**: 当前只有日K，应逐步接入分钟K和 Level-2 逐笔数据，支持日内策略回测。

4. **机器学习因子平台**: 从“规则型策略”扩展到“因子挖掘 + 自动优化”，接入 genetic programming / AutoML。

5. **合规与风控**: 增加策略合规检查（如单票仓位上限、行业集中度限制），为后续实盘信号做准备。

---

**评审人**: Claude (Technical Review)  
**下次评审建议**: 在完成 P0 和 P1 修复后进行复评。
