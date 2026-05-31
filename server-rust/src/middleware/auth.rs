//! JWT 认证中间件与 CurrentUser Extractor

use async_trait::async_trait;
use axum::{
    extract::{FromRequestParts, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::decode_token;
use crate::config::Settings;
use crate::db::DbPool;

/// 当前登录用户上下文
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    #[allow(dead_code)]
    pub email: String,
    pub is_admin: bool,
    #[allow(dead_code)]
    pub tier: String,
}

/// 认证中间件共享状态
#[derive(Clone)]
pub struct AuthState {
    pub db: DbPool,
    pub settings: Arc<Settings>,
}

/// 从 Cookie 中提取指定名称的值
fn extract_cookie(headers: &header::HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let kv = part.trim();
        if let Some(val) = kv.strip_prefix(name) {
            let val = val.strip_prefix('=').unwrap_or("").trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// 提取认证 token：优先 httpOnly cookie，其次 Authorization header
fn extract_token(headers: &header::HeaderMap) -> Option<String> {
    // 1. 优先从 httpOnly cookie 读取
    if let Some(token) = extract_cookie(headers, "acda_access") {
        return Some(token);
    }
    // 2. 降级到 Authorization header（Tauri 客户端使用）
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// 认证中间件：校验 JWT 并将 CurrentUser 注入请求扩展
///
/// 用法：
/// ```ignore
/// let auth_state = Arc::new(AuthState { db, settings });
/// router.route_layer(axum::middleware::from_fn_with_state(auth_state, require_auth))
/// ```
pub async fn require_auth(
    State(auth_state): State<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    let token = extract_token(request.headers()).ok_or(AuthError::MissingToken)?;

    let claims = decode_token(&token, &auth_state.settings).map_err(|_| AuthError::InvalidToken)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AuthError::InvalidToken)?;

    // 查询数据库获取完整用户信息（后续可缓存到 Redis）
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, is_admin, tier FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(&auth_state.db)
    .await
    .map_err(|e| {
        tracing::error!("Auth db query failed: {}", e);
        AuthError::Internal
    })?
    .ok_or(AuthError::InvalidToken)?;

    let current_user = CurrentUser {
        id: user.id,
        email: user.email,
        is_admin: user.is_admin,
        tier: user.tier,
    };

    request.extensions_mut().insert(current_user);
    Ok(next.run(request).await)
}

/// 管理员权限中间件：在 require_auth 之后挂载
///
/// 检查请求扩展中的 CurrentUser.is_admin
pub async fn require_admin(
    request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    let current_user = request
        .extensions()
        .get::<CurrentUser>()
        .ok_or(AuthError::Forbidden)?;

    if !current_user.is_admin {
        return Err(AuthError::Forbidden);
    }

    Ok(next.run(request).await)
}

// 内部查询结构体
#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    is_admin: bool,
    tier: String,
}

/// Axum Extractor：提取 CurrentUser
///
/// 当路由已挂载 `require_auth` 中间件时，直接从 extensions 取（零开销）。
/// 单独使用时，会重新解析 token（不推荐）。
#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // 优先从 extensions 取（已由 require_auth 中间件注入）
        if let Some(user) = parts.extensions.get::<CurrentUser>() {
            return Ok(user.clone());
        }

        // 未找到，说明该路由未挂载认证中间件
        Err(AuthError::MissingToken)
    }
}

/// 认证错误类型
#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken,
    Forbidden,
    Internal,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::MissingToken => (StatusCode::UNAUTHORIZED, "Authentication required"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid or expired token"),
            AuthError::Forbidden => (StatusCode::FORBIDDEN, "Access denied"),
            AuthError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };

        let body = Json(json!({
            "error": message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}
