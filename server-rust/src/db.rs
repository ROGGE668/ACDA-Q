//! 数据库连接池

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

use crate::config::Settings;

pub type DbPool = PgPool;

pub async fn create_pool(settings: &Settings) -> Result<DbPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&settings.database_url)
        .await
}

pub async fn create_sync_pool(settings: &Settings) -> Result<DbPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&settings.sync_database_url)
        .await
}

pub async fn create_timescale_pool(settings: &Settings) -> Result<DbPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(15)
        .min_connections(3)
        .acquire_timeout(Duration::from_secs(5))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&settings.timescale_database_url)
        .await
}
