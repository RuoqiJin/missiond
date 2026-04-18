//! Historical Scanner — batch scan past conversations for user operation habits.
//!
//! Runs at lowest priority (after all other learning tasks), only when system is idle.
//! Scans conversations that have never been analyzed for habits (habit_scanned_at IS NULL).

use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::engine::intent_engine::request_execution_slot;
use crate::event_bus::DaemonEvent;
use crate::state::{AppState, MEMORY_SLOW_SLOT_ID};
use missiond_core::SessionState;

/// Cadence guard key for daemon_state.
const LAST_HABIT_SCAN_KEY: &str = "last_habit_scan_at";
/// Minimum interval between scans (4 hours).
const HABIT_SCAN_INTERVAL_SECS: i64 = 4 * 3600;
/// Max conversations per scan batch.
const BATCH_SIZE: usize = 5;

/// Check if we should run a historical habit scan.
/// Gates: 4h cadence, no high-priority work, slow slot idle, unscanned conversations exist.
pub(crate) async fn check_historical_scan(state: &AppState) {
    // Gate 1: 4h cadence
    let now = chrono::Utc::now().timestamp();
    let last = state
        .store
        .daemon_state_get(LAST_HABIT_SCAN_KEY)
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last < HABIT_SCAN_INTERVAL_SECS {
        return;
    }

    // Gate 2: feature enabled (can be disabled at runtime)
    if std::env::var("MISSIOND_DISABLE_HABIT_SCAN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return;
    }

    // Gate 3: no high/medium priority open work
    if let Ok(urgent) = state
        .store
        .count_open_tasks_by_priority(&["high", "medium"])
        .await
    {
        if urgent > 0 {
            debug!(urgent, "habit_scan: skipping, urgent tasks pending");
            return;
        }
    }

    // Gate 4: check unscanned count
    let unscanned = match state.store.count_unscanned_conversations().await {
        Ok(n) => n,
        Err(_) => return,
    };
    if unscanned == 0 {
        debug!("habit_scan: all conversations scanned");
        // Update cadence to avoid repeated DB queries
        let _ = state.store.daemon_state_set(LAST_HABIT_SCAN_KEY, now).await;
        return;
    }

    // Gate 5: slow slot idle
    if !request_execution_slot(state, MEMORY_SLOW_SLOT_ID).await {
        debug!("habit_scan: slow slot not available");
        return;
    }
    let status = state.pty.get_status(MEMORY_SLOW_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        _ => {
            debug!("habit_scan: slow slot not idle");
            return;
        }
    }

    // Fetch batch of unscanned conversations
    let convs = match state.store.get_unscanned_conversations(BATCH_SIZE).await {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => return,
        Err(e) => {
            warn!(error = %e, "habit_scan: failed to fetch unscanned conversations");
            return;
        }
    };

    // Update cadence immediately to prevent re-entry
    let _ = state.store.daemon_state_set(LAST_HABIT_SCAN_KEY, now).await;

    let session_ids: Vec<String> = convs.iter().map(|c| c.id.clone()).collect();
    let batch_size = convs.len();

    info!(
        batch = batch_size,
        remaining = unscanned,
        "Historical habit scan: starting batch"
    );

    // Build the prompt with session IDs for the memory slot to fetch
    let session_list = session_ids
        .iter()
        .map(|id| format!("- {}", id))
        .collect::<Vec<_>>()
        .join("\n");

    let habits_prompt = state.prompts.extraction_habits();
    let prompt = format!(
        "{}\n\n---\n\n请依次分析以下 {} 个会话的用户消息，提取操作习惯:\n{}\n\n\
         使用 mission_conversation_get(sessionId=..., includeMessages=true) 获取每个会话内容。\n\
         每个会话分析完后继续下一个。全部完成后输出简短总结（提取了 N 条习惯）。",
        habits_prompt, batch_size, session_list
    );

    // Dispatch to slow memory slot
    let ev_dispatch = DaemonEvent::SlotTaskDispatched {
        slot_id: MEMORY_SLOW_SLOT_ID.to_string(),
        task_id: None,
        purpose: "habit_scan".to_string(),
        prompt_chars: prompt.len(),
        preview: format!(
            "Habit scan: {} sessions ({} remaining)",
            batch_size, unscanned
        ),
        cited_kb_ids: vec![],
    };
    state.event_bus.publish(ev_dispatch.clone());
    let _ = crate::bus::publish_v1_shim(&state.bus, &ev_dispatch).await;

    let pty = Arc::clone(&state.pty);
    let store = Arc::clone(&state.store);
    let event_bus = state.event_bus.clone();
    let bus = Arc::clone(&state.bus);
    let ids = session_ids.clone();

    tokio::spawn(async move {
        let timeout_ms = 600_000u64; // 10 min for batch
        match pty.send(MEMORY_SLOW_SLOT_ID, &prompt, timeout_ms).await {
            Ok(result) => {
                info!(
                    response_len = result.response.len(),
                    sessions = ids.len(),
                    "Habit scan batch completed"
                );
                // Mark all sessions as scanned
                for id in &ids {
                    if let Err(e) = store.mark_habit_scanned(id).await {
                        warn!(session_id = id, error = %e, "Failed to mark habit scanned");
                    }
                }
                let ev = DaemonEvent::MemoryPhaseChanged {
                    slot_id: MEMORY_SLOW_SLOT_ID.to_string(),
                    phase: "Idle".to_string(),
                    active_type: Some("habit_scan_complete".to_string()),
                };
                event_bus.publish(ev.clone());
                let _ = crate::bus::publish_v1_shim(&bus, &ev).await;
            }
            Err(e) => {
                warn!(error = %e, "Habit scan batch failed");
                // Still mark as scanned to prevent infinite retry on broken sessions
                for id in &ids {
                    let _ = store.mark_habit_scanned(id).await;
                }
            }
        }
    });
}
