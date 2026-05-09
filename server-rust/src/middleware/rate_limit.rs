//! 滑动窗口速率限制中间件
//!
//! 基于 Redis 实现分布式限流，支持按用户 ID 或 IP 限流。

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use redis::AsyncCommands;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

use crate::middleware::auth::CurrentUser;

/// 限流配置
#[derive(Clone)]
pub struct RateLimitConfig {
    pub max_requests: u64,
    pub window_seconds: u64,
    pub key_prefix: String,
}

impl RateLimitConfig {
    pub fn backtest() -> Self {
        Self {
            max_requests: 10,
            window_seconds: 60,
            key_prefix: "rl:backtest".to_string(),
        }
    }

    pub fn ai() -> Self {
        Self {
            max_requests: 5,
            window_seconds: 60,
            key_prefix: "rl:ai".to_string(),
        }
    }

    pub fn auth() -> Self {
        Self {
            max_requests: 5,
            window_seconds: 60,
            key_prefix: "rl:auth".to_string(),
        }
    }
}

/// 限流中间件共享状态
#[derive(Clone)]
pub struct RateLimitState {
    pub redis_url: String,
    pub config: RateLimitConfig,
}

/// 限流中间件
pub async fn rate_limit_middleware(
    State(state): State<Arc<RateLimitState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, RateLimitError> {
    let client = redis::Client::open(state.redis_url.as_str())
        .map_err(|e| {
            tracing::error!("Redis connection failed: {}", e);
            RateLimitError::Internal
        })?;

    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| {
            tracing::error!("Redis connection failed: {}", e);
            RateLimitError::Internal
        })?;

    // 优先从 extension 中的 CurrentUser 取 user_id，fallback 到 IP
    let key = request
        .extensions()
        .get::<CurrentUser>()
        .map(|u| u.id.to_string())
        .unwrap_or_else(|| addr.ip().to_string());

    let redis_key = format!("{}:{}", state.config.key_prefix, key);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as f64;
    let window_start = now - state.config.window_seconds as f64;

    let request_id = format!("{}-{}", now, uuid::Uuid::new_v4());

    let pipe_result: Result<(u64, u64), _> = redis::pipe()
        .zrembyscore(&redis_key, 0.0, window_start)
        .zcard(&redis_key)
        .query_async(&mut conn)
        .await;

    let current_count = match pipe_result {
        Ok((_, count)) => count,
        Err(e) => {
            tracing::error!("Redis query failed: {}", e);
            // Redis 故障时放行，避免服务完全不可用
            return Ok(next.run(request).await);
        }
    };

    if current_count >= state.config.max_requests {
        warn!(
            "Rate limit exceeded: key={}, count={}/{}",
            redis_key, current_count, state.config.max_requests
        );
        return Err(RateLimitError::RateLimited);
    }

    // 添加当前请求并设置 TTL
    let _: Result<(), _> = conn.zadd(&redis_key, &request_id, now).await;
    let _: Result<(), _> = conn.expire(&redis_key, state.config.window_seconds as i64 + 1).await;

    debug!(
        "Rate limit pass: key={}, count={}/{}",
        redis_key,
        current_count + 1,
        state.config.max_requests
    );

    Ok(next.run(request).await)
}

/// 限流错误
#[derive(Debug)]
pub enum RateLimitError {
    RateLimited,
    Internal,
}

impl axum::response::IntoResponse for RateLimitError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            RateLimitError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded. Please try again later.",
            ),
            RateLimitError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
            ),
        };

        let body = axum::Json(serde_json::json!({
            "error": message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}
