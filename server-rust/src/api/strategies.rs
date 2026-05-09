use std::sync::Arc;
use axum::extract::{Json, Path, State};
use sqlx::query_as;
use uuid::Uuid;

use crate::api::AppState;
use crate::error::AppError;
use crate::middleware::auth::CurrentUser;
use crate::models::{Strategy, StrategyCreate, StrategyUpdate};

pub async fn list_strategies(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
) -> Result<Json<Vec<Strategy>>, AppError> {
    let strategies: Vec<Strategy> = sqlx::query_as(
        "SELECT * FROM strategies WHERE user_id = $1 ORDER BY updated_at DESC"
    )
    .bind(current_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(strategies))
}

pub async fn create_strategy(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Json(payload): Json<StrategyCreate>,
) -> Result<Json<Strategy>, AppError> {
    let strategy: Strategy = sqlx::query_as(
        "INSERT INTO strategies (user_id, name, description, type, code, params) VALUES ($1, $2, $3, $4, $5, $6) RETURNING *"
    )
    .bind(current_user.id)
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(&payload.strategy_type)
    .bind(payload.code.as_deref().unwrap_or(""))
    .bind(payload.params.unwrap_or(serde_json::json!({})))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(strategy))
}

pub async fn get_strategy(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Strategy>, AppError> {
    let strategy: Strategy = sqlx::query_as(
        "SELECT * FROM strategies WHERE id = $1 AND user_id = $2"
    )
    .bind(id)
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(strategy))
}

pub async fn update_strategy(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<StrategyUpdate>,
) -> Result<Json<Strategy>, AppError> {
    let strategy: Strategy = sqlx::query_as(
        "UPDATE strategies SET
            name = COALESCE($2, name),
            description = COALESCE($3, description),
            code = COALESCE($4, code),
            params = COALESCE($5, params),
            version = version + 1,
            updated_at = NOW()
         WHERE id = $1 AND user_id = $6 RETURNING *"
    )
    .bind(id)
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(&payload.code)
    .bind(&payload.params)
    .bind(current_user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(strategy))
}

pub async fn delete_strategy(
    State(state): State<Arc<AppState>>,
    current_user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query("DELETE FROM strategies WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(current_user.id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Strategy not found".to_string()));
    }

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

pub async fn validate_strategy(
    State(_state): State<Arc<AppState>>,
    Path(_id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    use crate::backtest::validator;

    let strategy_type = payload
        .get("strategy_type")
        .and_then(|v| v.as_str())
        .unwrap_or("buy_and_hold");
    let default_params = serde_json::json!({});
    let params = payload.get("params").unwrap_or(&default_params);
    let code = payload
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut all_errors: Vec<String> = Vec::new();

    // 1. 内置策略参数验证
    let param_result = validator::validate_builtin_params(strategy_type, params);
    if !param_result.valid {
        all_errors.extend(param_result.errors);
    }

    // 2. 自定义策略代码安全检查（如果提供了 code）
    let security_result = if !code.is_empty() {
        let r = validator::validate_custom_code(code);
        if !r.valid {
            all_errors.extend(r.errors.clone());
        }
        Some(r)
    } else {
        None
    };

    // 3. Smoke Test（仅对内置策略）
    let smoke_result = if all_errors.is_empty() {
        let r = validator::smoke_test(strategy_type, params);
        if !r.valid {
            all_errors.extend(r.errors.clone());
        }
        r
    } else {
        validator::ValidationResult { valid: false, errors: vec!["Skipped due to earlier errors".to_string()] }
    };

    Ok(Json(serde_json::json!({
        "valid": all_errors.is_empty(),
        "errors": all_errors,
        "checks": {
            "params": param_result.valid,
            "security": security_result.as_ref().map(|r| r.valid),
            "smoke_test": smoke_result.valid,
        }
    })))
}
