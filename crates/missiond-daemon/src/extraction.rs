
use tracing::{debug, info, warn};

use crate::state::{AppState, ExtractionPhase};
use crate::autopilot::{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::supervisor::is_auth_error;
use crate::memory_scheduler::{ensure_memory_slot, ensure_memory_slot_by_id};
use missiond_core::SessionState;
use std::sync::Arc;
use crate::state::{CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES};
use crate::supervisor::check_extraction_gate;

pub(crate) async fn check_realtime_extraction(state: &AppState) {
    if !check_extraction_gate(&state.extraction_state, &state.mission, "realtime").await {
        return;
    }

    // Priority enforcement: unified scheduler guarantees submit tasks run before this.
    // Skip if slot-memory is occupied by a running submit task.
    if let Ok(running) = state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
        if running.iter().any(|t| t.slot_id.as_deref() == Some(MEMORY_SLOT_ID)) {
            debug!("realtime: skipping, running submit task on memory slot");
            return;
        }
    }

    // Watermark-based check: any conversations with messages beyond their realtime_forwarded_at?
    let raw_pending = match state.mission.db().get_pending_realtime_messages() {
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
        es.phase = ExtractionPhase::Sending;
        es.active_type = Some("realtime");
        es.phase_started_at = now;
        es.watermark_targets = watermark_targets;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    let prompt = "有新的对话内容待分析。\n\n\
         📋 工作流程:\n\
         1. 调用 mission_memory_pending 获取待分析内容\n\
         2. 用 mission_kb_search 去重检查\n\
         3. 用 mission_kb_remember 存入新知识\n\
         4. 发现 bug → mission_board_create 上报\n\n\
         ⚠️ 异常处理（重要）:\n\
         如果 MCP 工具调用失败、超时或不可用:\n\
         - 不要尝试用 Bash/sqlite3 等替代方案访问数据库\n\
         - 不要自行查找或修改文件系统中的 .db 文件\n\
         - 直接输出: <slot_anomaly type=\"mcp_unavailable\" tool=\"工具名\" error=\"错误描述\"/>\n\
         - 然后停止工作，等待 orchestrator 恢复\n\
         orchestrator 会自动检测并处理 MCP 连接问题，你只需上报即可。\n\n\
         📝 本工位职责:\n\
         - 数据来源: 仅 mission_memory_pending（跨会话分析归 deep-analysis 工位负责）\n\
         - 所有数据读写通过 MCP 工具完成，不直接访问文件系统中的数据库\n\n\
         提取目标（按优先级）:\n\
         - 用户偏好/原则/纠正 → category: preference\n\
         - 架构决策/技术事实 → category: memory 或 memory:architecture/memory:decision\n\
         - 已修 bug 根因 → category: memory:bugfix\n\
         - 运维痛点信号 → category: memory:ops\n\
         - 调试弯路经验 → category: memory:debug\n\
         不提取: 基础设施信息/API细节/版本号/通用技术知识/当天日志\n\
         去重: 提取前 mission_kb_search 检查。";

    info!("Triggering realtime extraction via MCP pull");

    let pty = Arc::clone(&state.pty);
    let extraction_state = Arc::clone(&state.extraction_state);
    let mission = Arc::clone(&state.mission);
    let slot_task_id_clone = slot_task_id;
    tokio::spawn(async move {
        match pty.send(MEMORY_SLOT_ID, prompt, 300_000).await {
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
                es.phase = ExtractionPhase::Idle;
                es.active_type = None;
                es.current_task_id = None;
                es.current_slot_task_id = None;
                es.is_checkpoint = false;
                es.checkpoint_message_id = None;
                es.watermark_targets.clear();
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

    // Priority enforcement: unified scheduler guarantees submit tasks run before this.
    // Skip if slow slot is occupied by a running submit task.
    if let Ok(running) = state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
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

    let db = state.mission.db();
    let pending_convs = match db.get_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES) {
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
             ⚠️ 重要: 消息级知识（偏好/决策/事实）已由 realtime 管道提取，不要重复提取。\n\
             你的任务仅限于:\n\
             1. 跨会话模式 — 用 mission_conversation_search 搜索相关会话，发现反复出现的主题\n\
             2. 工作流抽象 — 可以固化为工具/服务的重复操作\n\
             3. 知识关联 — 不同会话之间的隐含联系\n\
             4. 趋势发现 — 用户行为/需求的演变方向\n\
             5. 问题上报 — 发现 bug/资源浪费/反复出错等需要代码修复的问题时，调用 mission_board_create 创建任务\n\
             6. 运维链路审计 — 会话中是否有重复的多步手动操作（SSH→查日志→重启→再查）？\
             这些操作链可以封装为 MCP 工具一步完成。记录具体步骤序列和建议的工具名，存 category: memory:ops\n\
             7. 调试经验提炼 — 调试过程中走了哪些弯路（错误假设→验证失败→换方向）？\
             根因最终是什么？总结「正确排查路径」供下次遇到类似问题时参考，存 category: memory:debug\n\
             8. 架构决策模式 (policy:decision) — 用户在面对技术选项、报错排查或架构设计时的规律性偏好。\
             必须提炼为泛化规则（剥离具体变量名/版本号），而非单次操作记录。\
             summary 格式：[触发条件词簇] → [核心原则] → [动作]，富含可能出现在提问中的名词。\
             存 category: policy:decision\n\n\
             不要提取: 单条消息的偏好/决策/事实（realtime 已处理）、当天工作日志、版本细节。\n\
             绝对禁止写入 category: infra（基础设施由 servers.yaml 管理）。",
            session_id = conv.id,
            project = conv.project.as_deref().unwrap_or("unknown"),
            msg_count = msg_count,
            mode = if is_checkpoint { "checkpoint (活跃会话增量)" } else { "full (已完成会话)" },
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
            es.phase = ExtractionPhase::Sending;
            es.active_type = Some("deep_analysis");
            es.phase_started_at = now;
            es.current_deep_conv_id = Some(conv.id.clone());
            es.current_task_id = Some(task_id);
            es.current_slot_task_id = Some(slot_task_id.clone());
            es.is_checkpoint = is_checkpoint;
            es.checkpoint_message_id = max_message_id;
        }

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
    // Only run once per 24 hours
    let now = chrono::Utc::now().timestamp();
    let last = state.last_kb_consolidation_at.load(std::sync::atomic::Ordering::Relaxed);
    if last > 0 && now - last < 86400 {
        return;
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
        5. 完成后回复 '[DONE] 整理了 N 个分类，合并 M 条，蒸馏 K 条，删除 J 条'\n\n\
        规则:\n\
        - 不操作 memory:bugfix（有 30 天 Auto-GC）\n\
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
        es.phase = ExtractionPhase::Sending;
        es.active_type = Some("kb_consolidation");
        es.phase_started_at = now;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    // Update last consolidation timestamp
    state.last_kb_consolidation_at.store(now, std::sync::atomic::Ordering::Relaxed);

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
    use std::sync::atomic::Ordering;
    let now = chrono::Utc::now().timestamp();
    let last = state.last_auto_gc_at.load(Ordering::Relaxed);
    if now - last < 3600 {
        return;
    }

    match state.mission.db().kb_auto_gc() {
        Ok(n) if n > 0 => info!(deleted = n, "KB auto-GC completed"),
        Ok(_) => debug!("KB auto-GC: nothing to clean"),
        Err(e) => warn!(error = %e, "KB auto-GC failed"),
    }
    state.last_auto_gc_at.store(now, Ordering::Relaxed);
}
