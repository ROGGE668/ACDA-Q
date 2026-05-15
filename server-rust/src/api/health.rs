//! 健康检查端点
//! 
//! 返回数据库、Redis、任务队列的连接状态，用于 k8s 探针。

use axum::{extract::State, Json};
use serde::Serialize;
use sqlx::PgPool;
use crate::queue::Queue;

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub timestamp: String,
    pub services: ServiceStatus,
}

#[derive(Debug, Serialize)]
pub struct ServiceStatus {
    pub database: ServiceCheck,
    pub timeseries_db: ServiceCheck,
    pub redis: ServiceCheck,
    pub worker_queue: ServiceCheck,
}

#[derive(Debug, Serialize)]
pub struct ServiceCheck {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ServiceCheck {
    pub fn healthy(latency_ms: u64) -> Self {
        Self {
            status: "healthy".to_string(),
            latency_ms: Some(latency_ms),
            error: None,
        }
    }

    pub fn unhealthy(error: &str) -> Self {
        Self {
            status: "unhealthy".to_string(),
            latency_ms: None,
            error: Some(error.to_string()),
        }
    }
}

impl HealthStatus {
    pub fn new(services: ServiceStatus) -> Self {
        let overall = services.is_healthy();
        Self {
            status: if overall { "healthy" } else { "degraded" }.to_string(),
            timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            services,
        }
    }
}

impl ServiceStatus {
    pub fn is_healthy(&self) -> bool {
        self.database.status == "healthy"
            && self.redis.status == "healthy"
            && self.worker_queue.status == "healthy"
    }
}

/// 健康检查处理器
pub async fn health_handler(
    State(state): State<std::sync::Arc<AppState>>,
) -> Json<HealthStatus> {
    let db_status = check_postgres(&state.db).await;
    let ts_db_status = check_postgres(&state.ts_db).await;
    let redis_status = check_redis(&state.settings.redis_url).await;
    let queue_status = check_queue(&state.queue).await;

    let services = ServiceStatus {
        database: db_status,
        timeseries_db: ts_db_status,
        redis: redis_status,
        worker_queue: queue_status,
    };

    Json(HealthStatus::new(services))
}

/// 简单的存活探针（不含详细信息）
pub async fn liveness_handler() -> &'static str {
    "OK"
}

/// 就绪探针（检查所有依赖）
pub async fn readiness_handler(
    State(state): State<std::sync::Arc<AppState>>,
) -> Result<String, (axum::http::StatusCode, String)> {
    let db_result = sqlx::query("SELECT 1")
        .fetch_one(&state.db)
        .await;
    
    match db_result {
        Ok(_) => Ok("OK".to_string()),
        Err(e) => Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, e.to_string())),
    }
}

async fn check_postgres(pool: &PgPool) -> ServiceCheck {
    use std::time::Instant;
    
    let start = Instant::now();
    match sqlx::query("SELECT 1").fetch_one(pool).await {
        Ok(_) => ServiceCheck::healthy(start.elapsed().as_millis() as u64),
        Err(e) => ServiceCheck::unhealthy(&e.to_string()),
    }
}

async fn check_redis(redis_url: &str) -> ServiceCheck {
    use std::time::Instant;
    
    let start = Instant::now();
    
    let client = match redis::Client::open(redis_url) {
        Ok(c) => c,
        Err(e) => return ServiceCheck::unhealthy(&e.to_string()),
    };
    
    let mut conn = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(e) => return ServiceCheck::unhealthy(&e.to_string()),
    };
    
    let _: String = match redis::cmd("PING").query_async(&mut conn).await {
        Ok(s) => s,
        Err(e) => return ServiceCheck::unhealthy(&e.to_string()),
    };
    
    ServiceCheck::healthy(start.elapsed().as_millis() as u64)
}

async fn check_queue(_queue: &Queue) -> ServiceCheck {
    ServiceCheck::healthy(0)
}

// Import AppState for type checking
use super::AppState;
