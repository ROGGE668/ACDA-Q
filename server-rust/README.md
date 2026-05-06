# ACDA-Q Rust 回测引擎

高性能 A 股量化回测引擎，使用 Rust 重写 Python 版本。

## 特性

- **Decimal 精度**：金融计算零误差
- **A 股规则完整**：T+1、涨跌停（主板 10%/科创创业 20%/北交所 30%/ST 5%）、100 股整数倍、印花税、过户费
- **事件驱动**：按时间逐 bar 推送，与 Python 版本逻辑一致
- **性能**：预期比 Python 快 10-50 倍
- **Mock 数据**：确定性种子，便于测试对比

## 编译

```bash
cd ~/Documents/ACDA-Q/server-rust
cargo test          # 运行单元测试
cargo bench         # 运行性能基准测试
cargo build --release  # 编译优化版本
```

## 测试验证

```bash
cargo test test_dual_ma_backtest -- --nocapture
cargo test test_buy_hold_backtest -- --nocapture
cargo test test_t1_blocks_same_day_sell -- --nocapture
cargo test test_limit_up_blocks_buy -- --nocapture
```

## 与 Python 版本对比

```bash
# Python 版本
cd ~/Documents/ACDA-Q
python tests/test_backtest_engine.py

# Rust 版本
cd ~/Documents/ACDA-Q/server-rust
cargo bench
```

## 目录结构

```
server-rust/
├── Cargo.toml
├── Dockerfile
├── src/
│   ├── lib.rs              # PyO3 库入口
│   ├── backtest/
│   │   ├── mod.rs
│   │   ├── types.rs        # Bar/Order/Trade/Performance
│   │   ├── broker.rs       # 经纪商（资金、持仓、交易规则）
│   │   ├── context.rs      # 策略上下文（history/sma/ema/buy/sell）
│   │   └── engine.rs       # 回测主循环 + 绩效分析
│   └── main.rs             # API 服务入口（后续实现）
└── benches/
    └── backtest_bench.rs   # Criterion 基准测试
```
