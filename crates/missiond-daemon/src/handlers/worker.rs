use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::workers::registry::WorkerState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
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

    let handle = match state.worker_registry.get(&args.target) {
        Some(h) => h,
        None => {
            let known: Vec<_> = state.worker_registry.list_all()
                .iter().map(|w| w.name.clone()).collect();
            return Ok(ToolResult::error(format!(
                "Worker '{}' not found. Known workers: {}",
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
