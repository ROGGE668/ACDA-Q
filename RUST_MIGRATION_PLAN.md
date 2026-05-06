# ACDA-Q 服务端 Rust 迁移规划

**日期**: 2026-05-06
**目标**: 渐进式将 Python 后端迁移至 Rust，提升回测性能 10x+

---

## 一、为什么迁移

| 维度 | Python 现状 | Rust 目标 |
|------|------------|----------|
| 回测速度 | 逐条循环，500 只全市场扫描分钟级 | 向量化 + 并行，秒级 |
| 内存占用 | Python 对象开销大，全市场回测数 GB | 零成本抽象，内存降低 80% |
| 启动时间 | 冷启动 3-5 秒 | 毫秒级 |
| 类型安全 | 运行时错误（历史 bug：ME→M、pandas 版本兼容） | 编译期捕获 |
| 部署体积 | Python 镜像 ~500MB | Rust 静态链接 ~20MB |

---

## 二、技术栈

| 组件 | Rust 选型 | 说明 |
|------|----------|------|
| Web 框架 | axum | tokio 原生，中间件生态成熟 |
| 数据库 | sqlx | 编译时 SQL 校验，无 ORM 运行时开销 |
| Decimal | rust_decimal | 金融行业标准，支持 serde |
| 时序 | chrono | 日期处理 |
| Redis | redis-rs | 异步连接池 |
| 任务队列 | tokio + redis streams | 自研轻量队列，替代 Celery |
| Python 互操作 | PyO3 | 渐进迁移：Rust 模块先给 Python API 调用 |
| gRPC (可选) | tonic | 服务间通信 |

---

## 三、迁移路线图（3 个月）

### Phase 1: 回测引擎 Rust 化（2-3 周）

**目标**: Rust 回测引擎跑通双均线策略，性能对比 Python 提升 10x+

**模块**: `server-rust/backtest/`
- `broker.rs` — 资金、持仓、交易记录（Decimal 精度）
- `engine.rs` — 事件驱动回测主循环
- `context.rs` — history()、sma()、ema()、buy()、sell()
- `indicators.rs` — 技术指标（向量化计算）
- `analyzer.rs` — 绩效指标（夏普、最大回撤等）
- `datafeed.rs` — CSV / DB 数据加载

**验证**: 与 Python 回测结果逐 bar 对比，数值偏差 < 0.01%

### Phase 2: PyO3 绑定 + Python API 接入（1-2 周）

**目标**: Python FastAPI 通过 PyO3 调用 Rust 回测引擎，用户无感知

**做法**:
```python
# Python 端
from server_rust import BacktestEngine
engine = BacktestEngine(initial_cash=1_000_000)
engine.run_strategy(code, symbols, start, end)
```

### Phase 3: 数据同步 Rust 化（2 周）

**模块**: `server-rust/data/`
- Tushare / AKShare 批量接口调用
- 数据清洗、复权计算
- 批量写入 TimescaleDB（COPY 协议）

### Phase 4: API 服务 Rust 化（4-6 周）

**模块**: `server-rust/api/`
- axum 路由（Auth、Strategy、Backtest、Market）
- JWT 认证（jsonwebtoken）
- sqlx 数据库操作
- Redis 缓存
- MinIO 对象存储

### Phase 5: 全面切换（1 周）

- docker-compose.yml 替换 Python 服务为 Rust
- 性能回归测试
- 监控接入

---

## 四、目录结构

```
ACDA-Q/
├─ server/              # Python 后端（逐步退役）
├─ server-rust/         # Rust 后端（新增）
│  ├─ Cargo.toml
│  ├─ src/
│  │  ├─ main.rs        # API 服务入口
│  │  ├─ lib.rs         # PyO3 库入口
│  │  ├─ backtest/      # 回测引擎
│  │  ├─ api/           # HTTP 路由
│  │  ├─ data/          # 数据同步
│  │  ├─ models/        # 数据库模型
│  │  └─ config.rs      # 配置管理
│  └─ Dockerfile
├─ client/              # Tauri 前端（不变）
└─ docker-compose.yml
```

---

## 五、风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| Rust 学习曲线 | 开发速度初期下降 | 先从独立模块开始，不影响现有功能 |
| PyO3 绑定复杂度 | FFI 边界 bug | 详细测试覆盖，数值逐 bar 对比 |
| sqlx 编译时检查 | 开发体验变化 | 比运行时错误更安全 |
| 生态成熟度 | 某些 Python 库无 Rust 等价物 | 保留 Python 服务作为 fallback |

---

**下一步**: 执行 Phase 1，初始化 `server-rust/` 并实现 Broker 核心。
