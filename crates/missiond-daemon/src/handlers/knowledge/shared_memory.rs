use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_shared_memory" => match state.shared_memory.handle_action(&args).await {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                err.to_string(),
            ))),
        },
        "mission_context_slice" => match state.shared_memory.context_slice(&args).await {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                err.to_string(),
            ))),
        },
        "mission_claim_status" => match state.shared_memory.claim_status(&args).await {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                err.to_string(),
            ))),
        },
        _ => Ok(ToolResult::structured_error(ToolError::new(
            error_codes::UNKNOWN_TOOL,
            format!("Unknown shared-memory tool: {name}"),
        ))),
    }
}
