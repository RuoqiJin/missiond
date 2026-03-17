use std::collections::HashSet;
use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::event_bus::{DaemonEvent, TraceContext};
use crate::supervisor::{check_slot_context_levels, check_slot_stuck, check_pending_compact_restarts};
use crate::memory_scheduler::ensure_memory_slot_by_id;
use crate::llm_gateway::determine_llm_env;
use missiond_core::SessionState;
use crate::helpers::default_mission_home;
use crate::supervisor::truncate_safe;
use crate::supervisor::{is_auth_error, is_quota_exhausted};
use crate::memory_scheduler::{dispatch_queued_submit_tasks, reap_stale_submit_tasks};
use crate::claude_md_sync::sync_claude_md;
use crate::engine::learning_engine;
use crate::supervisor::schedule_supervisor_patrol;
use crate::flow_engine::{execute_flow_task, ensure_autopilot_pty};

// @beacon: orchestration

/// Notify Jarvis conversation when an async task fails.
/// Extracts conversation_id from task metadata, writes error message, and emits event.
async fn notify_jarvis_failure(state: &AppState, task: &missiond_core::types::BoardTask, reason: &str) {
    if task.category != "jarvis" { return; }
    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&task.description) {
        if let Some(conv_id) = meta.get("conversation_id").and_then(|v| v.as_str()) {
            if !conv_id.is_empty() {
                let error_msg = format!("❌ 后台任务执行失败：{}", reason);
                let _ = state.store.router_chat_append_messages(conv_id, &[
                    ("assistant".to_string(), error_msg),
                ]).await;
                state.event_bus.publish_traced(
                    DaemonEvent::JarvisTaskCompleted {
                        conversation_id: conv_id.to_string(),
                        task_id: task.id.to_string(),
                    },
                    TraceContext {
                        trace_id: Some(conv_id.to_string()),
                        summary: Some(format!("jarvis: task {} failed", &task.id.as_str()[..8.min(task.id.as_str().len())])),
                        ..Default::default()
                    },
                );
                warn!(task_id = %task.id, conv_id = %conv_id, "Jarvis async: failure notification sent");
            }
        }
    }
}

pub(crate) async fn autopilot_tick(state: &AppState) -> Result<()> {
    let tick_start = std::time::Instant::now();

    // Check PTY slots for low context — mark for graceful restart
    check_slot_context_levels(state).await;
    // Restart marked slots once they become Idle (before any task dispatch)
    check_pending_compact_restarts(state).await;

    // Complete stale active conversations (no messages for > 10 minutes)
    let cutoff = (chrono::Utc::now() - chrono::TimeDelta::minutes(10))
        .to_rfc3339();
    match state.store.complete_stale_conversations(&cutoff).await {
        Ok(n) if n > 0 => info!(count = n, "Completed stale conversations"),
        Err(e) => warn!(error = %e, "Failed to complete stale conversations"),
        _ => {}
    }

    // Reap expired dynamic slots (TTL lifecycle)
    reap_expired_dynamic_slots(state).await;

    // GC completed jobs older than 30 minutes
    gc_completed_jobs(state).await;

    let mut memory_paused = state.memory_paused.load(std::sync::atomic::Ordering::Relaxed);
    let global_paused = state.global_paused.load(std::sync::atomic::Ordering::Relaxed);

    // TTL auto-resume: if paused for > 2 hours, auto-resume
    if memory_paused {
        const PAUSE_TTL_SECS: i64 = 2 * 60 * 60; // 2 hours
        let paused_at = state.memory_paused_at.load(std::sync::atomic::Ordering::Relaxed);
        if paused_at > 0 {
            let now = chrono::Utc::now().timestamp();
            if now - paused_at > PAUSE_TTL_SECS {
                warn!(paused_secs = now - paused_at, "Memory pause TTL expired, auto-resuming");
                state.memory_paused.store(false, std::sync::atomic::Ordering::Relaxed);
                state.memory_paused_at.store(0, std::sync::atomic::Ordering::Relaxed);
                let _ = std::fs::remove_file(default_mission_home().join("memory_paused"));
                memory_paused = false;
            }
        }
    }

    if global_paused {
        debug!("autopilot: global pause active, skipping all task dispatches");
    } else {
        // Submit task dispatch — always runs, not gated by memory_paused
        dispatch_queued_submit_tasks(state).await;
    }

    if !memory_paused && !global_paused {
        // Check if memory slots are stuck in non-Idle state for too long
        check_slot_stuck(state, MEMORY_SLOT_ID, &state.memory_slot_busy_since, &state.extraction_state).await;
        check_slot_stuck(state, MEMORY_SLOW_SLOT_ID, &state.slow_slot_busy_since, &state.slow_extraction_state).await;

        // Phase 3c: schedule_memory_tasks removed — now event-driven via
        // realtime_extraction_consumer, session_reflection_consumer,
        // kb_consolidation_consumer in event_router.rs
    }

    // FTS dirty flag rebuild: after kb_forget sets dirty, rebuild here
    match state.store.kb_rebuild_fts_if_dirty().await {
        Ok(true) => info!("autopilot: FTS index rebuilt (dirty flag)"),
        Err(e) => warn!(error = %e, "FTS dirty rebuild failed"),
        _ => {}
    }

    // Sync KB preferences + hot topics into CLAUDE.md
    sync_claude_md(state).await;

    // ── Learning Engine tick (KB GC, decision reaper, harvest, timeline) ──
    learning_engine::learning_tick(state).await;

    // Hot-reload LLM prompts from ~/.xjp-mission/prompts/ (every 10 ticks ≈ 10 min)
    if state.stats.autopilot_ticks.load(std::sync::atomic::Ordering::Relaxed) % 10 == 0 {
        state.prompts.reload();
    }

    // Reaper: force-fail stale slot tasks (pending/running > 30 min)
    match state.store.reap_stale_slot_tasks(1800).await {
        Ok(n) if n > 0 => warn!(count = n, "Reaped stale slot tasks"),
        Err(e) => warn!(error = %e, "Slot task reaper failed"),
        _ => {}
    }

    // Reaper: timeout Running submit tasks after 15 minutes
    reap_stale_submit_tasks(state).await;

    // Supervisor patrol: every 5 minutes, send patrol task to slot-supervisor
    schedule_supervisor_patrol(state).await;

    // Extraction status summary (debug)
    {
        let now = chrono::Utc::now().timestamp();
        let fast_es = state.extraction_state.read().await;
        let fast_slot = state.pty.get_status(MEMORY_SLOT_ID).await
            .map(|s| format!("{:?}", s.state))
            .unwrap_or_else(|| "not_spawned".to_string());
        let slow_es = state.slow_extraction_state.read().await;
        let slow_slot = state.pty.get_status(MEMORY_SLOW_SLOT_ID).await
            .map(|s| format!("{:?}", s.state))
            .unwrap_or_else(|| "not_spawned".to_string());
        debug!(
            fast_slot = %fast_slot,
            fast_phase = ?fast_es.phase,
            fast_type = ?fast_es.active_type,
            fast_age = now - fast_es.phase_started_at,
            slow_slot = %slow_slot,
            slow_phase = ?slow_es.phase,
            slow_type = ?slow_es.active_type,
            slow_age = now - slow_es.phase_started_at,
            "autopilot: extraction status"
        );
    }

    // === Smart watchdog: recover running tasks where slot is already idle ===
    // This catches orphaned tasks much faster than the 15-min time-based fallback.
    // Scenario: daemon restart loses the in-flight send() call, slot finishes but
    // no one reads the result — task stays 'running' forever.
    match state.store.list_running_autopilot_tasks().await {
        Ok(running) if !running.is_empty() => {
            debug!(count = running.len(), "Watchdog: checking running autopilot tasks");
            for rt in &running {
                let slot_id = rt.claim_executor_id.as_deref().unwrap_or("");
                if slot_id.is_empty() { continue; }

                let claimed_age = rt.claimed_at.as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds())
                    .unwrap_or(0);

                if claimed_age <= 120 { continue; } // Too fresh, might still be in send()

                if let Some(info) = state.pty.get_status(slot_id).await {
                    if info.state == SessionState::Idle {
                        warn!(
                            task_id = %rt.id, slot_id, age_secs = claimed_age,
                            "Watchdog: slot idle but task still running — recovering orphaned task"
                        );
                        let _ = state.store.unclaim_board_task(rt.id.as_str()).await;
                        let _ = state.store.add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: rt.id.to_string(),
                                content: format!(
                                    "🔄 **看门狗回收** — 工位 {} 已 idle 但任务仍在 running（{}s），可能是 daemon 重启导致 send() 丢失。已 unclaim，下次 tick 重新执行。",
                                    slot_id, claimed_age
                                ),
                                note_type: Some("note".to_string()),
                                author: Some("watchdog".to_string()),
                            },
                        ).await;
                    }
                } else {
                    // No PTY session at all — slot not even spawned, definitely orphaned
                    warn!(
                        task_id = %rt.id, slot_id, age_secs = claimed_age,
                        "Watchdog: no PTY session for slot — recovering orphaned task"
                    );
                    let _ = state.store.unclaim_board_task(rt.id.as_str()).await;
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Watchdog: failed to list running autopilot tasks");
        }
        _ => {}
    }

    // Time-based fallback: recover running tasks stuck > 15 min (catch-all)
    let _ = state.store.recover_stale_running_tasks(15).await;

    dispatch_board_tasks(state).await?;

    // Safety net: running tasks with no recent notes → Inbox reminder
    check_stale_board_progress(state).await;

    // Record tick timing to DaemonStats
    let tick_ms = tick_start.elapsed().as_millis() as u64;
    state.stats.autopilot_ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state.stats.autopilot_total_ms.fetch_add(tick_ms, std::sync::atomic::Ordering::Relaxed);
    state.stats.autopilot_latency.record(tick_ms * 1000); // histogram expects microseconds

    Ok(())
}

/// Board task dispatch — extracted for reuse by idle-triggered dispatch.
/// Called from autopilot_tick (60s) and event-driven (slot became idle).
pub(crate) async fn dispatch_board_tasks(state: &AppState) -> Result<()> {
    if state.global_paused.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(());
    }

    let tasks = state.store.list_autopilot_tasks().await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if tasks.is_empty() {
        return Ok(());
    }

    info!(count = tasks.len(), "Autopilot: found executable tasks");

    // Slot-level exclusivity: only dispatch ONE task per slot per tick
    let mut dispatched_slots: HashSet<String> = HashSet::new();

    // Excluded roles: these slots have dedicated purposes, not for ad-hoc tasks
    const EXCLUDED_ROLES: &[&str] = &["jarvis", "memory", "supervisor", "deploy", "operator", "decision", "secret"];

    for task in tasks {
        // Dynamic slot assignment: if assignee is None, find an idle coder slot
        let slot_id = match &task.assignee {
            Some(id) => id.clone(),
            None => {
                let mut candidate: Option<String> = None;
                for slot in state.mission.list_slots() {
                    let role = slot.config.role.as_str();
                    if EXCLUDED_ROLES.contains(&role) { continue; }
                    if dispatched_slots.contains(&slot.config.id) { continue; }
                    if let Some(info) = state.pty.get_status(&slot.config.id).await {
                        if info.state == SessionState::Idle {
                            candidate = Some(slot.config.id.clone());
                            break;
                        }
                    }
                }
                match candidate {
                    Some(id) => {
                        info!(task_id = %task.id, slot_id = %id, "Autopilot: dynamically assigned idle coder slot");
                        // Don't persist assignee yet — avoid Task Pinning Bug.
                        // If claim/pty fails, task stays unassigned for next tick to re-route.
                        id
                    }
                    None => {
                        debug!(task_id = %task.id, "Autopilot: no idle coder slot available, deferring");
                        continue;
                    }
                }
            }
        };

        // Skip if this slot already received a task in this tick
        if dispatched_slots.contains(&slot_id) {
            debug!(task_id = %task.id, slot_id = %slot_id, "Autopilot: slot already dispatched this tick, skipping");
            continue;
        }

        // ===== DAG dependency check =====
        if !task.depends_on.is_empty() {
            match state.store.check_dependencies(&task.depends_on).await {
                Ok(missiond_core::types::DependencyStatus::Ready) => {
                    // All deps done — proceed
                }
                Ok(missiond_core::types::DependencyStatus::Pending) => {
                    debug!(task_id = %task.id, "Autopilot: DAG deps pending, skipping");
                    continue;
                }
                Ok(missiond_core::types::DependencyStatus::Blocked(reason)) => {
                    warn!(task_id = %task.id, reason = %reason, "Autopilot: DAG dep failed, blocking task");
                    let _ = state.store.update_board_task(
                        task.id.as_str(),
                        &missiond_core::types::UpdateBoardTaskInput {
                            status: Some("blocked".to_string()),
                            ..Default::default()
                        },
                    ).await;
                    state.event_bus.publish_traced(
                        DaemonEvent::BoardTaskStatusChanged {
                            task_id: task.id.to_string(), old_status: format!("{:?}", task.status), new_status: "blocked".to_string(),
                        },
                        TraceContext { trace_id: Some(task.id.to_string()), summary: Some(format!("board: {} → blocked", task.title)), ..Default::default() },
                    );
                    let _ = state.store.add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: format!("因前置任务失败或取消，本任务自动阻塞。\n原因：{}", reason),
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        },
                    ).await;
                    notify_jarvis_failure(state, &task, &format!("前置任务失败，本任务已阻塞：{}", reason)).await;
                    continue;
                }
                Err(e) => {
                    warn!(task_id = %task.id, error = %e, "Autopilot: DAG check failed, skipping");
                    continue;
                }
            }
        }

        // ===== Flow task handling =====
        if task.flow_phase.is_some() {
            dispatched_slots.insert(slot_id.clone());
            if let Err(e) = execute_flow_task(state, &task, &slot_id).await {
                warn!(task_id = %task.id, error = %e, "Flow task error");
            }
            continue;
        }

        // ===== Normal (non-flow) task handling =====

        // Build prompt: template > "title\n\ndescription"
        let prompt = if let Some(ref tmpl) = task.prompt_template {
            tmpl.clone()
        } else {
            let mut p = task.title.clone();
            if !task.description.is_empty() {
                p.push_str("\n\n");
                p.push_str(&task.description);
            }
            p
        };

        // Unified context injection via Context Prefetch Pipeline
        let (full_prompt, cited_kb_ids) = {
            let req = crate::context_pipeline::PrefetchRequest {
                query: task.title.clone(),
                source: crate::context_pipeline::PrefetchSource::Autopilot {
                    task_id: task.id.to_string(),
                },
                token_budget: 4000,
            };
            let result = crate::context_pipeline::execute(state, &req).await;
            let cited = result.cited_kb_ids.clone();
            if result.assembled.is_empty() {
                (prompt, cited)
            } else {
                (format!("{}\n\n{}", result.assembled, prompt), cited)
            }
        };

        // Slot throttling: skip if slot has 3+ consecutive failures within 30 min
        {
            let fail_map = state.slot_fail_counts.lock().unwrap();
            if let Some(&(count, last_fail)) = fail_map.get(&slot_id) {
                let now = chrono::Utc::now().timestamp();
                if count >= 3 && now - last_fail < 1800 {
                    debug!(slot_id = %slot_id, failures = count, "Autopilot: slot throttled, skipping");
                    continue;
                }
            }
        }

        info!(task_id = %task.id, slot_id = %slot_id, title = %task.title, "Autopilot: executing task");

        // Atomically claim the task (CAS: only succeeds if open + unclaimed)
        match state.store.claim_board_task(task.id.as_str(), &slot_id, "pty_slot").await {
            Ok(Some(_)) => {
                dispatched_slots.insert(slot_id.clone());
                // Set lease: normal autopilot tasks get 20 minutes
                let lease = (chrono::Utc::now() + chrono::TimeDelta::minutes(20)).to_rfc3339();
                let _ = state.store.set_board_task_lease(task.id.as_str(), &lease).await;
            }
            Ok(None) => {
                debug!(task_id = %task.id, slot_id = %slot_id, "Autopilot: task already claimed, skipping");
                continue;
            }
            Err(e) => {
                warn!(task_id = %task.id, error = %e, "Autopilot: failed to claim task");
                continue;
            }
        }

        // Dynamic LLM model routing based on task characteristics + slot role
        let slot_role = state.mission.get_slot(&slot_id)
            .map(|s| s.config.role.clone())
            .unwrap_or_default();
        let task_env = determine_llm_env(&task, &slot_role);

        // Check if PTY session exists, spawn if needed
        if !ensure_autopilot_pty(state, &task, &slot_id, task_env).await {
            continue;
        }

        // Link PTY session to task for audit trail
        if let Ok(Some(session_uuid)) = state.store.get_slot_session(&slot_id).await {
            let _ = state.store.set_conversation_task_id(&session_uuid, task.id.as_str()).await;
        }

        // Inject answered questions as context (Phase 2 linkage)
        let full_prompt = {
            let answered = state.store.list_questions_for_task(task.id.as_str()).await.unwrap_or_default();
            if answered.is_empty() {
                full_prompt
            } else {
                let qa_block: String = answered.iter()
                    .filter(|q| q.answer.is_some())
                    .map(|q| format!("Q: {}\nA: {}", q.question, q.answer.as_deref().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if qa_block.is_empty() {
                    full_prompt
                } else {
                    format!("[决策与指示 (Decisions & Directives)]\n{}\n\n{}", qa_block, full_prompt)
                }
            }
        };

        // Inject predecessor task context (DAG handover)
        let full_prompt = if !task.depends_on.is_empty() {
            let mut handover_blocks = Vec::new();
            for dep_id in &task.depends_on {
                if let Ok(Some(dep_with_notes)) = state.store.get_board_task_with_notes(dep_id.as_str()).await {
                    // Find last summary note from predecessor
                    let summary = dep_with_notes.notes.iter()
                        .rev()
                        .find(|n| n.note_type == missiond_core::types::BoardNoteType::Summary)
                        .map(|n| n.content.clone());
                    if let Some(text) = summary {
                        handover_blocks.push(format!(
                            "### {} (已完成)\n> {}",
                            dep_with_notes.task.title,
                            text.lines().collect::<Vec<_>>().join("\n> ")
                        ));
                    }
                }
            }
            if handover_blocks.is_empty() {
                full_prompt
            } else {
                format!("## 前置任务产出上下文\n{}\n\n{}", handover_blocks.join("\n\n"), full_prompt)
            }
        } else {
            full_prompt
        };

        // Append Decision Engine help suffix for all autopilot tasks (with taskId for context linkage)
        let full_prompt = format!(
            "{}\n\n---\n注：若遇架构选择或反复 debug 失败的死胡同，请调 `mission_question_create(target=\"master\", taskId=\"{}\", decisionType=\"...\")` 呼叫主控裁决，附带 options 方案。",
            full_prompt, task.id
        );

        // Ops task focus: prevent slot from getting sidetracked into code investigation
        let full_prompt = if task.category == "ops" {
            format!(
                "{}\n\n---\n⚠️ **运维任务执行规范**：\n\
                1. 严格按「建议操作」列表依次执行诊断工具（mission_reachability、mission_os_diagnose 等 MCP 工具）\n\
                2. 不要去调查或修改代码，不要理会 IDE/LSP 诊断\n\
                3. 诊断完成后给出简明结论：问题原因 + 当前状态 + 是否需要人工介入",
                full_prompt
            )
        } else {
            full_prompt
        };

        // Inject task ID + self-close instruction so slot can close the task itself
        // This makes the system resilient to daemon restarts during send()
        let full_prompt = format!(
            "{}\n\n---\n📋 **Board Task ID**: `{}`\n\
            任务完成后，你必须调用 `mission_board_update(id=\"{}\", status=\"done\")` 关闭此任务，\
            并用 `mission_board_note_add(taskId=\"{}\", content=\"...\", noteType=\"summary\")` 写入诊断摘要。",
            full_prompt, task.id, task.id, task.id
        );

        // Cache cited KB IDs for confidence feedback loop after task completion
        if !cited_kb_ids.is_empty() {
            let mut cache = state.task_cited_kbs.lock().unwrap();
            cache.insert(task.id.to_string(), cited_kb_ids.clone());
        }

        // Prompt snapshot: save full context for Skill auto-verification replay
        let _ = state.store.save_prompt_snapshot(
            task.id.as_str(),
            &full_prompt,
            &cited_kb_ids,
            &task.category,
        ).await;

        // Emit dispatch event for timeline visibility
        {
            let preview = if full_prompt.len() > 200 {
                let mut end = 200;
                while end > 0 && !full_prompt.is_char_boundary(end) { end -= 1; }
                format!("{}...", &full_prompt[..end])
            } else { full_prompt.clone() };
            state.event_bus.publish(crate::event_bus::DaemonEvent::SlotTaskDispatched {
                slot_id: slot_id.clone(),
                task_id: Some(task.id.to_string()),
                purpose: "board_auto_execute".to_string(),
                prompt_chars: full_prompt.len(),
                preview,
                cited_kb_ids,
            });
        }

        // Pre-send state verification with dispatch guard: atomically check idle + send.
        if !state.slot_dispatch.try_acquire(&slot_id) {
            debug!(task_id = %task.id, slot_id = %slot_id,
                "Autopilot: slot dispatch guard busy, releasing task");
            let _ = state.store.unclaim_board_task(task.id.as_str()).await;
            continue;
        }
        if let Some(pre_send_status) = state.pty.get_status(&slot_id).await {
            if pre_send_status.state != SessionState::Idle {
                state.slot_dispatch.release(&slot_id);
                debug!(task_id = %task.id, slot_id = %slot_id, state = ?pre_send_status.state,
                    "Autopilot: slot not Idle pre-send, releasing task without penalty");
                let _ = state.store.unclaim_board_task(task.id.as_str()).await;
                continue;
            }
        }
        // Guard held: slot confirmed idle, send will transition state.
        // Release after send initiation (pty.send blocks until completion, but state transitions immediately).
        // We release here because pty.send() is blocking — holding the guard for 10min would starve other callers.
        // After this point, PTY state is non-Idle so other callers will see it as busy.
        state.slot_dispatch.release(&slot_id);

        // Send prompt and wait for response
        let timeout_ms = 600_000; // 10 minutes
        match state.pty.send(&slot_id, &full_prompt, timeout_ms).await {
            Ok(res) => {
                // Check for auth errors in successful PTY response
                if is_auth_error(&res.response) {
                    warn!(slot_id = %slot_id, task_id = %task.id, "Autopilot: auth error detected in PTY response, treating as failure");
                    let _ = state.store.add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: format!("⚠️ **Auth Error** — slot {} OAuth token 可能已过期，需要 `/login`\n\n{}", slot_id, &truncate_safe(&res.response, 500)),
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        },
                    ).await;
                    // Treat as failure: increment slot_fail_counts
                    {
                        let mut fail_map = state.slot_fail_counts.lock().unwrap();
                        let entry = fail_map.entry(slot_id.clone()).or_insert((0, 0));
                        entry.0 += 1;
                        entry.1 = chrono::Utc::now().timestamp();
                        if entry.0 >= 2 {
                            warn!(slot_id = %slot_id, failures = entry.0, "Slot auth-throttled: OAuth expired, needs /login");
                        }
                    }
                    // Back to open for retry (don't mark done)
                    let new_retry = task.retry_count + 1;
                    if new_retry >= task.max_retries {
                        let _ = state.store.update_board_task(
                        task.id.as_str(),
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("failed".to_string()),
                                ..Default::default()
                            },
                        ).await;
                        state.event_bus.publish_traced(
                            DaemonEvent::BoardTaskStatusChanged {
                                task_id: task.id.to_string(), old_status: format!("{:?}", task.status), new_status: "failed".to_string(),
                            },
                            TraceContext { trace_id: Some(task.id.to_string()), summary: Some(format!("board: {} → failed", task.title)), ..Default::default() },
                        );
                        notify_jarvis_failure(state, &task, "OAuth token 过期，工位认证失败").await;
                    } else {
                        let _ = state.store.increment_board_task_retry(task.id.as_str(), new_retry).await;
                    }
                    continue;
                }

                // Check for quota exhaustion — circuit breaker: auto global_pause
                if is_quota_exhausted(&res.response) {
                    warn!(slot_id = %slot_id, task_id = %task.id, "🚨 Autopilot: API quota exhausted! Activating global pause circuit breaker");
                    // Activate global pause
                    state.global_paused.store(true, std::sync::atomic::Ordering::Relaxed);
                    let now = chrono::Utc::now().timestamp();
                    state.global_paused_at.store(now, std::sync::atomic::Ordering::Relaxed);
                    let _ = std::fs::write(
                        crate::helpers::default_mission_home().join("global_paused"),
                        now.to_string(),
                    );
                    // Add note to the task
                    let _ = state.store.add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: format!(
                                "🚨 **Quota Exhausted** — API 配额耗尽，已自动激活全局暂停\n\nslot: {}\n\n{}",
                                slot_id,
                                &truncate_safe(&res.response, 500),
                            ),
                            note_type: Some("note".to_string()),
                            author: Some("circuit-breaker".to_string()),
                        },
                    ).await;
                    // Return task to open for retry after quota resets
                    let _ = state.store.update_board_task(
                        task.id.as_str(),
                        &missiond_core::types::UpdateBoardTaskInput {
                            status: Some("open".to_string()),
                            ..Default::default()
                        },
                    ).await;
                    // Create incident for visibility
                    let incident = missiond_core::types::MissionIncident {
                        id: format!("inc-{}", uuid::Uuid::new_v4()),
                        severity: missiond_core::types::IncidentSeverity::Critical,
                        source: missiond_core::types::IncidentSource::PtySlot,
                        title: "API 配额耗尽 — 全局暂停已激活".to_string(),
                        description: format!(
                            "工位 {} 检测到 API 配额耗尽，系统已自动激活全局暂停。\n\
                             所有任务派发已停止，需手动 mission_pause(action=\"resume\") 恢复。",
                            slot_id
                        ),
                        server_id: None,
                        raw_payload: serde_json::json!({
                            "slot_id": slot_id,
                            "task_id": task.id,
                            "trigger": "quota_exhausted",
                        }),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    let _ = state.incident_tx.try_send(incident);
                    notify_jarvis_failure(state, &task, "API 配额耗尽，系统已全局暂停").await;
                    // Stop processing remaining tasks — quota is gone
                    break;
                }

                // Record result as a board note
                let note_content = format!("**Autopilot 执行完成** ({}ms)\n\n{}", res.duration_ms, res.response);
                let _ = state.store.add_board_task_note(
                    &missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.to_string(),
                        content: note_content,
                        note_type: Some("summary".to_string()),
                        author: Some("autopilot".to_string()),
                    },
                ).await;
                // CAS guard: only mark done if task is still in 'running' state.
                // If task was auto-blocked by mission_question_create, preserve 'blocked' status.
                let current_status = state.store.get_board_task(task.id.as_str()).await
                    .ok().flatten()
                    .map(|t| t.status);
                match current_status {
                    Some(missiond_core::types::BoardTaskStatus::Done) => {
                        // Slot already self-closed the task
                        info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task already done (self-closed)");
                    }
                    Some(missiond_core::types::BoardTaskStatus::Blocked) => {
                        // Task was blocked by pending questions — do NOT overwrite
                        let _ = state.store.add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.to_string(),
                                content: "⚠️ PTY 执行已返回，但任务有未回答的 pending questions，保持 blocked 状态。".to_string(),
                                note_type: Some("note".to_string()),
                                author: Some("autopilot".to_string()),
                            },
                        ).await;
                        warn!(task_id = %task.id, "Autopilot: pty.send completed but task is blocked — preserving blocked status");
                    }
                    _ => {
                        // Normal case: running → done
                        let _ = state.store.update_board_task(
                        task.id.as_str(),
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("done".to_string()),
                                ..Default::default()
                            },
                        ).await;
                        state.event_bus.publish_traced(
                            DaemonEvent::BoardTaskStatusChanged {
                                task_id: task.id.to_string(), old_status: format!("{:?}", task.status), new_status: "done".to_string(),
                            },
                            TraceContext { trace_id: Some(task.id.to_string()), summary: Some(format!("board: {} → done", task.title)), ..Default::default() },
                        );
                        info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task completed");
                        // Record outcome for Skill auto-verification replay
                        let _ = state.store.update_prompt_snapshot_outcome(task.id.as_str(), "success").await;
                    }
                }
                // Reset slot failure count on success
                {
                    let mut fail_map = state.slot_fail_counts.lock().unwrap();
                    fail_map.remove(&slot_id);
                }

                // Positive confidence feedback: reinforce cited KB entries on task success
                // Self-supervision: entries below 0.8 (likely penalized by prior attribution)
                // get a higher boost (+0.05) to recover faster from misattribution
                {
                    let cited = state.task_cited_kbs.lock().unwrap().remove(task.id.as_str());
                    if let Some(kb_ids) = cited {
                        let count = kb_ids.len();
                        for kb_id in &kb_ids {
                            // Check current confidence to determine boost amount
                            let delta = state.store.kb_get_by_id(kb_id).await
                                .ok().flatten()
                                .map(|e| if e.confidence < 0.8 { 0.05 } else { 0.03 })
                                .unwrap_or(0.03);
                            match state.store.kb_adjust_confidence(kb_id, delta).await {
                                Ok(Some(new_conf)) => debug!(kb_id = %kb_id, delta, new_conf, "KB confidence boost (task success)"),
                                Ok(None) => debug!(kb_id = %kb_id, "KB entry not found for confidence adjustment"),
                                Err(e) => warn!(kb_id = %kb_id, error = %e, "Failed to adjust KB confidence"),
                            }
                        }
                        if count > 0 {
                            info!(task_id = %task.id, kb_count = count, "KB feedback: boosted confidence for {} cited entries", count);
                        }
                        // Phase 4a: Utility score boost on task success (atomic SQL)
                        match state.store.kb_batch_apply_utility_feedback(&kb_ids, true).await {
                            Ok(n) if n > 0 => info!(task_id = %task.id, boosted = n, "KB utility: boosted for task success"),
                            Err(e) => warn!(task_id = %task.id, error = %e, "KB utility: boost failed"),
                            _ => {}
                        }
                    }
                }

                // Working Memory graduation: promote worthy scratchpad entries to global KB
                {
                    let scratchpad = state.store.kb_list_by_scope(task.id.as_str()).await;
                    if let Ok(entries) = scratchpad {
                        let mut graduated = 0u32;
                        let mut expired = 0u32;
                        for entry in &entries {
                            if entry.confidence >= 0.7 && entry.access_count > 0 {
                                // Graduate: clear scope to make it global
                                let _ = state.store.kb_clear_scope(&entry.id).await;
                                graduated += 1;
                            } else {
                                // Expire: remove low-value scratchpad entries
                                let _ = state.store.kb_forget(&entry.key).await;
                                expired += 1;
                            }
                        }
                        if graduated + expired > 0 {
                            info!(task_id = %task.id, graduated, expired, "Working memory: graduated {} entries, expired {}", graduated, expired);
                        }
                    }
                }

                // Jarvis task post-completion: append result to conversation
                if task.category == "jarvis" {
                    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&task.description) {
                        if let Some(conv_id) = meta.get("conversation_id").and_then(|v| v.as_str()) {
                            if !conv_id.is_empty() {
                                let _ = state.store.router_chat_append_messages(conv_id, &[
                                    ("assistant".to_string(), res.response.clone()),
                                ]).await;
                                state.event_bus.publish_traced(
                                    DaemonEvent::JarvisTaskCompleted {
                                        conversation_id: conv_id.to_string(),
                                        task_id: task.id.to_string(),
                                    },
                                    TraceContext {
                                        trace_id: Some(conv_id.to_string()),
                                        summary: Some(format!("jarvis: task {} completed", &task.id.as_str()[..8.min(task.id.as_str().len())])),
                                        ..Default::default()
                                    },
                                );
                                info!(task_id = %task.id, conv_id = %conv_id, "Jarvis async: result appended to conversation");
                            }
                        }
                    }
                }

                // Deploy task post-mortem: trigger memory-slow to review
                if task.category == "deploy" {
                    let review_state = state.clone();
                    let review_task_id = task.id.clone();
                    let review_title = task.title.clone();
                    let review_slot = slot_id.clone();
                    tokio::spawn(async move {
                        if !ensure_memory_slot_by_id(&review_state, MEMORY_SLOW_SLOT_ID).await {
                            warn!("Cannot spawn memory-slow for deploy review");
                            return;
                        }
                        let prompt = format!(
                            "部署任务刚刚完成，请复盘：\n\
                            - 任务: {} (id: {})\n\
                            - 执行工位: {}\n\n\
                            请做以下工作：\n\
                            1. 用 mission_board_get(id=\"{}\") 查看任务详情和 notes\n\
                            2. 用 mission_conversation_search 搜索该工位最近的部署对话\n\
                            3. 分析部署过程中是否有：失败重试、手动操作、缺失工具、耗时过长等问题\n\
                            4. 提炼有价值的经验 → mission_kb_remember(category=\"memory:ops\")\n\
                            5. 如发现缺失 MCP 工具或 Skill → mission_board_create 建改进任务\n\
                            6. 如一切顺利，简要记录即可，不需要过度分析",
                            review_title, review_task_id, review_slot,
                            review_task_id,
                        );
                        let _ = review_state.pty.send(MEMORY_SLOW_SLOT_ID, &prompt, 600_000).await;
                        info!(task_id = %review_task_id, "Deploy post-mortem review dispatched to memory-slow");
                    });
                }
            }
            Err(e) => {
                // Use {:#} to print full anyhow error chain (prevents .context() from hiding inner message)
                let err_msg = format!("{:#}", e);
                let is_transient = err_msg.contains("Cannot send message in state:");

                if is_transient {
                    // Slot not ready — transient failure, just unclaim without penalty
                    debug!(task_id = %task.id, slot_id = %slot_id, error = %err_msg,
                        "Autopilot: slot not ready (transient), returning task to queue");
                    let _ = state.store.unclaim_board_task(task.id.as_str()).await;
                } else {
                    // Real execution failure — track and retry
                    let note_content = format!("**Autopilot 执行失败** (retry {}/{})\n\n{}", task.retry_count + 1, task.max_retries, err_msg);
                    let _ = state.store.add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: note_content,
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        },
                    ).await;

                    // Track slot consecutive failures
                    {
                        let mut fail_map = state.slot_fail_counts.lock().unwrap();
                        let entry = fail_map.entry(slot_id.clone()).or_insert((0, 0));
                        entry.0 += 1;
                        entry.1 = chrono::Utc::now().timestamp();
                        if entry.0 >= 3 {
                            warn!(slot_id = %slot_id, failures = entry.0, "Slot throttled for 30 min due to consecutive failures");
                        }
                    }

                    // Retry logic: increment count, mark failed if exhausted
                    let new_retry = task.retry_count + 1;
                    if new_retry >= task.max_retries {
                        let _ = state.store.update_board_task(
                        task.id.as_str(),
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("failed".to_string()),
                                ..Default::default()
                            },
                        ).await;
                        state.event_bus.publish_traced(
                            DaemonEvent::BoardTaskStatusChanged {
                                task_id: task.id.to_string(), old_status: format!("{:?}", task.status), new_status: "failed".to_string(),
                            },
                            TraceContext { trace_id: Some(task.id.to_string()), summary: Some(format!("board: {} → failed", task.title)), ..Default::default() },
                        );
                        warn!(task_id = %task.id, retries = new_retry, "Autopilot: task failed after max retries");
                        let _ = state.store.update_prompt_snapshot_outcome(task.id.as_str(), "failed").await;
                        notify_jarvis_failure(state, &task, &err_msg).await;

                        // Negative feedback: confidence (LLM-attributed) + utility_score (blanket)
                        {
                            let cited = state.task_cited_kbs.lock().unwrap().remove(task.id.as_str());
                            if let Some(kb_ids) = cited {
                                if !kb_ids.is_empty() {
                                    // Phase 4a: Utility score penalty (sync, atomic SQL)
                                    match state.store.kb_batch_apply_utility_feedback(&kb_ids, false).await {
                                        Ok(n) if n > 0 => info!(task_id = %task.id, penalized = n, "KB utility: penalized for task failure"),
                                        Err(e) => warn!(task_id = %task.id, error = %e, "KB utility: penalty failed"),
                                        _ => {}
                                    }
                                    // Confidence penalty (async, LLM-attributed)
                                    let state2 = state.clone();
                                    let task_id2 = task.id.to_string();
                                    let err_msg2 = err_msg.clone();
                                    let task_title2 = task.title.clone();
                                    tokio::spawn(async move {
                                        apply_attributed_penalty(&state2, &task_id2, &task_title2, &err_msg2, &kb_ids).await;
                                    });
                                }
                            }
                        }
                    } else {
                        // Back to open for retry, increment retry_count
                        let _ = state.store.increment_board_task_retry(task.id.as_str(), new_retry).await;
                        warn!(task_id = %task.id, retry = new_retry, max = task.max_retries, error = %err_msg, "Autopilot: task failed, will retry");
                    }
                }
            }
        }
    }

    Ok(())
}

/// LLM-attributed confidence penalty: ask MiniMax which cited KB entries
/// actually contributed to the task failure, then apply differentiated penalties.
/// Falls back to blanket -0.02 if MiniMax is unavailable or returns invalid JSON.
async fn apply_attributed_penalty(
    state: &AppState,
    task_id: &str,
    task_title: &str,
    error_msg: &str,
    kb_ids: &[String],
) {
    use crate::minimax_client::ChatMessage;

    // Build KB summaries for context
    let mut kb_context = String::new();
    for kb_id in kb_ids {
        if let Ok(Some(entry)) = state.store.kb_get_by_id(kb_id).await {
            kb_context.push_str(&format!(
                "- [{}] category={}, key={}, summary={}\n",
                &kb_id[..8.min(kb_id.len())], entry.category, entry.key, entry.summary
            ));
        }
    }

    if kb_context.is_empty() { return; }

    // Log dehydration: semantic summary for long errors, preserving causal chain.
    // Falls back to head+tail if Sonnet unavailable.
    let error_preview = if error_msg.len() > 500 {
        // Try semantic summary via Sonnet
        let summary = if let Some(sonnet) = state.sonnet.as_ref() {
            use crate::minimax_client::ChatMessage;
            let summarize_prompt = format!(
                "以下是任务失败日志（{}字符）。提取关键因果链：错误类型、触发条件、失败点。≤400字，保留原始错误消息和关键状态变化。\n\n{}",
                error_msg.len(), error_msg
            );
            let msgs = vec![ChatMessage { role: "user".to_string(), content: summarize_prompt }];
            match sonnet.call_briefing(msgs, Some(512), Some(format!("log-dehydrate-{}", task_id))).await {
                Ok(resp) if !resp.trim().is_empty() => Some(resp),
                _ => None,
            }
        } else { None };

        summary.unwrap_or_else(|| {
            // Fallback: head+tail
            let mut head_end = 200;
            while head_end > 0 && !error_msg.is_char_boundary(head_end) { head_end -= 1; }
            let mut tail_start = error_msg.len().saturating_sub(200);
            while tail_start < error_msg.len() && !error_msg.is_char_boundary(tail_start) { tail_start += 1; }
            format!("{}\n...(truncated)...\n{}", &error_msg[..head_end], &error_msg[tail_start..])
        })
    } else {
        error_msg.to_string()
    };

    let prompt = format!(
        r#"你是 KB 信用分配分析师。一个任务执行失败，系统在执行时引用了以下知识库条目。请判断每条 KB 对失败的责任。

## 任务
标题: {task_title}
错误: {error_preview}

## 引用的 KB 条目
{kb_context}
## 输出要求
严格返回 JSON 数组，每个元素: {{"id": "KB前缀", "verdict": "innocent|contributed|caused"}}
- innocent: 该条目与失败无关（如网络超时、外部服务不可用）
- contributed: 该条目部分误导了执行方向
- caused: 该条目直接导致了错误决策

大多数情况下，失败是外部因素（网络/服务/权限），KB 应判 innocent。只有当 KB 内容明确错误或过时才判 contributed/caused。
只输出 JSON 数组，不要解释。"#
    );

    // Try Sonnet attribution
    let attribution = if let Some(sonnet) = state.sonnet.as_ref() {
        let messages = vec![ChatMessage { role: "user".to_string(), content: prompt }];
        match sonnet.call_briefing(messages, Some(512), Some(format!("kb-attr-{}", task_id))).await {
            Ok(resp) => parse_attribution(&resp, kb_ids),
            Err(e) => {
                debug!(task_id, error = %e, "KB attribution: Sonnet call failed, falling back to blanket penalty");
                None
            }
        }
    } else {
        None
    };

    match attribution {
        Some(verdicts) => {
            let mut stats = (0u32, 0u32, 0u32); // innocent, contributed, caused
            for (kb_id, verdict) in &verdicts {
                let delta = match verdict.as_str() {
                    "caused" => { stats.2 += 1; -0.15 },
                    "contributed" => { stats.1 += 1; -0.05 },
                    _ => { stats.0 += 1; 0.0 }, // innocent — no penalty
                };
                if delta != 0.0 {
                    match state.store.kb_adjust_confidence(kb_id, delta).await {
                        Ok(Some(new_conf)) => debug!(kb_id = %kb_id, delta, new_conf, "KB attributed penalty"),
                        Ok(None) => {}
                        Err(e) => warn!(kb_id = %kb_id, error = %e, "Failed to adjust KB confidence"),
                    }
                }
            }
            info!(task_id, innocent = stats.0, contributed = stats.1, caused = stats.2,
                "KB attribution: differentiated penalties applied");
        }
        None => {
            // Fallback: blanket -0.02
            for kb_id in kb_ids {
                let _ = state.store.kb_adjust_confidence(kb_id, -0.02).await;
            }
            info!(task_id, kb_count = kb_ids.len(), "KB feedback: fallback -0.02 for all cited entries");
        }
    }
}

/// Parse MiniMax attribution response into (kb_id, verdict) pairs.
fn parse_attribution(response: &str, kb_ids: &[String]) -> Option<Vec<(String, String)>> {
    // Strip markdown fences if present
    let json_str = response.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim();

    let arr: Vec<serde_json::Value> = serde_json::from_str(json_str).ok()?;
    let mut result = Vec::new();

    for item in &arr {
        let id_prefix = item.get("id").and_then(|v| v.as_str())?;
        let verdict = item.get("verdict").and_then(|v| v.as_str())?;
        if !["innocent", "contributed", "caused"].contains(&verdict) {
            return None; // Invalid verdict → discard entire response
        }
        // Match prefix back to full KB ID
        if let Some(full_id) = kb_ids.iter().find(|id| id.starts_with(id_prefix)) {
            result.push((full_id.clone(), verdict.to_string()));
        }
    }

    if result.is_empty() { None } else { Some(result) }
}

/// Reap expired dynamic slots: SIGTERM → 30s grace → unregister.
async fn reap_expired_dynamic_slots(state: &AppState) {
    // Find expired active slots
    let expired = match state.store.find_expired_dynamic_slots().await {
        Ok(slots) => slots,
        Err(e) => {
            warn!(error = %e, "Failed to find expired dynamic slots");
            return;
        }
    };

    for slot in &expired {
        info!(slot_id = %slot.id, template = %slot.template, "Reaping expired dynamic slot (TTL)");

        // Kill PTY session (SIGTERM → grace period handled by PTYManager)
        let _ = state.pty.kill(&slot.id).await;

        // Mark terminated in DB
        if let Err(e) = state.store.terminate_dynamic_slot(&slot.id, "ttl_expired").await {
            warn!(slot_id = %slot.id, error = %e, "Failed to terminate dynamic slot in DB");
        }

        // Unregister from SlotManager
        state.mission.unregister_dynamic_slot(&slot.id);
    }

    if !expired.is_empty() {
        info!(count = expired.len(), "Reaped expired dynamic slots");
    }

    // TTL warning: alert for slots expiring within 15 minutes
    if let Ok(expiring) = state.store.find_expiring_dynamic_slots(900).await {
        for slot in &expiring {
            debug!(slot_id = %slot.id, expires_at = %slot.expires_at, "Dynamic slot expiring soon (15min warning)");
        }
    }
}

/// Safety net: running Board tasks with no recent progress notes → Inbox reminder.
/// Runs every 5 ticks (~5 min). Deduplicates by checking existing unread inbox.
async fn check_stale_board_progress(state: &AppState) {
    let tick = state.stats.autopilot_ticks.load(std::sync::atomic::Ordering::Relaxed);
    if tick % 5 != 0 { return; }

    let running = state.store.list_board_tasks(Some("running"), false).await.unwrap_or_default();
    if running.is_empty() { return; }

    // Prefetch recent unread inbox for dedup
    let recent_inbox = state.store.get_inbox_messages(true, 50).await.unwrap_or_default();

    for task in &running {
        // Skip autopilot-managed tasks (they have their own watchdog above)
        if task.claim_executor_type.as_deref() == Some("pty_slot") { continue; }

        // Check latest note time
        let last_note_at = state.store.get_board_task_with_notes(task.id.as_str()).await
            .ok().flatten()
            .and_then(|r| r.notes.last().and_then(|n|
                chrono::DateTime::parse_from_rfc3339(&n.created_at).ok()
                    .map(|t| t.with_timezone(&chrono::Utc))
            ));

        let task_start = chrono::DateTime::parse_from_rfc3339(&task.updated_at)
            .ok()
            .map(|t| t.with_timezone(&chrono::Utc));

        let reference_time = last_note_at.or(task_start);
        if let Some(ref_time) = reference_time {
            let age_min = (chrono::Utc::now() - ref_time).num_minutes();
            if age_min >= 30 {
                // Dedup: skip if unread inbox already has a message about this task
                let task_prefix = &task.id.as_str()[..8.min(task.id.as_str().len())];
                let already_notified = recent_inbox.iter()
                    .any(|m| m.content.contains(task_prefix));
                if already_notified { continue; }

                let msg = missiond_core::types::InboxMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    task_id: task.id.to_string(),
                    from_role: "system".to_string(),
                    content: format!(
                        "Board 任务 '[{}] {}' 已运行 {}min 无进展更新。如已完成请标 done。",
                        task_prefix, task.title, age_min
                    ),
                    read: false,
                    created_at: chrono::Utc::now().timestamp(),
                };
                let _ = state.store.insert_inbox_message(&msg).await;
                debug!(task_id = %task.id, age_min, "Stale board task reminder sent to inbox");
            }
        }
    }
}

/// GC completed/failed jobs older than 30 minutes from the in-memory store.
async fn gc_completed_jobs(state: &AppState) {
    use missiond_core::types::AsyncJobStatus;

    let cutoff = chrono::Utc::now() - chrono::Duration::minutes(30);
    let cutoff_str = cutoff.to_rfc3339();

    let mut store = state.job_store.write().await;
    let before = store.len();
    store.retain(|_, job| {
        // Keep running jobs and recently completed ones
        if job.status == AsyncJobStatus::Running {
            return true;
        }
        match &job.completed_at {
            Some(t) => t.as_str() > cutoff_str.as_str(),
            None => true,
        }
    });
    let removed = before - store.len();
    if removed > 0 {
        debug!(removed, remaining = store.len(), "GC'd completed async jobs");
    }
}
