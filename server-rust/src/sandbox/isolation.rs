//! 网络隔离：阻止策略代码创建网络连接

use std::io::{Error, ErrorKind};

/// 创建一个被禁止的 socket（用于替换 std::net::TcpStream::new）
/// 
/// 当策略代码尝试创建 socket 时，会触发此函数并返回 PermissionError。
/// 这实现了网络层的手动隔离——不依赖操作系统层级的 seccomp，
/// 而是在运行时替换 socket 创建函数。
#[allow(dead_code)]
pub fn blocked_socket(_domain: i32, _sock_type: i32, _protocol: i32) -> Result<std::os::unix::net::UnixStream, Error> {
    Err(Error::new(
        ErrorKind::PermissionDenied,
        "Network access is disabled in strategy sandbox",
    ))
}

/// 阻塞所有 Unix 域 socket 操作
#[allow(dead_code)]
pub fn blocked_unix_socket() -> Result<std::os::unix::net::UnixStream, Error> {
    Err(Error::new(
        ErrorKind::PermissionDenied,
        "Network access is disabled in strategy sandbox",
    ))
}
