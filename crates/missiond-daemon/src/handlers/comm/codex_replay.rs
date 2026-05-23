use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name != "mission_codex_replay" {
        return Ok(ToolResult::error(format!("Unknown tool: {}", name)));
    }

    match state.codex_replay.handle_action(args).await {
        Ok(value) => Ok(ToolResult::json_pretty(&value)),
        Err(err) => Ok(ToolResult::error(format!("codex replay error: {}", err))),
    }
}
