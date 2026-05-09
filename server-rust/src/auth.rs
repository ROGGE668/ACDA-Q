//! JWT 认证与密码哈希

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Settings;
use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,      // user_id
    pub exp: i64,         // 过期时间 (timestamp)
    pub iat: i64,         // 签发时间
    pub jti: String,      // token 唯一标识
    pub family: String,   // refresh token family
    pub token_type: String, // "access" or "refresh"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

/// 对密码进行 bcrypt 哈希
pub fn hash_password(password: &str) -> Result<String, AppError> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST)
        .map_err(|_| AppError::Internal("Password hashing failed".to_string()))
}

/// 验证密码
pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

/// 创建 Access Token
pub fn create_access_token(user_id: Uuid, settings: &Settings) -> Result<String, AppError> {
    let now = Utc::now();
    let exp = now + Duration::minutes(settings.access_token_expire_minutes);
    let claims = Claims {
        sub: user_id.to_string(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: Uuid::new_v4().to_string(),
        family: Uuid::new_v4().to_string(),
        token_type: "access".to_string(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&settings.secret_key_bytes()),
    )
    .map_err(|e| AppError::Auth(format!("Token creation failed: {}", e)))
}

/// 创建 Refresh Token
pub fn create_refresh_token(user_id: Uuid, family: Option<String>, settings: &Settings) -> Result<(String, String, String), AppError> {
    let now = Utc::now();
    let exp = now + Duration::days(settings.refresh_token_expire_days);
    let jti = Uuid::new_v4().to_string();
    let family = family.unwrap_or_else(|| Uuid::new_v4().to_string());

    let claims = Claims {
        sub: user_id.to_string(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: jti.clone(),
        family: family.clone(),
        token_type: "refresh".to_string(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&settings.secret_key_bytes()),
    )
    .map_err(|e| AppError::Auth(format!("Refresh token creation failed: {}", e)))?;

    Ok((token, jti, family))
}

/// 验证并解码 Token
pub fn decode_token(token: &str, settings: &Settings) -> Result<Claims, AppError> {
    let mut validation = Validation::default();
    validation.validate_exp = true;
    validation.validate_nbf = false;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(&settings.secret_key_bytes()),
        &validation,
    )
    .map_err(|_| AppError::Auth("Invalid or expired token".to_string()))?;

    Ok(token_data.claims)
}

/// 从请求头中提取 Bearer Token
pub fn extract_bearer_token(auth_header: &str) -> Option<&str> {
    auth_header.strip_prefix("Bearer ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn test_settings() -> Settings {
        Settings {
            app_name: "test".to_string(),
            debug: false,
            host: "0.0.0.0".to_string(),
            port: 8000,
            database_url: "postgres://test".to_string(),
            sync_database_url: "postgres://test".to_string(),
            timescale_database_url: "postgres://test".to_string(),
            redis_url: "redis://localhost".to_string(),
            secret_key: "test-secret-key-at-least-32-chars-long-123".to_string(),
            access_token_expire_minutes: 30,
            refresh_token_expire_days: 7,
            cors_origins: "*".to_string(),
            cookie_secure: false,
            deepseek_api_key: "".to_string(),
            deepseek_base_url: "https://api.deepseek.com".to_string(),
            deepseek_model: "deepseek-chat".to_string(),
            backtest_commission: Decimal::new(3, 4),
            backtest_slippage: Decimal::new(1, 3),
            backtest_stamp_duty: Decimal::new(5, 4),
            backtest_transfer_fee: Decimal::new(1, 5),
            backtest_risk_free_rate: Decimal::new(2, 2),
            minio_endpoint: "localhost:9000".to_string(),
            minio_access_key: "".to_string(),
            minio_secret_key: "".to_string(),
            tushare_token: "".to_string(),
        }
    }

    #[test]
    fn test_password_hash_and_verify() {
        let password = "TestPassword123";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn test_access_token_lifecycle() {
        let settings = test_settings();
        let user_id = Uuid::new_v4();
        let token = create_access_token(user_id, &settings).unwrap();
        let claims = decode_token(&token, &settings).unwrap();
        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.token_type, "access");
    }

    #[test]
    fn test_refresh_token_lifecycle() {
        let settings = test_settings();
        let user_id = Uuid::new_v4();
        let (token, jti, family) = create_refresh_token(user_id, None, &settings).unwrap();
        let claims = decode_token(&token, &settings).unwrap();
        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.jti, jti);
        assert_eq!(claims.family, family);
        assert_eq!(claims.token_type, "refresh");
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(
            extract_bearer_token("Bearer abc123"),
            Some("abc123")
        );
        assert_eq!(extract_bearer_token("Basic abc123"), None);
    }
}
