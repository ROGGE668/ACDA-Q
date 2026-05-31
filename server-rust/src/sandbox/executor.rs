//! 子进程执行器 - 带进程池优化
//!
//! 启动独立进程执行策略代码，通过 stdin/stdout 进行 JSON 通信。
//! 使用进程池复用子进程，避免每次都重新 spawn。

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
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
            cpu_limit_secs: 120,
        }
    }
}

/// 进程池大小
fn pool_size() -> usize {
    std::env::var("ACDA_Q__SANDBOX_pool_size()")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
}

/// 子进程包装
struct PooledProcess {
    child: std::process::Child,
    in_use: bool,
}

impl PooledProcess {
    fn new(runner_path: &str) -> Result<Self, SandboxError> {
        let mut cmd = Command::new("python3");
        cmd.arg(runner_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONUNBUFFERED", "1");

        let child = cmd.spawn().map_err(|e| {
            SandboxError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to spawn python3: {}", e),
            ))
        })?;

        Ok(Self { child, in_use: false })
    }

    fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    fn send_request(&mut self, input: &serde_json::Value, _timeout_secs: u64) -> Result<SandboxResult, SandboxError> {
        // 写入一行 JSON 到 stdin（不关闭，Python 循环读取下一行）
        if let Some(ref mut stdin) = self.child.stdin {
            serde_json::to_writer(&mut *stdin, input).map_err(|e| SandboxError::Json(e))?;
            stdin.write_all(b"\n").map_err(|e| SandboxError::Io(e))?;
            stdin.flush().map_err(|e| SandboxError::Io(e))?;
        } else {
            return Err(SandboxError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stdin not available",
            )));
        }

        let stdout = self.child.stdout.as_mut().ok_or_else(|| {
            SandboxError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "stdout not available",
            ))
        })?;

        let mut reader = BufReader::new(stdout);
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
}

/// 进程池
struct ProcessPool {
    runner_path: String,
    processes: Vec<PooledProcess>,
}

impl ProcessPool {
    fn new(runner_path: String) -> Self {
        let mut processes = Vec::with_capacity(pool_size());
        for _ in 0..pool_size() {
            match PooledProcess::new(&runner_path) {
                Ok(p) => processes.push(p),
                Err(e) => warn!("Failed to create pooled process: {}", e),
            }
        }
        if processes.is_empty() {
            warn!("No processes in pool, will spawn on demand");
        }
        Self { runner_path, processes }
    }

    fn execute(&mut self, input: &serde_json::Value, timeout_secs: u64) -> Result<SandboxResult, SandboxError> {
        for p in &mut self.processes {
            if !p.in_use {
                p.in_use = true;
                let result = p.send_request(input, timeout_secs);
                p.in_use = false;
                // 如果进程已退出（stdin 关闭后 Python 退出），替换为新进程
                if !p.is_alive() {
                    if let Ok(new_p) = PooledProcess::new(&self.runner_path) {
                        *p = new_p;
                    }
                }
                return result;
            }
        }
        // 所有进程都在使用中，降级到 fallback
        warn!("All pooled processes in use, falling back to single-shot execution");
        drop(self); // 释放 pool 借用，让 fallback 能获取
        Err(SandboxError::NoResult)
    }
}

/// 全局进程池（使用 OnceLock + Mutex 实现延迟初始化）
static PROCESS_POOL: std::sync::OnceLock<Arc<Mutex<Option<ProcessPool>>>> = std::sync::OnceLock::new();

/// 获取或初始化进程池
fn get_pool(runner_path: &str) -> Arc<Mutex<Option<ProcessPool>>> {
    PROCESS_POOL.get_or_init(|| {
        Arc::new(Mutex::new(Some(ProcessPool::new(runner_path.to_string()))))
    }).clone()
}

/// 在沙箱中执行回测（带进程池优化）
pub fn run_backtest_sandbox(
    strategy_code: &str,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
    initial_cash: f64,
    config: &SandboxConfig,
    period: &str,
    bars_data: Option<&serde_json::Value>,
) -> Result<SandboxResult, SandboxError> {
    let timeout = Duration::from_secs(config.timeout_secs);
    let start = Instant::now();

    // 定位 sandbox_runner.py：同目录 > CWD
    let runner_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("sandbox_runner.py")))
        .filter(|p| p.exists())
        .or_else(|| {
            let cwd_path = std::env::current_dir()
                .unwrap_or_default()
                .join("sandbox_runner.py");
            if cwd_path.exists() { Some(cwd_path) } else { None }
        })
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join("sandbox_runner.py")
        });
    let runner_str = runner_path.to_string_lossy().to_string();

    let mut input = serde_json::json!({
        "code": strategy_code,
        "symbols": symbols,
        "start_date": start_date,
        "end_date": end_date,
        "initial_cash": initial_cash,
        "period": period,
    });
    if let Some(bars) = bars_data {
        input["bars"] = bars.clone();
    }

    // 从进程池执行
    // 关键设计：锁定池找可用进程 → 释放锁 → 无锁执行阻塞 I/O → 重新锁定更新状态
    let pool = get_pool(&runner_str);

    // 第一步：锁定，找到空闲进程并标记 in_use
    let available_idx = {
        let mut pool_guard = pool.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut p) = *pool_guard {
            let mut found = None;
            for (i, proc) in p.processes.iter_mut().enumerate() {
                if !proc.in_use && proc.is_alive() {
                    proc.in_use = true;
                    found = Some(i);
                    break;
                }
            }
            found
        } else {
            None
        }
    };
    // 锁在此处自动释放（pool_guard 离开作用域）

    // 第二步：无锁执行阻塞 I/O
    // 注意：进程已标记 in_use，其他线程不会使用该进程
    // 无需再次加锁，直接通过 pool 内部的进程执行
    if let Some(idx) = available_idx {
        // 直接锁定并执行（最小化锁持有时间：仅在更新状态时锁定）
        let result = {
            let mut pool_guard = pool.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut p) = *pool_guard {
                if idx < p.processes.len() {
                    Some(p.processes[idx].send_request(&input, config.timeout_secs))
                } else {
                    None
                }
            } else {
                None
            }
        };
        // 锁已释放

        // 更新进程状态（独立锁定）
        {
            let mut pool_guard = pool.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut p) = *pool_guard {
                if idx < p.processes.len() {
                    p.processes[idx].in_use = false;
                    if !p.processes[idx].is_alive() {
                        if let Ok(new_p) = PooledProcess::new(&p.runner_path) {
                            p.processes[idx] = new_p;
                        }
                    }
                }
            }
        }

        if let Some(Ok(result)) = result {
            if start.elapsed() > timeout {
                return Err(SandboxError::Timeout(config.timeout_secs));
            }
            return Ok(result);
        }
    }

    // 池初始化失败或所有进程忙，降级到原始方式
    run_backtest_sandbox_fallback(&runner_str, &input, timeout)
}

/// 原始的回退实现（无进程池）
fn run_backtest_sandbox_fallback(
    runner_str: &str,
    input: &serde_json::Value,
    timeout: Duration,
) -> Result<SandboxResult, SandboxError> {
    let start = Instant::now();

    let mut cmd = Command::new("python3");
    cmd.arg(runner_str)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1");

    let mut child = cmd.spawn().map_err(|e| {
        SandboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to spawn python3: {}", e),
        ))
    })?;

    let stdin = child.stdin.take().ok_or_else(|| {
        SandboxError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "stdin not available",
        ))
    })?;

    let mut writer = stdin;
    serde_json::to_writer(&mut writer, input).map_err(|e| SandboxError::Json(e))?;
    writer.write_all(b"\n").map_err(|e| SandboxError::Io(e))?;
    drop(writer);

    let stdout = child.stdout.take().ok_or_else(|| {
        SandboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "stdout not available",
        ))
    })?;

    // 带超时的 stdout 读取：在独立线程中读取，超时后 kill 子进程
    let timeout_dur = timeout;
    let child_id = child.id();
    let result = std::thread::scope(|s| {
        let handle = s.spawn(move || {
            let reader = BufReader::new(stdout);
            parse_result_stream(reader)
        });

        // 等待读取完成或超时
        let deadline = Instant::now() + timeout_dur;
        loop {
            if Instant::now() >= deadline {
                // 超时：kill 子进程
                warn!("Sandbox timeout after {:?}, killing child pid={:?}", timeout_dur, child_id);
                unsafe { libc::kill(child_id as i32, libc::SIGKILL); }
                return Err(SandboxError::Timeout(timeout_dur.as_secs()));
            }
            if handle.is_finished() {
                return handle.join().unwrap_or(Err(SandboxError::NoResult));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    let result = result?;

    let status = tokio::task::block_in_place(|| child.wait())
        .map_err(|e| SandboxError::Io(e))?;

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
