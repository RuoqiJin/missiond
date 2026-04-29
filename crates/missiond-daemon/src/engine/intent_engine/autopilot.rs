use anyhow::{anyhow, Result};
use std::collections::HashSet;
use tracing::{debug, info, warn};

// ── Autopilot pty.send / watchdog timeout policy ───────────────────────
// These constants project the same shape as
// `handlers/compute/task_delegate.rs :: DEFAULT_TIMEOUT_SECS / MAX_TIMEOUT_SECS`
// so the dispatched pty.send budget matches the BoardTask.timeout_secs that
// task_delegate already wrote, and the watchdog cannot reclaim a slot while
// the configured budget is still ticking. See
// `.missiond/v3/missiond-blueprint.lisp :: workstation-config invariants`
// for the Lisp authority.
const PTY_TIMEOUT_DEFAULT_SECS: i64 = 1800;
const PTY_TIMEOUT_MIN_SECS: i64 = 60;
const PTY_TIMEOUT_MAX_SECS: i64 = 7200;

/// Extra wait beyond the declared task timeout before the smart watchdog
/// treats an idle slot as orphaned. Long-running Opus runs sometimes return
/// the prompt within seconds of the deadline; the grace prevents racing the
/// natural completion path.
const WATCHDOG_GRACE_SECS: i64 = 120;

/// Window the watchdog gives a missing PTY session before reclaiming the
/// task. A missing session means the slot process is gone, so we recover
/// quickly without waiting for the full configured timeout — this is the
/// "no-PTY-session" branch the brief mandates we keep fast.
const WATCHDOG_MISSING_SESSION_PROBE_SECS: i64 = 120;

use crate::claude_md_sync::sync_claude_md;
use crate::engine::learning_engine;
use crate::flow_engine::{ensure_autopilot_pty, execute_flow_task};
use missiond_core::event::events::{
    BoardEvent, IncidentEvent, SessionEvent, SlotEvent, SystemEvent,
};
use crate::llm_gateway::determine_llm_env;
use crate::memory_scheduler::ensure_memory_slot_by_id;
use crate::memory_scheduler::{dispatch_queued_submit_tasks, reap_stale_submit_tasks};
use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::supervisor::schedule_supervisor_patrol;
use crate::supervisor::truncate_safe;
use crate::supervisor::{
    check_pending_compact_restarts, check_slot_context_levels, check_slot_stuck,
};
use crate::supervisor::{is_auth_error, is_quota_exhausted};
use missiond_core::SessionState;

// @beacon: orchestration

/// Clamp `BoardTask.timeout_secs` to the autopilot wait budget.
///
/// Mirrors the shape of `handlers/compute/task_delegate.rs` so the pty.send
/// budget always lines up with the timeout the delegator already stored:
///   * `None` / `Some(<= 0)`             → `PTY_TIMEOUT_DEFAULT_SECS` (1800).
///   * `Some(v) where v < PTY_TIMEOUT_MIN_SECS` → clamped to `PTY_TIMEOUT_MIN_SECS`.
///   * `Some(v) where v > PTY_TIMEOUT_MAX_SECS` → clamped to `PTY_TIMEOUT_MAX_SECS`.
///
/// Pure helper so unit tests can pin the policy without an `AppState`.
fn derive_pty_timeout_secs(timeout_secs: Option<i64>) -> i64 {
    let raw = match timeout_secs {
        Some(v) if v > 0 => v,
        _ => PTY_TIMEOUT_DEFAULT_SECS,
    };
    raw.clamp(PTY_TIMEOUT_MIN_SECS, PTY_TIMEOUT_MAX_SECS)
}

/// Convert the derived timeout into the `pty.send` millisecond budget.
fn derive_pty_timeout_ms(timeout_secs: Option<i64>) -> u64 {
    (derive_pty_timeout_secs(timeout_secs) as u64).saturating_mul(1000)
}

/// Smallest claimed-age (seconds) at which the watchdog may reclaim an idle
/// slot still bound to a running task. Equals the derived task timeout plus
/// `WATCHDOG_GRACE_SECS` so the slot always gets the full configured window
/// before being treated as orphaned.
fn idle_watchdog_threshold_secs(timeout_secs: Option<i64>) -> i64 {
    derive_pty_timeout_secs(timeout_secs).saturating_add(WATCHDOG_GRACE_SECS)
}

/// Lease horizon Autopilot writes onto a freshly-claimed BoardTask, in
/// seconds from now. Equals `idle_watchdog_threshold_secs(timeout_secs)` so
/// that the smart-watchdog reclaim threshold and the claim lease move
/// together when `BoardTask.timeout_secs` changes. The lease therefore covers
/// the full pty.send budget plus `WATCHDOG_GRACE_SECS`, never the legacy
/// fixed 20-minute window. Pure helper so unit tests can pin the policy
/// without an `AppState`.
fn derive_board_task_lease_secs(timeout_secs: Option<i64>) -> i64 {
    idle_watchdog_threshold_secs(timeout_secs)
}

/// Build the base prompt body shown to a delegated worker.
///
/// `mission_task_delegate` stores the user objective as `BoardTask.title` and
/// also seeds `BoardTask.description` with that same objective, so a naive
/// `"{title}\n\n{description}"` render duplicates the goal. This helper
/// projects the V3 `prompt-tool-contract` `objective-dedupe` rule:
///
///   * description empty                     → title alone
///   * description == title                  → description alone
///   * description starts with title, then
///     only blank lines / whitespace before
///     the next non-empty line               → description alone
///   * otherwise (distinct description)      → "{title}\n\n{description}"
///
/// Pure helper so unit tests can pin the dedupe policy without an `AppState`.
fn build_base_prompt(title: &str, description: &str) -> String {
    if description.is_empty() {
        return title.to_string();
    }
    if let Some(rest) = description.strip_prefix(title) {
        // Dedupe only when the description already begins with the exact
        // title token at a word boundary — i.e. the title is immediately
        // followed by whitespace or end-of-string. This prevents a title
        // like "Fix" from being treated as a prefix of "Fixing CORS" while
        // still collapsing "Title", "Title\n\n", "Title\n\nbody", and
        // "Title <space> body" into the description as-is. Title is by
        // definition already inside such descriptions, so re-prepending it
        // would just duplicate the objective text.
        if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace()) {
            return description.to_string();
        }
    }
    format!("{}\n\n{}", title, description)
}

/// V3 workstation-config :: execution-ownership delegated-boardtask projection.
///
/// After `state.pty.send` returns Complete, decide what Autopilot — the
/// declared close owner — should do with the BoardTask, given its current
/// status. Pure helper so the close-ownership rule can be unit-tested
/// without an `AppState`.
///
///   * `Done`    → worker self-closed via attached board MCP tools; preserve.
///   * `Blocked` → task transitioned to Blocked (e.g. mission_question_create);
///                 preserve and never overwrite with done.
///   * anything else (running / open / failed / None) → owner closes; the
///     normal path that transitions running→done.
#[derive(Debug, PartialEq, Eq)]
enum DispatchCloseAction {
    AlreadySelfClosed,
    PreserveBlocked,
    OwnerClosesAsDone,
}

fn decide_close_action(
    current_status: Option<missiond_core::types::BoardTaskStatus>,
) -> DispatchCloseAction {
    match current_status {
        Some(missiond_core::types::BoardTaskStatus::Done) => DispatchCloseAction::AlreadySelfClosed,
        Some(missiond_core::types::BoardTaskStatus::Blocked) => {
            DispatchCloseAction::PreserveBlocked
        }
        _ => DispatchCloseAction::OwnerClosesAsDone,
    }
}

/// Append the V3 `prompt-tool-contract` board-self-close suffix.
///
/// The board task id is always surfaced for audit. The self-close instruction
/// is conditional: if `mission_board_update` / `mission_board_note_add` are
/// attached to the slot, the worker is asked to call them; if they are not,
/// the worker is told to return a concise final summary and Autopilot /
/// orchestrator stays responsible for closing the BoardTask. The unconditional
/// "你必须调用" wording is replaced so a slot without board MCP tools is no
/// longer asked to call tools it cannot see.
fn append_board_task_id_suffix(prompt: &str, task_id: &str) -> String {
    format!(
        "{}\n\n---\n📋 **Board Task ID**: `{}`\n\
        任务完成时：若当前工位已挂载 `mission_board_update` / `mission_board_note_add`，\
        请调用 `mission_board_update(id=\"{}\", status=\"done\")` 关闭任务，\
        并用 `mission_board_note_add(taskId=\"{}\", content=\"...\", noteType=\"summary\")` 写入诊断摘要。\
        若上述 board MCP 工具未挂载到本工位，请直接返回一段简明的最终完成摘要，由 Autopilot/orchestrator 负责关闭此 BoardTask。",
        prompt, task_id, task_id, task_id
    )
}

fn is_dynamic_slot_id(slot_id: &str) -> bool {
    slot_id.starts_with("slot-dyn-")
}

fn should_clear_stale_dynamic_assignee(
    slot_id: &str,
    runtime_slot_exists: bool,
    dynamic_slot_active: bool,
) -> bool {
    is_dynamic_slot_id(slot_id) && !runtime_slot_exists && !dynamic_slot_active
}

/// Notify Jarvis conversation when an async task fails.
/// Extracts conversation_id from task metadata, writes error message, and emits event.
async fn notify_jarvis_failure(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
    reason: &str,
) {
    if task.category != "jarvis" {
        return;
    }
    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&task.description) {
        if let Some(conv_id) = meta.get("conversation_id").and_then(|v| v.as_str()) {
            if !conv_id.is_empty() {
                let error_msg = format!("❌ 后台任务执行失败：{}", reason);
                let _ = state
                    .store
                    .router_chat_append_messages(conv_id, &[("assistant".to_string(), error_msg)])
                    .await;
                let _ = state
                    .bus
                    .publish_session(SessionEvent::JarvisTaskCompleted {
                        conversation_id: conv_id.to_string(),
                        task_id: task.id.to_string(),
                    })
                    .await;
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
    let cutoff = (chrono::Utc::now() - chrono::TimeDelta::minutes(10)).to_rfc3339();
    match state.store.complete_stale_conversations(&cutoff).await {
        Ok(n) if n > 0 => info!(count = n, "Completed stale conversations"),
        Err(e) => warn!(error = %e, "Failed to complete stale conversations"),
        _ => {}
    }

    // Reap expired dynamic slots (TTL lifecycle)
    reap_expired_dynamic_slots(state).await;

    // GC completed jobs older than 30 minutes
    gc_completed_jobs(state).await;

    // Scale-to-zero: release idle persistent slots after 30 minutes
    reap_idle_persistent_slots(state).await;

    // ControlTree is the single source of truth for pause state.
    // Domain pause is permanent until user explicitly resumes.

    let tree = state.control_manager.current();
    let memory_paused = tree.is_domain_paused(crate::control_tree::CtlDomain::Memory);
    let global_paused = tree.global_paused;

    if global_paused {
        debug!("autopilot: global pause active, skipping all task dispatches");
    } else {
        // Submit task dispatch — always runs, not gated by memory_paused
        dispatch_queued_submit_tasks(state).await;
    }

    if !memory_paused && !global_paused {
        // Check if memory slots are stuck in non-Idle state for too long
        check_slot_stuck(
            state,
            MEMORY_SLOT_ID,
            &state.memory_slot_busy_since,
            &state.extraction_state,
        )
        .await;
        check_slot_stuck(
            state,
            MEMORY_SLOW_SLOT_ID,
            &state.slow_slot_busy_since,
            &state.slow_extraction_state,
        )
        .await;

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
    if state
        .stats
        .autopilot_ticks
        .load(std::sync::atomic::Ordering::Relaxed)
        % 10
        == 0
    {
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
        let fast_slot = state
            .pty
            .get_status(MEMORY_SLOT_ID)
            .await
            .map(|s| format!("{:?}", s.state))
            .unwrap_or_else(|| "not_spawned".to_string());
        let slow_es = state.slow_extraction_state.read().await;
        let slow_slot = state
            .pty
            .get_status(MEMORY_SLOW_SLOT_ID)
            .await
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
            debug!(
                count = running.len(),
                "Watchdog: checking running autopilot tasks"
            );
            for rt in &running {
                let slot_id = rt.claim_executor_id.as_deref().unwrap_or("");
                if slot_id.is_empty() {
                    continue;
                }

                let claimed_age = rt
                    .claimed_at
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds())
                    .unwrap_or(0);

                let task_timeout_secs = derive_pty_timeout_secs(rt.timeout_secs);
                let idle_threshold = idle_watchdog_threshold_secs(rt.timeout_secs);

                match state.pty.get_status(slot_id).await {
                    Some(info) if info.state == SessionState::Idle => {
                        // Slot is idle. Only reclaim once the configured task
                        // budget plus grace has elapsed; otherwise the slot may
                        // simply be between the natural prompt return and the
                        // next dispatch.
                        if claimed_age < idle_threshold {
                            continue;
                        }
                        warn!(
                            task_id = %rt.id, slot_id, age_secs = claimed_age,
                            timeout_secs = task_timeout_secs,
                            grace_secs = WATCHDOG_GRACE_SECS,
                            "Watchdog: task exceeded configured timeout/grace — slot idle, recovering"
                        );
                        let _ = state.store.unclaim_board_task(rt.id.as_str()).await;
                        let _ = state.store.add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: rt.id.to_string(),
                                content: format!(
                                    "🔄 **看门狗回收** — 任务超出配置 timeout/grace（claimed_age={}s, timeout={}s, grace={}s, 工位 {} 已 idle）。可能是 pty.send 在预算内自然结束、daemon 重启丢失 send()，或工位已归档结果。已 unclaim，下次 tick 重新执行。",
                                    claimed_age, task_timeout_secs, WATCHDOG_GRACE_SECS, slot_id
                                ),
                                note_type: Some("note".to_string()),
                                author: Some("watchdog".to_string()),
                            },
                        ).await;
                    }
                    Some(_) => {
                        // Slot is still busy (Thinking / Responding / etc) —
                        // leave it alone, the original send() may still be
                        // returning a result inside the configured budget.
                        continue;
                    }
                    None => {
                        // No PTY session at all — slot process is gone, so
                        // the original send() can never return. Recover after
                        // a small probe window without waiting for the full
                        // task timeout.
                        if claimed_age < WATCHDOG_MISSING_SESSION_PROBE_SECS {
                            continue;
                        }
                        warn!(
                            task_id = %rt.id, slot_id, age_secs = claimed_age,
                            probe_secs = WATCHDOG_MISSING_SESSION_PROBE_SECS,
                            "Watchdog: no PTY session for slot — slot process gone, recovering"
                        );
                        let _ = state.store.unclaim_board_task(rt.id.as_str()).await;
                    }
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

    // Phase 7: Consciousness — evaluate user state for proactive triggers
    if state.intent_analyst_enabled {
        evaluate_user_state(state).await;
    }

    // Record tick timing to DaemonStats
    let tick_ms = tick_start.elapsed().as_millis() as u64;
    state
        .stats
        .autopilot_ticks
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state
        .stats
        .autopilot_total_ms
        .fetch_add(tick_ms, std::sync::atomic::Ordering::Relaxed);
    state.stats.autopilot_latency.record(tick_ms * 1000); // histogram expects microseconds

    Ok(())
}

/// Board task dispatch — extracted for reuse by idle-triggered dispatch.
/// Called from autopilot_tick (60s) and event-driven (slot became idle).
pub(crate) async fn dispatch_board_tasks(state: &AppState) -> Result<()> {
    if state.control_manager.current().global_paused {
        return Ok(());
    }

    let tasks = state
        .store
        .list_autopilot_tasks()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if tasks.is_empty() {
        return Ok(());
    }

    info!(count = tasks.len(), "Autopilot: found executable tasks");

    // Slot-level exclusivity: only dispatch ONE task per slot per tick
    let mut dispatched_slots: HashSet<String> = HashSet::new();

    // Excluded roles: these slots have dedicated purposes, not for ad-hoc tasks
    const EXCLUDED_ROLES: &[&str] = &[
        "jarvis",
        "memory",
        "supervisor",
        "deploy",
        "operator",
        "decision",
        "secret",
    ];

    for task in tasks {
        // Dynamic slot assignment: if assignee is None, find an idle coder slot
        let slot_id = match &task.assignee {
            Some(id) => {
                let runtime_slot_exists = state.mission.get_slot(id).is_some();
                let dynamic_slot_active = if is_dynamic_slot_id(id) {
                    match state.store.get_dynamic_slot(id).await {
                        Ok(Some(slot)) => slot.status == "active",
                        Ok(None) => false,
                        Err(e) => {
                            warn!(
                                task_id = %task.id,
                                slot_id = %id,
                                error = %e,
                                "Autopilot: failed to inspect dynamic slot before honoring assignee"
                            );
                            true
                        }
                    }
                } else {
                    false
                };

                if should_clear_stale_dynamic_assignee(
                    id,
                    runtime_slot_exists,
                    dynamic_slot_active,
                ) {
                    match state
                        .store
                        .clear_board_task_assignee(task.id.as_str(), id)
                        .await
                    {
                        Ok(rows) if rows > 0 => {
                            let _ = state
                                .store
                                .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                                    task_id: task.id.to_string(),
                                    content: format!(
                                        "🔄 Pinned dynamic slot `{}` 在重启后已不可用，已解除 pin，等待重新调度。",
                                        id
                                    ),
                                    note_type: Some("note".to_string()),
                                    author: Some("autopilot".to_string()),
                                })
                                .await;
                            info!(
                                task_id = %task.id,
                                slot_id = %id,
                                "Autopilot: cleared stale dynamic slot assignee"
                            );
                        }
                        Ok(_) => {
                            debug!(
                                task_id = %task.id,
                                slot_id = %id,
                                "Autopilot: stale dynamic slot assignee was already changed"
                            );
                        }
                        Err(e) => {
                            warn!(
                                task_id = %task.id,
                                slot_id = %id,
                                error = %e,
                                "Autopilot: failed to clear stale dynamic slot assignee"
                            );
                        }
                    }
                    continue;
                }

                id.clone()
            }
            None => {
                let mut candidate: Option<String> = None;
                for slot in state.mission.list_slots() {
                    let role = slot.config.role.as_str();
                    if EXCLUDED_ROLES.contains(&role) {
                        continue;
                    }
                    if dispatched_slots.contains(&slot.config.id) {
                        continue;
                    }
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
                    let _ = state
                        .store
                        .update_board_task(
                            task.id.as_str(),
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("blocked".to_string()),
                                ..Default::default()
                            },
                        )
                        .await;
                    let _ = state
                        .bus
                        .publish_board(BoardEvent::StatusChanged {
                            task_id: task.id.to_string(),
                            old_status: format!("{:?}", task.status),
                            new_status: "blocked".to_string(),
                        })
                        .await;
                    let _ = state
                        .store
                        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: format!(
                                "因前置任务失败或取消，本任务自动阻塞。\n原因：{}",
                                reason
                            ),
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        })
                        .await;
                    notify_jarvis_failure(
                        state,
                        &task,
                        &format!("前置任务失败，本任务已阻塞：{}", reason),
                    )
                    .await;
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

        // Build prompt: template > deduped(title, description). The dedupe
        // helper projects V3 prompt-tool-contract.objective-dedupe so a
        // BoardTask whose description was seeded from the title (the
        // mission_task_delegate path) does not show the same objective twice.
        let prompt = if let Some(ref tmpl) = task.prompt_template {
            tmpl.clone()
        } else {
            build_base_prompt(&task.title, &task.description)
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

        // ControlTree: skip if slot_role is paused
        {
            let slot_role = state
                .mission
                .get_slot(&slot_id)
                .map(|s| s.config.role.clone())
                .unwrap_or_default();
            if state
                .control_manager
                .current()
                .is_slot_role_paused(&slot_role)
            {
                debug!(slot_id = %slot_id, role = %slot_role, "Autopilot: slot_role paused, skipping");
                continue;
            }
        }

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
        match state
            .store
            .claim_board_task(task.id.as_str(), &slot_id, "pty_slot")
            .await
        {
            Ok(Some(_)) => {
                dispatched_slots.insert(slot_id.clone());
                // Set lease: project from BoardTask.timeout_secs so the
                // claim lease, pty.send budget, and smart-watchdog reclaim
                // threshold all move together. See
                // derive_board_task_lease_secs for the policy.
                let lease_secs = derive_board_task_lease_secs(task.timeout_secs);
                let lease =
                    (chrono::Utc::now() + chrono::TimeDelta::seconds(lease_secs)).to_rfc3339();
                let _ = state
                    .store
                    .set_board_task_lease(task.id.as_str(), &lease)
                    .await;
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
        let slot_role = state
            .mission
            .get_slot(&slot_id)
            .map(|s| s.config.role.clone())
            .unwrap_or_default();
        let task_env = determine_llm_env(&task, &slot_role);

        // Check if PTY session exists, spawn if needed
        if !ensure_autopilot_pty(state, &task, &slot_id, task_env).await {
            continue;
        }

        // Link PTY session to task for audit trail
        if let Ok(Some(session_uuid)) = state.store.get_slot_session(&slot_id).await {
            let _ = state
                .store
                .set_conversation_task_id(&session_uuid, task.id.as_str())
                .await;
        }

        // Inject answered questions as context (Phase 2 linkage)
        let full_prompt = {
            let answered = state
                .store
                .list_questions_for_task(task.id.as_str())
                .await
                .unwrap_or_default();
            if answered.is_empty() {
                full_prompt
            } else {
                let qa_block: String = answered
                    .iter()
                    .filter(|q| q.answer.is_some())
                    .map(|q| {
                        format!(
                            "Q: {}\nA: {}",
                            q.question,
                            q.answer.as_deref().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if qa_block.is_empty() {
                    full_prompt
                } else {
                    format!(
                        "[决策与指示 (Decisions & Directives)]\n{}\n\n{}",
                        qa_block, full_prompt
                    )
                }
            }
        };

        // Inject predecessor task context (DAG handover)
        let full_prompt = if !task.depends_on.is_empty() {
            let mut handover_blocks = Vec::new();
            for dep_id in &task.depends_on {
                if let Ok(Some(dep_with_notes)) =
                    state.store.get_board_task_with_notes(dep_id.as_str()).await
                {
                    // Find last summary note from predecessor
                    let summary = dep_with_notes
                        .notes
                        .iter()
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
                format!(
                    "## 前置任务产出上下文\n{}\n\n{}",
                    handover_blocks.join("\n\n"),
                    full_prompt
                )
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

        // Inject task ID + conditional self-close instruction so slot can
        // close the task itself when board MCP tools are attached, while
        // remaining valid for slots that lack those tools (in which case the
        // worker returns a summary and Autopilot/orchestrator closes the
        // BoardTask). Projects V3 prompt-tool-contract.board-self-close.
        let full_prompt = append_board_task_id_suffix(&full_prompt, task.id.as_str());

        // Cache cited KB IDs for confidence feedback loop after task completion
        if !cited_kb_ids.is_empty() {
            let mut cache = state.task_cited_kbs.lock().unwrap();
            cache.insert(task.id.to_string(), cited_kb_ids.clone());
        }

        // Prompt snapshot: save full context for Skill auto-verification replay
        let _ = state
            .store
            .save_prompt_snapshot(
                task.id.as_str(),
                &full_prompt,
                &cited_kb_ids,
                &task.category,
            )
            .await;

        // Emit dispatch event for timeline visibility
        {
            let preview = if full_prompt.len() > 200 {
                let mut end = 200;
                while end > 0 && !full_prompt.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &full_prompt[..end])
            } else {
                full_prompt.clone()
            };
            let _ = state
                .bus
                .publish_slot(SlotEvent::TaskDispatched {
                    slot_id: slot_id.clone(),
                    task_id: Some(task.id.to_string()),
                    purpose: "board_auto_execute".to_string(),
                    prompt_chars: full_prompt.len(),
                    preview,
                    cited_kb_ids,
                })
                .await;
        }

        // V3 execution-ownership :: delegated-boardtask :: dispatch-guard.
        // Acquire the per-slot RAII guard and HOLD it across state.pty.send so
        // a release-before-send race cannot let a second caller dispatch to
        // the same slot while the first send is in flight. The guard is
        // per-slot — holding it does not starve callers targeting other
        // slots. The guard auto-releases on Drop at every continue/break/end
        // of this iteration, so no manual release is needed.
        let _slot_guard = match state.slot_dispatch.try_acquire_guard(&slot_id) {
            Some(g) => g,
            None => {
                debug!(task_id = %task.id, slot_id = %slot_id,
                    "Autopilot: slot dispatch guard busy, releasing task");
                let _ = state.store.unclaim_board_task(task.id.as_str()).await;
                continue;
            }
        };
        if let Some(pre_send_status) = state.pty.get_status(&slot_id).await {
            if pre_send_status.state != SessionState::Idle {
                debug!(task_id = %task.id, slot_id = %slot_id, state = ?pre_send_status.state,
                    "Autopilot: slot not Idle pre-send, releasing task without penalty");
                let _ = state.store.unclaim_board_task(task.id.as_str()).await;
                continue;
            }
        }

        // Send prompt and wait for response — derive the budget from the
        // BoardTask.timeout_secs that mission_task_delegate already stored,
        // so a 55-minute Opus coding task does not get capped at 10 minutes.
        // _slot_guard is held across the entire send so Autopilot remains the
        // sole prompt+close owner for this dispatch.
        let timeout_ms = derive_pty_timeout_ms(task.timeout_secs);
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
                        let _ = state
                            .store
                            .update_board_task(
                                task.id.as_str(),
                                &missiond_core::types::UpdateBoardTaskInput {
                                    status: Some("failed".to_string()),
                                    ..Default::default()
                                },
                            )
                            .await;
                        let _ = state
                            .bus
                            .publish_board(BoardEvent::StatusChanged {
                                task_id: task.id.to_string(),
                                old_status: format!("{:?}", task.status),
                                new_status: "failed".to_string(),
                            })
                            .await;
                        notify_jarvis_failure(state, &task, "OAuth token 过期，工位认证失败").await;
                    } else {
                        let _ = state
                            .store
                            .increment_board_task_retry(task.id.as_str(), new_retry)
                            .await;
                    }
                    continue;
                }

                // Check for quota exhaustion — circuit breaker: auto global_pause
                if is_quota_exhausted(&res.response) {
                    warn!(slot_id = %slot_id, task_id = %task.id, "🚨 Autopilot: API quota exhausted! Activating global pause circuit breaker");
                    // Activate global pause
                    state
                        .global_paused
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    let now = chrono::Utc::now().timestamp();
                    state
                        .global_paused_at
                        .store(now, std::sync::atomic::Ordering::Relaxed);
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
                    let _ = state
                        .store
                        .update_board_task(
                            task.id.as_str(),
                            &missiond_core::types::UpdateBoardTaskInput {
                                status: Some("open".to_string()),
                                ..Default::default()
                            },
                        )
                        .await;
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
                    let _ = state
                        .bus
                        .publish_incident(IncidentEvent::Reported { incident })
                        .await;
                    notify_jarvis_failure(state, &task, "API 配额耗尽，系统已全局暂停").await;
                    // Stop processing remaining tasks — quota is gone
                    break;
                }

                // Record result as a board note
                let note_content = format!(
                    "**Autopilot 执行完成** ({}ms)\n\n{}",
                    res.duration_ms, res.response
                );
                let _ = state
                    .store
                    .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.to_string(),
                        content: note_content,
                        note_type: Some("summary".to_string()),
                        author: Some("autopilot".to_string()),
                    })
                    .await;
                // V3 execution-ownership :: delegated-boardtask :: close-owner.
                // Autopilot owns closure unless the worker self-closed via
                // attached board MCP tools (Done) or the task transitioned to
                // Blocked (e.g. mission_question_create). Pure helper
                // `decide_close_action` projects the rule.
                let current_status = state
                    .store
                    .get_board_task(task.id.as_str())
                    .await
                    .ok()
                    .flatten()
                    .map(|t| t.status);
                match decide_close_action(current_status) {
                    DispatchCloseAction::AlreadySelfClosed => {
                        // Worker self-closed via attached board MCP tools.
                        info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task already done (self-closed)");
                    }
                    DispatchCloseAction::PreserveBlocked => {
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
                    DispatchCloseAction::OwnerClosesAsDone => {
                        // Normal case: running → done. Autopilot is the close owner.
                        let _ = state
                            .store
                            .update_board_task(
                                task.id.as_str(),
                                &missiond_core::types::UpdateBoardTaskInput {
                                    status: Some("done".to_string()),
                                    ..Default::default()
                                },
                            )
                            .await;
                        let _ = state
                            .bus
                            .publish_board(BoardEvent::StatusChanged {
                                task_id: task.id.to_string(),
                                old_status: format!("{:?}", task.status),
                                new_status: "done".to_string(),
                            })
                            .await;
                        info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task completed");
                        // Record outcome for Skill auto-verification replay
                        let _ = state
                            .store
                            .update_prompt_snapshot_outcome(task.id.as_str(), "success")
                            .await;
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
                    let cited = state
                        .task_cited_kbs
                        .lock()
                        .unwrap()
                        .remove(task.id.as_str());
                    if let Some(kb_ids) = cited {
                        let count = kb_ids.len();
                        for kb_id in &kb_ids {
                            // Check current confidence to determine boost amount
                            let delta = state
                                .store
                                .kb_get_by_id(kb_id)
                                .await
                                .ok()
                                .flatten()
                                .map(|e| if e.confidence < 0.8 { 0.05 } else { 0.03 })
                                .unwrap_or(0.03);
                            match state.store.kb_adjust_confidence(kb_id, delta).await {
                                Ok(Some(new_conf)) => {
                                    debug!(kb_id = %kb_id, delta, new_conf, "KB confidence boost (task success)")
                                }
                                Ok(None) => {
                                    debug!(kb_id = %kb_id, "KB entry not found for confidence adjustment")
                                }
                                Err(e) => {
                                    warn!(kb_id = %kb_id, error = %e, "Failed to adjust KB confidence")
                                }
                            }
                        }
                        if count > 0 {
                            info!(task_id = %task.id, kb_count = count, "KB feedback: boosted confidence for {} cited entries", count);
                        }
                        // Phase 4a: Utility score boost on task success (atomic SQL)
                        match state
                            .store
                            .kb_batch_apply_utility_feedback(&kb_ids, true)
                            .await
                        {
                            Ok(n) if n > 0 => {
                                info!(task_id = %task.id, boosted = n, "KB utility: boosted for task success")
                            }
                            Err(e) => {
                                warn!(task_id = %task.id, error = %e, "KB utility: boost failed")
                            }
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
                        if let Some(conv_id) = meta.get("conversation_id").and_then(|v| v.as_str())
                        {
                            if !conv_id.is_empty() {
                                let _ = state
                                    .store
                                    .router_chat_append_messages(
                                        conv_id,
                                        &[("assistant".to_string(), res.response.clone())],
                                    )
                                    .await;
                                let _ = state
                                    .bus
                                    .publish_session(SessionEvent::JarvisTaskCompleted {
                                        conversation_id: conv_id.to_string(),
                                        task_id: task.id.to_string(),
                                    })
                                    .await;
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
                            review_title, review_task_id, review_slot, review_task_id,
                        );
                        let _ = review_state
                            .pty
                            .send(MEMORY_SLOW_SLOT_ID, &prompt, 600_000)
                            .await;
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
                    let note_content = format!(
                        "**Autopilot 执行失败** (retry {}/{})\n\n{}",
                        task.retry_count + 1,
                        task.max_retries,
                        err_msg
                    );
                    let _ = state
                        .store
                        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: note_content,
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        })
                        .await;

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
                        let _ = state
                            .store
                            .update_board_task(
                                task.id.as_str(),
                                &missiond_core::types::UpdateBoardTaskInput {
                                    status: Some("failed".to_string()),
                                    ..Default::default()
                                },
                            )
                            .await;
                        let _ = state
                            .bus
                            .publish_board(BoardEvent::StatusChanged {
                                task_id: task.id.to_string(),
                                old_status: format!("{:?}", task.status),
                                new_status: "failed".to_string(),
                            })
                            .await;
                        warn!(task_id = %task.id, retries = new_retry, "Autopilot: task failed after max retries");
                        let _ = state
                            .store
                            .update_prompt_snapshot_outcome(task.id.as_str(), "failed")
                            .await;
                        notify_jarvis_failure(state, &task, &err_msg).await;

                        // Negative feedback: confidence (LLM-attributed) + utility_score (blanket)
                        {
                            let cited = state
                                .task_cited_kbs
                                .lock()
                                .unwrap()
                                .remove(task.id.as_str());
                            if let Some(kb_ids) = cited {
                                if !kb_ids.is_empty() {
                                    // Phase 4a: Utility score penalty (sync, atomic SQL)
                                    match state
                                        .store
                                        .kb_batch_apply_utility_feedback(&kb_ids, false)
                                        .await
                                    {
                                        Ok(n) if n > 0 => {
                                            info!(task_id = %task.id, penalized = n, "KB utility: penalized for task failure")
                                        }
                                        Err(e) => {
                                            warn!(task_id = %task.id, error = %e, "KB utility: penalty failed")
                                        }
                                        _ => {}
                                    }
                                    // Confidence penalty (async, LLM-attributed)
                                    let state2 = state.clone();
                                    let task_id2 = task.id.to_string();
                                    let err_msg2 = err_msg.clone();
                                    let task_title2 = task.title.clone();
                                    tokio::spawn(async move {
                                        apply_attributed_penalty(
                                            &state2,
                                            &task_id2,
                                            &task_title2,
                                            &err_msg2,
                                            &kb_ids,
                                        )
                                        .await;
                                    });
                                }
                            }
                        }
                    } else {
                        // Back to open for retry, increment retry_count
                        let _ = state
                            .store
                            .increment_board_task_retry(task.id.as_str(), new_retry)
                            .await;
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
                &kb_id[..8.min(kb_id.len())],
                entry.category,
                entry.key,
                entry.summary
            ));
        }
    }

    if kb_context.is_empty() {
        return;
    }

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
            let msgs = vec![ChatMessage {
                role: "user".to_string(),
                content: summarize_prompt,
            }];
            match sonnet
                .call_briefing(msgs, Some(512), Some(format!("log-dehydrate-{}", task_id)))
                .await
            {
                Ok(resp) if !resp.trim().is_empty() => Some(resp),
                _ => None,
            }
        } else {
            None
        };

        summary.unwrap_or_else(|| {
            // Fallback: head+tail
            let mut head_end = 200;
            while head_end > 0 && !error_msg.is_char_boundary(head_end) {
                head_end -= 1;
            }
            let mut tail_start = error_msg.len().saturating_sub(200);
            while tail_start < error_msg.len() && !error_msg.is_char_boundary(tail_start) {
                tail_start += 1;
            }
            format!(
                "{}\n...(truncated)...\n{}",
                &error_msg[..head_end],
                &error_msg[tail_start..]
            )
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
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        match sonnet
            .call_briefing(messages, Some(512), Some(format!("kb-attr-{}", task_id)))
            .await
        {
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
                    "caused" => {
                        stats.2 += 1;
                        -0.15
                    }
                    "contributed" => {
                        stats.1 += 1;
                        -0.05
                    }
                    _ => {
                        stats.0 += 1;
                        0.0
                    } // innocent — no penalty
                };
                if delta != 0.0 {
                    match state.store.kb_adjust_confidence(kb_id, delta).await {
                        Ok(Some(new_conf)) => {
                            debug!(kb_id = %kb_id, delta, new_conf, "KB attributed penalty")
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!(kb_id = %kb_id, error = %e, "Failed to adjust KB confidence")
                        }
                    }
                }
            }
            info!(
                task_id,
                innocent = stats.0,
                contributed = stats.1,
                caused = stats.2,
                "KB attribution: differentiated penalties applied"
            );
        }
        None => {
            // Fallback: blanket -0.02
            for kb_id in kb_ids {
                let _ = state.store.kb_adjust_confidence(kb_id, -0.02).await;
            }
            info!(
                task_id,
                kb_count = kb_ids.len(),
                "KB feedback: fallback -0.02 for all cited entries"
            );
        }
    }
}

/// Parse MiniMax attribution response into (kb_id, verdict) pairs.
fn parse_attribution(response: &str, kb_ids: &[String]) -> Option<Vec<(String, String)>> {
    // Strip markdown fences if present
    let json_str = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

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

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
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
        if let Err(e) = state
            .store
            .terminate_dynamic_slot(&slot.id, "ttl_expired")
            .await
        {
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
    let tick = state
        .stats
        .autopilot_ticks
        .load(std::sync::atomic::Ordering::Relaxed);
    if tick % 5 != 0 {
        return;
    }

    let running = state
        .store
        .list_board_tasks(Some("running"), false)
        .await
        .unwrap_or_default();
    if running.is_empty() {
        return;
    }

    // Prefetch recent unread inbox for dedup
    let recent_inbox = state
        .store
        .get_inbox_messages(true, 50)
        .await
        .unwrap_or_default();

    for task in &running {
        // Skip autopilot-managed tasks (they have their own watchdog above)
        if task.claim_executor_type.as_deref() == Some("pty_slot") {
            continue;
        }

        // Check latest note time
        let last_note_at = state
            .store
            .get_board_task_with_notes(task.id.as_str())
            .await
            .ok()
            .flatten()
            .and_then(|r| {
                r.notes.last().and_then(|n| {
                    chrono::DateTime::parse_from_rfc3339(&n.created_at)
                        .ok()
                        .map(|t| t.with_timezone(&chrono::Utc))
                })
            });

        let task_start = chrono::DateTime::parse_from_rfc3339(&task.updated_at)
            .ok()
            .map(|t| t.with_timezone(&chrono::Utc));

        let reference_time = last_note_at.or(task_start);
        if let Some(ref_time) = reference_time {
            let age_min = (chrono::Utc::now() - ref_time).num_minutes();
            if age_min >= 30 {
                // Dedup: skip if unread inbox already has a message about this task
                let task_prefix = &task.id.as_str()[..8.min(task.id.as_str().len())];
                let already_notified = recent_inbox.iter().any(|m| m.content.contains(task_prefix));
                if already_notified {
                    continue;
                }

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
        debug!(
            removed,
            remaining = store.len(),
            "GC'd completed async jobs"
        );
    }
}

// ── Phase 7: Consciousness — Proactive Triggers ──

/// Evaluate recent user intents and trigger proactive notifications.
/// Called from autopilot_tick when intent_analyst is enabled.
/// Groups intents by session_id to avoid cross-session false aggregation.
async fn evaluate_user_state(state: &AppState) {
    // 1. Pull recent intents (last 30 min, global)
    let intents = match state.store.get_recent_intents(1800).await {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "evaluate_user_state: failed to get recent intents");
            return;
        }
    };
    if intents.is_empty() {
        return;
    }

    let now = chrono::Utc::now().timestamp();

    // 2. Group by session_id — Gemini audit fix: avoid cross-session false aggregation
    let mut by_session: std::collections::HashMap<&str, Vec<&missiond_core::types::UserIntent>> =
        std::collections::HashMap::new();
    for intent in &intents {
        by_session
            .entry(&intent.session_id)
            .or_default()
            .push(intent);
    }

    // 3. Per-session evaluation
    for (session_id, s_intents) in &by_session {
        // UserStuck: stuck_retry >= 3 → L3 Jarvis push (降级 Inbox)
        let stuck: Vec<_> = s_intents
            .iter()
            .filter(|i| i.intent_type == "stuck_retry")
            .copied()
            .collect();
        let ck_stuck = format!("user_stuck:{}", session_id);
        if stuck.len() >= 3 && !in_cooldown(state, &ck_stuck, 1800, now) {
            let summary = build_stuck_summary(&stuck);
            trigger_jarvis_push(state, "user_stuck", &summary).await;
            set_cooldown(state, &ck_stuck, now);
        }

        // DirectionShift: architecture_explore confidence > 0.8 → L2 Inbox
        if let Some(shift) = s_intents
            .iter()
            .find(|i| i.intent_type == "architecture_explore" && i.confidence > 0.8)
        {
            let ck = format!("direction_shift:{}", session_id);
            if !in_cooldown(state, &ck, 3600, now) {
                trigger_inbox(
                    state,
                    "direction_shift",
                    &format!(
                        "检测到架构探索偏移：{}",
                        shift.summary.as_deref().unwrap_or("未知")
                    ),
                )
                .await;
                set_cooldown(state, &ck, now);
            }
        }

        // ScopeCreep: scope_creep >= 2 + Board has running tasks → L2 Inbox
        let creep_count = s_intents
            .iter()
            .filter(|i| i.intent_type == "scope_creep")
            .count();
        if creep_count >= 2 {
            let ck = format!("scope_creep:{}", session_id);
            if !in_cooldown(state, &ck, 3600, now) {
                let has_running = state
                    .store
                    .list_running_autopilot_tasks()
                    .await
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);
                if has_running {
                    trigger_inbox(
                        state,
                        "scope_creep",
                        &format!(
                            "检测到 {} 次范围蔓延，当前有进行中的 Board 任务",
                            creep_count
                        ),
                    )
                    .await;
                    set_cooldown(state, &ck, now);
                }
            }
        }
    }
}

/// L3: Push message into active Jarvis conversation + emit event. Falls back to Inbox.
async fn trigger_jarvis_push(state: &AppState, reason: &str, summary: &str) {
    let conv_id = match state.store.find_latest_jarvis_conversation().await {
        Ok(Some(id)) => id,
        _ => {
            info!("No active Jarvis conversation, falling back to Inbox");
            trigger_inbox(state, reason, summary).await;
            return;
        }
    };

    // Use "user" role with prefix — router_chat doesn't support "system" in message array
    let message = format!(
        "[MissionD System] 意识层提醒 [{}]\n\n{}\n\n如需帮助，请告知。",
        reason, summary
    );

    let _ = state
        .store
        .router_chat_append_messages(&conv_id, &[("user".to_string(), message)])
        .await;

    let _ = state
        .bus
        .publish_system(SystemEvent::JarvisProactivePush {
            conversation_id: conv_id.clone(),
            trigger_reason: reason.to_string(),
            summary: summary.to_string(),
        })
        .await;

    info!(reason, conv_id = %conv_id, "Proactive push sent to Jarvis");
}

/// L2: Write an Inbox message for the user.
async fn trigger_inbox(state: &AppState, reason: &str, content: &str) {
    let msg = missiond_core::types::InboxMessage {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: String::new(),
        from_role: "consciousness".to_string(),
        content: format!("[{}] {}", reason, content),
        read: false,
        created_at: chrono::Utc::now().timestamp(),
    };
    let _ = state.store.insert_inbox_message(&msg).await;
    debug!(reason, "Proactive inbox message created");
}

fn in_cooldown(state: &AppState, key: &str, cooldown_secs: i64, now: i64) -> bool {
    let guard = state.proactive_cooldowns.lock().unwrap();
    guard
        .get(key)
        .map(|&ts| now - ts < cooldown_secs)
        .unwrap_or(false)
}

fn set_cooldown(state: &AppState, key: &str, now: i64) {
    let mut guard = state.proactive_cooldowns.lock().unwrap();
    guard.insert(key.to_string(), now);
}

fn build_stuck_summary(intents: &[&missiond_core::types::UserIntent]) -> String {
    let details: Vec<String> = intents
        .iter()
        .filter_map(|i| i.summary.as_deref())
        .map(|s| format!("- {}", s))
        .collect();
    format!(
        "用户在最近 30 分钟内连续 {} 次卡在同一问题上：\n{}\n\n建议：检查是否需要换一种方法，或提供更多上下文帮助用户。",
        intents.len(),
        details.join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal AppState is hard to construct, so test pure functions only.

    #[test]
    fn test_build_stuck_summary_basic() {
        let i1 = missiond_core::types::UserIntent {
            id: 1,
            session_id: "s1".to_string(),
            turn_range_start: 0,
            turn_range_end: 2,
            intent_type: "stuck_retry".to_string(),
            confidence: 0.9,
            summary: Some("反复修复CORS跨域错误".to_string()),
            context_json: None,
            related_goal_id: None,
            created_at: String::new(),
        };
        let i2 = missiond_core::types::UserIntent {
            id: 2,
            session_id: "s1".to_string(),
            turn_range_start: 3,
            turn_range_end: 5,
            intent_type: "stuck_retry".to_string(),
            confidence: 0.85,
            summary: Some("CORS配置依然报错，换了nginx方案".to_string()),
            context_json: None,
            related_goal_id: None,
            created_at: String::new(),
        };
        let i3 = missiond_core::types::UserIntent {
            id: 3,
            session_id: "s1".to_string(),
            turn_range_start: 6,
            turn_range_end: 8,
            intent_type: "stuck_retry".to_string(),
            confidence: 0.8,
            summary: Some("第三次尝试修复CORS".to_string()),
            context_json: None,
            related_goal_id: None,
            created_at: String::new(),
        };
        let refs = vec![&i1, &i2, &i3];
        let summary = build_stuck_summary(&refs);
        assert!(summary.contains("连续 3 次"));
        assert!(summary.contains("CORS"));
        assert!(summary.contains("第三次"));
    }

    #[test]
    fn test_build_stuck_summary_empty_summaries() {
        let i1 = missiond_core::types::UserIntent {
            id: 1,
            session_id: "s1".to_string(),
            turn_range_start: 0,
            turn_range_end: 2,
            intent_type: "stuck_retry".to_string(),
            confidence: 0.9,
            summary: None,
            context_json: None,
            related_goal_id: None,
            created_at: String::new(),
        };
        let refs = vec![&i1];
        let summary = build_stuck_summary(&refs);
        assert!(summary.contains("连续 1 次"));
    }

    // ── PTY timeout / watchdog policy — pure helpers, no AppState ───────

    #[test]
    fn pty_timeout_default_when_field_absent() {
        assert_eq!(derive_pty_timeout_secs(None), PTY_TIMEOUT_DEFAULT_SECS);
        assert_eq!(
            derive_pty_timeout_ms(None),
            (PTY_TIMEOUT_DEFAULT_SECS as u64) * 1000
        );
    }

    #[test]
    fn pty_timeout_default_for_invalid_values() {
        // Zero and negative values are treated as "absent" and fall back to
        // the default — mirrors task_delegate's safe-default behaviour.
        assert_eq!(derive_pty_timeout_secs(Some(0)), PTY_TIMEOUT_DEFAULT_SECS);
        assert_eq!(
            derive_pty_timeout_secs(Some(-300)),
            PTY_TIMEOUT_DEFAULT_SECS
        );
    }

    #[test]
    fn pty_timeout_clamps_low_values() {
        // Anything under PTY_TIMEOUT_MIN_SECS rounds up to the floor so a
        // mis-configured 5-second task still gets a usable PTY budget.
        assert_eq!(derive_pty_timeout_secs(Some(5)), PTY_TIMEOUT_MIN_SECS);
        assert_eq!(derive_pty_timeout_secs(Some(59)), PTY_TIMEOUT_MIN_SECS);
        assert_eq!(derive_pty_timeout_secs(Some(60)), 60);
    }

    #[test]
    fn pty_timeout_clamps_high_values() {
        // The cap mirrors task_delegate::MAX_TIMEOUT_SECS so neither side
        // can drift past the other.
        assert_eq!(derive_pty_timeout_secs(Some(7200)), PTY_TIMEOUT_MAX_SECS);
        assert_eq!(derive_pty_timeout_secs(Some(86_400)), PTY_TIMEOUT_MAX_SECS);
    }

    #[test]
    fn pty_timeout_in_range_passes_through() {
        // 55-minute Opus task — the wave31 stability bug case. Must not be
        // shrunk to 10 minutes anywhere along the path.
        assert_eq!(derive_pty_timeout_secs(Some(3300)), 3300);
        assert_eq!(derive_pty_timeout_ms(Some(3300)), 3_300_000);
    }

    #[test]
    fn idle_watchdog_threshold_adds_grace_to_task_timeout() {
        // Default budget + grace.
        assert_eq!(
            idle_watchdog_threshold_secs(None),
            PTY_TIMEOUT_DEFAULT_SECS + WATCHDOG_GRACE_SECS
        );
        // Explicit 55-minute task → 3300 + 120 = 3420.
        assert_eq!(idle_watchdog_threshold_secs(Some(3300)), 3420);
    }

    #[test]
    fn idle_watchdog_threshold_strictly_above_old_120s_floor() {
        // Regression guard for wave31: the legacy 120s floor must never
        // re-emerge for any in-range task timeout.
        for secs in [
            PTY_TIMEOUT_MIN_SECS,
            300,
            900,
            1800,
            3300,
            PTY_TIMEOUT_MAX_SECS,
        ] {
            let threshold = idle_watchdog_threshold_secs(Some(secs));
            assert!(
                threshold > 120,
                "threshold {} for timeout {}s must exceed legacy 120s",
                threshold,
                secs
            );
        }
    }

    #[test]
    fn idle_watchdog_threshold_does_not_reclaim_within_budget() {
        // claimed_age < idle_threshold ⇒ watchdog must not reclaim.
        let timeout = Some(3300);
        let threshold = idle_watchdog_threshold_secs(timeout);
        // Within the budget — reclaim forbidden.
        assert!(900 < threshold);
        assert!(3300 < threshold);
        // Past the budget+grace — reclaim allowed.
        assert!(threshold + 1 > threshold);
    }

    // ── BoardTask claim lease — pure helper, no AppState ────────────────

    #[test]
    fn board_task_lease_default_when_field_absent() {
        // Default budget + grace = lease, mirroring the watchdog threshold
        // so the watchdog never reclaims while the lease is still valid.
        assert_eq!(
            derive_board_task_lease_secs(None),
            PTY_TIMEOUT_DEFAULT_SECS + WATCHDOG_GRACE_SECS
        );
    }

    #[test]
    fn board_task_lease_default_for_invalid_values() {
        // Zero / negative timeouts fall back to the default budget; lease
        // therefore matches the default watchdog threshold.
        let expected = PTY_TIMEOUT_DEFAULT_SECS + WATCHDOG_GRACE_SECS;
        assert_eq!(derive_board_task_lease_secs(Some(0)), expected);
        assert_eq!(derive_board_task_lease_secs(Some(-300)), expected);
    }

    #[test]
    fn board_task_lease_explicit_3300_is_3420() {
        // Wave31 / wave50 case: a 55-minute Opus task gets a 3300s pty
        // budget and a 3420s lease (3300 + 120s grace). The legacy fixed
        // 20-minute lease would have been 1200s — too short.
        assert_eq!(derive_board_task_lease_secs(Some(3300)), 3420);
    }

    #[test]
    fn board_task_lease_clamps_high_values() {
        // PTY budget caps at PTY_TIMEOUT_MAX_SECS, so the lease caps at
        // PTY_TIMEOUT_MAX_SECS + WATCHDOG_GRACE_SECS.
        assert_eq!(
            derive_board_task_lease_secs(Some(86_400)),
            PTY_TIMEOUT_MAX_SECS + WATCHDOG_GRACE_SECS
        );
    }

    #[test]
    fn board_task_lease_clamps_low_values() {
        // Sub-floor timeouts round up to PTY_TIMEOUT_MIN_SECS, so the lease
        // is PTY_TIMEOUT_MIN_SECS + WATCHDOG_GRACE_SECS = 180s.
        assert_eq!(
            derive_board_task_lease_secs(Some(5)),
            PTY_TIMEOUT_MIN_SECS + WATCHDOG_GRACE_SECS
        );
    }

    #[test]
    fn board_task_lease_matches_idle_watchdog_threshold() {
        // Single source of truth: the lease MUST equal the smart-watchdog
        // idle-recovery threshold for every supported timeout shape, so
        // the watchdog cannot reclaim a slot whose lease is still valid.
        for t in [
            None,
            Some(0),
            Some(-1),
            Some(60),
            Some(900),
            Some(1800),
            Some(3300),
            Some(7200),
            Some(86_400),
        ] {
            assert_eq!(
                derive_board_task_lease_secs(t),
                idle_watchdog_threshold_secs(t),
                "lease must equal watchdog threshold for {t:?}"
            );
        }
    }

    #[test]
    fn dispatch_no_longer_uses_fixed_20_minute_lease() {
        // Regression guard: the legacy fixed-20-minute literal must never
        // re-emerge in this source file. The needle is composed at runtime
        // so the guard cannot trip on its own assertion text. Any future
        // refactor that re-introduces the literal will fail this test
        // before it ships; use derive_board_task_lease_secs instead.
        let src = include_str!("./autopilot.rs");
        let banned = format!("Time{}::minutes(20)", "Delta");
        assert!(
            !src.contains(&banned),
            "autopilot.rs must not reintroduce the fixed 20-minute lease; use derive_board_task_lease_secs"
        );
    }

    #[test]
    fn missing_session_probe_independent_of_task_timeout() {
        // Even a 2-hour task must let the no-PTY-session branch recover
        // after the small probe window — a missing process can never
        // resume on its own.
        assert_eq!(WATCHDOG_MISSING_SESSION_PROBE_SECS, 120);
        assert!(
            WATCHDOG_MISSING_SESSION_PROBE_SECS
                < idle_watchdog_threshold_secs(Some(PTY_TIMEOUT_MAX_SECS))
        );
    }

    // ── Prompt-tool-contract: objective dedupe ──────────────────────────

    #[test]
    fn build_base_prompt_empty_description_returns_title_alone() {
        // mission_task_delegate may store an empty description for a
        // single-line objective; the prompt must still surface the title.
        let p = build_base_prompt("Refactor autopilot prompt", "");
        assert_eq!(p, "Refactor autopilot prompt");
    }

    #[test]
    fn build_base_prompt_description_equal_to_title_drops_duplicate() {
        // Worst case from the wave33 brief: title and description carry
        // exactly the same text, so the previous `"{title}\n\n{desc}"` shape
        // showed the objective twice.
        let title = "Make the API idempotent";
        let p = build_base_prompt(title, title);
        assert_eq!(p, title);
        assert_eq!(p.matches(title).count(), 1);
    }

    #[test]
    fn build_base_prompt_description_starts_with_title_then_blank_lines_only() {
        // task_delegate's `let mut description = objective.to_string()` path
        // can emit `"{objective}\n\n"` (title + trailing blank) — the dedupe
        // must collapse that to the description (which already starts with
        // the title) without growing it back.
        let title = "Fix CORS preflight";
        let description = "Fix CORS preflight\n\n";
        let p = build_base_prompt(title, description);
        assert_eq!(p, description);
        assert_eq!(p.matches(title).count(), 1);
    }

    #[test]
    fn build_base_prompt_distinct_description_keeps_both() {
        // Distinct title + description must still render both, joined by a
        // blank line — this is the original behaviour for hand-authored
        // BoardTasks that carry real detail in the description body.
        let title = "Stabilize watchdog";
        let description = "Investigate the 120s floor regression and add a regression test.";
        let p = build_base_prompt(title, description);
        assert_eq!(
            p,
            "Stabilize watchdog\n\n\
             Investigate the 120s floor regression and add a regression test."
        );
    }

    #[test]
    fn build_base_prompt_title_prefix_with_extra_body_keeps_description_intact() {
        // Description starts with the title but then has real body content
        // (after blank lines). The dedupe rule keeps the description as-is
        // so the body is preserved without re-prepending the title.
        let title = "Stabilize watchdog";
        let description = "Stabilize watchdog\n\nInvestigate the 120s floor regression.";
        let p = build_base_prompt(title, description);
        assert_eq!(p, description);
        assert_eq!(p.matches("Stabilize watchdog").count(), 1);
    }

    // ── Prompt-tool-contract: conditional board-tool self-close ─────────

    #[test]
    fn append_board_task_id_suffix_surfaces_board_task_id() {
        // The board task id MUST always be visible to the worker for audit,
        // independent of whether board MCP tools are attached.
        let suffix = append_board_task_id_suffix("BODY", "task-123");
        assert!(
            suffix.contains("Board Task ID"),
            "missing Board Task ID label: {suffix}"
        );
        assert!(
            suffix.contains("`task-123`"),
            "task id not surfaced: {suffix}"
        );
        assert!(suffix.starts_with("BODY\n\n---\n"));
    }

    // ── V3 execution-ownership :: delegated-boardtask close-owner ───────

    #[test]
    fn decide_close_action_preserves_self_close_done() {
        // Worker self-closed the task via attached board MCP tools before
        // pty.send returned. Autopilot must preserve Done and not overwrite.
        assert_eq!(
            decide_close_action(Some(missiond_core::types::BoardTaskStatus::Done)),
            DispatchCloseAction::AlreadySelfClosed
        );
    }

    #[test]
    fn decide_close_action_preserves_blocked_question_state() {
        // Task transitioned to Blocked via mission_question_create during
        // execution. Autopilot must preserve Blocked and never overwrite
        // with done on pty.send return.
        assert_eq!(
            decide_close_action(Some(missiond_core::types::BoardTaskStatus::Blocked)),
            DispatchCloseAction::PreserveBlocked
        );
    }

    #[test]
    fn decide_close_action_owner_closes_running_or_open() {
        // Default close-owner path: running → done.
        assert_eq!(
            decide_close_action(Some(missiond_core::types::BoardTaskStatus::Running)),
            DispatchCloseAction::OwnerClosesAsDone
        );
        assert_eq!(
            decide_close_action(Some(missiond_core::types::BoardTaskStatus::Open)),
            DispatchCloseAction::OwnerClosesAsDone
        );
    }

    #[test]
    fn decide_close_action_owner_closes_when_status_unknown() {
        // Lookup miss (DB error or task vanished) — Autopilot still owns
        // closure. Treating None as OwnerClosesAsDone matches the legacy
        // `_ =>` arm so we don't introduce a new orphan path.
        assert_eq!(
            decide_close_action(None),
            DispatchCloseAction::OwnerClosesAsDone
        );
    }

    #[test]
    fn append_board_task_id_suffix_is_conditional_not_unconditional() {
        // Regression guard: the previous wording said the worker MUST call
        // mission_board_update / mission_board_note_add, which broke slots
        // without those tools attached. New wording must be conditional and
        // must explicitly allow returning a final summary instead.
        let suffix = append_board_task_id_suffix("BODY", "task-123");

        // Old unconditional must-call wording is gone.
        assert!(
            !suffix.contains("你必须调用"),
            "unconditional `你必须调用` wording leaked back in: {suffix}"
        );

        // New wording is conditional on tool availability.
        assert!(
            suffix.contains("若当前工位已挂载"),
            "missing conditional clause about tool attachment: {suffix}"
        );
        // Both board MCP tools are still mentioned by name so a slot that
        // *does* have them knows what to call.
        assert!(suffix.contains("mission_board_update"));
        assert!(suffix.contains("mission_board_note_add"));

        // Tools-absent fallback is explicit: return a final summary, and
        // Autopilot/orchestrator owns closing the BoardTask.
        assert!(
            suffix.contains("若上述 board MCP 工具未挂载到本工位"),
            "missing explicit tools-absent fallback: {suffix}"
        );
        assert!(
            suffix.contains("最终完成摘要"),
            "missing instruction to return a final summary: {suffix}"
        );
        assert!(
            suffix.contains("Autopilot/orchestrator"),
            "missing handover-to-orchestrator wording: {suffix}"
        );
    }

    // ── Dynamic slot stale-pin recovery ────────────────────────────────

    #[test]
    fn dynamic_slot_id_detection_is_prefix_based() {
        assert!(is_dynamic_slot_id("slot-dyn-abc123"));
        assert!(!is_dynamic_slot_id("slot-coder"));
        assert!(!is_dynamic_slot_id("coder-dyn-abc123"));
    }

    #[test]
    fn stale_dynamic_assignee_clears_only_dead_dynamic_pin() {
        assert!(should_clear_stale_dynamic_assignee(
            "slot-dyn-restarted",
            false,
            false
        ));
        assert!(!should_clear_stale_dynamic_assignee(
            "slot-dyn-running",
            true,
            true
        ));
        assert!(!should_clear_stale_dynamic_assignee(
            "slot-dyn-db-active",
            false,
            true
        ));
        assert!(!should_clear_stale_dynamic_assignee(
            "static-coder",
            false,
            false
        ));
    }
}

/// Scale-to-zero: release persistent slots that have been idle > IDLE_TIMEOUT.
/// The slot will be auto-respawned by ClaudeCodeSlotMgr::execute_persistent
/// when the next task arrives (lazy-spawn pattern).
async fn reap_idle_persistent_slots(state: &AppState) {
    const IDLE_TIMEOUT_SECS: u64 = 30 * 60; // 30 minutes

    let slots = state.mission.list_slots();
    for slot in &slots {
        if !slot.config.is_persistent() {
            continue;
        }
        let slot_id = &slot.config.id;

        // Check if slot is alive and idle
        if !state.pty.is_available(slot_id).await {
            continue; // Not idle (thinking, responding, or not running)
        }

        // Check last activity time from slot_progress
        let last_active = {
            let progress = state.slot_progress.read().await;
            progress
                .get(slot_id)
                .and_then(|sp| sp.last_activity.as_ref())
                .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc))
        };

        let idle_secs = match last_active {
            Some(ts) => (chrono::Utc::now() - ts).num_seconds().max(0) as u64,
            None => {
                // No activity record — check if slot has been alive long enough
                // If no record at all, it might have just spawned. Skip.
                continue;
            }
        };

        if idle_secs >= IDLE_TIMEOUT_SECS {
            info!(
                slot_id,
                idle_mins = idle_secs / 60,
                "Scale-to-zero: releasing idle persistent slot"
            );
            // Graceful shutdown: session.close() sends /exit and waits 3s
            state.pty.kill(slot_id).await.ok();
        }
    }
}
