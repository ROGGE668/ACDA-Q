//! 安全沙箱执行器
//! 
//! 在独立子进程中执行策略代码，实现：
//! - 网络隔离（禁用 socket）
//! - 资源限制（内存、CPU）
//! - 超时自动 kill
//! - 进程间通信（JSON）

mod executor;
mod isolation;
mod resource;

pub use executor::{run_backtest_sandbox, SandboxConfig};
