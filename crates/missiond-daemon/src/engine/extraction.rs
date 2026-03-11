
use tracing::{debug, info, warn};

use crate::state::{AppState, ExtractionPhase, ExtractionState};
use crate::state::{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::event_bus::{DaemonEvent, EventBus, TraceContext};
use crate::supervisor::is_auth_error;
use crate::memory_scheduler::{ensure_memory_slot, ensure_memory_slot_by_id};
use missiond_core::SessionState;
use std::sync::Arc;
use crate::state::{CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES};
use crate::supervisor::check_extraction_gate;

/// Helper: update extraction phase and publish MemoryPhaseChanged event with trace context.
fn set_extraction_phase(
    es: &mut ExtractionState,
    phase: ExtractionPhase,
    active_type: Option<&str>,
    event_bus: &EventBus,
    slot_id: &str,
    trace_id: Option<&str>,
) {
    es.phase = phase;
    event_bus.publish_traced(
        DaemonEvent::MemoryPhaseChanged {
            slot_id: slot_id.to_string(),
            phase: format!("{:?}", phase),
            active_type: active_type.map(|s| s.to_string()),
        },
        TraceContext {
            trace_id: trace_id.map(|s| s.to_string()),
            summary: Some(format!("{} → {:?}", slot_id, phase)),
            ..Default::default()
        },
    );
}

/// Helper: emit SlotTaskDispatched event for timeline visibility.
fn emit_dispatch_event(event_bus: &EventBus, slot_id: &str, purpose: &str, prompt: &str) {
    let preview = if prompt.len() > 200 {
        let mut end = 200;
        while end > 0 && !prompt.is_char_boundary(end) { end -= 1; }
        format!("{}...", &prompt[..end])
    } else {
        prompt.to_string()
    };
    event_bus.publish(DaemonEvent::SlotTaskDispatched {
        slot_id: slot_id.to_string(),
        task_id: None,
        purpose: purpose.to_string(),
        prompt_chars: prompt.len(),
        preview,
    });
}

// @beacon: memory
pub(crate) async fn check_realtime_extraction(state: &AppState) {
    if !check_extraction_gate(&state.extraction_state, &state.mission, "realtime").await {
        return;
    }

    // Priority enforcement: unified scheduler guarantees submit tasks run before this.
    // Skip if slot-memory is occupied by a running submit task (spawn_blocking: batch scan).
    if let Ok(running) = state.db_exec.run(|db| db.get_tasks_by_status(missiond_core::types::TaskStatus::Running)).await {
        if running.iter().any(|t| t.slot_id.as_deref() == Some(MEMORY_SLOT_ID)) {
            debug!("realtime: skipping, running submit task on memory slot");
            return;
        }
    }

    // Watermark-based check (spawn_blocking: complex join + watermark query)
    let raw_pending = match state.db_exec.run(|db| db.get_pending_realtime_messages()).await {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => {
            debug!("realtime: no pending messages (watermark)");
            return;
        }
        Err(_) => return,
    };

    // Triage: skip sessions with zero user messages, auto-advance their watermarks
    let db = state.mission.db();
    let mut pending = Vec::new();
    for (session_id, project, msgs) in raw_pending {
        if msgs.iter().any(|m| m.role == "user") {
            pending.push((session_id, project, msgs));
        } else {
            if let Some(last) = msgs.last() {
                let _ = db.update_realtime_forwarded_at(&session_id, &last.timestamp);
            }
        }
    }
    if pending.is_empty() {
        debug!("realtime: all sessions filtered (no user messages)");
        return;
    }

    // Capture watermark targets: (session_id, max_timestamp) for advancing on completion
    let watermark_targets: Vec<(String, String)> = pending
        .iter()
        .filter_map(|(session_id, _, msgs)| {
            msgs.last().map(|m| (session_id.clone(), m.timestamp.clone()))
        })
        .collect();

    let session_count = pending.len();
    let msg_count: usize = pending.iter().map(|(_, _, msgs)| msgs.len()).sum();

    // Ensure memory slot is spawned, then check it's idle
    if !ensure_memory_slot(state).await {
        debug!("realtime: memory slot not available");
        return;
    }
    let status = state.pty.get_status(MEMORY_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        Some(s) => {
            debug!(state = ?s.state, "realtime: slot not idle");
            return;
        }
        None => {
            debug!("realtime: slot status unavailable");
            return;
        }
    }

    info!(sessions = session_count, messages = msg_count, "Realtime extraction: locking batch (watermark)");

    // Generate task_id and tag the memory slot's current session
    let task_id = uuid::Uuid::new_v4().to_string();
    if let Ok(Some(current_session)) = state.mission.db().get_slot_session(MEMORY_SLOT_ID) {
        let _ = state.mission.db().set_conversation_task_id(&current_session, &task_id);
    }

    // Record slot task for history tracking
    let source_session_ids: Vec<String> = pending.iter().map(|(sid, _, _)| sid.clone()).collect();
    let slot_task_id = uuid::Uuid::new_v4().to_string();
    let slot_task = missiond_core::types::SlotTask {
        id: slot_task_id.clone(),
        slot_id: MEMORY_SLOT_ID.to_string(),
        task_type: "realtime_extract".to_string(),
        status: "pending".to_string(),
        prompt_summary: Some(format!("{} 个会话, {} 条消息", session_count, msg_count)),
        source_sessions: Some(serde_json::to_string(&source_session_ids).unwrap_or_default()),
        output_count: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        started_at: None,
        completed_at: None,
        duration_ms: None,
        error: None,
        conversation_id: None,
    };
    let _ = state.mission.db().insert_slot_task(&slot_task);

    // Store watermark targets for advancing on completion
    {
        let now = chrono::Utc::now().timestamp();
        let mut es = state.extraction_state.write().await;
        set_extraction_phase(&mut es, ExtractionPhase::Sending, Some("realtime"), &state.event_bus, MEMORY_SLOT_ID, Some(&task_id));
        es.phase_started_at = now;
        es.watermark_targets = watermark_targets;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    let prompt = state.prompts.extraction_realtime();

    info!("Triggering realtime extraction via MCP pull");

    // Emit dispatch event for timeline visibility
    emit_dispatch_event(&state.event_bus, MEMORY_SLOT_ID, "extraction", &prompt);

    let pty = Arc::clone(&state.pty);
    let extraction_state = Arc::clone(&state.extraction_state);
    let mission = Arc::clone(&state.mission);
    let slot_task_id_clone = slot_task_id;
    tokio::spawn(async move {
        match pty.send(MEMORY_SLOT_ID, &prompt, 300_000).await {
            Ok(res) => {
                if is_auth_error(&res.response) {
                    warn!("Realtime extraction: auth error on {}, aborting", MEMORY_SLOT_ID);
                    let _ = mission.db().slot_task_set_failed(&slot_task_id_clone, "OAuth token expired");
                    let mut es = extraction_state.write().await;
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                    return;
                }
                info!(duration_ms = res.duration_ms, "realtime extraction send() returned");
                // send() blocks until slot finishes and returns to Idle.
                // Complete extraction directly — don't enter WaitingForSlotIdle
                // (race condition: Idle transition may have already fired and been
                // ignored due to phase_age < 3s guard).
                let mut es = extraction_state.write().await;
                if es.phase == ExtractionPhase::Sending || es.phase == ExtractionPhase::WaitingForSlotIdle {
                    // Advance watermarks for processed sessions
                    if !es.watermark_targets.is_empty() {
                        let db = mission.db();
                        for (session_id, timestamp) in &es.watermark_targets {
                            let _ = db.update_realtime_forwarded_at(session_id, timestamp);
                        }
                        info!(sessions = es.watermark_targets.len(), "Realtime: advanced watermarks (send-complete)");
                        es.watermark_targets.clear();
                    }
                    // Mark slot task completed
                    let _ = mission.db().slot_task_set_completed(&slot_task_id_clone, 0);
                    info!(duration_ms = res.duration_ms, "Realtime extraction complete (send-path)");
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                }
            }
            Err(e) => {
                warn!(error = %e, "realtime extraction trigger failed");
                let _ = mission.db().slot_task_set_failed(&slot_task_id_clone, &e.to_string());
                let mut es = extraction_state.write().await;
                // P5 fix: advance watermarks even on send() failure to prevent infinite loop.
                // Messages were already prepared; not advancing causes permanent stall.
                if matches!(es.active_type, Some("realtime")) && !es.watermark_targets.is_empty() {
                    let db = mission.db();
                    for (session_id, timestamp) in &es.watermark_targets {
                        if let Err(we) = db.update_realtime_forwarded_at(session_id, timestamp) {
                            warn!(session_id, error = %we, "Failed to advance watermark on send error");
                        }
                    }
                    warn!(sessions = es.watermark_targets.len(), "Realtime: advanced watermarks on send failure (preventing stall)");
                }
                es.watermark_targets.clear();
                es.phase = ExtractionPhase::Idle;
                es.active_type = None;
                es.current_task_id = None;
                es.current_slot_task_id = None;
                es.is_checkpoint = false;
                es.checkpoint_message_id = None;
            }
        }
    });
}

/// Deep analysis on slow lane (slot-memory-slow).
/// Reviews completed conversations using conversation-level watermark.
pub(crate) async fn check_deep_analysis(state: &AppState) {
    if !check_extraction_gate(&state.slow_extraction_state, &state.mission, "deep_analysis").await {
        return;
    }

    // Priority enforcement (spawn_blocking: batch scan).
    // Skip if slow slot is occupied by a running submit task.
    if let Ok(running) = state.db_exec.run(|db| db.get_tasks_by_status(missiond_core::types::TaskStatus::Running)).await {
        if running.iter().any(|t| t.slot_id.as_deref() == Some(MEMORY_SLOW_SLOT_ID)) {
            debug!("deep_analysis: skipping, running submit task on slow slot");
            return;
        }
    }

    // Ensure slow slot is idle before proceeding
    let status = state.pty.get_status(MEMORY_SLOW_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        Some(s) => {
            debug!(state = ?s.state, "deep_analysis: slow slot not idle");
            return;
        }
        None => {
            // Slot not spawned yet — will be spawned below by ensure_memory_slot_by_id
        }
    }

    // spawn_blocking: pending deep analysis query
    let pending_convs = match state.db_exec.run(|db| db.get_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)).await {
        Ok(convs) => convs,
        Err(_) => return,
    };

    if pending_convs.is_empty() {
        return;
    }

    let db = state.mission.db();
    for conv in &pending_convs {
        let is_checkpoint = conv.status == "active";
        let since_id = if is_checkpoint && conv.deep_analyzed_message_id > 0 {
            Some(conv.deep_analyzed_message_id)
        } else {
            None
        };

        let msgs = db
            .get_conversation_messages(&conv.id, since_id, 1000)
            .unwrap_or_default();
        let msg_count = msgs.len();

        // Skip conversations with too few messages — mark analyzed and move on
        // (only for completed sessions; checkpoints already passed the 100-msg threshold)
        if !is_checkpoint && msg_count < 6 {
            let _ = db.mark_analysis_complete(&conv.id, CURRENT_ANALYSIS_VERSION);
            continue;
        }

        // Meta-sessions excluded at SQL level via conversation_type = 'user'.

        if !ensure_memory_slot_by_id(state, MEMORY_SLOW_SLOT_ID).await {
            break;
        }

        // Record the max message ID for checkpoint watermark advancement
        let max_message_id = msgs.last().map(|m| m.id);

        let checkpoint_hint = if is_checkpoint {
            format!(
                "\n⚠️ 这是增量 checkpoint 分析（活跃会话）。\n\
                 仅分析 since_id={} 之后的 {} 条新消息。\n\
                 调用 mission_conversation_get(sessionId: \"{}\", sinceId: {}) 获取新消息。\n",
                since_id.unwrap_or(0), msg_count, conv.id, since_id.unwrap_or(0)
            )
        } else if conv.deep_analyzed_message_id > 0 {
            // Completed session with previous checkpoint — only analyze remaining
            format!(
                "\n⚠️ 此会话已有 checkpoint，仅分析 since_id={} 之后的 {} 条剩余消息。\n\
                 调用 mission_conversation_get(sessionId: \"{}\", sinceId: {}) 获取剩余消息。\n",
                conv.deep_analyzed_message_id, msg_count, conv.id, conv.deep_analyzed_message_id
            )
        } else {
            format!(
                "\n调用 mission_conversation_get(sessionId: \"{}\") 获取完整会话内容。\n\
                 返回中如有 subagents 字段，表示该会话产生了子任务(Task tool)，可按需获取子会话内容。\n",
                conv.id
            )
        };

        let prompt = format!(
            "[deep-analysis]\n\
             session_id: {session_id}\n\
             project: {project}\n\
             消息数: {msg_count}\n\
             模式: {mode}\n\
             {checkpoint_hint}\n\
             {deep_rules}",
            session_id = conv.id,
            project = conv.project.as_deref().unwrap_or("unknown"),
            msg_count = msg_count,
            mode = if is_checkpoint { "checkpoint (活跃会话增量)" } else { "full (已完成会话)" },
            deep_rules = state.prompts.extraction_deep(),
            checkpoint_hint = checkpoint_hint,
        );

        let task_type = if is_checkpoint { "deep_checkpoint" } else { "deep_analysis" };
        info!(conv_id = %conv.id, msg_count, retries = conv.analysis_retries, is_checkpoint, "Deep analysis: sending to slow lane");

        // Generate task_id and tag the slow slot's current session
        let task_id = uuid::Uuid::new_v4().to_string();
        if let Ok(Some(current_session)) = state.mission.db().get_slot_session(MEMORY_SLOW_SLOT_ID) {
            let _ = state.mission.db().set_conversation_task_id(&current_session, &task_id);
        }

        // Record slot task for history tracking
        let slot_task_id = uuid::Uuid::new_v4().to_string();
        let slot_task = missiond_core::types::SlotTask {
            id: slot_task_id.clone(),
            slot_id: MEMORY_SLOW_SLOT_ID.to_string(),
            task_type: task_type.to_string(),
            status: "pending".to_string(),
            prompt_summary: Some(format!("session: {}, {} msgs{}", &conv.id[..8.min(conv.id.len())], msg_count, if is_checkpoint { " [checkpoint]" } else { "" })),
            source_sessions: Some(serde_json::to_string(&[&conv.id]).unwrap_or_default()),
            output_count: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error: None,
            conversation_id: None,
        };
        let _ = state.mission.db().insert_slot_task(&slot_task);

        // Set slow extraction state with conv_id for marking complete on Idle
        {
            let now = chrono::Utc::now().timestamp();
            let mut es = state.slow_extraction_state.write().await;
            set_extraction_phase(&mut es, ExtractionPhase::Sending, Some("deep_analysis"), &state.event_bus, MEMORY_SLOW_SLOT_ID, Some(&conv.id));
            es.phase_started_at = now;
            es.current_deep_conv_id = Some(conv.id.clone());
            es.current_task_id = Some(task_id);
            es.current_slot_task_id = Some(slot_task_id.clone());
            es.is_checkpoint = is_checkpoint;
            es.checkpoint_message_id = max_message_id;
        }

        // Emit dispatch event for timeline visibility
        emit_dispatch_event(&state.event_bus, MEMORY_SLOW_SLOT_ID, "deep_analysis", &prompt);

        let conv_id = conv.id.clone();
        let pty = Arc::clone(&state.pty);
        let extraction_state = Arc::clone(&state.slow_extraction_state);
        let mission = Arc::clone(&state.mission);
        tokio::spawn(async move {
            match pty.send(MEMORY_SLOW_SLOT_ID, &prompt, 900_000).await {
                Ok(res) => {
                    if is_auth_error(&res.response) {
                        warn!(conv_id = %conv_id, "Deep analysis: auth error on {}, aborting", MEMORY_SLOW_SLOT_ID);
                        let _ = mission.db().slot_task_set_failed(&slot_task_id, "OAuth token expired");
                        let mut es = extraction_state.write().await;
                        es.phase = ExtractionPhase::Idle;
                        es.active_type = None;
                        es.current_task_id = None;
                        es.current_slot_task_id = None;
                        es.current_deep_conv_id = None;
                        es.is_checkpoint = false;
                        es.checkpoint_message_id = None;
                        return;
                    }
                    info!(conv_id = %conv_id, duration_ms = res.duration_ms, "Deep analysis send() returned");
                    // send() blocks until slot finishes — complete directly
                    let mut es = extraction_state.write().await;
                    if es.phase == ExtractionPhase::Sending || es.phase == ExtractionPhase::WaitingForSlotIdle {
                        // Mark deep analysis conversation as analyzed
                        let deep_cid = es.current_deep_conv_id.clone();
                        let is_ckpt = es.is_checkpoint;
                        let ckpt_msg_id = es.checkpoint_message_id.take();
                        if let Some(cid) = &deep_cid {
                            if is_ckpt {
                                if let Some(msg_id) = ckpt_msg_id {
                                    let _ = mission.db().update_deep_checkpoint(cid, msg_id);
                                    info!(conv_id = %cid, msg_id, "Deep checkpoint: advanced watermark (send-path)");
                                }
                            } else {
                                let _ = mission.db().mark_analysis_complete(cid, CURRENT_ANALYSIS_VERSION);
                                info!(conv_id = %cid, "Deep analysis: marked complete (send-path)");
                            }
                        }
                        let _ = mission.db().slot_task_set_completed(&slot_task_id, 0);
                        info!(conv_id = %conv_id, duration_ms = res.duration_ms, "Deep analysis complete (send-path)");
                        es.phase = ExtractionPhase::Idle;
                        es.active_type = None;
                        es.current_task_id = None;
                        es.current_slot_task_id = None;
                        es.current_deep_conv_id = None;
                        es.is_checkpoint = false;
                        es.checkpoint_message_id = None;
                    }
                }
                Err(e) => {
                    warn!(conv_id = %conv_id, error = %e, "Deep analysis send() failed");
                    let _ = mission.db().mark_analysis_failed(&conv_id);
                    let _ = mission.db().slot_task_set_failed(&slot_task_id, &e.to_string());
                    let mut es = extraction_state.write().await;
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.current_deep_conv_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                }
            }
        });
        break;
    }
}

/// KB consolidation on slow lane (slot-memory-slow).
/// Periodic (every 24h) KB dedup, merge, and cleanup.
pub(crate) async fn check_kb_consolidation(state: &AppState) {
    // Only run once per 24 hours (persisted in DB to survive daemon restarts)
    let now = chrono::Utc::now().timestamp();
    if let Ok(Some(last)) = state.mission.db().last_completed_slot_task_at("kb_consolidation") {
        if now - last < 86400 {
            return;
        }
    }

    // Yield to deep analysis if there's pending work
    let has_deep_pending = state.mission.db()
        .has_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)
        .unwrap_or(false);
    if has_deep_pending {
        return;
    }

    // Gate: slow lane must be idle
    if !check_extraction_gate(&state.slow_extraction_state, &state.mission, "kb_consolidation").await {
        return;
    }

    // Ensure slow slot is spawned and idle
    if !ensure_memory_slot_by_id(state, MEMORY_SLOW_SLOT_ID).await {
        return;
    }
    let status = state.pty.get_status(MEMORY_SLOW_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        _ => return,
    }

    info!("KB consolidation: starting periodic cleanup on slow lane");

    let prompt = "[kb-consolidation] 知识库自治整理任务（Board-Aware 模式）。\n\n\
        执行流：\n\
        1. 对 memory 分类调用 mission_kb_analyze(mode=\"consolidation_plan\", target_category=\"memory\", include_board_context=true)\n\
        2. 预检: 遍历 plan.actions，用 mission_kb_get 验证每个 target_key 确实存在，跳过不存在的 key\n\
        3. 遍历 plan.actions 执行：\n\
           - merge/distill: 先 mission_kb_remember 写入 new_entry，再 mission_kb_batch_forget(keys: target_keys) 批量删除\n\
           - update: 如有 linked_task_id，记录关联（暂存到 reason 中即可）\n\
           - delete: 直接 mission_kb_batch_forget(keys: target_keys) 批量删除\n\
        4. 再对 preference 分类执行同样流程（include_board_context=true）\n\
        5. Policy 提炼：用 mission_kb_search(category=\"memory:decision\") 和 mission_kb_search(category=\"memory:architecture\") 扫描最近条目，\n\
           判断是否有可泛化为 policy:decision 的复用规则（如：'X 场景永远用 Y 方案'、'遇到 A 问题先查 B'）。\n\
           条件：跨 2+ 会话反复出现的模式 OR 用户明确要求的偏好。\n\
           命中则 mission_kb_remember(category=\"policy:decision\") 写入，summary 用祈使句/箭头格式。\n\
        6. 完成后回复 '[DONE] 整理了 N 个分类，合并 M 条，蒸馏 K 条，删除 J 条，提炼 P 条 policy'\n\n\
        规则:\n\
        - 不操作 memory:bugfix（有 30 天 Auto-GC）、memory:debug（有 14 天 Auto-GC）\n\
        - 坚决执行 mission_kb_analyze 返回的 actions，无需二次判断\n\
        - distill = 从已完成项目的多条流水账提取精华，升维到 architecture/feature\n\
        - 如果 plan.actions 为空数组，说明该分类不需要整理，跳过\n\
        - 如果 mission_kb_analyze 返回了 parse_warning（非 JSON），降级为手动分析\n\
        - 删除操作必须使用 mission_kb_batch_forget 批量删除，禁止逐条调用 mission_kb_forget";

    // Generate task_id and tag session
    let task_id = uuid::Uuid::new_v4().to_string();
    if let Ok(Some(current_session)) = state.mission.db().get_slot_session(MEMORY_SLOW_SLOT_ID) {
        let _ = state.mission.db().set_conversation_task_id(&current_session, &task_id);
    }

    // Record slot task for history tracking
    let slot_task_id = uuid::Uuid::new_v4().to_string();
    let slot_task = missiond_core::types::SlotTask {
        id: slot_task_id.clone(),
        slot_id: MEMORY_SLOW_SLOT_ID.to_string(),
        task_type: "kb_consolidation".to_string(),
        status: "pending".to_string(),
        prompt_summary: Some("periodic KB dedup/merge/cleanup".to_string()),
        source_sessions: None,
        output_count: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        started_at: None,
        completed_at: None,
        duration_ms: None,
        error: None,
        conversation_id: None,
    };
    let _ = state.mission.db().insert_slot_task(&slot_task);

    // Set slow extraction state
    {
        let mut es = state.slow_extraction_state.write().await;
        set_extraction_phase(&mut es, ExtractionPhase::Sending, Some("kb_consolidation"), &state.event_bus, MEMORY_SLOW_SLOT_ID, Some(&task_id));
        es.phase_started_at = now;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    // Emit dispatch event for timeline visibility
    emit_dispatch_event(&state.event_bus, MEMORY_SLOW_SLOT_ID, "consolidation", prompt);

    let pty = Arc::clone(&state.pty);
    let extraction_state = Arc::clone(&state.slow_extraction_state);
    let mission = Arc::clone(&state.mission);
    tokio::spawn(async move {
        match pty.send(MEMORY_SLOW_SLOT_ID, prompt, 900_000).await {
            Ok(res) => {
                if is_auth_error(&res.response) {
                    warn!("KB consolidation: auth error on {}, aborting", MEMORY_SLOW_SLOT_ID);
                    let _ = mission.db().slot_task_set_failed(&slot_task_id, "OAuth token expired");
                    let mut es = extraction_state.write().await;
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                    return;
                }
                info!(duration_ms = res.duration_ms, "KB consolidation send() returned");
                // send() blocks until slot finishes — complete directly
                let mut es = extraction_state.write().await;
                if es.phase == ExtractionPhase::Sending || es.phase == ExtractionPhase::WaitingForSlotIdle {
                    let _ = mission.db().slot_task_set_completed(&slot_task_id, 0);
                    info!(duration_ms = res.duration_ms, "KB consolidation complete (send-path)");
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                }
            }
            Err(e) => {
                warn!(error = %e, "KB consolidation send() failed");
                let _ = mission.db().slot_task_set_failed(&slot_task_id, &e.to_string());
                let mut es = extraction_state.write().await;
                es.phase = ExtractionPhase::Idle;
                es.active_type = None;
                es.current_task_id = None;
                es.current_slot_task_id = None;
                es.is_checkpoint = false;
                es.checkpoint_message_id = None;
            }
        }
    });
}

/// KB auto-GC: delete infra, expired bugfix, stale zero-access entries. Runs hourly.
pub(crate) fn check_kb_auto_gc(state: &AppState) {
    let now = chrono::Utc::now().timestamp();
    let last = state.mission.db().daemon_state_get("last_auto_gc_at").unwrap_or(None).unwrap_or(0);
    if now - last < 3600 {
        return;
    }

    match state.mission.db().kb_auto_gc() {
        Ok(n) if n > 0 => info!(deleted = n, "KB auto-GC completed"),
        Ok(_) => debug!("KB auto-GC: nothing to clean"),
        Err(e) => warn!(error = %e, "KB auto-GC failed"),
    }
    let _ = state.mission.db().daemon_state_set("last_auto_gc_at", now);
}
