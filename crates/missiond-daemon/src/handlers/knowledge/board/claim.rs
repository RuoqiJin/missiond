use super::*;
use crate::engine::control_plane_kernel::{ControlPlaneKernel, RequireCapabilityCommand};

pub(super) async fn handle_claim(state: &AppState, args: Value) -> Result<ToolResult> {
    let task_id = claim_string_arg(&args, &["taskId", "task_id"])
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
    let grant_id = claim_string_arg(
        &args,
        &[
            "grant_id",
            "grantId",
            "capability_grant_id",
            "capabilityGrantId",
        ],
    )
    .map(str::to_string);
    let subject_kind =
        claim_string_arg(&args, &["subject_kind", "subjectKind"]).unwrap_or_else(|| {
            if executor_type == "pty_slot" {
                "worker"
            } else {
                "operator"
            }
        });
    let subject_id = claim_string_arg(&args, &["subject_id", "subjectId"]).unwrap_or(executor_id);
    let operator_confirmed = claim_bool_arg(
        &args,
        &[
            "confirm",
            "operator_confirm",
            "operatorConfirm",
            "operator_confirmed",
            "operatorConfirmed",
        ],
    )
    .unwrap_or(false);
    if let Err(err) = ControlPlaneKernel::new(state)
        .require_capability_command(RequireCapabilityCommand {
            grant_id,
            subject_kind: subject_kind.to_string(),
            subject_id: subject_id.to_string(),
            operation: "claim".to_string(),
            scope_kind: "task".to_string(),
            scope_key: task_id.to_string(),
            task_id: Some(task_id.to_string()),
            allow_system_bypass: operator_confirmed,
            bypass_reason: operator_confirmed.then(|| {
                "operator confirmed mission_board_claim without subject-bound grant".to_string()
            }),
            details: serde_json::json!({
                "source": "mission_board_claim",
                "executor_id": executor_id,
                "executor_type": executor_type
            }),
        })
        .await
    {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::CAPABILITY_DENIED, err.to_string())
                .with_details(serde_json::json!({
                    "operation": "claim",
                    "scope_kind": "task",
                    "scope_key": task_id,
                    "subject_kind": subject_kind,
                    "subject_id": subject_id,
                    "grant_required": true
                }))
                .with_suggestion(
                    "pass an active claim:task capability grant for this subject, or use confirm=true only for an operator claim",
                ),
        ));
    }
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

fn claim_string_arg<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn claim_bool_arg(args: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_bool))
}
