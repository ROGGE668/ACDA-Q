use std::sync::Arc;
use axum::extract::{Json, State};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::{create_access_token, create_refresh_token, decode_token, hash_password, verify_password};
use crate::error::AppError;
use crate::middleware::auth::CurrentUser;
use crate::models::{UserLogin, UserOut, UserRegister};
use crate::models::User;

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserRegister>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 密码强度校验
    if payload.password.len() < 8
        || !payload.password.chars().any(|c| c.is_uppercase())
        || !payload.password.chars().any(|c| c.is_lowercase())
        || !payload.password.chars().any(|c| c.is_ascii_digit())
    {
        return Err(AppError::BadRequest(
            "Password must be at least 8 chars, contain uppercase, lowercase and digit".to_string(),
        ));
    }

    // 检查邮箱是否已注册
    let existing: Option<User> = sqlx::query_as(
        "SELECT * FROM users WHERE email = $1"
    )
    .bind(&payload.email)
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::BadRequest("Email already registered".to_string()));
    }

    let password_hash = hash_password(&payload.password)?;
    let nickname = payload.nickname.unwrap_or_else(|| {
        payload.email.split('@').next().unwrap_or("user").to_string()
    });

    let user: User = sqlx::query_as(
        "INSERT INTO users (email, password_hash, nickname) VALUES ($1, $2, $3) RETURNING *"
    )
    .bind(&payload.email)
    .bind(&password_hash)
    .bind(&nickname)
    .fetch_one(&state.db)
    .await?;

    // Auto-create default free subscription for new users so quota checks always work
    sqlx::query(
        "INSERT INTO subscriptions (user_id, tier, status, max_devices, ai_quota_daily, backtest_quota_daily)
         VALUES ($1, 'free', 'active', 1, 5, 10)
         ON CONFLICT (user_id) DO NOTHING"
    )
    .bind(user.id)
    .execute(&state.db)
    .await
    .ok(); // NON-FATAL: missing subscription is handled by default quotas downstream

    let access_token = create_access_token(user.id, &state.settings)?;
    let (refresh_token, jti, family) = create_refresh_token(user.id, None, &state.settings)?;

    // 存储 refresh token
    sqlx::query(
        "INSERT INTO refresh_tokens (user_id, token_jti, family_id, expires_at) VALUES ($1, $2, $3, NOW() + INTERVAL '7 days')"
    )
    .bind(user.id)
    .bind(&jti)
    .bind(&family)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "access_token": access_token,
        "refresh_token": refresh_token,
    })))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserLogin>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user: Option<User> = sqlx::query_as(
        "SELECT * FROM users WHERE email = $1"
    )
    .bind(&payload.email)
    .fetch_optional(&state.db)
    .await?;

    let user = match user {
        Some(u) if verify_password(&payload.password, &u.password_hash)? => u,
        _ => return Err(AppError::Auth("Invalid credentials".to_string())),
    };

    let access_token = create_access_token(user.id, &state.settings)?;
    let (refresh_token, jti, family) = create_refresh_token(user.id, None, &state.settings)?;

    sqlx::query(
        "INSERT INTO refresh_tokens (user_id, token_jti, family_id, expires_at) VALUES ($1, $2, $3, NOW() + INTERVAL '7 days')"
    )
    .bind(user.id)
    .bind(&jti)
    .bind(&family)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "access_token": access_token,
        "refresh_token": refresh_token,
    })))
}

pub async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let refresh_token = payload.get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Auth("Missing refresh token".to_string()))?;

    let claims = decode_token(refresh_token, &state.settings)?;
    if claims.token_type != "refresh" {
        return Err(AppError::Auth("Invalid token type".to_string()));
    }

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Auth("Invalid user ID".to_string()))?;

    // Use FOR UPDATE to prevent TOCTOU race: two concurrent refresh requests with
    // the same token will now conflict on the row lock. The first one revokes the
    // token and succeeds; the second sees it revoked and gets an error.
    let tx_result = sqlx::query_as::<_, (bool,)>(
        "SELECT revoked FROM refresh_tokens WHERE token_jti = $1 FOR UPDATE",
    )
    .bind(&claims.jti)
    .fetch_optional(&state.db)
    .await;

    let (is_revoked,): (bool,) = match tx_result {
        Ok(Some((revoked,))) => (revoked,),
        Ok(None) => {
            // Token not found — revoke entire family as a precaution
            sqlx::query("UPDATE refresh_tokens SET revoked = true WHERE family_id = $1")
                .bind(&claims.family)
                .execute(&state.db)
                .await
                .ok();
            return Err(AppError::Auth("Refresh token has been revoked".to_string()));
        }
        Err(e) => {
            tracing::error!("Failed to fetch refresh token: {}", e);
            return Err(AppError::Internal("Token validation failed".to_string()));
        }
    };

    if is_revoked {
        sqlx::query("UPDATE refresh_tokens SET revoked = true WHERE family_id = $1")
            .bind(&claims.family)
            .execute(&state.db)
            .await
            .ok();
        return Err(AppError::Auth("Refresh token has been revoked".to_string()));
    }

    // Mark old token as revoked
    sqlx::query("UPDATE refresh_tokens SET revoked = true WHERE token_jti = $1")
        .bind(&claims.jti)
        .execute(&state.db)
        .await?;

    // Generate new token pair
    let access_token = create_access_token(user_id, &state.settings)?;
    let (new_refresh_token, new_jti, new_family) =
        create_refresh_token(user_id, Some(claims.family), &state.settings)?;

    sqlx::query(
        "INSERT INTO refresh_tokens (user_id, token_jti, family_id, expires_at) VALUES ($1, $2, $3, NOW() + INTERVAL '7 days')"
    )
    .bind(user_id)
    .bind(&new_jti)
    .bind(&new_family)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "access_token": access_token,
        "refresh_token": new_refresh_token,
    })))
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(token) = payload.get("refresh_token").and_then(|v| v.as_str()) {
        if let Ok(claims) = decode_token(token, &state.settings) {
            if let Some(family) = payload.get("family").and_then(|v| v.as_str()) {
                sqlx::query("UPDATE refresh_tokens SET revoked = true WHERE family_id = $1")
                    .bind(family)
                    .execute(&state.db)
                    .await?;
            } else {
                sqlx::query("UPDATE refresh_tokens SET revoked = true WHERE token_jti = $1")
                    .bind(&claims.jti)
                    .execute(&state.db)
                    .await?;
            }
        }
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn get_me(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
) -> Result<Json<UserOut>, AppError> {
    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(current_user.id)
        .fetch_one(&state.db)
        .await?;

    let sub: Option<crate::models::Subscription> = sqlx::query_as(
        "SELECT * FROM subscriptions WHERE user_id = $1"
    )
    .bind(current_user.id)
    .fetch_optional(&state.db)
    .await?;

    let ai_used_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_generations WHERE user_id = $1 AND created_at >= CURRENT_DATE"
    )
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    Ok(Json(UserOut {
        id: user.id,
        email: user.email,
        nickname: user.nickname,
        tier: sub.as_ref().map(|s| s.tier.clone()).unwrap_or(user.tier),
        is_admin: user.is_admin,
        quota_ai_daily: sub.as_ref().map(|s| s.ai_quota_daily).unwrap_or(user.quota_ai_daily),
        ai_used_today: ai_used_today as i32,
        created_at: user.created_at,
    }))
}
