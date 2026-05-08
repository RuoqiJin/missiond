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
mod review;

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
use review::handle_kb_review;

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
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("gc")
                .to_string();
            if action == "compact" {
                return handle_kb_compact(state, args).await;
            }
            let (legacy, args) = route_kb_ops_to_legacy(&action, args);
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
        "mission_kb_review" => handle_kb_review(state, args).await,

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

/// Rewrite a `mission_kb_ops` envelope into the legacy direct-tool form.
///
/// The unified `mission_kb_ops` schema uses the outer `action` field to
/// select the family (gc/analyze/discover/queue_status/execute_plan), and an
/// inner `gc_action` field to select the gc verb (stats/stale/duplicates/...).
/// Legacy direct handlers (e.g. `mission_kb_gc`) read their verb from
/// `action`. When forwarding to gc we must substitute the outer `action` with
/// the inner `gc_action` value, otherwise the gc handler sees `action="gc"`
/// and returns "Unknown gc action: gc".
fn route_kb_ops_to_legacy(action: &str, args: Value) -> (&'static str, Value) {
    match action {
        "analyze" => ("mission_kb_analyze", args),
        "discover" => ("mission_kb_discover", args),
        "queue_status" => ("mission_kb_queue_status", args),
        "execute_plan" => ("mission_kb_execute_plan", args),
        _ => {
            let mut args = args;
            if let Some(gc_action) = args.get("gc_action").cloned() {
                if let Some(obj) = args.as_object_mut() {
                    obj.insert("action".to_string(), gc_action);
                }
            }
            ("mission_kb_gc", args)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kb_ops_routes_gc_stats_substitutes_action() {
        let args = json!({"action": "gc", "gc_action": "stats"});
        let (legacy, rewritten) = route_kb_ops_to_legacy("gc", args);
        assert_eq!(legacy, "mission_kb_gc");
        assert_eq!(rewritten["action"], json!("stats"));
        assert_eq!(rewritten["gc_action"], json!("stats"));
    }

    #[test]
    fn kb_ops_routes_gc_duplicates_substitutes_action() {
        let args = json!({"action": "gc", "gc_action": "duplicates"});
        let (legacy, rewritten) = route_kb_ops_to_legacy("gc", args);
        assert_eq!(legacy, "mission_kb_gc");
        assert_eq!(rewritten["action"], json!("duplicates"));
    }

    #[test]
    fn kb_ops_routes_gc_stale_with_days() {
        let args = json!({"action": "gc", "gc_action": "stale", "days": 60});
        let (legacy, rewritten) = route_kb_ops_to_legacy("gc", args);
        assert_eq!(legacy, "mission_kb_gc");
        assert_eq!(rewritten["action"], json!("stale"));
        assert_eq!(rewritten["days"], json!(60));
    }

    #[test]
    fn kb_ops_routes_gc_clean_actions_substitutes_action() {
        for verb in ["clean_stale", "clean_duplicates"] {
            let args = json!({"action": "gc", "gc_action": verb});
            let (legacy, rewritten) = route_kb_ops_to_legacy("gc", args);
            assert_eq!(legacy, "mission_kb_gc");
            assert_eq!(rewritten["action"], json!(verb));
        }
    }

    #[test]
    fn kb_ops_gc_without_gc_action_preserves_action_for_clear_error() {
        let args = json!({"action": "gc"});
        let (legacy, rewritten) = route_kb_ops_to_legacy("gc", args);
        assert_eq!(legacy, "mission_kb_gc");
        assert_eq!(rewritten["action"], json!("gc"));
    }

    #[test]
    fn kb_ops_routes_analyze_without_rewriting_args() {
        let args = json!({"action": "analyze", "mode": "overview", "limit": 10});
        let (legacy, rewritten) = route_kb_ops_to_legacy("analyze", args);
        assert_eq!(legacy, "mission_kb_analyze");
        assert_eq!(rewritten["action"], json!("analyze"));
        assert_eq!(rewritten["mode"], json!("overview"));
    }

    #[test]
    fn kb_ops_routes_discover_queue_status_execute_plan_intact() {
        let cases = [
            ("discover", "mission_kb_discover"),
            ("queue_status", "mission_kb_queue_status"),
            ("execute_plan", "mission_kb_execute_plan"),
        ];
        for (action, expected) in cases {
            let args = json!({"action": action});
            let (legacy, rewritten) = route_kb_ops_to_legacy(action, args);
            assert_eq!(legacy, expected);
            assert_eq!(rewritten["action"], json!(action));
        }
    }
}
