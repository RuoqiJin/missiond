use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::json;
use tracing::{info, error};

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, _name: &str, args: serde_json::Value) -> Result<ToolResult> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("run");

    match action {
        "list" => {
            let flows = crate::engine::flow::loader::list_flows()?;
            Ok(ToolResult::json_pretty(&json!({ "flows": flows, "count": flows.len() })))
        }
        "status" => {
            let task_id = args.get("task_id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("'task_id' required for status"))?;
            let task = state.store.get_board_task(task_id).await
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
            let ctx: Option<crate::engine::flow::FlowContext> = task.flow_context
                .as_ref().and_then(|s| serde_json::from_str(s).ok());
            Ok(ToolResult::json_pretty(&json!({
                "task_id": task_id,
                "flow_phase": task.flow_phase,
                "status": task.status.as_str(),
                "context": ctx,
            })))
        }
        "run" => {
            let flow_id = args.get("flow_id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("'flow_id' required"))?;

            let flow = crate::engine::flow::loader::load_flow(flow_id)?;

            // Create Board Task to track
            let input = missiond_core::types::CreateBoardTaskInput {
                title: format!("Flow: {}", flow.name),
                category: Some("flow".to_string()),
                description: Some(format!("Flow v2: '{}'", flow.id)),
                flow_template: Some(flow.id.clone()),
                ..Default::default()
            };
            let task = state.store.create_board_task(&input).await
                .map_err(|e| anyhow!("DB: {}", e))?;
            let task_id = task.id.to_string();

            // Init FlowContext with user params
            let mut ctx = crate::engine::flow::FlowContext::new();
            if let Some(params) = args.get("params").and_then(|v| v.as_object()) {
                for (k, v) in params {
                    ctx.set(k.clone(), v.as_str().unwrap_or(&v.to_string()));
                }
            }

            // Persist initial context
            let _ = state.store.update_board_task(&task_id, &missiond_core::types::UpdateBoardTaskInput {
                flow_phase: Some("running".to_string()),
                flow_context: Some(serde_json::to_string(&ctx).unwrap_or_default()),
                status: Some("running".to_string()),
                ..Default::default()
            }).await;

            // Execute flow inline (blocks MCP call until completion).
            // Background spawn deferred: requires resolving Send bounds across
            // the dispatch_tool → run_flow → execute_node → dispatch_tool chain.
            info!(flow_id = %flow.id, task_id = %task_id, "Flow: executing");
            let result = crate::engine::flow::runner::run_flow(state, &flow, &mut ctx, &task_id).await;

            match result {
                Ok(()) => {
                    let _ = state.store.update_board_task(&task_id, &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("completed".to_string()),
                        status: Some("done".to_string()),
                        ..Default::default()
                    }).await;
                    Ok(ToolResult::json_pretty(&json!({
                        "task_id": task_id,
                        "flow_id": flow_id,
                        "status": "completed",
                        "completed_nodes": ctx.completed_nodes,
                    })))
                }
                Err(e) => {
                    error!(task_id = %task_id, error = %e, "Flow: failed");
                    let _ = state.store.update_board_task(&task_id, &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("failed".to_string()),
                        status: Some("failed".to_string()),
                        ..Default::default()
                    }).await;
                    Err(e)
                }
            }
        }
        _ => Ok(ToolResult::error(format!("Unknown action: {}. Use: run, list, status", action))),
    }
}
