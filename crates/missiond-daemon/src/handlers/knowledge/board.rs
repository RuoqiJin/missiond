use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::decision_harvest::harvest_decisions_for_task;
use crate::lenient;
use crate::llm::gemini_client::current_session_id;
use crate::state::{AppState, SessionTaskBinding};
use missiond_core::event::events::{BoardEvent, SlotEvent};

mod claim;
mod create;
mod decompose;
mod delete;
mod events;
mod note;
mod query;
mod retry;
mod session;
mod update;

use events::{publish_board_created, publish_board_status_changed, publish_board_update};
use session::record_session_task_binding;

// @beacon: board
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Consolidated Query =====
        "mission_board_query"
        | "mission_board_list"
        | "mission_board_get"
        | "mission_board_search"
        | "mission_board_summary" => query::handle_query(state, name, args).await,
        "mission_board_create" => create::handle_create(state, args).await,
        // ===== Unified Update (single + batch, absorbs toggle) =====
        "mission_board_update" | "mission_board_batch_update" | "mission_board_toggle" => {
            update::handle_update(state, name, args).await
        }
        "mission_board_delete" => delete::handle_delete(state, args).await,
        "mission_board_claim" => claim::handle_claim(state, args).await,
        "mission_board_note_add" => note::handle_note_add(state, args).await,
        "mission_board_decompose" => decompose::handle_decompose(state, args).await,
        "mission_board_retry" => retry::handle_retry(state, args).await,
        _ => Err(anyhow!("Unknown board tool: {name}")),
    }
}
