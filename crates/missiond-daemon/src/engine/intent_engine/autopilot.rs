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
use crate::memory_scheduler::{dispatch_queued_submit_tasks, schedule_memory_tasks, reap_stale_submit_tasks};
use crate::claude_md_sync::sync_claude_md;
use crate::engine::learning_engine;
use crate::supervisor::schedule_supervisor_patrol;
use crate::flow_engine::{execute_flow_task, ensure_autopilot_pty};

// @beacon: orchestration

/// Notify Jarvis conversation when an async task fails.
/// Extracts conversation_id from task metadata, writes error message, and emits event.
fn notify_jarvis_failure(state: &AppState, task: &missiond_core::types::BoardTask, reason: &str) {
    if task.category != "jarvis" { return; }
    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&task.description) {
        if let Some(conv_id) = meta.get("conversation_id").and_then(|v| v.as_str()) {
            if !conv_id.is_empty() {
                let error_msg = format!("❌ 后台任务执行失败：{}", reason);
                let _ = state.mission.db().router_chat_append_messages(conv_id, &[
                    ("assistant".to_string(), error_msg),
                ]);
                state.event_bus.publish_traced(
                    DaemonEvent::JarvisTaskCompleted {
                        conversation_id: conv_id.to_string(),
                        task_id: task.id.clone(),
                    },
                    TraceContext {
                        trace_id: Some(conv_id.to_string()),
                        summary: Some(format!("jarvis: task {} failed", &task.id[..8.min(task.id.len())])),
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
    match state.mission.db().complete_stale_conversations(&cutoff) {
        Ok(n) if n > 0 => info!(count = n, "Completed stale conversations"),
        Err(e) => warn!(error = %e, "Failed to complete stale conversations"),
        _ => {}
    }

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

        // Memory scheduler: realtime > deep > consolidation
        schedule_memory_tasks(state).await;
    }

    // FTS dirty flag rebuild: after kb_forget sets dirty, rebuild here
    match state.mission.db().kb_rebuild_fts_if_dirty() {
        Ok(true) => info!("autopilot: FTS index rebuilt (dirty flag)"),
        Err(e) => warn!(error = %e, "FTS dirty rebuild failed"),
        _ => {}
    }

    // Sync KB preferences + hot topics into CLAUDE.md
    sync_claude_md(state);

    // ── Learning Engine tick (KB GC, decision reaper, harvest, timeline) ──
    learning_engine::learning_tick(state).await;

    // Hot-reload LLM prompts from ~/.xjp-mission/prompts/ (every 10 ticks ≈ 10 min)
    if state.stats.autopilot_ticks.load(std::sync::atomic::Ordering::Relaxed) % 10 == 0 {
        state.prompts.reload();
    }

    // Reaper: force-fail stale slot tasks (pending/running > 30 min)
    match state.mission.db().reap_stale_slot_tasks(1800) {
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
    match state.mission.db().list_running_autopilot_tasks() {
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
                        let _ = state.mission.db().unclaim_board_task(&rt.id);
                        let _ = state.mission.db().add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: rt.id.clone(),
                                content: format!(
                                    "🔄 **看门狗回收** — 工位 {} 已 idle 但任务仍在 running（{}s），可能是 daemon 重启导致 send() 丢失。已 unclaim，下次 tick 重新执行。",
                                    slot_id, claimed_age
                                ),
                                note_type: Some("note".to_string()),
                                author: Some("watchdog".to_string()),
                            },
                        );
                    }
                } else {
                    // No PTY session at all — slot not even spawned, definitely orphaned
                    warn!(
                        task_id = %rt.id, slot_id, age_secs = claimed_age,
                        "Watchdog: no PTY session for slot — recovering orphaned task"
                    );
                    let _ = state.mission.db().unclaim_board_task(&rt.id);
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Watchdog: failed to list running autopilot tasks");
        }
        _ => {}
    }

    // Time-based fallback: recover running tasks stuck > 15 min (catch-all)
    let _ = state.mission.db().recover_stale_running_tasks(15);

    dispatch_board_tasks(state).await?;

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

    let tasks = state.mission.db().list_autopilot_tasks()
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
            match state.mission.db().check_dependencies(&task.depends_on) {
                Ok(missiond_core::types::DependencyStatus::Ready) => {
                    // All deps done — proceed
                }
                Ok(missiond_core::types::DependencyStatus::Pending) => {
                    debug!(task_id = %task.id, "Autopilot: DAG deps pending, skipping");
                    continue;
                }
                Ok(missiond_core::types::DependencyStatus::Blocked(reason)) => {
                    warn!(task_id = %task.id, reason = %reason, "Autopilot: DAG dep failed, blocking task");
                    let _ = state.mission.db().update_board_task(
                        &task.id,
                        &missiond_core::types::UpdateBoardTaskInput {
                            status: Some("blocked".to_string()),
                            ..Default::default()
                        },
                    );
                    state.event_bus.publish_traced(
                        DaemonEvent::BoardTaskStatusChanged {
                            task_id: task.id.clone(), old_status: format!("{:?}", task.status), new_status: "blocked".to_string(),
                        },
                        TraceContext { trace_id: Some(task.id.clone()), summary: Some(format!("board: {} → blocked", task.title)), ..Default::default() },
                    );
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!("因前置任务失败或取消，本任务自动阻塞。\n原因：{}", reason),
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        },
                    );
                    notify_jarvis_failure(state, &task, &format!("前置任务失败，本任务已阻塞：{}", reason));
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
                    task_id: task.id.clone(),
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
        match state.mission.db().claim_board_task(&task.id, &slot_id, "pty_slot") {
            Ok(Some(_)) => {
                dispatched_slots.insert(slot_id.clone());
                // Set lease: normal autopilot tasks get 20 minutes
                let lease = (chrono::Utc::now() + chrono::TimeDelta::minutes(20)).to_rfc3339();
                let _ = state.mission.db().set_board_task_lease(&task.id, &lease);
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
        if let Ok(Some(session_uuid)) = state.mission.db().get_slot_session(&slot_id) {
            let _ = state.mission.db().set_conversation_task_id(&session_uuid, &task.id);
        }

        // Inject answered questions as context (Phase 2 linkage)
        let full_prompt = {
            let answered = state.mission.db().list_questions_for_task(&task.id).unwrap_or_default();
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
                if let Ok(Some(dep_with_notes)) = state.mission.db().get_board_task_with_notes(dep_id) {
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

        // Emit dispatch event for timeline visibility
        {
            let preview = if full_prompt.len() > 200 {
                let mut end = 200;
                while end > 0 && !full_prompt.is_char_boundary(end) { end -= 1; }
                format!("{}...", &full_prompt[..end])
            } else { full_prompt.clone() };
            state.event_bus.publish(crate::event_bus::DaemonEvent::SlotTaskDispatched {
                slot_id: slot_id.clone(),
                task_id: Some(task.id.clone()),
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
            let _ = state.mission.db().unclaim_board_task(&task.id);
            continue;
        }
        if let Some(pre_send_status) = state.pty.get_status(&slot_id).await {
            if pre_send_status.state != SessionState::Idle {
                state.slot_dispatch.release(&slot_id);
                debug!(task_id = %task.id, slot_id = %slot_id, state = ?pre_send_status.state,
                    "Autopilot: slot not Idle pre-send, releasing task without penalty");
                let _ = state.mission.db().unclaim_board_task(&task.id);
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
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!("⚠️ **Auth Error** — slot {} OAuth token 可能已过期，需要 `/login`\n\n{}", slot_id, &truncate_safe(&res.response, 500)),
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        },
                    );
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
                        let _ = state.mission.db().update_board_task(
                            &task.id,
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("failed".to_string()),
                                ..Default::default()
                            },
                        );
                        state.event_bus.publish_traced(
                            DaemonEvent::BoardTaskStatusChanged {
                                task_id: task.id.clone(), old_status: format!("{:?}", task.status), new_status: "failed".to_string(),
                            },
                            TraceContext { trace_id: Some(task.id.clone()), summary: Some(format!("board: {} → failed", task.title)), ..Default::default() },
                        );
                        notify_jarvis_failure(state, &task, "OAuth token 过期，工位认证失败");
                    } else {
                        let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
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
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!(
                                "🚨 **Quota Exhausted** — API 配额耗尽，已自动激活全局暂停\n\nslot: {}\n\n{}",
                                slot_id,
                                &truncate_safe(&res.response, 500),
                            ),
                            note_type: Some("note".to_string()),
                            author: Some("circuit-breaker".to_string()),
                        },
                    );
                    // Return task to open for retry after quota resets
                    let _ = state.mission.db().update_board_task(
                        &task.id,
                        &missiond_core::types::UpdateBoardTaskInput {
                            status: Some("open".to_string()),
                            ..Default::default()
                        },
                    );
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
                    notify_jarvis_failure(state, &task, "API 配额耗尽，系统已全局暂停");
                    // Stop processing remaining tasks — quota is gone
                    break;
                }

                // Record result as a board note
                let note_content = format!("**Autopilot 执行完成** ({}ms)\n\n{}", res.duration_ms, res.response);
                let _ = state.mission.db().add_board_task_note(
                    &missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.clone(),
                        content: note_content,
                        note_type: Some("summary".to_string()),
                        author: Some("autopilot".to_string()),
                    },
                );
                // CAS guard: only mark done if task is still in 'running' state.
                // If task was auto-blocked by mission_question_create, preserve 'blocked' status.
                let current_status = state.mission.db().get_board_task(&task.id)
                    .ok().flatten()
                    .map(|t| t.status);
                match current_status {
                    Some(missiond_core::types::BoardTaskStatus::Done) => {
                        // Slot already self-closed the task
                        info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task already done (self-closed)");
                    }
                    Some(missiond_core::types::BoardTaskStatus::Blocked) => {
                        // Task was blocked by pending questions — do NOT overwrite
                        let _ = state.mission.db().add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.clone(),
                                content: "⚠️ PTY 执行已返回，但任务有未回答的 pending questions，保持 blocked 状态。".to_string(),
                                note_type: Some("note".to_string()),
                                author: Some("autopilot".to_string()),
                            },
                        );
                        warn!(task_id = %task.id, "Autopilot: pty.send completed but task is blocked — preserving blocked status");
                    }
                    _ => {
                        // Normal case: running → done
                        let _ = state.mission.db().update_board_task(
                            &task.id,
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("done".to_string()),
                                ..Default::default()
                            },
                        );
                        state.event_bus.publish_traced(
                            DaemonEvent::BoardTaskStatusChanged {
                                task_id: task.id.clone(), old_status: format!("{:?}", task.status), new_status: "done".to_string(),
                            },
                            TraceContext { trace_id: Some(task.id.clone()), summary: Some(format!("board: {} → done", task.title)), ..Default::default() },
                        );
                        info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task completed");
                    }
                }
                // Reset slot failure count on success
                {
                    let mut fail_map = state.slot_fail_counts.lock().unwrap();
                    fail_map.remove(&slot_id);
                }

                // Jarvis task post-completion: append result to conversation
                if task.category == "jarvis" {
                    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&task.description) {
                        if let Some(conv_id) = meta.get("conversation_id").and_then(|v| v.as_str()) {
                            if !conv_id.is_empty() {
                                let _ = state.mission.db().router_chat_append_messages(conv_id, &[
                                    ("assistant".to_string(), res.response.clone()),
                                ]);
                                state.event_bus.publish_traced(
                                    DaemonEvent::JarvisTaskCompleted {
                                        conversation_id: conv_id.to_string(),
                                        task_id: task.id.clone(),
                                    },
                                    TraceContext {
                                        trace_id: Some(conv_id.to_string()),
                                        summary: Some(format!("jarvis: task {} completed", &task.id[..8.min(task.id.len())])),
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
                    let _ = state.mission.db().unclaim_board_task(&task.id);
                } else {
                    // Real execution failure — track and retry
                    let note_content = format!("**Autopilot 执行失败** (retry {}/{})\n\n{}", task.retry_count + 1, task.max_retries, err_msg);
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: note_content,
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        },
                    );

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
                        let _ = state.mission.db().update_board_task(
                            &task.id,
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("failed".to_string()),
                                ..Default::default()
                            },
                        );
                        state.event_bus.publish_traced(
                            DaemonEvent::BoardTaskStatusChanged {
                                task_id: task.id.clone(), old_status: format!("{:?}", task.status), new_status: "failed".to_string(),
                            },
                            TraceContext { trace_id: Some(task.id.clone()), summary: Some(format!("board: {} → failed", task.title)), ..Default::default() },
                        );
                        warn!(task_id = %task.id, retries = new_retry, "Autopilot: task failed after max retries");
                        notify_jarvis_failure(state, &task, &err_msg);
                    } else {
                        // Back to open for retry, increment retry_count
                        let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
                        warn!(task_id = %task.id, retry = new_retry, max = task.max_retries, error = %err_msg, "Autopilot: task failed, will retry");
                    }
                }
            }
        }
    }

    Ok(())
}
