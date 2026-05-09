//! Prometheus 指标收集

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use prometheus::{
    CounterVec, Encoder, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};
use std::sync::Arc;
use std::time::Instant;

use crate::api::AppState;

#[derive(Clone)]
pub struct Metrics {
    pub registry: Arc<Registry>,
    pub request_count: CounterVec,
    pub request_duration: HistogramVec,
    pub backtest_total: CounterVec,
    pub backtest_duration: HistogramVec,
    pub ai_generation_total: CounterVec,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let request_count = CounterVec::new(
            Opts::new("http_requests_total", "Total HTTP requests"),
            &["method", "endpoint", "status_code"],
        )
        .expect("metric can be created");

        let request_duration = HistogramVec::new(
            HistogramOpts::new("http_request_duration_seconds", "HTTP request duration"),
            &["method", "endpoint"],
        )
        .expect("metric can be created");

        let backtest_total = CounterVec::new(
            Opts::new("backtest_total", "Total backtests"),
            &["status", "scope"],
        )
        .expect("metric can be created");

        let backtest_duration = HistogramVec::new(
            HistogramOpts::new("backtest_duration_seconds", "Backtest execution duration"),
            &["scope"],
        )
        .expect("metric can be created");

        let ai_generation_total = CounterVec::new(
            Opts::new("ai_generation_total", "Total AI generations"),
            &["status"],
        )
        .expect("metric can be created");

        registry.register(Box::new(request_count.clone())).ok();
        registry.register(Box::new(request_duration.clone())).ok();
        registry.register(Box::new(backtest_total.clone())).ok();
        registry.register(Box::new(backtest_duration.clone())).ok();
        registry.register(Box::new(ai_generation_total.clone())).ok();

        Self {
            registry: Arc::new(registry),
            request_count,
            request_duration,
            backtest_total,
            backtest_duration,
            ai_generation_total,
        }
    }
}

/// HTTP 请求指标追踪中间件
pub async fn track_metrics(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    let response = next.run(req).await;

    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    state.metrics
        .request_count
        .with_label_values(&[&method, &path, &status])
        .inc();
    state.metrics
        .request_duration
        .with_label_values(&[&method, &path])
        .observe(duration);

    response
}

/// Prometheus metrics 导出端点
pub async fn metrics_handler(
    State(state): State<Arc<AppState>>,
) -> Response<Body> {
    let encoder = TextEncoder::new();
    let metric_families = state.metrics.registry.gather();
    let mut buffer = Vec::new();

    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        return Response::builder()
            .status(500)
            .body(Body::from(format!("Failed to encode metrics: {}", e)))
            .unwrap();
    }

    Response::builder()
        .header("Content-Type", encoder.format_type())
        .body(Body::from(buffer))
        .unwrap()
}
