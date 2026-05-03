# 量化投资App开发文档

## 1. 项目概述

### 1.1 项目目标
开发一款面向A股市场的量化投资平台，支持用户通过自然语言描述策略、AI自动生成并执行回测，最终输出可落地的交易信号与绩效报告。

### 1.2 核心特性
- **跨平台客户端**：支持 macOS 与 Windows
- **云端回测**：所有数据与计算均在云端完成，客户端仅作为交互入口
- **AI策略生成**：用户输入文字描述，AI生成可执行策略代码
- **多层级回测**：支持个股、多股组合、全市场扫描
- **闭源保护**：核心算法与数据不暴露于客户端

---

## 2. 系统架构

```
┌─────────────────────────────────────────────────────────────────┐
│                        客户端层 (Client)                         │
│  ┌──────────────┐  ┌──────────────┐                             │
│  │  macOS App   │  │ Windows App  │     Tauri + React/Vue       │
│  │ (Tauri/Web)  │  │ (Tauri/Web)  │     仅UI渲染 + 本地配置      │
│  └──────────────┘  └──────────────┘                             │
└───────────────────────────┬─────────────────────────────────────┘
                            │ HTTPS/WSS
┌───────────────────────────▼─────────────────────────────────────┐
│                      网关层 (Gateway)                            │
│              Nginx + CloudFlare (WAF/RateLimit)                 │
└───────────────────────────┬─────────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────────┐
│                      服务层 (Backend)                            │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  API Service (FastAPI / Go Gin)                          │   │
│  │  - 用户认证/授权 (JWT)                                    │   │
│  │  - 策略管理CRUD                                          │   │
│  │  - 任务调度与状态查询                                     │   │
│  │  - 回测结果查询                                           │   │
│  └─────────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  AI Service (Python + OpenAI/Claude API)                 │   │
│  │  - 自然语言 → 策略代码 (Prompt Engineering + RAG)         │   │
│  │  - 代码审查与安全沙箱                                     │   │
│  └─────────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Backtest Worker (Celery + Redis)                        │   │
│  │  - 个股回测                                              │   │
│  │  - 组合回测                                              │   │
│  │  - 全市场扫描                                            │   │
│  └─────────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Data Pipeline (Airflow / Cron)                          │   │
│  │  - A股历史行情数据同步                                    │   │
│  │  - 财务数据/基本面数据                                    │   │
│  │  - 实时行情订阅与缓存                                     │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────────┐
│                      数据层 (Data)                               │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
│  │ PostgreSQL  │ │   Redis     │ │  MinIO/S3   │               │
│  │ 用户/策略   │ │  缓存/队列  │ │  回测报告   │               │
│  │ 元数据      │ │  实时行情   │ │  静态资源   │               │
│  └─────────────┘ └─────────────┘ └─────────────┘               │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  ClickHouse / TimescaleDB                                │   │
│  │  - 行情时序数据 (日线/分钟线/Tick)                        │   │
│  │  - 回测日志与成交记录                                     │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────────┐
│                      数据源 (External)                           │
│  Tushare / AKShare / Baostock / Wind / 同花顺iFinD              │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. 技术栈选型

| 层级 | 技术选型 | 说明 |
|------|---------|------|
| **客户端** | Tauri 2.0 + React + TypeScript | 跨平台、体积小、性能接近原生，前端代码无核心逻辑 |
| **API网关** | Nginx + CloudFlare | 负载均衡、SSL终结、WAF、抗DDoS |
| **主服务** | Python FastAPI | 异步高性能，生态成熟，量化领域首选 |
| **回测引擎** | Python + NumPy/Pandas + 自研 | 参考Backtrader架构，自研保证灵活与闭源 |
| **AI服务** | Python + OpenAI API / 国产LLM | GPT-4 / Claude / 通义千问 / 文心一言 |
| **任务队列** | Celery + Redis | 异步回测任务调度 |
| **时序数据库** | TimescaleDB (PostgreSQL扩展) | 原生SQL，兼容性好，适合金融时序 |
| **缓存** | Redis | 会话、限频、热点数据、实时行情缓存 |
| **对象存储** | MinIO (兼容S3) | 回测报告PDF/CSV、用户头像 |
| **监控** | Prometheus + Grafana | 服务健康、回测队列堆积、API延迟 |
| **日志** | Loki + Grafana | 统一日志收集与查询 |

---

## 4. 功能模块设计

### 4.1 用户系统
- 注册/登录（邮箱/手机验证码）
- JWT Token刷新机制
- 会员等级与配额（回测次数、AI调用次数、市场范围限制）
- API Key管理（可选开放给高级用户）

### 4.2 策略中心
- **策略列表**：我的策略、官方示例、社区策略（可选）
- **策略编辑器**：支持自然语言输入 + 代码查看/手动编辑
- **版本管理**：策略修改历史记录，支持回滚
- **策略分享**：加密分享链接（仅展示绩效，不暴露源码）

### 4.3 AI策略生成
- **输入**：用户自然语言描述（如："10日均线上穿30日均线买入，跌破卖出，回测贵州茅台2020-2023年"）
- **处理流程**：
  1. 语义解析（提取标的、周期、条件、时间范围）
  2. 模板匹配（常见策略模板库RAG检索）
  3. LLM代码生成（生成Python回测代码）
  4. 代码安全审查（限制import白名单、禁止网络/文件操作）
  5. 语法校验与模拟运行
- **输出**：可执行策略代码 + 参数面板

### 4.4 回测引擎

#### 4.4.1 数据支持范围
| 数据类型 | 覆盖范围 | 频率 |
|---------|---------|------|
| A股行情 | 全部沪深京股票 | 日K / 分钟K / Tick（高级） |
| 指数行情 | 上证指数、沪深300、中证500等 | 日K / 分钟K |
| 基金 | ETF、LOF、场外基金 | 日K |
| 期货/期权 | 股指期货、商品期货（扩展） | 日K / 分钟K |
| 基本面 | 财务报表、业绩预告、股东持仓 | 季度/年度 |
| 技术指标 | MACD、KDJ、RSI、布林带等 | 实时计算 |
| 宏观数据 | 利率、CPI、PMI等（扩展） | 月度/季度 |

#### 4.4.2 回测模式
1. **个股回测**
   - 单标的策略验证
   - 支持参数调优（网格搜索/遗传算法）
   - 输出：净值曲线、交易记录、绩效指标

2. **组合回测**
   - 多标的等权/市值加权/自定义权重
   - 再平衡周期设置
   - 相关性分析、行业分布

3. **全市场扫描**
   - 对全市场标的运行策略筛选
   - 每日/每周定时扫描
   - 输出：股票池 + 买入/卖出信号列表

#### 4.4.3 回测报告
- 累计收益率、年化收益率
- 最大回撤、夏普比率、索提诺比率
- Calmar比率、胜率、盈亏比
- Alpha / Beta / 信息比率
- 月度/年度收益分布热力图
- 交易明细表（支持导出CSV/Excel）
- 净值曲线 vs 基准对比图

### 4.5 实时与模拟交易（二期）
- 对接券商API（QMT/Ptrade/聚宽）
- 模拟盘跟踪（Paper Trading）
- 实盘信号推送（Webhook/App通知）

---

## 5. 数据库设计（核心表）

```sql
-- 用户表
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) UNIQUE NOT NULL,
    phone VARCHAR(20),
    password_hash VARCHAR(255) NOT NULL,
    nickname VARCHAR(50),
    tier VARCHAR(20) DEFAULT 'free', -- free/pro/enterprise
    quota_backtest_daily INT DEFAULT 10,
    quota_ai_daily INT DEFAULT 5,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- 策略表
CREATE TABLE strategies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id),
    name VARCHAR(255) NOT NULL,
    description TEXT,
    type VARCHAR(50), -- single_stock / multi_stock / market_scan
    code TEXT NOT NULL, -- Python策略代码（加密存储）
    params JSONB DEFAULT '{}', -- 默认参数
    is_public BOOLEAN DEFAULT FALSE,
    version INT DEFAULT 1,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- 回测任务表
CREATE TABLE backtest_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id),
    strategy_id UUID REFERENCES strategies(id),
    status VARCHAR(20) DEFAULT 'pending', -- pending/running/success/failed
    scope VARCHAR(20), -- single/multi/scan
    symbols TEXT[], -- 标的列表
    start_date DATE,
    end_date DATE,
    initial_cash DECIMAL(15,2) DEFAULT 1000000,
    params JSONB DEFAULT '{}', -- 运行时参数
    result_summary JSONB, -- 绩效摘要
    result_report_path VARCHAR(255), -- MinIO报告路径
    error_message TEXT,
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);

-- AI生成记录表
CREATE TABLE ai_generations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id),
    prompt TEXT NOT NULL,
    generated_code TEXT,
    model VARCHAR(50),
    tokens_used INT,
    status VARCHAR(20),
    created_at TIMESTAMP DEFAULT NOW()
);
```

---

## 6. API设计（核心接口）

### 6.1 认证
```
POST /api/v1/auth/register
POST /api/v1/auth/login
POST /api/v1/auth/refresh
```

### 6.2 策略
```
GET    /api/v1/strategies              # 列表
POST   /api/v1/strategies              # 创建（含AI生成）
GET    /api/v1/strategies/{id}         # 详情
PUT    /api/v1/strategies/{id}         # 更新
DELETE /api/v1/strategies/{id}         # 删除
POST   /api/v1/strategies/{id}/backtest # 发起回测
```

### 6.3 回测
```
POST   /api/v1/backtests               # 提交回测任务
GET    /api/v1/backtests               # 回测历史列表
GET    /api/v1/backtests/{id}          # 回测详情与状态
GET    /api/v1/backtests/{id}/result   # 回测结果（含报告URL）
DELETE /api/v1/backtests/{id}          # 删除记录
```

### 6.4 市场数据（有限开放）
```
GET    /api/v1/market/stocks           # 股票列表（分页）
GET    /api/v1/market/quote/{symbol}   # 最新行情
GET    /api/v1/market/history/{symbol} # 历史K线
```

### 6.5 WebSocket
```
WS     /ws/v1/backtest/{job_id}        # 回测进度实时推送
```

---

## 7. 数据方案（A股）

### 7.1 数据来源
| 数据 | 来源 | 备注 |
|------|------|------|
| 历史行情 | Tushare Pro / AKShare | 日K免费，分钟K需积分/付费 |
| 实时行情 | Tushare / 新浪/腾讯接口 | 5秒级延迟，合规考虑 |
| 财务数据 | Tushare / 东方财富 | 季度更新 |
| 股票基础信息 | Tushare / Baostock | 上市状态、行业、概念 |

### 7.2 数据同步策略
- **初始全量**：上市以来所有历史数据
- **每日增量**：交易日收盘后自动拉取当日数据
- **复权处理**：前复权/后复权价格计算
- **数据校验**：成交量、价格范围异常检测

### 7.3 存储方案
- 原始数据 → TimescaleDB（ hypertable 按时间分区）
- 热点数据（最近2年）→ Redis缓存
- 下载的原始CSV → MinIO归档

---

## 8. AI策略生成详细设计

### 8.1 架构
```
用户输入
  → 意图识别模块 (Intent Classifier)
    → 提取：标的、时间、买入条件、卖出条件、仓位管理
  → RAG检索 (向量数据库: pgvector)
    → 相似策略模板 / 函数文档 / 示例代码
  → Prompt组装
    → 系统提示词 + 上下文 + 用户输入 + 约束条件
  → LLM生成 (GPT-4 / Claude / 通义千问)
    → 原始代码
  → 后处理
    → 语法检查 (AST解析)
    → 安全扫描 (禁止os/sys/subprocess等)
    → 模拟运行（空数据验证无异常）
  → 返回用户
```

### 8.2 安全约束
- **Import白名单**：仅允许 `numpy`, `pandas`, `ta-lib`, `scipy` 等量化相关库
- **黑名单**：禁止 `os`, `sys`, `subprocess`, `socket`, `requests`, `open`, `eval`, `exec`
- **资源限制**：CPU时间≤30s，内存≤512MB，禁止网络访问
- **沙箱执行**：Docker容器 / Firecracker MicroVM 中运行用户代码

### 8.3 Prompt模板示例
```
你是一位专业的量化策略工程师。请根据以下用户需求，编写一个符合我们平台API的Python策略。

平台约束：
- 类名必须为 Strategy，继承自 BaseStrategy
- 必须实现 on_bar(self, context, bar) 方法
- 使用 context.buy(symbol, amount) / context.sell(symbol, amount) 下单
- 可用指标库：talib, pandas, numpy
- 策略参数通过 self.params 访问

用户需求：
{user_input}

请输出完整可运行的Python代码，不要包含任何解释文字。
```

---

## 9. 回测引擎设计

### 9.1 核心架构
```python
class BacktestEngine:
    """基于事件驱动的回测引擎"""

    def __init__(self, initial_cash=1_000_000, commission=0.0003, slippage=0.001):
        self.initial_cash = initial_cash
        self.commission = commission
        self.slippage = slippage
        self.broker = Broker(cash=initial_cash, commission=commission, slippage=slippage)
        self.data_feed = DataFeed()
        self.strategy = None
        self.analyzer = PerformanceAnalyzer()

    def set_strategy(self, strategy_cls, params=None):
        self.strategy = strategy_cls(params=params)
        self.strategy.set_broker(self.broker)

    def run(self, symbols, start_date, end_date):
        # 加载数据
        bars = self.data_feed.load_bars(symbols, start_date, end_date)
        # 按时间戳顺序推送事件
        for timestamp, bar_group in bars.groupby('datetime'):
            context = Context(timestamp, self.broker, bar_group)
            self.strategy.on_bar(context, bar_group)
            self.broker.execute_orders(bar_group)
        # 计算绩效
        return self.analyzer.calculate(self.broker.trades, self.broker.equity_curve)
```

### 9.2 关键特性
- **事件驱动**：按时间轴顺序处理行情事件，支持多标的同时回测
- **滑点与佣金**：真实模拟交易成本
- **复权处理**：回测全程使用前复权价格，成交时记录真实价格
- **停牌处理**：自动跳过停牌日，不产生交易信号
- **涨跌停限制**：模拟A股T+1与涨跌停板约束
- **仓位与风控**：支持满仓、固定金额、Kelly公式等仓位管理

---

## 10. 闭源与安全防护

### 10.1 核心原则
**客户端永不暴露：策略源码、原始行情数据、回测中间结果**

### 10.2 防护措施
| 层级 | 措施 |
|------|------|
| **传输** | 全站HTTPS/WSS，TLS 1.3，证书固定 |
| **认证** | JWT短期令牌 + Refresh Token，API限频 |
| **代码** | 策略代码加密存储（AES-256），仅运行时解密 |
| **数据** | 行情数据不整包下发，仅返回回测结果摘要与图表数据 |
| **客户端** | Tauri打包，前端代码混淆，禁止DevTools（生产环境） |
| **服务端** | 代码沙箱执行，Docker隔离，网络策略限制外联 |
| **法律** | 用户协议明确代码所有权归属平台，禁止逆向工程 |

### 10.3 反逆向
- 服务端渲染关键图表（生成PNG/SVG返回），而非传递原始数据由客户端绘制
- 策略代码在服务端执行，客户端仅接收绩效指标与可视化结果
- 关键算法（如回测引擎核心）使用Cython编译或Rust扩展，增加反编译难度

---

## 11. 部署架构（Ubuntu服务器）

### 11.1 服务器配置
| 服务 | 配置 | 数量 |
|------|------|------|
| API服务 | 4C8G | 2+ |
| 回测Worker | 8C16G | 3+（根据负载弹性） |
| 数据库 | 4C16G + SSD | 2（主从） |
| TimescaleDB | 8C32G + NVMe | 1（可扩展集群） |
| Redis | 2C4G | 1（集群模式） |
| MinIO | 4C8G + HDD | 2（分布式） |

### 11.2 部署工具
- **容器化**：Docker + Docker Compose（开发）/ Kubernetes（生产）
- **CI/CD**：GitHub Actions / GitLab CI → 构建镜像 → 推送私有仓库 → K8s滚动更新
- **配置管理**：Helm Charts / Kustomize
- **监控栈**：Prometheus + Grafana + Alertmanager

### 11.3 网络拓扑
```
[用户] → CloudFlare CDN/WAF → Nginx (SSL/TCP) → K8s Ingress
                                      ↓
                                ┌─────┴─────┐
                                ↓           ↓
                           API Pods     WebSocket Pods
                                ↓           ↓
                           Celery Worker (回测)
                                ↓
                           Redis / PostgreSQL / TimescaleDB / MinIO
```

---

## 12. 开发里程碑

### Phase 1: MVP（8-10周）
- [ ] 基础用户系统与认证
- [ ] Tauri客户端框架搭建（登录、主界面）
- [ ] 历史行情数据同步（日K，全市场）
- [ ] 基础回测引擎（个股、日线）
- [ ] AI策略生成（基础自然语言→代码）
- [ ] 回测报告（净值曲线、基础指标）

### Phase 2: 功能完善（6-8周）
- [ ] 组合回测与全市场扫描
- [ ] 分钟级数据支持
- [ ] 技术指标库集成（TA-Lib）
- [ ] 策略模板库与社区分享
- [ ] 参数优化（网格搜索）
- [ ] 高级回测报告（多维度分析）

### Phase 3: 商业化与扩展（持续）
- [ ] 会员系统与支付集成
- [ ] 模拟盘与实盘信号
- [ ] 券商API对接
- [ ] 移动端App（React Native / Flutter）
- [ ] 机构版（私有化部署）

---

## 13. 风险与应对

| 风险 | 影响 | 应对措施 |
|------|------|---------|
| 数据源中断/涨价 | 高 | 多源冗余（Tushare+AKShare+自采），本地缓存全量历史 |
| LLM API成本/限流 | 中 | 国产模型备用（通义/文心），Prompt缓存，结果缓存 |
| 用户策略代码安全 | 高 | 沙箱执行、资源限制、代码审计、法律约束 |
| 回测性能瓶颈 | 中 | Celery水平扩展、预计算指标、TimescaleDB分区优化 |
| 合规监管 | 高 | 不提供直接下单功能，仅研究与信号；用户协议免责；资质咨询 |

---

## 14. 目录结构

```
quant-investment-app/
├── client/                     # Tauri客户端
│   ├── src/
│   │   ├── components/         # UI组件
│   │   ├── pages/              # 页面（策略/回测/报告）
│   │   ├── services/           # API调用封装
│   │   └── stores/             # 状态管理 (Zustand)
│   ├── src-tauri/              # Rust后端（仅窗口/系统调用）
│   └── package.json
├── server/                     # 云端服务
│   ├── api/                    # FastAPI主服务
│   │   ├── routers/
│   │   ├── models/
│   │   ├── services/
│   │   └── main.py
│   ├── ai/                     # AI策略生成服务
│   │   ├── prompts/
│   │   ├── generators/
│   │   └── validators/
│   ├── backtest/               # 回测引擎
│   │   ├── engine/
│   │   ├── indicators/
│   │   ├── broker/
│   │   └── analyzers/
│   ├── worker/                 # Celery任务
│   │   └── tasks.py
│   ├── data/                   # 数据同步
│   │   ├── syncers/
│   │   └── pipelines/
│   └── docker-compose.yml
├── database/                   # 迁移脚本
├── docs/                       # 文档
└── DEVELOPMENT_DOC.md          # 本文档
```

---

## 15. 附录

### 15.1 参考开源项目
- Backtrader: Python回测框架架构参考
- Tushare: A股数据接口规范
- VectorBT: 高性能向量化回测思路

### 15.2 合规提示
- 本平台定位为**量化研究与学习工具**，不构成投资建议
- 如需提供直接交易功能，需申请相应金融牌照或与持牌机构合作
- 用户策略运行结果不代表未来收益，需充分提示风险

---

**文档版本**: v1.0  
**编写日期**: 2026-04-25  
**状态**: 草稿，待评审
