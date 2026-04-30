use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle_consolidated(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    match action {
        "test" => crate::handlers::misc::handle(state, "mission_incident_test", args).await,
        "list" => crate::handlers::misc::handle(state, "mission_incident_list", args).await,
        "get" => crate::handlers::misc::handle(state, "mission_incident_get", args).await,
        "remediate" => {
            crate::handlers::misc::handle(state, "mission_incident_remediate", args).await
        }
        "status" => crate::handlers::misc::handle(state, "mission_incident_status", args).await,
        "close" => crate::handlers::misc::handle(state, "mission_incident_close", args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}

pub(super) async fn handle_legacy(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_incident_test"
        | "mission_incident_list"
        | "mission_incident_get"
        | "mission_incident_remediate"
        | "mission_incident_status"
        | "mission_incident_close" => crate::handlers::misc::handle(state, name, args).await,
        _ => Ok(ToolResult::error(format!(
            "Unknown incident tool: {}",
            name
        ))),
    }
}
