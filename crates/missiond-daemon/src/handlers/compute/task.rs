use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::lenient;
use crate::slot_env::build_slot_tracking_env;
use crate::slot_env::capture_slot_session_uuid;
use crate::state::AppState;
use missiond_core::PTYSpawnOptions;

#[derive(Deserialize)]
struct SubmitArgs {
    role: String,
    prompt: Option<String>,
    question: Option<String>,
    #[serde(rename = "slotId")]
    slot_id: Option<String>,
}

#[derive(Deserialize)]
struct AskArgs {
    role: String,
    question: String,
    #[serde(rename = "timeoutMs", default)]
    #[allow(dead_code)]
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
    // Consolidated tool dispatch
    match name {
        "mission_task_submit" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("async");
            match action {
                "async" => handle_submit(state, args).await,
                "sync" => handle_ask(state, args).await,
                _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
            }
        }
        "mission_task_query" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("list");
            match action {
                "status" => handle_status(state, args).await,
                "list" => handle_task_list(state, args).await,
                "ack" => handle_task_ack(state, args).await,
                "track" => handle_task_track(state, args).await,
                _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
            }
        }
        "mission_task_cancel" => handle_cancel(state, args).await,
        // Legacy names
        "mission_submit" => handle_submit(state, args).await,
        "mission_ask" => handle_ask(state, args).await,
        "mission_status" => handle_status(state, args).await,
        "mission_cancel" => handle_cancel(state, args).await,
        "mission_task" => handle_task_list(state, args).await,
        "mission_task_ack" => handle_task_ack(state, args).await,
        "mission_task_track" => handle_task_track(state, args).await,
        _ => Err(anyhow!("Unknown task tool: {name}")),
    }
}

async fn handle_submit(state: &AppState, args: Value) -> Result<ToolResult> {
    let submit_args: SubmitArgs = serde_json::from_value(args)?;
    let role = submit_args.role;
    let prompt = submit_args
        .prompt
        .or(submit_args.question)
        .ok_or_else(|| anyhow!("prompt or question is required"))?;
    let target_slot = submit_args.slot_id;

    let task_id = crate::state::submit_task(state.store.as_ref(), &role, &prompt).await?;

    // If slotId specified, store it on the task for autopilot fallback
    if let Some(ref target) = target_slot {
        let _ = state
            .store
            .update_task(
                &task_id,
                &missiond_core::types::TaskUpdate {
                    slot_id: Some(target.clone()),
                    ..Default::default()
                },
            )
            .await;
    }

    // Try immediate dispatch
    let mut dispatched_to: Option<String> = None;
    let slots = state.mission.list_slots();

    // Build candidate list: if slotId specified, only that slot; otherwise all matching role
    let candidates: Vec<&str> = if let Some(ref target) = target_slot {
        vec![target.as_str()]
    } else {
        slots
            .iter()
            .filter(|s| s.config.role == role)
            .map(|s| s.config.id.as_str())
            .collect()
    };

    for candidate_id in &candidates {
        // Acquire per-slot dispatch guard to prevent concurrent sends
        if !state.slot_dispatch.try_acquire(candidate_id) {
            continue; // another caller is dispatching to this slot
        }
        let sent = if let Some(status) = state.pty.get_status(candidate_id).await {
            if status.state == missiond_core::pty::SessionState::Idle {
                state
                    .pty
                    .send_fire_and_forget(candidate_id, &prompt)
                    .await
                    .ok()
                    .is_some()
            } else {
                false
            }
        } else {
            false
        };
        state.slot_dispatch.release(candidate_id);

        if sent {
            let now = chrono::Utc::now().timestamp_millis();
            let slot_session = state
                .store
                .get_slot_session(candidate_id)
                .await
                .ok()
                .flatten();
            let _ = state
                .store
                .update_task(
                    &task_id,
                    &missiond_core::types::TaskUpdate {
                        status: Some(missiond_core::types::TaskStatus::Running),
                        slot_id: Some(candidate_id.to_string()),
                        session_id: slot_session,
                        started_at: Some(now),
                        ..Default::default()
                    },
                )
                .await;
            let preview = if prompt.len() > 200 {
                let mut end = 200;
                while end > 0 && !prompt.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &prompt[..end])
            } else {
                prompt.clone()
            };
            state
                .event_bus
                .publish(crate::event_bus::DaemonEvent::SlotTaskDispatched {
                    slot_id: candidate_id.to_string(),
                    task_id: Some(task_id.clone()),
                    purpose: "submit".to_string(),
                    prompt_chars: prompt.len(),
                    preview,
                    cited_kb_ids: vec![],
                });
            dispatched_to = Some(candidate_id.to_string());
            info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: dispatched to idle slot");
            break;
        }
    }

    // Phase 2: No idle slot — auto-spawn an exited/no-session slot then dispatch
    if dispatched_to.is_none() {
        for candidate_id in &candidates {
            if !state.slot_dispatch.try_acquire(candidate_id) {
                continue;
            }
            let status = state.pty.get_status(candidate_id).await;
            let is_spawnable = match &status {
                Some(s) => s.state == missiond_core::pty::SessionState::Exited,
                None => true,
            };
            if !is_spawnable {
                state.slot_dispatch.release(candidate_id);
                continue;
            }

            let slot = match slots.iter().find(|s| s.config.id == *candidate_id) {
                Some(s) => s,
                None => {
                    state.slot_dispatch.release(candidate_id);
                    continue;
                }
            };

            let pty_slot = missiond_core::PTYSlot {
                id: slot.config.id.clone(),
                role: slot.config.role.clone(),
                cwd: slot.config.cwd.as_deref().map(std::path::PathBuf::from),
                engine: slot.config.engine,
            };
            let mcp_config = slot.config.mcp_config.clone().map(std::path::PathBuf::from);
            info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: auto-spawning exited slot");
            let spawn_ok = crate::slot_orchestrator::spawner::spawn_tracked_slot(
                &state.pty,
                &state.store,
                &state.pty_session_uuids,
                &pty_slot,
                PTYSpawnOptions {
                    auto_restart: false,
                    wait_for_idle: true,
                    timeout_secs: Some(30),
                    mcp_config,
                    dangerously_skip_permissions: slot
                        .config
                        .dangerously_skip_permissions
                        .unwrap_or(false),
                    model: slot.config.model.clone(),
                    extra_env: std::collections::HashMap::new(),
                },
                slot.config.env.as_ref(),
            )
            .await
            .is_ok();

            let sent = if spawn_ok {
                state
                    .pty
                    .send_fire_and_forget(candidate_id, &prompt)
                    .await
                    .ok()
                    .is_some()
            } else {
                warn!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: auto-spawn failed");
                false
            };
            state.slot_dispatch.release(candidate_id);

            if sent {
                let now = chrono::Utc::now().timestamp_millis();
                let slot_session = state
                    .store
                    .get_slot_session(candidate_id)
                    .await
                    .ok()
                    .flatten();
                let _ = state
                    .store
                    .update_task(
                        &task_id,
                        &missiond_core::types::TaskUpdate {
                            status: Some(missiond_core::types::TaskStatus::Running),
                            slot_id: Some(candidate_id.to_string()),
                            session_id: slot_session,
                            started_at: Some(now),
                            ..Default::default()
                        },
                    )
                    .await;
                let preview = if prompt.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !prompt.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &prompt[..end])
                } else {
                    prompt.clone()
                };
                state
                    .event_bus
                    .publish(crate::event_bus::DaemonEvent::SlotTaskDispatched {
                        slot_id: candidate_id.to_string(),
                        task_id: Some(task_id.clone()),
                        purpose: "submit".to_string(),
                        prompt_chars: prompt.len(),
                        preview,
                        cited_kb_ids: vec![],
                    });
                dispatched_to = Some(candidate_id.to_string());
                info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: spawned + dispatched");
                break;
            }
        }
    }

    let mut result = serde_json::json!({ "taskId": task_id });
    if let Some(slot_id) = dispatched_to {
        result["dispatched"] = serde_json::json!(true);
        result["slotId"] = serde_json::json!(slot_id);
    } else {
        result["dispatched"] = serde_json::json!(false);
        result["hint"] =
            serde_json::json!("No idle slot found, task queued for autopilot dispatch");
        state.event_bus.publish_traced(
            crate::event_bus::DaemonEvent::TaskCreated {
                task_id: task_id.clone(),
            },
            crate::event_bus::TraceContext {
                trace_id: Some(task_id.clone()),
                summary: Some("Submit task queued for autopilot".to_string()),
                ..Default::default()
            },
        );
    }
    Ok(ToolResult::json(&result))
}

async fn handle_ask(state: &AppState, args: Value) -> Result<ToolResult> {
    let AskArgs {
        role,
        question,
        timeout_ms: _,
    } = serde_json::from_value(args)?;
    let task_id = crate::state::submit_task(state.store.as_ref(), &role, &question).await?;
    Ok(ToolResult::json(&serde_json::json!({
        "taskId": task_id,
        "hint": "Task created. Use mission_submit for PTY dispatch."
    })))
}

async fn handle_status(state: &AppState, args: Value) -> Result<ToolResult> {
    let StatusArgs { task_id } = serde_json::from_value(args)?;
    if let Some(task) = state.store.get_task(&task_id).await.ok().flatten() {
        Ok(ToolResult::json(&task))
    } else {
        Ok(ToolResult::error("Task not found"))
    }
}

async fn handle_cancel(state: &AppState, args: Value) -> Result<ToolResult> {
    let CancelArgs { task_id } = serde_json::from_value(args)?;
    // Cancel: update task status to Cancelled if currently Queued or Running
    let cancelled = if let Ok(Some(task)) = state.store.get_task(&task_id).await {
        if task.status == missiond_core::types::TaskStatus::Queued
            || task.status == missiond_core::types::TaskStatus::Running
        {
            let now = chrono::Utc::now().timestamp_millis();
            state
                .store
                .update_task(
                    &task_id,
                    &missiond_core::types::TaskUpdate {
                        status: Some(missiond_core::types::TaskStatus::Cancelled),
                        finished_at: Some(now),
                        ..Default::default()
                    },
                )
                .await
                .is_ok()
        } else {
            false
        }
    } else {
        false
    };
    Ok(ToolResult::json(
        &serde_json::json!({ "cancelled": cancelled }),
    ))
}

async fn handle_task_list(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct Args {
        status: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_i64")]
        limit: Option<i64>,
    }
    let args: Args = serde_json::from_value(args).unwrap_or(Args {
        status: None,
        limit: None,
    });
    let limit = args.limit.unwrap_or(20);
    let tasks = if let Some(ref status_str) = args.status {
        if let Some(status) = missiond_core::types::TaskStatus::from_str(status_str) {
            state.store.get_tasks_by_status(status).await?
        } else {
            return Ok(ToolResult::error(format!(
                "Invalid status: {}. Use: queued, running, done, failed",
                status_str
            )));
        }
    } else {
        state.store.get_all_tasks(limit).await?
    };
    Ok(ToolResult::json(&tasks))
}

async fn handle_task_ack(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
    let since = args_val.get("since").and_then(|v| v.as_i64());
    let tasks = state.store.ack_completed_tasks(since).await?;
    Ok(ToolResult::json(&tasks))
}

async fn handle_task_track(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        task_id: String,
    }
    let Args { task_id } = serde_json::from_value(args)?;

    // 1. Task status
    let task = state
        .store
        .get_task(&task_id)
        .await
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
            if let Ok(Some(session_uuid)) = state.store.get_slot_session(slot_id).await {
                slot_obj["sessionId"] = json!(session_uuid);
                if let Ok(Some(conv)) = state.store.get_conversation(&session_uuid).await {
                    if let Some(ref jp) = conv.jsonl_path {
                        if let Ok(md) = std::fs::metadata(jp) {
                            if let Ok(m) = md.modified() {
                                slot_obj["lastActivitySecsAgo"] =
                                    json!(m.elapsed().unwrap_or_default().as_secs());
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
                        while end > 0 && !resp.is_char_boundary(end) {
                            end -= 1;
                        }
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
