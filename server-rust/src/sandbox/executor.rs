//! 子进程执行器
//! 
//! 启动独立进程执行策略代码，通过 stdin/stdout 进行 JSON 通信。

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{error, warn};

/// 沙箱执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traceback: Option<String>,
}

/// 沙箱错误类型
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Timeout: strategy execution exceeded {0} seconds")]
    Timeout(u64),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("No result returned from subprocess")]
    NoResult,
}

/// 沙箱配置
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// 超时时间（秒）
    pub timeout_secs: u64,
    /// 内存限制（字节）
    pub memory_limit_bytes: u64,
    /// CPU 时间限制（秒）
    pub cpu_limit_secs: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 300,
            memory_limit_bytes: 512 * 1024 * 1024,
            cpu_limit_secs: 60,
        }
    }
}

/// 在沙箱中执行回测
pub fn run_backtest_sandbox(
    strategy_code: &str,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
    initial_cash: f64,
    config: &SandboxConfig,
) -> Result<SandboxResult, SandboxError> {
    let timeout = Duration::from_secs(config.timeout_secs);
    let start = Instant::now();

    let mut cmd = Command::new("python3");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("SANDBOX_MEMORY_LIMIT", config.memory_limit_bytes.to_string())
        .env("SANDBOX_CPU_LIMIT", config.cpu_limit_secs.to_string());

    let mut child = cmd.spawn().map_err(|e| {
        SandboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to spawn python3: {}", e),
        ))
    })?;

    let input = serde_json::json!({
        "code": strategy_code,
        "symbols": symbols,
        "start_date": start_date,
        "end_date": end_date,
        "initial_cash": initial_cash,
    });

    let stdin = child.stdin.take().ok_or_else(|| {
        SandboxError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "stdin not available",
        ))
    })?;

    let mut writer = stdin;
    serde_json::to_writer(&mut writer, &input).map_err(|e| SandboxError::Json(e))?;
    writer.write_all(b"\n").map_err(|e| SandboxError::Io(e))?;
    drop(writer);

    let stdout = child.stdout.take().ok_or_else(|| {
        SandboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "stdout not available",
        ))
    })?;

    let reader = BufReader::new(stdout);
    let result = parse_result_stream(reader)?;

    if start.elapsed() > timeout {
        child.kill().ok();
        return Err(SandboxError::Timeout(config.timeout_secs));
    }

    let status = child.wait().map_err(|e| SandboxError::Io(e))?;
    
    if !status.success() {
        warn!("Subprocess exited with status: {:?}", status);
    }

    Ok(result)
}

fn parse_result_stream<R: BufRead>(reader: R) -> Result<SandboxResult, SandboxError> {
    for line in reader.lines() {
        let line = line.map_err(|e| SandboxError::Io(e))?;
        if line.trim().is_empty() {
            continue;
        }
        let result: SandboxResult = serde_json::from_str(&line)
            .map_err(|e| {
                error!("Failed to parse result JSON: {}", e);
                SandboxError::Json(e)
            })?;
        return Ok(result);
    }
    Err(SandboxError::NoResult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::default();
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.memory_limit_bytes, 512 * 1024 * 1024);
        assert_eq!(config.cpu_limit_secs, 60);
    }

    #[test]
    fn test_sandbox_result_serialization() {
        let result = SandboxResult {
            status: "success".to_string(),
            result: Some(serde_json::json!({"test": 123})),
            error: None,
            traceback: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("test"));
    }
}
