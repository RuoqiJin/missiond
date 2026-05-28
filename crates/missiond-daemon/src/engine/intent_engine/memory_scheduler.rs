use tracing::{debug, info, warn};

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;
use crate::engine::control_plane_kernel::{ControlPlaneKernel, RequireCapabilityCommand};
use crate::extraction::{check_deep_analysis, check_kb_consolidation, check_realtime_extraction};
use crate::state::AppState;
use crate::state::MEMORY_SLOT_ID;
use missiond_core::PTYSpawnOptions;
use missiond_core::SessionState;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;

pub(crate) async fn ensure_memory_slot_by_id(state: &AppState, slot_id: &str) -> bool {
    // Check if session is actually running (not just initialized/exited)
    if let Some(info) = state.pty.get_status(slot_id).await {
        if info.state != SessionState::Exited {
            return true;
        }
    }
    let slot = state
        .mission
        .list_slots()
        .into_iter()
        .find(|s| s.config.id == slot_id);
    let Some(slot) = slot else {
        warn!(slot_id, "Memory slot not configured in slots.yaml");
        return false;
    };
    // ControlTree: refuse spawn if slot_role is paused
    if state
        .control_manager
        .current()
        .is_slot_role_paused(&slot.config.role)
    {
        tracing::warn!(slot_id, role = %slot.config.role, "ensure_memory_slot: slot_role paused, refusing spawn");
        return false;
    }
    let pty_slot = missiond_core::PTYSlot {
        id: slot.config.id.clone(),
        role: slot.config.role.clone(),
        cwd: slot.config.cwd.as_deref().map(PathBuf::from),
        engine: slot.config.engine,
    };
    let config_root = slot
        .config
        .project_root
        .as_deref()
        .or(slot.config.cwd.as_deref());
    let runtime_config = match WorkstationRuntimeConfig::load_for_project_root(config_root) {
        Ok(config) => config,
        Err(err) => {
            warn!(
                slot_id,
                error = %err,
                "Failed to load V3 workstation runtime config for memory slot spawn"
            );
            return false;
        }
    };
    let spawn_timeout_secs = runtime_config.dynamic_slot_spawn_timeout_secs();
    let slot_env = slot.config.env.as_ref();
    let mcp_config = slot.config.mcp_config.map(PathBuf::from);
    match crate::slot_orchestrator::spawner::spawn_tracked_slot(
        &state.pty,
        &state.store,
        &state.pty_session_uuids,
        &state.project_registry,
        state.permission.learned(),
        &pty_slot,
        PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: true,
            timeout_secs: Some(spawn_timeout_secs),
            mcp_config,
            dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
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
        },
        slot_env,
    )
    .await
    {
        Ok(_) => {
            info!(slot_id, "Memory slot spawned (auto_restart=true)");
            true
        }
        Err(e) => {
            warn!(slot_id, error = %e, "Failed to spawn memory slot");
            false
        }
    }
}

/// Convenience wrapper for fast-lane memory slot.
pub(crate) async fn ensure_memory_slot(state: &AppState) -> bool {
    ensure_memory_slot_by_id(state, MEMORY_SLOT_ID).await
}

/// Unified priority scheduler for memory slots.
// @beacon: memory
/// Enforces strict priority: Submit Tasks > Realtime Extraction > Deep Analysis > KB Consolidation.
/// Called from autopilot_tick (60s fallback) and event-driven paths (immediate).
pub(crate) async fn schedule_memory_tasks(state: &AppState) {
    // P1: Submit tasks — highest priority, dispatch to any idle memory slot
    dispatch_queued_submit_tasks(state).await;

    // P2: Fast lane — realtime extraction on slot-memory
    // (only runs if slot-memory wasn't grabbed by P1)
    check_realtime_extraction(state).await;

    // P3: Slow lane — deep analysis + consolidation on slot-memory-slow
    // (independent of fast lane, only blocked if P1 grabbed slot-memory-slow)
    check_deep_analysis(state).await;
    check_kb_consolidation(state).await;
}

/// Dispatch queued memory-hook tasks from `board_tasks` (trigger_source='memory_hook', status='open').
///
/// v0.5.0: migrated from legacy `tasks` table. Field mapping:
///   - `role` → `category` (e.g. "memory")
///   - `prompt` → `prompt_template`
///   - `slot_id` pin → `assignee`
///   - Queued→open, Running→running, Done→done, Failed→failed.
///
/// Returns true if at least one task was dispatched.
pub(crate) async fn dispatch_queued_submit_tasks(state: &AppState) -> bool {
    // Batch scan: all open memory-hook board_tasks.
    let queued = match state
        .store
        .list_board_tasks_by_trigger("memory_hook", "open", 256)
        .await
    {
        Ok(tasks) => tasks,
        Err(e) => {
            warn!(error = %e, "Failed to query queued memory-hook board_tasks");
            return false;
        }
    };

    if queued.is_empty() {
        return false;
    }

    info!(
        count = queued.len(),
        "Autopilot: found queued memory-hook tasks"
    );

    let mut any_dispatched = false;
    // Track slots used in this dispatch round to avoid sending multiple tasks to the same slot
    let mut used_slots: HashSet<String> = HashSet::new();

    for task in &queued {
        let slots = state.mission.list_slots();
        let mut dispatched = false;

        let role = task.category.as_str();
        let prompt = task.prompt_template.as_deref().unwrap_or("");
        if prompt.is_empty() {
            warn!(task_id = %task.id, "Memory-hook task has empty prompt_template, skipping");
            continue;
        }

        let candidates: Vec<String> = if let Some(ref target) = task.assignee {
            vec![target.clone()]
        } else {
            slots
                .iter()
                .filter(|s| s.config.role == role)
                .map(|s| s.config.id.clone())
                .collect()
        };

        // Phase 1: Try idle slots (skip slots already used in this round)
        for slot_id in &candidates {
            if used_slots.contains(slot_id) {
                continue;
            }
            if !require_memory_hook_claim_authority(state, task.id.as_str(), slot_id, role).await {
                continue;
            }
            // Acquire per-slot dispatch guard
            if !state.slot_dispatch.try_acquire(slot_id) {
                continue;
            }
            let status = match state.pty.get_status(slot_id).await {
                Some(s) => s,
                None => {
                    state.slot_dispatch.release(slot_id);
                    continue;
                }
            };
            let sent = if status.state == missiond_core::pty::SessionState::Idle {
                state
                    .pty
                    .send_fire_and_forget(slot_id, prompt)
                    .await
                    .ok()
                    .is_some()
            } else {
                false
            };
            state.slot_dispatch.release(slot_id);

            if sent {
                // Claim the board task atomically (sets status=running + claim_executor + claimed_at).
                let _ = state
                    .store
                    .claim_board_task(task.id.as_str(), slot_id, "pty_slot")
                    .await;
                let preview = if prompt.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !prompt.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &prompt[..end])
                } else {
                    prompt.to_string()
                };
                let _ = state
                    .bus
                    .publish_slot(missiond_core::event::events::SlotEvent::TaskDispatched {
                        slot_id: slot_id.clone(),
                        task_id: Some(task.id.as_str().to_string()),
                        purpose: "submit".to_string(),
                        prompt_chars: prompt.len(),
                        preview,
                        cited_kb_ids: vec![],
                    })
                    .await;
                info!(task_id = %task.id, slot_id = %slot_id, role = %role, "Autopilot: dispatched queued memory-hook task");
                state
                    .stats
                    .tasks_dispatched
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                used_slots.insert(slot_id.clone());
                dispatched = true;
                any_dispatched = true;
                break;
            }
        }

        if !dispatched {
            debug!(task_id = %task.id, role = %role, "Autopilot: no idle slot for queued memory-hook task");
        }
    }

    // Phase 2: Wake-on-Demand — auto-spawn stopped slots for queued tasks
    // Respects pinned assignee: if task specifies a slot, only wake that exact slot.
    {
        let remaining = match state
            .store
            .list_board_tasks_by_trigger("memory_hook", "open", 256)
            .await
        {
            Ok(t) => t,
            Err(_) => vec![],
        };
        if !remaining.is_empty() {
            let slots = state.mission.list_slots();
            let mut woken_slots: HashSet<String> = HashSet::new();
            for task in &remaining {
                let role = task.category.as_str();
                // Determine target slot(s) to wake
                let target_slot_id = if let Some(ref pinned) = task.assignee {
                    // Pinned task: only wake the exact slot
                    if !slots.iter().any(|s| s.config.id == *pinned) {
                        warn!(task_id = %task.id, slot_id = %pinned, "Autopilot: pinned slot not found, marking memory-hook task Failed");
                        let _ = state
                            .store
                            .update_board_task(
                                task.id.as_str(),
                                &missiond_core::types::UpdateBoardTaskInput {
                                    status: Some("failed".to_string()),
                                    ..Default::default()
                                },
                            )
                            .await;
                        let _ = state
                            .store
                            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.as_str().to_string(),
                                content: format!("pinned slot '{pinned}' not found in slots.yaml"),
                                note_type: Some("note".to_string()),
                                author: Some("memory-hook".to_string()),
                            })
                            .await;
                        continue;
                    }
                    pinned.clone()
                } else {
                    // Unpinned: find any slot with matching role
                    match slots.iter().find(|s| {
                        s.config.role == role
                            && !used_slots.contains(&s.config.id)
                            && !woken_slots.contains(&s.config.id)
                    }) {
                        Some(s) => s.config.id.clone(),
                        None => continue,
                    }
                };

                if used_slots.contains(&target_slot_id) || woken_slots.contains(&target_slot_id) {
                    continue;
                }

                let status = state.pty.get_status(&target_slot_id).await;
                let is_spawnable = match &status {
                    Some(s) => s.state == missiond_core::pty::SessionState::Exited,
                    None => true,
                };
                if !is_spawnable {
                    continue;
                }

                woken_slots.insert(target_slot_id.clone());
                let state_clone = state.clone();
                let slot_id_clone = target_slot_id.clone();
                info!(slot_id = %target_slot_id, role = %role, task_id = %task.id, "Autopilot: auto-spawning slot for queued memory-hook task (Wake-on-Demand)");
                tokio::spawn(async move {
                    if ensure_memory_slot_by_id(&state_clone, &slot_id_clone).await {
                        let _ = state_clone
                            .bus
                            .publish_task(missiond_core::event::events::TaskEvent::Created {
                                task_id: String::new(),
                            })
                            .await;
                    }
                });
            }
        }
    }

    any_dispatched
}

async fn require_memory_hook_claim_authority(
    state: &AppState,
    task_id: &str,
    slot_id: &str,
    role: &str,
) -> bool {
    match ControlPlaneKernel::new(state)
        .require_capability_command(RequireCapabilityCommand {
            grant_id: None,
            subject_kind: "system".to_string(),
            subject_id: "memory_scheduler".to_string(),
            operation: "claim".to_string(),
            scope_kind: "task".to_string(),
            scope_key: task_id.to_string(),
            task_id: Some(task_id.to_string()),
            allow_system_bypass: true,
            bypass_reason: Some("memory scheduler internal BoardTask claim".to_string()),
            details: json!({
                "slot_id": slot_id,
                "role": role,
                "source": "memory_scheduler.dispatch_queued_submit_tasks"
            }),
        })
        .await
    {
        Ok(_) => true,
        Err(err) => {
            warn!(
                task_id,
                slot_id,
                role,
                error = %err,
                "Memory scheduler: BoardTask claim capability denied"
            );
            false
        }
    }
}

/// Reap memory-hook board_tasks stuck in Running state for too long (15 min).
///
/// v0.5.0: migrated to board_tasks. Uses:
///   - `list_board_tasks_by_trigger("memory_hook", "running", _)` for scan
///   - `BoardStore::recover_stale_running_tasks` for the hard-timeout path
///   - JSONL compensation preserved for tasks between 2–15 min elapsed.
///
/// If the slot is Idle at hard timeout, mark Done; otherwise mark Failed.
pub(crate) async fn reap_stale_submit_tasks(state: &AppState) {
    // Hard-timeout path — release pillar 二 pattern: recover_stale_running_tasks handles
    // stale-lease recovery (status=running → open, clear claim) for tasks whose lease is
    // way past. For memory-hook our lease is not set, so we fall back to manual scan.
    let running = match state
        .store
        .list_board_tasks_by_trigger("memory_hook", "running", 256)
        .await
    {
        Ok(t) => t,
        Err(_) => return,
    };
    if running.is_empty() {
        return;
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    const JSONL_CHECK_THRESHOLD_MS: i64 = 2 * 60 * 1000; // 2 minutes: start checking JSONL
    const SUBMIT_TASK_TIMEOUT_MS: i64 = 15 * 60 * 1000; // 15 minutes: hard timeout

    for task in &running {
        let started_ms =
            parse_rfc3339_to_ms(task.claimed_at.as_deref().unwrap_or(&task.updated_at))
                .unwrap_or(now_ms);
        let elapsed = now_ms - started_ms;

        if elapsed < JSONL_CHECK_THRESHOLD_MS {
            continue;
        }

        let slot_id = task.claim_executor_id.as_deref();

        // --- JSONL completion detection (compensating path for missed PTY Idle events) ---
        if elapsed < SUBMIT_TASK_TIMEOUT_MS {
            if let Some(slot_id) = slot_id {
                if let Some(jsonl_path) = resolve_jsonl_path(state, slot_id).await {
                    let path = std::path::Path::new(&jsonl_path);
                    if missiond_core::jsonl_has_completed_turn(path).await {
                        // JSONL confirms turn completed — extract result and close
                        let result_text = missiond_core::extract_last_assistant_text(path)
                            .await
                            .unwrap_or_else(|| "completed (JSONL turn_duration)".to_string());
                        let result_text = truncate_result(&result_text);
                        close_memory_task(state, task, "done", &result_text).await;
                        // Board progress extraction: parse JSON output and write notes
                        let prompt = task.prompt_template.as_deref().unwrap_or("");
                        if prompt.starts_with("[board_progress]") {
                            apply_board_progress_result(state, &result_text).await;
                        }
                        info!(
                            task_id = %task.id, slot_id = %slot_id,
                            age_min = elapsed / 60000,
                            "Memory-hook task closed via JSONL turn_duration compensation"
                        );
                        continue;
                    }
                }
            }
            // Not yet at hard timeout and JSONL didn't confirm — wait
            continue;
        }

        // --- Hard timeout (15 min) ---
        let slot_idle = if let Some(sid) = slot_id {
            state
                .pty
                .get_status(sid)
                .await
                .map(|s| s.state == missiond_core::pty::SessionState::Idle)
                .unwrap_or(true)
        } else {
            true
        };

        let (new_status, result_msg) = if slot_idle {
            // Try JSONL result even at timeout
            let jsonl_result = if let Some(sid) = slot_id {
                if let Some(jsonl_path) = resolve_jsonl_path(state, sid).await {
                    missiond_core::extract_last_assistant_text(std::path::Path::new(&jsonl_path))
                        .await
                } else {
                    None
                }
            } else {
                None
            };
            (
                "done",
                jsonl_result.unwrap_or_else(|| "completed (timeout reaper)".to_string()),
            )
        } else {
            ("failed", "timed out after 15 minutes".to_string())
        };

        let result_msg = truncate_result(&result_msg);
        close_memory_task(state, task, new_status, &result_msg).await;
        if new_status == "done" {
            continue;
        }
        let kb_status = new_status;
        if let Ok(true) = state
            .store
            .kb_ops_complete_by_task_id(task.id.as_str(), kb_status, Some(&result_msg))
            .await
        {
            info!(task_id = %task.id, kb_status = kb_status, "KB operation updated via reaper");
        }
        let prompt = task.prompt_template.as_deref().unwrap_or("");
        if new_status == "done" && prompt.starts_with("[board_progress]") {
            apply_board_progress_result(state, &result_msg).await;
        }
        warn!(
            task_id = %task.id,
            slot_id = ?slot_id,
            status = %new_status,
            age_min = elapsed / 60000,
            "Reaped stale memory-hook task"
        );
    }
}

/// Parse RFC3339 timestamp to epoch millis (for elapsed-time math on board_tasks columns).
fn parse_rfc3339_to_ms(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Resolve the JSONL path for a given slot via slot_sessions → conversations.
async fn resolve_jsonl_path(state: &AppState, slot_id: &str) -> Option<String> {
    let session_uuid = state.store.get_slot_session(slot_id).await.ok().flatten()?;
    let conv = state
        .store
        .get_conversation(&session_uuid)
        .await
        .ok()
        .flatten()?;
    conv.jsonl_path
}

/// Safe UTF-8 truncation to 4KB for result text (shared by JSONL + timeout paths).
fn truncate_result(text: &str) -> String {
    const LIMIT: usize = 4096;
    if text.len() <= LIMIT {
        return text.to_string();
    }
    let mut end = LIMIT;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...(truncated)", &text[..end])
}

/// Close a memory-hook board task: set status + append result note.
async fn close_memory_task(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
    new_status: &str,
    result_text: &str,
) {
    if new_status == "done" {
        let _ = crate::engine::control_plane_kernel::ControlPlaneKernel::new(state)
            .record_observation(
                task.id.as_str(),
                "memory-hook",
                json!({
                    "schema": "missiond.memory-hook-observation.v1",
                    "status_candidate": "done",
                    "summary": result_text,
                    "authority": "observation-only",
                    "note": "memory scheduler cannot close BoardTask without canonical task_result_artifact + worker_settle"
                }),
            )
            .await;
        let _ = state
            .store
            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.as_str().to_string(),
                content: result_text.to_string(),
                note_type: Some("summary".to_string()),
                author: Some("memory-hook".to_string()),
            })
            .await;
        return;
    }
    let _ = state
        .store
        .update_board_task(
            task.id.as_str(),
            &missiond_core::types::UpdateBoardTaskInput {
                status: Some(new_status.to_string()),
                ..Default::default()
            },
        )
        .await;
    let _ = state
        .store
        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
            task_id: task.id.as_str().to_string(),
            content: result_text.to_string(),
            note_type: Some("summary".to_string()),
            author: Some("memory-hook".to_string()),
        })
        .await;
    let _ = state
        .store
        .kb_ops_complete_by_task_id(task.id.as_str(), new_status, Some(result_text))
        .await;
}

/// Parse structured JSON from board_progress extraction task and write notes/status to DB.
/// Worker outputs: { "task_progress": [{ task_id, summary, is_done, confidence }] }
/// Daemon writes directly to DB — no MCP tool calls needed (Gemini ARB recommendation).
async fn apply_board_progress_result(state: &AppState, json_text: &str) {
    #[derive(serde::Deserialize)]
    struct ProgressOutput {
        task_progress: Vec<TaskProgress>,
    }
    #[derive(serde::Deserialize)]
    struct TaskProgress {
        task_id: String,
        summary: String,
        #[serde(default)]
        is_done: bool,
        #[serde(default = "default_confidence")]
        confidence: f64,
    }
    fn default_confidence() -> f64 {
        0.5
    }

    // Extract JSON from response (may have surrounding text)
    let json_str = if let (Some(start), Some(end)) = (json_text.find('{'), json_text.rfind('}')) {
        &json_text[start..=end]
    } else {
        json_text
    };

    let output: ProgressOutput = match serde_json::from_str(json_str) {
        Ok(o) => o,
        Err(e) => {
            warn!(error = %e, preview = %json_text.chars().take(200).collect::<String>(),
                "Failed to parse board progress JSON");
            return;
        }
    };

    for tp in &output.task_progress {
        if tp.confidence < 0.5 {
            continue;
        }

        // Write progress note
        let _ = state
            .store
            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                task_id: tp.task_id.clone(),
                content: tp.summary.clone(),
                note_type: Some("progress".to_string()),
                author: Some("auto-extract".to_string()),
            })
            .await;
        info!(task_id = %tp.task_id, is_done = tp.is_done, confidence = tp.confidence,
            "Board progress note written (auto-extract)");

        // PTY/provider progress is an observation only. The control plane may
        // settle the task only after a canonical task_result_artifact exists.
        if tp.is_done && tp.confidence >= 0.8 {
            let _ = crate::engine::control_plane_kernel::ControlPlaneKernel::new(state)
                .record_observation(
                    &tp.task_id,
                    "memory_scheduler",
                    json!({
                        "schema": "missiond.task-progress-observation.v1",
                        "is_done": tp.is_done,
                        "confidence": tp.confidence,
                        "summary": tp.summary
                    }),
                )
                .await;
            debug!(task_id = %tp.task_id, confidence = tp.confidence, "Board task completion observation recorded; canonical artifact still required");
        }
    }
}
