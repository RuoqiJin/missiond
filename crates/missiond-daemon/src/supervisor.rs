
use tracing::{debug, info, warn};

use crate::state::{AppState, ExtractionPhase, ExtractionState, MAX_WAIT_FOR_IDLE_SECS};
use crate::state::SUPERVISOR_SLOT_ID;
use missiond_core::SessionState;
use crate::memory_scheduler::ensure_memory_slot_by_id;
use missiond_core::PTYSpawnOptions;
use std::path::PathBuf;
use crate::slot_env::{build_slot_tracking_env, capture_slot_session_uuid};
use crate::helpers::char_boundary_at;
use std::sync::Arc;
use missiond_core::MissionControl;

pub(crate) async fn check_slot_context_levels(state: &AppState) {
    let slots = state.mission.list_slots();
    // P2: Spawn each slot's check+restart concurrently so total time = max(per-slot)
    // instead of sum(per-slot). Previously N low-context slots blocked N × 123s.
    let mut handles = Vec::new();
    for slot in slots {
        let state = state.clone();
        handles.push(tokio::spawn(async move {
            let status = state.pty.get_status(&slot.config.id).await;
            let Some(info) = status else { return };
            // Only check running slots
            if info.state == SessionState::Exited {
                return;
            }

            let screen = match state.pty.get_screen(&slot.config.id).await {
                Ok(s) => s,
                Err(_) => return,
            };

            // Look for "Context left until auto-compact: XX%"
            if let Some(pct) = extract_context_percentage(&screen) {
                if pct < 10 {
                    warn!(
                        slot_id = %slot.config.id,
                        context_pct = pct,
                        "Slot context critically low, restarting"
                    );
                    // Kill the slot
                    if let Err(e) = state.pty.kill(&slot.config.id).await {
                        warn!(error = %e, slot_id = %slot.config.id, "Failed to kill low-context slot");
                        return;
                    }
                    // Requeue any Running submit tasks assigned to this slot
                    match state.mission.db().requeue_running_tasks_for_slot(&slot.config.id) {
                        Ok(n) if n > 0 => warn!(slot_id = %slot.config.id, count = n, "Requeued running submit tasks after low-context restart"),
                        Err(e) => warn!(slot_id = %slot.config.id, error = %e, "Failed to requeue tasks after low-context restart"),
                        _ => {}
                    }
                    // Release any board task claims held by this slot
                    let _ = state.mission.db().release_board_claims_by_executor(&slot.config.id);
                    // Wait for exit
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                    // Respawn
                    let pty_slot = missiond_core::PTYSlot {
                        id: slot.config.id.clone(),
                        role: slot.config.role.clone(),
                        cwd: slot.config.cwd.as_deref().map(PathBuf::from),
                        engine: slot.config.engine,
                    };
                    let mcp_config = slot.config.mcp_config.clone().map(PathBuf::from);
                    let (extra_env, session_file) = build_slot_tracking_env(&slot.config.id, slot.config.env.as_ref()).await;
                    match state.pty.spawn(&pty_slot, PTYSpawnOptions {
                        auto_restart: false,
                        wait_for_idle: true,
                        timeout_secs: Some(120),
                        mcp_config,
                        dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                        extra_env,
                    }).await {
                        Ok(_) => {
                            capture_slot_session_uuid(&state, &slot.config.id, &session_file).await;
                            info!(slot_id = %slot.config.id, "Slot restarted due to low context");
                        }
                        Err(e) => {
                            warn!(error = %e, slot_id = %slot.config.id, "Failed to respawn slot after context kill");
                        }
                    }
                }
            }
        }));
    }
    // Wait for all slot checks to complete concurrently
    for handle in handles {
        let _ = handle.await;
    }
}

/// Extract context percentage from PTY screen text.
/// Looks for "Context left until auto-compact: XX%" pattern.
/// Truncate a string at a safe UTF-8 char boundary.
/// Strip prompt echo from PTY response.
/// When a prompt is sent to PTY via bracketed paste, the terminal echoes it back.
/// This removes lines that appear in the prompt from the response to reduce noise.
pub(crate) fn strip_prompt_echo(response: &str, prompt: &str) -> String {
    // Build a set of significant prompt lines (skip short/empty ones)
    let prompt_lines: std::collections::HashSet<&str> = prompt.lines()
        .map(|l| l.trim())
        .filter(|l| l.len() > 20) // Only match substantial lines to avoid false positives
        .collect();

    if prompt_lines.is_empty() {
        return response.to_string();
    }

    let mut result = Vec::new();
    let mut consecutive_echo = 0;
    for line in response.lines() {
        let trimmed = line.trim();
        if prompt_lines.contains(trimmed) {
            consecutive_echo += 1;
            // Allow up to 2 echo lines for context, skip the rest
            if consecutive_echo <= 2 {
                continue;
            }
        } else {
            consecutive_echo = 0;
        }
        result.push(line);
    }
    result.join("\n")
}

/// Detect OAuth/auth errors in PTY response content.
/// Claude Code outputs these messages when the OAuth token has expired.
pub(crate) fn is_auth_error(response: &str) -> bool {
    // Check first ~2000 bytes — auth errors appear at the start of output
    let check = &response[..char_boundary_at(response, 2000)];
    let lower = check.to_lowercase();
    lower.contains("oauth token has expired")
        || lower.contains("please run /login")
        || lower.contains("unauthorized")
        || lower.contains("authentication required")
        || lower.contains("token expired")
        || lower.contains("auth error")
}

pub(crate) fn truncate_safe(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub(crate) fn extract_context_percentage(screen: &str) -> Option<u32> {
    // The status bar text: "Context left until auto-compact: 12%"
    for line in screen.lines().rev() {
        if let Some(pos) = line.find("Context left until auto-compact:") {
            let after = &line[pos + "Context left until auto-compact:".len()..];
            let trimmed = after.trim().trim_end_matches('%');
            if let Ok(pct) = trimmed.parse::<u32>() {
                return Some(pct);
            }
        }
    }
    None
}

pub(crate) fn get_task_jsonl_path(state: &AppState, task: &missiond_core::types::Task) -> Option<String> {
    let slot_id = task.slot_id.as_ref()?;
    let session_uuid = state.mission.db().get_slot_session(slot_id).ok()??;
    let conv = state.mission.db().get_conversation(&session_uuid).ok()??;
    conv.jsonl_path
}

/// Supervisor patrol interval (5 minutes).
const SUPERVISOR_PATROL_INTERVAL_SECS: i64 = 300;

/// Schedule a periodic patrol task on the supervisor slot.
/// The supervisor uses Opus model to inspect all slot PTY screens, detect anomalies,
/// and take corrective action (kill misbehaving slots, report issues).
pub(crate) async fn schedule_supervisor_patrol(state: &AppState) {
    let now = chrono::Utc::now().timestamp();
    let last = state.last_supervisor_patrol_at.load(std::sync::atomic::Ordering::Relaxed);
    if now - last < SUPERVISOR_PATROL_INTERVAL_SECS {
        return;
    }

    // Check if supervisor slot exists and is idle
    let status = state.pty.get_status(SUPERVISOR_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        Some(s) => {
            debug!(state = ?s.state, "supervisor: not idle, skipping patrol");
            return;
        }
        None => {
            // Slot not spawned — try to ensure it's running
            ensure_memory_slot_by_id(state, SUPERVISOR_SLOT_ID).await;
            return;
        }
    }

    state.last_supervisor_patrol_at.store(now, std::sync::atomic::Ordering::Relaxed);

    // Collect slot status summary for the patrol prompt
    let all_status = state.pty.get_all_status().await;
    let mut status_lines = Vec::new();
    for info in &all_status {
        if info.slot_id == SUPERVISOR_SLOT_ID {
            continue; // Don't inspect self
        }
        status_lines.push(format!("- {} [{}] state={:?}", info.slot_id, info.role, info.state));
    }
    let status_summary = status_lines.join("\n");

    let prompt = format!(
        "定期巡检。以下是当前所有工位的状态快照：\n\n\
         {status_summary}\n\n\
         ## 巡检任务\n\
         1. 对状态为 Running 或 Thinking 的工位，用 mission_pty_screen 查看最近输出\n\
         2. 检查是否存在以下异常：\n\
            - 直接用 sqlite3/bash 操作 mission.db（严重！立即 mission_kill）\n\
            - MCP 工具连续报错或不可用\n\
            - 长时间无输出或陷入循环\n\
            - 工位偏离任务目标\n\
         3. 对 idle 工位无需检查，它们正常待命\n\
         4. 如果所有工位正常，简短报告「巡检完成，无异常」即可\n\n\
         ## 干预措施\n\
         - 发现严重异常 → mission_kill(slotId) 立即终止 + mission_kb_remember 记录\n\
         - 发现潜在问题 → mission_board_create 建任务跟进\n\
         - 所有正常 → 不需要任何操作\n\n\
         注意：保持巡检高效，不要花超过 2 分钟。只看 Running/Thinking 的工位。"
    );

    info!(slots = all_status.len(), "Dispatching supervisor patrol");

    let pty = Arc::clone(&state.pty);
    tokio::spawn(async move {
        match pty.send(SUPERVISOR_SLOT_ID, &prompt, 180_000).await {
            Ok(res) => {
                info!(
                    response_len = res.response.len(),
                    "Supervisor patrol completed"
                );
            }
            Err(e) => {
                warn!(error = %e, "Supervisor patrol failed");
            }
        }
    });
}

/// Threshold for considering the memory slot stuck (10 minutes).
const STUCK_THRESHOLD_SECS: i64 = 600;

/// Detect and recover from a memory slot stuck in non-Idle state.
pub(crate) async fn check_slot_stuck(
    state: &AppState,
    slot_id: &str,
    busy_since_atomic: &std::sync::atomic::AtomicI64,
    extraction_state: &tokio::sync::RwLock<ExtractionState>,
) {
    let now = chrono::Utc::now().timestamp();
    let busy_since = busy_since_atomic.load(std::sync::atomic::Ordering::SeqCst);

    // Poll actual slot state: if slot is non-Idle but busy_since is 0 (lost track),
    // re-initialize busy_since so stuck detection can work.
    if busy_since == 0 {
        let is_busy = state.pty.get_status(slot_id).await
            .map(|s| s.state != SessionState::Idle)
            .unwrap_or(false);
        if is_busy {
            busy_since_atomic.store(now, std::sync::atomic::Ordering::SeqCst);
            debug!(slot_id, "slot_stuck: re-initialized busy_since (slot is non-Idle but was untracked)");
        }
        return;
    }

    let stuck_duration = now - busy_since;
    if stuck_duration < STUCK_THRESHOLD_SECS {
        return;
    }

    let status = state.pty.get_status(slot_id).await;
    let current_state = status.as_ref()
        .map(|s| format!("{:?}", s.state))
        .unwrap_or_else(|| "unknown".to_string());

    // If slot is actually Idle, just clear the counter
    if status.as_ref().map(|s| s.state == SessionState::Idle).unwrap_or(false) {
        busy_since_atomic.store(0, std::sync::atomic::Ordering::SeqCst);
        return;
    }

    // P2: Check JSONL activity before killing — slot may be doing bulk MCP tool_use
    // which shows as Thinking the entire time (especially with MiniMax M2.5)
    if let Ok(Some(session_uuid)) = state.mission.db().get_slot_session(slot_id) {
        if let Ok(Some(conv)) = state.mission.db().get_conversation(&session_uuid) {
            if let Some(ref jsonl_path) = conv.jsonl_path {
                if let Ok(metadata) = std::fs::metadata(jsonl_path) {
                    if let Ok(modified) = metadata.modified() {
                        let elapsed = modified.elapsed().unwrap_or_default();
                        if elapsed.as_secs() < 120 {
                            info!(
                                slot_id,
                                stuck_secs = stuck_duration,
                                jsonl_activity_secs_ago = elapsed.as_secs(),
                                "Slot appears stuck but JSONL is active, deferring kill"
                            );
                            return;
                        }
                    }
                }
            }
        }
    }

    warn!(
        slot_id,
        state = %current_state,
        stuck_secs = stuck_duration,
        "Memory slot stuck, killing and respawning"
    );

    // Kill and respawn instead of just Ctrl+C (Ctrl+C often doesn't recover)
    if let Err(e) = state.pty.kill(slot_id).await {
        warn!(slot_id, error = %e, "Failed to kill stuck memory slot");
    }

    // Requeue any Running submit tasks assigned to this slot — they were lost with the old session
    match state.mission.db().requeue_running_tasks_for_slot(slot_id) {
        Ok(n) if n > 0 => warn!(slot_id, count = n, "Requeued running submit tasks after slot restart"),
        Err(e) => warn!(slot_id, error = %e, "Failed to requeue tasks after slot restart"),
        _ => {}
    }
    // Release board task claims held by this slot
    let _ = state.mission.db().release_board_claims_by_executor(slot_id);

    // Allow next tick to respawn
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let _ = ensure_memory_slot_by_id(state, slot_id).await;

    // Reset extraction state so next tick can trigger
    {
        let mut es = extraction_state.write().await;
        // Mark deep analysis as failed so it increments retry count (not silently lost)
        if let Some(conv_id) = es.current_deep_conv_id.take() {
            let _ = state.mission.db().mark_analysis_failed(&conv_id);
            warn!(conv_id = %conv_id, "Marked stuck deep analysis as failed");
        }
        // Mark slot task as failed (stuck)
        if let Some(ref st_id) = es.current_slot_task_id {
            let _ = state.mission.db().slot_task_set_failed(st_id, "slot stuck, force reset");
        }
        es.phase = ExtractionPhase::Idle;
        es.active_type = None;
        es.current_task_id = None;
        es.current_slot_task_id = None;
        es.is_checkpoint = false;
        es.checkpoint_message_id = None;
        // Advance realtime watermarks before clearing — same fix as check_extraction_gate.
        if matches!(es.active_type, Some("realtime")) && !es.watermark_targets.is_empty() {
            let db = state.mission.db();
            for (session_id, timestamp) in &es.watermark_targets {
                let _ = db.update_realtime_forwarded_at(session_id, timestamp);
            }
            warn!(
                slot_id,
                sessions = es.watermark_targets.len(),
                "Advanced watermarks before slot kill (preventing stall)"
            );
        }
        es.watermark_targets.clear();
    }
    // Don't reset to 0 — set to now so we can detect if respawn also gets stuck
    busy_since_atomic.store(now, std::sync::atomic::Ordering::SeqCst);
}

/// Check extraction state gate: returns true if extraction is allowed.
/// Handles safety-valve timeout for WaitingForSlotIdle phase.
pub(crate) async fn check_extraction_gate(
    extraction_state: &tokio::sync::RwLock<ExtractionState>,
    mission: &MissionControl,
    label: &str,
) -> bool {
    let now = chrono::Utc::now().timestamp();
    let es = extraction_state.read().await;
    if es.phase == ExtractionPhase::Idle {
        return true;
    }
    let age = now - es.phase_started_at;
    // Safety valve: force reset if WaitingForSlotIdle exceeds MAX_WAIT_FOR_IDLE_SECS
    if es.phase == ExtractionPhase::WaitingForSlotIdle && age > MAX_WAIT_FOR_IDLE_SECS {
        drop(es);
        let mut es = extraction_state.write().await;
        if es.phase == ExtractionPhase::WaitingForSlotIdle {
            warn!(age_secs = age, "{}: extraction stuck in WaitingForSlotIdle, forcing reset", label);
            // Mark deep analysis as failed (increments retry count) instead of silently losing
            if let Some(conv_id) = es.current_deep_conv_id.take() {
                let _ = mission.db().mark_analysis_failed(&conv_id);
            }
            // Mark slot task as failed (timeout)
            if let Some(ref st_id) = es.current_slot_task_id {
                let _ = mission.db().slot_task_set_failed(st_id, "WaitingForSlotIdle timeout");
            }
            es.phase = ExtractionPhase::Idle;
            es.active_type = None;
            es.current_task_id = None;
            es.current_slot_task_id = None;
            es.is_checkpoint = false;
            es.checkpoint_message_id = None;
            // Advance realtime watermarks before clearing — prevents permanent
            // watermark stall. Messages were already sent to slot; even if
            // processing was incomplete, not advancing causes 100% message leak.
            if matches!(es.active_type, Some("realtime")) && !es.watermark_targets.is_empty() {
                let db = mission.db();
                for (session_id, timestamp) in &es.watermark_targets {
                    let _ = db.update_realtime_forwarded_at(session_id, timestamp);
                }
                warn!(
                    sessions = es.watermark_targets.len(),
                    "{}: advanced watermarks on timeout (preventing stall)", label
                );
            }
            es.watermark_targets.clear();
            return true;
        }
    } else {
        debug!(
            phase = ?es.phase,
            active_type = ?es.active_type,
            age_secs = age,
            "{}: skipped (extraction in progress)", label
        );
    }
    false
}
