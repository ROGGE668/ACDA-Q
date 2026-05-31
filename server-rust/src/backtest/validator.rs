//! 策略验证器 — 参数检查 + 安全过滤 + Smoke Test
//!
//! Phase 3 最小实现：
//! - 内置策略参数合法性检查
//! - 自定义策略代码危险关键词过滤
//! - Mock 数据 Smoke Test（执行一次回测验证不 panic）

use rust_decimal_macros::dec;
use serde_json::Value;

use crate::backtest::engine::{generate_mock_bars, Engine};
use crate::backtest::worker::{BuyAndHold, DualMA};

/// 验证结果
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

impl ValidationResult {
    fn ok() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
        }
    }

    fn err(errors: Vec<String>) -> Self {
        Self {
            valid: errors.is_empty(),
            errors,
        }
    }
}

// ========== 参数验证 ==========

/// 验证内置策略参数
pub fn validate_builtin_params(strategy_type: &str, params: &Value) -> ValidationResult {
    let mut errors = Vec::new();

    match strategy_type {
        "buy_and_hold" => {
            // 无需额外参数
        }
        "dual_ma" => {
            let short = params
                .get("short_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(5);
            let long = params
                .get("long_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(20);

            if short == 0 || long == 0 {
                errors.push("window sizes must be greater than 0".to_string());
            }
            if short >= long {
                errors.push(format!(
                    "short_window ({}) must be less than long_window ({})",
                    short, long
                ));
            }
            if long > 250 {
                errors.push("long_window must not exceed 250".to_string());
            }
        }
        _ => {
            errors.push(format!(
                "Unknown built-in strategy type: {}. Supported: buy_and_hold, dual_ma",
                strategy_type
            ));
        }
    }

    // 通用检查
    if let Some(initial_cash) = params.get("initial_cash").and_then(|v| v.as_f64()) {
        if initial_cash <= 0.0 {
            errors.push("initial_cash must be greater than 0".to_string());
        }
    }

    if let Some(symbols) = params.get("symbols").and_then(|v| v.as_array()) {
        if symbols.is_empty() {
            errors.push("symbols must not be empty".to_string());
        }
    }

    ValidationResult::err(errors)
}

// ========== 安全过滤 ==========

/// 危险关键词黑名单
const FORBIDDEN_KEYWORDS: &[&str] = &[
    "import os",
    "import sys",
    "import shutil",
    "import pathlib",
    "import socket",
    "import urllib",
    "import requests",
    "import http",
    "import ftp",
    "import pickle",
    "import marshal",
    "import ctypes",
    "import threading",
    "import multiprocessing",
    "subprocess",
    "open(",
    "exec(",
    "eval(",
    "compile(",
    "__import__",
    "os.",
    "sys.",
    "shutil.",
    "socket.",
    "urllib.",
    "requests.",
    "http.",
    "ftp.",
    "pickle.",
    "marshal.",
    "ctypes.",
    "threading.",
    "multiprocessing.",
    "__builtins__",
    "importlib",
    "builtins",
    "__subclasses__",
    "__globals__",
    "__code__",
    "__class__",
    "__bases__",
    "__mro__",
    "__loader__",
    "__spec__",
    "__file__",
    "__name__",
];

/// 验证自定义策略代码安全性
pub fn validate_custom_code(code: &str) -> ValidationResult {
    let mut errors = Vec::new();

    if code.trim().is_empty() {
        errors.push("Strategy code is empty".to_string());
        return ValidationResult::err(errors);
    }

    // 危险关键词检查
    for keyword in FORBIDDEN_KEYWORDS {
        if code.contains(keyword) {
            errors.push(format!("Forbidden keyword detected: {}", keyword));
        }
    }

    // 基础语法检查：Python 策略应包含 on_bar
    if !code.contains("on_bar") {
        errors.push("Strategy code must contain an 'on_bar' method".to_string());
    }

    ValidationResult::err(errors)
}

// ========== Smoke Test ==========

/// 用 mock 数据执行一次回测，验证策略不会 panic
pub fn smoke_test(strategy_type: &str, params: &Value) -> ValidationResult {
    let mut errors = Vec::new();

    let bars = generate_mock_bars("000001.SZ", 60, 99);
    if bars.is_empty() {
        return ValidationResult::err(vec!["Failed to generate mock bars".to_string()]);
    }

    match strategy_type {
        "buy_and_hold" => {
            let result = std::panic::catch_unwind(move || {
                let mut engine = Engine::new(dec!(1_000_000));
                let mut strategy = BuyAndHold::new();
                let _ = engine.run(&mut strategy, &bars);
            });
            if result.is_err() {
                errors.push("Strategy panicked during smoke test".to_string());
            }
        }
        "dual_ma" => {
            let short = params
                .get("short_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as usize;
            let long = params
                .get("long_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as usize;
            let result = std::panic::catch_unwind(move || {
                let mut engine = Engine::new(dec!(1_000_000));
                let mut strategy = DualMA::new(short, long);
                let _ = engine.run(&mut strategy, &bars);
            });
            if result.is_err() {
                errors.push("Strategy panicked during smoke test".to_string());
            }
        }
        _ => {
            errors.push(format!(
                "Unknown strategy type for smoke test: {}",
                strategy_type
            ));
        }
    }

    ValidationResult::err(errors)
}
