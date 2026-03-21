use anyhow::Result;
use missiond_core::types::AsyncJobStatus;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("poll");

    match action {
        "poll" => poll_job(state, &args).await,
        "list" => list_jobs(state).await,
        "cancel" => cancel_job(state, &args).await,
        _ => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("Unknown action: {}", action),
            )
            .with_suggestion("Use: poll, list, cancel"),
        )),
    }
}

async fn poll_job(state: &AppState, args: &Value) -> Result<ToolResult> {
    let job_id = match args.get("job_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "'job_id' is required",
            )))
        }
    };

    let store = state.job_store.read().await;
    match store.get(job_id) {
        Some(job) => Ok(ToolResult::json_pretty(job)),
        None => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("Job '{}' not found", job_id),
            )
            .with_suggestion("Use action=list to see all jobs"),
        )),
    }
}

async fn list_jobs(state: &AppState) -> Result<ToolResult> {
    let store = state.job_store.read().await;

    let running: Vec<&missiond_core::types::AsyncJob> = store
        .values()
        .filter(|j| j.status == AsyncJobStatus::Running)
        .collect();
    let completed: Vec<&missiond_core::types::AsyncJob> = store
        .values()
        .filter(|j| j.status != AsyncJobStatus::Running)
        .collect();

    Ok(ToolResult::json_pretty(&json!({
        "running": running.len(),
        "completed": completed.len(),
        "jobs": store.values().collect::<Vec<_>>(),
    })))
}

async fn cancel_job(state: &AppState, args: &Value) -> Result<ToolResult> {
    let job_id = match args.get("job_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "'job_id' is required",
            )))
        }
    };

    let mut store = state.job_store.write().await;
    match store.get_mut(job_id) {
        Some(job) if job.status == AsyncJobStatus::Running => {
            job.fail("Cancelled by user".to_string());
            Ok(ToolResult::json_pretty(&json!({
                "job_id": job_id,
                "status": "cancelled",
            })))
        }
        Some(_) => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("Job '{}' already completed", job_id),
            )
            .with_suggestion("Completed jobs cannot be cancelled"),
        )),
        None => Ok(ToolResult::structured_error(ToolError::new(
            error_codes::NOT_FOUND,
            format!("Job '{}' not found", job_id),
        ))),
    }
}
