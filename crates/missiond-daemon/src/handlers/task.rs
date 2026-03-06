use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::slot_env::capture_slot_session_uuid;
use crate::lenient;
use crate::slot_env::build_slot_tracking_env;
use missiond_core::PTYSpawnOptions;

#[derive(Deserialize)]
struct SubmitArgs {
    role: String,
    prompt: String,
    #[serde(rename = "slotId")]
    slot_id: Option<String>,
}

#[derive(Deserialize)]
struct AskArgs {
    role: String,
    question: String,
    #[serde(rename = "timeoutMs", default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct StatusArgs {
    #[serde(rename = "taskId")]
    task_id: String,
}

#[derive(Deserialize)]
struct CancelArgs {
    #[serde(rename = "taskId")]
    task_id: String,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Task operations =====
        "mission_submit" => {
            let SubmitArgs { role, prompt, slot_id: target_slot } = serde_json::from_value(args)?;
            let task_id = state.mission.submit(&role, &prompt)?;

            // If slotId specified, store it on the task for autopilot fallback
            if let Some(ref target) = target_slot {
                let _ = state.mission.db().update_task(
                    &task_id,
                    &missiond_core::types::TaskUpdate {
                        slot_id: Some(target.clone()),
                        ..Default::default()
                    },
                );
            }

            // Try immediate dispatch
            let mut dispatched_to: Option<String> = None;
            let slots = state.mission.list_slots();

            // Build candidate list: if slotId specified, only that slot; otherwise all matching role
            let candidates: Vec<&str> = if let Some(ref target) = target_slot {
                vec![target.as_str()]
            } else {
                slots.iter()
                    .filter(|s| s.config.role == role)
                    .map(|s| s.config.id.as_str())
                    .collect()
            };

            for candidate_id in &candidates {
                if let Some(status) = state.pty.get_status(candidate_id).await {
                    if status.state == missiond_core::pty::SessionState::Idle {
                        match state.pty.send_fire_and_forget(candidate_id, &prompt).await {
                            Ok(()) => {
                                let now = chrono::Utc::now().timestamp_millis();
                                let slot_session = state.mission.db().get_slot_session(candidate_id).ok().flatten();
                                let _ = state.mission.db().update_task(
                                    &task_id,
                                    &missiond_core::types::TaskUpdate {
                                        status: Some(missiond_core::types::TaskStatus::Running),
                                        slot_id: Some(candidate_id.to_string()),
                                        session_id: slot_session,
                                        started_at: Some(now),
                                        ..Default::default()
                                    },
                                );
                                dispatched_to = Some(candidate_id.to_string());
                                info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: dispatched to idle slot");
                                break;
                            }
                            Err(e) => {
                                warn!(task_id = %task_id, slot_id = %candidate_id, error = %e, "mission_submit: dispatch failed");
                            }
                        }
                    }
                }
            }

            // Phase 2: No idle slot — auto-spawn an exited/no-session slot then dispatch
            if dispatched_to.is_none() {
                for candidate_id in &candidates {
                    let status = state.pty.get_status(candidate_id).await;
                    let is_spawnable = match &status {
                        Some(s) => s.state == missiond_core::pty::SessionState::Exited,
                        None => true, // no session at all
                    };
                    if !is_spawnable { continue; }

                    // Find slot config for spawn
                    let slot = slots.iter().find(|s| s.config.id == *candidate_id);
                    let slot = match slot {
                        Some(s) => s,
                        None => continue,
                    };

                    let pty_slot = missiond_core::PTYSlot {
                        id: slot.config.id.clone(),
                        role: slot.config.role.clone(),
                        cwd: slot.config.cwd.as_deref().map(std::path::PathBuf::from),
                    };
                    let mcp_config = slot.config.mcp_config.clone().map(std::path::PathBuf::from);
                    let (extra_env, session_file) = build_slot_tracking_env(candidate_id, slot.config.env.as_ref()).await;

                    info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: auto-spawning exited slot");
                    match state.pty.spawn(&pty_slot, PTYSpawnOptions {
                        auto_restart: false,
                        wait_for_idle: true,
                        timeout_secs: Some(30),
                        mcp_config,
                        dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                        extra_env,
                    }).await {
                        Ok(_info) => {
                            capture_slot_session_uuid(state, candidate_id, &session_file).await;
                            match state.pty.send_fire_and_forget(candidate_id, &prompt).await {
                                Ok(()) => {
                                    let now = chrono::Utc::now().timestamp_millis();
                                    let slot_session = state.mission.db().get_slot_session(candidate_id).ok().flatten();
                                    let _ = state.mission.db().update_task(
                                        &task_id,
                                        &missiond_core::types::TaskUpdate {
                                            status: Some(missiond_core::types::TaskStatus::Running),
                                            slot_id: Some(candidate_id.to_string()),
                                            session_id: slot_session,
                                            started_at: Some(now),
                                            ..Default::default()
                                        },
                                    );
                                    dispatched_to = Some(candidate_id.to_string());
                                    info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: spawned + dispatched");
                                    break;
                                }
                                Err(e) => warn!(task_id = %task_id, slot_id = %candidate_id, error = %e, "mission_submit: send after spawn failed"),
                            }
                        }
                        Err(e) => warn!(task_id = %task_id, slot_id = %candidate_id, error = %e, "mission_submit: auto-spawn failed"),
                    }
                }
            }

            let mut result = serde_json::json!({ "taskId": task_id });
            if let Some(slot_id) = dispatched_to {
                result["dispatched"] = serde_json::json!(true);
                result["slotId"] = serde_json::json!(slot_id);
            } else {
                result["dispatched"] = serde_json::json!(false);
                result["hint"] = serde_json::json!("No idle slot found, task queued for autopilot dispatch");
                // Signal unified scheduler to dispatch immediately
                state.event_bus.publish_traced(
                    crate::event_bus::DaemonEvent::TaskCreated { task_id: task_id.clone() },
                    crate::event_bus::TraceContext {
                        trace_id: Some(task_id.clone()),
                        summary: Some("Submit task queued for autopilot".to_string()),
                        ..Default::default()
                    },
                );
            }
            Ok(ToolResult::json(&result))
        }
        "mission_ask" => {
            let AskArgs {
                role,
                question,
                timeout_ms,
            } = serde_json::from_value(args)?;
            let timeout_ms = timeout_ms.unwrap_or(120_000);
            let result = state.mission.ask_expert(&role, &question, timeout_ms).await?;
            Ok(ToolResult::text(result))
        }
        "mission_status" => {
            let StatusArgs { task_id } = serde_json::from_value(args)?;
            if let Some(task) = state.mission.get_status(&task_id) {
                Ok(ToolResult::json(&task))
            } else {
                Ok(ToolResult::error("Task not found"))
            }
        }
        "mission_cancel" => {
            let CancelArgs { task_id } = serde_json::from_value(args)?;
            let cancelled = state.mission.cancel(&task_id).await?;
            Ok(ToolResult::json(&serde_json::json!({ "cancelled": cancelled })))
        }
        "mission_task" => {
            #[derive(Deserialize)]
            struct Args {
                status: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
            }
            let args: Args = serde_json::from_value(args).unwrap_or(Args { status: None, limit: None });
            let limit = args.limit.unwrap_or(20);
            let tasks = if let Some(ref status_str) = args.status {
                if let Some(status) = missiond_core::types::TaskStatus::from_str(status_str) {
                    state.mission.db().get_tasks_by_status(status)?
                } else {
                    return Ok(ToolResult::error(format!("Invalid status: {}. Use: queued, running, done, failed", status_str)));
                }
            } else {
                state.mission.db().get_all_tasks(limit)?
            };
            Ok(ToolResult::json(&tasks))
        }

        "mission_task_ack" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let since = args_val.get("since").and_then(|v| v.as_i64());
            let tasks = state.mission.db().ack_completed_tasks(since)?;
            Ok(ToolResult::json(&tasks))
        }

        "mission_task_track" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args { task_id: String }
            let Args { task_id } = serde_json::from_value(args)?;

            // 1. Task status
            let task = state.mission.db().get_task(&task_id)
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;
            let mut result = serde_json::json!({
                "task": {
                    "id": task.id,
                    "role": task.role,
                    "status": format!("{:?}", task.status),
                    "slotId": task.slot_id,
                    "createdAt": task.created_at,
                    "startedAt": task.started_at,
                    "finishedAt": task.finished_at,
                    "result": task.result,
                    "error": task.error,
                }
            });

            // 2. Slot PTY status + progress + lastResponse (if assigned)
            if let Some(ref slot_id) = task.slot_id {
                if let Some(info) = state.pty.get_status(slot_id).await {
                    let mut slot_obj = serde_json::json!({
                        "state": format!("{:?}", info.state),
                        "statusText": info.status_text,
                    });
                    // Session & activity
                    if let Ok(Some(session_uuid)) = state.mission.db().get_slot_session(slot_id) {
                        slot_obj["sessionId"] = json!(session_uuid);
                        if let Ok(Some(conv)) = state.mission.db().get_conversation(&session_uuid) {
                            if let Some(ref jp) = conv.jsonl_path {
                                if let Ok(md) = std::fs::metadata(jp) {
                                    if let Ok(m) = md.modified() {
                                        slot_obj["lastActivitySecsAgo"] = json!(m.elapsed().unwrap_or_default().as_secs());
                                    }
                                }
                            }
                        }
                    }
                    // Progress
                    {
                        let progress = state.slot_progress.read().await;
                        if let Some(sp) = progress.get(slot_id) {
                            if sp.total_calls > 0 {
                                slot_obj["progress"] = serde_json::to_value(sp).unwrap_or_default();
                            }
                        }
                    }
                    // Last response
                    {
                        let responses = state.slot_last_responses.read().await;
                        if let Some(resp) = responses.get(slot_id) {
                            let truncated = if resp.len() > 2048 {
                                let mut end = 2048;
                                while end > 0 && !resp.is_char_boundary(end) { end -= 1; }
                                format!("{}...(truncated)", &resp[..end])
                            } else {
                                resp.clone()
                            };
                            slot_obj["lastResponse"] = json!(truncated);
                        }
                    }
                    result["slot"] = slot_obj;
                }
            }

            Ok(ToolResult::json_pretty(&result))
        }


        _ => Err(anyhow!("Unknown task tool: {name}")),
    }
}
