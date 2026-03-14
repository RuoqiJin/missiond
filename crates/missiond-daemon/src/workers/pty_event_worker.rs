//! PTY Event Stream Worker — processes PTY manager broadcast events.
//!
//! Handles all PTY lifecycle events:
//! - TextComplete: save last response + delegate to message handler
//! - Exited: session cleanup + embedding trigger
//! - StateChange: memory extraction lane management + submit task closure
//! - ConfirmRequired: auto-approve tools for worker slots
//! - McpToolError: incident creation for MCP failures

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::event_bus;
use crate::state::{
    AppState, EmbeddingTask, ExtractionPhase,
    CURRENT_ANALYSIS_VERSION, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID,
};
use crate::supervisor::get_task_jsonl_path;
use crate::infra::message_handler::handle_pty_text_complete;

pub(crate) struct PtyEventWorker {
    pub pty_rx: broadcast::Receiver<missiond_core::ManagerEvent>,
}

impl super::BackgroundWorker for PtyEventWorker {
    fn name(&self) -> &'static str { "pty_events" }

    async fn run(self, state: Arc<AppState>, _ctx: super::WorkerContext) {
        let mut rx = self.pty_rx;
        loop {
            match rx.recv().await {
                Ok(missiond_core::ManagerEvent::TextComplete { slot_id, turn_id, content, timestamp }) => {
                    if !content.is_empty() {
                        state.slot_last_responses.write().await.insert(slot_id.clone(), content.clone());
                    }
                    handle_pty_text_complete(&state, slot_id, turn_id, content, timestamp);
                }
                Ok(missiond_core::ManagerEvent::Exited { slot_id, exit_code }) => {
                    handle_exited(&state, &slot_id, exit_code).await;
                }
                Ok(missiond_core::ManagerEvent::StateChange { ref slot_id, new_state, prev_state }) => {
                    handle_state_change(&state, slot_id, new_state, prev_state).await;
                }
                Ok(missiond_core::ManagerEvent::ConfirmRequired { slot_id, prompt: _, tool_info }) => {
                    handle_confirm_required(&state, &slot_id, tool_info).await;
                }
                Ok(missiond_core::ManagerEvent::McpToolError { slot_id, tool_name, error }) => {
                    handle_mcp_tool_error(&state, &slot_id, &tool_name, &error);
                }
                Ok(missiond_core::ManagerEvent::Spawned { .. }) => {}
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "PTY logger lagged");
                }
                Err(_) => {}
            }
        }
    }
}

async fn handle_exited(s: &AppState, slot_id: &str, exit_code: i32) {
    info!(slot_id = %slot_id, exit_code = exit_code, "PTY session exited");
    let old_uuid = s.mission.db().get_slot_session(slot_id).unwrap_or(None);
    if let Some(ref uuid) = old_uuid {
        let _ = s.mission.db().complete_conversation(uuid);
        let _ = s.embedding_tx.try_send(EmbeddingTask::ProcessSession(uuid.clone()));
        s.pty_session_uuids.write().await.remove(uuid);
    }
    s.mission.db().clear_slot_session(slot_id);
}

async fn handle_state_change(
    s: &AppState,
    slot_id: &str,
    new_state: missiond_core::SessionState,
    prev_state: missiond_core::SessionState,
) {
    // Publish slot state change with trace context
    let trace_id = s.mission.db().get_slot_session(slot_id).ok().flatten();
    s.event_bus.publish_traced(
        event_bus::DaemonEvent::SlotStateChanged {
            slot_id: slot_id.to_string(),
            new_state: format!("{:?}", new_state),
            prev_state: format!("{:?}", prev_state),
        },
        event_bus::TraceContext {
            trace_id,
            summary: Some(format!("{}: {:?} → {:?}", slot_id, prev_state, new_state)),
            ..Default::default()
        },
    );

    // Route memory slot state changes to the correct lane
    handle_memory_lane_state(s, slot_id, new_state, prev_state).await;

    // Close Running submit tasks when slot returns to Idle
    if new_state == missiond_core::SessionState::Idle && prev_state != missiond_core::SessionState::Idle {
        handle_submit_task_closure(s, slot_id).await;

        // Emit SlotBecameIdle for ALL slots (not just memory).
        // Memory slots already emit via handle_memory_lane_state; non-memory slots need this
        // to trigger event-driven board dispatch instead of waiting for 60s autopilot tick.
        if slot_id != MEMORY_SLOT_ID && slot_id != MEMORY_SLOW_SLOT_ID {
            s.event_bus.publish(event_bus::DaemonEvent::SlotBecameIdle {
                slot_id: slot_id.to_string(),
            });
        }
        // Signal autopilot to try board dispatch immediately
        s.board_dispatch_notify.notify_one();
    }
}

async fn handle_memory_lane_state(
    s: &AppState,
    slot_id: &str,
    new_state: missiond_core::SessionState,
    prev_state: missiond_core::SessionState,
) {
    let lane = if slot_id == MEMORY_SLOT_ID {
        Some(("fast", &s.extraction_state, &s.memory_slot_busy_since))
    } else if slot_id == MEMORY_SLOW_SLOT_ID {
        Some(("slow", &s.slow_extraction_state, &s.slow_slot_busy_since))
    } else {
        return;
    };

    let (lane_name, es_lock, busy_since) = lane.unwrap();

    if new_state == missiond_core::SessionState::Idle {
        busy_since.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut es = es_lock.write().await;
        if es.phase == ExtractionPhase::WaitingForSlotIdle || es.phase == ExtractionPhase::Sending {
            let phase_age = chrono::Utc::now().timestamp() - es.phase_started_at;
            if phase_age < 3 {
                debug!(lane = lane_name, phase_age, "Ignoring early Idle transition (likely spawn init)");
            } else {
                let is_realtime = matches!(es.active_type, Some("realtime"));
                info!(
                    lane = lane_name,
                    extraction_type = ?es.active_type,
                    phase_age,
                    "Extraction complete: slot returned to Idle"
                );
                if is_realtime {
                    if !es.watermark_targets.is_empty() {
                        let db = s.mission.db();
                        for (session_id, timestamp) in &es.watermark_targets {
                            let _ = db.update_realtime_forwarded_at(session_id, timestamp);
                        }
                        info!(sessions = es.watermark_targets.len(), "Realtime: advanced watermarks");
                        es.watermark_targets.clear();
                    }
                }
                if matches!(es.active_type, Some("deep_analysis")) {
                    if let Some(conv_id) = es.current_deep_conv_id.take() {
                        if es.is_checkpoint {
                            if let Some(msg_id) = es.checkpoint_message_id.take() {
                                if let Err(e) = s.mission.db().update_deep_checkpoint(&conv_id, msg_id) {
                                    warn!(conv_id = %conv_id, error = %e, "Failed to advance checkpoint watermark");
                                } else {
                                    info!(conv_id = %conv_id, msg_id, "Deep analysis checkpoint: advanced watermark");
                                }
                            }
                        } else {
                            if let Err(e) = s.mission.db().mark_analysis_complete(&conv_id, CURRENT_ANALYSIS_VERSION) {
                                warn!(conv_id = %conv_id, error = %e, "Failed to mark analysis complete");
                            } else {
                                info!(conv_id = %conv_id, version = CURRENT_ANALYSIS_VERSION, "Deep analysis: marked complete");
                            }
                        }
                    }
                }
                if let Some(ref st_id) = es.current_slot_task_id {
                    let _ = s.mission.db().slot_task_set_completed(st_id, 0);
                }
                let mem_trace_id = es.current_deep_conv_id.clone()
                    .or_else(|| es.current_task_id.clone());
                s.event_bus.publish_traced(
                    event_bus::DaemonEvent::MemoryPhaseChanged {
                        slot_id: slot_id.to_string(),
                        phase: "Idle".to_string(),
                        active_type: es.active_type.map(|s| s.to_string()),
                    },
                    event_bus::TraceContext {
                        trace_id: mem_trace_id,
                        summary: Some(format!("{}: {:?} → Idle", slot_id, es.active_type)),
                        ..Default::default()
                    },
                );
                es.phase = ExtractionPhase::Idle;
                es.active_type = None;
                es.current_task_id = None;
                es.current_slot_task_id = None;
                es.is_checkpoint = false;
                es.checkpoint_message_id = None;
                es.pending_served = false;
                s.event_bus.publish(event_bus::DaemonEvent::SlotBecameIdle { slot_id: slot_id.to_string() });
            }
        }
    } else if prev_state == missiond_core::SessionState::Idle {
        busy_since.store(
            chrono::Utc::now().timestamp(),
            std::sync::atomic::Ordering::SeqCst,
        );
    }
}

async fn handle_submit_task_closure(s: &AppState, slot_id: &str) {
    if let Ok(running_tasks) = s.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
        let now = chrono::Utc::now().timestamp_millis();
        const MIN_EXECUTION_MS: i64 = 5_000;
        const MIN_JSONL_EXECUTION_MS: i64 = 3_000;
        let pty_resp = s.slot_last_responses.write().await.remove(slot_id);
        let jsonl_resp = match s.mission.db().get_slot_session(slot_id) {
            Ok(Some(session_uuid)) => {
                match s.mission.db().get_conversation(&session_uuid) {
                    Ok(Some(conv)) => {
                        if let Some(ref jsonl_path) = conv.jsonl_path {
                            missiond_core::extract_last_assistant_text(std::path::Path::new(jsonl_path)).await
                        } else { None }
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let jsonl_confirmed = if let Some(jsonl_path) = get_task_jsonl_path(s, &missiond_core::types::Task {
            id: String::new(), role: String::new(), prompt: String::new(),
            status: missiond_core::types::TaskStatus::Running,
            slot_id: Some(slot_id.to_string()), session_id: None,
            result: None, error: None, created_at: 0, started_at: None, finished_at: None,
        }) {
            missiond_core::jsonl_has_completed_turn(std::path::Path::new(&jsonl_path)).await
        } else { false };
        for task in &running_tasks {
            if task.slot_id.as_deref() == Some(slot_id) {
                let started = task.started_at.unwrap_or(task.created_at);
                let elapsed = now - started;
                if elapsed < MIN_JSONL_EXECUTION_MS {
                    debug!(
                        task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                        "Submit task NOT closed: too short even for JSONL ({elapsed}ms < {MIN_JSONL_EXECUTION_MS}ms)"
                    );
                    continue;
                }
                if elapsed < MIN_EXECUTION_MS && !jsonl_confirmed {
                    debug!(
                        task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                        "Submit task NOT closed: execution too short ({elapsed}ms < {MIN_EXECUTION_MS}ms) and no JSONL confirmation"
                    );
                    continue;
                }
                let result_text = jsonl_resp.clone()
                    .or_else(|| {
                        if pty_resp.is_some() {
                            warn!(task_id = %task.id, "JSONL result unavailable, falling back to PTY");
                        }
                        pty_resp.clone()
                    })
                    .unwrap_or_else(|| "completed".to_string());
                let result_text = if result_text.len() > 4096 {
                    let mut end = 4096;
                    while !result_text.is_char_boundary(end) && end > 0 { end -= 1; }
                    format!("{}...(truncated)", &result_text[..end])
                } else {
                    result_text
                };
                let _ = s.mission.db().update_task(
                    &task.id,
                    &missiond_core::types::TaskUpdate {
                        status: Some(missiond_core::types::TaskStatus::Done),
                        finished_at: Some(now),
                        result: Some(result_text.clone()),
                        ..Default::default()
                    },
                );
                if let Ok(true) = s.mission.db().kb_ops_complete_by_task_id(&task.id, "done", Some(&result_text)) {
                    info!(task_id = %task.id, "KB operation marked done via task completion");
                }
                info!(task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                    jsonl_result = jsonl_resp.is_some(),
                    "Submit task closed: slot returned to Idle");
                s.event_bus.publish_traced(
                    event_bus::DaemonEvent::TaskCompleted { task_id: task.id.clone() },
                    event_bus::TraceContext {
                        trace_id: Some(task.id.clone()),
                        summary: Some(format!("Task completed on {}", slot_id)),
                        ..Default::default()
                    },
                );
            }
        }
    }
    // Always signal submit dispatcher when any slot becomes Idle
    s.event_bus.publish(event_bus::DaemonEvent::TaskCompleted { task_id: String::new() });
}

async fn handle_confirm_required(
    s: &AppState,
    slot_id: &str,
    tool_info: Option<missiond_core::ConfirmInfo>,
) {
    let tool_name = tool_info.as_ref()
        .and_then(|info| info.tool.as_ref())
        .map(|t| t.name.as_str());
    let mcp_server = tool_info.as_ref()
        .and_then(|info| info.tool.as_ref())
        .and_then(|t| t.mcp_server.as_deref());

    let should_auto_approve = match (tool_name, mcp_server) {
        (Some(name), Some("missiond")) | (Some(name), Some("mission")) => {
            info!(slot_id = %slot_id, tool = name, "Auto-confirming MissionD MCP tool");
            true
        }
        (Some("Read" | "Glob" | "Grep" | "LSP"), _) => {
            info!(slot_id = %slot_id, tool = tool_name.unwrap(), "Auto-confirming read-only tool");
            true
        }
        (Some("Write" | "Edit" | "NotebookEdit"), _) => {
            info!(slot_id = %slot_id, tool = tool_name.unwrap(), "Auto-confirming edit tool for worker slot");
            true
        }
        (Some("Bash"), _) => {
            info!(slot_id = %slot_id, tool = "Bash", "Auto-confirming Bash for worker slot");
            true
        }
        (Some(name), Some(_server)) => {
            info!(slot_id = %slot_id, tool = name, server = _server, "Auto-confirming MCP tool");
            true
        }
        (Some(name), None) => {
            warn!(slot_id = %slot_id, tool = name, "Auto-confirming unknown tool (no MCP server info)");
            true
        }
        (None, _) => {
            warn!(slot_id = %slot_id, "Auto-confirming with no tool info");
            true
        }
    };

    if should_auto_approve {
        let pty = s.pty.clone();
        let sid = slot_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = pty.confirm(&sid, missiond_core::ConfirmResponse::Yes).await {
                warn!(slot_id = %sid, error = %e, "Failed to auto-confirm tool");
            }
        });
    }
}

/// In-memory rate limiter for MCP tool error incidents.
/// Key: "slot_id:tool_name", Value: last incident creation time.
/// Prevents the same slot+tool from flooding incidents (30-second cooldown).
static MCP_ERROR_COOLDOWN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Cooldown period: same slot+tool can only create 1 incident per 30 seconds.
const MCP_ERROR_COOLDOWN_SECS: u64 = 30;

fn handle_mcp_tool_error(s: &AppState, slot_id: &str, tool_name: &str, error: &str) {
    let cooldown_key = format!("{}:{}", slot_id, tool_name);
    {
        let mut cache = MCP_ERROR_COOLDOWN.lock().unwrap();
        let now = std::time::Instant::now();
        if let Some(last) = cache.get(&cooldown_key) {
            if now.duration_since(*last).as_secs() < MCP_ERROR_COOLDOWN_SECS {
                debug!(slot_id = %slot_id, tool = %tool_name, "MCP tool error suppressed (cooldown)");
                return;
            }
        }
        // Lazy cleanup: purge expired entries when map exceeds 100 keys
        if cache.len() > 100 {
            cache.retain(|_, last_time| now.duration_since(*last_time).as_secs() < MCP_ERROR_COOLDOWN_SECS);
        }
        cache.insert(cooldown_key, now);
    }

    warn!(slot_id = %slot_id, tool = %tool_name, "MCP tool error detected, creating incident");
    let incident = missiond_core::types::MissionIncident {
        id: uuid::Uuid::new_v4().to_string(),
        severity: missiond_core::types::IncidentSeverity::High,
        source: missiond_core::types::IncidentSource::PtySlot,
        title: format!("MCP 工具不可用: {} ({})", tool_name, slot_id),
        description: format!(
            "工位 `{}` 调用 MCP 工具 `{}` 失败。\n\n错误信息:\n```\n{}\n```\n\n建议操作: 重启工位或检查 MCP 服务器配置。",
            slot_id, tool_name, error
        ),
        server_id: None,
        raw_payload: serde_json::json!({
            "slot_id": slot_id,
            "tool_name": tool_name,
            "error": error,
        }),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = s.incident_tx.try_send(incident) {
        warn!("Incident channel full, dropping MCP error incident: {}", e);
    }
}
