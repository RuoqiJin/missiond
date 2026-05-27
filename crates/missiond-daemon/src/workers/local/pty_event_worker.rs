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

use crate::context::v3_blueprint_runtime::LearningEngineRuntimeConfig;
use crate::engine::master_control::{
    CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND, CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND,
};
use crate::infra::message_handler::handle_pty_text_complete;
use crate::state::{
    AppState, EmbeddingTask, ExtractionPhase, CURRENT_ANALYSIS_VERSION, MEMORY_SLOT_ID,
    MEMORY_SLOW_SLOT_ID,
};
use missiond_core::event::events::{
    IncidentEvent, MemoryEvent, SessionEndStatus, SessionEvent, SlotEvent, TaskEvent,
};

pub(crate) struct PtyEventWorker {
    pub pty_rx: broadcast::Receiver<missiond_core::ManagerEvent>,
}

impl super::BackgroundWorker for PtyEventWorker {
    const KIND: super::WorkerKind = super::WorkerKind::Local;

    fn name(&self) -> &'static str {
        "pty_events"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: super::WorkerContext) {
        let mut rx = self.pty_rx;
        loop {
            ctx.wait_if_paused().await;
            match rx.recv().await {
                Ok(missiond_core::ManagerEvent::TextComplete {
                    slot_id,
                    turn_id,
                    content,
                    timestamp,
                }) => {
                    ctx.begin_task(
                        Some(format!("worker:pty_events:text:{slot_id}")),
                        Some(slot_id.clone()),
                        Some(300),
                    );
                    ctx.progress("handling PTY text completion");
                    if !content.is_empty() {
                        state
                            .slot_last_responses
                            .write()
                            .await
                            .insert(slot_id.clone(), content.clone());
                    }
                    handle_pty_text_complete(&state, slot_id, turn_id, content, timestamp).await;
                    ctx.record_success();
                }
                Ok(missiond_core::ManagerEvent::Exited { slot_id, exit_code }) => {
                    ctx.begin_task(
                        Some(format!("worker:pty_events:exit:{slot_id}")),
                        Some(slot_id.clone()),
                        Some(120),
                    );
                    ctx.progress("handling PTY exit");
                    handle_exited(&state, &slot_id, exit_code).await;
                    ctx.record_success();
                }
                Ok(missiond_core::ManagerEvent::StateChange {
                    ref slot_id,
                    new_state,
                    prev_state,
                }) => {
                    ctx.begin_task(
                        Some(format!("worker:pty_events:state:{slot_id}")),
                        Some(slot_id.clone()),
                        Some(120),
                    );
                    ctx.progress("handling PTY state change");
                    handle_state_change(&state, slot_id, new_state, prev_state).await;
                    ctx.record_success();
                }
                Ok(missiond_core::ManagerEvent::ConfirmRequired {
                    slot_id,
                    prompt: _,
                    tool_info,
                }) => {
                    ctx.begin_task(
                        Some(format!("worker:pty_events:confirm:{slot_id}")),
                        Some(slot_id.clone()),
                        Some(120),
                    );
                    ctx.progress("handling PTY confirmation request");
                    handle_confirm_required(&state, &slot_id, tool_info).await;
                    ctx.record_success();
                }
                Ok(missiond_core::ManagerEvent::McpToolError {
                    slot_id,
                    tool_name,
                    error,
                }) => {
                    ctx.begin_task(
                        Some(format!("worker:pty_events:mcp-error:{slot_id}")),
                        Some(slot_id.clone()),
                        Some(120),
                    );
                    ctx.progress(format!("handling MCP tool error from {tool_name}"));
                    handle_mcp_tool_error(&state, &slot_id, &tool_name, &error);
                    ctx.record_success();
                }
                Ok(missiond_core::ManagerEvent::Spawned { .. }) => {}
                Ok(missiond_core::ManagerEvent::MessageSent {
                    slot_id,
                    project_path,
                    prompt,
                }) => {
                    ctx.begin_task(
                        Some(format!("worker:pty_events:message-sent:{slot_id}")),
                        Some(slot_id.clone()),
                        Some(120),
                    );
                    if let Some(path) = project_path {
                        ctx.progress("recording PTY spawn expectation");
                        state.pending_slot_spawns.write().await.push((
                            path,
                            slot_id.clone(),
                            prompt.clone(),
                            tokio::time::Instant::now(),
                        ));
                        tracing::info!(
                            slot_id = %slot_id,
                            "Issued expectation ticket on message sent"
                        );
                    }
                    ctx.record_success();
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    ctx.retrying(format!("PTY event stream lagged by {n} events"));
                    warn!(skipped = n, "PTY logger lagged");
                }
                Err(_) => {}
            }
        }
    }
}

async fn handle_exited(s: &AppState, slot_id: &str, exit_code: i32) {
    info!(slot_id = %slot_id, exit_code = exit_code, "PTY session exited");
    let old_uuid = s.store.get_slot_session(slot_id).await.unwrap_or(None);
    if let Some(ref uuid) = old_uuid {
        let _ = s.store.complete_conversation(uuid).await;
        // Ground truth: persist exit code for Learning Engine
        let _ = s.store.save_conversation_exit_code(uuid, exit_code).await;
        let _ = s
            .embedding_tx
            .try_send(EmbeddingTask::ProcessSession(uuid.clone()));

        // Phase 3: Emit SessionCompleted for event-driven reflection pipeline
        let (msg_count, duration) = s
            .store
            .get_conversation(uuid)
            .await
            .ok()
            .flatten()
            .map(|conv| {
                let msgs = conv.message_count as u32;
                let dur = conv
                    .ended_at
                    .as_ref()
                    .and_then(|end| chrono::DateTime::parse_from_rfc3339(end).ok())
                    .and_then(|end| {
                        chrono::DateTime::parse_from_rfc3339(&conv.started_at)
                            .ok()
                            .map(|start| (end - start).num_seconds().max(0) as u64)
                    })
                    .unwrap_or(0);
                (msgs, dur)
            })
            .unwrap_or((0, 0));
        let end_status = if exit_code == 0 {
            SessionEndStatus::Success
        } else if exit_code == -1 || exit_code == 130 {
            SessionEndStatus::Aborted // SIGINT or force kill
        } else {
            SessionEndStatus::Error
        };
        let _ = s
            .bus
            .publish_session(SessionEvent::Completed {
                session_id: uuid.clone(),
                slot_id: Some(slot_id.to_string()),
                message_count: msg_count,
                duration_secs: duration,
                status: end_status,
            })
            .await;

        s.pty_session_uuids.write().await.remove(uuid);
    }
    let _ = s.store.delete_slot_session(slot_id).await;
}

async fn handle_state_change(
    s: &AppState,
    slot_id: &str,
    new_state: missiond_core::SessionState,
    prev_state: missiond_core::SessionState,
) {
    // Publish slot state change
    let _ = s
        .bus
        .publish_slot(SlotEvent::StateChanged {
            slot_id: slot_id.to_string(),
            new_state: format!("{:?}", new_state),
            prev_state: format!("{:?}", prev_state),
        })
        .await;

    // Route memory slot state changes to the correct lane
    handle_memory_lane_state(s, slot_id, new_state, prev_state).await;

    // Close Running submit tasks when slot returns to Idle
    if new_state == missiond_core::SessionState::Idle
        && prev_state != missiond_core::SessionState::Idle
    {
        handle_submit_task_closure(s, slot_id).await;

        // Emit SlotBecameIdle for ALL slots (not just memory).
        // Memory slots already emit via handle_memory_lane_state; non-memory slots need this
        // to trigger event-driven board dispatch instead of waiting for 60s autopilot tick.
        if slot_id != MEMORY_SLOT_ID && slot_id != MEMORY_SLOW_SLOT_ID {
            let _ = s
                .bus
                .publish_slot(SlotEvent::BecameIdle {
                    slot_id: slot_id.to_string(),
                })
                .await;
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
                debug!(
                    lane = lane_name,
                    phase_age, "Ignoring early Idle transition (likely spawn init)"
                );
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
                        for (session_id, timestamp) in &es.watermark_targets {
                            let _ = s
                                .store
                                .update_realtime_forwarded_at(session_id, timestamp)
                                .await;
                        }
                        info!(
                            sessions = es.watermark_targets.len(),
                            "Realtime: advanced watermarks"
                        );
                        es.watermark_targets.clear();
                    }
                }
                let mut output_count = 0_u32;
                if matches!(es.active_type, Some("deep_analysis")) {
                    output_count = es.current_output_count;
                    if let Some(conv_id) = es.current_deep_conv_id.take() {
                        if es.is_checkpoint {
                            if let Some(msg_id) = es.checkpoint_message_id.take() {
                                if let Err(e) =
                                    s.store.update_deep_checkpoint(&conv_id, msg_id).await
                                {
                                    warn!(conv_id = %conv_id, error = %e, "Failed to advance checkpoint watermark");
                                } else {
                                    info!(conv_id = %conv_id, msg_id, "Deep analysis checkpoint: advanced watermark");
                                }
                            }
                        } else {
                            if let Err(e) = s
                                .store
                                .mark_analysis_complete(&conv_id, CURRENT_ANALYSIS_VERSION)
                                .await
                            {
                                warn!(conv_id = %conv_id, error = %e, "Failed to mark analysis complete");
                            } else {
                                info!(conv_id = %conv_id, version = CURRENT_ANALYSIS_VERSION, "Deep analysis: marked complete");
                            }
                        }
                        let _ = s
                            .bus
                            .publish_memory(MemoryEvent::DeepAnalysisCompleted {
                                session_id: conv_id,
                                kb_entries_created: output_count,
                            })
                            .await;
                    }
                    let config = LearningEngineRuntimeConfig::load_for_current_dir()
                        .unwrap_or_else(|_| LearningEngineRuntimeConfig::default());
                    let fused = es.record_deep_analysis_completion(
                        output_count,
                        config.deep_analysis_zero_output_fuse_threshold,
                        config.deep_analysis_zero_output_fuse_secs,
                        chrono::Utc::now().timestamp(),
                    );
                    if fused {
                        warn!(
                            consecutive_zero_output = es.deep_analysis_zero_output_count,
                            fuse_until = es.deep_analysis_fuse_until,
                            "Deep analysis zero-output saturation fuse engaged"
                        );
                    }
                }
                if let Some(ref st_id) = es.current_slot_task_id {
                    let _ = s
                        .store
                        .slot_task_set_completed(st_id, output_count as i64)
                        .await;
                }
                let mem_trace_id = es
                    .current_deep_conv_id
                    .clone()
                    .or_else(|| es.current_task_id.clone());
                let active_type = es.active_type.map(|a| a.to_string());
                let _ = mem_trace_id; // retained for future SpanContext wiring
                let _ = s
                    .bus
                    .publish_memory(MemoryEvent::PhaseChanged {
                        slot_id: slot_id.to_string(),
                        phase: "Idle".to_string(),
                        active_type,
                    })
                    .await;
                es.phase = ExtractionPhase::Idle;
                es.active_type = None;
                es.current_task_id = None;
                es.current_slot_task_id = None;
                es.is_checkpoint = false;
                es.checkpoint_message_id = None;
                es.reset_current_output_count();
                es.clear_pending_batch_replay();
                let _ = s
                    .bus
                    .publish_slot(SlotEvent::BecameIdle {
                        slot_id: slot_id.to_string(),
                    })
                    .await;
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
    // v0.5.0: memory-hook tasks now live in board_tasks (trigger_source='memory_hook').
    // Poll running memory-hook tasks claimed by this slot.
    let running_tasks = match s
        .store
        .list_board_tasks_by_trigger("memory_hook", "running", 256)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "Failed to query running memory-hook board_tasks");
            // Still emit completion signal so dispatcher wakes up.
            let _ = s
                .bus
                .publish_task(TaskEvent::Completed {
                    task_id: String::new(),
                })
                .await;
            return;
        }
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    const MIN_EXECUTION_MS: i64 = 5_000;
    const MIN_JSONL_EXECUTION_MS: i64 = 3_000;
    let pty_resp = s.slot_last_responses.write().await.remove(slot_id);
    let jsonl_resp = match s.store.get_slot_session(slot_id).await {
        Ok(Some(session_uuid)) => match s.store.get_conversation(&session_uuid).await {
            Ok(Some(conv)) => {
                if let Some(ref jsonl_path) = conv.jsonl_path {
                    missiond_core::extract_last_assistant_text(std::path::Path::new(jsonl_path))
                        .await
                } else {
                    None
                }
            }
            _ => None,
        },
        _ => None,
    };
    let jsonl_confirmed = {
        let session_uuid = s.store.get_slot_session(slot_id).await.ok().flatten();
        let jsonl_path = match session_uuid {
            Some(uuid) => match s.store.get_conversation(&uuid).await {
                Ok(Some(conv)) => conv.jsonl_path,
                _ => None,
            },
            None => None,
        };
        match jsonl_path {
            Some(path) => {
                missiond_core::jsonl_has_completed_turn(std::path::Path::new(&path)).await
            }
            None => false,
        }
    };

    for task in &running_tasks {
        if task.claim_executor_id.as_deref() != Some(slot_id) {
            continue;
        }
        let started_ms =
            parse_rfc3339_to_ms(task.claimed_at.as_deref().unwrap_or(&task.updated_at))
                .unwrap_or(now_ms);
        let elapsed = now_ms - started_ms;
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
        let result_text = jsonl_resp
            .clone()
            .or_else(|| {
                if pty_resp.is_some() {
                    warn!(task_id = %task.id, "JSONL result unavailable, falling back to PTY");
                }
                pty_resp.clone()
            })
            .unwrap_or_else(|| "completed".to_string());
        let result_text = if result_text.len() > 4096 {
            let mut end = 4096;
            while !result_text.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}...(truncated)", &result_text[..end])
        } else {
            result_text
        };
        let _ = crate::engine::control_plane_kernel::ControlPlaneKernel::new(s)
            .record_observation(
                task.id.as_str(),
                "pty_event_worker",
                serde_json::json!({
                    "schema": "missiond.pty-completion-observation.v1",
                    "slot_id": slot_id,
                    "jsonl_confirmed": jsonl_confirmed,
                    "elapsed_ms": elapsed,
                    "summary": result_text
                }),
            )
            .await;
        // Append the result as a board_task note for audit trail (replaces TaskUpdate.result).
        let _ = s
            .store
            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.as_str().to_string(),
                content: result_text.clone(),
                note_type: Some("summary".to_string()),
                author: Some("memory-hook".to_string()),
            })
            .await;
        if let Ok(true) = s
            .store
            .kb_ops_complete_by_task_id(task.id.as_str(), "done", Some(&result_text))
            .await
        {
            info!(task_id = %task.id, "KB operation marked done via task completion");
        }
        info!(task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
            jsonl_result = jsonl_resp.is_some(),
            "Submit task closed: slot returned to Idle");
        let _ = s
            .bus
            .publish_task(TaskEvent::Completed {
                task_id: task.id.as_str().to_string(),
            })
            .await;
    }
    // Always signal submit dispatcher when any slot becomes Idle
    let _ = s
        .bus
        .publish_task(TaskEvent::Completed {
            task_id: String::new(),
        })
        .await;
}

/// Parse RFC3339 timestamp string back to epoch millis for elapsed-time math.
fn parse_rfc3339_to_ms(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

async fn handle_confirm_required(
    s: &AppState,
    slot_id: &str,
    tool_info: Option<missiond_core::ConfirmInfo>,
) {
    let tool_name = tool_info
        .as_ref()
        .and_then(|info| info.tool.as_ref())
        .map(|t| t.name.as_str());
    let mcp_server = tool_info
        .as_ref()
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
            // No tool identified — could be AskUserQuestion (not a tool confirmation).
            // AskUserQuestion options have custom labels (not "Yes"/"Allow").
            let is_ask_user = tool_info
                .as_ref()
                .map(|info| {
                    !info.options.is_empty()
                        && !info.options.iter().any(|o| {
                            let lower = o.to_lowercase();
                            lower.starts_with("yes") || lower.starts_with("allow")
                        })
                })
                .unwrap_or(false);
            if is_ask_user {
                info!(slot_id = %slot_id, "AskUserQuestion detected, forwarding to user");
                false
            } else {
                warn!(slot_id = %slot_id, "Auto-confirming with no tool info");
                true
            }
        }
    };

    if should_auto_approve {
        // Prefer Option(2) "Yes, don't ask again for X" over Option(1) "Yes, this time" so
        // Claude Code persists the allowlist into project-level settings.local.json — avoids
        // the same prompt re-firing on every subsequent identical tool call.
        let opt2_text = tool_info
            .as_ref()
            .and_then(|info| info.options.get(1))
            .cloned();
        let use_allowlist = opt2_text
            .as_deref()
            .map(|opt2| {
                // Normalize curly quotes (Claude Code TUI sometimes renders U+2019)
                // to ASCII apostrophe before substring matching.
                let normalized = opt2
                    .to_lowercase()
                    .replace('\u{2019}', "'")
                    .replace('\u{2018}', "'");
                normalized.contains("don't ask")
                    || normalized.contains("dont ask")
                    || normalized.contains("always")
                    || normalized.contains("trust this")
                    || normalized.contains("不再")
            })
            .unwrap_or(false);
        let response = if use_allowlist {
            missiond_core::ConfirmResponse::Option(2)
        } else {
            missiond_core::ConfirmResponse::Yes
        };
        info!(
            slot_id = %slot_id,
            tool = ?tool_name,
            use_allowlist,
            opt2_text = ?opt2_text,
            "Auto-approve sending response"
        );

        // Single-write-path: when choosing the allowlist option we must ALSO persist
        // the learned entry here. `mission_pty_confirm` (the MCP manual path) only
        // runs for user-driven confirms; auto-approve would otherwise silently skip
        // learn() and leave `learned_permissions.yaml` empty, making perm_injector
        // a no-op on next spawn.
        if use_allowlist {
            if let Some(learned) = s.permission.learned() {
                let extracted = opt2_text
                    .as_deref()
                    .and_then(crate::permission_extract::extract_confirm);
                if let Some(ext) = extracted {
                    // Resolve the slot's role for scope_id.
                    let role = s
                        .pty
                        .get_status(slot_id)
                        .await
                        .map(|st| st.role.clone())
                        .unwrap_or_default();
                    // Resolve the tool name from tool_info (guard against None).
                    let tool_name_owned = tool_info
                        .as_ref()
                        .and_then(|info| info.tool.as_ref())
                        .map(|t| t.name.clone());

                    if let (false, Some(tool_name)) = (role.is_empty(), tool_name_owned) {
                        // Always learn at role scope.
                        match learned.learn(
                            "role",
                            &role,
                            &tool_name,
                            "allow",
                            Some(ext.pattern.as_str()),
                        ) {
                            Ok(()) => info!(
                                slot_id = %slot_id,
                                role = %role,
                                tool = %tool_name,
                                pattern = %ext.pattern,
                                "Auto-approve learned permission (role scope)"
                            ),
                            Err(e) => warn!(
                                slot_id = %slot_id,
                                error = %e,
                                "Auto-approve failed to learn permission (role scope)"
                            ),
                        }
                        // If the dialog text carried a "commands in <path>" hint,
                        // also learn at project scope.
                        if let Some(path) = ext.project_path.as_deref() {
                            let project_id = s
                                .project_registry
                                .read()
                                .await
                                .resolve(path)
                                .map(|s| s.to_string());
                            if let Some(pid) = project_id {
                                match learned.learn(
                                    "project",
                                    &pid,
                                    &tool_name,
                                    "allow",
                                    Some(ext.pattern.as_str()),
                                ) {
                                    Ok(()) => info!(
                                        slot_id = %slot_id,
                                        project_id = %pid,
                                        tool = %tool_name,
                                        pattern = %ext.pattern,
                                        "Auto-approve learned permission (project scope)"
                                    ),
                                    Err(e) => warn!(
                                        slot_id = %slot_id,
                                        error = %e,
                                        "Auto-approve failed to learn permission (project scope)"
                                    ),
                                }
                            } else {
                                debug!(
                                    slot_id = %slot_id,
                                    path = %path,
                                    "Auto-approve: no registered project matches confirm dialog path"
                                );
                            }
                        }
                    } else {
                        debug!(
                            slot_id = %slot_id,
                            role_empty = role.is_empty(),
                            "Auto-approve: missing role or tool name, skipping learn"
                        );
                    }
                } else {
                    debug!(
                        slot_id = %slot_id,
                        opt2_text = ?opt2_text,
                        "Auto-approve: could not extract pattern from allowlist option"
                    );
                }
            }
        }

        let pty = s.pty.clone();
        let sid = slot_id.to_string();
        tokio::spawn(async move {
            // Brief delay to let Claude Code's TUI fully render the dialog and bind
            // its keystroke handlers — without it, the arrow-key sequence used by
            // Option(2) can land before the dialog is interactive (8ms is too fast).
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            match pty.confirm(&sid, response).await {
                Ok(()) => {
                    info!(slot_id = %sid, "Auto-confirm sent successfully");
                }
                Err(e) => {
                    warn!(slot_id = %sid, error = %e, "Failed to auto-confirm tool");
                }
            }
        });
    }
}

/// In-memory rate limiter for MCP tool error incidents.
/// Key: "slot_id:tool_name", Value: last incident creation time.
/// Prevents the same slot+tool from flooding incidents (30-second cooldown).
static MCP_ERROR_COOLDOWN: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

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
            cache.retain(|_, last_time| {
                now.duration_since(*last_time).as_secs() < MCP_ERROR_COOLDOWN_SECS
            });
        }
        cache.insert(cooldown_key, now);
    }

    warn!(slot_id = %slot_id, tool = %tool_name, "MCP tool error detected, creating incident");
    // Tag the initial incident with the Lisp-pinned `claude_code_mcp_missing`
    // kind from `claude-code-mcp-recovery`; master_control's IncidentEvent
    // subscriber filters on this kind to wake the resident master without
    // requiring the operator to notice the PTY screen.
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
            "kind": CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND,
            "slot_id": slot_id,
            "tool_name": tool_name,
            "error": error,
        }),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let bus = std::sync::Arc::clone(&s.bus);
    let pty = std::sync::Arc::clone(&s.pty);
    let slot_id_owned = slot_id.to_string();
    let tool_name_owned = tool_name.to_string();
    tokio::spawn(async move {
        // Publish the initial `claude_code_mcp_missing` incident first so the
        // master is woken even if the reconnect attempt below stalls.
        let _ = bus
            .publish_incident(IncidentEvent::Reported { incident })
            .await;

        // Reconnect ritual is ClaudeCode-only (the `/mcp` picker is a Claude
        // Code TUI surface); skip silently for other engines and let the
        // initial incident be the wakeup signal.
        let engine = match pty.get_status(&slot_id_owned).await {
            Some(info) => info.engine,
            None => return,
        };
        if engine != missiond_core::CliEngine::ClaudeCode {
            return;
        }

        // Budget = 1 attempt per `claude-code-mcp-recovery
        // :reconnect-budget-attempts 1` — we never silently retry.
        let outcome = match pty.mcp_reconnect(&slot_id_owned).await {
            Ok(outcome) => outcome,
            Err(err) => {
                warn!(
                    slot_id = %slot_id_owned,
                    error = %err,
                    "Arrow-key MCP reconnect ritual could not run"
                );
                let follow_up = missiond_core::types::MissionIncident {
                    id: uuid::Uuid::new_v4().to_string(),
                    severity: missiond_core::types::IncidentSeverity::High,
                    source: missiond_core::types::IncidentSource::PtySlot,
                    title: format!("MCP 重连失败: {} (无法启动重连仪式)", slot_id_owned),
                    description: format!(
                        "工位 `{}` 的 `/mcp` 重连仪式无法启动。\n\n原因:\n```\n{}\n```",
                        slot_id_owned, err
                    ),
                    server_id: None,
                    raw_payload: serde_json::json!({
                        "kind": CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND,
                        "slot_id": slot_id_owned,
                        "tool_name": tool_name_owned,
                        "reason": err.to_string(),
                    }),
                    created_at: chrono::Utc::now().to_rfc3339(),
                };
                let _ = bus
                    .publish_incident(IncidentEvent::Reported {
                        incident: follow_up,
                    })
                    .await;
                return;
            }
        };

        if outcome.success {
            info!(
                slot_id = %slot_id_owned,
                down_arrows = outcome.down_arrows_used,
                "ClaudeCode MCP arrow-key reconnect ritual reported success"
            );
            // Successful reconnect is observable on the next PTY message;
            // emitting a `claude_code_mcp_reconnect_failed` here would be
            // misleading. The master-control wakeup from the original
            // missing-incident is sufficient signal.
            return;
        }

        warn!(
            slot_id = %slot_id_owned,
            down_arrows = outcome.down_arrows_used,
            reason = %outcome.reason,
            "ClaudeCode MCP arrow-key reconnect ritual failed within budget"
        );
        let follow_up = missiond_core::types::MissionIncident {
            id: uuid::Uuid::new_v4().to_string(),
            severity: missiond_core::types::IncidentSeverity::High,
            source: missiond_core::types::IncidentSource::PtySlot,
            title: format!("MCP 重连失败: {} ({})", slot_id_owned, outcome.reason),
            description: format!(
                "工位 `{}` 的 `/mcp` 仪式按 Lisp 合约执行 (键盘箭头, 拒绝数字快捷键), 但仍未恢复 mission_* 工具。\n\n原因: {}\nDown 按键次数: {}\n\n屏幕摘录:\n```\n{}\n```",
                slot_id_owned, outcome.reason, outcome.down_arrows_used, outcome.screen_excerpt
            ),
            server_id: None,
            raw_payload: serde_json::json!({
                "kind": CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND,
                "slot_id": slot_id_owned,
                "tool_name": tool_name_owned,
                "reason": outcome.reason,
                "down_arrows_used": outcome.down_arrows_used,
                "screen_excerpt": outcome.screen_excerpt,
            }),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = bus
            .publish_incident(IncidentEvent::Reported {
                incident: follow_up,
            })
            .await;
    });
}
