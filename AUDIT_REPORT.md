# ACDA-Q 生产部署前代码审计报告

**审计日期**: 2026-05-31  
**审计范围**: Rust 后端 / React 前端 / Python 沙箱 / 数据同步 / 部署配置  
**审计方法**: 多模块并行代码审查

---

## 📊 总览

| 严重程度 | 数量 | 说明 |
|---------|------|------|
| 🔴 Critical | 3 | 需立即修复，存在安全/数据风险 |
| 🟠 High | 5 | 应尽快修复，影响稳定性 |
| 🟡 Medium | 8 | 建议修复，改善可靠性 |
| 🟢 Low | 6 | 建议改进，非阻塞 |

---

## 🔴 Critical（3 项）

### C1. 沙箱 exec() 隔离不完整 — 用户代码可访问 `os` 模块
- **文件**: `sandbox_runner.py:355-356`
- **问题**: `local_ns` 中 `__builtins__` 被替换为 `_SAFE_BUILTINS`，但 `exec()` 的全局命名空间中仍可通过 `__import__('os')` 绕过限制。`_SAFE_BUILTINS` 包含了 `__build_class__`，这允许通过类定义间接导入被禁止的模块。
- **风险**: 恶意策略代码可能逃逸沙箱，访问文件系统、执行系统命令。
- **修复建议**: 
  1. 在 `_SAFE_BUILTINS` 中移除 `__build_class__`
  2. 添加自定义 `__import__` 函数，只允许导入 `math`, `datetime`, `pandas` 等安全模块
  3. 考虑使用 `seccomp` 或 Docker 容器做进程级隔离

### C2. SECRET_KEY 硬编码在 .env 文件中
- **文件**: `.env.acdaq` (本地仓库)
- **问题**: `SECRET_KEY=686d35904d55d21a81d4f8838788d3722933dbd9b5a8fdf9a0ce1c112e8a5fac` 直接写在文件中，且该文件在 Git 仓库中。
- **风险**: 密钥泄露可导致 JWT 伪造、API Key 解密、用户数据泄露。
- **修复建议**: 
  1. 确认 `.gitignore` 包含 `.env*`
  2. 生产环境使用环境变量注入，不存储在文件中
  3. 如果已推送到 Git，立即轮换密钥

### C3. 数据库密码在 Python 脚本中明文拼接
- **文件**: `sandbox_runner.py:75`, `scripts/fetch_minute_data.py`
- **问题**: `url = f"postgresql://{DB_USER}:{DB_PASSWORD}@{DB_HOST}:{DB_PORT}/{DB_NAME}"` 密码直接拼接到连接字符串。
- **风险**: 进程列表、日志、错误信息中可能泄露密码。
- **修复建议**: 使用 `sqlalchemy.URL.create()` 或环境变量隔离密码。

---

## 🟠 High（5 项）

### H1. WebSocket 连接无认证
- **文件**: `server-rust/src/websocket.rs:ws_backtest_handler`
- **问题**: WebSocket 升级端点 `/ws/backtests/{job_id}/ws` 没有认证中间件，任何知道 job_id 的人都能订阅进度。
- **风险**: 信息泄露（其他用户的回测进度）。
- **修复建议**: 在 WebSocket 握手时验证 JWT token 或 session cookie。

### H2. CORS 配置可能过于宽松
- **文件**: `server-rust/src/main.rs:80-85`
- **问题**: `cors_origins` 默认为 `*`，虽然生产环境有检查（`debug=false` 时不允许 `*`），但如果 debug 模式误开，CORS 完全开放。
- **风险**: 跨站请求伪造、数据窃取。
- **修复建议**: 硬编码生产环境允许的域名列表，不依赖 debug 标志。

### H3. 沙箱资源限制在非 Linux 平台无效
- **文件**: `server-rust/src/sandbox/resource.rs:67-70`
- **问题**: macOS 上 `RLIMIT_AS` 不支持，资源限制代码被跳过（仅打印日志）。
- **风险**: 在 macOS 开发环境测试时，恶意策略代码可能耗尽内存。
- **修复建议**: macOS 上使用 `resource.setrlimit` 设置 `RLIMIT_AS`（虽然不完全支持，但可限制虚拟内存）。

### H4. 进程池 Mutex 锁可能存在死锁
- **文件**: `server-rust/src/sandbox/executor.rs` (ProcessPool execute 方法)
- **问题**: `pool.lock()` 获取锁后在持有锁的情况下调用 `send_request`（可能阻塞），然后再次获取锁更新状态。虽然有 `unwrap_or_else(|e| e.into_inner())` 处理 poisoned lock，但长时间持锁会阻塞其他回测任务。
- **风险**: 并发回测时性能瓶颈或死锁。
- **修复建议**: 缩小锁持有范围，只在获取/释放进程时加锁。

### H5. refresh token rotation 未实现
- **文件**: `server-rust/src/auth.rs:create_refresh_token`
- **问题**: refresh token 有 `family` 字段（用于 rotation 检测），但没有实现 rotation 逻辑。同一个 refresh token 可以无限次使用。
- **风险**: refresh token 泄露后无法被检测和撤销。
- **修复建议**: 实现 refresh token rotation，每次刷新时发放新 token 并废弃旧 token。

---

## 🟡 Medium（8 项）

### M1. 前端 token 刷新存在竞态条件
- **文件**: `client/src/services/api.ts:95-120`
- **问题**: 多个并发请求同时收到 401 时，会同时触发 refresh token 请求，导致多个 refresh 操作竞争。
- **修复建议**: 使用 mutex 或 queue 模式，确保同时只有一个 refresh 请求在执行。

### M2. sandbox_runner.py DB 密码从 URL 解析但无 SSL
- **文件**: `sandbox_runner.py:75`
- **问题**: 数据库连接使用明文 PostgreSQL 协议（非 SSL），密码在网络上明文传输。
- **修复建议**: 在数据库连接字符串中添加 `?sslmode=require`。

### M3. 数据同步脚本无速率限制保护
- **文件**: `scripts/fetch_minute_data.py`
- **问题**: 虽然有 `RATE_LIMIT_DELAY=0.3`，但 10 线程并行时实际请求频率可达 ~33/s，可能触发 API 限流。
- **修复建议**: 使用令牌桶算法控制全局请求频率，而非简单的 sleep。

### M4. TimescaleDB 压缩 chunk 解压后未重新压缩
- **文件**: `scripts/fetch_minute_data.py:decompress_overlapping_chunks`
- **问题**: 解压后的 chunk 不会自动重新压缩，随着时间推移，压缩率会下降。
- **修复建议**: 在同步完成后调用 `compress_chunk()` 重新压缩。

### M5. 前端 `confirm()` 弹窗用户体验差
- **文件**: `client/src/services/api.ts:116`
- **问题**: token 过期时使用 `window.confirm()` 弹窗，阻塞主线程且样式不可定制。
- **修复建议**: 使用 Toast 或自定义 Modal 组件。

### M6. 配置文件 `.env.acdaq` 包含敏感信息
- **文件**: `.env.acdaq`
- **问题**: 文件包含 SECRET_KEY、数据库密码等，且未在 `.gitignore` 中排除。
- **修复建议**: 将 `.env.acdaq` 加入 `.gitignore`，使用 `.env.example` 代替。

### M7. sandbox_runner.py 中 `print` 被静默吞掉
- **文件**: `sandbox_runner.py:355`
- **问题**: `"print": lambda *a: None` 静默丢弃所有 print 输出，用户调试策略时无法看到输出。
- **修复建议**: 将 print 输出重定向到 stderr 或日志。

### M8. 前端 `EquityCurveChart` tooltip 颜色未完全适配主题
- **文件**: `client/src/components/EquityCurveChart.tsx`
- **问题**: tooltip 内的文字颜色（如 `d2.style.cssText`）仍使用硬编码颜色，未完全跟随主题切换。
- **修复建议**: 使用 CSS 变量或在 tooltip 创建时读取主题颜色。

---

## 🟢 Low（6 项）

### L1. `secret_key_bytes()` 每次调用创建新 Vec
- **文件**: `server-rust/src/config.rs:secret_key_bytes`
- **问题**: 每次验证 token 都会分配新内存。
- **修复建议**: 缓存为 `Vec<u8>` 或使用 `Bytes`。

### L2. 进程池大小硬编码为 3
- **文件**: `server-rust/src/sandbox/executor.rs:const POOL_SIZE: usize = 3`
- **问题**: 不同服务器配置可能需要不同的池大小。
- **修复建议**: 从配置或环境变量读取。

### L3. `sqlx::migrate!` 失败仅 warn 不 fail
- **文件**: `server-rust/src/main.rs:65-67`
- **问题**: 数据库迁移失败只打 warn 日志，服务继续启动，可能导致数据不一致。
- **修复建议**: 生产环境迁移失败应阻止启动。

### L4. 前端 K 线图 `getTradeColor` 使用 HSL 但色相分布不均
- **文件**: `client/src/components/KLineChart.tsx:getTradeColor`
- **问题**: `hue = (index * 137.508) % 360` 黄金角分布，但 0.3 透明度在浅色主题下对比度不足。
- **修复建议**: 浅色主题下增加不透明度。

### L5. `fetch_minute_data.py` 中 `import akshare` 仍保留但未使用
- **文件**: `scripts/fetch_minute_data.py:28-31`
- **问题**: akshare 导入保留但实际使用 Sina API，增加启动时间和依赖。
- **修复建议**: 移除未使用的 akshare 导入。

### L6. `SandboxConfig` 默认 CPU 限制 60s 但 timeout 300s
- **文件**: `server-rust/src/sandbox/executor.rs:46-48`
- **问题**: CPU 时间限制（60s）远小于超时时间（300s），大部分时间进程在等待 I/O。
- **修复建议**: 调整 CPU 限制为更合理的值（如 120s）。

---

## ✅ 安全亮点

1. **JWT 认证**: 使用 HS256 + bcrypt，token 有 jti 和 family 字段
2. **CORS 生产保护**: debug=false 时禁止通配符 CORS
3. **错误隐藏**: 生产环境 `HIDE_INTERNAL_DETAILS=true`，不暴露内部错误
4. **加密模块**: AES-256-GCM 加密敏感配置值，自动解密
5. **输入验证**: SECRET_KEY 最小长度 32 字符
6. **Token 存储**: 浏览器模式使用 httpOnly cookie，JS 不可读
7. **设备指纹**: 使用 FingerprintJS 生成稳定指纹

---

## 📋 修复优先级建议

| 优先级 | 编号 | 修复内容 | 预估工时 |
|--------|------|---------|---------|
| P0 | C1 | 沙箱 exec() 隔离加固 | 2h |
| P0 | C2 | 轮换 SECRET_KEY + .gitignore | 30min |
| P1 | H1 | WebSocket 添加认证 | 1h |
| P1 | H5 | Refresh token rotation | 2h |
| P1 | M1 | 前端 token 刷新竞态修复 | 1h |
| P2 | H2 | CORS 硬编码生产域名 | 30min |
| P2 | M4 | 压缩 chunk 重新压缩 | 1h |
| P3 | 其他 | Low/Medium 项 | 3-4h |

---

*报告生成时间: 2026-05-31 12:00 CST*
