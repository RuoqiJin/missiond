use tracing::{debug, info, warn};

use crate::bus::BusServices;
use crate::context::v3_blueprint_runtime::LearningEngineRuntimeConfig;
use crate::engine::intent_engine::{request_default_slot, request_execution_slot};
use crate::state::{AppState, ExtractionPhase, ExtractionState};
use crate::state::{CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES};
use crate::state::{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::supervisor::check_extraction_gate;
use crate::supervisor::{is_auth_error, is_quota_exhausted};
use missiond_core::event::events::{MemoryEvent, SlotEvent};
use missiond_core::SessionState;
use std::sync::Arc;

fn load_learning_engine_config() -> Option<LearningEngineRuntimeConfig> {
    match LearningEngineRuntimeConfig::load_for_current_dir() {
        Ok(config) => Some(config),
        Err(err) => {
            warn!(error = %err, "Learning Engine: V3 learning-engine-policy unavailable");
            None
        }
    }
}

/// Helper: update extraction phase and publish MemoryPhaseChanged event with trace context.
fn set_extraction_phase(
    es: &mut ExtractionState,
    phase: ExtractionPhase,
    active_type: Option<&str>,
    bus: &Arc<BusServices>,
    slot_id: &str,
    _trace_id: Option<&str>,
) {
    es.phase = phase;
    let ev = MemoryEvent::PhaseChanged {
        slot_id: slot_id.to_string(),
        phase: format!("{:?}", phase),
        active_type: active_type.map(|s| s.to_string()),
    };
    let bus_arc = Arc::clone(bus);
    tokio::spawn(async move {
        let _ = bus_arc.publish_memory(ev).await;
    });
}

/// Helper: emit SlotTaskDispatched event for timeline visibility.
fn emit_dispatch_event(bus: &Arc<BusServices>, slot_id: &str, purpose: &str, prompt: &str) {
    let preview = if prompt.len() > 200 {
        let mut end = 200;
        while end > 0 && !prompt.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &prompt[..end])
    } else {
        prompt.to_string()
    };
    let ev = SlotEvent::TaskDispatched {
        slot_id: slot_id.to_string(),
        task_id: None,
        purpose: purpose.to_string(),
        prompt_chars: prompt.len(),
        preview,
        cited_kb_ids: vec![],
    };
    let bus_arc = Arc::clone(bus);
    tokio::spawn(async move {
        let _ = bus_arc.publish_slot(ev).await;
    });
}

// @beacon: memory
pub(crate) async fn check_realtime_extraction(state: &AppState) {
    if !check_extraction_gate(&state.extraction_state, state, "realtime").await {
        return;
    }
    let Some(config) = load_learning_engine_config() else {
        return;
    };

    // Priority enforcement: unified scheduler guarantees submit tasks run before this.
    // Skip if slot-memory is occupied by a running submit task (spawn_blocking: batch scan).
    if let Ok(running) = state
        .store
        .get_tasks_by_status(missiond_core::types::TaskStatus::Running)
        .await
    {
        if running
            .iter()
            .any(|t| t.slot_id.as_deref() == Some(MEMORY_SLOT_ID))
        {
            debug!("realtime: skipping, running submit task on memory slot");
            return;
        }
    }

    // Watermark-based check (spawn_blocking: complex join + watermark query)
    let raw_pending = match state.store.get_pending_realtime_messages().await {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => {
            debug!("realtime: no pending messages (watermark)");
            return;
        }
        Err(_) => return,
    };

    // Triage: skip sessions with zero user messages, auto-advance their watermarks.
    // Exception: compacted sessions always pass through (their tail won't get new user messages).
    let mut pending = Vec::new();
    for (session_id, project, msgs) in raw_pending {
        let has_user = msgs.iter().any(|m| m.role == "user");
        // Lazy: only query DB for compacted status when no user messages (short-circuit)
        let is_compacted = if !has_user {
            state
                .store
                .get_conversation(&session_id)
                .await
                .map(|opt| opt.map_or(false, |c| c.status == "compacted"))
                .unwrap_or(false)
        } else {
            false
        };
        if has_user || is_compacted {
            pending.push((session_id, project, msgs));
        } else {
            if let Some(last) = msgs.last() {
                let _ = state
                    .store
                    .update_realtime_forwarded_at(&session_id, &last.timestamp)
                    .await;
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
            msgs.last()
                .map(|m| (session_id.clone(), m.timestamp.clone()))
        })
        .collect();

    let session_count = pending.len();
    let msg_count: usize = pending.iter().map(|(_, _, msgs)| msgs.len()).sum();

    // Ensure memory slot is spawned, then check it's idle
    if !request_default_slot(state).await {
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

    info!(
        sessions = session_count,
        messages = msg_count,
        "Realtime extraction: locking batch (watermark)"
    );

    // Generate task_id and tag the memory slot's current session
    let task_id = uuid::Uuid::new_v4().to_string();
    if let Ok(Some(current_session)) = state.store.get_slot_session(MEMORY_SLOT_ID).await {
        let _ = state
            .store
            .set_conversation_task_id(&current_session, &task_id)
            .await;
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
    let _ = state.store.insert_slot_task(&slot_task).await;

    // Store watermark targets for advancing on completion
    {
        let now = chrono::Utc::now().timestamp();
        let mut es = state.extraction_state.write().await;
        set_extraction_phase(
            &mut es,
            ExtractionPhase::Sending,
            Some("realtime"),
            &state.bus,
            MEMORY_SLOT_ID,
            Some(&task_id),
        );
        es.phase_started_at = now;
        es.watermark_targets = watermark_targets;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    let prompt = state.prompts.extraction_realtime();

    info!("Triggering realtime extraction via MCP pull");

    // Emit dispatch event for timeline visibility
    emit_dispatch_event(&state.bus, MEMORY_SLOT_ID, "extraction", &prompt);

    let pty = Arc::clone(&state.pty);
    let extraction_state = Arc::clone(&state.extraction_state);
    let store = Arc::clone(&state.store);
    let slot_task_id_clone = slot_task_id;
    let timeout_ms = config.realtime_extraction_timeout_ms();
    tokio::spawn(async move {
        match pty.send(MEMORY_SLOT_ID, &prompt, timeout_ms).await {
            Ok(res) => {
                if is_auth_error(&res.response) || is_quota_exhausted(&res.response) {
                    let reason = if is_quota_exhausted(&res.response) {
                        "API quota exhausted"
                    } else {
                        "OAuth token expired"
                    };
                    warn!(
                        "Realtime extraction: {} on {}, aborting",
                        reason, MEMORY_SLOT_ID
                    );
                    let _ = store
                        .slot_task_set_failed(&slot_task_id_clone, reason)
                        .await;
                    let mut es = extraction_state.write().await;
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                    es.pending_served = false;
                    return;
                }
                info!(
                    duration_ms = res.duration_ms,
                    "realtime extraction send() returned"
                );
                // send() blocks until slot finishes and returns to Idle.
                // Complete extraction directly — don't enter WaitingForSlotIdle
                // (race condition: Idle transition may have already fired and been
                // ignored due to phase_age < 3s guard).
                let mut es = extraction_state.write().await;
                if es.phase == ExtractionPhase::Sending
                    || es.phase == ExtractionPhase::WaitingForSlotIdle
                {
                    // Advance watermarks for processed sessions
                    if !es.watermark_targets.is_empty() {
                        for (session_id, timestamp) in &es.watermark_targets {
                            let _ = store
                                .update_realtime_forwarded_at(session_id, timestamp)
                                .await;
                        }
                        info!(
                            sessions = es.watermark_targets.len(),
                            "Realtime: advanced watermarks (send-complete)"
                        );
                        es.watermark_targets.clear();
                    }
                    // Mark slot task completed
                    let _ = store.slot_task_set_completed(&slot_task_id_clone, 0).await;
                    info!(
                        duration_ms = res.duration_ms,
                        "Realtime extraction complete (send-path)"
                    );
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                    es.pending_served = false;
                }
            }
            Err(e) => {
                warn!(error = %e, "realtime extraction trigger failed");
                let _ = store
                    .slot_task_set_failed(&slot_task_id_clone, &e.to_string())
                    .await;
                let mut es = extraction_state.write().await;
                // P5 fix: advance watermarks even on send() failure to prevent infinite loop.
                // Messages were already prepared; not advancing causes permanent stall.
                if matches!(es.active_type, Some("realtime")) && !es.watermark_targets.is_empty() {
                    for (session_id, timestamp) in &es.watermark_targets {
                        if let Err(we) = store
                            .update_realtime_forwarded_at(session_id, timestamp)
                            .await
                        {
                            warn!(session_id, error = %we, "Failed to advance watermark on send error");
                        }
                    }
                    warn!(
                        sessions = es.watermark_targets.len(),
                        "Realtime: advanced watermarks on send failure (preventing stall)"
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
        }
    });
}

/// Deep analysis on slow lane (slot-memory-slow).
/// Reviews completed conversations using conversation-level watermark.
pub(crate) async fn check_deep_analysis(state: &AppState) {
    if !check_extraction_gate(&state.slow_extraction_state, state, "deep_analysis").await {
        return;
    }

    // Priority enforcement (spawn_blocking: batch scan).
    // Skip if slow slot is occupied by a running submit task.
    if let Ok(running) = state
        .store
        .get_tasks_by_status(missiond_core::types::TaskStatus::Running)
        .await
    {
        if running
            .iter()
            .any(|t| t.slot_id.as_deref() == Some(MEMORY_SLOW_SLOT_ID))
        {
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
    let pending_convs = match state
        .store
        .get_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)
        .await
    {
        Ok(convs) => convs,
        Err(_) => return,
    };

    if pending_convs.is_empty() {
        return;
    }

    for conv in &pending_convs {
        let is_checkpoint = conv.status == "active";
        let since_id = if is_checkpoint && conv.deep_analyzed_message_id > 0 {
            Some(conv.deep_analyzed_message_id)
        } else {
            None
        };

        let msgs = state
            .store
            .get_conversation_messages(&conv.id, since_id, 1000)
            .await
            .unwrap_or_default();
        let msg_count = msgs.len();

        // Skip conversations with too few messages — mark analyzed and move on
        // (only for completed sessions; checkpoints already passed the 100-msg threshold)
        if !is_checkpoint && msg_count < 6 {
            let _ = state
                .store
                .mark_analysis_complete(&conv.id, CURRENT_ANALYSIS_VERSION)
                .await;
            continue;
        }

        // Meta-sessions excluded at SQL level via conversation_type = 'user'.

        if !request_execution_slot(state, MEMORY_SLOW_SLOT_ID).await {
            break;
        }

        // Record the max message ID for checkpoint watermark advancement
        let max_message_id = msgs.last().map(|m| m.id);

        let checkpoint_hint = if is_checkpoint {
            format!(
                "\n⚠️ 这是增量 checkpoint 分析（活跃会话）。\n\
                 仅分析 since_id={} 之后的 {} 条新消息。\n\
                 调用 mission_conversation_get(sessionId: \"{}\", sinceId: {}) 获取新消息。\n",
                since_id.unwrap_or(0),
                msg_count,
                conv.id,
                since_id.unwrap_or(0)
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
            mode = if is_checkpoint {
                "checkpoint (活跃会话增量)"
            } else {
                "full (已完成会话)"
            },
            deep_rules = state.prompts.extraction_deep(),
            checkpoint_hint = checkpoint_hint,
        );

        let task_type = if is_checkpoint {
            "deep_checkpoint"
        } else {
            "deep_analysis"
        };
        info!(conv_id = %conv.id, msg_count, retries = conv.analysis_retries, is_checkpoint, "Deep analysis: sending to slow lane");

        // Generate task_id and tag the slow slot's current session
        let task_id = uuid::Uuid::new_v4().to_string();
        if let Ok(Some(current_session)) = state.store.get_slot_session(MEMORY_SLOW_SLOT_ID).await {
            let _ = state
                .store
                .set_conversation_task_id(&current_session, &task_id)
                .await;
        }

        // Record slot task for history tracking
        let slot_task_id = uuid::Uuid::new_v4().to_string();
        let slot_task = missiond_core::types::SlotTask {
            id: slot_task_id.clone(),
            slot_id: MEMORY_SLOW_SLOT_ID.to_string(),
            task_type: task_type.to_string(),
            status: "pending".to_string(),
            prompt_summary: Some(format!(
                "session: {}, {} msgs{}",
                &conv.id[..8.min(conv.id.len())],
                msg_count,
                if is_checkpoint { " [checkpoint]" } else { "" }
            )),
            source_sessions: Some(serde_json::to_string(&[&conv.id]).unwrap_or_default()),
            output_count: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error: None,
            conversation_id: None,
        };
        let _ = state.store.insert_slot_task(&slot_task).await;

        // Set slow extraction state with conv_id for marking complete on Idle
        {
            let now = chrono::Utc::now().timestamp();
            let mut es = state.slow_extraction_state.write().await;
            set_extraction_phase(
                &mut es,
                ExtractionPhase::Sending,
                Some("deep_analysis"),
                &state.bus,
                MEMORY_SLOW_SLOT_ID,
                Some(&conv.id),
            );
            es.phase_started_at = now;
            es.current_deep_conv_id = Some(conv.id.clone());
            es.current_task_id = Some(task_id);
            es.current_slot_task_id = Some(slot_task_id.clone());
            es.is_checkpoint = is_checkpoint;
            es.checkpoint_message_id = max_message_id;
        }

        // Emit dispatch event for timeline visibility
        emit_dispatch_event(&state.bus, MEMORY_SLOW_SLOT_ID, "deep_analysis", &prompt);

        let conv_id = conv.id.clone();
        let pty = Arc::clone(&state.pty);
        let extraction_state = Arc::clone(&state.slow_extraction_state);
        let store = Arc::clone(&state.store);
        tokio::spawn(async move {
            match pty.send(MEMORY_SLOW_SLOT_ID, &prompt, 900_000).await {
                Ok(res) => {
                    if is_auth_error(&res.response) || is_quota_exhausted(&res.response) {
                        let reason = if is_quota_exhausted(&res.response) {
                            "API quota exhausted"
                        } else {
                            "OAuth token expired"
                        };
                        warn!(conv_id = %conv_id, "Deep analysis: {} on {}, aborting", reason, MEMORY_SLOW_SLOT_ID);
                        let _ = store.slot_task_set_failed(&slot_task_id, reason).await;
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
                    if es.phase == ExtractionPhase::Sending
                        || es.phase == ExtractionPhase::WaitingForSlotIdle
                    {
                        // Mark deep analysis conversation as analyzed
                        let deep_cid = es.current_deep_conv_id.clone();
                        let is_ckpt = es.is_checkpoint;
                        let ckpt_msg_id = es.checkpoint_message_id.take();
                        if let Some(cid) = &deep_cid {
                            if is_ckpt {
                                if let Some(msg_id) = ckpt_msg_id {
                                    let _ = store.update_deep_checkpoint(cid, msg_id).await;
                                    info!(conv_id = %cid, msg_id, "Deep checkpoint: advanced watermark (send-path)");
                                }
                            } else {
                                let _ = store
                                    .mark_analysis_complete(cid, CURRENT_ANALYSIS_VERSION)
                                    .await;
                                info!(conv_id = %cid, "Deep analysis: marked complete (send-path)");
                            }
                        }
                        let _ = store.slot_task_set_completed(&slot_task_id, 0).await;
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
                    let _ = store.mark_analysis_failed(&conv_id).await;
                    let _ = store
                        .slot_task_set_failed(&slot_task_id, &e.to_string())
                        .await;
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
    let Some(config) = load_learning_engine_config() else {
        return;
    };
    // Only run once per 24 hours (persisted in DB to survive daemon restarts)
    let now = chrono::Utc::now().timestamp();
    if let Ok(Some(last)) = state
        .store
        .last_completed_slot_task_at("kb_consolidation")
        .await
    {
        if now - last < config.kb_consolidation_interval_secs {
            return;
        }
    }

    // Yield to deep analysis if there's pending work
    let has_deep_pending = state
        .store
        .has_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)
        .await
        .unwrap_or(false);
    if has_deep_pending {
        return;
    }

    // Gate: slow lane must be idle
    if !check_extraction_gate(&state.slow_extraction_state, state, "kb_consolidation").await {
        return;
    }

    // Ensure slow slot is spawned and idle
    if !request_execution_slot(state, MEMORY_SLOW_SLOT_ID).await {
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
    if let Ok(Some(current_session)) = state.store.get_slot_session(MEMORY_SLOW_SLOT_ID).await {
        let _ = state
            .store
            .set_conversation_task_id(&current_session, &task_id)
            .await;
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
    let _ = state.store.insert_slot_task(&slot_task).await;

    // Set slow extraction state
    {
        let mut es = state.slow_extraction_state.write().await;
        set_extraction_phase(
            &mut es,
            ExtractionPhase::Sending,
            Some("kb_consolidation"),
            &state.bus,
            MEMORY_SLOW_SLOT_ID,
            Some(&task_id),
        );
        es.phase_started_at = now;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    // Emit dispatch event for timeline visibility
    emit_dispatch_event(&state.bus, MEMORY_SLOW_SLOT_ID, "consolidation", prompt);

    let pty = Arc::clone(&state.pty);
    let extraction_state = Arc::clone(&state.slow_extraction_state);
    let store = Arc::clone(&state.store);
    tokio::spawn(async move {
        match pty.send(MEMORY_SLOW_SLOT_ID, prompt, 900_000).await {
            Ok(res) => {
                if is_auth_error(&res.response) || is_quota_exhausted(&res.response) {
                    let reason = if is_quota_exhausted(&res.response) {
                        "API quota exhausted"
                    } else {
                        "OAuth token expired"
                    };
                    warn!(
                        "KB consolidation: {} on {}, aborting",
                        reason, MEMORY_SLOW_SLOT_ID
                    );
                    let _ = store.slot_task_set_failed(&slot_task_id, reason).await;
                    let mut es = extraction_state.write().await;
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                    return;
                }
                info!(
                    duration_ms = res.duration_ms,
                    "KB consolidation send() returned"
                );
                // send() blocks until slot finishes — complete directly
                let mut es = extraction_state.write().await;
                if es.phase == ExtractionPhase::Sending
                    || es.phase == ExtractionPhase::WaitingForSlotIdle
                {
                    let _ = store.slot_task_set_completed(&slot_task_id, 0).await;
                    info!(
                        duration_ms = res.duration_ms,
                        "KB consolidation complete (send-path)"
                    );
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
                let _ = store
                    .slot_task_set_failed(&slot_task_id, &e.to_string())
                    .await;
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
pub(crate) async fn check_kb_auto_gc(state: &AppState) {
    let Some(config) = load_learning_engine_config() else {
        return;
    };
    let now = chrono::Utc::now().timestamp();
    let last = state
        .store
        .daemon_state_get("last_auto_gc_at")
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last < config.kb_auto_gc_interval_secs {
        return;
    }

    match state.store.kb_auto_gc().await {
        Ok(n) if n > 0 => info!(deleted = n, "KB auto-GC completed"),
        Ok(_) => debug!("KB auto-GC: nothing to clean"),
        Err(e) => warn!(error = %e, "KB auto-GC failed"),
    }
    let _ = state.store.daemon_state_set("last_auto_gc_at", now).await;
}

/// Phase 4c: Weekly LLM reflection on low-utility KB entries.
/// Sonnet diagnoses why certain KBs underperform and recommends actions.
pub(crate) async fn check_kb_reflection(state: &AppState) {
    let Some(config) = load_learning_engine_config() else {
        return;
    };
    let now = chrono::Utc::now().timestamp();
    let last = state
        .store
        .daemon_state_get("last_kb_reflection")
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last < config.kb_reflection_interval_secs {
        return;
    }

    let sonnet = match state.sonnet.as_ref() {
        Some(s) => s,
        None => {
            debug!("KB reflection: Sonnet gateway not available, skipping");
            return;
        }
    };

    let entries = match state
        .store
        .kb_list_low_utility(
            config.kb_reflection_utility_threshold,
            config.kb_reflection_min_access,
            config.kb_reflection_max_entries,
        )
        .await
    {
        Ok(e) if e.is_empty() => {
            debug!("KB reflection: no low-utility entries to reflect on");
            let _ = state
                .store
                .daemon_state_set("last_kb_reflection", now)
                .await;
            return;
        }
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "KB reflection: failed to list low-utility entries");
            return;
        }
    };

    info!(
        count = entries.len(),
        "KB reflection: analyzing low-utility entries"
    );

    // Build context
    let mut kb_context = String::new();
    for entry in &entries {
        kb_context.push_str(&format!(
            "- [{}] category={}, key=\"{}\", utility={:.2}, access={}, summary=\"{}\"\n",
            &entry.id[..8.min(entry.id.len())],
            entry.category,
            entry.key,
            entry.utility_score,
            entry.access_count,
            entry.summary,
        ));
    }

    let prompt = format!(
        r#"你是知识库质量分析师。以下 KB 条目的效用评分低（被引用但任务表现不佳）。诊断原因并给出处置建议。

## 低效用 KB 条目
{kb_context}
## 诊断维度
- 粒度不当（太粗/太细）→ 建议拆分/合并
- 内容过时 → 建议删除
- 上下文缺失 → 建议补充关联信息
- 提取质量差 → 建议重新提取
- 正常但场景有限 → 建议保留

## 输出要求
严格返回 JSON 数组（不要 markdown 围栏）：
[{{"id": "完整ID", "reason": "诊断原因", "action": "keep|delete|re-extract", "detail": "具体说明"}}]

保守原则：不确定时选 keep。只有明确过时/错误的才选 delete。"#
    );

    let messages = vec![crate::minimax_client::ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];

    let analysis = match sonnet
        .call_briefing(
            messages,
            Some(config.kb_reflection_max_tokens),
            Some("kb-reflection".to_string()),
        )
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            warn!(error = %e, "KB reflection: Sonnet call failed");
            return;
        }
    };

    // Parse and execute actions
    let actions: Vec<serde_json::Value> = match serde_json::from_str(&analysis) {
        Ok(a) => a,
        Err(e) => {
            warn!(error = %e, raw = %analysis, "KB reflection: failed to parse Sonnet response");
            let _ = state
                .store
                .daemon_state_set("last_kb_reflection", now)
                .await;
            return;
        }
    };

    let mut deleted = 0usize;
    let mut re_extract = 0usize;
    let mut kept = 0usize;

    for action in &actions {
        let id = match action["id"].as_str() {
            Some(s) => s,
            None => continue,
        };
        match action["action"].as_str() {
            Some("delete") => {
                // Gemini ARB safety net: verify utility is actually low before trusting LLM delete verdict
                if let Ok(Some(entry)) = state.store.kb_get_by_id(id).await {
                    if entry.utility_score >= config.kb_reflection_utility_threshold {
                        warn!(
                            kb_id = id,
                            utility = entry.utility_score,
                            "KB reflection: LLM suggested delete but utility recovered, skipping"
                        );
                        kept += 1;
                        continue;
                    }
                    match state.store.kb_forget(&entry.key).await {
                        Ok(true) => {
                            deleted += 1;
                        }
                        Ok(false) => {}
                        Err(e) => warn!(kb_id = id, error = %e, "KB reflection: delete failed"),
                    }
                }
            }
            Some("re-extract") => {
                match state
                    .store
                    .kb_mark_needs_re_extraction(&[id.to_string()])
                    .await
                {
                    Ok(n) => {
                        re_extract += n;
                    }
                    Err(e) => {
                        warn!(kb_id = id, error = %e, "KB reflection: mark re-extract failed")
                    }
                }
            }
            _ => {
                // keep: mild boost to recover
                let _ = state
                    .store
                    .kb_batch_apply_utility_feedback(&[id.to_string()], true)
                    .await;
                kept += 1;
            }
        }
    }

    info!(
        deleted,
        re_extract,
        kept,
        total = actions.len(),
        "KB reflection: completed"
    );
    let _ = state
        .store
        .daemon_state_set("last_kb_reflection", now)
        .await;
}
