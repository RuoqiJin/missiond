use tracing::{debug, info, warn};

use crate::helpers::char_boundary_at;
use crate::memory_scheduler::ensure_memory_slot_by_id;
use crate::state::SUPERVISOR_SLOT_ID;
use crate::state::{AppState, ExtractionPhase, ExtractionState, MAX_WAIT_FOR_IDLE_SECS};
use missiond_core::SessionState;
use std::sync::Arc;

/// Threshold: mark for graceful restart when context drops below this %.
const COMPACT_GRACEFUL_THRESHOLD: u32 = 15;
/// Emergency threshold: force-kill immediately regardless of slot state.
const COMPACT_EMERGENCY_THRESHOLD: u32 = 3;

pub(crate) async fn check_slot_context_levels(state: &AppState) {
    let slots = state.mission.list_slots();
    let mut handles = Vec::new();
    for slot in slots {
        let state = state.clone();
        handles.push(tokio::spawn(async move {
            let status = state.pty.get_status(&slot.config.id).await;
            let Some(info) = status else { return };
            if info.state == SessionState::Exited {
                return;
            }

            let screen = match state.pty.get_screen(&slot.config.id).await {
                Ok(s) => s,
                Err(_) => return,
            };

            if let Some(pct) = extract_context_percentage(&screen) {
                if pct < COMPACT_EMERGENCY_THRESHOLD {
                    // Emergency: context about to hit zero, force-kill immediately
                    warn!(
                        slot_id = %slot.config.id,
                        context_pct = pct,
                        "Slot context critically low, emergency kill"
                    );
                    if let Err(e) = state.pty.kill(&slot.config.id).await {
                        warn!(error = %e, slot_id = %slot.config.id, "Failed to emergency kill slot");
                        return;
                    }
                    match state.store.requeue_running_tasks_for_slot(&slot.config.id).await {
                        Ok(n) if n > 0 => warn!(slot_id = %slot.config.id, count = n, "Requeued tasks after emergency kill"),
                        Err(e) => warn!(slot_id = %slot.config.id, error = %e, "Failed to requeue after emergency kill"),
                        _ => {}
                    }
                    let _ = state.store.release_board_claims_by_executor(&slot.config.id).await;
                    // Clear from pending set (will be respawned by ensure_memory_slot or autopilot)
                    state.pending_compact_restart.lock().unwrap().remove(&slot.config.id);
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let _ = ensure_memory_slot_by_id(&state, &slot.config.id).await;
                } else if pct < COMPACT_GRACEFUL_THRESHOLD {
                    // Graceful: mark for restart when slot becomes Idle
                    let mut pending = state.pending_compact_restart.lock().unwrap();
                    if pending.insert(slot.config.id.clone()) {
                        warn!(
                            slot_id = %slot.config.id,
                            context_pct = pct,
                            "Slot context low, marked for graceful restart on idle"
                        );
                    }
                }
            }
        }));
    }
    for handle in handles {
        let _ = handle.await;
    }
}

/// Restart slots that were marked for compact restart once they become Idle.
/// Must be called BEFORE any task dispatch to avoid missing the Idle window.
pub(crate) async fn check_pending_compact_restarts(state: &AppState) {
    let pending: Vec<String> = {
        let set = state.pending_compact_restart.lock().unwrap();
        if set.is_empty() {
            return;
        }
        set.iter().cloned().collect()
    };

    for slot_id in pending {
        let status = state.pty.get_status(&slot_id).await;
        let is_idle = status
            .as_ref()
            .map(|s| s.state == SessionState::Idle)
            .unwrap_or(false);

        if !is_idle {
            continue;
        }

        // Acquire dispatch guard to prevent task dispatch racing with restart
        if !state.slot_dispatch.try_acquire(&slot_id) {
            debug!(slot_id = %slot_id, "Compact restart: dispatch guard busy, deferring");
            continue;
        }

        // Re-verify idle after acquiring guard (double-check)
        let still_idle = state
            .pty
            .get_status(&slot_id)
            .await
            .map(|s| s.state == SessionState::Idle)
            .unwrap_or(false);
        if !still_idle {
            state.slot_dispatch.release(&slot_id);
            continue;
        }

        info!(slot_id = %slot_id, "Slot idle with pending compact restart, restarting now");

        // Kill
        if let Err(e) = state.pty.kill(&slot_id).await {
            warn!(error = %e, slot_id = %slot_id, "Failed to kill slot for compact restart");
            state.slot_dispatch.release(&slot_id);
            continue;
        }
        // Requeue any Running submit tasks
        match state.store.requeue_running_tasks_for_slot(&slot_id).await {
            Ok(n) if n > 0 => {
                warn!(slot_id = %slot_id, count = n, "Requeued running submit tasks after compact restart")
            }
            Err(e) => {
                warn!(slot_id = %slot_id, error = %e, "Failed to requeue tasks after compact restart")
            }
            _ => {}
        }
        let _ = state.store.release_board_claims_by_executor(&slot_id).await;

        // Release dispatch guard before blocking sleep+respawn
        state.slot_dispatch.release(&slot_id);

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Respawn
        let _ = ensure_memory_slot_by_id(state, &slot_id).await;

        // Remove from pending set
        state
            .pending_compact_restart
            .lock()
            .unwrap()
            .remove(&slot_id);

        info!(slot_id = %slot_id, "Slot restarted due to compact (graceful)");
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
    let prompt_lines: std::collections::HashSet<&str> = prompt
        .lines()
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

/// Detect Claude API quota/usage exhaustion in PTY response content.
/// Checks the TAIL of the response (errors appear at end of PTY output).
pub(crate) fn is_quota_exhausted(response: &str) -> bool {
    // Check last ~8KB — PTY errors appear at the end, after command echo/spinners
    let len = response.len();
    let check_start = if len > 8000 {
        let mut start = len - 8000;
        while start < len && !response.is_char_boundary(start) {
            start += 1;
        }
        start
    } else {
        0
    };
    let lower = response[check_start..].to_lowercase();
    // Hard quota / billing exhaustion (should NOT auto-recover)
    lower.contains("out of extra usage")
        || lower.contains("usage limit exceeded")
        || lower.contains("you've exceeded")
        || lower.contains("quota exceeded")
        || lower.contains("billing limit")
        || lower.contains("spending limit")
        || lower.contains("credit balance")
        || lower.contains("insufficient_quota")
        || lower.contains("exceeded your current quota")
        || lower.contains("over_limit_error")
        || lower.contains("402 payment required")
        || lower.contains("credit balance is too low")
        || lower.contains("organization has been restricted")
        // Rate limit (temporary, but still should pause to prevent cascade)
        || lower.contains("rate limit exceeded")
        || lower.contains("rate_limit_error")
        || lower.contains("429 too many requests")
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

pub(crate) async fn get_task_jsonl_path(
    state: &AppState,
    task: &missiond_core::types::Task,
) -> Option<String> {
    let slot_id = task.slot_id.as_ref()?;
    let session_uuid = state.store.get_slot_session(slot_id).await.ok()??;
    let conv = state.store.get_conversation(&session_uuid).await.ok()??;
    conv.jsonl_path
}

/// Supervisor patrol interval (5 minutes).
const SUPERVISOR_PATROL_INTERVAL_SECS: i64 = 300;

/// Schedule a periodic patrol task on the supervisor slot.
// @beacon: slot
/// The supervisor uses Opus model to inspect all slot PTY screens, detect anomalies,
/// and take corrective action (kill misbehaving slots, report issues).
///
/// V3 gating: the supervisor patrol is only active when `slot-supervisor` is
/// registered as a worker through the V3 workstation-pool / runtime config.
/// Absent a workstation-pool entry, the patrol stays inert — we no longer
/// repeatedly call ensure_memory_slot_by_id and emit "Memory slot not
/// configured in slots.yaml" (slots.yaml is retired in V3).
pub(crate) async fn schedule_supervisor_patrol(state: &AppState) {
    // V3 SSOT: supervisor must be projected from workstation-pool / runtime config.
    let supervisor_registered = state
        .mission
        .list_slots()
        .into_iter()
        .any(|s| s.config.id == SUPERVISOR_SLOT_ID);
    if !supervisor_registered {
        return;
    }

    let now = chrono::Utc::now().timestamp();
    let last = state
        .last_supervisor_patrol_at
        .load(std::sync::atomic::Ordering::Relaxed);
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

    state
        .last_supervisor_patrol_at
        .store(now, std::sync::atomic::Ordering::Relaxed);

    // Collect slot status summary for the patrol prompt
    let all_status = state.pty.get_all_status().await;
    let mut status_lines = Vec::new();
    for info in &all_status {
        if info.slot_id == SUPERVISOR_SLOT_ID {
            continue; // Don't inspect self
        }
        status_lines.push(format!(
            "- {} [{}] state={:?}",
            info.slot_id, info.role, info.state
        ));
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

/// Absolute ceiling: kill slot regardless of JSONL activity (90 minutes).
/// Prevents infinite loops where model produces tool calls that keep JSONL active.
const ABSOLUTE_STUCK_CEILING_SECS: i64 = 90 * 60;

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
        let is_busy = state
            .pty
            .get_status(slot_id)
            .await
            .map(|s| s.state != SessionState::Idle)
            .unwrap_or(false);
        if is_busy {
            busy_since_atomic.store(now, std::sync::atomic::Ordering::SeqCst);
            debug!(
                slot_id,
                "slot_stuck: re-initialized busy_since (slot is non-Idle but was untracked)"
            );
        }
        return;
    }

    let stuck_duration = now - busy_since;
    if stuck_duration < STUCK_THRESHOLD_SECS {
        return;
    }

    let status = state.pty.get_status(slot_id).await;
    let current_state = status
        .as_ref()
        .map(|s| format!("{:?}", s.state))
        .unwrap_or_else(|| "unknown".to_string());

    // If slot is actually Idle, just clear the counter
    if status
        .as_ref()
        .map(|s| s.state == SessionState::Idle)
        .unwrap_or(false)
    {
        busy_since_atomic.store(0, std::sync::atomic::Ordering::SeqCst);
        return;
    }

    // P2: Check JSONL activity before killing — slot may be doing bulk MCP tool_use
    // which shows as Thinking the entire time (especially with MiniMax M2.5).
    // However, enforce an absolute ceiling to prevent infinite loops where the model
    // produces useless tool calls that keep JSONL active indefinitely.
    if stuck_duration < ABSOLUTE_STUCK_CEILING_SECS {
        if let Ok(Some(session_uuid)) = state.store.get_slot_session(slot_id).await {
            if let Ok(Some(conv)) = state.store.get_conversation(&session_uuid).await {
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
    } else {
        warn!(
            slot_id,
            stuck_secs = stuck_duration,
            ceiling = ABSOLUTE_STUCK_CEILING_SECS,
            "Slot exceeded absolute stuck ceiling despite JSONL activity, force killing"
        );
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
    // Clear from compact restart pending set (slot is being rebuilt)
    state
        .pending_compact_restart
        .lock()
        .unwrap()
        .remove(slot_id);

    // Requeue any Running submit tasks assigned to this slot — they were lost with the old session
    match state.store.requeue_running_tasks_for_slot(slot_id).await {
        Ok(n) if n > 0 => warn!(
            slot_id,
            count = n,
            "Requeued running submit tasks after slot restart"
        ),
        Err(e) => warn!(slot_id, error = %e, "Failed to requeue tasks after slot restart"),
        _ => {}
    }
    // Release board task claims held by this slot
    let _ = state.store.release_board_claims_by_executor(slot_id).await;

    // Allow next tick to respawn
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let _ = ensure_memory_slot_by_id(state, slot_id).await;

    // Reset extraction state so next tick can trigger
    {
        let mut es = extraction_state.write().await;
        // Mark deep analysis as failed so it increments retry count (not silently lost)
        if let Some(conv_id) = es.current_deep_conv_id.take() {
            let _ = state.store.mark_analysis_failed(&conv_id).await;
            warn!(conv_id = %conv_id, "Marked stuck deep analysis as failed");
        }
        // Mark slot task as failed (stuck)
        if let Some(ref st_id) = es.current_slot_task_id {
            let _ = state
                .store
                .slot_task_set_failed(st_id, "slot stuck, force reset")
                .await;
        }
        // Advance realtime watermarks BEFORE clearing active_type (P0 fix: use-after-clear)
        let was_realtime = matches!(es.active_type, Some("realtime"));
        if was_realtime && !es.watermark_targets.is_empty() {
            for (session_id, timestamp) in &es.watermark_targets {
                if let Err(e) = state
                    .store
                    .update_realtime_forwarded_at(session_id, timestamp)
                    .await
                {
                    warn!(slot_id, session_id, error = %e, "Failed to advance watermark on slot kill");
                }
            }
            warn!(
                slot_id,
                sessions = es.watermark_targets.len(),
                "Advanced watermarks before slot kill (preventing stall)"
            );
        }
        es.watermark_targets.clear();
        es.phase = ExtractionPhase::Idle;
        es.active_type = None;
        es.current_task_id = None;
        es.current_slot_task_id = None;
        es.is_checkpoint = false;
        es.checkpoint_message_id = None;
        es.pending_served = false;
    }
    // Don't reset to 0 — set to now so we can detect if respawn also gets stuck
    busy_since_atomic.store(now, std::sync::atomic::Ordering::SeqCst);
}

/// Check extraction state gate: returns true if extraction is allowed.
/// Handles safety-valve timeout for WaitingForSlotIdle phase.
pub(crate) async fn check_extraction_gate(
    extraction_state: &tokio::sync::RwLock<ExtractionState>,
    state: &AppState,
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
            warn!(
                age_secs = age,
                "{}: extraction stuck in WaitingForSlotIdle, forcing reset", label
            );
            // Mark deep analysis as failed (increments retry count) instead of silently losing
            if let Some(conv_id) = es.current_deep_conv_id.take() {
                let _ = state.store.mark_analysis_failed(&conv_id).await;
            }
            // Mark slot task as failed (timeout)
            if let Some(ref st_id) = es.current_slot_task_id {
                let _ = state
                    .store
                    .slot_task_set_failed(st_id, "WaitingForSlotIdle timeout")
                    .await;
            }
            // Advance realtime watermarks BEFORE clearing active_type (P0 fix: use-after-clear)
            let was_realtime = matches!(es.active_type, Some("realtime"));
            if was_realtime && !es.watermark_targets.is_empty() {
                for (session_id, timestamp) in &es.watermark_targets {
                    if let Err(e) = state
                        .store
                        .update_realtime_forwarded_at(session_id, timestamp)
                        .await
                    {
                        warn!(session_id, error = %e, "{}: failed to advance watermark on timeout", label);
                    }
                }
                warn!(
                    sessions = es.watermark_targets.len(),
                    "{}: advanced watermarks on timeout (preventing stall)", label
                );
            }
            es.watermark_targets.clear();
            es.phase = ExtractionPhase::Idle;
            es.active_type = None;
            es.current_task_id = None;
            es.current_slot_task_id = None;
            es.is_checkpoint = false;
            es.checkpoint_message_id = None;
            es.pending_served = false;
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
