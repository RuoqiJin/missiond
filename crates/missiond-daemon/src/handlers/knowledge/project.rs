use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

mod context;
mod reconcile;
mod registry;
mod survey;
mod universe;
mod vault;

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");

    match action {
        "list" => registry::handle_list(state).await,
        "get" => registry::handle_get(state, args).await,
        "status" => registry::handle_status(state, args).await,
        "resolve" => registry::handle_resolve(state, args).await,
        "set_active" => registry::handle_set_active(state, args).await,
        "sync" => registry::handle_sync(state).await,
        "init" => registry::handle_init(state, args).await,
        "context" => context::handle_context(state, args).await,
        "memories" => context::handle_memories(state, args).await,
        "universe" => universe::handle_universe(args).await,
        "reconcile" => reconcile::handle_reconcile(state, args).await,
        "survey" => survey::handle_survey(state, args).await,
        "vault_sync" => vault::handle_vault_sync(state, args).await,
        "import_universe" => registry::handle_import_universe(state, args).await,
        _ => Ok(ToolResult::error(format!(
            "Unknown project action: {}. Use: list, get, status, resolve, set_active, sync, init, context, memories, universe, reconcile, vault_sync, import_universe, survey",
            action
        ))),
    }
}
