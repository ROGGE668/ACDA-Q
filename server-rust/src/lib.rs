//! ACDA-Q 回测引擎 — Rust 核心实现
//!
//! 高性能、零开销抽象、编译期类型安全。

pub mod backtest;

// PyO3 绑定暂时注释，后续实现
// #[cfg(feature = "pyo3-binding")]
// use pyo3::prelude::*;
//
// #[cfg(feature = "pyo3-binding")]
// #[pymodule]
// fn acda_q(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
//     m.add_class::<backtest::BacktestEngine>()?;
//     Ok(())
// }
