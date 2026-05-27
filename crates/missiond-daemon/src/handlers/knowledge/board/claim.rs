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
    let storage = state.storage_plane();
    match storage
        .ports
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
            let event_plane = state.event_plane();
            let _ = event_plane.bus.publish_board_event(ev).await;
            Ok(ToolResult::json_pretty(&task))
        }
        Ok(None) => {
            // Check why it failed: task not found vs already claimed.
            match storage.ports.get_board_task(task_id).await {
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
                    Ok(ToolResult::structured_error(
                        ToolError::new(error_codes::CLAIM_CONFLICT, msg)
                            .with_details(serde_json::json!({
                                "scope_kind": "board_task",
                                "scope_key": task_id,
                                "holder": existing.claim_executor_id,
                                "lease_expires_at": existing.lease_expires_at,
                                "status": existing.status.as_str()
                            }))
                            .with_suggestion(
                                "inspect the task claim, wait for lease expiry, release stale ownership, or choose a different task",
                            ),
                    ))
                }
                _ => Ok(ToolResult::structured_error(ToolError::new(
                    error_codes::NOT_FOUND,
                    "Task not found",
                ))),
            }
        }
        Err(e) => Ok(super::board_store_error("mission_board_claim", e)),
    }
}
