use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::context::v3_blueprint_runtime::ComputePrimitivesRuntimeConfig;
use crate::lenient;
use crate::state::AppState;
use missiond_core::types::{BoardTask, BoardTaskStatus, Task, TaskStatus};
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
            let _ = state
                .bus
                .publish_slot(missiond_core::event::events::SlotEvent::TaskDispatched {
                    slot_id: candidate_id.to_string(),
                    task_id: Some(task_id.clone()),
                    purpose: "submit".to_string(),
                    prompt_chars: prompt.len(),
                    preview,
                    cited_kb_ids: vec![],
                })
                .await;
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
            let spawn_timeout_secs =
                load_spawn_timeout_secs_for_slot(state, pty_slot.cwd.as_deref()).await?;
            info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: auto-spawning exited slot");
            let spawn_ok = crate::slot_orchestrator::spawner::spawn_tracked_slot(
                &state.pty,
                &state.store,
                &state.pty_session_uuids,
                &state.project_registry,
                state.permission.learned(),
                &pty_slot,
                PTYSpawnOptions {
                    auto_restart: false,
                    wait_for_idle: true,
                    timeout_secs: Some(spawn_timeout_secs),
                    mcp_config,
                    dangerously_skip_permissions: slot
                        .config
                        .dangerously_skip_permissions
                        .unwrap_or(false),
                    model: slot.config.model.clone(),
                    reasoning_effort: slot.config.reasoning_effort.clone(),
                    search_enabled: slot.config.search_enabled.unwrap_or(false),
                    sandbox: slot.config.sandbox.clone(),
                    approval_policy: slot.config.approval_policy.clone(),
                    tool_policy_path: slot
                        .config
                        .tool_policy_path
                        .clone()
                        .map(std::path::PathBuf::from),
                    extra_env: std::collections::HashMap::new(),
                    initial_prompt: None,
                    command_override: None,
                    ..Default::default()
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
                let _ = state
                    .bus
                    .publish_slot(missiond_core::event::events::SlotEvent::TaskDispatched {
                        slot_id: candidate_id.to_string(),
                        task_id: Some(task_id.clone()),
                        purpose: "submit".to_string(),
                        prompt_chars: prompt.len(),
                        preview,
                        cited_kb_ids: vec![],
                    })
                    .await;
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
        let _ = state
            .bus
            .publish_task(missiond_core::event::events::TaskEvent::Created {
                task_id: task_id.clone(),
            })
            .await;
    }
    Ok(ToolResult::json(&result))
}

async fn load_spawn_timeout_secs_for_slot(
    state: &AppState,
    cwd: Option<&std::path::Path>,
) -> Result<u64> {
    let resolved_project_root = match cwd {
        Some(cwd) => match crate::slot_orchestrator::project_root::resolve_target_project_root(
            None,
            Some(cwd),
            None,
            &state.project_registry,
        )
        .await
        {
            Ok(r) => Some(r.project_root),
            Err(_) => Some(cwd.to_path_buf()),
        },
        None => None,
    };
    let project_root = resolved_project_root
        .as_ref()
        .map(|cwd| cwd.to_string_lossy());
    let runtime_config =
        ComputePrimitivesRuntimeConfig::load_for_project_root(project_root.as_deref())
            .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))?;
    Ok(runtime_config.pty_spawn_timeout_secs())
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
    } else if let Some(task) = state.store.get_board_task(&task_id).await.ok().flatten() {
        let value = board_task_query_json(state, &task).await;
        Ok(ToolResult::json(&value))
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
    if let Some(ref status_str) = args.status {
        if let Some(status) = TaskStatus::from_str(status_str) {
            let mut tasks = Vec::new();
            for task in state.store.get_tasks_by_status(status).await? {
                tasks.push(legacy_task_query_json(task));
            }
            for board_status in board_statuses_for_task_query(status) {
                for task in state
                    .store
                    .list_board_tasks(Some(board_status.as_str()), true)
                    .await?
                {
                    tasks.push(board_task_query_json(state, &task).await);
                }
            }
            tasks.truncate(limit.max(0) as usize);
            Ok(ToolResult::json(&tasks))
        } else {
            Ok(ToolResult::error(format!(
                "Invalid status: {}. Use: queued, running, done, failed",
                status_str
            )))
        }
    } else {
        let tasks = state.store.get_all_tasks(limit).await?;
        Ok(ToolResult::json(&tasks))
    }
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

fn legacy_task_query_json(task: Task) -> Value {
    serde_json::to_value(task).unwrap_or_else(|_| json!({}))
}

fn board_statuses_for_task_query(status: TaskStatus) -> &'static [BoardTaskStatus] {
    match status {
        TaskStatus::Queued => &[BoardTaskStatus::Open],
        TaskStatus::Running => &[BoardTaskStatus::Running, BoardTaskStatus::Verifying],
        TaskStatus::Done => &[BoardTaskStatus::Done, BoardTaskStatus::Skipped],
        TaskStatus::Failed => &[BoardTaskStatus::Failed, BoardTaskStatus::Blocked],
        TaskStatus::Cancelled => &[],
    }
}

fn board_task_query_status(status: BoardTaskStatus) -> &'static str {
    match status {
        BoardTaskStatus::Open => "queued",
        BoardTaskStatus::Running | BoardTaskStatus::Verifying => "running",
        BoardTaskStatus::Done | BoardTaskStatus::Skipped => "done",
        BoardTaskStatus::Failed | BoardTaskStatus::Blocked => "failed",
    }
}

fn board_task_slot_id(task: &BoardTask) -> Option<&str> {
    task.claim_executor_id
        .as_deref()
        .or(task.assignee.as_deref())
}

async fn board_task_query_json(state: &AppState, task: &BoardTask) -> Value {
    let slot_id = board_task_slot_id(task);
    let session_id = if let Some(slot_id) = slot_id {
        state.store.get_slot_session(slot_id).await.ok().flatten()
    } else {
        None
    };
    board_task_query_json_with_session(task, session_id.as_deref())
}

fn board_task_query_json_with_session(task: &BoardTask, session_id: Option<&str>) -> Value {
    let slot_id = board_task_slot_id(task);
    json!({
        "id": task.id.as_str(),
        "role": task.context_intent.as_deref().unwrap_or(task.category.as_str()),
        "prompt": task.description.as_str(),
        "status": board_task_query_status(task.status),
        "boardStatus": task.status.as_str(),
        "slotId": slot_id,
        "sessionId": session_id,
        "result": Value::Null,
        "error": Value::Null,
        "createdAt": task.created_at.as_str(),
        "startedAt": task.claimed_at.as_deref(),
        "finishedAt": match task.status {
            BoardTaskStatus::Done | BoardTaskStatus::Skipped | BoardTaskStatus::Failed => {
                Some(task.updated_at.as_str())
            }
            _ => None,
        },
        "source": "board_task",
        "boardTask": {
            "id": task.id.as_str(),
            "title": task.title.as_str(),
            "status": task.status.as_str(),
            "project": task.project.as_deref(),
            "category": task.category.as_str(),
            "parentId": task.parent_id.as_ref().map(|id| id.as_str()),
            "assignee": task.assignee.as_deref(),
            "claimExecutorId": task.claim_executor_id.as_deref(),
            "claimExecutorType": task.claim_executor_type.as_deref(),
            "updatedAt": task.updated_at.as_str(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_core::types::TaskId;

    fn sample_board_task(status: BoardTaskStatus) -> BoardTask {
        BoardTask {
            id: TaskId::from_trusted("11111111-1111-4111-8111-111111111111".to_string()),
            title: "Fix running visibility".to_string(),
            description: "Bridge BoardTask into mission_task_query".to_string(),
            status,
            priority: "high".to_string(),
            category: "dev".to_string(),
            project: Some("missiond".to_string()),
            server: None,
            due_date: None,
            parent_id: Some(TaskId::from_trusted(
                "22222222-2222-4222-8222-222222222222".to_string(),
            )),
            assignee: Some("slot-dyn-example".to_string()),
            auto_execute: true,
            prompt_template: None,
            hidden: false,
            retry_count: 0,
            max_retries: 2,
            order_idx: 1,
            created_at: "2026-05-05T17:35:42Z".to_string(),
            updated_at: "2026-05-05T17:36:42Z".to_string(),
            claim_executor_id: Some("slot-dyn-example".to_string()),
            claim_executor_type: Some("pty_slot".to_string()),
            claimed_at: Some("2026-05-05T17:35:43Z".to_string()),
            flow_phase: None,
            flow_context: None,
            flow_template: None,
            depends_on: vec![],
            lease_expires_at: None,
            dedupe_key: None,
            timeout_secs: Some(5400),
            context_intent: Some("code".to_string()),
            trigger_source: Some("mission_task_delegate".to_string()),
            runtime_metadata: serde_json::json!({}),
            notes_count: 0,
        }
    }

    #[test]
    fn task_query_running_status_covers_active_board_tasks() {
        assert_eq!(
            board_statuses_for_task_query(TaskStatus::Running),
            &[BoardTaskStatus::Running, BoardTaskStatus::Verifying]
        );
        assert_eq!(board_task_query_status(BoardTaskStatus::Running), "running");
        assert_eq!(
            board_task_query_status(BoardTaskStatus::Verifying),
            "running"
        );
    }

    #[test]
    fn board_task_query_json_preserves_slot_and_board_linkage() {
        let task = sample_board_task(BoardTaskStatus::Running);
        let value = board_task_query_json_with_session(&task, Some("session-1"));
        assert_eq!(value["source"], "board_task");
        assert_eq!(value["status"], "running");
        assert_eq!(value["boardStatus"], "running");
        assert_eq!(value["slotId"], "slot-dyn-example");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["boardTask"]["claimExecutorId"], "slot-dyn-example");
        assert_eq!(
            value["boardTask"]["parentId"],
            "22222222-2222-4222-8222-222222222222"
        );
    }
}
