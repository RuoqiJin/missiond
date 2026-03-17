use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::codex_cli::set_codex_disabled;
use crate::llm_gate::{self, LlmProvider};
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
    let gate_status = llm_gate::all_status();
    Ok(ToolResult::json_pretty(&serde_json::json!({
        "workers": workers,
        "llm_gates": gate_status.iter().map(|(provider, disabled)| {
            serde_json::json!({
                "model": provider.as_str(),
                "disabled": disabled,
                "status": if *disabled { "closed" } else { "open" },
            })
        }).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct ControlArgs {
    target: String,
    action: String,
}

fn worker_control(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: ControlArgs = serde_json::from_value(args)?;

    // ── LLM Gate targets ──
    if let Some(provider) = LlmProvider::from_str(&args.target) {
        return if provider == LlmProvider::Codex {
            codex_gate_control(state, &args.action)
        } else {
            llm_gate_control(provider, &args.action)
        };
    }

    let handle = match state.worker_registry.get(&args.target) {
        Some(h) => h,
        None => {
            let known: Vec<_> = state.worker_registry.list_all()
                .iter().map(|w| w.name.clone()).collect();
            return Ok(ToolResult::error(format!(
                "Worker '{}' not found. Known: {} | LLM gates: codex, gemini, sonnet",
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

/// Unified LLM gate control for gemini/sonnet.
fn llm_gate_control(provider: LlmProvider, action: &str) -> Result<ToolResult> {
    let name = provider.as_str();
    match action {
        "pause" | "disable" => {
            llm_gate::set_disabled(provider, true);
            Ok(ToolResult::text(format!(
                "⏸ {} 已关闸（持久化）。所有 {} API 调用将立即拒绝。\n\
                 恢复：mission_worker(action=\"control\", target=\"{}\", control_action=\"resume\")",
                name, name, name
            )))
        }
        "resume" | "enable" => {
            llm_gate::set_disabled(provider, false);
            Ok(ToolResult::text(format!("▶️ {} 闸口已开启，API 调用恢复正常。", name)))
        }
        "status" => {
            let disabled = llm_gate::is_disabled(provider);
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "model": name,
                "disabled": disabled,
                "status": if disabled { "closed" } else { "open" },
            })))
        }
        _ => Ok(ToolResult::error("Unknown action. Use 'pause'/'disable', 'resume'/'enable', or 'status'")),
    }
}

/// Codex gate control — also pauses/resumes dependent workers.
fn codex_gate_control(state: &AppState, action: &str) -> Result<ToolResult> {
    match action {
        "pause" | "disable" => {
            set_codex_disabled(true);
            if let Some(h) = state.worker_registry.get("vision_worker") {
                h.set_state(WorkerState::Paused);
            }
            if let Some(h) = state.worker_registry.get("step_narrator") {
                h.set_state(WorkerState::Paused);
            }
            Ok(ToolResult::text(
                "⏸ Codex/GPT-5.4 已关闸（持久化）。vision_worker 和 step_narrator 已暂停。\n\
                 恢复：mission_worker(action=\"control\", target=\"codex\", control_action=\"resume\")"
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
            Ok(ToolResult::text("▶️ Codex/GPT-5.4 闸口已开启。vision_worker 和 step_narrator 已恢复运行。"))
        }
        "status" => {
            let disabled = crate::codex_cli::is_codex_disabled();
            let vision_state = state.worker_registry.get("vision_worker")
                .map(|h| format!("{:?}", h.current_state()));
            let narrator_state = state.worker_registry.get("step_narrator")
                .map(|h| format!("{:?}", h.current_state()));
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "model": "codex",
                "disabled": disabled,
                "status": if disabled { "closed" } else { "open" },
                "vision_worker": vision_state,
                "step_narrator": narrator_state,
            })))
        }
        _ => Ok(ToolResult::error("Unknown action. Use 'pause'/'disable', 'resume'/'enable', or 'status'")),
    }
}
