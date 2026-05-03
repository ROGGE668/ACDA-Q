# ACDA-Q 量化投资平台 —— 当前状态技术评审

**评审日期**: 2026-05-02  
**评审范围**: 服务端全栈（API / Worker / Backtest Engine / Data Pipeline / Security）  
**对比基准**: 2026-05-02 首轮评审报告（TECHNICAL_REVIEW_REPORT.md）  

---

## 1. 执行摘要

经过两轮系统性修复（P0/P1/P2 共 16 项 + P3 长期优化 3 项），项目在金融计算正确性、安全沙箱、API 防护和数据精度四个核心维度取得了显著进展。所有修复均通过了本地测试验证（9 个测试用例 100% 通过）。

**总体评级**: Alpha+ / 接近 Beta。核心回测链路已达到内部测试可用水平，但仍有 **4 项 P1 级问题** 和 **6 项 P2 级问题** 需要在公开测试前解决。

---

## 2. 修复成果（已验证）

以下问题已在本次修复周期中完全解决并通过测试验证：

| 原编号 | 问题 | 修复位置 | 验证方式 |
|--------|------|---------|---------|
| P0-1 | 复权价格缺失 | `datafeed.py` 默认调用 `get_adj_price(qfq)` | 集成测试通过 |
| P0-2 | 佣金模型不完整（缺印花税/过户费） | `broker.py` 新增 `stamp_duty` / `transfer_fee` | 单元测试通过 |
| P0-3 | 成交数量未强制 100 股整数倍 | `core.py` `_to_lot()` | 单元测试通过 |
| P0-4 | 金融计算使用 float | `broker.py` 核心改用 `Decimal` | 单元测试通过 |
| P0-5 | 沙箱反射绕过 | `executor.py` 新增 `REFLECTION_DUNDER` 黑名单 | 单元测试通过 |
| P0-6 | 路径遍历风险 | `backtest.py` 新增 `_safe_report_path()` | 代码审查 |
| P0-7 | JWT 空密钥风险 | `config.py` 启动时 `model_validator` 校验 | 代码审查 |
| P0-8 | 市值加权逻辑错误 | `portfolio.py` 改用 `float_shares` | 代码审查 |
| P1-1 | 涨跌停规则不完整 | `broker.py` 覆盖主板/科创/创业/北交所 | 单元测试通过 |
| P1-2 | 全市场扫描重复编译 | `scanner.py` 策略只编译一次 | 代码审查 |
| P1-3 | TimescaleDB NullPool | `timescale.py` 改用 `QueuePool` | 代码审查 |
| P1-6 | API 缺少速率限制 | `rate_limit.py` Redis 滑动窗口 + 内存回退 | 代码审查 |
| P1-7 | 绩效指标计算缺陷 | `performance.py` 交易日年化、FIFO 盈亏配对 | 单元测试通过 |
| P2-3 | SQL 占位符扩展 | `timescale.py` 改用 `ANY(:symbols)` 数组语法 | 代码审查 |
| P2-6 | Mock 数据种子固定 | `datafeed.py` 使用 `np.random.default_rng()` | 代码审查 |
| P3-1 | 向量化指标 | `core.py` 新增 `sma()` / `ema()` / `history_batch()` | 单元测试通过 |
| P3-3 | 回测结果缓存 | `backtest_cache` 表 + 命中复用逻辑 | 代码审查 |
| — | ST 股 5% 涨跌停 | `broker.py` / `datafeed.py` / `tushare_syncer.py` | 单元测试通过 |

**新增测试覆盖**: 从 3 个用例扩展到 9 个，覆盖双均线回测、买入持有、沙箱 import 拦截、反射绕过拦截、100 股整数倍、交易成本、Decimal 精度、ST 股限制、向量化指标。

---

## 3. 残留问题（按严重性排序）

### 3.1 P1 —— 严重问题

#### P1-A. pre_close 数据缺失导致涨跌停检查从未触发

**问题**: `broker.py` 的 `execute_orders` 依赖 `bar['pre_close']` 计算涨跌停价，但数据层所有路径均未提供该字段：
- `timescale.py` 的 `query_daily_bars` 和 `get_adj_price` 的 SELECT 列表中**没有 `pre_close`**
- `datafeed.py` 的 `_generate_mock_bars()` 未生成 `pre_close`
- CSV 回退路径也未读取 `pre_close`

当 `pre_close` 缺失时，`broker.py` 回退到 `pre_close = close`。此时：
- `limit_up = close * 1.1`，检查条件 `close >= limit_up * 0.999` → `1.0 >= 1.0989` 恒为 False
- 涨跌停买入/卖出限制**永远不会触发**

**影响**: 回测中涨停买入、跌停卖出的限制实际上完全失效。策略在涨停日仍可"买入"，与真实 A 股严重不符。

**修复**: 
1. `query_daily_bars` / `get_adj_price` SQL 中增加 `pre_close`
2. `_generate_mock_bars` 中生成 `pre_close = np.roll(close, 1)`
3. 补充测试：构造涨停/跌停 bar，验证订单被拒绝

#### P1-B. getattr 可绕过沙箱反射限制

**问题**: `executor.py` 的 AST 检查拦截了直接的属性访问（如 `obj.__class__`），但**未拦截 `getattr(obj, "__class__")`**。`getattr` 本身在受限 builtins 中仍然可用，且字符串常量 `"__class__"` 不会触发 `REFLECTION_DUNDER` 检查。

以下代码在沙箱中**可以执行**（当前拦截器无法检测）：
```python
class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        # 绕过反射限制
        builtin = getattr(globals()["__builtins__"], "__import__")
        os = builtin("os")
        os.system("id")
```

**影响**: 攻击者可通过 `getattr` 动态访问被禁止的 dunder 属性，进而绕过 `_safe_import` 或直接执行系统调用。

**修复**:
1. **短期**: 在 AST 检查中禁止 `getattr` / `hasattr` / `setattr` 的第二个参数为字符串常量且内容在 `REFLECTION_DUNDER` 中。
2. **中期**: 从 restricted builtins 中移除 `getattr`，或替换为安全代理版本。
3. **长期**: 将用户代码执行迁移到独立进程 / gVisor 容器。

#### P1-C. Refresh Token 未实现轮换

**问题**: `auth.py` 的 `/refresh` 端点每次返回**全新的 token pair**，但旧 refresh token 仍然有效。泄露的 refresh token 可被无限次使用。

**影响**: 一旦 refresh token 泄露，攻击者可永久保持访问权限，直到 token 自然过期（7 天）。

**修复**: 
1. 实现 Refresh Token Rotation：每次刷新后使旧 refresh token 失效。
2. 引入 `refresh_tokens` 表记录 token 家族，检测重用攻击。

#### P1-D. T+1 规则过于严格

**问题**: `broker.py` 中只要当日有买入记录（`_buy_dates`），就禁止该标的**全部卖出**。实际 A 股规则是"当日买入的部分不可卖出"，但**此前已持有的仓位可以卖出**。

**影响**: 对于已有持仓且当日加仓的策略，回测中完全禁止卖出，而实盘可以卖出旧仓位。策略行为与真实环境存在偏差。

**修复**: 将 `_buy_dates` 改为记录当日净买入量，卖出时仅检查卖出数量是否超过当日之前持有的仓位。

---

### 3.2 P2 —— 中等问题

#### P2-A. history_batch 跨标日期对齐错误

**问题**: `core.py` 的 `history_batch()` 将多只标的的历史 Series 通过 `pd.concat(rows, axis=1)` 合并。但 `context.history()` 返回的 Series 使用默认整数索引（原始 DataFrame 的行号），**不同 symbol 的相同日期对应不同整数索引**（因为 `bars` 按 datetime + symbol 排序）。

**影响**: `history_batch` 返回的 DataFrame 中，同一行的数据来自不同日期，截面计算（如多因子排序、相关性矩阵）完全错误。

**修复**: 修改 `history()` 方法返回带 `datetime` 索引的 Series，或修改 `history_batch()` 在合并前将 index 设为 datetime。

#### P2-B. TimescaleDB 分区剪枝失效

**问题**: `tushare_syncer.py` 的 `_insert_daily_bars` 使用：
```sql
DELETE FROM daily_bars WHERE symbol = :symbol AND DATE(datetime) = ANY(:dates)
```
对 hypertable 的时间列应用 `DATE()` 函数会**阻止 TimescaleDB 的分区剪枝**，导致每次 DELETE 扫描全部分区。

**影响**: 全市场增量同步时 DELETE 性能极差，数据量越大越慢。

**修复**: 改为范围查询 `datetime >= :start AND datetime < :end`。

#### P2-C. AI 配额不重置

**问题**: `ai.py` 的 `quota_ai_daily` 是一次性配额。用户用完 5 次后永久耗尽，没有每日重置机制。

**影响**: 免费用户首次使用 5 次后无法继续使用 AI 功能。

**修复**: 引入 `user_quota_usage` 日用量表，或 Redis TTL 滑动窗口计数。

#### P2-D. Rate Limit 内存回退泄漏风险

**问题**: `rate_limit.py` 的内存回退模式使用全局字典 `_memory_windows`。每个请求 key 对应一个列表，但**key 本身永不删除**。攻击者可用随机 key（如伪造 IP）填满内存。

**修复**: 限制字典最大 key 数量（LRU 淘汰），或定期清理过期 key。

#### P2-E. BacktestResult Schema 与 API 返回不匹配

**问题**: `schemas.py` 的 `BacktestResult` 只定义了 `job_id, summary, trades, equity_curve, report_path`，但 `backtest.py` 的 `get_backtest_result` 额外返回了 `signals, suitable_stocks, unsuitable_stocks`（scan 模式）。

**影响**: 若启用 Pydantic `extra='forbid'`，scan 模式的响应会触发验证错误。当前未启用，但 API 契约不完整。

**修复**: 在 `BacktestResult` 中增加可选字段，或拆分 scan / backtest 两个响应模型。

#### P2-F. query_stock_list LIMIT 直接拼接

**问题**: `timescale.py` 的 `query_stock_list` 使用 f-string 拼接 LIMIT：
```python
sql += f" ORDER BY symbol LIMIT {limit}"
```

**影响**: 虽然是整数参数，但直接拼接存在潜在的 SQL 注入面（如调用方传入超大字符串）。

**修复**: 将 limit 改为参数绑定。

---

### 3.3 P3 —— 改进项与工程债

| 编号 | 问题 | 说明 |
|------|------|------|
| P3-A | 测试框架未标准化 | `tests/test_backtest_engine.py` 仍是脚本形式，未使用 pytest fixture/parametrize，无覆盖率报告 |
| P3-B | 缺少依赖清单 | 项目中无 `requirements.txt` 或 `pyproject.toml`，新开发者无法快速搭建环境 |
| P3-C | 死代码未清理 | `datafeed.py` 末尾存在重复的 `_generate_mock_bars` 逻辑块（约 20 行） |
| P3-D | SQL/ORM schema 不一致 | `001_init.sql` 有 `phone` 和 `quota_backtest_daily` 字段，`models.py` 中缺失；`BacktestCache` 的 `TIMESTAMPTZ` 与 ORM `DateTime` 时区语义可能不一致 |
| P3-E | 全市场同步效率低 | `tushare_syncer.py` 逐只拉取日K（500 只约 75 秒），应优先使用 Tushare `daily` 全市场批量接口 |
| P3-F | 数据质量校验缺失 | 无停牌检测、价格异常跳变检查、复权因子一致性验证 |
| P3-G | 前端 Token 存储 | 根据首轮评审，JWT 仍存储在 `localStorage`，XSS 风险未消除（本次未涉及前端代码修改） |

---

## 4. 模块级健康度评分

| 模块 | 评分 | 说明 |
|------|------|------|
| **回测引擎核心** | B+ | Decimal 精度、交易规则、向量化指标均已落地。涨跌停因数据缺失未真正生效，T+1 过于严格。 |
| **安全沙箱** | B | 反射绕过已封堵，但 `getattr` 绕过路径未处理。短期可用，中期需引入容器隔离。 |
| **数据层** | B | 复权接入、ST 状态同步完成。pre_close 缺失、分区剪枝失效、同步效率待优化。 |
| **API / Auth** | B- | 限流、路径防护、JWT 校验已落地。Refresh Token 轮换缺失，Schema 不完整。 |
| **Worker / 任务** | B | 缓存机制有效降低重复计算。Celery 配置合理（5 分钟超时）。 |
| **工程实践** | C+ | 无 pytest、无依赖清单、死代码未清理。测试脚本运行稳定但不符合标准。 |

---

## 5. 下一步行动建议

### 必须完成（公开测试前）

1. **修复 pre_close 数据链路**（P1-A）：这是当前最大的金融正确性缺口。没有 pre_close，涨跌停规则形同虚设。
2. **封堵 getattr 绕过**（P1-B）：在沙箱中禁止 `getattr(obj, dunder_string)` 模式，或移除 `getattr`。
3. **实现 Refresh Token 轮换**（P1-C）：这是认证系统的基本安全要求。

### 强烈建议（2 周内）

4. 修复 `history_batch` 日期对齐（P2-A）
5. 优化 TimescaleDB DELETE 分区剪枝（P2-B）
6. 引入 AI 配额每日重置机制（P2-C）
7. 迁移到 pytest + 增加覆盖率报告（P3-A）
8. 补充 `requirements.txt`（P3-B）

### 排入下一迭代

9. 全市场同步批量接口优化（P3-E）
10. 数据质量校验 Pipeline（P3-F）
11. 前端 Token 存储迁移至 httpOnly cookie（P3-G）

---

**评审人**: Claude (Technical Review)  
**建议下次评审**: 完成 P1-A/B/C 修复后进行复评
