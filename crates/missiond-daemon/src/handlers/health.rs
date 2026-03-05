use anyhow::Result;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Delegate to misc handler for now (health/incidents/power are in misc)
    crate::handlers::misc::handle(state, name, args).await
}
