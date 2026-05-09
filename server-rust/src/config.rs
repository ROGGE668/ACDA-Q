//! 应用配置

use config::{Config, ConfigError, Environment, File};
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub app_name: String,
    pub debug: bool,
    pub host: String,
    pub port: u16,

    // Database
    pub database_url: String,
    pub sync_database_url: String,
    pub timescale_database_url: String,

    // Redis
    pub redis_url: String,

    // Security
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
    pub refresh_token_expire_days: i64,

    // CORS
    pub cors_origins: String,
    pub cookie_secure: bool,

    // AI
    pub deepseek_api_key: String,
    pub deepseek_base_url: String,
    pub deepseek_model: String,

    // Backtest
    pub backtest_commission: Decimal,
    pub backtest_slippage: Decimal,
    pub backtest_stamp_duty: Decimal,
    pub backtest_transfer_fee: Decimal,
    pub backtest_risk_free_rate: Decimal,

    // MinIO
    pub minio_endpoint: String,
    pub minio_access_key: String,
    pub minio_secret_key: String,

    // Data source
    pub tushare_token: String,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name("server-rust/.env").required(false))
            .add_source(Environment::with_prefix("ACDA_Q").separator("__"))
            .set_default("app_name", "ACDA-Q API")?
            .set_default("debug", false)?
            .set_default("host", "0.0.0.0")?
            .set_default("port", 8000)?
            .set_default("access_token_expire_minutes", 30)?
            .set_default("refresh_token_expire_days", 7)?
            .set_default("cors_origins", "*")?
            .set_default("cookie_secure", true)?
            .set_default("deepseek_base_url", "https://api.deepseek.com")?
            .set_default("deepseek_model", "deepseek-chat")?
            .set_default("backtest_commission", "0.0003")?
            .set_default("backtest_slippage", "0.001")?
            .set_default("backtest_stamp_duty", "0.0005")?
            .set_default("backtest_transfer_fee", "0.00001")?
            .set_default("backtest_risk_free_rate", "0.02")?
            .build()?;

        let settings: Settings = s.try_deserialize()?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.secret_key.len() < 32 {
            return Err(ConfigError::Message(
                "SECRET_KEY must be at least 32 characters long. \
                 Generate a secure key with: openssl rand -hex 32".to_string(),
            ));
        }
        if self.access_token_expire_minutes < 1 {
            return Err(ConfigError::Message(
                "ACCESS_TOKEN_EXPIRE_MINUTES must be at least 1".to_string(),
            ));
        }
        if self.database_url.is_empty() {
            return Err(ConfigError::Message(
                "DATABASE_URL must be set".to_string(),
            ));
        }
        if self.redis_url.is_empty() {
            return Err(ConfigError::Message(
                "REDIS_URL must be set".to_string(),
            ));
        }
        Ok(())
    }

    pub fn secret_key_bytes(&self) -> Vec<u8> {
        self.secret_key.as_bytes().to_vec()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new().expect("Failed to load default settings")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> Settings {
        Settings {
            app_name: "test".to_string(),
            debug: true,
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
    fn test_secret_key_validation() {
        let mut s = test_settings();
        s.secret_key = "short".to_string();
        assert!(s.validate().is_err());

        s.secret_key = "this-is-a-long-enough-secret-key-32!".to_string();
        assert!(s.validate().is_ok());
    }

    #[test]
    fn test_empty_database_url_fails() {
        let mut s = test_settings();
        s.database_url = "".to_string();
        assert!(s.validate().is_err());
    }
}
