use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle(state: &AppState, args: Value) -> Result<ToolResult> {
    crate::handlers::misc::handle(state, "mission_gemini_auth", args).await
}
