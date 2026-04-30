use crate::state::AppState;
use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

mod analyze;
mod args;
mod beacon;
mod code_search;
mod compact;
mod conflicts;
mod discovery;
mod gc;
mod import;
mod mutate;
mod ops;
mod quality;
mod query;
mod remember;

use analyze::handle_kb_analyze;
use beacon::{
    handle_beacon_annotate, handle_beacon_list, handle_beacon_map, handle_beacon_tag,
    route_beacon_action,
};
use code_search::handle_code_search;
use compact::handle_kb_compact;
use discovery::handle_kb_discover;
use gc::handle_kb_gc;
use import::handle_kb_import;
use mutate::{
    handle_kb_batch_forget, handle_kb_batch_set_project, handle_kb_forget, handle_kb_update,
};
use ops::{handle_kb_execute_plan, handle_kb_queue_status};
use query::{handle_kb_get, handle_kb_list, handle_kb_search};
use remember::handle_kb_remember;

// @beacon: knowledge
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Merged tool dispatch: map unified tool names to legacy handler names
    let (name, args) = match name {
        "mission_kb_query" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("search");
            let legacy = match action {
                "get" => "mission_kb_get",
                "list" => "mission_kb_list",
                _ => "mission_kb_search",
            };
            (legacy, args)
        }
        "mission_kb_mutate" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("forget");
            let legacy = match action {
                "update" => "mission_kb_update",
                "import" => "mission_kb_import",
                "forget" if args.get("keys").is_some() => "mission_kb_batch_forget",
                _ => "mission_kb_forget",
            };
            (legacy, args)
        }
        "mission_kb_ops" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("gc");
            if action == "compact" {
                return handle_kb_compact(state, args).await;
            }
            let legacy = match action {
                "analyze" => "mission_kb_analyze",
                "discover" => "mission_kb_discover",
                "queue_status" => "mission_kb_queue_status",
                "execute_plan" => "mission_kb_execute_plan",
                _ => "mission_kb_gc",
            };
            (legacy, args)
        }
        "mission_beacon" => route_beacon_action(args),
        other => (other, args),
    };
    match name {
        // ===== Knowledge Base (Jarvis Memory) =====
        "mission_kb_remember" => handle_kb_remember(state, args).await,
        "mission_kb_forget" => handle_kb_forget(state, args).await,
        "mission_kb_batch_forget" => handle_kb_batch_forget(state, args).await,
        "mission_kb_batch_set_project" => handle_kb_batch_set_project(state, args).await,
        "mission_kb_update" => handle_kb_update(state, args).await,
        "mission_kb_search" => handle_kb_search(state, args).await,
        "mission_kb_get" => handle_kb_get(state, args).await,
        "mission_kb_list" => handle_kb_list(state, args).await,
        "mission_kb_import" => handle_kb_import(state, args).await,

        "mission_kb_discover" => handle_kb_discover(state, args).await,

        "mission_kb_gc" => handle_kb_gc(state, args).await,

        // ===== KB Analysis (via external AI) =====
        "mission_kb_analyze" => handle_kb_analyze(state, args).await,

        // ===== KB Operation Queue =====
        "mission_kb_queue_status" => handle_kb_queue_status(state, args).await,
        "mission_kb_execute_plan" => handle_kb_execute_plan(state, args).await,

        // ===== Holographic Beacon (P4) =====
        "mission_beacon_list" => handle_beacon_list(state).await,
        "mission_beacon_map" => handle_beacon_map(state, args).await,
        "mission_beacon_tag" => handle_beacon_tag(state, args).await,
        "mission_beacon_annotate" => handle_beacon_annotate(state, args).await,

        // ===== Code Context (P3.5) =====
        "mission_code_search" => handle_code_search(state, args).await,

        _ => Err(anyhow!("Unknown kb tool: {name}")),
    }
}
