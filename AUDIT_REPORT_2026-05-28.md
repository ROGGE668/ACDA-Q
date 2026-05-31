# ACDA-Q 综合审计报告

**审计日期**: 2026-05-28  
**审计方式**: 4 Agent 并行审计 + 自动化测试验证  
**项目版本**: v0.2.0 (Rust 后端 + React 前端)

---

## 一、测试验证结果

| 测试项 | 结果 | 详情 |
|--------|------|------|
| Rust 单元测试 | ✅ 22/22 通过 | broker, engine, auth, config, sandbox, ai, market |
| TypeScript 类型检查 | ✅ 无错误 | `tsc --noEmit` 通过 |
| Vite 构建 | ✅ 成功 | 669ms, 434KB JS bundle |
| sandbox_runner.py KPI 计算 | ✅ 8/8 指标正确 | total_return, max_drawdown, sharpe, sortino, calmar, win_rate, profit_ratio, duration |
| 前后端 API 路由一致性 | ⚠️ 27/28 | `strategyAPI.validate` 路由不匹配（死代码） |

---

## 二、审计汇总

| 审计维度 | Critical | High | Medium | Low | 合计 |
|----------|----------|------|--------|-----|------|
| 安全审计 | 5 | 5 | 7 | 4 | **21** |
| 错误处理 | 0 | 5 | 11 | 6 | **22** |
| 前端质量 | 3 | 7 | 10 | 4 | **24** |
| 基础设施 | 3 | 7 | 8 | 5 | **23** |
| **合计** | **11** | **24** | **36** | **19** | **90** |

---

## 三、Critical 问题（11 个）

### 🔴 安全 - 5 个

| # | 问题 | 文件 | 说明 |
|---|------|------|------|
| S-C1 | `.env` 含真实密钥未排除版本控制 | `server-rust/.env` | SECRET_KEY 可能已提交 Git 历史 |
| S-C2 | 数据库默认弱密码硬编码 | `docker-compose*.yml` | `quant123` 作为回退默认值 |
| S-C3 | Admin API 泄露密码哈希 | `server-rust/src/api/admin.rs` | list_users 返回 password_hash |
| S-C4 | 沙箱 exec() 无 builtins 隔离 | `sandbox_runner.py` | 用户代码可访问 `__import__` |
| S-C5 | 沙箱安全过滤可绕过 | `server-rust/src/backtest/validator.rs` | `importlib`、`__builtins__` 未过滤 |

### 🔴 错误处理 - 0 个

### 🔴 前端 - 3 个

| # | 问题 | 文件 | 说明 |
|---|------|------|------|
| F-C1 | `backtestStore.fetchJobs` 无 try/catch | `client/src/stores/backtestStore.ts:13` | API 失败时 UI 永久卡死 loading |
| F-C2 | `BacktestListPage.loadJobs` 无 try/catch | `client/src/pages/BacktestListPage.tsx:26` | 同上 |
| F-C3 | authStore.login/register 为死代码 | `client/src/stores/authStore.ts:30` | LoginPage 绕过 store 直接调 API |

### 🔴 基础设施 - 3 个

| # | 问题 | 文件 | 说明 |
|---|------|------|------|
| I-C1 | Dockerfile.local 缺少 Python 依赖 | `Dockerfile.local` | 沙箱执行将失败 |
| I-C2 | docker-compose.prod.yml 引用旧 Python 服务 | `docker-compose.prod.yml:68` | `build.context: ./server` 而非 `./server-rust` |
| I-C3 | 沙箱资源限制空实现 | `server-rust/sandbox/resource.rs:80` | `build_constrained_command()` 不做任何限制 |

---

## 四、High 问题（24 个）

### 🟠 安全 - 5 个

| # | 问题 | 文件 |
|---|------|------|
| S-H1 | Admin API 无速率限制 | `middleware/rate_limit.rs` |
| S-H2 | 登录接口无 CAPTCHA | `api/auth.rs` |
| S-H3 | CORS 配置可能过于宽松 | `config.rs` |
| S-H4 | Token 刷新无旋转 | `auth.rs` |
| S-H5 | WebSocket 无认证检查 | `websocket.rs` |

### 🟠 错误处理 - 5 个

| # | 问题 | 文件 |
|---|------|------|
| E-H1 | Mutex lock().unwrap() 可能 panic | `executor.rs:229` |
| E-H2 | analyzer.rs running_max 除零风险 | `analyzer.rs:47` |
| E-H3 | executor 阻塞读取无超时 | `executor.rs:138` |
| E-H4 | 进程池不回收死亡子进程 | `executor.rs:156` |
| E-H5 | queue.rs 每次操作新建 Redis 连接 | `queue.rs` |

### 🟠 前端 - 7 个

| # | 问题 | 文件 |
|---|------|------|
| F-H1 | Token 刷新失败后双重响应 | `api.ts:112` |
| F-H2 | PrivateRoute hydration 竞态 | `PrivateRoute.tsx:9` |
| F-H3 | 全项目 6+ 处使用 alert() | `pages/*.tsx` |
| F-H4 | API Key 明文存 localStorage | `aiSettingsStore.ts:47` |
| F-H5 | Layout logout 导航冲突 | `Layout.tsx:42` |
| F-H6 | useEffect 无限循环风险 | `DashboardPage.tsx:16` |
| F-H7 | 策略编辑器含 debug alert 代码 | `StrategyEditorPage.tsx:105` |

### 🟠 基础设施 - 7 个

| # | 问题 | 文件 |
|---|------|------|
| I-H1 | Dockerfile 构建缓存未优化 | `server-rust/Dockerfile:9` |
| I-H2 | RUST_BACKTRACE=1 生产暴露 | `server-rust/Dockerfile:39` |
| I-H3 | Nginx 无安全头 | `nginx/nginx.rust-only.conf` |
| I-H4 | 所有端口暴露到宿主机 | `docker-compose.rust-only.yml` |
| I-H5 | 无 Docker 网络隔离 | `docker-compose.rust-only.yml` |
| I-H6 | CI/CD 无回滚机制 | `.github/workflows/deploy.yml` |
| I-H7 | WebSocket 代理超时 24h | `nginx/nginx.rust-only.conf:53` |

---

## 五、Medium 问题（36 个）— 精选关键项

| 维度 | # | 问题 | 文件 |
|------|---|------|------|
| 安全 | S-M1 | IP 限流可被代理池绕过 | `rate_limit.rs:98` |
| 安全 | S-M2 | Validator 黑名单不完整 | `validator.rs:100` |
| 错误 | E-M1 | 三个连接池 idle_timeout 不一致 | `db.rs` |
| 错误 | E-M2 | T+1 检查仅用 bars[0] 判断日期 | `broker.rs:161` |
| 错误 | E-M3 | 新上市首日涨跌停判断异常 | `broker.rs:187` |
| 错误 | E-M4 | context.rs history_cache 每天重建 | `context.rs:22` |
| 错误 | E-M5 | datafeed volume i64→u64 可溢出 | `datafeed.rs:79` |
| 错误 | E-M6 | stderr 被丢弃无法调试 | `executor.rs:107` |
| 前端 | F-M1 | BacktestJob 接口重复定义 | `BacktestListPage.tsx:9` |
| 前端 | F-M2 | StockSelector 无防抖 | `StockSelector.tsx:44` |
| 前端 | F-M3 | innerHTML XSS 隐患 | `EquityCurveChart.tsx:135` |
| 前端 | F-M4 | 参数提取逻辑重复 | `StrategyEditorPage.tsx:88` |
| 前端 | F-M5 | 3 个未使用依赖包 | `package.json` |
| 前端 | F-M6 | escapeHtml 遗漏单引号 | `BacktestResultPage.tsx:196` |
| 基础 | I-M1 | Nginx 缺速率限制和 body 大小限制 | `nginx.rust-only.conf` |
| 基础 | I-M2 | API 服务无 restart 策略 | `docker-compose.rust-only.yml` |
| 基础 | I-M3 | config.rs 缺少关键字段验证 | `config.rs:86` |
| 基础 | I-M4 | TimescaleDB 未配置压缩策略 | `migrations/002_market_data.sql` |
| 基础 | I-M5 | 数据库迁移无回滚脚本 | `migrations/` |

---

## 六、Low 问题（19 个）— 精选

| 维度 | 问题 |
|------|------|
| 前端 | KLineChart 大量 `as any` 类型断言 |
| 前端 | BacktestResultPage 548 行应拆分 |
| 前端 | SettingsPage 加载失败无重试 |
| 基础 | Docker Compose version 字段已废弃 |
| 基础 | Nginx 无日志配置 |
| 基础 | Nginx 缺 gzip 压缩 |
| 基础 | ssl/ 目录为空 |
| 错误 | AppError 缺少 Clone/PartialEq |
| 错误 | config Default impl 中的 expect |

---

## 七、优先修复路线图

### Phase 1 — 立即修复（生产阻断）
1. `.gitignore` 排除 `.env` + 轮换已暴露密钥
2. Admin API 移除 password_hash 字段
3. `backtestStore.fetchJobs` 添加 try/catch
4. 删除 `StrategyEditorPage.tsx:105` 的 debug alert
5. `Dockerfile.local` 添加 Python 依赖

### Phase 2 — 本周修复（安全加固）
6. 沙箱 builtins 隔离（移除 `__import__`、`open`、`exec`）
7. Validator 黑名单补充 `__builtins__`、`importlib`
8. executor.rs Mutex poison 容错
9. executor.rs 添加阻塞读取超时
10. 进程池死亡子进程自动回收

### Phase 3 — 下周修复（质量提升）
11. Nginx 添加安全头
12. Docker 网络隔离
13. 替换所有 alert() 为 toast 组件
14. 连接池 idle_timeout 统一配置
15. 前端 API 路由一致性修复

### Phase 4 — 持续改进
16. CSS Modules/Tailwind 迁移
17. CI/CD 回滚机制
18. TimescaleDB 压缩策略
19. 数据库迁移版本管理

---

## 八、亮点

- **SQL 注入防护优秀**: 全部 handler 使用 sqlx 参数化查询
- **生产环境错误隐藏**: `HIDE_INTERNAL_DETAILS` 保护内部错误
- **Rust 单元测试覆盖**: 22 个测试全部通过
- **前后端路由高度一致**: 28 个路由 27 个匹配
- **进程池复用优化**: 沙箱冷启动 2.2s → 复用 0.5s
- **KPI 计算完整**: 所有绩效指标正确计算

---

*报告由 4 个并行审计 Agent 生成，覆盖安全、错误处理、前端质量、基础设施四个维度。*
