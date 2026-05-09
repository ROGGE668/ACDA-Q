use std::sync::Arc;
use axum::extract::{Json, State};
use serde::Deserialize;

use crate::ai::deepseek::DeepSeekClient;
use crate::api::AppState;
use crate::error::AppError;
use crate::middleware::auth::CurrentUser;
use crate::models::AIGeneration;

#[derive(Deserialize)]
pub struct AIGenerateRequest {
    prompt: String,
    model: Option<String>,
}

pub async fn generate_strategy(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Json(payload): Json<AIGenerateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if state.settings.deepseek_api_key.is_empty() {
        return Err(AppError::Internal("AI service not configured".to_string()));
    }

    let client = DeepSeekClient::new(&state.settings.deepseek_api_key)
        .with_base_url(&state.settings.deepseek_base_url)
        .with_model(payload.model.as_deref().unwrap_or(&state.settings.deepseek_model));

    let (generated_code, tokens_used) = client.generate_strategy(&payload.prompt).await?;

    // 记录 AI 生成日志
    let _record: AIGeneration = sqlx::query_as(
        "INSERT INTO ai_generations (user_id, prompt, generated_code, model, tokens_used, status)
         VALUES ($1, $2, $3, $4, $5, 'success') RETURNING *"
    )
    .bind(current_user.id)
    .bind(&payload.prompt)
    .bind(&generated_code)
    .bind(payload.model.as_deref().unwrap_or(&state.settings.deepseek_model))
    .bind(tokens_used.map(|t| t as i32))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "generated_code": generated_code,
        "model": payload.model.unwrap_or_else(|| state.settings.deepseek_model.clone()),
        "tokens_used": tokens_used,
    })))
}

pub async fn extract_params(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let code = payload.get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if code.is_empty() {
        return Ok(Json(serde_json::json!({"params": []})));
    }

    // 使用 DeepSeekClient 的参数提取逻辑（纯本地，无需 API 调用）
    let client = DeepSeekClient::new("");
    let params = client.extract_params(code);

    Ok(Json(serde_json::json!({
        "params": params,
    })))
}
