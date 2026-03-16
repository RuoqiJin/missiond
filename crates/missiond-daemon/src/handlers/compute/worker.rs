use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::codex_cli::set_codex_disabled;
use crate::state::AppState;
use crate::workers::registry::WorkerState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_worker
    if name == "mission_worker" {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        return match action {
            "list" => list_workers(state),
            "control" => {
                // Remap control_action to action for the inner handler
                let mut inner_args = args.clone();
                if let Some(ca) = args.get("control_action").cloned() {
                    inner_args.as_object_mut().map(|m| m.insert("action".to_string(), ca));
                }
                worker_control(state, inner_args)
            }
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    match name {
        "mission_workers" => list_workers(state),
        "mission_worker_control" => worker_control(state, args),
        _ => Ok(ToolResult::error(format!("Unknown tool: {}", name))),
    }
}

fn list_workers(state: &AppState) -> Result<ToolResult> {
    let workers = state.worker_registry.list_all();
    Ok(ToolResult::json_pretty(&workers))
}

#[derive(Deserialize)]
struct ControlArgs {
    target: String,
    action: String,
}

fn worker_control(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: ControlArgs = serde_json::from_value(args)?;

    // Special target: "codex" — persistent kill switch for all Codex/GPT-5.4 usage
    if args.target == "codex" {
        return match args.action.as_str() {
            "pause" | "disable" => {
                set_codex_disabled(true);
                // Also pause the two workers that use Codex
                if let Some(h) = state.worker_registry.get("vision_worker") {
                    h.set_state(WorkerState::Paused);
                }
                if let Some(h) = state.worker_registry.get("step_narrator") {
                    h.set_state(WorkerState::Paused);
                }
                Ok(ToolResult::text(
                    "⏸ Codex/GPT-5.4 已完全禁用（持久化）。vision_worker 和 step_narrator 已暂停。\n\
                     恢复：mission_worker_control(target=\"codex\", action=\"resume\")"
                ))
            }
            "resume" | "enable" => {
                set_codex_disabled(false);
                if let Some(h) = state.worker_registry.get("vision_worker") {
                    h.set_state(WorkerState::Running);
                }
                if let Some(h) = state.worker_registry.get("step_narrator") {
                    h.set_state(WorkerState::Running);
                }
                Ok(ToolResult::text("▶️ Codex/GPT-5.4 已恢复。vision_worker 和 step_narrator 已恢复运行。"))
            }
            "status" => {
                let disabled = crate::codex_cli::is_codex_disabled();
                let vision_state = state.worker_registry.get("vision_worker")
                    .map(|h| format!("{:?}", h.current_state()));
                let narrator_state = state.worker_registry.get("step_narrator")
                    .map(|h| format!("{:?}", h.current_state()));
                Ok(ToolResult::json_pretty(&serde_json::json!({
                    "codex_disabled": disabled,
                    "vision_worker": vision_state,
                    "step_narrator": narrator_state,
                })))
            }
            _ => Ok(ToolResult::error("Unknown action. Use 'pause'/'disable', 'resume'/'enable', or 'status'")),
        };
    }

    let handle = match state.worker_registry.get(&args.target) {
        Some(h) => h,
        None => {
            let known: Vec<_> = state.worker_registry.list_all()
                .iter().map(|w| w.name.clone()).collect();
            return Ok(ToolResult::error(format!(
                "Worker '{}' not found. Known workers: {} (+ 'codex' for GPT-5.4 kill switch)",
                args.target, known.join(", ")
            )));
        }
    };

    match args.action.as_str() {
        "pause" => {
            handle.set_state(WorkerState::Paused);
            Ok(ToolResult::text(format!("Worker '{}' paused", args.target)))
        }
        "resume" => {
            handle.set_state(WorkerState::Running);
            Ok(ToolResult::text(format!("Worker '{}' resumed", args.target)))
        }
        _ => Ok(ToolResult::error(format!("Unknown action: '{}'. Use 'pause' or 'resume'", args.action))),
    }
}
