use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

mod auth;
mod decision;
mod incident;
mod llm_trace;
mod question_flow;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_question" => question_flow::handle_consolidated(state, args).await,
        "mission_question_create"
        | "mission_question_list"
        | "mission_question_get"
        | "mission_question_answer"
        | "mission_question_dismiss" => question_flow::handle_legacy(state, name, args).await,
        "mission_decision_stats" => decision::handle_stats(state, args).await,
        "mission_llm_trace" => llm_trace::handle(state, args).await,
        "mission_gemini_auth" => auth::handle(state, args).await,
        "mission_incident" => incident::handle_consolidated(state, args).await,
        n if n.starts_with("mission_incident_") => incident::handle_legacy(state, n, args).await,
        "mission_jarvis_logs"
        | "mission_jarvis_trace"
        | "mission_gemini_trace"
        | "mission_gemini_stats"
        | "mission_gemini_content"
        | "mission_gemini_watch" => llm_trace::handle_legacy(state, name, args).await,
        _ => Err(anyhow!("Unknown question tool: {name}")),
    }
}
