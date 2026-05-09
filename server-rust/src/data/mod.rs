//! 数据同步模块
//!
//! 负责从外部数据源（Tushare、AKShare）拉取行情数据，写入 TimescaleDB。

pub mod tushare;

pub use tushare::TushareClient;
