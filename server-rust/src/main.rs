//! ACDA-Q API 服务入口
//!
//! 基于 axum + tokio + sqlx 构建的量化投资平台后端。
//!
//! 启动模式：
//!   ./acda-q-server          → API server（默认）
//!   ./acda-q-server --worker → 回测 Worker

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    http::{HeaderValue, Method},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod auth;
mod backtest;
mod config;
mod data;
mod db;
mod error;
mod metrics;
mod middleware;
mod models;
mod queue;
mod websocket;
mod ai;

use api::{create_router, AppState};
use backtest::worker::BacktestWorker;
use config::Settings;
use db::{create_pool, create_sync_pool, create_timescale_pool};
use metrics::Metrics;
use queue::Queue;
use websocket::WsManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 解析 CLI
    let is_worker = std::env::args().any(|a| a == "--worker");

    // 初始化日志
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "acda_q=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    if is_worker {
        info!("ACDA-Q Worker starting...");
        run_worker().await
    } else {
        info!("ACDA-Q API server starting...");
        run_api_server().await
    }
}

/// API Server 模式：提供 HTTP API + WebSocket
async fn run_api_server() -> anyhow::Result<()> {
    // 加载配置
    let settings = Arc::new(Settings::new()?);
    info!("Configuration loaded: {}:{}", settings.host, settings.port);

    // 初始化数据库连接池
    let db = create_pool(&settings).await?;
    let sync_db = create_sync_pool(&settings).await?;
    let ts_db = create_timescale_pool(&settings).await?;

    info!("Database pools initialized");

    // 运行数据库迁移
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Migration skipped or failed: {}", e);
        });

    // 初始化 Redis 队列
    let queue = Arc::new(
        Queue::new(&settings.redis_url, "acda_q:backtest")
            .map_err(|e| anyhow::anyhow!("Failed to create queue: {}", e))?,
    );
    queue.init_consumer_group().await.ok();
    info!("Redis queue initialized");

    // 初始化 WebSocket 管理器
    let ws_manager = WsManager::new(1024);
    info!("WebSocket manager initialized");

    // 初始化指标
    let metrics = Arc::new(Metrics::new());
    info!("Metrics initialized");

    // CORS 配置
    let cors = CorsLayer::new()
        .allow_origin(settings.cors_origins.parse::<HeaderValue>()?)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any)
        .allow_credentials(true);

    // 构建 AppState
    let state = AppState {
        db,
        sync_db,
        ts_db,
        settings: settings.clone(),
        metrics,
        ws_manager,
        queue,
    };

    // 构建路由
    let app = create_router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // 启动服务
    let addr: SocketAddr = format!("{}:{}", settings.host, settings.port).parse()?;
    info!("Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Worker 模式：消费 Redis Streams 回测任务
async fn run_worker() -> anyhow::Result<()> {
    // 加载配置
    let settings = Arc::new(Settings::new()?);
    info!("Configuration loaded");

    // 初始化数据库连接池（Worker 需要主库和时序库）
    let db = create_pool(&settings).await?;
    let ts_db = create_timescale_pool(&settings).await?;
    info!("Database pools initialized");

    // 初始化 Redis 队列
    let queue = Queue::new(&settings.redis_url, "acda_q:backtest")
        .map_err(|e| anyhow::anyhow!("Failed to create queue: {}", e))?;
    queue.init_consumer_group().await.ok();
    info!("Redis queue initialized");

    // 创建 BacktestWorker
    let worker = BacktestWorker::new(db, ts_db, queue.clone(), settings.clone());
    info!("BacktestWorker initialized");

    // 优雅退出信号
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to listen for ctrl+c: {}", e);
        }
        let _ = shutdown_tx_clone.send(true);
    });

    info!("Worker starting, consuming from Redis Streams...");

    // 启动 worker 循环
    queue::start_worker::<crate::queue::BacktestPayload, BacktestWorker>(
        queue,
        worker,
        shutdown_rx,
    )
    .await;

    info!("Worker shut down gracefully");
    Ok(())
}
