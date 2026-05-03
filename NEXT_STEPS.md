# ACDA-Q 量化投资平台 —— 下一步行动路线图

**文档日期**: 2026-05-02  
**当前状态**: 核心回测链路可用（13 个测试全部通过），P0/P1/P2 修复完毕，评级接近 Beta。

---

## 当前状态速览

### 已完成的里程碑

| 里程碑 | 状态 |
|--------|------|
| 金融计算精度（Decimal） | ✅ 完成 |
| 安全沙箱（反射绕过 + getattr 绕过） | ✅ 完成 |
| A股交易规则（T+1精确/涨跌停/ST/100股整数倍/印花税/过户费） | ✅ 完成 |
| 数据复权（前复权默认） | ✅ 完成 |
| 回测结果缓存 | ✅ 完成 |
| Refresh Token 轮换 | ✅ 完成 |
| API 速率限制 | ✅ 完成 |
| 路径遍历防护 | ✅ 完成 |

### 仍存在的已知问题（不影响内部测试）

- 前端 JWT 仍存储在 localStorage（XSS 风险）—— 需前端配合修改
- 无 requirements.txt，新开发者环境搭建困难
- 无 pytest 标准运行方式，测试以脚本形式存在
- 数据质量校验缺失（停牌检测、价格异常）
- 全市场同步逐只拉取效率低（500 只约 75 秒）
- 无监控/健康检查端点

---

## 迭代路线图（三波推进）

### 第一波：工程基础设施（1 周，可并行）

**目标**: 让项目可以被其他开发者在干净的机器上一键跑起来，让测试可以被 CI 自动执行。

| 编号 | 任务 | 模块 | 验收标准 |
|------|------|------|---------|
| W1-1 | 补充 `requirements.txt` / `pyproject.toml` | 根目录 | 新 clone 仓库后 `pip install -r requirements.txt` 可运行所有测试 |
| W1-2 | 迁移测试到 pytest 标准 | `tests/` | `pytest tests/` 一键运行全部 13 个用例并输出覆盖率 |
| W1-3 | 补充数据库 migration 脚本 | `database/migrations/` | 001-003 SQL 已覆盖，需补充 004（refresh_tokens 表） |
| W1-4 | 添加 `/health` 健康检查端点 | `api/routers/` | 返回 db/redis/celery 连接状态，用于 k8s 探针 |
| W1-5 | 引入 Ruff lint + mypy type check | `.ruff.toml` | 至少让 `server/` 目录通过无错误检查 |

### 第二波：数据质量 + 性能优化（1-2 周）

**目标**: 数据层从"能跑"到"可靠"，同步效率从分钟级降到秒级。

| 编号 | 任务 | 模块 | 验收标准 |
|------|------|------|---------|
| W2-1 | 全市场同步改用批量接口 | `tushare_syncer.py` | `daily` 接口传空 ts_code 一次性拉取全市场日K，单批次 < 5 秒 |
| W2-2 | 停牌检测与标记 | `datafeed.py` / 数据库 | 停牌日 bar_group 不含该标的，策略无需手动处理 |
| W2-3 | 价格异常跳变告警 | `tushare_syncer.py` | 同步时检测单日涨跌幅 > 30%（非新股）并记录告警日志 |
| W2-4 | 复权因子一致性校验 | `datafeed.py` | 复权后价格序列与复权因子乘积偏差 < 0.01% |
| W2-5 | TimescaleDB 压缩策略配置 | `database/migrations/` | daily_bars 超过 2 年自动压缩，minute_bars 超过 1 年自动归档 |

### 第三波：安全隔离 + 监控（2 周）

**目标**: 沙箱从"AST 检查"升级到"进程级隔离"，系统具备基本的可观测性。

| 编号 | 任务 | 模块 | 验收标准 |
|------|------|------|---------|
| W3-1 | 用户策略代码独立进程执行 | `sandbox/executor.py` | 策略在子进程中运行（multiprocessing），父进程通过队列获取信号，子进程超时 5 分钟自动 kill |
| W3-2 | 子进程资源限制（CPU/内存） | `sandbox/executor.py` | 子进程内存限制 512MB，CPU 时间限制 30 秒，超限自动终止并返回 SecurityError |
| W3-3 | 结构化日志 + 请求追踪 | `api/core/logging.py` | 每个请求生成 request_id，贯穿 FastAPI -> Celery -> DB，日志输出 JSON 格式 |
| W3-4 | 回测任务进度实时推送 | `api/routers/` + WebSocket | 客户端通过 WS 接收 "pending -> running -> success" 状态变更，替代轮询 |
| W3-5 | 基础 Metrics 端点 | `api/routers/` | `/metrics` 暴露回测队列长度、平均执行时间、AI 调用次数等 Prometheus 格式指标 |

### 第四波：前端 + 体验优化（与后端并行，需前端人力）

| 编号 | 任务 | 说明 |
|------|------|------|
| W4-1 | JWT 迁移到 httpOnly cookie | 后端提供 `Set-Cookie` 响应，前端移除 localStorage 读写 |
| W4-2 | 回测结果图表降采样 | equity_curve 超过 1000 点时后端自动降采样为周度/月度 |
| W4-3 | 策略编辑器防抖 | 代码输入停止 500ms 后再触发参数提取正则 |

---

## 建议立即启动的三件事

### 1. 数据库 migration 004

需要为本次新增的 `refresh_tokens` 和 `backtest_cache` 表创建 migration：

```sql
-- 004_auth_and_cache.sql
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    token_jti VARCHAR(36) UNIQUE NOT NULL,
    family_id VARCHAR(36) NOT NULL,
    revoked BOOLEAN DEFAULT FALSE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_refresh_tokens_jti ON refresh_tokens(token_jti);
CREATE INDEX idx_refresh_tokens_family ON refresh_tokens(family_id);
```

### 2. requirements.txt

```txt
fastapi>=0.110.0
uvicorn[standard]>=0.27.0
sqlalchemy[asyncio]>=2.0.0
asyncpg>=0.29.0
psycopg2-binary>=2.9.0
alembic>=1.13.0
redis>=5.0.0
celery>=5.3.0
pandas>=2.0.0
numpy>=1.24.0
httpx>=0.27.0
python-jose[cryptography]>=3.3.0
passlib[bcrypt]>=1.7.4
pydantic>=2.5.0
pydantic-settings>=2.1.0
python-multipart>=0.0.6
```

### 3. pytest 入口

将 `tests/test_backtest_engine.py` 改为 pytest 风格（fixture + parametrize），新增 `pytest.ini`：

```ini
[pytest]
testpaths = tests
pythonpath = server
asyncio_mode = auto
```

---

## 风险与依赖

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 前端人力不足 | W4 波次延期 | 后端先完成 W1-W3，前端可独立并行 |
| Tushare 免费版限频 | W2-1 批量接口可能受限 | 申请 Tushare Pro 或接入 AKShare 作为备用 |
| 子进程隔离复杂度 | W3-1 可能引入跨平台问题 | 先支持 Linux（生产环境），Windows/Mac 后续适配 |

---

## 建议的评审节点

- **第一波完成后**: 进行工程实践复评，确认 CI 流水线就绪
- **第二波完成后**: 进行数据层复评，确认同步效率和质量校验
- **第三波完成后**: 进行安全复评，确认沙箱隔离有效，准备公开 Beta

---

**规划人**: Claude (Technical Review)  
**建议下次同步**: 第一波完成后（约 1 周后）
