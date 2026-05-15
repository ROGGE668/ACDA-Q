//! HTTP API 路由层

use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;

use crate::db::DbPool;
use crate::metrics::{metrics_handler, Metrics};
use crate::middleware::auth::{require_admin, require_auth, AuthState};
use crate::middleware::rate_limit::{rate_limit_middleware, RateLimitConfig, RateLimitState};
use crate::config::Settings;
use crate::queue::Queue;
use crate::websocket::WsManager;

mod auth;
mod backtest;
mod strategies;
mod market;
mod subscription;
mod admin;
mod ai;
mod health;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    #[allow(dead_code)]
    pub sync_db: DbPool,
    pub ts_db: DbPool,
    pub settings: Arc<Settings>,
    pub metrics: Arc<Metrics>,
    #[allow(dead_code)]
    pub ws_manager: WsManager,
    pub queue: Arc<Queue>,
}

/// 构建所有路由
pub fn create_router(state: AppState) -> Router {
    let state = Arc::new(state);

    let auth_state = Arc::new(AuthState {
        db: state.db.clone(),
        settings: state.settings.clone(),
    });

    let backtest_rl_state = Arc::new(RateLimitState {
        redis_url: state.settings.redis_url.clone(),
        config: RateLimitConfig::backtest(),
    });
    let ai_rl_state = Arc::new(RateLimitState {
        redis_url: state.settings.redis_url.clone(),
        config: RateLimitConfig::ai(),
    });
    let auth_rl_state = Arc::new(RateLimitState {
        redis_url: state.settings.redis_url.clone(),
        config: RateLimitConfig::auth(),
    });

    let auth_public = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/refresh", post(auth::refresh))
        .route_layer(middleware::from_fn_with_state(auth_rl_state, rate_limit_middleware))
        .route("/logout", post(auth::logout));

    let auth_me = Router::new()
        .route("/me", get(auth::get_me))
        .route_layer(middleware::from_fn_with_state(auth_state.clone(), require_auth));

    let auth_routes = auth_public.merge(auth_me);

    let strategy_routes = Router::new()
        .route("/", get(strategies::list_strategies).post(strategies::create_strategy))
        .route("/:id", get(strategies::get_strategy).put(strategies::update_strategy).delete(strategies::delete_strategy))
        .route("/:id/validate", post(strategies::validate_strategy));

    let backtest_routes = Router::new()
        .route("/", get(backtest::list_backtests).post(backtest::submit_backtest))
        .route("/:job_id", get(backtest::get_backtest))
        .route("/:job_id/result", get(backtest::get_backtest_result))
        .route("/:job_id/chart", get(backtest::get_backtest_chart))
        .route("/:job_id/trades", get(backtest::get_backtest_trades))
        .route("/:job_id/ws", get(crate::websocket::ws_backtest_handler))
        .route_layer(middleware::from_fn_with_state(backtest_rl_state, rate_limit_middleware));

    let market_routes = Router::new()
        .route("/stocks", get(market::list_stocks))
        .route("/history/:symbol", get(market::get_history));

    let ai_routes = Router::new()
        .route("/generate", post(ai::generate_strategy))
        .route("/extract-params", post(ai::extract_params))
        .route_layer(middleware::from_fn_with_state(ai_rl_state, rate_limit_middleware));

    let subscription_routes = Router::new()
        .route("/subscription", get(subscription::get_subscription))
        .route("/devices/register", post(subscription::register_device))
        .route("/devices/heartbeat", post(subscription::device_heartbeat))
        .route("/devices", get(subscription::list_devices))
        .route("/devices/:id/revoke", post(subscription::revoke_device))
        .route("/payments", post(subscription::create_payment).get(subscription::get_payments))
        .route("/payments/:order_no", get(subscription::get_payment_status))
        .route("/payments/:order_no/cancel", post(subscription::cancel_payment));

    let admin_routes = Router::new()
        .route("/stats", get(admin::get_dashboard_stats))
        .route("/users", get(admin::list_users))
        .route("/users/:id/admin", put(admin::toggle_admin))
        .route("/users/:id", delete(admin::delete_user))
        .route("/devices", get(admin::list_all_devices))
        .route("/devices/:id/revoke", post(admin::revoke_device))
        .route("/subscriptions", get(admin::list_subscriptions))
        .route("/subscriptions/:id", put(admin::update_subscription))
        .route("/payments", get(admin::list_payments))
        .route("/payments/:id/status", put(admin::update_payment_status))
        .route("/backtests", get(admin::list_all_backtests))
        .route("/backtests/:id", delete(admin::delete_backtest_job))
        .route("/sync/stock-list", post(admin::sync_stock_list))
        .route("/sync/daily-bars", post(admin::sync_daily_bars))
        .layer(middleware::from_fn(require_admin));

    let protected = Router::new()
        .nest("/api/v1/strategies", strategy_routes)
        .nest("/api/v1/backtests", backtest_routes)
        .nest("/api/v1/ai", ai_routes)
        .nest("/api/v1/market", market_routes)
        .nest("/api/v1", subscription_routes)
        .nest("/api/v1/admin", admin_routes)
        .route_layer(middleware::from_fn_with_state(auth_state, require_auth));

    Router::new()
        .route("/health", get(health::health_handler))
        .route("/health/live", get(health::liveness_handler))
        .route("/health/ready", get(health::readiness_handler))
        .route("/metrics", get(metrics_handler))
        .nest("/api/v1/auth", auth_routes)
        .merge(protected)
        .with_state(state)
}
