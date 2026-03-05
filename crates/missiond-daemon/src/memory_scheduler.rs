
use tracing::{debug, info, warn};

use crate::state::AppState;
use crate::autopilot::MEMORY_SLOT_ID;
use crate::extraction::{check_realtime_extraction, check_deep_analysis, check_kb_consolidation};
use missiond_core::SessionState;
use crate::slot_env::build_slot_tracking_env;
use crate::slot_env::capture_slot_session_uuid;
use missiond_core::PTYSpawnOptions;
use std::path::PathBuf;
use std::collections::HashSet;
use crate::supervisor::get_task_jsonl_path;

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
    let pty_slot = missiond_core::PTYSlot {
        id: slot.config.id.clone(),
        role: slot.config.role.clone(),
        cwd: slot.config.cwd.as_deref().map(PathBuf::from),
    };
    let slot_env = slot.config.env.as_ref();
    let mcp_config = slot.config.mcp_config.map(PathBuf::from);
    let (extra_env, session_file) = build_slot_tracking_env(slot_id, slot_env).await;
    match state.pty.spawn(&pty_slot, PTYSpawnOptions {
        auto_restart: true,
        wait_for_idle: true,
        timeout_secs: Some(120),
        mcp_config,
        dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
        extra_env,
    }).await {
        Ok(_) => {
            capture_slot_session_uuid(state, slot_id, &session_file).await;
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

/// Dispatch queued tasks from the `tasks` table (created by mission_submit).
/// Returns true if at least one task was dispatched.
/// Part of the unified priority scheduler — called before extraction checks.
pub(crate) async fn dispatch_queued_submit_tasks(state: &AppState) -> bool {
    let queued = match state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Queued) {
        Ok(tasks) => tasks,
        Err(e) => {
            warn!(error = %e, "Failed to query queued submit tasks");
            return false;
        }
    };

    if queued.is_empty() {
        return false;
    }

    info!(count = queued.len(), "Autopilot: found queued submit tasks");

    let mut any_dispatched = false;
    // Track slots used in this dispatch round to avoid sending multiple tasks to the same slot
    let mut used_slots: HashSet<String> = HashSet::new();

    for task in &queued {
        let slots = state.mission.list_slots();
        let mut dispatched = false;

        let candidates: Vec<String> = if let Some(ref target) = task.slot_id {
            vec![target.clone()]
        } else {
            slots.iter()
                .filter(|s| s.config.role == task.role)
                .map(|s| s.config.id.clone())
                .collect()
        };

        // Phase 1: Try idle slots (skip slots already used in this round)
        for slot_id in &candidates {
            if used_slots.contains(slot_id) {
                continue;
            }
            let status = match state.pty.get_status(slot_id).await {
                Some(s) => s,
                None => continue,
            };
            if status.state != missiond_core::pty::SessionState::Idle {
                continue;
            }

            match state.pty.send_fire_and_forget(slot_id, &task.prompt).await {
                Ok(()) => {
                    let now = chrono::Utc::now().timestamp_millis();
                    let _ = state.mission.db().update_task(
                        &task.id,
                        &missiond_core::types::TaskUpdate {
                            status: Some(missiond_core::types::TaskStatus::Running),
                            slot_id: Some(slot_id.clone()),
                            started_at: Some(now),
                            ..Default::default()
                        },
                    );
                    info!(task_id = %task.id, slot_id = %slot_id, role = %task.role, "Autopilot: dispatched queued submit task");
                    used_slots.insert(slot_id.clone());
                    dispatched = true;
                    any_dispatched = true;
                    break;
                }
                Err(e) => {
                    warn!(task_id = %task.id, slot_id = %slot_id, error = %e, "Autopilot: dispatch to slot failed");
                }
            }
        }

        if !dispatched {
            debug!(task_id = %task.id, role = %task.role, "Autopilot: no idle slot for queued task");
        }
    }

    // Phase 2: Wake-on-Demand — auto-spawn stopped slots for roles that still have queued tasks
    {
        let remaining = match state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Queued) {
            Ok(t) => t,
            Err(_) => vec![],
        };
        let remaining_roles: HashSet<String> = remaining.iter().map(|t| t.role.clone()).collect();
        if !remaining_roles.is_empty() {
            let slots = state.mission.list_slots();
            for role in &remaining_roles {
                for slot in slots.iter().filter(|s| s.config.role == *role) {
                    let slot_id = &slot.config.id;
                    if used_slots.contains(slot_id) { continue; }
                    let status = state.pty.get_status(slot_id).await;
                    let is_spawnable = match &status {
                        Some(s) => s.state == missiond_core::pty::SessionState::Exited,
                        None => true,
                    };
                    if !is_spawnable { continue; }

                    let state_clone = state.clone();
                    let slot_id_clone = slot_id.clone();
                    info!(slot_id = %slot_id, role = %role, "Autopilot: auto-spawning slot for queued tasks (Wake-on-Demand)");
                    tokio::spawn(async move {
                        if ensure_memory_slot_by_id(&state_clone, &slot_id_clone).await {
                            state_clone.submit_notify.notify_one();
                        }
                    });
                    break; // One spawn attempt per role
                }
            }
        }
    }

    any_dispatched
}

/// Reap submit tasks stuck in Running state for too long (15 min).
/// If the slot is Idle, mark Done; otherwise mark Failed after timeout.
pub(crate) async fn reap_stale_submit_tasks(state: &AppState) {
    let running = match state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
        Ok(t) => t,
        Err(_) => return,
    };
    if running.is_empty() {
        return;
    }

    let now = chrono::Utc::now().timestamp_millis();
    const JSONL_CHECK_THRESHOLD_MS: i64 = 2 * 60 * 1000; // 2 minutes: start checking JSONL
    const SUBMIT_TASK_TIMEOUT_MS: i64 = 15 * 60 * 1000; // 15 minutes: hard timeout

    for task in &running {
        let started = task.started_at.unwrap_or(task.created_at);
        let elapsed = now - started;

        if elapsed < JSONL_CHECK_THRESHOLD_MS {
            continue;
        }

        // --- JSONL completion detection (compensating path for missed PTY Idle events) ---
        if elapsed < SUBMIT_TASK_TIMEOUT_MS {
            if let Some(ref slot_id) = task.slot_id {
                // Guard 1: slot's current session must match task's session
                let session_matches = match (
                    state.mission.db().get_slot_session(slot_id),
                    &task.session_id,
                ) {
                    (Ok(Some(current_session)), Some(task_session)) => current_session == *task_session,
                    // No session tracking → can't verify, skip JSONL check
                    _ => false,
                };

                if session_matches {
                    if let Some(jsonl_path) = get_task_jsonl_path(state, task) {
                        let path = std::path::Path::new(&jsonl_path);
                        if missiond_core::jsonl_has_completed_turn(path).await {
                            // JSONL confirms turn completed — extract result and close
                            let result_text = missiond_core::extract_last_assistant_text(path).await
                                .unwrap_or_else(|| "completed (JSONL turn_duration)".to_string());
                            // Safe UTF-8 truncation to 4KB
                            let result_text = if result_text.len() > 4096 {
                                let mut end = 4096;
                                while !result_text.is_char_boundary(end) && end > 0 { end -= 1; }
                                format!("{}...(truncated)", &result_text[..end])
                            } else {
                                result_text
                            };
                            let _ = state.mission.db().update_task(
                                &task.id,
                                &missiond_core::types::TaskUpdate {
                                    status: Some(missiond_core::types::TaskStatus::Done),
                                    finished_at: Some(now),
                                    result: Some(result_text.clone()),
                                    ..Default::default()
                                },
                            );
                            // Update associated kb_operation
                            let _ = state.mission.db().kb_ops_complete_by_task_id(&task.id, "done", Some(&result_text));
                            info!(
                                task_id = %task.id, slot_id = %slot_id,
                                age_min = elapsed / 60000,
                                "Submit task closed via JSONL turn_duration compensation"
                            );
                            continue;
                        }
                    }
                }
            }
            // Not yet at hard timeout and JSONL didn't confirm — wait
            continue;
        }

        // --- Hard timeout (15 min) ---
        let slot_idle = if let Some(ref sid) = task.slot_id {
            state.pty.get_status(sid).await
                .map(|s| s.state == missiond_core::pty::SessionState::Idle)
                .unwrap_or(true) // no session = treat as idle
        } else {
            true
        };

        let (new_status, result_msg) = if slot_idle {
            // Try JSONL result even at timeout
            let jsonl_result = if task.slot_id.is_some() {
                if let Some(jsonl_path) = get_task_jsonl_path(state, task) {
                    missiond_core::extract_last_assistant_text(std::path::Path::new(&jsonl_path)).await
                } else { None }
            } else { None };
            (missiond_core::types::TaskStatus::Done,
             jsonl_result.unwrap_or_else(|| "completed (timeout reaper)".to_string()))
        } else {
            (missiond_core::types::TaskStatus::Failed, "timed out after 15 minutes".to_string())
        };

        let _ = state.mission.db().update_task(
            &task.id,
            &missiond_core::types::TaskUpdate {
                status: Some(new_status),
                finished_at: Some(now),
                result: Some(result_msg.clone()),
                ..Default::default()
            },
        );
        // Update associated kb_operation (done or failed depending on task status)
        let kb_status = if new_status == missiond_core::types::TaskStatus::Done { "done" } else { "failed" };
        if let Ok(true) = state.mission.db().kb_ops_complete_by_task_id(&task.id, kb_status, Some(&result_msg)) {
            info!(task_id = %task.id, kb_status = kb_status, "KB operation updated via reaper");
        }
        warn!(
            task_id = %task.id,
            slot_id = ?task.slot_id,
            status = ?new_status,
            age_min = elapsed / 60000,
            "Reaped stale submit task"
        );
    }
}
