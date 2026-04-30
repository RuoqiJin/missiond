use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle_stats(state: &AppState, args: Value) -> Result<ToolResult> {
    let hours = args.get("hours").and_then(|v| v.as_i64()).unwrap_or(24);
    let stats = state
        .store
        .decision_stats(hours)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&stats))
}
