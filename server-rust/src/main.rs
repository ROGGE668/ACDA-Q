//! ACDA-Q API 服务入口

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    http::{HeaderValue, Method},
};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod auth;
mod backtest;
mod config;
mod crypto;
mod data;
mod db;
mod error;
mod metrics;
mod middleware;
mod models;
mod queue;
mod websocket;
mod ai;
mod sandbox;

use api::{create_router, AppState};
use backtest::worker::BacktestWorker;
use config::Settings;
use db::{create_pool, create_sync_pool, create_timescale_pool};
use metrics::Metrics;
use queue::{start_worker, Queue};
use websocket::WsManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // CLI: 加密敏感配置值 (acda-q-server --encrypt <plaintext>)
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && (args[1] == "--encrypt" || args[1] == "--encrypt-key") {
        let settings = crate::config::Settings::new()?;
        let encrypted = crate::crypto::encrypt(&args[2], &settings.secret_key)?;
        println!("{}", encrypted);
        return Ok(());
    }
    let is_worker = std::env::args().any(|a| a == "--worker");

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

async fn run_api_server() -> anyhow::Result<()> {
    let settings = Arc::new(Settings::new()?);
    info!("Configuration loaded: {}:{}", settings.host, settings.port);

    let db = create_pool(&settings).await?;
    let sync_db = create_sync_pool(&settings).await?;
    let ts_db = create_timescale_pool(&settings).await?;
    info!("Database pools initialized");

    match sqlx::migrate!("./migrations").run(&db).await {
        Ok(_) => info!("Database migrations applied successfully"),
        Err(e) => {
            if settings.debug {
                tracing::warn!("Migration skipped (debug mode): {}", e);
            } else {
                return Err(anyhow::anyhow!("Database migration failed: {}", e));
            }
        }
    }

    let queue = Arc::new(
        Queue::new(&settings.redis_url, "acda_q:backtest")
            .map_err(|e| anyhow::anyhow!("Failed to create queue: {}", e))?,
    );
    queue.init_consumer_group().await.ok();
    info!("Redis queue initialized");

    let ws_manager = WsManager::new(1024);
    info!("WebSocket manager initialized");

    let metrics = Arc::new(Metrics::new());
    info!("Metrics initialized");

    let cors = CorsLayer::new()
        .allow_origin(settings.cors_origins.parse::<HeaderValue>()?)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);
    // Note: allow_credentials removed - incompatible with allow_headers(*)

    let state = AppState {
        db,
        sync_db,
        ts_db,
        settings: settings.clone(),
        metrics,
        ws_manager,
        queue,
        chart_cache: Default::default(),
    };

    // 前端静态文件目录
    let static_dir = std::env::var("ACDA_Q__STATIC_DIR")
        .unwrap_or_else(|_| "/home/hong/ACDA-Q-RUST-v1/client/dist".to_string());
    let static_path = PathBuf::from(&static_dir);
    
    let spa_fallback = if static_path.join("index.html").exists() {
        info!("Serving static files from: {}", static_dir);
        ServeDir::new(&static_path)
            .fallback(ServeFile::new(static_path.join("index.html")))
    } else {
        info!("No static dir found at {}, running API-only mode", static_dir);
        ServeDir::new("/nonexistent").fallback(ServeFile::new("/nonexistent"))
    };

    let app = create_router(state)
        .fallback_service(spa_fallback)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", settings.host, settings.port).parse()?;
    info!("Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;

    Ok(())
}

async fn run_worker() -> anyhow::Result<()> {
    let settings = Arc::new(Settings::new()?);
    info!("Configuration loaded");

    let db = create_pool(&settings).await?;
    let ts_db = create_timescale_pool(&settings).await?;
    info!("Database pools initialized");

    let queue = Queue::new(&settings.redis_url, "acda_q:backtest")
        .map_err(|e| anyhow::anyhow!("Failed to create queue: {}", e))?;
    info!("Redis queue initialized");

    let worker = BacktestWorker::new(db, ts_db, queue.clone(), settings);
    info!("BacktestWorker initialized, starting worker loop...");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Received shutdown signal");
        let _ = shutdown_tx.send(true);
    });

    start_worker(queue, worker, shutdown_rx).await;

    info!("Worker stopped");
    Ok(())
}
