//! ACDA-Q 量化投资平台 — Rust 核心库
//!
//! 包含高性能回测引擎、数据模型、认证模块、任务队列、AI 生成。
//! 可作为独立库使用，也可通过 PyO3 暴露给 Python。

#![allow(dependency_on_unit_never_type_fallback)]

pub mod ai;
pub mod api;
pub mod auth;
pub mod backtest;
pub mod config;
pub mod data;
pub mod db;
pub mod error;
pub mod metrics;
pub mod middleware;
pub mod models;
pub mod queue;
pub mod websocket;
