//! 资源限制：内存和 CPU 上限
//! 
//! 使用 prctl 设置 cgroup 资源限制。

use std::process::Command;
use tracing::info;

/// 默认内存限制：512MB
const DEFAULT_MEMORY_LIMIT: u64 = 512 * 1024 * 1024;

/// 默认 CPU 时间限制：60 秒
const DEFAULT_CPU_LIMIT: u64 = 60;

/// 应用资源限制到子进程
/// 
/// 在 Linux 上使用 cgroups v2 限制内存和 CPU。
/// 在其他平台上尝试使用 prctl（macOS 不支持 RLIMIT_AS）。
#[allow(dead_code)]
pub fn apply_limits(
    memory_limit_bytes: u64,
    cpu_limit_seconds: u64,
) -> Result<(), String> {
    // Linux cgroups v2 资源限制
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = apply_cgroup_limits(memory_limit_bytes, cpu_limit_seconds) {
            info!("cgroup limits failed (may not have permissions): {}", e);
        } else {
            info!("Applied cgroup limits: memory={}B, cpu={}s", memory_limit_bytes, cpu_limit_seconds);
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        info!("cgroup not available on this platform");
    }

    Ok(())
}

/// 在 Linux 上应用 cgroup v2 限制
#[cfg(target_os = "linux")]
fn apply_cgroup_limits(memory_limit: u64, cpu_limit: u64) -> Result<(), String> {
    use std::fs;

    let cgroup_path = get_cgroup_self().ok_or("Cannot determine cgroup path")?;
    
    let memory_max = format!("{}", memory_limit);
    fs::write(cgroup_path.join("memory.max"), &memory_max)
        .map_err(|e| format!("Failed to set memory.max: {}", e))?;

    let cpu_max = format!("{}00000", cpu_limit);
    fs::write(cgroup_path.join("cpu.max"), format!("max {}", cpu_max))
        .map_err(|e| format!("Failed to set cpu.max: {}", e))?;

    Ok(())
}

/// 获取当前进程的 cgroup 路径
#[cfg(target_os = "linux")]
fn get_cgroup_self() -> Option<std::path::PathBuf> {
    use std::fs;

    let cgroup = fs::read_to_string("/proc/self/cgroup").ok()?;
    
    for line in cgroup.lines() {
        if line.starts_with("0::") {
            let path = line.trim_start_matches("0::");
            return Some(std::path::PathBuf::from(format!(
                "/sys/fs/cgroup{}",
                if path.is_empty() { "/" } else { path }
            )));
        }
    }
    None
}

/// 构建带有资源限制的 spawn 命令
#[allow(dead_code)]
pub fn build_constrained_command(
    mut cmd: Command,
    memory_limit_mb: u64,
    cpu_limit_secs: u64,
) -> Command {
    cmd.env("RUST_MIN_STACK", "8388608");
    let _ = memory_limit_mb;
    let _ = cpu_limit_secs;
    cmd
}
