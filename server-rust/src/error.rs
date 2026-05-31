//! 统一错误处理
//!
//! 生产环境内部错误不暴露细节，仅通过日志记录。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Database(sqlx::Error),
    Validation(String),
    Auth(String),
    NotFound(String),
    BadRequest(String),
    RateLimited,
    ServiceUnavailable(String),
    Internal(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Database(e) => write!(f, "Database error: {}", e),
            AppError::Validation(e) => write!(f, "Validation error: {}", e),
            AppError::Auth(e) => write!(f, "Auth error: {}", e),
            AppError::NotFound(e) => write!(f, "Not found: {}", e),
            AppError::BadRequest(e) => write!(f, "Bad request: {}", e),
            AppError::RateLimited => write!(f, "Rate limit exceeded"),
            AppError::ServiceUnavailable(e) => write!(f, "Service unavailable: {}", e),
            AppError::Internal(e) => write!(f, "Internal error: {}", e),
        }
    }
}

impl PartialEq for AppError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AppError::Database(a), AppError::Database(b)) => a.to_string() == b.to_string(),
            (AppError::Validation(a), AppError::Validation(b)) => a == b,
            (AppError::Auth(a), AppError::Auth(b)) => a == b,
            (AppError::NotFound(a), AppError::NotFound(b)) => a == b,
            (AppError::BadRequest(a), AppError::BadRequest(b)) => a == b,
            (AppError::RateLimited, AppError::RateLimited) => true,
            (AppError::ServiceUnavailable(a), AppError::ServiceUnavailable(b)) => a == b,
            (AppError::Internal(a), AppError::Internal(b)) => a == b,
            _ => false,
        }
    }
}

impl std::error::Error for AppError {}

/// 生产环境是否隐藏内部错误详情
const HIDE_INTERNAL_DETAILS: bool = true;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Database(_) => {
                tracing::error!("Database error: {}", self);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Auth(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded".to_string(),
            ),
            AppError::ServiceUnavailable(msg) => {
                tracing::warn!("Service unavailable: {}", msg);
                (StatusCode::SERVICE_UNAVAILABLE, "Service temporarily unavailable".to_string())
            }
            AppError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                if HIDE_INTERNAL_DETAILS {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, msg.clone())
                }
            }
        };

        let body = Json(json!({
            "error": message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => AppError::NotFound("Resource not found".to_string()),
            sqlx::Error::PoolClosed => AppError::ServiceUnavailable("Database pool closed".to_string()),
            sqlx::Error::PoolTimedOut => AppError::ServiceUnavailable("Database pool timed out".to_string()),
            _ => AppError::Database(err),
        }
    }
}

impl From<bcrypt::BcryptError> for AppError {
    fn from(_: bcrypt::BcryptError) -> Self {
        AppError::Internal("Password hashing failed".to_string())
    }
}

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(_: jsonwebtoken::errors::Error) -> Self {
        AppError::Auth("Invalid token".to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Internal(format!("HTTP client error: {}", err))
    }
}

impl From<rust_decimal::Error> for AppError {
    fn from(err: rust_decimal::Error) -> Self {
        AppError::Validation(format!("Decimal parse error: {}", err))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal(format!("IO error: {}", err))
    }
}
