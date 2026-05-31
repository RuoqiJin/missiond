use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle_conversation_query(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    let mapped_name = match action {
        "list" => "mission_conversation_list",
        "get" => "mission_conversation_get",
        "search" => "mission_conversation_search",
        "message_search" => "mission_message_search",
        "analysis_context" => "mission_conversation_analysis_context",
        "context" => "mission_context_around",
        "events" => "mission_conversation_events",
        "user_index" => "mission_user_message_index",
        "set_label" => "mission_conversation_set_label",
        "delete_label" => "mission_conversation_delete_label",
        "audit_classification" => "mission_conversation_classification_audit",
        "backfill_classification" => "mission_conversation_classification_backfill",
        "audit_message_roles" => "mission_conversation_message_role_audit",
        "backfill_message_roles" => "mission_conversation_message_role_backfill",
        "turn_backfill" => "mission_conversation_turn_backfill",
        "label_audit" => "mission_conversation_label_audit",
        "label_backfill" => "mission_conversation_label_backfill",
        "gemini_reconcile" => "mission_conversation_gemini_reconcile",
        other => return Err(anyhow!("Unknown conversation action: {other}")),
    };
    // Forward with the same args; action is ignored by leaf handlers.
    Box::pin(super::handle(state, mapped_name, args)).await
}

pub(super) async fn handle_conversation_analyze(
    state: &AppState,
    args: Value,
) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("conversation_analyze action is required"))?;
    match action {
        "retrospective" => {
            super::super::retrospective::handle(state, "mission_retrospective", args).await
        }
        "trajectory" => super::events::handle_events(state, "mission_agent_trajectory", args).await,
        "activity" => super::events::handle_events(state, "mission_activity_report", args).await,
        "analysis_context" => {
            super::query::handle_query(state, "mission_conversation_analysis_context", args).await
        }
        other => Err(anyhow!("Unknown conversation_analyze action: {other}")),
    }
}

pub(super) async fn handle_retrospective_manage(
    state: &AppState,
    args: Value,
) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("retrospective_manage action is required"))?;
    let mapped_name = match action {
        "list" => "mission_retrospective_list",
        "backfill" => "mission_retrospective_backfill",
        other => return Err(anyhow!("Unknown retrospective_manage action: {other}")),
    };
    super::super::retrospective::handle(state, mapped_name, args).await
}
