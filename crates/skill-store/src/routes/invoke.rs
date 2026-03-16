use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};

use crate::error::AppResult;
use crate::middleware::auth::AuthUser;
use crate::models::InvokeSkillRequest;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/skills/{skill_id}/invoke", post(invoke_skill))
}

async fn invoke_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(skill_id): Path<String>,
    Json(req): Json<InvokeSkillRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let result = state
        .executor
        .invoke(&auth.user_id, &skill_id, &req.inputs)
        .await?;

    Ok(Json(serde_json::json!({
        "content": result.content,
        "executionId": result.execution_id,
        "usage": {
            "inputTokens": result.input_tokens,
            "outputTokens": result.output_tokens,
        }
    })))
}
