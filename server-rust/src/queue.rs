//! Redis 轻量任务队列 — 替代 Celery
//!
//! 基于 Redis Streams 实现，支持任务分发、状态追踪、重试机制。

use redis::{AsyncCommands, Client, RedisResult};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info};
use uuid::Uuid;

/// 任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Success,
    Failed,
    Retrying,
}

/// 通用任务定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task<T> {
    pub id: String,
    pub task_type: String,
    pub payload: T,
    pub status: TaskStatus,
    pub retry_count: u32,
    pub max_retries: u32,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
}

/// 回测任务载荷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestPayload {
    pub job_id: Uuid,
    pub user_id: Uuid,
    pub code: String,
    pub symbols: Vec<String>,
    pub start_date: String,
    pub end_date: String,
    pub initial_cash: String, // rust_decimal::Decimal serialized as string
    pub params: serde_json::Value,
    pub scope: String,
}

/// 队列客户端
#[derive(Clone)]
pub struct Queue {
    client: Client,
    stream_key: String,
    consumer_group: String,
    consumer_name: String,
}

impl Queue {
    pub fn new(redis_url: &str, stream_key: &str) -> RedisResult<Self> {
        let client = Client::open(redis_url)?;
        Ok(Self {
            client,
            stream_key: stream_key.to_string(),
            consumer_group: "acda_q_workers".to_string(),
            consumer_name: format!("worker_{}", Uuid::new_v4()),
        })
    }

    /// 订阅指定 job 的进度频道（返回独立的 PubSub 连接）
    /// 每个 WebSocket 连接需要自己的订阅连接，不能跨连接共享。
    pub async fn subscribe_progress(&self, job_id: &str) -> RedisResult<redis::aio::PubSub> {
        let conn = self.client.get_async_connection().await?;
        let mut pubsub = conn.into_pubsub();
        let channel = format!("backtest:progress:{}", job_id);
        pubsub.subscribe(&channel).await?;
        info!("Subscribed to Redis channel: {}", channel);
        Ok(pubsub)
    }

    /// 初始化消费者组（幂等）
    pub async fn init_consumer_group(&self) -> RedisResult<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let result: RedisResult<()> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&self.stream_key)
            .arg(&self.consumer_group)
            .arg("$")
            .arg("MKSTREAM")
            .query_async(&mut conn)
            .await;

        if let Err(ref e) = result {
            if e.to_string().contains("Consumer Group name already exists") {
                return Ok(());
            }
        }
        result
    }

    /// 推送任务到队列
    pub async fn push_task<T: Serialize>(&self, task_type: &str, payload: &T) -> RedisResult<String> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let task_id = format!("{}-{}", task_type, Uuid::new_v4());

        let task = Task {
            id: task_id.clone(),
            task_type: task_type.to_string(),
            payload: payload,
            status: TaskStatus::Pending,
            retry_count: 0,
            max_retries: 3,
            created_at: chrono::Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            error_message: None,
        };

        let task_json = serde_json::to_string(&task).unwrap_or_default();

        let id: String = conn
            .xadd(&self.stream_key, "*", &[("task", task_json)])
            .await?;

        info!("Task pushed: {} -> {}", task_id, id);
        Ok(task_id)
    }

    /// 消费任务（阻塞读取）
    pub async fn consume_task<T: DeserializeOwned>(
        &self,
        _block_ms: usize,
    ) -> RedisResult<Option<(String, Task<T>)>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;

        // 使用 redis::cmd 手动构建 XREADGROUP 命令
        let result: redis::Value = redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(&self.consumer_group)
            .arg(&self.consumer_name)
            .arg("STREAMS")
            .arg(&self.stream_key)
            .arg(">")
            .query_async(&mut conn)
            .await?;

        // 解析 Redis 返回的 Value
        if let redis::Value::Bulk(streams) = result {
            for stream in streams {
                if let redis::Value::Bulk(entries) = stream {
                    for entry in entries {
                        if let redis::Value::Bulk(items) = entry {
                            if items.len() >= 2 {
                                let entry_id = match &items[0] {
                                    redis::Value::Data(v) => String::from_utf8_lossy(v).to_string(),
                                    _ => continue,
                                };
                                if let redis::Value::Bulk(fields) = &items[1] {
                                    for i in (0..fields.len()).step_by(2) {
                                        if i + 1 < fields.len() {
                                            let key = match &fields[i] {
                                                redis::Value::Data(v) => String::from_utf8_lossy(v),
                                                _ => continue,
                                            };
                                            if key == "task" {
                                                let val = match &fields[i + 1] {
                                                    redis::Value::Data(v) => String::from_utf8_lossy(v).to_string(),
                                                    _ => continue,
                                                };
                                                let task: Task<T> = serde_json::from_str(&val).unwrap_or_else(|_| Task {
                                                    id: entry_id.clone(),
                                                    task_type: "unknown".to_string(),
                                                    payload: serde_json::from_str("{}").unwrap_or_else(|_| panic!("Failed to parse default payload")),
                                                    status: TaskStatus::Failed,
                                                    retry_count: 0,
                                                    max_retries: 0,
                                                    created_at: chrono::Utc::now().to_rfc3339(),
                                                    started_at: None,
                                                    completed_at: None,
                                                    error_message: Some("Failed to deserialize task".to_string()),
                                                });
                                                return Ok(Some((entry_id, task)));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// 确认任务完成（ACK）
    pub async fn ack_task(&self, entry_id: &str) -> RedisResult<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let _: () = conn.xack(&self.stream_key, &self.consumer_group, &[entry_id]).await?;
        Ok(())
    }

    /// 更新任务状态（存入 Redis Hash）
    pub async fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        error: Option<String>,
    ) -> RedisResult<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("task:status:{}", task_id);

        let mut fields = vec![
            ("status", serde_json::to_string(&status).unwrap_or_default()),
            ("updated_at", chrono::Utc::now().to_rfc3339()),
        ];

        if let Some(err) = error {
            fields.push(("error", err));
        }

        let _: () = conn.hset_multiple(&key, &fields).await?;
        let _: () = conn.expire(&key, 7 * 24 * 3600).await?; // 7天过期
        Ok(())
    }

    /// 获取任务状态
    pub async fn get_task_status(&self, task_id: &str) -> RedisResult<Option<TaskStatus>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("task:status:{}", task_id);

        let status_json: Option<String> = conn.hget(&key, "status").await?;
        match status_json {
            Some(json) => Ok(serde_json::from_str(&json).ok()),
            None => Ok(None),
        }
    }

    /// 发布进度更新（Pub/Sub）
    ///
    /// 发布到 `backtest:progress:{job_id}` 频道，格式与 Python 的
    /// `publish_backtest_update_sync` 兼容。
    pub async fn publish_progress(
        &self,
        job_id: &str,
        progress: f64,
        message: &str,
        status: &str,
    ) -> RedisResult<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let channel = format!("backtest:progress:{}", job_id);
        let payload = serde_json::json!({
            "job_id": job_id,
            "status": status,
            "progress": (progress * 100.0).round() / 100.0,
            "message": message,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _: () = conn.publish(&channel, payload.to_string()).await?;
        Ok(())
    }

    /// 清理挂起的旧任务（死信处理）
    pub async fn reclaim_pending(&self, min_idle_ms: usize) -> RedisResult<Vec<(String, String)>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;

        // XPENDING 返回四元组数组: (id, consumer, idle_ms, deliveries)
        let result: Vec<(String, String, usize, usize)> = redis::cmd("XPENDING")
            .arg(&self.stream_key)
            .arg(&self.consumer_group)
            .arg("-")
            .arg("+")
            .arg(100)
            .query_async(&mut conn)
            .await?;

        let mut reclaimed = Vec::new();
        for (entry_id, _consumer, _idle, _deliveries) in result {
            let claimed: redis::Value = redis::cmd("XCLAIM")
                .arg(&self.stream_key)
                .arg(&self.consumer_group)
                .arg(&self.consumer_name)
                .arg(min_idle_ms)
                .arg(&entry_id)
                .query_async(&mut conn)
                .await?;

            if let redis::Value::Bulk(items) = claimed {
                for item in items {
                    if let redis::Value::Bulk(fields) = item {
                        for i in (0..fields.len()).step_by(2) {
                            if i + 1 < fields.len() {
                                let key = match &fields[i] {
                                    redis::Value::Data(v) => String::from_utf8_lossy(v),
                                    _ => continue,
                                };
                                if key == "task" {
                                    let val = match &fields[i + 1] {
                                        redis::Value::Data(v) => String::from_utf8_lossy(v).to_string(),
                                        _ => continue,
                                    };
                                    reclaimed.push((entry_id.clone(), val));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(reclaimed)
    }
}

/// 工作器 trait：用户实现业务逻辑
#[async_trait::async_trait]
pub trait Worker<T: Send + Sync + Serialize + DeserializeOwned> {
    async fn process(&self, task: &Task<T>) -> Result<(), String>;
}

/// 启动工作器循环
pub async fn start_worker<T, W>(
    queue: Queue,
    worker: W,
    shutdown: tokio::sync::watch::Receiver<bool>,
) where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
    W: Worker<T> + Send + Sync + 'static,
{
    queue.init_consumer_group().await.ok();

    let mut shutdown_rx = shutdown;
    let mut reclaim_interval = interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Worker shutting down gracefully");
                    break;
                }
            }
            _ = reclaim_interval.tick() => {
                match queue.reclaim_pending(300_000).await {
                    Ok(tasks) if !tasks.is_empty() => {
                        info!("Reclaimed {} pending tasks", tasks.len());
                    }
                    _ => {}
                }
            }
            result = queue.consume_task::<T>(5000) => {
                match result {
                    Ok(Some((entry_id, task))) => {
                        info!("Processing task: {}", task.id);

                        queue.update_task_status(&task.id, TaskStatus::Running, None).await.ok();

                        match worker.process(&task).await {
                            Ok(()) => {
                                queue.ack_task(&entry_id).await.ok();
                                queue.update_task_status(&task.id, TaskStatus::Success, None).await.ok();
                                info!("Task completed: {}", task.id);
                            }
                            Err(err) => {
                                error!("Task failed: {} - {}", task.id, err);
                                queue.update_task_status(&task.id, TaskStatus::Failed, Some(err)).await.ok();
                                // 不重试：让死信队列处理
                            }
                        }
                    }
                    Ok(None) => {
                        // 无任务，继续循环
                    }
                    Err(e) => {
                        error!("Queue consumption error: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}
