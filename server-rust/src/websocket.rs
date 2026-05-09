//! WebSocket 回测进度推送
//!
//! 基于 axum WebSocket + Redis Pub/Sub 实现。Worker 通过 Redis 发布进度，
//! 本模块订阅对应频道并将消息实时转发给客户端。

use axum::{
    extract::{ws::WebSocket, Path, State, WebSocketUpgrade},
    response::Response,
};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::api::AppState;

/// 进度消息（与 Python publish_backtest_update_sync 格式保持一致）
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressMessage {
    pub job_id: String,
    pub status: String,      // pending / running / success / failed
    pub progress: f64,       // 0.0 ~ 1.0
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// WebSocket 连接管理器（内存内广播，用于同进程多消费者）
#[derive(Clone)]
pub struct WsManager {
    pub tx: broadcast::Sender<ProgressMessage>,
}

impl WsManager {
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// 发布进度到内存广播（供同进程内的其他消费者使用）
    pub fn publish(&self, msg: ProgressMessage) {
        let _ = self.tx.send(msg);
    }
}

impl Default for WsManager {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// WebSocket 升级处理器
pub async fn ws_backtest_handler(
    ws: WebSocketUpgrade,
    Path(job_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, job_id, state))
}

/// 处理单个 WebSocket 连接
///
/// 流程：
/// 1. 发送连接成功确认
/// 2. 订阅 Redis Pub/Sub 频道 `backtest:progress:{job_id}`
/// 3. 循环：接收 Redis 消息 → 转发给 WebSocket 客户端
/// 4. 同时处理客户端关闭/Ping 消息
async fn handle_ws_socket(
    mut socket: WebSocket,
    job_id: String,
    state: Arc<AppState>,
) {
    info!("WebSocket connected for job: {}", job_id);

    // Send initial connection confirmation
    let init_msg = serde_json::json!({
        "type": "connected",
        "job_id": &job_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    if let Err(e) = socket
        .send(axum::extract::ws::Message::Text(init_msg.to_string()))
        .await
    {
        error!("Failed to send init message: {}", e);
        return;
    }

    // Subscribe to Redis Pub/Sub for this job's progress updates
    let mut pubsub = match state.queue.subscribe_progress(&job_id).await {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to subscribe to Redis progress channel for job {}: {}", job_id, e);
            let err_msg = serde_json::json!({
                "type": "error",
                "message": "Failed to subscribe to progress updates",
            });
            let _ = socket.send(axum::extract::ws::Message::Text(err_msg.to_string())).await;
            return;
        }
    };

    // Convert redis::aio::PubSub into a Stream of messages we can use in tokio::select!
    let mut redis_stream = pubsub.on_message();

    // Also keep a heartbeat interval for keep-alive
    let mut heartbeat_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            // Forward Redis Pub/Sub messages to the WebSocket client
            msg = redis_stream.next() => {
                match msg {
                    Some(redis_msg) => {
                        let payload: String = redis_msg.get_payload().unwrap_or_default();
                        // The payload is a JSON string from Queue::publish_progress
                        // Forward it directly to the client
                        if let Err(e) = socket
                            .send(axum::extract::ws::Message::Text(payload))
                            .await
                        {
                            error!("Failed to send progress to WebSocket for job {}: {}", job_id, e);
                            break;
                        }
                    }
                    None => {
                        info!("Redis Pub/Sub stream ended for job: {}", job_id);
                        break;
                    }
                }
            }

            // Handle WebSocket messages from the client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        info!("WebSocket closed by client for job: {}", job_id);
                        break;
                    }
                    Some(Ok(axum::extract::ws::Message::Ping(data))) => {
                        if let Err(e) = socket.send(axum::extract::ws::Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Some(Ok(_)) => {
                        // Ignore other messages from client
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error for job {}: {}", job_id, e);
                        break;
                    }
                    None => {
                        info!("WebSocket connection ended for job: {}", job_id);
                        break;
                    }
                }
            }

            // Heartbeat to keep connection alive (every 30s)
            _ = heartbeat_interval.tick() => {
                let heartbeat = serde_json::json!({
                    "type": "heartbeat",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                });
                if let Err(e) = socket
                    .send(axum::extract::ws::Message::Text(heartbeat.to_string()))
                    .await
                {
                    debug!("Heartbeat failed for job {}: {}", job_id, e);
                    break;
                }
            }
        }
    }

    // pubsub and redis_stream are dropped here when the function returns.
    // Redis will clean up the subscription automatically when the connection closes.
    info!("WebSocket disconnected for job: {}", job_id);
}

