use super::*;

pub(super) async fn handle_claim(state: &AppState, args: Value) -> Result<ToolResult> {
    let task_id = args
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("taskId is required"))?;
    let executor_type = args
        .get("executorType")
        .and_then(|v| v.as_str())
        .unwrap_or("manual_session");
    // Use explicit executorId, fall back to MCP session ID, then default.
    let executor_id = args
        .get("executorId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(current_session_id)
        .unwrap_or_else(|| "claude-code-session".to_string());
    let executor_id = executor_id.as_str();
    match state
        .store
        .claim_board_task(task_id, executor_id, executor_type)
        .await
    {
        Ok(Some(task)) => {
            super::record_session_task_binding(state, task.id.as_str(), &task.title);
            let ev = BoardEvent::Claimed {
                task_id: task.id.to_string(),
                slot_id: executor_id.to_string(),
            };
            crate::engine::master_control::notify_board_event_direct(&ev);
            let _ = state.bus.publish_board(ev).await;
            Ok(ToolResult::json_pretty(&task))
        }
        Ok(None) => {
            // Check why it failed: task not found vs already claimed.
            match state.store.get_board_task(task_id).await {
                Ok(Some(existing)) => {
                    let msg = if let Some(ref claimer) = existing.claim_executor_id {
                        format!(
                            "Task already claimed by {} ({})",
                            claimer,
                            existing.claim_executor_type.as_deref().unwrap_or("unknown")
                        )
                    } else {
                        format!(
                            "Task cannot be claimed (status: {})",
                            existing.status.as_str()
                        )
                    };
                    Ok(ToolResult::error(msg))
                }
                _ => Ok(ToolResult::error("Task not found")),
            }
        }
        Err(e) => Ok(ToolResult::error(format!("DB error: {}", e))),
    }
}
