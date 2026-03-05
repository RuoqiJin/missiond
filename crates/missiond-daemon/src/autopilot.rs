use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::supervisor::{check_slot_context_levels, check_slot_stuck, strip_prompt_echo};
use crate::slot_env::{build_slot_tracking_env, capture_slot_session_uuid};
use crate::memory_scheduler::ensure_memory_slot_by_id;
use crate::decision_engine::harvest_decisions_for_task;
use crate::llm_gateway::{call_gemini_for_flow, determine_llm_env};
use missiond_core::SessionState;
use missiond_core::PTYSpawnOptions;
use std::sync::Arc;
use std::path::PathBuf;
use crate::helpers::default_mission_home;
use crate::supervisor::truncate_safe;
use crate::supervisor::is_auth_error;
use std::collections::HashSet;
use crate::memory_scheduler::{dispatch_queued_submit_tasks, schedule_memory_tasks, reap_stale_submit_tasks};
use crate::claude_md_sync::sync_claude_md;
use crate::extraction::check_kb_auto_gc;
use crate::decision_engine::reap_stale_decision_tasks;
use crate::supervisor::schedule_supervisor_patrol;

pub(crate) async fn autopilot_tick(state: &AppState) -> Result<()> {
    // Check PTY slots for low context — restart if < 10%
    check_slot_context_levels(state).await;

    // Complete stale active conversations (no messages for > 10 minutes)
    let cutoff = (chrono::Utc::now() - chrono::TimeDelta::minutes(10))
        .to_rfc3339();
    match state.mission.db().complete_stale_conversations(&cutoff) {
        Ok(n) if n > 0 => info!(count = n, "Completed stale conversations"),
        Err(e) => warn!(error = %e, "Failed to complete stale conversations"),
        _ => {}
    }

    let mut memory_paused = state.memory_paused.load(std::sync::atomic::Ordering::Relaxed);

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

    // Submit task dispatch — always runs, not gated by memory_paused
    dispatch_queued_submit_tasks(state).await;

    if !memory_paused {
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

    // KB auto-GC: every hour
    check_kb_auto_gc(state);

    // Reaper: force-fail stale slot tasks (pending/running > 30 min)
    match state.mission.db().reap_stale_slot_tasks(1800) {
        Ok(n) if n > 0 => warn!(count = n, "Reaped stale slot tasks"),
        Err(e) => warn!(error = %e, "Slot task reaper failed"),
        _ => {}
    }

    // Decision Engine reaper: 15min timeout for master questions
    reap_stale_decision_tasks(state).await;

    // Decision Engine: checkpoint harvester (every 24h, tasks with ≥3 unharvested decisions)
    {
        let now = chrono::Utc::now().timestamp();
        let last = state.mission.db().daemon_state_get("last_decision_harvest_at").unwrap_or(None).unwrap_or(0);
        if now - last > 86400 {
            let _ = state.mission.db().daemon_state_set("last_decision_harvest_at", now);
            if let Ok(tasks) = state.mission.db().find_tasks_with_unharvested_decisions(3) {
                for (task_id, task_title, count) in &tasks {
                    info!(task_id, count, "Decision harvester checkpoint: incremental harvest");
                    let state_clone = state.clone();
                    let tid = task_id.clone();
                    let tt = task_title.clone();
                    tokio::spawn(async move {
                        harvest_decisions_for_task(&state_clone, &tid, &tt).await;
                    });
                }
            }
        }
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

    let tasks = state.mission.db().list_autopilot_tasks()
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if tasks.is_empty() {
        return Ok(());
    }

    info!(count = tasks.len(), "Autopilot: found executable tasks");

    for task in tasks {
        let slot_id = match &task.assignee {
            Some(id) => id.clone(),
            None => continue,
        };

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
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!("因前置任务失败或取消，本任务自动阻塞。\n原因：{}", reason),
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        },
                    );
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

        // Inject context from Phase B skills
        let context = state.skills.build_context(&task.title);
        let full_prompt = if context.contains("No matching skills") {
            prompt
        } else {
            format!("{}\n\n{}", context, prompt)
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

        // Dynamic LLM model routing based on task characteristics
        let task_env = determine_llm_env(&task);

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
                    } else {
                        let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
                    }
                    continue;
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
                        info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task completed");
                    }
                }
                // Reset slot failure count on success
                {
                    let mut fail_map = state.slot_fail_counts.lock().unwrap();
                    fail_map.remove(&slot_id);
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
                // Record failure as a note
                let note_content = format!("**Autopilot 执行失败** (retry {}/{})\n\n{}", task.retry_count + 1, task.max_retries, e);
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
                    warn!(task_id = %task.id, retries = new_retry, "Autopilot: task failed after max retries");
                } else {
                    // Back to open for retry, increment retry_count
                    let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
                    warn!(task_id = %task.id, retry = new_retry, max = task.max_retries, error = %e, "Autopilot: task failed, will retry");
                }
            }
        }
    }

    Ok(())
}

/// Execute a flow-enabled Board task through the Engineering Flow Engine.
/// Handles all phase types: slot phases (send to PTY), daemon phases (call Gemini), and Done.
pub(crate) async fn execute_flow_task(state: &AppState, task: &missiond_core::types::BoardTask, slot_id: &str) -> Result<()> {
    // Reentry guard: skip if this task is already being processed
    {
        let mut in_progress = state.flow_in_progress.lock().unwrap();
        if !in_progress.insert(task.id.clone()) {
            debug!(task_id = %task.id, "Flow engine: task already in progress, skipping");
            return Ok(());
        }
    }
    // RAII guard to remove task from in-progress set on exit
    struct FlowGuard {
        set: Arc<std::sync::Mutex<HashSet<String>>>,
        id: String,
    }
    impl Drop for FlowGuard {
        fn drop(&mut self) {
            self.set.lock().unwrap().remove(&self.id);
        }
    }
    let _guard = FlowGuard {
        set: state.flow_in_progress.clone(),
        id: task.id.clone(),
    };

    let phase_str = task.flow_phase.as_deref().unwrap_or("investigate");
    let phase = missiond_core::types::EngineeringPhase::from_str(phase_str)
        .ok_or_else(|| anyhow!("Unknown flow phase: {}", phase_str))?;

    let mut ctx: missiond_core::types::FlowContext = task.flow_context
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    info!(task_id = %task.id, phase = %phase_str, slot_id, "Flow engine: processing phase");

    match phase {
        // === Done phase: mark task complete ===
        missiond_core::types::EngineeringPhase::Done => {
            let _ = state.mission.db().update_board_task(
                &task.id,
                &missiond_core::types::UpdateBoardTaskInput {
                    status: Some("done".to_string()),
                    ..Default::default()
                },
            );
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: "✅ Flow Engine: 全部阶段完成，任务标记 done".to_string(),
                    note_type: Some("progress".to_string()),
                    author: Some("flow-engine".to_string()),
                },
            );
            info!(task_id = %task.id, "Flow engine: task completed (all phases done)");

            // Decision Engine: harvest decisions from completed task
            let state_clone = state.clone();
            let task_id = task.id.clone();
            let task_title = task.title.clone();
            tokio::spawn(async move {
                harvest_decisions_for_task(&state_clone, &task_id, &task_title).await;
            });
        }

        // === Daemon phases: call Gemini directly ===
        p if p.is_daemon_phase() => {
            // Claim task as running + set lease
            let _ = state.mission.db().update_board_task(
                &task.id,
                &missiond_core::types::UpdateBoardTaskInput {
                    status: Some("running".to_string()),
                    ..Default::default()
                },
            );
            let lease = (chrono::Utc::now() + chrono::TimeDelta::seconds(p.timeout_secs() as i64 + 60)).to_rfc3339();
            let _ = state.mission.db().set_board_task_lease(&task.id, &lease);

            let (gemini_prompt, artifact_field) = match p {
                missiond_core::types::EngineeringPhase::ConsultGemini1 => {
                    let report = ctx.investigation_report.as_deref().unwrap_or("(无调查报告)");
                    (
                        format!(
                            "# 架构咨询\n\n## 任务\n{}\n\n## 描述\n{}\n\n## 代码调查报告\n{}\n\n请给出架构层面的解决方案和建议。重点关注：\n1. 技术选型与现有架构的兼容性\n2. 潜在的风险和边界情况\n3. 推荐的实现路径",
                            task.title, task.description, report
                        ),
                        "gemini_advice_1"
                    )
                }
                missiond_core::types::EngineeringPhase::ConsultGemini2 => {
                    let plan = ctx.execution_plan.as_deref().unwrap_or("(无执行方案)");
                    let advice1 = ctx.gemini_advice_1.as_deref().unwrap_or("");
                    (
                        format!(
                            "# 执行方案审查\n\n## 任务\n{}\n\n## 第一轮架构建议\n{}\n\n## 执行方案\n{}\n\n请审查此方案，指出：\n1. 遗漏或风险点\n2. 与第一轮建议的一致性\n3. 优化建议",
                            task.title, advice1, plan
                        ),
                        "gemini_advice_2"
                    )
                }
                _ => return Ok(()),
            };

            // Call Gemini via router API
            let gemini_response = call_gemini_for_flow(state, &task.id, &gemini_prompt).await;

            match gemini_response {
                Ok(response) => {
                    // Store artifact
                    match artifact_field {
                        "gemini_advice_1" => ctx.gemini_advice_1 = Some(response.clone()),
                        "gemini_advice_2" => ctx.gemini_advice_2 = Some(response.clone()),
                        _ => {}
                    }

                    // Advance phase — unclaim so next tick can re-claim for next phase
                    let next_phase = p.next().unwrap_or(missiond_core::types::EngineeringPhase::Done);
                    let _ = state.mission.db().update_board_task(
                        &task.id,
                        &missiond_core::types::UpdateBoardTaskInput {
                            flow_phase: Some(next_phase.as_str().to_string()),
                            flow_context: Some(serde_json::to_string(&ctx).unwrap_or_default()),
                            ..Default::default()
                        },
                    );
                    let _ = state.mission.db().unclaim_board_task(&task.id);

                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!(
                                "✅ {} 完成 → 进入 {}\n\nGemini 回复摘要 (前500字):\n{}",
                                p.display_name(),
                                next_phase.display_name(),
                                &truncate_safe(&response, 500)
                            ),
                            note_type: Some("progress".to_string()),
                            author: Some("flow-engine".to_string()),
                        },
                    );

                    info!(task_id = %task.id, from = %phase_str, to = %next_phase.as_str(), "Flow engine: daemon phase completed");
                }
                Err(e) => {
                    warn!(task_id = %task.id, phase = %phase_str, error = %e, "Flow engine: Gemini call failed");
                    let _ = state.mission.db().unclaim_board_task(&task.id);
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!("❌ {} Gemini 调用失败: {}", p.display_name(), e),
                            note_type: Some("note".to_string()),
                            author: Some("flow-engine".to_string()),
                        },
                    );
                }
            }
        }

        // === Slot phases: send prompt to PTY ===
        p if p.is_slot_phase() => {
            // Claim the task
            match state.mission.db().claim_board_task(&task.id, slot_id, "pty_slot") {
                Ok(Some(_)) => {
                    // Set lease based on phase timeout (e.g. Execute = 60min)
                    let lease = (chrono::Utc::now() + chrono::TimeDelta::seconds(p.timeout_secs() as i64 + 300)).to_rfc3339();
                    let _ = state.mission.db().set_board_task_lease(&task.id, &lease);
                }
                Ok(None) => {
                    debug!(task_id = %task.id, slot_id, "Flow engine: task already claimed, skipping");
                    return Ok(());
                }
                Err(e) => {
                    warn!(task_id = %task.id, error = %e, "Flow engine: failed to claim task");
                    return Ok(());
                }
            }

            // Ensure PTY is running (flow tasks also get model routing)
            let task_env = determine_llm_env(task);
            if !ensure_autopilot_pty(state, task, slot_id, task_env).await {
                return Ok(());
            }

            // Link PTY session to task for audit trail
            if let Ok(Some(session_uuid)) = state.mission.db().get_slot_session(slot_id) {
                let _ = state.mission.db().set_conversation_task_id(&session_uuid, &task.id);
            }

            // Build phase-specific prompt
            let prompt = build_flow_phase_prompt(task, &p, &ctx);

            // Inject answered Q&A context (Phase 0: Decision Engine prerequisite)
            let prompt = {
                let answered = state.mission.db().list_questions_for_task(&task.id).unwrap_or_default();
                if answered.is_empty() {
                    prompt
                } else {
                    let qa_block: String = answered.iter()
                        .filter(|q| q.answer.is_some())
                        .map(|q| format!("Q: {}\nA: {}", q.question, q.answer.as_deref().unwrap_or("")))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if qa_block.is_empty() {
                        prompt
                    } else {
                        format!("[决策与指示 (Decisions & Directives)]\n{}\n\n{}", qa_block, prompt)
                    }
                }
            };

            let timeout_ms = p.timeout_secs() * 1000;
            info!(task_id = %task.id, phase = %phase_str, timeout_ms, "Flow engine: sending phase prompt to PTY");

            match state.pty.send(slot_id, &prompt, timeout_ms).await {
                Ok(res) => {
                    // Check for auth errors
                    if is_auth_error(&res.response) {
                        warn!(task_id = %task.id, slot_id, phase = %phase_str, "Flow engine: auth error detected in PTY response");
                        let _ = state.mission.db().add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.clone(),
                                content: format!("⚠️ **Auth Error** — slot {} OAuth token 过期，Flow phase {} 中止", slot_id, p.display_name()),
                                note_type: Some("note".to_string()),
                                author: Some("flow-engine".to_string()),
                            },
                        );
                        // Back to open for retry + track failure
                        let _ = state.mission.db().unclaim_board_task(&task.id);
                        {
                            let mut fail_map = state.slot_fail_counts.lock().unwrap();
                            let entry = fail_map.entry(slot_id.to_string()).or_insert((0, 0));
                            entry.0 += 1;
                            entry.1 = chrono::Utc::now().timestamp();
                        }
                        return Ok(());
                    }

                    // Strip prompt echo from PTY response to reduce noise in board notes
                    let clean_response = strip_prompt_echo(&res.response, &prompt);
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!(
                                "**Flow Phase {} PTY 响应** ({}ms)\n\n{}",
                                p.display_name(),
                                res.duration_ms,
                                &truncate_safe(&clean_response, 2000)
                            ),
                            note_type: Some("progress".to_string()),
                            author: Some("flow-engine".to_string()),
                        },
                    );

                    // Phase advancement is handled by submit_phase_result MCP tool.
                    // After PTY response, check if phase was already advanced.
                    let updated_task = state.mission.db().get_board_task(&task.id);
                    if let Ok(Some(updated)) = updated_task {
                        let current_phase = updated.flow_phase.as_deref().unwrap_or(phase_str);
                        if current_phase != phase_str {
                            // Phase was advanced by submit_phase_result → unclaim for next tick
                            let _ = state.mission.db().unclaim_board_task(&task.id);
                            info!(task_id = %task.id, from = %phase_str, to = %current_phase, "Flow engine: phase advanced by slot");
                        } else {
                            // Slot didn't call submit_phase_result — possible stuck
                            warn!(task_id = %task.id, phase = %phase_str, "Flow engine: slot completed PTY but didn't submit phase result");
                            let _ = state.mission.db().unclaim_board_task(&task.id);
                        }
                    }

                    // Reset slot failure count
                    {
                        let mut fail_map = state.slot_fail_counts.lock().unwrap();
                        fail_map.remove(slot_id);
                    }
                }
                Err(e) => {
                    warn!(task_id = %task.id, phase = %phase_str, error = %e, "Flow engine: PTY send failed");
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!("❌ Flow Phase {} PTY 失败: {}", p.display_name(), e),
                            note_type: Some("note".to_string()),
                            author: Some("flow-engine".to_string()),
                        },
                    );
                    // Revert to open for retry
                    let _ = state.mission.db().unclaim_board_task(&task.id);
                    // Track failure
                    {
                        let mut fail_map = state.slot_fail_counts.lock().unwrap();
                        let entry = fail_map.entry(slot_id.to_string()).or_insert((0, 0));
                        entry.0 += 1;
                        entry.1 = chrono::Utc::now().timestamp();
                    }
                }
            }
        }

        _ => {}
    }

    Ok(())
}

// call_gemini_for_flow and determine_llm_env moved to llm_gateway.rs (Phase 2 S1)

/// Ensure a PTY session is running for the given slot (autopilot task execution).
/// Returns true if PTY is available, false if spawn failed.
/// `task_env`: task-specific env overrides (e.g., model routing) merged at spawn time.
pub(crate) async fn ensure_autopilot_pty(state: &AppState, task: &missiond_core::types::BoardTask, slot_id: &str, task_env: HashMap<String, String>) -> bool {
    // Check if session is already running
    if let Some(info) = state.pty.get_status(slot_id).await {
        if info.state != SessionState::Exited {
            // Check if model env changed — kill PTY and respawn if different
            let new_model = task_env.get("ANTHROPIC_MODEL").cloned().unwrap_or_default();
            let model_changed = {
                let models = state.slot_current_model.lock().unwrap();
                models.get(slot_id).map(|m| m != &new_model).unwrap_or(false)
            };
            if model_changed && !new_model.is_empty() {
                info!(task_id = %task.id, slot_id, new_model = %new_model, "Autopilot: model changed, killing PTY for respawn");
                let _ = state.pty.kill(slot_id).await;
                // Fall through to spawn below
            } else {
                return true;
            }
        }
    }

    // Find slot config
    let slot = state.mission.list_slots()
        .into_iter()
        .find(|s| s.config.id == slot_id);

    let Some(slot) = slot else {
        warn!(task_id = %task.id, slot_id, "Autopilot: slot not found, skipping");
        // Record failure note + increment retry
        let _ = state.mission.db().add_board_task_note(
            &missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.clone(),
                content: format!("❌ Slot `{}` 不存在，无法执行任务。请检查 slots.yaml 配置。", slot_id),
                note_type: Some("note".to_string()),
                author: Some("autopilot".to_string()),
            },
        );
        let new_retry = task.retry_count + 1;
        if new_retry >= task.max_retries {
            let _ = state.mission.db().update_board_task(
                &task.id,
                &missiond_core::types::UpdateBoardTaskInput {
                    status: Some("failed".to_string()),
                    ..Default::default()
                },
            );
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: format!("🛑 Slot `{}` 连续 {} 次不可用，任务标记为 failed。", slot_id, new_retry),
                    note_type: Some("note".to_string()),
                    author: Some("autopilot".to_string()),
                },
            );
            warn!(task_id = %task.id, retries = new_retry, "Autopilot: task failed — slot not found after max retries");
        } else {
            let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
            let _ = state.mission.db().unclaim_board_task(&task.id);
        }
        return false;
    };

    let pty_slot = missiond_core::PTYSlot {
        id: slot.config.id.clone(),
        role: slot.config.role.clone(),
        cwd: slot.config.cwd.as_deref().map(PathBuf::from),
    };
    let slot_env = slot.config.env.as_ref();
    let mcp_config = slot.config.mcp_config.clone().map(PathBuf::from);
    let (mut extra_env, session_file) = build_slot_tracking_env(slot_id, slot_env).await;

    // Merge task-level env overrides (model routing etc.) — task_env wins over slot defaults
    for (k, v) in &task_env {
        info!(task_id = %task.id, slot_id, key = %k, value = %v, "Autopilot: LLM route override");
        extra_env.insert(k.clone(), v.clone());
    }

    match state.pty.spawn(&pty_slot, PTYSpawnOptions {
        auto_restart: false,
        wait_for_idle: true,
        timeout_secs: Some(120),
        mcp_config,
        dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
        extra_env,
    }).await {
        Ok(_) => {
            capture_slot_session_uuid(state, slot_id, &session_file).await;
            // Record current model for future env-change detection
            if let Some(model) = task_env.get("ANTHROPIC_MODEL") {
                state.slot_current_model.lock().unwrap().insert(slot_id.to_string(), model.clone());
            }
            info!(task_id = %task.id, slot_id, "Autopilot: PTY spawned for task");
            true
        }
        Err(e) => {
            warn!(task_id = %task.id, slot_id, error = %e, "Autopilot: failed to spawn PTY (process may still be loading)");
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: format!("⏳ PTY spawn 失败（120s 超时）。\n\n{}", e),
                    note_type: Some("note".to_string()),
                    author: Some("autopilot".to_string()),
                },
            );
            // Retry with backoff or fail
            let new_retry = task.retry_count + 1;
            if new_retry >= task.max_retries {
                let _ = state.mission.db().update_board_task(
                    &task.id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        status: Some("failed".to_string()),
                        ..Default::default()
                    },
                );
                let _ = state.mission.db().add_board_task_note(
                    &missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.clone(),
                        content: format!("🛑 PTY spawn 连续 {} 次失败，任务标记为 failed。", new_retry),
                        note_type: Some("note".to_string()),
                        author: Some("autopilot".to_string()),
                    },
                );
            } else {
                let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
                let _ = state.mission.db().unclaim_board_task(&task.id);
            }
            false
        }
    }
}

/// Build phase-specific prompt for the AI slot to execute.
/// Write a flow artifact to a temp file, return the file path.
/// Falls back to inline if file write fails.
pub(crate) fn write_flow_artifact(task_id: &str, name: &str, content: &str) -> Option<String> {
    let dir = format!("/tmp/missiond-flow/{}", task_id);
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = format!("{}/{}.md", dir, name);
    if std::fs::write(&path, content).is_ok() {
        Some(path)
    } else {
        None
    }
}

/// Format an artifact reference: file path instruction if written, or inline fallback.
pub(crate) fn artifact_ref(task_id: &str, name: &str, label: &str, content: &str) -> String {
    if content.len() < 500 {
        // Short content: inline is fine
        return format!("### {}\n{}", label, content);
    }
    match write_flow_artifact(task_id, name, content) {
        Some(path) => format!(
            "### {}\n内容已保存到文件，请先读取：\n```\ncat {}\n```",
            label, path
        ),
        None => format!("### {}\n{}", label, content),
    }
}

pub(crate) fn build_flow_phase_prompt(
    task: &missiond_core::types::BoardTask,
    phase: &missiond_core::types::EngineeringPhase,
    ctx: &missiond_core::types::FlowContext,
) -> String {
    let task_id = &task.id;

    let mut base = match phase {
        missiond_core::types::EngineeringPhase::Investigate => {
            format!(
                r#"# 工程任务调查阶段

## 任务
**{title}**

{description}

## 你的工作
1. 阅读任务描述，理解需求
2. 调查相关代码，找到关键文件和依赖
3. 分析现有架构，识别影响范围
4. 记录发现的问题和约束条件

## 完成后
调用 `mission_submit_phase_result` 提交调查报告：
```
taskId: "{task_id}"
artifactType: "investigation_report"
content: "<你的调查报告，包含关键文件、架构分析、问题清单>"
```"#,
                title = task.title,
                description = task.description,
                task_id = task_id,
            )
        }

        missiond_core::types::EngineeringPhase::Plan => {
            let investigation = ctx.investigation_report.as_deref().unwrap_or("(无调查报告)");
            let gemini_advice = ctx.gemini_advice_1.as_deref().unwrap_or("(无 Gemini 建议)");
            let inv_ref = artifact_ref(task_id, "investigation_report", "调查报告", investigation);
            let gem_ref = artifact_ref(task_id, "gemini_advice_1", "Gemini 架构建议", gemini_advice);
            format!(
                r#"# 执行方案制定阶段

## 任务
**{title}**

{description}

## 前置信息

{inv_ref}

{gem_ref}

## 你的工作
1. 综合调查报告和 Gemini 建议，制定详细执行方案
2. 进行第二轮精确调查（验证 Gemini 建议的可行性）
3. 列出具体的代码变更清单（文件、函数、改动内容）
4. 识别风险点和回退方案

## 完成后
调用 `mission_submit_phase_result` 提交执行方案：
```
taskId: "{task_id}"
artifactType: "execution_plan"
content: "<详细执行方案，包含变更清单、风险分析、实施步骤>"
```"#,
                title = task.title,
                description = task.description,
                inv_ref = inv_ref,
                gem_ref = gem_ref,
                task_id = task_id,
            )
        }

        missiond_core::types::EngineeringPhase::Execute => {
            let plan = ctx.execution_plan.as_deref().unwrap_or("(无执行方案)");
            let gemini_advice2 = ctx.gemini_advice_2.as_deref().unwrap_or("(无 Gemini 审查意见)");
            let plan_ref = artifact_ref(task_id, "execution_plan", "执行方案", plan);
            let gem2_ref = artifact_ref(task_id, "gemini_advice_2", "Gemini 方案审查意见", gemini_advice2);
            format!(
                r#"# 执行阶段

## 任务
**{title}**

{plan_ref}

{gem2_ref}

## 你的工作
1. 根据执行方案和 Gemini 审查意见，开始编码实现
2. 遇到问题记录在 execution_result 中
3. 确保代码质量：类型安全、错误处理、测试

## 完成后
调用 `mission_submit_phase_result` 提交执行结果：
```
taskId: "{task_id}"
artifactType: "execution_result"
content: "<执行结果摘要：完成的变更、测试结果、遗留问题>"
```"#,
                title = task.title,
                plan_ref = plan_ref,
                gem2_ref = gem2_ref,
                task_id = task_id,
            )
        }

        missiond_core::types::EngineeringPhase::Finalize => {
            let result = ctx.execution_result.as_deref().unwrap_or("(无执行结果)");
            let result_ref = artifact_ref(task_id, "execution_result", "执行结果", result);
            format!(
                r#"# 收尾阶段

## 任务
**{title}**

{result_ref}

## 你的工作
1. 创建 git commit（描述清晰的 commit message）
2. 如有需要，更新相关 Skill 文件记录新知识
3. 确认所有变更已提交

## 完成后
调用 `mission_submit_phase_result` 提交 commit hash：
```
taskId: "{task_id}"
artifactType: "commit_hash"
content: "<commit hash 或提交摘要>"
```"#,
                title = task.title,
                result_ref = result_ref,
                task_id = task_id,
            )
        }

        _ => format!("Flow phase {:?} — no prompt template available", phase),
    };

    // Inject Decision Engine help protocol for all slot phases (with taskId for auto-linkage)
    if phase.is_slot_phase() {
        let protocol = format!(r#"

---
【主控求助协议】
当你遇到**阻断性困境**时，严禁自行盲目尝试超过 3 次或随意猜测架构意图。调用 `mission_question_create(target="master", taskId="{task_id}")` 呼叫主控。
**呼叫条件与 decisionType 映射（严格遵守）：**
1. `architecture`：涉及引入新依赖、修改数据库表、变更核心状态机（必须呼叫）
2. `risk`：发现方案可能破坏现有功能或数据（必须呼叫）
3. `implementation`：有两种可行方案无法权衡（附带 options）
4. `investigation`：遇到不熟悉的黑盒 API（附带已查阅的上下文）
5. `debug`：同一致命报错尝试修复 2 次仍失败（附带报错和尝试记录）

**参数要求：** 必须在 `options` 中提供分析或候选项（如 "A: 修改基类, B: 新增 wrapper"），不能只抛出问题。"#, task_id = task_id);
        base.push_str(&protocol);
    }

    base
}

// Slot ID constants moved to state.rs (Phase 2 S1)

/// Detect if a new unknown session is a compacted replacement for an active slot session.
///
/// When Claude Code runs out of context, it compacts into a new session (new JSONL file).
/// The old session stops being written to, but the PTY process continues.
/// We detect this by checking if any active slot has a session in the same project directory.
///
/// Returns (slot_id, old_session_id, old_task_id) if compaction is detected.
pub(crate) fn detect_compaction(
    state: &AppState,
    new_session_id: &str,
    new_project: &str,
) -> Option<(String, String, Option<String>)> {
    let db = state.mission.db();
    let all_slot_sessions = db.get_all_slot_sessions().ok()?;

    for (slot_id, old_uuid) in &all_slot_sessions {
        if old_uuid == new_session_id {
            continue; // Same session, not compaction
        }
        let old_conv = db.get_conversation(old_uuid).ok()??;
        // Must be same project and still active
        if old_conv.project.as_deref() != Some(new_project) || old_conv.status != "active" {
            continue;
        }
        // The old session should have been written to recently (within 10 min)
        // to avoid false positives with stale slot sessions.
        // Use updated_at (last message time) when available, fall back to started_at.
        let last_active = old_conv.updated_at.as_deref()
            .unwrap_or(&old_conv.started_at);
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(last_active) {
            let age = chrono::Utc::now().signed_duration_since(t);
            if age > chrono::Duration::minutes(10) {
                continue; // No messages in last 10 min — not a live compaction
            }
        }
        return Some((
            slot_id.clone(),
            old_uuid.clone(),
            old_conv.task_id.clone(),
        ));
    }
    None
}
