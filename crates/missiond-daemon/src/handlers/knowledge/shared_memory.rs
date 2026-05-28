use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;

use crate::state::AppState;

fn shared_memory_error(err: anyhow::Error) -> ToolError {
    if let Some(control) =
        err.downcast_ref::<crate::engine::shared_memory::StructuredControlError>()
    {
        let mut tool_error = ToolError::new(control.code, control.message.clone())
            .with_details(control.details.clone());
        if let Some(suggestion) = &control.suggestion {
            tool_error = tool_error.with_suggestion(suggestion.clone());
        }
        return tool_error;
    }
    ToolError::new(error_codes::INVALID_PARAM, err.to_string()).with_suggestion(
        "use mission_shared_memory(action=\"task_result_put\"|\"worker_settle\"|\"claim\") with typed fields; Board notes and PTY text are projections only",
    )
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_shared_memory" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("query")
                .trim();
            let result = match action {
                "task_result_put" | "put_task_result" => {
                    state.shared_memory.task_result_put_typed(&args).await
                }
                "worker_settle" | "completion_settle" | "settle_worker" => {
                    state.shared_memory.settle_worker_typed(args.clone()).await
                }
                "claim" => state.shared_memory.claim_typed(&args).await,
                "release" => state.shared_memory.release_typed(args.clone()).await,
                "heartbeat" => state.shared_memory.heartbeat_typed(args.clone()).await,
                "capability_check" | "check_capability" => {
                    state.shared_memory.capability_check_typed(&args).await
                }
                "job_event" | "record_job_event" => {
                    state.shared_memory.job_event_typed(args.clone()).await
                }
                _ => state.shared_memory.handle_action(&args).await,
            };
            match result {
                Ok(value) => Ok(ToolResult::json_pretty(&value)),
                Err(err) => Ok(ToolResult::structured_error(shared_memory_error(err))),
            }
        }
        "mission_context_slice" => match state.shared_memory.context_slice(&args).await {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(shared_memory_error(err))),
        },
        "mission_claim_status" => match state.shared_memory.claim_status(&args).await {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(shared_memory_error(err))),
        },
        _ => Ok(ToolResult::structured_error(ToolError::new(
            error_codes::UNKNOWN_TOOL,
            format!("Unknown shared-memory tool: {name}"),
        ))),
    }
}
