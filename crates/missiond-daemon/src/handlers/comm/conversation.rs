use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

mod events;
mod maintenance;
mod query;
mod router;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_conversation_query" => router::handle_conversation_query(state, args).await,
        "mission_conversation_analyze" => router::handle_conversation_analyze(state, args).await,
        "mission_retrospective_manage" => router::handle_retrospective_manage(state, args).await,
        "mission_token_stats"
        | "mission_conversation_list"
        | "mission_conversation_get"
        | "mission_conversation_search"
        | "mission_message_search"
        | "mission_user_message_index"
        | "mission_conversation_set_label"
        | "mission_conversation_delete_label"
        | "mission_context_around" => query::handle_query(state, name, args).await,
        "mission_conversation_events"
        | "mission_agent_trajectory"
        | "mission_conversation_message"
        | "mission_activity_report" => events::handle_events(state, name, args).await,
        "mission_trigger_backfill"
        | "mission_habit_scan"
        | "mission_embedding_stats"
        | "mission_embedding_ops"
        | "mission_conversation_reconcile"
        | "mission_conversation_classification_audit"
        | "mission_conversation_classification_backfill"
        | "mission_conversation_turn_backfill" => {
            maintenance::handle_maintenance(state, name, args).await
        }
        _ => Err(anyhow!("Unknown conversation tool: {name}")),
    }
}
