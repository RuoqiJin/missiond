use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini_trace");
    match action {
        "gemini_trace" => crate::handlers::misc::handle(state, "mission_gemini_trace", args).await,
        "gemini_stats" => crate::handlers::misc::handle(state, "mission_gemini_stats", args).await,
        "gemini_watch" => {
            let mut args = args;
            if let Some(wa) = args.get("watch_action").cloned() {
                args.as_object_mut()
                    .map(|m| m.insert("action".to_string(), wa));
            }
            crate::handlers::misc::handle(state, "mission_gemini_watch", args).await
        }
        "gemini_auth" => crate::handlers::misc::handle(state, "mission_gemini_auth", args).await,
        "jarvis_logs" => crate::handlers::misc::handle(state, "mission_jarvis_logs", args).await,
        "jarvis_trace" => crate::handlers::misc::handle(state, "mission_jarvis_trace", args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}
