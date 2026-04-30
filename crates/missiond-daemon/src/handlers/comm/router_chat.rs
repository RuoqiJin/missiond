use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

mod chat;
mod files;
mod manage;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_router_chat" => chat::handle_chat(state, args).await,
        "mission_router_chat_manage" => manage::handle_consolidated(state, args).await,
        "mission_router_chat_history"
        | "mission_router_chat_list"
        | "mission_router_chat_delete"
        | "mission_router_chat_clear"
        | "mission_router_chat_delete_message"
        | "mission_router_chat_restore"
        | "mission_router_chat_stats"
        | "mission_router_chat_compress" => manage::handle_legacy(state, name, args).await,
        _ => Err(anyhow!("Unknown router_chat tool: {name}")),
    }
}
