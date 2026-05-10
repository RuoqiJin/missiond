use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::claude_md_sync::sync_claude_md;
use crate::context::v3_blueprint_runtime::{
    AutopilotRuntimeConfig, RouterRuntimeConfig, WorkstationRuntimeConfig,
};
use crate::engine::learning_engine;
use crate::flow_engine::{ensure_autopilot_pty, execute_flow_task};
use crate::handlers::knowledge::agent_execution;
use crate::llm_gateway::determine_llm_env;
use crate::memory_scheduler::ensure_memory_slot_by_id;
use crate::memory_scheduler::{dispatch_queued_submit_tasks, reap_stale_submit_tasks};
use crate::slot_dispatch::SlotDispatchGuard;
use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::supervisor::schedule_supervisor_patrol;
use crate::supervisor::truncate_safe;
use crate::supervisor::{
    check_pending_compact_restarts, check_slot_context_levels, check_slot_stuck,
};
use crate::supervisor::{is_auth_error, is_quota_exhausted};
use missiond_core::event::events::{
    BoardEvent, IncidentEvent, SessionEvent, SlotEvent, SystemEvent,
};
use missiond_core::SessionState;
use missiond_mcp::tools::{ToolContent, ToolResult};

// @beacon: orchestration

/// Clamp `BoardTask.timeout_secs` to the autopilot wait budget.
///
/// The default/floor/ceiling come from V3 `workstation-config
/// timeout-policy boardtask-dispatch`, loaded through `AutopilotRuntimeConfig`,
/// so the pty.send budget always lines up with the timeout policy the
/// delegator already used to write `BoardTask.timeout_secs`.
///
/// Pure helper so unit tests can pin the policy without an `AppState`.
fn derive_pty_timeout_secs(config: &AutopilotRuntimeConfig, timeout_secs: Option<i64>) -> i64 {
    let raw = match timeout_secs {
        Some(v) if v > 0 => v,
        _ => config.boardtask_timeout_policy.default_secs,
    };
    raw.clamp(
        config.boardtask_timeout_policy.min_secs,
        config.boardtask_timeout_policy.max_secs,
    )
}

/// Convert the derived timeout into the `pty.send` millisecond budget.
fn derive_pty_timeout_ms(config: &AutopilotRuntimeConfig, timeout_secs: Option<i64>) -> u64 {
    (derive_pty_timeout_secs(config, timeout_secs) as u64).saturating_mul(1000)
}

/// Context prefetch is noisy while KB/history are still being normalized.
///
/// Default off keeps delegated worker prompts scoped to explicit BoardTask
/// contract fields (`read_scope`, `context_pack_path`, `acceptance`) instead of
/// hidden KB/Skill snippets. Operators can temporarily opt in for a dedicated
/// memory-audit workflow with `MISSIOND_AUTOPILOT_CONTEXT_PREFETCH=1`.
fn autopilot_context_prefetch_enabled() -> bool {
    autopilot_context_prefetch_enabled_from(
        std::env::var("MISSIOND_AUTOPILOT_CONTEXT_PREFETCH")
            .ok()
            .as_deref(),
    )
}

fn autopilot_context_prefetch_enabled_from(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Default settle window after `pty.send` reports completion before Autopilot
/// writes the durable BoardTask close state.
///
/// PTY return is only a high-confidence completion signal, not the durable
/// provider log itself. The short window lets Claude/Codex/Gemini JSONL/SSE
/// final messages and MissionD conversation ingestion land before the
/// orchestrator writes the BoardTask summary and `mission_execution` synthesis.
pub(crate) const AUTOPILOT_FINAL_SETTLE_WINDOW_MS_DEFAULT: u64 = 5000;

fn worker_final_settle_window_ms() -> u64 {
    std::env::var("MISSIOND_AUTOPILOT_FINAL_SETTLE_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .map(|ms| ms.min(30_000))
        .unwrap_or(AUTOPILOT_FINAL_SETTLE_WINDOW_MS_DEFAULT)
}

async fn wait_for_worker_final_settle_window() {
    let settle_ms = worker_final_settle_window_ms();
    if settle_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(settle_ms)).await;
    }
}

/// Smallest claimed-age (seconds) at which the watchdog may reclaim an idle
/// slot still bound to a running task. Equals the derived task timeout plus
/// V3 `:watchdog_grace_secs` so the slot always gets the full configured
/// window before being treated as orphaned.
fn idle_watchdog_threshold_secs(config: &AutopilotRuntimeConfig, timeout_secs: Option<i64>) -> i64 {
    derive_pty_timeout_secs(config, timeout_secs)
        .saturating_add(config.boardtask_timeout_policy.watchdog_grace_secs)
}

/// Lease horizon Autopilot writes onto a freshly-claimed BoardTask, in
/// seconds from now. Equals `idle_watchdog_threshold_secs(timeout_secs)` so
/// that the smart-watchdog reclaim threshold and the claim lease move
/// together when `BoardTask.timeout_secs` changes. The lease therefore covers
/// the full pty.send budget plus V3 watchdog grace, never the legacy
/// fixed 20-minute window. Pure helper so unit tests can pin the policy
/// without an `AppState`.
fn derive_board_task_lease_secs(config: &AutopilotRuntimeConfig, timeout_secs: Option<i64>) -> i64 {
    idle_watchdog_threshold_secs(config, timeout_secs)
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

/// V3 lisp-code-sync :: stale runtime-evidence dispatch revalidation.
///
/// `.missiond/v3/runtime/lisp-code-sync/**` reports are cold runtime evidence,
/// not active authoring source. A past bug created auto-executable BoardTasks
/// from those self-written report paths; even after the watcher/report GC fix,
/// old open tasks can remain in the Board and would otherwise keep consuming
/// worker slots. Revalidate this class at the Autopilot dispatch boundary and
/// close it as stale evidence before any slot selection/PTY send happens.
fn is_stale_lisp_code_sync_runtime_report_task(task: &missiond_core::types::BoardTask) -> bool {
    let text = format!(
        "{}\n{}\n{}",
        task.title,
        task.description,
        task.dedupe_key.as_deref().unwrap_or("")
    )
    .to_ascii_lowercase();
    let references_lisp_sync_runtime = text.contains("runtime/lisp-code-sync/")
        || text.contains(".missiond/v3/runtime/lisp-code-sync/")
        || text.contains(".missiond/v3/runtime/lisp-code-sync");
    if !references_lisp_sync_runtime {
        return false;
    }
    text.contains("sync code for lisp change")
        || text.contains("lisp-code-sync")
        || text.contains("lisp code sync")
}

async fn resolve_stale_lisp_code_sync_runtime_report_task(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
) {
    let note = "✅ resolved_by_runtime_fix / stale_evidence — this BoardTask points at a lisp-code-sync runtime report under `.missiond/v3/runtime/lisp-code-sync/**`. Runtime reports are cold evidence, not editable SSOT source; the watcher now ignores runtime output and report GC bounds the directory. Autopilot therefore closed this stale self-loop task before dispatch instead of spending another worker slot.";
    let _ = state
        .store
        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
            task_id: task.id.to_string(),
            content: note.to_string(),
            note_type: Some("summary".to_string()),
            author: Some("autopilot".to_string()),
        })
        .await;
    let _ = state
        .store
        .update_board_task(
            task.id.as_str(),
            &missiond_core::types::UpdateBoardTaskInput {
                status: Some("done".to_string()),
                auto_execute: Some(false),
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
    info!(
        task_id = %task.id,
        title = %task.title,
        "Autopilot: closed stale lisp-code-sync runtime report task before dispatch"
    );
}

/// V3 slot-attribution :: stale-running-claim detector.
///
/// Walk the running BoardTask list and collect every task other than
/// `incoming_task_id` whose claim_executor still points at `slot_id` via
/// pty_slot. The returned ids are unclaimed at the dispatch site so the slot
/// only ever has one running BoardTask attribution at a time.
///
/// Pure helper over `state.store` so callers can swap in a stub for tests; the
/// inline pure-data check lives in `is_stale_running_claim_for_slot` below.
async fn stale_running_claims_for_slot(
    state: &AppState,
    slot_id: &str,
    incoming_task_id: &str,
) -> Result<Vec<String>> {
    let running = state
        .store
        .list_board_tasks(Some("running"), true)
        .await
        .map_err(|e| anyhow!("list_board_tasks(running) failed: {}", e))?;
    Ok(running
        .into_iter()
        .filter(|task| is_stale_running_claim_for_slot(task, slot_id, incoming_task_id))
        .map(|task| task.id.to_string())
        .collect())
}

/// Pure predicate so the slot-attribution invariant can be unit-tested without
/// an `AppState`. Returns true when this BoardTask is a running row claimed by
/// `slot_id` (pty_slot) but is *not* the dispatch we're about to start.
fn is_stale_running_claim_for_slot(
    task: &missiond_core::types::BoardTask,
    slot_id: &str,
    incoming_task_id: &str,
) -> bool {
    if task.status != missiond_core::types::BoardTaskStatus::Running {
        return false;
    }
    if task.id.as_str() == incoming_task_id {
        return false;
    }
    let executor_type = task
        .claim_executor_type
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    let executor_id = task
        .claim_executor_id
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    executor_type == "pty_slot" && executor_id == slot_id
}

/// V3 slot-attribution :: dispatch-time conversation rebind.
///
/// Authoritatively rewrite the slot's active provider conversation row to
/// point at `task_id`, displacing any stale binding from a previous dispatch
/// on the same session. Best-effort: silent no-op when the slot session is
/// not yet registered (the post-completion backfill path will reconcile via
/// `conversation_task_binding_update_allowed`).
async fn rebind_slot_conversation_for_dispatch(state: &AppState, slot_id: &str, task_id: &str) {
    let session_uuid = match state.store.get_slot_session(slot_id).await {
        Ok(Some(uuid)) => uuid,
        _ => return,
    };
    if let Ok(Some(conv)) = state.store.get_conversation(&session_uuid).await {
        if let Some(existing) = conv
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != task_id)
        {
            warn!(
                slot_id,
                task_id,
                session_id = %session_uuid,
                displaced_task_id = %existing,
                "Autopilot: dispatch-time conversation rebind displaced previous task binding"
            );
        }
    }
    if let Err(err) = state
        .store
        .set_conversation_task_id(&session_uuid, task_id)
        .await
    {
        warn!(
            slot_id,
            task_id,
            session_id = %session_uuid,
            error = %err,
            "Autopilot: dispatch-time conversation rebind failed"
        );
    }
}

fn is_durable_completion_summary_note(note: &missiond_core::types::BoardTaskNote) -> bool {
    if note.note_type != missiond_core::types::BoardNoteType::Summary {
        return false;
    }
    let content = note.content.trim();
    !content.is_empty()
        && !content.contains("PTY 返回了仍在运行")
        && !content.contains("Autopilot 未关闭任务")
}

fn has_durable_completion_summary_after_claim(
    notes: &[missiond_core::types::BoardTaskNote],
    claimed_at: Option<&str>,
) -> bool {
    let claimed_at = claimed_at.and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok());
    notes.iter().any(|note| {
        if !is_durable_completion_summary_note(note) {
            return false;
        }
        let Some(claimed_at) = claimed_at else {
            return true;
        };
        chrono::DateTime::parse_from_rfc3339(note.created_at.as_str())
            .map(|note_at| note_at >= claimed_at)
            .unwrap_or(true)
    })
}

#[derive(Debug, Clone)]
struct DurableProviderCompletion {
    session_id: String,
    source: String,
    summary: String,
}

fn timestamp_is_after_or_unknown(timestamp: &str, threshold: Option<&str>) -> bool {
    let Some(threshold) = threshold else {
        return true;
    };
    let Ok(threshold) = chrono::DateTime::parse_from_rfc3339(threshold) else {
        return true;
    };
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|ts| ts >= threshold)
        .unwrap_or(true)
}

fn latest_assistant_after_task_prompt(
    messages: &[missiond_core::types::ConversationMessage],
    task_id: &str,
    claimed_at: Option<&str>,
) -> Option<String> {
    let mut seen_task_prompt = false;
    let mut latest: Option<String> = None;
    for msg in messages {
        if !timestamp_is_after_or_unknown(&msg.timestamp, claimed_at) {
            continue;
        }
        let content = msg.content.trim();
        if content.is_empty() {
            continue;
        }
        if msg.role != "assistant" && content.contains(task_id) {
            seen_task_prompt = true;
            latest = None;
            continue;
        }
        if seen_task_prompt
            && msg.role == "assistant"
            && !is_probably_active_tui_summary(content)
            && !is_probably_provider_tool_invocation_message(content)
        {
            latest = Some(content.to_string());
        }
    }
    latest
}

fn latest_assistant_after_claim(
    messages: &[missiond_core::types::ConversationMessage],
    claimed_at: Option<&str>,
) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|msg| {
            msg.role == "assistant"
                && timestamp_is_after_or_unknown(&msg.timestamp, claimed_at)
                && !msg.content.trim().is_empty()
                && !is_probably_active_tui_summary(msg.content.trim())
                && !is_probably_provider_tool_invocation_message(msg.content.trim())
        })
        .map(|msg| msg.content.trim().to_string())
}

fn provider_completion_summary_for_task(
    messages: &[missiond_core::types::ConversationMessage],
    task_id: &str,
    claimed_at: Option<&str>,
    conversation_task_id: Option<&str>,
) -> Option<String> {
    latest_assistant_after_task_prompt(messages, task_id, claimed_at).or_else(|| {
        if conversation_task_id == Some(task_id) {
            latest_assistant_after_claim(messages, claimed_at)
        } else {
            None
        }
    })
}

fn is_probably_provider_tool_invocation_message(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with("[Tool:")
        || trimmed.starts_with("[tool:")
        || (trimmed.starts_with("Tool:")
            && (trimmed.contains("command:") || trimmed.contains("description:")))
}

async fn durable_provider_completion_for_slot_task(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
    slot_id: &str,
) -> Result<Option<DurableProviderCompletion>> {
    let mut candidates = state
        .store
        .get_conversations_by_task_id(task.id.as_str())
        .await
        .unwrap_or_default();

    if let Ok(Some(session_id)) = state.store.get_slot_session(slot_id).await {
        if !candidates.iter().any(|conv| conv.id == session_id) {
            if let Some(conv) = state.store.get_conversation(&session_id).await? {
                candidates.push(conv);
            }
        }
    }

    candidates.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    for conv in candidates {
        let messages = state
            .store
            .get_conversation_messages(&conv.id, None, 80)
            .await
            .unwrap_or_default();
        let summary = provider_completion_summary_for_task(
            &messages,
            task.id.as_str(),
            task.claimed_at.as_deref(),
            conv.task_id.as_deref(),
        );
        if let Some(summary) = summary {
            if conv.task_id.as_deref() != Some(task.id.as_str())
                && crate::flow_engine::conversation_task_binding_update_allowed(
                    conv.task_id.as_deref(),
                    task.id.as_str(),
                )
            {
                let _ = state
                    .store
                    .set_conversation_task_id(&conv.id, task.id.as_str())
                    .await;
            }
            if conv.status == "active" {
                let _ = state.store.complete_conversation(&conv.id).await;
            }
            return Ok(Some(DurableProviderCompletion {
                session_id: conv.id,
                source: conv.source,
                summary,
            }));
        }
    }

    Ok(None)
}

async fn await_durable_provider_completion_for_slot_task(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
    slot_id: &str,
) -> Result<Option<DurableProviderCompletion>> {
    let poll_budget_ms = worker_final_settle_window_ms().clamp(1_000, 30_000);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(poll_budget_ms);
    let mut latest: Option<DurableProviderCompletion> = None;
    loop {
        reconcile_slot_provider_conversation(state, slot_id).await;
        if let Some(completion) =
            durable_provider_completion_for_slot_task(state, task, slot_id).await?
        {
            latest = Some(completion);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(latest);
        }
        tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    }
}

async fn reconcile_slot_provider_conversation(state: &AppState, slot_id: &str) {
    let Ok(Some(session_id)) = state.store.get_slot_session(slot_id).await else {
        return;
    };
    let Ok(Some(conv)) = state.store.get_conversation(&session_id).await else {
        return;
    };
    let Some(jsonl_path) = conv.jsonl_path.as_deref() else {
        return;
    };
    crate::events_sync::reconcile_conversation_messages(state, conv.id.as_str(), jsonl_path).await;
}

async fn close_idle_running_task_from_durable_summary(
    state: &AppState,
    task_id: &str,
    slot_id: &str,
) -> Result<bool> {
    let Some(task_with_notes) = state.store.get_board_task_with_notes(task_id).await? else {
        return Ok(false);
    };
    if task_with_notes.task.status != missiond_core::types::BoardTaskStatus::Running {
        return Ok(false);
    }
    let mut has_durable_summary = has_durable_completion_summary_after_claim(
        &task_with_notes.notes,
        task_with_notes.task.claimed_at.as_deref(),
    );
    if !has_durable_summary {
        if let Some(completion) =
            durable_provider_completion_for_slot_task(state, &task_with_notes.task, slot_id).await?
        {
            let summary_for_note =
                truncate_safe(&completion.summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES);
            state
                .store
                .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task_id.to_string(),
                    content: format!(
                        "**Provider durable final observed** ({} / {})\n\n{}",
                        completion.source, completion.session_id, summary_for_note
                    ),
                    note_type: Some("summary".to_string()),
                    author: Some("autopilot".to_string()),
                })
                .await?;
            has_durable_summary = true;
        }
    }
    if !has_durable_summary {
        return Ok(false);
    }

    state
        .store
        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
            task_id: task_id.to_string(),
            content: format!(
                "✅ **Durable completion observed** — 工位 {} 已 idle，且 BoardTask 已有 claim 之后写入的 summary note；Autopilot 使用 durable note + idle slot 闭合任务，未单独依赖 PTY running/idle 文本。",
                slot_id
            ),
            note_type: Some("note".to_string()),
            author: Some("autopilot".to_string()),
        })
        .await?;
    state
        .store
        .update_board_task(
            task_id,
            &missiond_core::types::UpdateBoardTaskInput {
                status: Some("done".to_string()),
                ..Default::default()
            },
        )
        .await?;
    let _ = state
        .bus
        .publish_board(BoardEvent::StatusChanged {
            task_id: task_id.to_string(),
            old_status: format!("{:?}", task_with_notes.task.status),
            new_status: "done".to_string(),
        })
        .await;
    info!(
        task_id,
        slot_id, "Autopilot: closed idle running task from durable completion summary note"
    );
    Ok(true)
}

fn extract_delegated_execution_id(prompt: &str) -> Option<String> {
    extract_between(prompt, "Execution log: `", "`")
        .or_else(|| extract_between(prompt, "execution_id=\"", "\""))
        .filter(|id| id.starts_with("plan-") && !id.contains(|c: char| c.is_whitespace()))
}

fn extract_between(source: &str, prefix: &str, suffix: &str) -> Option<String> {
    let start = source.find(prefix)? + prefix.len();
    let rest = &source[start..];
    let end = rest.find(suffix)?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn tool_result_json_value(result: &ToolResult) -> Option<serde_json::Value> {
    result.content.iter().find_map(|content| match content {
        ToolContent::Text { text } => serde_json::from_str(text).ok(),
    })
}

fn execution_status_has_completion(status: &serde_json::Value) -> bool {
    status
        .get("completed_phases")
        .and_then(|v| v.as_array())
        .map(|items| !items.is_empty())
        .unwrap_or(false)
}

async fn project_id_for_execution_log(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
    execution_id: &str,
) -> Option<String> {
    if let Some(project) = task.project.as_ref().filter(|s| !s.trim().is_empty()) {
        return Some(project.clone());
    }

    let file_name = format!("{}.lisp", execution_id);
    let registry = state.project_registry.read().await;
    registry.all_projects().iter().find_map(|project| {
        let root = Path::new(&project.path);
        let canonical = root
            .join(".missiond/v3/runtime/executions")
            .join(&file_name);
        let legacy = root.join(".missiond/v2").join(&file_name);
        if canonical.exists() || legacy.exists() {
            Some(project.id.clone())
        } else {
            None
        }
    })
}

async fn maybe_complete_delegated_execution_log(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
    full_prompt: &str,
    worker_response: &str,
    duration_ms: u64,
) -> Result<bool> {
    let Some(execution_id) = extract_delegated_execution_id(full_prompt) else {
        return Ok(false);
    };
    let project_id = project_id_for_execution_log(state, task, &execution_id).await;

    let mut status_args = serde_json::json!({
        "action": "status",
        "execution_id": execution_id,
    });
    if let Some(project) = &project_id {
        status_args["project"] = serde_json::json!(project);
    }

    let status_result = agent_execution::handle(state, "mission_execution", status_args).await?;
    if status_result.is_error.unwrap_or(false) {
        return Ok(false);
    }
    if tool_result_json_value(&status_result)
        .map(|v| execution_status_has_completion(&v))
        .unwrap_or(false)
    {
        return Ok(false);
    }

    let mut complete_args = serde_json::json!({
        "action": "complete",
        "execution_id": execution_id,
        "phase": "delegated-boardtask",
        "agent_name": "autopilot-orchestrator",
        "summary": truncate_safe(worker_response, 500),
        "deliverables": format!(
            "BoardTask {} completed through Autopilot; orchestrator synthesized the mission_execution completion because the worker returned a final summary instead of calling the MCP tool.",
            task.id
        ),
        "verification": format!(
            "Autopilot observed pty.send completion after {}ms and stored the worker final summary as the BoardTask completion note.",
            duration_ms
        ),
        "commit_status": "not-required",
        "enforce_scoped_commit": true,
    });
    if let Some(project) = &project_id {
        complete_args["project"] = serde_json::json!(project);
    }

    let complete_result =
        agent_execution::handle(state, "mission_execution", complete_args).await?;
    Ok(!complete_result.is_error.unwrap_or(false))
}

/// Maximum byte length of the worker final summary written into the
/// `**Autopilot 执行完成**` BoardTask note. The legacy site embedded the
/// entire `res.response` screen capture (echoed prompt, task contract, tool
/// logs, paste-collapse markers) which polluted every completed BoardTask
/// record. The cap pairs with `truncate_safe` (UTF-safe, char-boundary)
/// so a long worker tail still produces a readable note.
pub(crate) const AUTOPILOT_SUMMARY_NOTE_MAX_BYTES: usize = 4000;

/// V3 execution-ownership :: delegated-boardtask :: summary-note source.
///
/// Extract the worker's concise final summary from the raw PTY screen
/// capture (`res.response`) so the `**Autopilot 执行完成**` note and the
/// delegated `mission_execution(action=complete)` summary record only the
/// worker's actual final answer — never the echoed prompt, task contract,
/// tool log lines, or `[Pasted text +N lines, paste again to expand]`
/// collapse markers that the Claude Code TUI surfaces in the screen blob.
///
/// The extractor is a deterministic, allocation-cheap pure helper so the
/// note path and the `mission_execution` completion path always project
/// the same sanitized summary, and so the rule can be unit-tested without
/// constructing an `AppState`. Auth-error and quota-exhausted diagnostic
/// notes intentionally use the raw response and bypass this extractor.
pub(crate) fn extract_worker_final_summary(response: &str, dispatched_prompt: &str) -> String {
    let after_echo = strip_prompt_echo(response, dispatched_prompt);
    let final_region = focus_final_summary_region(after_echo);
    let cleaned = strip_tui_artifacts(final_region);
    trim_board_summary_tail(&cleaned).trim().to_string()
}

/// Strip the echoed dispatched prompt from the screen capture.
///
/// The dispatched prompt always ends with the `append_board_task_id_suffix`
/// tail block; its terminal phrase is unique enough that the LAST occurrence
/// in the screen capture marks the boundary between the echoed task contract
/// and the worker's actual output. When the TUI collapsed the paste so the
/// terminal phrase never reached the screen, fall back to the BoardTask
/// label anchor and skip its line. When neither anchor exists, return the
/// input unchanged so artifact stripping can still run.
fn strip_prompt_echo<'a>(response: &'a str, _dispatched_prompt: &str) -> &'a str {
    const TAIL_ANCHOR: &str = "负责关闭此 BoardTask。";
    if let Some(idx) = response.rfind(TAIL_ANCHOR) {
        let cut = idx + TAIL_ANCHOR.len();
        return &response[cut..];
    }
    const LABEL_ANCHOR: &str = "📋 **Board Task ID**:";
    if let Some(idx) = response.rfind(LABEL_ANCHOR) {
        let after = &response[idx..];
        if let Some(nl) = after.find('\n') {
            return &response[idx + nl + 1..];
        }
    }
    response
}

/// Prefer the final assistant summary region when the TUI screen contains the
/// whole investigation transcript. Claude Code often leaves "Now let me ..."
/// narration, diffs, and tool cards before the final `Summary` heading; keeping
/// the last summary block makes the Board note useful instead of merely less
/// polluted.
///
/// Three anchor classes cooperate:
///
/// 1. Heading anchors (`\nSummary`, `\n⏺ Summary`, `\n## Summary`, ...): the
///    leading `\n` is consumed so the heading line is kept, and multi-section
///    finals like `⏺ Summary` / `⏺ Diagnosis` / `⏺ Validation` carry their
///    section labels into the BoardTask note.
/// 2. Closeout-phrase anchors (`diagnostic summary for the BoardTask:` /
///    `All acceptance gates pass`): the worker writes a multi-paragraph
///    diagnostic block (`Fix:` / `Root cause:` / `Changes` / `Verification`)
///    without using a `Summary` heading. We back up to the start of the
///    containing line so the closeout sentence is preserved as a lead-in;
///    everything before it — `⏺ Now I'll edit ...` narration, `+`/`-` diff
///    hunk lines that survive `strip_tui_artifacts`, and other transcript
///    cruft — is dropped.
/// 3. Fix:/Verification: closeout-pair fallback (Gemini-style): the worker
///    omits both a `Summary` heading and the standard closeout lead-in,
///    ending instead with a `Fix: …` line followed somewhere later by a
///    `Verification: …` line beneath a `诊断报告` / English diagnosis
///    bullets block. Used only when no heading or phrase anchor matched, so
///    the existing closeout-phrase tests (which prefer the lead-in line) are
///    not regressed.
///
/// Across heading and phrase classes we take the LAST anchor occurrence
/// (whichever ends up later in the response wins) so a final closeout phrase
/// that comes after an earlier mid-investigation `Summary` mention still wins.
fn focus_final_summary_region(input: &str) -> &str {
    const HEADING_ANCHORS: [&str; 12] = [
        "\n⏺ Smoke Summary",
        "\n⏺ Final Summary",
        "\n⏺ Summary",
        "\nSmoke Summary",
        "\nFinal Summary",
        "\nSummary",
        "\n  Summary",
        "\n## Summary",
        // Long evaluation tasks (review-class) write a top-level markdown
        // H1 like `# Auth KB Cleanup — READ-ONLY Evaluation Report` instead
        // of a `Summary`-style heading. Anchor on the explicit H1 forms so
        // the final report region wins over earlier progress narration.
        "\n# Final",
        "\n# Summary",
        "\n## Final",
        "\n# Smoke Summary",
    ];
    const PHRASE_ANCHORS: [&str; 4] = [
        "diagnostic summary for the BoardTask:",
        "All acceptance gates pass",
        // Common long-form report closeouts (review tasks). `rfind` keeps
        // them safe when the worker also mentions "Evaluation Report" as a
        // mid-narration aside — the LAST occurrence wins.
        "Evaluation Report",
        "Final Report",
    ];

    let mut best: Option<usize> = None;
    for anchor in HEADING_ANCHORS.iter() {
        if let Some(idx) = input.rfind(anchor) {
            // Skip only the leading `\n` so the heading line is kept.
            let line_start = idx + 1;
            best = Some(best.map_or(line_start, |cur| cur.max(line_start)));
        }
    }
    for phrase in PHRASE_ANCHORS.iter() {
        if let Some(idx) = input.rfind(phrase) {
            // Back up to the start of the containing line so the closeout
            // sentence (with whatever lead-in punctuation) is preserved.
            let line_start = input[..idx].rfind('\n').map(|nl| nl + 1).unwrap_or(0);
            best = Some(best.map_or(line_start, |cur| cur.max(line_start)));
        }
    }

    if best.is_none() {
        if let Some(idx) = find_fix_verification_anchor(input) {
            best = Some(idx);
        }
    }

    match best {
        Some(idx) => &input[idx..],
        None => input,
    }
}

/// Locate the start of the LAST `Fix:` line whose companion `Verification:`
/// follows it. Used as a fallback closeout anchor for Gemini-style outputs
/// that omit a `Summary` heading and the `diagnostic summary for the
/// BoardTask:` / `All acceptance gates pass` lead-ins.
///
/// A `Fix:` candidate qualifies when it appears at the start of a line
/// (preceded only by optional `**` markdown emphasis, Gemini's `✦` assistant
/// bullet, and whitespace) and the
/// remaining input contains the literal `Verification:`. The literal-colon
/// match deliberately excludes `**Verification**:` (where `:` sits outside
/// the bold), so blocks already covered by the closeout-phrase anchors keep
/// their existing lead-in line and this fallback does not steal them.
fn find_fix_verification_anchor(input: &str) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut search_start = 0;
    while let Some(rel) = input[search_start..].find("Fix:") {
        let abs = search_start + rel;
        let line_start = input[..abs].rfind('\n').map(|nl| nl + 1).unwrap_or(0);
        let leading = &input[line_start..abs];
        let leading_trimmed = leading.trim();
        let line_start_ok =
            leading_trimmed.is_empty() || leading_trimmed == "**" || leading_trimmed == "✦";
        if line_start_ok && input[abs + "Fix:".len()..].contains("Verification:") {
            best = Some(line_start);
        }
        search_start = abs + "Fix:".len();
    }
    best
}

fn trim_board_summary_tail(input: &str) -> &str {
    let mut seen_verification = false;
    let mut separator_start: Option<usize> = None;
    let mut offset = 0;

    for line in input.split_inclusive('\n') {
        let line_start = offset;
        let trimmed = line.trim();

        if !seen_verification {
            if trimmed.contains("Verification:") {
                seen_verification = true;
            }
            offset += line.len();
            continue;
        }

        if trimmed.is_empty() {
            offset += line.len();
            continue;
        }

        if trimmed == "---" {
            separator_start.get_or_insert(line_start);
            offset += line.len();
            continue;
        }

        if is_board_summary_heading(trimmed) {
            let cut = separator_start.unwrap_or(line_start);
            return input[..cut].trim_end();
        }

        separator_start = None;
        offset += line.len();
    }

    input.trim_end()
}

fn is_board_summary_heading(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("任务诊断摘要") || compact.to_ascii_lowercase().contains("boardtasksummary")
}

/// Drop Claude Code TUI artifact lines (paste collapse markers, tool-call
/// log markers, status / hint bars, user-input echoes) and collapse runs of
/// blank lines.
fn strip_tui_artifacts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_blank = false;
    let mut skip_tool_continuation = false;
    for raw in input.lines() {
        let line = raw.trim_end();
        let indented_continuation = raw.starts_with(' ') || raw.starts_with('\t');
        if is_tui_artifact_line(line) {
            skip_tool_continuation = looks_like_bare_tool_call_marker(line.trim_start());
            continue;
        }
        if skip_tool_continuation && indented_continuation {
            continue;
        }
        skip_tool_continuation = false;
        if line.trim().is_empty() {
            if prev_blank {
                continue;
            }
            prev_blank = true;
        } else {
            prev_blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Predicate: is this line a Claude Code TUI screen artifact rather than
/// worker-authored content? The matchers are intentionally narrow — they
/// only recognize markers the TUI emits — so worker prose containing
/// e.g. a `>` quote at the start of a line is not stripped.
fn is_tui_artifact_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.is_empty() {
        return false;
    }
    if t.contains("paste again to expand") || t.starts_with("[Pasted text") {
        return true;
    }
    // Hint / status bar rendered at the bottom of the TUI.
    if t.starts_with("⏵⏵") || t.starts_with("? for shortcuts") || is_tui_progress_line(t) {
        return true;
    }
    // Tool-result marker — Claude Code TUI emits `⎿ …` for every tool result
    // line. Tool results are never assistant prose and never section labels,
    // so always strip them.
    if t.starts_with('⎿') {
        return true;
    }
    // Tool-call invocation marker. Claude Code uses `⏺` / `●` as the bullet
    // for ALL assistant content blocks (including `⏺ Summary`, `⏺ Diagnosis`,
    // `⏺ Validation` section headings and brief one-line answers), so we MUST
    // NOT strip every `⏺`/`●` line — that would truncate a multi-section
    // final summary down to a single body block. Only strip lines that look
    // like a function-call signature: `⏺ Ident(...)` / `● Ident(...)`. The
    // worker's prose and section labels never match the `Ident(...)` shape,
    // so they survive. Auth-error / quota notes bypass this extractor.
    if (t.starts_with('⏺') || t.starts_with('●')) && looks_like_tool_call_marker(t) {
        return true;
    }
    if looks_like_bare_tool_call_marker(t) {
        return true;
    }
    if matches!(
        t.chars().next(),
        Some('✻' | '✽' | '✳' | '✶' | '✢' | '·' | '❯')
    ) {
        return true;
    }
    if t.contains("ctrl+o to expand") || t.contains("ctrl+b to run in background") {
        return true;
    }
    if t.starts_with("YOLO Ctrl+Y") || (t.contains("GEMINI.md file") && t.contains("skills")) {
        return true;
    }
    if t.starts_with("*   Type your message") || t.starts_with("* Type your message") {
        return true;
    }
    if t.starts_with("workspace (/directory)") {
        return true;
    }
    if t.starts_with("~/")
        && t.contains("no sandbox")
        && (t.contains("Gemini") || t.contains("gemini-") || t.contains("Auto ("))
    {
        return true;
    }
    // User-input echo prefix used by the TUI when re-rendering the user's
    // last paste / typed line. A bare leading `>` followed by space matches
    // the echo; worker markdown blockquotes survive because Markdown
    // blockquotes typically appear inside a paragraph, while the echo line
    // is the entire visual line.
    if t == ">" || t.starts_with("> ") {
        return true;
    }
    false
}

fn looks_like_bare_tool_call_marker(trimmed: &str) -> bool {
    const TOOL_PREFIXES: [&str; 16] = [
        "Bash(",
        "Read(",
        "Edit(",
        "Write(",
        "MultiEdit(",
        "Grep(",
        "Glob(",
        "LS(",
        "TodoWrite(",
        "WebFetch(",
        "WebSearch(",
        "NotebookRead(",
        "NotebookEdit(",
        "mcp__",
        "mission_",
        "Task(",
    ];
    TOOL_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

/// Heuristic: does this `⏺ …` / `● …` line look like a tool-call signature
/// (`⏺ Read(path)`, `⏺ Bash(cmd)`, `⏺ Update(file.rs)`) rather than worker
/// prose or a section heading? The first non-bullet whitespace-delimited
/// token must contain `(` so that `⏺ Summary`, `⏺ Diagnosis`,
/// `⏺ Validation`, `⏺ Done. Fix verified.`, `⏺ The fix is in autopilot.rs
/// (line 234).` all survive — only `⏺ Tool(args)`-shaped lines are stripped.
fn looks_like_tool_call_marker(trimmed: &str) -> bool {
    let after_marker = trimmed
        .trim_start_matches(|c| c == '⏺' || c == '●')
        .trim_start();
    let Some(first_token) = after_marker.split_whitespace().next() else {
        return false;
    };
    let Some(paren_at) = first_token.find('(') else {
        return false;
    };
    // Require the prefix before `(` to look like an identifier — letters,
    // digits, `_`, `-`, `.`, `:` — so `⏺ The fix is (X).` is NOT treated as
    // a tool call (its first token is `The`, no `(`), but a real tool call
    // like `⏺ missiond_kb-search(query="…")` is. Empty prefix is rejected.
    let prefix = &first_token[..paren_at];
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/'))
}

fn is_probably_active_tui_summary(summary: &str) -> bool {
    let trimmed = summary.trim();
    trimmed.is_empty()
        || looks_like_active_tui_progress(trimmed)
        || looks_like_insight_only_progress(trimmed)
        || looks_like_retry_or_wakeup_progress(trimmed)
        || looks_like_intermediate_assistant_narration(trimmed)
}

fn worker_final_close_blocker(summary: &str) -> Option<&'static str> {
    let lower = summary.to_ascii_lowercase();
    const BLOCKING_MARKERS: [(&str, &str); 13] = [
        ("gpg pinentry", "gpg-pinentry"),
        ("pinentry was cancelled", "gpg-pinentry"),
        ("pinentry was canceled", "gpg-pinentry"),
        ("pinentry canceled", "gpg-pinentry"),
        ("pinentry cancelled", "gpg-pinentry"),
        ("commit failed", "commit-failed"),
        ("failed to commit", "commit-failed"),
        ("could not commit", "commit-failed"),
        ("commit did not succeed", "commit-failed"),
        ("plan mode and cannot directly modify", "plan-mode-no-write"),
        ("plan mode and can't directly modify", "plan-mode-no-write"),
        ("cannot directly modify", "plan-mode-no-write"),
        ("cannot write the file", "plan-mode-no-write"),
    ];
    BLOCKING_MARKERS
        .iter()
        .find_map(|(needle, reason)| lower.contains(needle).then_some(*reason))
}

fn delegated_write_close_evidence_blocker(
    task_description: &str,
    has_durable_provider_final: bool,
    summary: &str,
) -> Option<&'static str> {
    if delegated_task_is_read_only(task_description) {
        return None;
    }
    if delegated_write_scope(task_description).is_empty() {
        return None;
    }
    if !has_durable_provider_final {
        return Some("missing-durable-provider-final");
    }
    if !worker_final_has_acceptance_evidence(summary) {
        return Some("missing-acceptance-evidence");
    }
    None
}

/// V3 evidence-authority :: PTY-only close gate.
///
/// When the durable provider final is unavailable, the PTY screen is the only
/// source for the BoardTask close summary. PTY captures can include
/// intermediate assistant sentences (research narration, "now let me ...",
/// share-insights progress) that look complete enough to skip the existing
/// `is_probably_active_tui_summary` check but are not the final artifact.
///
/// For delegated worker BoardTasks (`## Swarm metadata` or
/// `## Dispatch metadata` block) the artifact is the structured report the
/// worker emits — `Findings / Evidence / Recommendations / Verification`,
/// `Summary`, `acceptance`, etc. Require at least one of those structural
/// markers when there is no durable provider final; otherwise preserve
/// running so the watchdog/next tick can re-extract once the provider log
/// settles.
///
/// Repro task ids: a5ebf6c4..., 5599b07a..., b5be6eed....
///
/// Pure helper so the gate can be unit-tested without an `AppState`.
fn pty_only_close_blocker(
    task_description: &str,
    has_durable_provider_final: bool,
    summary: &str,
) -> Option<&'static str> {
    if has_durable_provider_final {
        return None;
    }
    if !is_delegated_worker_description(task_description) {
        return None;
    }
    if pty_summary_has_structured_artifact(summary) {
        return None;
    }
    Some("missing-pty-final-artifact")
}

/// Some worker prompts declare an explicit structured artifact contract, e.g.
/// `Findings / Evidence / Recommendations / Verification`. A long-lived
/// provider session can briefly expose an older durable assistant summary after
/// dispatch-time rebind but before the current task's final lands. Generic
/// acceptance words such as "changed files" are not enough for those tasks; the
/// close summary must satisfy the declared sections.
fn output_contract_close_blocker(task_description: &str, summary: &str) -> Option<&'static str> {
    if !is_delegated_worker_description(task_description) {
        return None;
    }
    if !task_description
        .to_ascii_lowercase()
        .contains("findings / evidence / recommendations / verification")
    {
        return None;
    }
    const REQUIRED: [&str; 4] = ["findings", "evidence", "recommendations", "verification"];
    if REQUIRED
        .iter()
        .all(|heading| summary_has_report_heading(summary, heading))
    {
        return None;
    }
    if memory_review_summary_satisfies_output_contract(summary) {
        return None;
    }
    Some("missing-output-contract-sections")
}

fn memory_review_summary_satisfies_output_contract(summary: &str) -> bool {
    if !summary_has_report_heading(summary, "findings")
        || !summary_has_report_heading(summary, "verification")
    {
        return false;
    }
    let has_candidate_block = summary_has_report_heading(summary, "active memory candidates")
        || summary_has_report_heading(summary, "ssot-workflow backfill candidates")
        || summary_has_report_heading(summary, "needs human");
    let has_rationale = summary_has_report_heading(summary, "discard rationale")
        || summary_has_report_heading(summary, "recommendations");
    has_candidate_block && has_rationale
}

fn summary_has_report_heading(summary: &str, expected: &str) -> bool {
    summary.lines().any(|line| {
        let normalized = line
            .trim()
            .trim_start_matches('#')
            .trim_start_matches('*')
            .trim()
            .trim_end_matches(':')
            .trim()
            .to_ascii_lowercase();
        normalized == expected || normalized.starts_with(&format!("{expected} "))
    })
}

/// Detect a delegated worker BoardTask by its description envelope.
/// `mission_task_delegate` injects `## Dispatch metadata`; `mission_swarm_run`
/// injects `## Swarm metadata`. Either marker is the V3 envelope guarantee
/// that this task is a worker dispatch with a write_policy / read_scope /
/// acceptance contract — and therefore should produce a structured artifact
/// rather than a single-sentence chat answer.
fn is_delegated_worker_description(task_description: &str) -> bool {
    task_description.contains("## Swarm metadata")
        || task_description.contains("## Dispatch metadata")
}

/// Pure check: does `summary` look like a structured worker artifact?
/// Matches the section headings the V3 worker prompt contracts ask for plus
/// the existing acceptance-evidence markers. Match is case-insensitive on the
/// heading body so `# Findings`, `## Findings`, `Findings:` all qualify.
fn pty_summary_has_structured_artifact(summary: &str) -> bool {
    if worker_final_has_acceptance_evidence(summary) {
        return true;
    }
    let lower = summary.to_ascii_lowercase();
    const STRUCTURAL_MARKERS: &[&str] = &[
        "findings",
        "recommendations",
        "verification",
        "evidence",
        "## summary",
        "# summary",
        "final summary",
        "smoke summary",
        "diagnostic summary",
        "evaluation report",
        "final report",
        "next shards",
    ];
    STRUCTURAL_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

fn delegated_task_is_read_only(task_description: &str) -> bool {
    metadata_line_value(task_description, "write_policy")
        .map(|value| value.eq_ignore_ascii_case("read-only"))
        .unwrap_or(false)
}

fn delegated_write_scope(task_description: &str) -> Vec<String> {
    metadata_line_value(task_description, "write_scope")
        .map(split_metadata_list)
        .unwrap_or_default()
}

fn metadata_line_value<'a>(task_description: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("- {key}:");
    task_description.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "[]")
    })
}

fn split_metadata_list(value: &str) -> Vec<String> {
    value
        .split(|ch| ch == ',' || ch == '|')
        .map(str::trim)
        .filter(|item| !item.is_empty() && *item != "[]")
        .map(ToString::to_string)
        .collect()
}

fn worker_final_has_acceptance_evidence(summary: &str) -> bool {
    let lower = summary.to_ascii_lowercase();
    const EVIDENCE_PHRASES: &[&str] = &[
        "all gates green",
        "both gates green",
        "both gates pass",
        "gates green",
        "gates pass",
        "gate confirmation",
        "evidence-only gate confirmation",
        "final m10 evidence-only gate",
        "checks pass",
        "checks passed",
        "checker passes",
        "checker passed",
        "check.sh passes",
        "check.sh passed",
        "acceptance commands pass",
        "acceptance commands passed",
        "acceptance output",
        "final gate passes",
        "final gate passed",
        "m10 evidence-only passes",
        "m10 evidence-only passed",
    ];
    if EVIDENCE_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return true;
    }
    const EVIDENCE_MARKERS: &[&str] = &[
        "acceptance",
        "changed file",
        "changed files",
        "created",
        "files changed",
        "git diff --check",
        "passed",
        "test result",
        "tests pass",
        "verification",
        "verified",
        "worktree",
    ];
    EVIDENCE_MARKERS.iter().any(|marker| lower.contains(marker))
}

fn looks_like_active_tui_progress(text: &str) -> bool {
    text.lines()
        .any(|line| is_tui_progress_line(line.trim_start()))
}

fn looks_like_insight_only_progress(text: &str) -> bool {
    let normalized = text
        .trim_start_matches(|c: char| {
            c == '`' || c == '"' || c == '⏺' || c == '●' || c.is_whitespace()
        })
        .trim_start();
    let lower = normalized.to_ascii_lowercase();
    if !(lower.starts_with("★ insight")
        || lower.starts_with("insight ")
        || lower.contains("\n★ insight"))
    {
        return false;
    }
    const COMPLETION_EVIDENCE_MARKERS: [&str; 17] = [
        "acceptance",
        "changed file",
        "changed files",
        "commit hash",
        "commit ",
        "commit:",
        "commit_status",
        "completed",
        "done",
        "git diff --check",
        "passed",
        "scope",
        "test result",
        "tests pass",
        "verification",
        "verified",
        "worktree",
    ];
    !COMPLETION_EVIDENCE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

fn looks_like_retry_or_wakeup_progress(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const RETRY_PROGRESS_MARKERS: [&str; 10] = [
        "wakeup will fire",
        "wakeup is scheduled",
        "scheduled to retry",
        "wait for that retry",
        "retry rather than poll",
        "retry later",
        "will retry",
        "enospc",
        "no space left on device",
        "disk to clear",
    ];
    RETRY_PROGRESS_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

fn looks_like_intermediate_assistant_narration(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const INVESTIGATION_VERBS: &[&str] = &[
        "let me start",
        "let me re-verify",
        "let me inspect",
        "let me peek",
        "let me check",
        "let me read",
        "let me look",
        "let me run",
        "let me verify",
        "let me validate",
        "let me confirm",
        "let me corroborate",
        "let me compare",
        "let me write",
        "let me create",
        "let me generate",
        "let me produce",
        "let me gather",
        "let me add",
        "let me append",
        "let me update",
        "let me modify",
        "let me capture",
        "let me lay out",
        "let me explain the situation",
        "let me explain the architectural",
        "i need to inspect",
        "i need to check",
        "i need to read",
        "i need to verify",
        "i need to confirm",
        "i'll execute",
        "i’ll execute",
        "i will execute",
        "i'll produce",
        "i’ll produce",
        "i will produce",
        "i will redo",
        "i'll begin",
        "i’ll begin",
        "i will begin",
        "i'll gather",
        "i’ll gather",
        "i will gather",
        "i'll start",
        "i’ll start",
        "i will start",
        "i'll treat",
        "i’ll treat",
        "i'm going to",
        "i am going to",
        "acknowledged:",
        "acknowledged;",
        "i have enough context",
        "now i have all the context",
        "now i have a complete picture",
        "now i have the full picture",
        "now i have full clarity",
        "now i'll ",
        "now i’ll ",
        "now i will ",
    ];
    const MUTATION_PROGRESS_MARKERS: &[&str] = &[
        "now committing",
        "committing only",
        "committing the single",
        "now staging",
        "staging and committing",
        "staging the",
        "now running",
        "now verifying",
        "now checking",
        "now writing",
        "let me make the planned edits",
        "now let me append",
        "now let me update",
        "let me share insights",
        "then update ssot",
        "declare the blocker via ssot",
        "retrying once",
        "file is untracked",
        "now committing only",
        "i'll commit",
        "i will commit",
        "i'll stage",
        "i will stage",
        "i'll run",
        "i will run",
        "i'll verify",
        "i will verify",
        "i'll write",
        "i will write",
        "writing the",
        "writing .",
        "i'll insert",
        "i will insert",
        "i'll update",
        "i will update",
        "i'll append",
        "i will append",
    ];
    const SURVEY_PROGRESS_PREFIXES: [&str; 8] = [
        "checking ",
        "surveying ",
        "reading ",
        "inspecting ",
        "reviewing ",
        "looking at ",
        "looking through ",
        "gathering ",
    ];
    if INVESTIGATION_VERBS
        .iter()
        .chain(MUTATION_PROGRESS_MARKERS.iter())
        .any(|marker| lower.contains(marker))
    {
        return true;
    }
    let trimmed = lower.trim_start();
    if SURVEY_PROGRESS_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return true;
    }
    if trimmed.starts_with("let me ") {
        return true;
    }
    (trimmed.starts_with("good —") || trimmed.starts_with("good -"))
        && (trimmed.contains("let me ") || trimmed.contains("i need to "))
}

fn is_tui_progress_line(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if trimmed.contains("esc to cancel")
        && (matches!(first, '✦' | '✧' | '*' | '⏺' | '●')
            || trimmed.contains("Thinking")
            || trimmed.contains("Catapulting")
            || trimmed.contains("Combobulating"))
    {
        return true;
    }
    matches!(
        first,
        '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'
    ) && trimmed.contains("esc to cancel")
}

/// V3 execution-ownership :: delegated-boardtask :: dispatch-guard.
///
/// Owned RAII handle for the per-slot dispatch lock. Mirrors
/// `SlotDispatchGuard::try_acquire_guard` but holds an `Arc<SlotDispatchGuard>`
/// internally so the guard can be moved into a spawned dispatch task. The
/// borrowed `SlotAcquireGuard` shape ties the lock lifetime to the
/// `&AppState` reference, which prevents `dispatch_board_tasks` from starting
/// `state.pty.send` for one slot concurrently with another slot's send under
/// `tokio::task::JoinSet`. The owned variant releases on Drop, so same-slot
/// exclusion is preserved across the entire send + post-send tail.
pub(crate) struct OwnedSlotDispatchGuard {
    dispatch: Arc<SlotDispatchGuard>,
    slot_id: String,
}

impl OwnedSlotDispatchGuard {
    /// Try to acquire the per-slot dispatch lock with an owned guard. Returns
    /// `None` when the slot is already locked. Projects the same semantics as
    /// `state.slot_dispatch.try_acquire_guard(&slot_id)` but without borrowing
    /// `state.slot_dispatch` so the guard may travel through a `'static`
    /// task boundary while still holding the lock across the send.
    pub(crate) fn try_acquire(dispatch: &Arc<SlotDispatchGuard>, slot_id: &str) -> Option<Self> {
        if dispatch.try_acquire(slot_id) {
            Some(Self {
                dispatch: Arc::clone(dispatch),
                slot_id: slot_id.to_string(),
            })
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub(crate) fn slot_id(&self) -> &str {
        &self.slot_id
    }
}

impl Drop for OwnedSlotDispatchGuard {
    fn drop(&mut self) {
        self.dispatch.release(&self.slot_id);
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
    // The advisory block sits BEFORE the Board Task ID close-task block so
    // the suffix still ends with the TAIL_ANCHOR phrase
    // `"负责关闭此 BoardTask。"`. `strip_prompt_echo` rfind()'s that anchor to
    // separate the echoed prompt from the worker's output — moving the
    // anchor later in the suffix would leak the advisory into final
    // summaries.
    format!(
        "{}\n\n---\n📐 **多仓库 git status 输出规范**：\
        若一次回答中需要在多个 git 仓库之间切换，请在每段输出**之前**用一行 `===<repo-name>===` 标记仓库，再执行 `git status --short`；\
        勿把仓库名放在输出之后（标签会与下一段合并）。`cd` 会重置 shell cwd，请改用 `git -C <path> status --short` 或并行 Bash 调用。\
        \n\n---\n📋 **Board Task ID**: `{}`\n\
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
    let runtime_config = AutopilotRuntimeConfig::load_for_current_dir()?;

    // Check PTY slots for low context — mark for graceful restart
    check_slot_context_levels(state).await;
    // Restart marked slots once they become Idle (before any task dispatch)
    check_pending_compact_restarts(state).await;

    // Complete stale active conversations.
    let cutoff = (chrono::Utc::now()
        - chrono::TimeDelta::minutes(runtime_config.stale_conversation_minutes))
    .to_rfc3339();
    match state.store.complete_stale_conversations(&cutoff).await {
        Ok(n) if n > 0 => info!(count = n, "Completed stale conversations"),
        Err(e) => warn!(error = %e, "Failed to complete stale conversations"),
        _ => {}
    }

    // Reap expired dynamic slots (TTL lifecycle)
    reap_expired_dynamic_slots(state, &runtime_config).await;

    // GC completed jobs older than 30 minutes
    gc_completed_jobs(state, &runtime_config).await;

    // Scale-to-zero: release idle persistent slots after 30 minutes
    reap_idle_persistent_slots(state, &runtime_config).await;

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
    match state
        .store
        .reap_stale_slot_tasks(runtime_config.slot_task_reap_stale_secs)
        .await
    {
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

                let task_timeout_secs = derive_pty_timeout_secs(&runtime_config, rt.timeout_secs);
                let idle_threshold = idle_watchdog_threshold_secs(&runtime_config, rt.timeout_secs);
                let watchdog_grace_secs =
                    runtime_config.boardtask_timeout_policy.watchdog_grace_secs;

                match state.pty.get_status(slot_id).await {
                    Some(info) if info.state == SessionState::Idle => {
                        if let Ok(true) = close_idle_running_task_from_durable_summary(
                            state,
                            rt.id.as_str(),
                            slot_id,
                        )
                        .await
                        {
                            continue;
                        }
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
                            grace_secs = watchdog_grace_secs,
                            "Watchdog: task exceeded configured timeout/grace — slot idle, recovering"
                        );
                        let _ = state.store.unclaim_board_task(rt.id.as_str()).await;
                        let _ = state.store.add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: rt.id.to_string(),
                                content: format!(
                                    "🔄 **看门狗回收** — 任务超出配置 timeout/grace（claimed_age={}s, timeout={}s, grace={}s, 工位 {} 已 idle）。可能是 pty.send 在预算内自然结束、daemon 重启丢失 send()，或工位已归档结果。已 unclaim，下次 tick 重新执行。",
                                    claimed_age, task_timeout_secs, watchdog_grace_secs, slot_id
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
                        let missing_session_probe_secs = runtime_config
                            .boardtask_timeout_policy
                            .missing_session_probe_secs;
                        if claimed_age < missing_session_probe_secs {
                            continue;
                        }
                        warn!(
                            task_id = %rt.id, slot_id, age_secs = claimed_age,
                            probe_secs = missing_session_probe_secs,
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
    let _ = state
        .store
        .recover_stale_running_tasks(runtime_config.recover_stale_running_minutes)
        .await;

    dispatch_board_tasks_with_config(state, &runtime_config).await?;

    // Safety net: running tasks with no recent notes → Inbox reminder
    check_stale_board_progress(state, &runtime_config).await;

    // Phase 7: Consciousness — evaluate user state for proactive triggers
    if state.intent_analyst_enabled {
        evaluate_user_state(state, &runtime_config).await;
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
    let runtime_config = AutopilotRuntimeConfig::load_for_current_dir()?;
    dispatch_board_tasks_with_config(state, &runtime_config).await
}

/// Parse a `- field: value` line out of a `## Dispatch metadata` or
/// `## Swarm metadata` block embedded in a BoardTask description. Returns the
/// trimmed value when the field appears under one of those headings; falls
/// back to scanning the whole description so externally-built BoardTasks that
/// only use a `task_class:` line still hit. Empty values are dropped so
/// `field:` placeholders never override the structured class.
pub(crate) fn extract_dispatch_metadata_field(description: &str, field: &str) -> Option<String> {
    let needle = format!("- {}:", field);
    let mut in_metadata = false;
    let mut found_outside: Option<String> = None;
    for line in description.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let lowered = rest.to_ascii_lowercase();
            in_metadata =
                lowered.starts_with("dispatch metadata") || lowered.starts_with("swarm metadata");
            continue;
        }
        if let Some(value) = trimmed.strip_prefix(needle.as_str()) {
            let value = value.trim().trim_end_matches(',').trim().to_string();
            if value.is_empty() {
                continue;
            }
            if in_metadata {
                return Some(value);
            }
            if found_outside.is_none() {
                found_outside = Some(value);
            }
        }
    }
    found_outside
}

/// Recognised structured task classes the workstation pool understands. Used
/// to gate `extract_dispatch_metadata_field` results so a stray `task_class:`
/// note never coerces the autopilot into an unknown route.
const KNOWN_TASK_CLASSES: &[&str] = &[
    "research",
    "general",
    "ops",
    "code",
    "review",
    "context-pack",
    "lisp-compression",
];

fn class_from_str(value: &str) -> Option<&'static str> {
    let lowered = value.trim().to_ascii_lowercase();
    KNOWN_TASK_CLASSES
        .iter()
        .find(|known| **known == lowered.as_str())
        .copied()
}

fn board_task_workstation_class(task: &missiond_core::types::BoardTask) -> &'static str {
    if task.category == "ops" {
        return "ops";
    }
    match task.context_intent.as_deref().map(str::trim) {
        Some("research") => "research",
        Some("general") | Some("") | None => {
            // Default/general fall-through: prefer the structured task_class
            // line embedded in the dispatch/swarm metadata block before
            // applying title/description keyword heuristics. This lets
            // externally-created BoardTasks (mission_board_create, scripts,
            // operator paste-in) route as `review`, `context-pack`, etc.
            // without round-tripping through `intent`.
            if let Some(value) = extract_dispatch_metadata_field(&task.description, "task_class") {
                if let Some(class) = class_from_str(&value) {
                    return class;
                }
            }
            let title = task.title.to_ascii_lowercase();
            let description = task.description.to_ascii_lowercase();
            if title.contains("read-only")
                || description.contains("read-only")
                || title.contains("survey")
                || description.contains("survey")
                || title.contains("investigate")
                || description.contains("investigate")
            {
                "research"
            } else {
                "code"
            }
        }
        Some("ops") => "ops",
        Some("code") => "code",
        Some("review") => "review",
        Some("context-pack") => "context-pack",
        Some("lisp-compression") => "lisp-compression",
        _ => "code",
    }
}

#[derive(Debug, Clone)]
struct WorkstationSlotSelection {
    slot_id: String,
    reroute_reason: Option<String>,
}

fn workstation_worker_matches_dispatch_hints(
    worker: &crate::context::v3_blueprint_runtime::WorkstationPoolRuntimeConfig,
    engine_hint: Option<&str>,
    pool_hint: Option<&str>,
) -> bool {
    let engine_match = engine_hint
        .map(|hint| worker.engine.eq_ignore_ascii_case(hint))
        .unwrap_or(true);
    let pool_match = pool_hint
        .map(|hint| {
            worker.id.eq_ignore_ascii_case(hint)
                || worker.role.eq_ignore_ascii_case(hint)
                || worker.slot_id.eq_ignore_ascii_case(hint)
        })
        .unwrap_or(true);
    engine_match && pool_match
}

async fn select_workstation_pool_slot(
    state: &AppState,
    workstation_config: &WorkstationRuntimeConfig,
    task: &missiond_core::types::BoardTask,
    dispatched_slots: &HashSet<String>,
    excluded_roles: &[&str],
) -> Option<WorkstationSlotSelection> {
    let task_class = board_task_workstation_class(task);
    let engine_hint = extract_dispatch_metadata_field(&task.description, "engine_hint");
    let pool_hint = extract_dispatch_metadata_field(&task.description, "pool_hint");
    let mut candidates: Vec<&_> = workstation_config
        .boardtask_pool_candidates(task_class)
        .into_iter()
        .collect();
    if pool_hint.is_some() {
        let matching_candidates: Vec<_> = workstation_config
            .workstation_pool()
            .iter()
            .filter(|worker| {
                worker.accepts_boardtask
                    && workstation_worker_matches_dispatch_hints(
                        worker,
                        engine_hint.as_deref(),
                        pool_hint.as_deref(),
                    )
            })
            .collect();
        if !matching_candidates.is_empty() {
            // Explicit engine/pool hints are operator intent. Treat a matching
            // worker declared anywhere in the V3 pool as a hard constraint
            // even when task_class parsing or class membership would otherwise
            // narrow it away; if that worker is busy, defer instead of
            // spending a different provider.
            candidates = matching_candidates;
        }
    } else if engine_hint.is_some() {
        let matching_candidates: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|worker| {
                workstation_worker_matches_dispatch_hints(
                    worker,
                    engine_hint.as_deref(),
                    pool_hint.as_deref(),
                )
            })
            .collect();
        if !matching_candidates.is_empty() {
            // Engine hints rank/filter the task-class candidates only. They
            // must not widen a `task_class=code` shard into the Sonnet
            // fast-patch lane just because both are Claude Code workers.
            candidates = matching_candidates;
        }
    }
    // Re-rank: workers that match an explicit engine_hint / pool_hint go
    // first. If the V3 pool declares at least one exact match, the filter
    // above makes the hint a hard constraint: a busy ClaudeCode worker should
    // defer the task, not silently spend a Gemini lane. Fallback reroutes are
    // only possible when no declared worker satisfies the hint at all.
    if engine_hint.is_some() || pool_hint.is_some() {
        candidates.sort_by_key(|worker| {
            let engine_match = engine_hint
                .as_deref()
                .map(|hint| worker.engine.eq_ignore_ascii_case(hint))
                .unwrap_or(true);
            let pool_match = pool_hint
                .as_deref()
                .map(|hint| {
                    worker.id.eq_ignore_ascii_case(hint)
                        || worker.role.eq_ignore_ascii_case(hint)
                        || worker.slot_id.eq_ignore_ascii_case(hint)
                })
                .unwrap_or(true);
            // 0 = both match (best), 1 = pool only, 2 = engine only, 3 = neither.
            match (engine_match, pool_match) {
                (true, true) => 0,
                (true, false) => 2,
                (false, true) => 1,
                (false, false) => 3,
            }
        });
    }
    for worker in candidates {
        if task_class == "code" && !worker.write_allowed {
            continue;
        }
        if dispatched_slots.contains(&worker.slot_id) {
            continue;
        }
        let Some(slot) = state.mission.get_slot(&worker.slot_id) else {
            continue;
        };
        if excluded_roles.contains(&slot.config.role.as_str()) {
            continue;
        }
        let pick = || {
            let engine_match = engine_hint
                .as_deref()
                .map(|hint| worker.engine.eq_ignore_ascii_case(hint))
                .unwrap_or(true);
            let pool_match = pool_hint
                .as_deref()
                .map(|hint| {
                    worker.id.eq_ignore_ascii_case(hint)
                        || worker.role.eq_ignore_ascii_case(hint)
                        || worker.slot_id.eq_ignore_ascii_case(hint)
                })
                .unwrap_or(true);
            let reroute_reason = if !engine_match || !pool_match {
                let reason = format!(
                    "engine_hint/pool_hint not satisfied; requested_engine={}, requested_pool={}, chosen_engine={}, chosen_pool={}, chosen_slot={}",
                    engine_hint.as_deref().unwrap_or("-"),
                    pool_hint.as_deref().unwrap_or("-"),
                    worker.engine,
                    worker.id,
                    worker.slot_id,
                );
                tracing::warn!(
                    task_id = %task.id,
                    task_class,
                    requested_engine = engine_hint.as_deref().unwrap_or("-"),
                    requested_pool = pool_hint.as_deref().unwrap_or("-"),
                    chosen_engine = %worker.engine,
                    chosen_slot = %worker.slot_id,
                    "Autopilot: dispatch reroute — engine/pool hint not satisfied by available pool, fell back to nearest candidate"
                );
                Some(reason)
            } else {
                None
            };
            WorkstationSlotSelection {
                slot_id: worker.slot_id.clone(),
                reroute_reason,
            }
        };
        if let Some(info) = state.pty.get_status(&worker.slot_id).await {
            if matches!(
                info.state,
                SessionState::Idle | SessionState::Exited | SessionState::Error
            ) {
                return Some(pick());
            }
            continue;
        }
        if slot.config.lifecycle == Some(missiond_core::types::Lifecycle::Persistent) {
            return Some(pick());
        }
    }
    None
}

async fn dispatch_board_tasks_with_config(
    state: &AppState,
    runtime_config: &AutopilotRuntimeConfig,
) -> Result<()> {
    if state.control_manager.current().global_paused {
        return Ok(());
    }
    let router_config = RouterRuntimeConfig::load_for_current_dir()?;
    let workstation_config = WorkstationRuntimeConfig::load_for_current_dir()?;

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

    // V3 execution-ownership :: delegated-boardtask :: dispatch-guard.
    // Concurrent slot dispatch: each ready BoardTask hands its prepared
    // send + post-send tail to a detached `tokio::spawn` task so different
    // slots' state.pty.send calls can run while the Autopilot event loop
    // keeps processing later Board/Slot events. The OwnedSlotDispatchGuard
    // travels into each spawned task and is dropped only after the post-send
    // tail completes, so same-slot exclusion still covers the entire send +
    // close-owner / KB-feedback / deploy-review sequence. The legacy serial
    // loop awaited state.pty.send inline, and the later JoinSet drain variant
    // still blocked later ticks until early workers completed.

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
        if is_stale_lisp_code_sync_runtime_report_task(&task) {
            resolve_stale_lisp_code_sync_runtime_report_task(state, &task).await;
            continue;
        }

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

                if should_clear_stale_dynamic_assignee(id, runtime_slot_exists, dynamic_slot_active)
                {
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
                match select_workstation_pool_slot(
                    state,
                    &workstation_config,
                    &task,
                    &dispatched_slots,
                    EXCLUDED_ROLES,
                )
                .await
                {
                    Some(selection) => {
                        let id = selection.slot_id;
                        if let Some(reason) = selection.reroute_reason {
                            let _ = state
                                .store
                                .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                                    task_id: task.id.to_string(),
                                    content: format!(
                                        "⚠️ Workstation dispatch reroute recorded: {}",
                                        reason
                                    ),
                                    note_type: Some("note".to_string()),
                                    author: Some("autopilot".to_string()),
                                })
                                .await;
                        }
                        info!(task_id = %task.id, slot_id = %id, "Autopilot: selected V3 workstation-pool slot");
                        // Don't persist assignee yet — avoid Task Pinning Bug.
                        // If claim/pty fails, task stays unassigned for next tick to re-route.
                        id
                    }
                    None => {
                        debug!(task_id = %task.id, "Autopilot: no V3 workstation-pool slot available, deferring");
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

        // Unified context injection via Context Prefetch Pipeline. Default is
        // off until memory stores are cleaned up; worker prompts should be
        // driven by explicit BoardTask scope rather than hidden KB/Skill noise.
        let (full_prompt, cited_kb_ids) = {
            if autopilot_context_prefetch_enabled() {
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
            } else {
                (prompt, Vec::new())
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
                if count >= 3 && now - last_fail < runtime_config.slot_failure_throttle_secs {
                    debug!(slot_id = %slot_id, failures = count, "Autopilot: slot throttled, skipping");
                    continue;
                }
            }
        }

        info!(task_id = %task.id, slot_id = %slot_id, title = %task.title, "Autopilot: executing task");

        // V3 slot-attribution :: single-running-task-per-slot invariant.
        //
        // Defensively unclaim any other task whose claim_executor still points
        // at this slot before the new dispatch claims it. Without this guard,
        // a task that finished without clearing its claim (crash, killed PTY,
        // race in update path) leaves a stale running row glued to the slot.
        // The display layer (`active_board_task_for_slot`) then sees two
        // running tasks for one slot and reported `task 738c96f5 and 5599b07a
        // running` on slot-claude-code-default during BoardTask
        // 31e5449c-e315-4003-ad59-c3eebd5eb837. Take the dispatch site as the
        // last write authority and reset the slot to a single-claim state.
        let stale_running_claims = stale_running_claims_for_slot(state, &slot_id, task.id.as_str())
            .await
            .unwrap_or_default();
        for stale_id in &stale_running_claims {
            warn!(
                slot_id = %slot_id,
                task_id = %task.id,
                stale_task_id = %stale_id,
                "Autopilot: clearing stale running claim before new dispatch"
            );
            let _ = state.store.unclaim_board_task(stale_id).await;
            let _ = state
                .store
                .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                    task_id: stale_id.clone(),
                    content: format!(
                        "🧹 Autopilot dispatch reset: claim on slot `{}` displaced by task `{}`. The previous run left this row claimed without closing — releasing so the new dispatch owns the slot.",
                        slot_id, task.id
                    ),
                    note_type: Some("note".to_string()),
                    author: Some("autopilot".to_string()),
                })
                .await;
        }

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
                let lease_secs = derive_board_task_lease_secs(runtime_config, task.timeout_secs);
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
        let task_env = determine_llm_env(&task, &slot_role, &router_config);

        // Check if PTY session exists, spawn if needed
        if !ensure_autopilot_pty(state, &task, &slot_id, task_env).await {
            let _ = state.store.unclaim_board_task(task.id.as_str()).await;
            continue;
        }

        // Link PTY session to task for audit trail (durable, with bounded
        // retry so JSONL-hook race doesn't strand the linkage until the
        // completion-time backfill in durable_provider_completion_for_slot_task).
        let _ =
            crate::flow_engine::bind_conversation_to_task(state, &slot_id, task.id.as_str()).await;
        // V3 slot-attribution :: dispatch is the rebind authority.
        //
        // bind_conversation_to_task preserves an existing binding when it
        // disagrees with the new task id — that protects the post-completion
        // reconciliation path but lets a stale conv.task_id from the previous
        // dispatch survive into the next one. During BoardTask
        // 31e5449c-e315-4003-ad59-c3eebd5eb837, this caused
        // `mission_conversation_query(taskId=738c96f5)` to return the 5599b07a
        // conversation. Force-rebind here so the dispatch site is the single
        // source of truth for "which BoardTask owns this slot's session right
        // now". Best-effort: if the slot session is not yet registered, the
        // bounded-retry loop above already deferred to durable backfill.
        rebind_slot_conversation_for_dispatch(state, &slot_id, task.id.as_str()).await;

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
        // slots. The owned variant lets the guard travel into the spawned
        // dispatch task while still releasing on Drop, so same-slot
        // exclusion covers the entire send + post-send tail without
        // serializing different-slot sends inside this dispatch tick. The
        // legacy borrow-shaped `state.slot_dispatch.try_acquire_guard(&slot_id)`
        // tied the lock lifetime to `&AppState`, which forced the loop to
        // await one send before another slot's send could even start.
        let slot_guard = match OwnedSlotDispatchGuard::try_acquire(&state.slot_dispatch, &slot_id) {
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

        // Spawn the send + post-send tail as a detached task. The
        // OwnedSlotDispatchGuard moves into the spawned task and is dropped
        // only after the post-send tail completes, so the per-slot dispatch
        // guard is held across the entire state.pty.send call. Each spawned
        // task carries an `AppState` clone (Arc-backed) plus a cloned
        // `BoardTask`, the resolved slot_id, and the assembled prompt.
        // Quota / KB-feedback / deploy-review / retry semantics update
        // durable state from the background tail instead of blocking the next
        // dispatch tick.
        let timeout_ms = derive_pty_timeout_ms(runtime_config, task.timeout_secs);
        let deploy_review_timeout_ms = runtime_config.deploy_review_timeout_ms();
        let send_state = state.clone();
        let send_task = task.clone();
        let send_slot_id = slot_id.clone();
        let send_full_prompt = full_prompt;
        tokio::spawn(async move {
            let _slot_guard = slot_guard;
            let state: &AppState = &send_state;
            let task: &missiond_core::types::BoardTask = &send_task;
            let slot_id: String = send_slot_id;
            let full_prompt: String = send_full_prompt;
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
                            notify_jarvis_failure(state, &task, "OAuth token 过期，工位认证失败")
                                .await;
                        } else {
                            let _ = state
                                .store
                                .increment_board_task_retry(task.id.as_str(), new_retry)
                                .await;
                        }
                        return;
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
                        // Quota is gone — stop this task's post-send tail. Other
                        // in-flight sends already started concurrently (different
                        // slots), so they finish on their own; the next dispatch
                        // tick will short-circuit on global_paused above.
                        return;
                    }

                    // V3 resident-master-control :: evidence-authority / settle-policy.
                    // `pty.send` completion is a high-confidence turn-level signal,
                    // but provider JSONL/SSE final text and MissionD conversation
                    // ingestion can lag the prompt returning. Wait briefly before
                    // writing the durable BoardTask close state; PTY idle alone is
                    // never used as completion authority.
                    wait_for_worker_final_settle_window().await;

                    // JSONL/session discovery can race the pre-send binding,
                    // especially for external-project persistent ClaudeCode
                    // sessions whose provider log lands after the first prompt.
                    // Re-bind after the durable-final settle window so
                    // mission_conversation_query(taskId=...) has a stable
                    // BoardTask -> provider conversation join for worker audit.
                    let rebound = crate::flow_engine::bind_conversation_to_task(
                        state,
                        &slot_id,
                        task.id.as_str(),
                    )
                    .await;
                    if rebound {
                        debug!(
                            task_id = %task.id,
                            slot_id = %slot_id,
                            "Autopilot: rebound provider conversation to BoardTask after settle"
                        );
                    }
                    // V3 execution-ownership :: delegated-boardtask :: summary-note source.
                    // Prefer the durable provider final after the JSONL/chat-store
                    // settle + single-session reconcile. If the first reconcile
                    // still races the provider final write, poll the durable log
                    // for one more settle budget before falling back to the TUI
                    // screen. The TUI `res.response` screen is only the fallback
                    // because it can lag, clip, or render an intermediate frame
                    // even after the prompt returns. Auth/quota diagnostic notes
                    // above intentionally bypass this path and keep the raw
                    // response.
                    let durable_completion = await_durable_provider_completion_for_slot_task(
                    state,
                    task,
                    &slot_id,
                )
                .await
                .unwrap_or_else(|err| {
                    warn!(
                        task_id = %task.id,
                        slot_id = %slot_id,
                        error = %err,
                        "Autopilot: durable provider final lookup failed; falling back to PTY screen summary"
                    );
                    None
                });
                    let final_summary = durable_completion
                        .as_ref()
                        .map(|completion| completion.summary.trim().to_string())
                        .filter(|summary| !summary.is_empty())
                        .unwrap_or_else(|| {
                            extract_worker_final_summary(&res.response, &full_prompt)
                        });
                    if is_probably_active_tui_summary(&final_summary) {
                        warn!(
                            task_id = %task.id,
                            slot_id = %slot_id,
                            duration_ms = res.duration_ms,
                            "Autopilot: pty.send returned an active/progress frame; preserving running task state"
                        );
                        let _ = state
                        .store
                        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: "⚠️ PTY 返回了仍在运行的进度帧，Autopilot 未关闭任务；等待工位稳定后由 watchdog/后续 tick 处理。".to_string(),
                            note_type: Some("note".to_string()),
                            author: Some("autopilot".to_string()),
                        })
                        .await;
                        return;
                    }
                    if let Some(blocker) =
                        output_contract_close_blocker(&task.description, &final_summary)
                    {
                        warn!(
                            task_id = %task.id,
                            slot_id = %slot_id,
                            duration_ms = res.duration_ms,
                            blocker,
                            "Autopilot: completion summary does not satisfy worker output contract; preserving task"
                        );
                        let note = format!(
                            "⚠️ **Autopilot blocked close** — worker summary is missing `{}`. The BoardTask stays running so the next settle/recovery pass can capture the current task's structured artifact instead of a stale provider-session summary.\n\n{}",
                            blocker,
                            truncate_safe(&final_summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES),
                        );
                        let _ = state
                            .store
                            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.to_string(),
                                content: note,
                                note_type: Some("note".to_string()),
                                author: Some("autopilot".to_string()),
                            })
                            .await;
                        return;
                    }
                    if let Some(blocker) = worker_final_close_blocker(&final_summary) {
                        warn!(
                            task_id = %task.id,
                            slot_id = %slot_id,
                            duration_ms = res.duration_ms,
                            blocker,
                            "Autopilot: worker final reports a blocking commit/tool failure; preserving task for recovery"
                        );
                        let note = format!(
                        "⚠️ **Autopilot blocked close** — worker final indicates `{}`. The BoardTask stays blocked so a supervisor/worker can recover instead of recording a false done state.\n\n{}",
                        blocker,
                        truncate_safe(&final_summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES),
                    );
                        let _ = state
                            .store
                            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.to_string(),
                                content: note,
                                note_type: Some("note".to_string()),
                                author: Some("autopilot".to_string()),
                            })
                            .await;
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
                            .update_prompt_snapshot_outcome(task.id.as_str(), "blocked")
                            .await;
                        return;
                    }
                    if let Some(blocker) = pty_only_close_blocker(
                        &task.description,
                        durable_completion.is_some(),
                        &final_summary,
                    ) {
                        warn!(
                            task_id = %task.id,
                            slot_id = %slot_id,
                            duration_ms = res.duration_ms,
                            blocker,
                            "Autopilot: PTY-only completion lacks structured artifact; preserving task until provider final settles"
                        );
                        let note = format!(
                            "⚠️ **Autopilot blocked close** — PTY-only summary missing `{}`. Final artifact not durable yet (no provider JSONL/SSE final after settle window). The BoardTask stays running so the watchdog/next tick can re-extract once the provider log lands.\n\n{}",
                            blocker,
                            truncate_safe(&final_summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES),
                        );
                        let _ = state
                            .store
                            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.to_string(),
                                content: note,
                                note_type: Some("note".to_string()),
                                author: Some("autopilot".to_string()),
                            })
                            .await;
                        return;
                    }
                    if let Some(blocker) = delegated_write_close_evidence_blocker(
                        &task.description,
                        durable_completion.is_some(),
                        &final_summary,
                    ) {
                        warn!(
                            task_id = %task.id,
                            slot_id = %slot_id,
                            duration_ms = res.duration_ms,
                            blocker,
                            "Autopilot: write-scope task lacks durable completion/acceptance evidence; preserving task for recovery"
                        );
                        let note = format!(
                        "⚠️ **Autopilot blocked close** — write-scope task is missing `{}`. The BoardTask stays blocked so a supervisor/worker can recover instead of recording a false done state.\n\n{}",
                        blocker,
                        truncate_safe(&final_summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES),
                    );
                        let _ = state
                            .store
                            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.to_string(),
                                content: note,
                                note_type: Some("note".to_string()),
                                author: Some("autopilot".to_string()),
                            })
                            .await;
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
                            .update_prompt_snapshot_outcome(task.id.as_str(), "blocked")
                            .await;
                        return;
                    }
                    let summary_for_note =
                        truncate_safe(&final_summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES);
                    let durable_source = durable_completion
                        .as_ref()
                        .map(|completion| {
                            format!(
                                "; durable final {} / {}",
                                completion.source, completion.session_id
                            )
                        })
                        .unwrap_or_default();
                    let note_content = format!(
                        "**Autopilot 执行完成** ({}ms{})\n\n{}",
                        res.duration_ms, durable_source, summary_for_note
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
                    match maybe_complete_delegated_execution_log(
                        state,
                        task,
                        &full_prompt,
                        &final_summary,
                        res.duration_ms,
                    )
                    .await
                    {
                        Ok(true) => {
                            info!(task_id = %task.id, "Autopilot: synthesized mission_execution completion");
                        }
                        Ok(false) => {}
                        Err(err) => {
                            warn!(task_id = %task.id, error = %err, "Autopilot: mission_execution completion synthesis failed");
                        }
                    }
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
                        if let Ok(meta) =
                            serde_json::from_str::<serde_json::Value>(&task.description)
                        {
                            if let Some(conv_id) =
                                meta.get("conversation_id").and_then(|v| v.as_str())
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
                                .send(MEMORY_SLOW_SLOT_ID, &prompt, deploy_review_timeout_ms)
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
        });
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
async fn reap_expired_dynamic_slots(state: &AppState, runtime_config: &AutopilotRuntimeConfig) {
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

    // TTL warning: alert for slots expiring soon.
    if let Ok(expiring) = state
        .store
        .find_expiring_dynamic_slots(runtime_config.dynamic_slot_expiring_soon_secs)
        .await
    {
        for slot in &expiring {
            debug!(slot_id = %slot.id, expires_at = %slot.expires_at, "Dynamic slot expiring soon (15min warning)");
        }
    }
}

/// Safety net: running Board tasks with no recent progress notes → Inbox reminder.
/// Runs every 5 ticks (~5 min). Deduplicates by checking existing unread inbox.
async fn check_stale_board_progress(state: &AppState, runtime_config: &AutopilotRuntimeConfig) {
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
            if age_min >= runtime_config.stale_board_progress_minutes {
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
async fn gc_completed_jobs(state: &AppState, runtime_config: &AutopilotRuntimeConfig) {
    use missiond_core::types::AsyncJobStatus;

    let cutoff =
        chrono::Utc::now() - chrono::Duration::minutes(runtime_config.completed_job_gc_minutes);
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
async fn evaluate_user_state(state: &AppState, runtime_config: &AutopilotRuntimeConfig) {
    // 1. Pull recent intents (last 30 min, global)
    let intents = match state
        .store
        .get_recent_intents(runtime_config.recent_intents_window_secs)
        .await
    {
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
        if stuck.len() >= 3
            && !in_cooldown(
                state,
                &ck_stuck,
                runtime_config.user_stuck_cooldown_secs,
                now,
            )
        {
            let summary = build_stuck_summary(&stuck, runtime_config.recent_intents_window_secs);
            trigger_jarvis_push(state, "user_stuck", &summary).await;
            set_cooldown(state, &ck_stuck, now);
        }

        // DirectionShift: architecture_explore confidence > 0.8 → L2 Inbox
        if let Some(shift) = s_intents
            .iter()
            .find(|i| i.intent_type == "architecture_explore" && i.confidence > 0.8)
        {
            let ck = format!("direction_shift:{}", session_id);
            if !in_cooldown(
                state,
                &ck,
                runtime_config.direction_shift_cooldown_secs,
                now,
            ) {
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
            if !in_cooldown(
                state,
                &ck,
                runtime_config.direction_shift_cooldown_secs,
                now,
            ) {
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

fn build_stuck_summary(
    intents: &[&missiond_core::types::UserIntent],
    recent_window_secs: i64,
) -> String {
    let details: Vec<String> = intents
        .iter()
        .filter_map(|i| i.summary.as_deref())
        .map(|s| format!("- {}", s))
        .collect();
    let window_minutes = (recent_window_secs / 60).max(1);
    format!(
        "用户在最近 {} 分钟内连续 {} 次卡在同一问题上：\n{}\n\n建议：检查是否需要换一种方法，或提供更多上下文帮助用户。",
        window_minutes,
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
        let summary = build_stuck_summary(&refs, 1800);
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
        let summary = build_stuck_summary(&refs, 1800);
        assert!(summary.contains("连续 1 次"));
    }

    // ── PTY timeout / watchdog policy — pure helpers, no AppState ───────

    fn runtime_config() -> AutopilotRuntimeConfig {
        AutopilotRuntimeConfig::default()
    }

    #[test]
    fn pty_timeout_default_when_field_absent() {
        let cfg = runtime_config();
        assert_eq!(
            derive_pty_timeout_secs(&cfg, None),
            cfg.boardtask_timeout_policy.default_secs
        );
        assert_eq!(
            derive_pty_timeout_ms(&cfg, None),
            (cfg.boardtask_timeout_policy.default_secs as u64) * 1000
        );
    }

    #[test]
    fn pty_timeout_default_for_invalid_values() {
        let cfg = runtime_config();
        // Zero and negative values are treated as "absent" and fall back to
        // the default — mirrors task_delegate's safe-default behaviour.
        assert_eq!(
            derive_pty_timeout_secs(&cfg, Some(0)),
            cfg.boardtask_timeout_policy.default_secs
        );
        assert_eq!(
            derive_pty_timeout_secs(&cfg, Some(-300)),
            cfg.boardtask_timeout_policy.default_secs
        );
    }

    #[test]
    fn pty_timeout_clamps_low_values() {
        let cfg = runtime_config();
        // Anything under the configured floor rounds up so a
        // mis-configured 5-second task still gets a usable PTY budget.
        assert_eq!(
            derive_pty_timeout_secs(&cfg, Some(5)),
            cfg.boardtask_timeout_policy.min_secs
        );
        assert_eq!(
            derive_pty_timeout_secs(&cfg, Some(59)),
            cfg.boardtask_timeout_policy.min_secs
        );
        assert_eq!(derive_pty_timeout_secs(&cfg, Some(60)), 60);
    }

    #[test]
    fn pty_timeout_clamps_high_values() {
        let cfg = runtime_config();
        // The cap mirrors task_delegate::MAX_TIMEOUT_SECS so neither side
        // can drift past the other.
        assert_eq!(
            derive_pty_timeout_secs(&cfg, Some(7200)),
            cfg.boardtask_timeout_policy.max_secs
        );
        assert_eq!(
            derive_pty_timeout_secs(&cfg, Some(86_400)),
            cfg.boardtask_timeout_policy.max_secs
        );
    }

    #[test]
    fn pty_timeout_in_range_passes_through() {
        let cfg = runtime_config();
        // 55-minute Opus task — the wave31 stability bug case. Must not be
        // shrunk to 10 minutes anywhere along the path.
        assert_eq!(derive_pty_timeout_secs(&cfg, Some(3300)), 3300);
        assert_eq!(derive_pty_timeout_ms(&cfg, Some(3300)), 3_300_000);
    }

    #[test]
    fn idle_watchdog_threshold_adds_grace_to_task_timeout() {
        let cfg = runtime_config();
        // Default budget + grace.
        assert_eq!(
            idle_watchdog_threshold_secs(&cfg, None),
            cfg.boardtask_timeout_policy.default_secs
                + cfg.boardtask_timeout_policy.watchdog_grace_secs
        );
        // Explicit 55-minute task → 3300 + 120 = 3420.
        assert_eq!(idle_watchdog_threshold_secs(&cfg, Some(3300)), 3420);
    }

    #[test]
    fn idle_watchdog_threshold_strictly_above_old_120s_floor() {
        let cfg = runtime_config();
        // Regression guard for wave31: the legacy 120s floor must never
        // re-emerge for any in-range task timeout.
        for secs in [
            cfg.boardtask_timeout_policy.min_secs,
            300,
            900,
            1800,
            3300,
            cfg.boardtask_timeout_policy.max_secs,
        ] {
            let threshold = idle_watchdog_threshold_secs(&cfg, Some(secs));
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
        let cfg = runtime_config();
        // claimed_age < idle_threshold ⇒ watchdog must not reclaim.
        let timeout = Some(3300);
        let threshold = idle_watchdog_threshold_secs(&cfg, timeout);
        // Within the budget — reclaim forbidden.
        assert!(900 < threshold);
        assert!(3300 < threshold);
        // Past the budget+grace — reclaim allowed.
        assert!(threshold + 1 > threshold);
    }

    // ── BoardTask claim lease — pure helper, no AppState ────────────────

    #[test]
    fn board_task_lease_default_when_field_absent() {
        let cfg = runtime_config();
        // Default budget + grace = lease, mirroring the watchdog threshold
        // so the watchdog never reclaims while the lease is still valid.
        assert_eq!(
            derive_board_task_lease_secs(&cfg, None),
            cfg.boardtask_timeout_policy.default_secs
                + cfg.boardtask_timeout_policy.watchdog_grace_secs
        );
    }

    #[test]
    fn board_task_lease_default_for_invalid_values() {
        let cfg = runtime_config();
        // Zero / negative timeouts fall back to the default budget; lease
        // therefore matches the default watchdog threshold.
        let expected = cfg.boardtask_timeout_policy.default_secs
            + cfg.boardtask_timeout_policy.watchdog_grace_secs;
        assert_eq!(derive_board_task_lease_secs(&cfg, Some(0)), expected);
        assert_eq!(derive_board_task_lease_secs(&cfg, Some(-300)), expected);
    }

    #[test]
    fn board_task_lease_explicit_3300_is_3420() {
        let cfg = runtime_config();
        // Wave31 / wave50 case: a 55-minute Opus task gets a 3300s pty
        // budget and a 3420s lease (3300 + 120s grace). The legacy fixed
        // 20-minute lease would have been 1200s — too short.
        assert_eq!(derive_board_task_lease_secs(&cfg, Some(3300)), 3420);
    }

    #[test]
    fn board_task_lease_clamps_high_values() {
        let cfg = runtime_config();
        // PTY budget caps at the configured max, so the lease caps at
        // max + configured watchdog grace.
        assert_eq!(
            derive_board_task_lease_secs(&cfg, Some(86_400)),
            cfg.boardtask_timeout_policy.max_secs
                + cfg.boardtask_timeout_policy.watchdog_grace_secs
        );
    }

    #[test]
    fn board_task_lease_clamps_low_values() {
        let cfg = runtime_config();
        // Sub-floor timeouts round up to the configured min, so the lease
        // is min + configured watchdog grace.
        assert_eq!(
            derive_board_task_lease_secs(&cfg, Some(5)),
            cfg.boardtask_timeout_policy.min_secs
                + cfg.boardtask_timeout_policy.watchdog_grace_secs
        );
    }

    #[test]
    fn board_task_lease_matches_idle_watchdog_threshold() {
        let cfg = runtime_config();
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
                derive_board_task_lease_secs(&cfg, t),
                idle_watchdog_threshold_secs(&cfg, t),
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
        let cfg = runtime_config();
        // Even a 2-hour task must let the no-PTY-session branch recover
        // after the small probe window — a missing process can never
        // resume on its own.
        assert_eq!(cfg.boardtask_timeout_policy.missing_session_probe_secs, 120);
        assert!(
            cfg.boardtask_timeout_policy.missing_session_probe_secs
                < idle_watchdog_threshold_secs(&cfg, Some(cfg.boardtask_timeout_policy.max_secs))
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
    fn autopilot_context_prefetch_is_opt_in_only() {
        assert!(!autopilot_context_prefetch_enabled_from(None));
        assert!(!autopilot_context_prefetch_enabled_from(Some("")));
        assert!(!autopilot_context_prefetch_enabled_from(Some("false")));
        assert!(!autopilot_context_prefetch_enabled_from(Some("0")));
        assert!(autopilot_context_prefetch_enabled_from(Some("1")));
        assert!(autopilot_context_prefetch_enabled_from(Some("true")));
        assert!(autopilot_context_prefetch_enabled_from(Some("ON")));
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

    fn test_note(
        note_type: missiond_core::types::BoardNoteType,
        content: &str,
        created_at: &str,
    ) -> missiond_core::types::BoardTaskNote {
        missiond_core::types::BoardTaskNote {
            id: "note-1".to_string(),
            task_id: "task-1".to_string(),
            content: content.to_string(),
            note_type,
            author: Some("codex".to_string()),
            created_at: created_at.to_string(),
        }
    }

    fn test_conversation_message(
        id: i64,
        role: &str,
        content: &str,
        timestamp: &str,
    ) -> missiond_core::types::ConversationMessage {
        missiond_core::types::ConversationMessage {
            id,
            session_id: "session-1".to_string(),
            role: role.to_string(),
            content: content.to_string(),
            raw_content: None,
            message_uuid: None,
            parent_uuid: None,
            model: None,
            timestamp: timestamp.to_string(),
            metadata: None,
            tool_name: None,
            raw_role: None,
            content_types: None,
            has_image: false,
            has_tool_use: false,
            has_tool_result: false,
            token_count: None,
            seq: None,
            role_display: None,
        }
    }

    #[test]
    fn durable_completion_summary_note_rejects_progress_warning() {
        let note = test_note(
            missiond_core::types::BoardNoteType::Note,
            "⚠️ PTY 返回了仍在运行的进度帧，Autopilot 未关闭任务",
            "2026-05-02T15:19:39Z",
        );
        assert!(!is_durable_completion_summary_note(&note));

        let summary = test_note(
            missiond_core::types::BoardNoteType::Summary,
            "Read-only smoke inspected git status; no files were changed.",
            "2026-05-02T15:19:50Z",
        );
        assert!(is_durable_completion_summary_note(&summary));
    }

    #[test]
    fn provider_final_summary_requires_claim_after_task_prompt() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask old-task-id prompt",
                "2026-05-03T10:14:00Z",
            ),
            test_conversation_message(2, "assistant", "old answer", "2026-05-03T10:14:30Z"),
            test_conversation_message(
                3,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-03T10:15:00Z",
            ),
            test_conversation_message(
                4,
                "assistant",
                "MissionD successfully projected Gemini into Plan Mode.",
                "2026-05-03T10:15:40Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-03T10:14:50Z"))
                .as_deref(),
            Some("MissionD successfully projected Gemini into Plan Mode.")
        );
        assert_eq!(
            latest_assistant_after_task_prompt(
                &messages,
                "old-task-id",
                Some("2026-05-03T10:14:50Z")
            ),
            None
        );
    }

    #[test]
    fn provider_final_summary_prefers_current_task_prompt_anchor_in_reused_session() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "Execute BoardTask task-s2 from wave jarvis-m6.",
                "2026-05-04T06:31:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "JARVIS_S2_INVARIANTS_DONE\ncommit: `a1c77a1`",
                "2026-05-04T06:39:00Z",
            ),
            test_conversation_message(
                3,
                "system",
                "Execute BoardTask task-s3 from wave jarvis-m6.",
                "2026-05-04T06:41:00Z",
            ),
            test_conversation_message(
                4,
                "assistant",
                "JARVIS_S3_DATA_DONE\ncommit: `edd5c96`",
                "2026-05-04T06:42:00Z",
            ),
        ];

        assert_eq!(
            provider_completion_summary_for_task(
                &messages,
                "task-s3",
                Some("2026-05-04T06:30:00Z"),
                Some("task-s3"),
            )
            .as_deref(),
            Some("JARVIS_S3_DATA_DONE\ncommit: `edd5c96`")
        );
    }

    #[test]
    fn provider_final_summary_rejects_active_tui_progress_frame() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-03T10:15:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "✦ Thinking... (esc to cancel)",
                "2026-05-03T10:15:20Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-03T10:14:50Z")),
            None
        );
    }

    #[test]
    fn provider_final_summary_rejects_retrying_once_progress() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T15:06:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "GPG pinentry was cancelled — retrying once with the same command.",
                "2026-05-04T15:08:00Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T15:05:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(
                "GPG pinentry was cancelled — retrying once with the same command."
            ),
            "retry progress must not be treated as a durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_survey_progress_prefixes() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "Checking jarvis-forge SSOT-convergence evidence...",
                "2026-05-04T18:00:12Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary("Checking jarvis-forge SSOT-convergence evidence..."),
            "survey progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_let_me_corroborate_progress() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "Let me corroborate the MySQL classification by greping inside read scope and confirming the live DB driver.",
                "2026-05-04T18:01:00Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary("Let me corroborate the MySQL classification by greping inside read scope and confirming the live DB driver."),
            "corroboration progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_initial_worker_intent_progress() {
        let progress = "I'll execute this read-only static analysis task. Let me start by reading the context pack and surveying the auth service structure.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "initial worker intent must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_begin_by_reading_progress() {
        let progress = "I'll begin by reading the context pack and the existing test/harness/SSOT files in parallel to map out what's already in place.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "begin-by-reading progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_let_me_examine_progress() {
        let progress = "Let me examine the actual table schemas to know what columns to seed.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "let-me-examine progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_now_ill_examine_progress() {
        let progress = "Now I'll examine the users_repo to understand `find_or_create_google_user` and the registration_disabled error path, plus the DB schema for users/identities to know exactly how to seed.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "now-i'll-examine progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_report_prep_progress() {
        let progress = "I'll produce a read-only report. Let me gather the static evidence by reading the SSOT files, route handlers, and checker scripts in parallel.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "report-prep progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_accepts_final_report_after_report_prep() {
        let final_report = "I have enough static evidence. Composing the report now.\n\n# BoardTask Report\n\ncommit_status: not-required\n\n## Findings\n- Google callback returns an auth code redirect.\n\n## Verification\n- read-only report only.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "I'll produce a read-only report. Let me gather the static evidence.",
                "2026-05-04T18:01:00Z",
            ),
            test_conversation_message(3, "assistant", final_report, "2026-05-04T18:02:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z"))
                .as_deref(),
            Some(final_report)
        );
    }

    #[test]
    fn provider_final_summary_rejects_acknowledged_reverify_progress() {
        let progress = "Acknowledged: this is a re-dispatch under new BoardTask `b731331d-b6c8-4e49-9f7b-239e2fe36cf9`. I will redo the static analysis fresh and not recall conclusions from that prior task as evidence. Let me re-verify the load-bearing facts directly.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "acknowledged re-verify progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_context_then_plan_progress() {
        let progress = "Now I have all the context I need. Let me lay out the plan briefly before writing code.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "context-then-plan progress must not be treated as durable final"
        );
    }

    /// V3 autopilot-runtime regression :: exact progress frame observed while
    /// supervising auth child `2b8b04fe-d8ec-4a5e-a9d9-6707dbd4d724` and
    /// wrapper `b1ba3cd8-fc14-400f-a4b7-6be7b92b9860`. Autopilot accepted
    /// this "complete picture / share insights / start executing" narration
    /// as durable final before the worker wrote SSOT/checker edits and commit
    /// `1fb19fe`. Keep it non-final.
    #[test]
    fn provider_final_summary_rejects_complete_picture_share_insights_progress() {
        let progress = "Now I have a complete picture. Let me share insights and start executing.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-05T00:03:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-05T00:08:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(
                &messages,
                "task-123",
                Some("2026-05-05T00:02:50Z")
            ),
            None,
            "complete-picture/share-insights/start-executing progress frame must not be selected as durable final"
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "complete-picture/share-insights/start-executing frame must classify as active progress"
        );
    }

    #[test]
    fn provider_final_summary_rejects_full_clarity_explanation_progress() {
        let progress = "Both checkers pass at baseline. Now I have full clarity. Let me explain the situation, then make the changes.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "full-clarity explanation progress must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_full_picture_planned_edits_progress() {
        let progress =
            "Working tree is clean. Now I have the full picture. Let me make the planned edits.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T23:10:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T23:11:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(
                &messages,
                "task-123",
                Some("2026-05-04T23:09:50Z")
            ),
            None,
            "observed full-picture/planned-edits progress frame must not be picked up as durable final"
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "full-picture planned-edits progress must classify as active progress"
        );
    }

    /// V3 autopilot-runtime regression :: pinned sentence from BoardTask
    /// `eb536f4c-63e4-4239-a915-f89eb36ce3f4`. The exact frame
    /// "Working tree is clean. Now I have the full picture. Let me make the
    /// planned edits." was falsely accepted as durable final by the M6 auth
    /// shard summary-close path. The string is held verbatim here so the
    /// V3 autopilot-runtime isomorphism checker can require the test name as
    /// part of the summary-close contract; renaming or removing this test
    /// MUST also update `scripts/check-v3-autopilot-runtime-isomorphism.mjs`.
    #[test]
    fn provider_final_summary_rejects_working_tree_clean_full_picture_edit_progress() {
        let progress =
            "Working tree is clean. Now I have the full picture. Let me make the planned edits.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T23:10:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T23:11:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(
                &messages,
                "task-123",
                Some("2026-05-04T23:09:50Z")
            ),
            None,
            "exact M6-observed working-tree-clean / full-picture / planned-edits frame must not be selected as durable final"
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "exact M6-observed working-tree-clean / full-picture / planned-edits frame must classify as active progress"
        );
    }

    /// V3 autopilot-runtime regression :: pinned frame from BoardTask
    /// `57302086-5a70-486c-a69c-6bd703bcaaf2` (M6 auth Google callback
    /// token-session shard). The worker was still explaining a structural
    /// blocker and announced more SSOT/report work, but Autopilot closed
    /// the wrapper before the worker committed and wrote the child summary.
    /// Keep this exact "I have enough context" blocker-planning shape
    /// non-final.
    #[test]
    fn provider_final_summary_rejects_blocker_planning_progress() {
        let progress = "I have enough context. The dispatch asks for a Google callback product-token-session migration shard, but my analysis shows it requires changes to files in `must_not_touch`. Let me explain the architectural constraint and then declare the blocker via SSOT updates.\n\nLet me capture current acceptance baseline first, then update SSOT to record the blocker.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T23:35:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T23:42:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T23:34:50Z")),
            None,
            "context/blocker/SSOT-update progress frame must not be selected as durable final"
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "context/blocker/SSOT-update progress frame must classify as active progress"
        );
    }

    /// V3 autopilot-runtime regression :: exact-text guard pinning the
    /// stand-alone `Let me capture current acceptance baseline first, then
    /// update SSOT to record the blocker.` frame observed in note 201f4715
    /// (wrapper 57302086-5a70-486c-a69c-6bd703bcaaf2). It must classify as
    /// blocker-planning progress, never as a durable final summary.
    #[test]
    fn provider_final_summary_rejects_blocker_planning_progress_capture_baseline() {
        let progress = "Let me capture current acceptance baseline first, then update SSOT to record the blocker.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T23:35:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T23:42:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(
                &messages,
                "task-123",
                Some("2026-05-04T23:34:50Z")
            ),
            None,
            "capture-baseline-then-update-SSOT progress frame must not be selected as durable final"
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "capture-baseline-then-update-SSOT progress must classify as active progress"
        );
    }

    #[test]
    fn provider_final_summary_rejects_insight_only_no_evidence() {
        let progress = "★ Insight ─────────────────────────────────────\nThe SSOT establishes a layered closure pattern: g1a (config seam) → g1b (AppState harness) → g1c (handler fixtures) → checker rewrite → success-path runtime. Each addendum narrows the remaining gap rather than rewriting history. This dispatch's job is the final piece: success-path coverage.\n─────────────────────────────────────────────────";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "insight-only explanation blocks must not be treated as durable final evidence"
        );
        assert!(!is_probably_active_tui_summary(
            "★ Insight\nVerification: cargo test passed.\nCommit hash: abc1234."
        ));
    }

    #[test]
    fn provider_final_summary_rejects_wakeup_retry_blocker() {
        let progress = "The wakeup will fire in 100s. I'll wait for that retry rather than poll.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T18:00:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T18:01:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T17:59:00Z")),
            None
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "future wakeup / retry progress must not be treated as durable final evidence"
        );
        assert!(is_probably_active_tui_summary(
            "ENOSPC: no space left on device. A wakeup is scheduled to retry."
        ));
    }

    #[test]
    fn worker_final_close_blocker_detects_commit_failures() {
        assert_eq!(
            worker_final_close_blocker("GPG pinentry was cancelled."),
            Some("gpg-pinentry")
        );
        assert_eq!(
            worker_final_close_blocker("The scoped commit failed after staging."),
            Some("commit-failed")
        );
        assert_eq!(
            worker_final_close_blocker("commit_status=not-required; read-only smoke."),
            None
        );
    }

    #[test]
    fn worker_final_close_blocker_detects_plan_mode_no_write() {
        assert_eq!(
            worker_final_close_blocker(
                "I am operating in Plan Mode and cannot directly modify the requested file."
            ),
            Some("plan-mode-no-write")
        );
        assert_eq!(
            worker_final_close_blocker(
                "The analysis is complete and no file mutation was required."
            ),
            None
        );
    }

    #[test]
    fn write_scope_close_requires_durable_provider_final() {
        let description = "\
Implement the shard.

## Swarm metadata
- write_policy: write
- write_scope: services/auth/.missiond/backend/auth.lisp
- acceptance: node scripts/check-service-ssot.mjs auth --json";
        assert_eq!(
            delegated_write_close_evidence_blocker(
                description,
                false,
                "Verification: check-service-ssot passed."
            ),
            Some("missing-durable-provider-final")
        );
    }

    #[test]
    fn write_scope_close_requires_acceptance_evidence() {
        let description = "\
Implement the shard.

## Swarm metadata
- write_policy: write
- write_scope: services/auth/.missiond/backend/auth.lisp
- acceptance: node scripts/check-service-ssot.mjs auth --json";
        assert_eq!(
            delegated_write_close_evidence_blocker(description, true, "Done."),
            Some("missing-acceptance-evidence")
        );
        assert_eq!(
            delegated_write_close_evidence_blocker(
                description,
                true,
                "Changed files: services/auth/.missiond/backend/auth.lisp\nVerification: node scripts/check-service-ssot.mjs auth --json passed."
            ),
            None
        );
        assert_eq!(
            delegated_write_close_evidence_blocker(
                description,
                true,
                "Both gates green. Run the JSON evidence-only gate to capture full evidence/structural snapshot, then verify git status shows only the intended changes."
            ),
            None
        );
        assert_eq!(
            delegated_write_close_evidence_blocker(
                description,
                true,
                "Clean — must-not-touch paths are untouched. Final M10 evidence-only gate confirmation:"
            ),
            None
        );
        assert_eq!(
            delegated_write_close_evidence_blocker(
                description,
                true,
                "Acceptance commands passed. check.sh passed and the M10 evidence-only gate passed."
            ),
            None
        );
    }

    #[test]
    fn read_only_close_does_not_require_write_evidence() {
        let description = "\
Review only.

## Swarm metadata
- write_policy: read-only
- write_scope: []
- must_not_touch: **/*";
        assert_eq!(
            delegated_write_close_evidence_blocker(description, false, "Findings complete."),
            None
        );
    }

    #[test]
    fn provider_final_summary_rejects_claude_tool_invocation_records() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-03T10:15:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "[Tool: Bash] command: \"git status --short\"",
                "2026-05-03T10:15:20Z",
            ),
            test_conversation_message(
                3,
                "assistant",
                "## Smoke Result\n\nAutopilot used the provider durable final summary.",
                "2026-05-03T10:15:40Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-03T10:14:50Z"))
                .as_deref(),
            Some("## Smoke Result\n\nAutopilot used the provider durable final summary.")
        );
    }

    #[test]
    fn provider_final_summary_rejects_intermediate_investigation_narration() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-03T10:15:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "The four checks already cover schema and they all pass. Let me peek at L1 intent and the evidence sidecar to confirm M6 maturity claims align with reality.",
                "2026-05-03T10:15:20Z",
            ),
            test_conversation_message(
                3,
                "assistant",
                "Only `.missiond/intent.lisp` is in my modification set. The other dirty files were pre-existing. Now committing only the intent.lisp.",
                "2026-05-03T10:15:30Z",
            ),
            test_conversation_message(
                4,
                "assistant",
                "M6 closure is already in place.\n\n## Verification Report\n- checker-first mapping passes\n- no edits needed",
                "2026-05-03T10:15:40Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-03T10:14:50Z"))
                .as_deref(),
            Some("M6 closure is already in place.\n\n## Verification Report\n- checker-first mapping passes\n- no edits needed")
        );
    }

    #[test]
    fn provider_final_summary_rejects_intermediate_write_narration() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T06:14:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "Now I have a complete picture. Let me write the surfaces file.",
                "2026-05-04T06:15:00Z",
            ),
            test_conversation_message(
                3,
                "assistant",
                "JARVIS_S5_SURFACES_DONE\n\nCommit: `7fbdd2a feat(jarvis): add M6 surface map`.",
                "2026-05-04T06:17:00Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T06:13:50Z"))
                .as_deref(),
            Some("JARVIS_S5_SURFACES_DONE\n\nCommit: `7fbdd2a feat(jarvis): add M6 surface map`.")
        );
    }

    #[test]
    fn provider_final_summary_rejects_intermediate_create_narration() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T07:04:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "Now I have enough evidence. Let me create the runtime topology dossier.",
                "2026-05-04T07:06:00Z",
            ),
            test_conversation_message(
                3,
                "assistant",
                "JARVIS_S6_RUNTIME_DONE\n\nCommit: `b4ceff8 feat(jarvis): add M6 runtime topology`.",
                "2026-05-04T07:09:00Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T07:03:50Z"))
                .as_deref(),
            Some("JARVIS_S6_RUNTIME_DONE\n\nCommit: `b4ceff8 feat(jarvis): add M6 runtime topology`.")
        );
    }

    #[test]
    fn provider_final_summary_rejects_intermediate_writing_narration() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T07:35:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "I have sufficient evidence. Writing the S9 policy dossier now.",
                "2026-05-04T07:39:00Z",
            ),
            test_conversation_message(
                3,
                "assistant",
                "JARVIS_S9_POLICY_DONE\n\nCommit: `c782034 feat(jarvis): add M6 policy dossier`.",
                "2026-05-04T07:42:00Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T07:34:50Z"))
                .as_deref(),
            Some("JARVIS_S9_POLICY_DONE\n\nCommit: `c782034 feat(jarvis): add M6 policy dossier`.")
        );
    }

    #[test]
    fn provider_final_summary_rejects_staging_and_committing_narration() {
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T06:56:00Z",
            ),
            test_conversation_message(
                2,
                "assistant",
                "File is untracked (no diff yet — expected). Staging and committing the single shard file.",
                "2026-05-04T06:58:00Z",
            ),
            test_conversation_message(
                3,
                "assistant",
                "JARVIS_S4_FLOWS_DONE\ncommit: `f885362`",
                "2026-05-04T06:59:00Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T06:55:50Z"))
                .as_deref(),
            Some("JARVIS_S4_FLOWS_DONE\ncommit: `f885362`")
        );
    }

    #[test]
    fn provider_final_summary_rejects_check_passed_then_append_report_progress() {
        let progress =
            "`git diff --check` is clean. Now let me append §13 to the convergence report.";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T14:20:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T14:21:00Z"),
            test_conversation_message(
                3,
                "assistant",
                "AUTH_M6_GOOGLE_BRIDGE_DONE\n\nCommit: `47d76fa feat(auth): bridge google callback to product-access reporter (M6)`.",
                "2026-05-04T14:24:00Z",
            ),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(&messages, "task-123", Some("2026-05-04T14:19:50Z"))
                .as_deref(),
            Some("AUTH_M6_GOOGLE_BRIDGE_DONE\n\nCommit: `47d76fa feat(auth): bridge google callback to product-access reporter (M6)`.")
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "report append progress after clean checks must not be treated as durable final"
        );
    }

    #[test]
    fn provider_final_summary_rejects_exact_git_diff_clean_then_append_progress() {
        // Regression guard for the exact worker frame observed in the wild
        // (BoardTask 8315b1b3): the worker reports a clean `git diff --check`
        // and, in the same line, announces more report-appending work. The
        // ellipsis tail and the lack of backticks around `git diff --check`
        // distinguish this from the earlier `…check passed then append…`
        // regression — both shapes must continue to be classified as
        // non-final progress so Autopilot does not close on a frame that
        // still carries pending mutation intent.
        let progress = "git diff --check is clean. Now let me append §13...";
        let messages = vec![
            test_conversation_message(
                1,
                "system",
                "BoardTask task-123 prompt",
                "2026-05-04T14:20:00Z",
            ),
            test_conversation_message(2, "assistant", progress, "2026-05-04T14:21:00Z"),
        ];
        assert_eq!(
            latest_assistant_after_task_prompt(
                &messages,
                "task-123",
                Some("2026-05-04T14:19:50Z")
            ),
            None,
            "exact observed `git diff --check is clean. Now let me append §13...` frame must not be picked up as the durable final"
        );
        assert!(
            is_probably_active_tui_summary(progress),
            "exact observed git-diff-clean-then-append frame must classify as active progress"
        );
    }

    #[test]
    fn durable_completion_summary_must_be_after_claim_when_known() {
        let before = test_note(
            missiond_core::types::BoardNoteType::Summary,
            "old summary",
            "2026-05-02T15:18:00Z",
        );
        let after = test_note(
            missiond_core::types::BoardNoteType::Summary,
            "new durable summary",
            "2026-05-02T15:19:50Z",
        );
        assert!(!has_durable_completion_summary_after_claim(
            &[before],
            Some("2026-05-02T15:18:41Z")
        ));
        assert!(has_durable_completion_summary_after_claim(
            &[after],
            Some("2026-05-02T15:18:41Z")
        ));
    }

    #[test]
    fn autopilot_final_settle_window_is_pinned_before_close() {
        // PTY completion is a high-confidence turn signal, but provider JSONL
        // and MissionD conversation ingestion may settle just after the TUI
        // returns. Keep a non-zero default and pin the close path to the helper
        // so a future refactor cannot close on PTY idle alone.
        assert!(
            AUTOPILOT_FINAL_SETTLE_WINDOW_MS_DEFAULT >= 5000,
            "default settle window must leave room for provider final evidence"
        );
        let src = include_str!("./autopilot.rs");
        assert!(
            src.contains("wait_for_worker_final_settle_window().await"),
            "Autopilot close path must wait for durable final evidence settle window"
        );
        assert!(
            src.contains("await_durable_provider_completion_for_slot_task("),
            "Autopilot close path must poll durable provider final evidence after settle"
        );
        assert!(
            src.contains("durable_provider_completion_for_slot_task("),
            "Autopilot close path must prefer durable provider final evidence"
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

    // ── Summary-note pollution: worker final summary extractor ─────────

    fn dispatched_prompt(task_id: &str) -> String {
        // Mirrors append_board_task_id_suffix's tail block so the extractor's
        // anchor strategy is exercised against a real-shaped prompt.
        append_board_task_id_suffix("Refactor the autopilot summary path", task_id)
    }

    #[test]
    fn extract_worker_final_summary_strips_echoed_boardtask_contract() {
        // The TUI screen capture starts with the echoed user paste (which
        // includes the `📋 Board Task ID` block ending in `负责关闭此 BoardTask。`)
        // followed by the worker's actual final answer. The extractor MUST
        // return only the worker's tail, never the echoed contract.
        let prompt = dispatched_prompt("task-abc");
        let response = format!(
            "{}\n\n执行摘要：\n- 已完成 sanitizer 实现\n- 单元测试 PASS",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("📋 **Board Task ID**"),
            "echoed task-id label leaked: {summary}"
        );
        assert!(
            !summary.contains("负责关闭此 BoardTask。"),
            "echoed task-contract tail leaked: {summary}"
        );
        assert!(
            summary.contains("已完成 sanitizer 实现"),
            "worker final answer must survive: {summary}"
        );
        assert!(
            summary.contains("单元测试 PASS"),
            "worker final answer must survive: {summary}"
        );
    }

    #[test]
    fn extract_worker_final_summary_strips_paste_collapse_marker() {
        // Claude Code TUI collapses long pastes into `[Pasted text +N lines,
        // paste again to expand]`. That marker must never reach the BoardTask
        // summary note.
        let prompt = dispatched_prompt("task-paste");
        let response = format!(
            "> [Pasted text #3 +120 lines, paste again to expand]\n\n{}\n\n最终摘要：完成。",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("paste again to expand"),
            "paste collapse marker leaked: {summary}"
        );
        assert!(
            !summary.contains("[Pasted text"),
            "pasted-text bracket leaked: {summary}"
        );
        assert!(summary.contains("最终摘要：完成。"));
    }

    #[test]
    fn extract_worker_final_summary_strips_tool_call_log_lines() {
        // Tool-call log lines (`●` legacy invocation, `⏺` current Claude Code
        // invocation, `⎿` result) are TUI artifacts — the worker's final
        // summary alone should reach the BoardTask note.
        let prompt = dispatched_prompt("task-tools");
        let response = format!(
            "{}\n\n● Read(file=\"foo.rs\")\n⎿ ok, 42 lines\n⏺ Bash(cargo test)\n⎿ test result: ok\n\n最终结论：通过。",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("● Read"),
            "tool invocation marker leaked: {summary}"
        );
        assert!(
            !summary.contains("⏺ Bash"),
            "current Claude Code tool invocation marker leaked: {summary}"
        );
        assert!(
            !summary.contains("⎿"),
            "tool result marker leaked: {summary}"
        );
        assert!(summary.contains("最终结论：通过。"));
    }

    #[test]
    fn extract_worker_final_summary_strips_bare_tool_call_log_lines() {
        let prompt = dispatched_prompt("task-bare-tool");
        let response = format!(
            "{}\n\nBash(git -C /Users/jinchen/Projects/missiond status --short && echo \"---\" && git -C /Users/jinchen/Projects/missiond\n      rev-parse --short HEAD)\n⏺ CLAUDE_DURABLE_FINAL_SMOKE_OK\n  Findings\n  Worktree unchanged.",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("Bash("),
            "bare tool invocation marker leaked: {summary}"
        );
        assert!(
            !summary.contains("rev-parse --short HEAD)"),
            "bare tool invocation continuation leaked: {summary}"
        );
        assert!(summary.contains("CLAUDE_DURABLE_FINAL_SMOKE_OK"));
        assert!(summary.contains("Worktree unchanged."));
    }

    #[test]
    fn extract_worker_final_summary_strips_user_echo_and_status_bar() {
        // The TUI sometimes re-renders the user's last paste as a `> ...` echo
        // line and shows a `⏵⏵` hint bar at the bottom.
        let prompt = dispatched_prompt("task-echo");
        let response = format!(
            "{}\n> please continue\n\n收尾：done.\n\n⏵⏵ accept edits on (shift+tab to cycle)",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(!summary.contains("> please continue"));
        assert!(!summary.contains("⏵⏵ accept edits"));
        assert!(summary.contains("收尾：done."));
    }

    #[test]
    fn extract_worker_final_summary_prefers_last_summary_block() {
        // Real Autopilot notes previously captured the entire screen after
        // the prompt echo: planning narration, edit hunks, repeated tool
        // headings, then the final Summary. The extractor should focus the
        // final block rather than merely deleting a few marker lines.
        let prompt = dispatched_prompt("task-summary-anchor");
        let legacy_bad_pair = format!("res.duration_ms, res.{}", "response");
        let response = format!(
            "{}\n\n⏺ Now let me look at the v3 blueprint and checker:\n\
             ⏺ Update(crates/missiond-daemon/src/engine/intent_engine/autopilot.rs)\n\
             1454 -                    {}\n\
             1455 +                    res.duration_ms, summary_for_note\n\
             ⏺ All acceptance commands pass. Now let me commit the owned files:\n\
             ⏺ Summary\n\n\
             - Implemented deterministic summary extraction.\n\
             - Tests and V3 checker pass.",
            prompt, legacy_bad_pair
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("Now let me"),
            "planning narration leaked: {summary}"
        );
        assert!(
            !summary.contains("res.duration_ms"),
            "edit hunk leaked: {summary}"
        );
        assert!(summary.contains("Implemented deterministic summary extraction."));
        assert!(summary.contains("Tests and V3 checker pass."));
    }

    #[test]
    fn extract_worker_final_summary_keeps_clean_worker_text_intact() {
        // No echo, no artifacts — extractor must be a near no-op (only
        // trimming surrounding whitespace).
        let prompt = dispatched_prompt("task-clean");
        let response = "  执行结果：所有断言通过。  \n";
        let summary = extract_worker_final_summary(response, &prompt);
        assert_eq!(summary, "执行结果：所有断言通过。");
    }

    #[test]
    fn extract_worker_final_summary_handles_empty_response() {
        let prompt = dispatched_prompt("task-empty");
        let summary = extract_worker_final_summary("", &prompt);
        assert_eq!(summary, "");
    }

    #[test]
    fn extract_worker_final_summary_uses_last_anchor_when_repeated() {
        // If the worker quoted the task contract earlier in its output, the
        // LAST occurrence of the anchor still marks the boundary between
        // echoed contract and worker tail.
        let prompt = dispatched_prompt("task-repeat");
        let response = format!(
            "echo-1\n{prompt}\nintermediate\n{prompt}\n最终：done.",
            prompt = prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(summary.starts_with("最终：done."));
        assert!(!summary.contains("intermediate"));
        assert!(!summary.contains("echo-1"));
    }

    #[test]
    fn extract_worker_final_summary_falls_back_to_label_anchor_when_paste_collapsed() {
        // When the paste was collapsed and only the label `📋 **Board Task ID**:`
        // line is visible (no `负责关闭此 BoardTask。` tail), the extractor must
        // skip the label line and still recover the worker tail.
        let response =
            "📋 **Board Task ID**: `task-collapsed`\n[Pasted text +200 lines, paste again to expand]\n\n执行总结：合格。";
        let prompt = dispatched_prompt("task-collapsed");
        let summary = extract_worker_final_summary(response, &prompt);
        assert!(
            !summary.contains("📋 **Board Task ID**"),
            "label anchor leaked: {summary}"
        );
        assert!(
            !summary.contains("paste again to expand"),
            "paste marker leaked: {summary}"
        );
        assert!(summary.contains("执行总结：合格。"));
    }

    #[test]
    fn extract_worker_final_summary_collapses_blank_runs() {
        // Multiple consecutive blank lines (left behind after artifact
        // stripping) MUST collapse to a single blank so the note is compact.
        let prompt = dispatched_prompt("task-blank");
        let response = format!("{}\n\n● Tool(x)\n\n\n\n\n⎿ ok\n\n\n\n最终：合格。", prompt);
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("\n\n\n"),
            "extractor must collapse blank runs: {summary:?}"
        );
        assert!(summary.contains("最终：合格。"));
    }

    #[test]
    fn extract_worker_final_summary_keeps_multi_section_final_summary() {
        // Real-shaped Claude Code final summary: the worker writes
        // `⏺ Summary` / `⏺ Diagnosis` / `⏺ Validation` as section headings
        // with bullets under each. Section labels MUST survive — the legacy
        // extractor stripped every `⏺` line, which truncated multi-section
        // finals down to the body of the first block.
        let prompt = dispatched_prompt("task-multi-section");
        let response = format!(
            "{prompt}\n\n\
             ⏺ Update(crates/missiond-daemon/src/engine/intent_engine/autopilot.rs)\n\
             ⎿ ok, 12 lines\n\n\
             ⏺ Summary\n\
             - Implemented deterministic summary extraction.\n\
             - Multi-section labels survive the BoardTask note.\n\n\
             ⏺ Diagnosis\n\
             - Root cause: legacy strip stripped every `⏺` line, including\n\
               section labels and brief one-line answers.\n\n\
             ⏺ Validation\n\
             - cargo test -p missiond-daemon autopilot PASS\n\
             - V3 isomorphism checker PASS",
            prompt = prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        // Tool-call invocation + result lines are still stripped.
        assert!(
            !summary.contains("⏺ Update("),
            "tool-call invocation leaked: {summary}"
        );
        assert!(
            !summary.contains("⎿ ok, 12 lines"),
            "tool-result marker leaked: {summary}"
        );
        // Section labels survive (they are NOT tool calls).
        assert!(
            summary.contains("⏺ Summary"),
            "Summary section label dropped: {summary}"
        );
        assert!(
            summary.contains("⏺ Diagnosis"),
            "Diagnosis section label dropped: {summary}"
        );
        assert!(
            summary.contains("⏺ Validation"),
            "Validation section label dropped: {summary}"
        );
        // Section bodies survive.
        assert!(summary.contains("deterministic summary extraction"));
        assert!(summary.contains("legacy strip stripped every"));
        assert!(summary.contains("V3 isomorphism checker PASS"));
    }

    #[test]
    fn extract_worker_final_summary_keeps_brief_single_line_answer() {
        // When the worker's final answer is a single bullet line like
        // `⏺ Done. Fix verified.`, the legacy extractor stripped the line
        // because it began with `⏺` and produced an empty note. Prose
        // `⏺` lines that are NOT shaped like `Ident(args)` MUST survive.
        let prompt = dispatched_prompt("task-brief");
        let response = format!("{}\n\n⏺ Done. Fix verified — cargo test PASS.", prompt);
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(!summary.is_empty(), "brief final answer truncated to empty");
        assert!(
            summary.contains("Done. Fix verified"),
            "brief final answer dropped: {summary}"
        );
        assert!(
            summary.contains("cargo test PASS"),
            "brief final answer dropped: {summary}"
        );
    }

    #[test]
    fn extract_worker_final_summary_keeps_inline_summary_heading_content() {
        // When the heading line carries inline summary content
        // (`⏺ Summary: implemented X`), the legacy focus discarded the
        // remainder of the heading line by skipping past the next newline.
        // The extractor MUST keep the inline content.
        let prompt = dispatched_prompt("task-inline-heading");
        let response = format!(
            "{}\n\n⏺ Summary: Implemented deterministic extraction; tests pass.",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            summary.contains("Implemented deterministic extraction"),
            "inline heading content dropped: {summary}"
        );
        assert!(
            summary.contains("tests pass"),
            "inline heading content dropped: {summary}"
        );
    }

    #[test]
    fn extract_worker_final_summary_keeps_prose_with_parenthetical_aside() {
        // Worker prose like `⏺ The fix is in autopilot.rs (line 234).` looks
        // superficially like it has parentheses, but it is NOT a tool call
        // signature. The strip rule only matches `⏺ Ident(...)`-shaped first
        // tokens, so this prose survives.
        let prompt = dispatched_prompt("task-paren-aside");
        let response = format!(
            "{}\n\n⏺ The fix is in autopilot.rs (line 234) and tests pass.",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            summary.contains("The fix is in autopilot.rs"),
            "prose with parenthetical aside dropped: {summary}"
        );
        assert!(summary.contains("tests pass"));
    }

    #[test]
    fn extract_worker_final_summary_strips_tool_calls_within_final_region() {
        // Inside the final region, leftover tool-call invocations and result
        // lines (e.g. a final `⏺ Bash(cargo test)` smoke check) MUST still
        // be stripped — only prose / section labels survive.
        let prompt = dispatched_prompt("task-mixed-region");
        let response = format!(
            "{}\n\n⏺ Summary\n\
             ⏺ Bash(cargo test)\n\
             ⎿ test result: ok\n\
             - Implemented X.\n\
             - Verified by cargo test.",
            prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("⏺ Bash("),
            "tool-call invocation in final region leaked: {summary}"
        );
        assert!(
            !summary.contains("⎿ test result"),
            "tool-result line in final region leaked: {summary}"
        );
        assert!(summary.contains("⏺ Summary"));
        assert!(summary.contains("Implemented X."));
        assert!(summary.contains("Verified by cargo test."));
    }

    #[test]
    fn extract_worker_final_summary_anchors_on_diagnostic_summary_closeout() {
        // BoardTask 353c1b59 regression: the worker did NOT use a `Summary`
        // heading. It wrote a multi-paragraph diagnostic block led in by
        // `All acceptance gates pass. Here's the diagnostic summary for the
        // BoardTask:` followed by `**Fix: ...**` / `**Root cause**:` /
        // `**Changes**` / `**Verification**:`. Without a closeout-phrase
        // anchor, the extractor fell through and captured the whole transcript
        // — `⏺ Now I'll edit ...` narration, surviving `+`/`-` diff hunk
        // lines, and intermediate prose. The closeout anchors must locate the
        // last lead-in and keep only the diagnostic block.
        let prompt = dispatched_prompt("task-353c1b59");
        // Compose the legacy bad pair at runtime to dodge the source-level
        // guard `autopilot_note_site_no_longer_passes_raw_res_response`.
        let legacy_bad_pair = format!("res.duration_ms, res.{}", "response");
        let response = format!(
            "{prompt}\n\n\
             ⏺ Now I'll edit `task_delegate.rs` to wire the gemini routing.\n\
             ⏺ Update(crates/missiond-daemon/src/handlers/compute/task_delegate.rs)\n\
             1454 -                    {bad}\n\
             1455 +                    res.duration_ms, summary_for_note\n\
             ⏺ Bash(cargo test -p missiond-daemon)\n\
             ⎿ test result: ok. 10 passed.\n\n\
             All acceptance gates pass. Here's the diagnostic summary for the BoardTask:\n\n\
             **Fix: research intent → V3 gemini researcher pool routing**\n\n\
             **Root cause**: task_delegate.rs mapped intent=research to template `researcher` ...\n\n\
             **Changes** (`9336a182`):\n\
             - Blueprint: added research-default model-profile.\n\
             - Runtime: registered research-default in spawn-arg map.\n\
             - task_delegate: prefer gemini researcher slot for research intent.\n\n\
             **Verification**:\n\
             - 10/10 task_delegate tests pass.\n\
             - V3 isomorphism checker PASS.",
            prompt = prompt,
            bad = legacy_bad_pair
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        // Pre-closeout transcript pollution must be gone.
        assert!(
            !summary.contains("Now I'll edit"),
            "⏺ Now I'll edit narration leaked: {summary}"
        );
        assert!(
            !summary.contains(&legacy_bad_pair),
            "diff hunk leaked: {summary}"
        );
        assert!(
            !summary.contains("res.duration_ms, summary_for_note"),
            "diff hunk leaked: {summary}"
        );
        assert!(
            !summary.contains("⎿ test result"),
            "tool-result marker leaked: {summary}"
        );
        // Closeout lead-in is preserved as a header for the diagnostic block.
        assert!(
            summary.contains("diagnostic summary for the BoardTask:"),
            "closeout phrase lead-in dropped: {summary}"
        );
        // The actual diagnostic body survives.
        assert!(
            summary.contains("**Fix: research intent"),
            "Fix block dropped: {summary}"
        );
        assert!(
            summary.contains("**Root cause**"),
            "Root cause block dropped: {summary}"
        );
        assert!(
            summary.contains("**Changes**"),
            "Changes block dropped: {summary}"
        );
        assert!(
            summary.contains("**Verification**"),
            "Verification block dropped: {summary}"
        );
        assert!(summary.contains("V3 isomorphism checker PASS"));
    }

    #[test]
    fn extract_worker_final_summary_anchors_on_all_acceptance_gates_pass() {
        // The shorter closeout phrase `All acceptance gates pass` MUST also
        // anchor the focus region when the worker omits the `diagnostic
        // summary for the BoardTask:` lead-in (e.g. when they end with just
        // a single closeout sentence followed by a tight Fix paragraph).
        let prompt = dispatched_prompt("task-acceptance-gate");
        let response = format!(
            "{prompt}\n\n\
             ⏺ Update(crates/foo.rs)\n\
             ⎿ ok, 3 lines\n\
             ⏺ Bash(cargo test)\n\
             ⎿ test result: ok\n\n\
             All acceptance gates pass.\n\n\
             Fix: tightened the regex so it no longer matches `bar` prefixes.\n\
             Verification: cargo test PASS.",
            prompt = prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            !summary.contains("⏺ Update("),
            "tool-call invocation leaked: {summary}"
        );
        assert!(
            !summary.contains("⎿ test result"),
            "tool-result marker leaked: {summary}"
        );
        assert!(
            summary.contains("All acceptance gates pass."),
            "closeout lead-in dropped: {summary}"
        );
        assert!(
            summary.contains("Fix: tightened the regex"),
            "Fix block dropped: {summary}"
        );
        assert!(summary.contains("Verification: cargo test PASS."));
    }

    #[test]
    fn extract_worker_final_summary_prefers_last_closeout_phrase() {
        // If the worker quoted a closeout phrase earlier in the transcript
        // (e.g. inside a `**Verification**` recap of an earlier task), the
        // LAST occurrence still wins so only the final block survives.
        let prompt = dispatched_prompt("task-repeat-closeout");
        let response = format!(
            "{prompt}\n\n\
             stale-1\n\
             All acceptance gates pass.\nFix: stale fix A.\n\n\
             stale-2\n\
             diagnostic summary for the BoardTask:\nFix: live fix B.\nVerification: ok.",
            prompt = prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(!summary.contains("stale-1"));
        assert!(!summary.contains("stale-2"));
        assert!(!summary.contains("stale fix A"));
        assert!(summary.contains("live fix B"));
        assert!(summary.contains("Verification: ok."));
    }

    #[test]
    fn extract_worker_final_summary_anchors_on_gemini_fix_verification_pair() {
        // BoardTask 7dbddf43 regression (live Gemini smoke 02e5da3f). The
        // gemini-cli worker did NOT emit a `Summary` heading and did NOT use
        // the `diagnostic summary for the BoardTask:` / `All acceptance gates
        // pass` lead-in. Its transcript opened with a `Researching ...`
        // status line, included tool-box drawings (╭...╰), then a Chinese
        // 诊断报告 bullets block, ending with a tight `Fix: N/A` /
        // `Verification: ...` pair. Without a Fix-Verification fallback the
        // extractor fell through to the whole stripped transcript, so the
        // BoardTask note captured the initial `Researching ...` line instead
        // of the closeout block.
        let prompt = dispatched_prompt("task-7dbddf43");
        let response = format!(
            "{prompt}\n\n\
             Researching MissionD Deployment: Performing live smoke test...\n\n\
             ╭─────────────────────────────────────────╮\n\
             │ gemini-cli :: research-default          │\n\
             ╰─────────────────────────────────────────╯\n\n\
             **诊断报告**\n\
             - 部署链路确认存活，Gemini 路由命中 research-default 槽位。\n\
             - Live smoke run 全程未触发降级，无 fallback 痕迹。\n\
             - V3 workstation pool 显示 researcher slot 持有该任务。\n\n\
             Fix: N/A — routing already deployed in a371d114; live smoke confirms behavior.\n\
             Verification: live Gemini reachable; researcher slot returned diagnostic block; no quota error.",
            prompt = prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        // The initial Gemini status line and tool-box drawings must NOT leak
        // into the BoardTask note.
        assert!(
            !summary.contains("Researching MissionD Deployment"),
            "Gemini status line leaked: {summary}"
        );
        assert!(
            !summary.contains("Performing live smoke test"),
            "Gemini status line tail leaked: {summary}"
        );
        assert!(
            !summary.contains('╭'),
            "tool-box top edge leaked: {summary}"
        );
        assert!(
            !summary.contains('╰'),
            "tool-box bottom edge leaked: {summary}"
        );
        assert!(
            !summary.contains("gemini-cli :: research-default"),
            "tool-box body line leaked: {summary}"
        );
        // The Fix + Verification closeout pair survives.
        assert!(summary.contains("Fix: N/A"), "Fix block dropped: {summary}");
        assert!(
            summary.contains("routing already deployed in a371d114"),
            "Fix body dropped: {summary}"
        );
        assert!(
            summary.contains("Verification: live Gemini reachable"),
            "Verification block dropped: {summary}"
        );
    }

    #[test]
    fn extract_worker_final_summary_anchors_on_gemini_bullet_fix_verification_pair() {
        // BoardTask 9aeb14b6 regression: Gemini can render the final answer as
        // `✦ Fix:` on the same line as its assistant bullet. Treat that bullet
        // as TUI prose chrome so the final BoardTask note does not keep the
        // earlier status sentence.
        let prompt = dispatched_prompt("task-9aeb14b6");
        let response = format!(
            "{prompt}\n\n\
             只读冒烟测试验证: 执行只读冒烟测试，验证部署后的状态。\n\n\
             ╭─────────────────────────────────────────╮\n\
             │ ✓  Shell git status --short             │\n\
             ╰─────────────────────────────────────────╯\n\n\
             ✦ Fix: This was a read-only smoke of MissionD Autopilot/PTY completion capture.\n\
               Verification: Current commit is 03fe34ac and only pre-existing packages/board/src/App.tsx is dirty.\n\n\
             YOLO Ctrl+Y                                                                               1 GEMINI.md file · 12 skills\n\
             *   Type your message or @path/to/file\n\
             workspace (/directory)               branch              sandbox                  /model                         quota\n\
             ~/Projects/missiond                  main                no sandbox               Auto (Gemini 3)              3% used",
            prompt = prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);

        assert!(
            !summary.contains("只读冒烟测试验证"),
            "early Gemini status sentence leaked: {summary}"
        );
        assert!(
            summary.contains(
                "✦ Fix: This was a read-only smoke of MissionD Autopilot/PTY completion capture."
            ),
            "Gemini bullet Fix line dropped: {summary}"
        );
        assert!(
            summary.contains("Verification: Current commit is 03fe34ac"),
            "Verification line dropped: {summary}"
        );
        assert!(
            !summary.contains("YOLO Ctrl+Y")
                && !summary.contains("Type your message")
                && !summary.contains("workspace (/directory)")
                && !summary.contains("Auto (Gemini 3)"),
            "Gemini footer lines leaked: {summary}"
        );
    }

    #[test]
    fn extract_worker_final_summary_trims_board_summary_tail_after_closeout() {
        // BoardTask 1600de56 regression: Gemini obeyed the requested concise
        // Fix/Verification closeout, then appended the generic BoardTask
        // diagnostic-summary block from the dispatch suffix. The Board note
        // should keep the closeout pair and drop the second summary block.
        let prompt = dispatched_prompt("task-1600de56");
        let response = format!(
            "{prompt}\n\n\
             Fix: read-only smoke of MissionD Autopilot/PTY completion capture\n\
               Verification: current commit is 182c0f7f and only pre-existing packages/board/src/App.tsx is dirty.\n\n\
               ---\n\
               任 务 诊 断 摘 要  (Board Task Summary)\n\
               已 完 成 对 MissionD Autopilot/PTY 完 成 捕 获 的 最 终 冒 烟 检 查 。",
            prompt = prompt
        );

        let summary = extract_worker_final_summary(&response, &prompt);
        assert_eq!(
            summary,
            "Fix: read-only smoke of MissionD Autopilot/PTY completion capture\nVerification: current commit is 182c0f7f and only pre-existing packages/board/src/App.tsx is dirty."
        );
        assert!(!summary.contains("Board Task Summary"));
        assert!(!summary.contains("任 务 诊 断 摘 要"));
    }

    #[test]
    fn extract_worker_final_summary_strips_gemini_progress_frames() {
        let prompt = dispatched_prompt("task-progress-frame");
        let response = format!(
            "{prompt}\n\n\
             ⠋ Thinking... (esc to cancel, 0s)                                                                      ? for shortcuts\n\
             ⠸ Defining the Scope (esc to cancel, 10s)                                                              ? for shortcuts\n\
             ⠴ Confirming the Closeout (esc to cancel, 12s)                                                         ? for shortcuts",
            prompt = prompt
        );

        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(summary.is_empty(), "progress frame leaked: {summary:?}");
        assert!(is_probably_active_tui_summary(&summary));
    }

    #[test]
    fn extract_worker_final_summary_truncate_safe_pairs_at_char_boundary() {
        // The note site applies `truncate_safe(&summary, MAX)`; the extractor
        // must return a UTF-8 string so multibyte boundaries survive the cap.
        let prompt = dispatched_prompt("task-utf");
        // 1000 chars of CJK ≈ 3000 bytes — exceeds the 4000-byte cap with
        // remaining headroom but still triggers truncation when the prompt
        // header + repeats are added below.
        let body = "测试".repeat(2500);
        let response = format!("{}\n\n{}", prompt, body);
        let summary = extract_worker_final_summary(&response, &prompt);
        let capped = truncate_safe(&summary, AUTOPILOT_SUMMARY_NOTE_MAX_BYTES);
        // Capped output MUST be valid UTF-8 (slice on a char boundary).
        assert!(capped.is_char_boundary(0));
        assert!(capped.is_char_boundary(capped.len()));
        assert!(capped.len() <= AUTOPILOT_SUMMARY_NOTE_MAX_BYTES);
    }

    #[test]
    fn autopilot_note_site_no_longer_passes_raw_res_response() {
        // Source-level guard: the `**Autopilot 执行完成**` note literal MUST
        // be paired with a sanitized summary, never `res.response`. This
        // prevents a refactor from accidentally re-introducing the legacy
        // pollution path. The needles are composed at runtime so the guard
        // does not trip on its own assertion text.
        let src = include_str!("./autopilot.rs");
        let banner = "**Autopilot \u{6267}\u{884c}\u{5b8c}\u{6210}** ({}ms{})";
        let raw_pair = format!("res.duration_ms, res.{}", "response");
        assert!(
            src.contains(banner),
            "summary-note banner must remain present"
        );
        assert!(
            !src.contains(&raw_pair),
            "autopilot.rs must not pass raw res.response into the summary-note format string; \
             use extract_worker_final_summary + truncate_safe instead"
        );
    }

    #[test]
    fn delegated_execution_id_extracts_preopened_log_id() {
        let prompt = "Execution log: `plan-abc-123`\n\n## Completion handoff";
        assert_eq!(
            extract_delegated_execution_id(prompt).as_deref(),
            Some("plan-abc-123")
        );
    }

    #[test]
    fn delegated_execution_id_extracts_completion_handoff_id() {
        let prompt =
            "call `mission_execution(action=complete, execution_id=\"plan-def-456\")` with args";
        assert_eq!(
            extract_delegated_execution_id(prompt).as_deref(),
            Some("plan-def-456")
        );
    }

    #[test]
    fn delegated_execution_id_rejects_non_plan_or_whitespace_ids() {
        assert_eq!(
            extract_delegated_execution_id("Execution log: `exec-1`"),
            None
        );
        assert_eq!(
            extract_delegated_execution_id("Execution log: `plan-has space`"),
            None
        );
    }

    // ── Dynamic slot stale-pin recovery ────────────────────────────────

    #[test]
    fn dynamic_slot_id_detection_is_prefix_based() {
        assert!(is_dynamic_slot_id("slot-dyn-abc123"));
        assert!(!is_dynamic_slot_id("slot-coder"));
        assert!(!is_dynamic_slot_id("coder-dyn-abc123"));
    }

    // ── Concurrent slot dispatch — pure invariant guard ─────────────────

    #[test]
    fn owned_dispatch_guard_allows_concurrent_different_slots() {
        // The legacy borrow-shaped SlotAcquireGuard tied the lock lifetime
        // to `&AppState`, which prevented dispatch_board_tasks from holding
        // a guard while moving it into a detached tokio send-task. The
        // owned shape MUST allow two different slots to hold a guard at the
        // same time so different-slot pty.send calls can start concurrently
        // within a single dispatch tick.
        let dispatch = Arc::new(SlotDispatchGuard::new());
        let g1 = OwnedSlotDispatchGuard::try_acquire(&dispatch, "slot-1");
        let g2 = OwnedSlotDispatchGuard::try_acquire(&dispatch, "slot-2");
        assert!(g1.is_some(), "slot-1 must acquire");
        assert!(g2.is_some(), "slot-2 must acquire while slot-1 is held");
    }

    #[test]
    fn owned_dispatch_guard_preserves_same_slot_exclusion() {
        // Same-slot work MUST remain exclusive across the entire send +
        // post-send tail. Acquiring twice on the same slot id must fail
        // until the first guard is dropped.
        let dispatch = Arc::new(SlotDispatchGuard::new());
        let g1 = OwnedSlotDispatchGuard::try_acquire(&dispatch, "slot-coder").expect("acquire");
        let g2 = OwnedSlotDispatchGuard::try_acquire(&dispatch, "slot-coder");
        assert!(
            g2.is_none(),
            "same-slot double-acquire must fail while first guard is held"
        );
        drop(g1);
        let g3 = OwnedSlotDispatchGuard::try_acquire(&dispatch, "slot-coder");
        assert!(g3.is_some(), "after drop, same slot must be re-acquirable");
    }

    #[test]
    fn dispatch_board_tasks_detaches_send_tail_without_joinset_drain() {
        // Source-level guard: the implementation MUST stop awaiting one
        // slot's pty.send before scheduling another slot's send, and it must
        // not drain a JoinSet inside the dispatch tick. The detached send
        // tail lets pre-provisioned dynamic slots that become idle after the
        // first tick get picked up by later ticks while earlier workers keep
        // running. The needles are composed at runtime so the guard cannot
        // trip on its own assertion text.
        let src = include_str!("./autopilot.rs");
        let detached_spawn_call = format!("tokio::{}(async move", "spawn");
        let drain_call = format!("send_jobs.{}().await", "join_next");
        let join_set_decl = format!("{}::JoinSet<()>", "tokio::task");
        assert!(
            src.contains(&detached_spawn_call),
            "dispatch_board_tasks must detach each pty.send + post-send tail"
        );
        assert!(
            !src.contains(&drain_call),
            "dispatch_board_tasks must not wait for worker completion via JoinSet drain"
        );
        assert!(
            !src.contains(&join_set_decl),
            "dispatch_board_tasks must not declare a JoinSet that aborts or drains long-running send tails"
        );
    }

    #[test]
    fn dispatch_board_tasks_unclaims_when_pty_not_ready() {
        let src = include_str!("./autopilot.rs");
        let ensure_call = "if !ensure_autopilot_pty(state, &task, &slot_id, task_env).await";
        let unclaim_call = "state.store.unclaim_board_task(task.id.as_str()).await";
        assert!(
            src.contains(ensure_call),
            "dispatch must check ensure_autopilot_pty before sending"
        );
        assert!(
            src.contains(unclaim_call),
            "a claimed BoardTask must be released when PTY spawn/readiness is transiently unavailable"
        );
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

    // ── Dispatch metadata projection (BoardTask 2b685fcf — review routing) ──

    fn make_board_task(
        title: &str,
        description: &str,
        category: &str,
        context_intent: Option<&str>,
    ) -> missiond_core::types::BoardTask {
        missiond_core::types::BoardTask {
            id: missiond_core::types::TaskId::from_trusted("task-test".to_string()),
            title: title.to_string(),
            description: description.to_string(),
            status: missiond_core::types::BoardTaskStatus::Open,
            priority: "medium".to_string(),
            category: category.to_string(),
            project: None,
            server: None,
            due_date: None,
            parent_id: None,
            assignee: None,
            auto_execute: false,
            prompt_template: None,
            hidden: false,
            retry_count: 0,
            max_retries: 2,
            order_idx: 0,
            created_at: String::new(),
            updated_at: String::new(),
            claim_executor_id: None,
            claim_executor_type: None,
            claimed_at: None,
            flow_phase: None,
            flow_context: None,
            flow_template: None,
            depends_on: Vec::new(),
            lease_expires_at: None,
            dedupe_key: None,
            timeout_secs: None,
            context_intent: context_intent.map(str::to_string),
            trigger_source: None,
            notes_count: 0,
        }
    }

    #[test]
    fn lisp_code_sync_runtime_report_task_is_stale_before_dispatch() {
        let task = make_board_task(
            "Sync code for Lisp change: runtime/lisp-code-sync/20260508T053538Z-f84cd729.report.lisp",
            "The changed path is .missiond/v3/runtime/lisp-code-sync/20260508T053538Z-f84cd729.report.lisp",
            "dev",
            Some("code"),
        );
        assert!(
            is_stale_lisp_code_sync_runtime_report_task(&task),
            "runtime lisp-code-sync report tasks must be resolved before slot dispatch"
        );
    }

    #[test]
    fn lisp_code_sync_real_authoring_task_is_not_stale() {
        let task = make_board_task(
            "Sync code for Lisp change: .missiond/v3/missiond-blueprint.lisp",
            "A real authoring SSOT file changed and may require code-isomorphism work.",
            "dev",
            Some("code"),
        );
        assert!(
            !is_stale_lisp_code_sync_runtime_report_task(&task),
            "active SSOT authoring changes must still be eligible for dispatch"
        );
    }

    /// Pins BoardTask 2b685fcf finding (1): externally-created BoardTasks
    /// that ship a `## Dispatch metadata` block with `task_class: review`
    /// must route to the `review` workstation pool — not silently fall back
    /// to `code` because `context_intent` was left at the "general" default.
    #[test]
    fn workstation_class_picks_up_review_from_dispatch_metadata() {
        let description = "Read /Users/jinchen/Projects/...\n\n\
             ## Dispatch metadata\n\
             - task_class: review\n\
             - pool_hint: claude-code-default\n\
             - engine_hint: claude-code\n\
             - acceptance: read SSOT | git status proves no edits";
        let task = make_board_task("Auth KB cleanup eval", description, "dev", Some("general"));
        assert_eq!(board_task_workstation_class(&task), "review");
    }

    /// Same projection works when `context_intent` is missing entirely
    /// (older BoardTasks pre-context-intent migration).
    #[test]
    fn workstation_class_picks_up_review_when_context_intent_missing() {
        let description = "## Dispatch metadata\n- task_class: review\n";
        let task = make_board_task("Eval", description, "dev", None);
        assert_eq!(board_task_workstation_class(&task), "review");
    }

    /// `## Swarm metadata` blocks (mission_swarm_run output) must be honored
    /// the same way as `## Dispatch metadata`.
    #[test]
    fn workstation_class_picks_up_class_from_swarm_metadata_block() {
        let description = "objective\n\n## Swarm metadata\n- task_class: context-pack\n";
        let task = make_board_task("Survey", description, "dev", None);
        assert_eq!(board_task_workstation_class(&task), "context-pack");
    }

    /// An unknown task_class value must NOT coerce routing — fall back to
    /// the title/description heuristic instead.
    #[test]
    fn workstation_class_ignores_unknown_dispatch_metadata_class() {
        let description = "## Dispatch metadata\n- task_class: not-a-real-class\n";
        let task = make_board_task("Investigate something", description, "dev", None);
        // "investigate" keyword in title → research fallback applies.
        assert_eq!(board_task_workstation_class(&task), "research");
    }

    /// Sanity: explicit `context_intent` still wins. The metadata-block scan
    /// only kicks in when context_intent is the default "general"/missing.
    #[test]
    fn workstation_class_explicit_context_intent_overrides_metadata_block() {
        let description = "## Dispatch metadata\n- task_class: review\n";
        let task = make_board_task("Eval", description, "dev", Some("code"));
        assert_eq!(board_task_workstation_class(&task), "code");
    }

    #[test]
    fn extract_dispatch_metadata_field_finds_value_under_dispatch_block() {
        let description =
            "blah\n## Dispatch metadata\n- task_class: review\n- engine_hint: claude-code\n";
        assert_eq!(
            extract_dispatch_metadata_field(description, "task_class"),
            Some("review".to_string())
        );
        assert_eq!(
            extract_dispatch_metadata_field(description, "engine_hint"),
            Some("claude-code".to_string())
        );
        assert_eq!(
            extract_dispatch_metadata_field(description, "pool_hint"),
            None
        );
    }

    #[test]
    fn extract_dispatch_metadata_field_drops_empty_values() {
        let description = "## Dispatch metadata\n- task_class:\n- engine_hint:   \n";
        assert!(extract_dispatch_metadata_field(description, "task_class").is_none());
        assert!(extract_dispatch_metadata_field(description, "engine_hint").is_none());
    }

    #[test]
    fn explicit_dispatch_hints_are_hard_constraints_when_worker_exists() {
        let cfg = WorkstationRuntimeConfig::default();
        let mut candidates = cfg.boardtask_pool_candidates("review");
        assert!(
            candidates
                .iter()
                .any(|worker| worker.id == "gemini-ultra-pro"),
            "review baseline should include Gemini before hint filtering"
        );
        let matching: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|worker| {
                workstation_worker_matches_dispatch_hints(
                    worker,
                    Some("claude-code"),
                    Some("claude-code-default"),
                )
            })
            .collect();
        assert_eq!(
            matching
                .iter()
                .map(|worker| worker.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-code-default"]
        );
        if !matching.is_empty() {
            candidates = matching;
        }
        assert!(
            candidates
                .iter()
                .all(|worker| worker.engine == "claude-code"),
            "explicit Claude hints must not leave Gemini as a fallback candidate"
        );
    }

    #[test]
    fn explicit_dispatch_hints_search_full_pool_before_task_class_fallback() {
        let cfg = WorkstationRuntimeConfig::default();
        let class_candidates = cfg.boardtask_pool_candidates("research");
        assert!(
            !class_candidates
                .iter()
                .any(|worker| worker.id == "claude-code-default"),
            "research baseline should prefer read-only lanes before explicit hints"
        );
        let matching: Vec<_> = cfg
            .workstation_pool()
            .iter()
            .filter(|worker| {
                worker.accepts_boardtask
                    && workstation_worker_matches_dispatch_hints(
                        worker,
                        Some("claude-code"),
                        Some("claude-code-default"),
                    )
            })
            .collect();
        assert_eq!(
            matching
                .iter()
                .map(|worker| worker.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-code-default"],
            "explicit engine/pool hints must be resolved against the full V3 pool"
        );
    }

    #[test]
    fn engine_hint_alone_does_not_widen_code_class_to_fast_patch() {
        let cfg = WorkstationRuntimeConfig::default();
        let candidates = cfg.boardtask_pool_candidates("code");
        assert_eq!(
            candidates
                .iter()
                .map(|worker| worker.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-code-default"],
            "code class must start from the default Opus coding lane only"
        );

        let matching: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|worker| {
                workstation_worker_matches_dispatch_hints(worker, Some("claude-code"), None)
            })
            .collect();

        assert_eq!(
            matching
                .iter()
                .map(|worker| worker.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-code-default"],
            "engine_hint=claude-code alone must not pull claude-code-fast-patch into a complex code shard"
        );
    }

    /// Pins BoardTask 2b685fcf finding (4): long evaluation tasks anchor
    /// their final report on a markdown H1 like
    /// `# Auth KB Cleanup — READ-ONLY Evaluation Report`. The extractor
    /// MUST land on that H1 (preserving the heading) and drop earlier
    /// in-progress narration.
    #[test]
    fn extract_worker_final_summary_anchors_on_evaluation_report_h1() {
        let prompt = dispatched_prompt("task-eval-h1");
        let response = format!(
            "{prompt}\n\n\
             Now let me read the SSOT files...\n\
             ⏺ Read(intent.lisp)\n\
             ⎿ ok\n\n\
             # Auth KB Cleanup — READ-ONLY Evaluation Report\n\n\
             **SSOT verification**:\n\
             - Read intent.lisp ✓\n\
             - Canonical issuer = `https://auth.xiaojinpro.com` ✓\n\n\
             ## A. Superseded-by-Lisp candidates\n- entry one\n- entry two\n",
            prompt = prompt
        );
        let summary = extract_worker_final_summary(&response, &prompt);
        assert!(
            summary.contains("Auth KB Cleanup"),
            "report H1 dropped: {summary}"
        );
        assert!(
            summary.contains("auth.xiaojinpro.com"),
            "report body dropped: {summary}"
        );
        assert!(
            !summary.contains("Now let me read"),
            "earlier narration leaked: {summary}"
        );
    }

    /// Pins BoardTask 2b685fcf finding (5): the dispatch suffix must
    /// instruct workers to print a repo label BEFORE multi-repo git output,
    /// and use `git -C <path>` instead of `cd` between repos.
    #[test]
    fn append_board_task_id_suffix_includes_multi_repo_git_advisory() {
        let suffix = append_board_task_id_suffix("BODY", "task-multi-repo-evidence");
        assert!(
            suffix.contains("多仓库 git status 输出规范"),
            "multi-repo advisory missing: {suffix}"
        );
        assert!(
            suffix.contains("git -C") || suffix.contains("并行 Bash 调用"),
            "advisory must offer a `cd`-free alternative: {suffix}"
        );
        assert!(
            suffix.contains("===<repo-name>==="),
            "advisory must show the label-before-output format: {suffix}"
        );
    }

    // ── Slot-attribution invariants (BoardTask 31e5449c regression) ─────

    fn make_running_task_claimed_by(
        task_id: &str,
        slot_id: &str,
    ) -> missiond_core::types::BoardTask {
        let mut task = make_board_task(
            "running task",
            "## Dispatch metadata\n- task_class: code",
            "dev",
            None,
        );
        task.id = missiond_core::types::TaskId::from_trusted(task_id.to_string());
        task.status = missiond_core::types::BoardTaskStatus::Running;
        task.claim_executor_id = Some(slot_id.to_string());
        task.claim_executor_type = Some("pty_slot".to_string());
        task.claimed_at = Some("2026-05-05T00:00:00Z".to_string());
        task
    }

    /// Pin V3 slot-attribution :: stale claim must be detected so a new
    /// dispatch on the same slot displaces it. Repro: BoardTask
    /// 31e5449c-e315-4003-ad59-c3eebd5eb837 saw `task 738c96f5 and 5599b07a
    /// running` on slot-claude-code-default because the previous task left
    /// its claim live.
    #[test]
    fn stale_running_claim_for_slot_matches_other_running_pty_slot_task() {
        let other = make_running_task_claimed_by("5599b07a", "slot-claude-code-default");
        assert!(is_stale_running_claim_for_slot(
            &other,
            "slot-claude-code-default",
            "738c96f5"
        ));
    }

    /// The newly-incoming dispatch's own task id must NOT be reported as
    /// stale — that would unclaim the dispatch we're about to start.
    #[test]
    fn stale_running_claim_for_slot_excludes_incoming_task() {
        let incoming = make_running_task_claimed_by("738c96f5", "slot-claude-code-default");
        assert!(!is_stale_running_claim_for_slot(
            &incoming,
            "slot-claude-code-default",
            "738c96f5"
        ));
    }

    /// A task in done/open/blocked status is not "running on this slot" even
    /// if the claim_executor still points at the slot — only running rows
    /// matter for the single-running-task-per-slot invariant.
    #[test]
    fn stale_running_claim_for_slot_ignores_non_running_status() {
        let mut task = make_running_task_claimed_by("done-task", "slot-claude-code-default");
        task.status = missiond_core::types::BoardTaskStatus::Done;
        assert!(!is_stale_running_claim_for_slot(
            &task,
            "slot-claude-code-default",
            "738c96f5"
        ));
        task.status = missiond_core::types::BoardTaskStatus::Open;
        assert!(!is_stale_running_claim_for_slot(
            &task,
            "slot-claude-code-default",
            "738c96f5"
        ));
        task.status = missiond_core::types::BoardTaskStatus::Blocked;
        assert!(!is_stale_running_claim_for_slot(
            &task,
            "slot-claude-code-default",
            "738c96f5"
        ));
    }

    /// A claim on a different slot must not be touched — only this slot's
    /// dispatch is the authority for this slot's attribution.
    #[test]
    fn stale_running_claim_for_slot_does_not_touch_other_slots() {
        let other_slot = make_running_task_claimed_by("other-task", "slot-other");
        assert!(!is_stale_running_claim_for_slot(
            &other_slot,
            "slot-claude-code-default",
            "738c96f5"
        ));
    }

    /// An assignee-only claim (assignee=slot, claim_executor=None) is queued,
    /// not running on this slot — leave it alone so queued tasks don't get
    /// silently unclaimed during another task's dispatch.
    #[test]
    fn stale_running_claim_for_slot_excludes_assignee_only_attribution() {
        let mut queued = make_board_task(
            "queued task",
            "## Dispatch metadata\n- task_class: code",
            "dev",
            None,
        );
        queued.id = missiond_core::types::TaskId::from_trusted("queued".to_string());
        queued.status = missiond_core::types::BoardTaskStatus::Running;
        queued.assignee = Some("slot-claude-code-default".to_string());
        queued.claim_executor_id = None;
        queued.claim_executor_type = None;
        assert!(!is_stale_running_claim_for_slot(
            &queued,
            "slot-claude-code-default",
            "738c96f5"
        ));
    }

    // ── PTY-only close gate (Defect 2 regression) ─────────────────────────

    fn delegated_dispatch_description() -> &'static str {
        "Audit something\n\n## Dispatch metadata\n- task_class: code\n- write_policy: read-only\n"
    }

    fn delegated_swarm_description() -> &'static str {
        "Survey shards\n\n## Swarm metadata\n- task_class: context-pack\n- write_policy: read-only\n"
    }

    fn delegated_context_pack_with_output_contract() -> &'static str {
        "Survey shards\n\n## Dispatch metadata\n- task_class: context-pack\n- write_policy: read-only\n- output_contract: return a structured artifact with Findings / Evidence / Recommendations / Verification\n"
    }

    /// Durable provider final available → never block, regardless of summary.
    #[test]
    fn pty_only_close_blocker_passes_when_durable_final_present() {
        assert_eq!(
            pty_only_close_blocker(
                delegated_dispatch_description(),
                /* has_durable_provider_final */ true,
                "anything",
            ),
            None
        );
    }

    /// Non-delegated tasks (no Dispatch/Swarm metadata) — nothing to gate.
    /// Chat-style ad-hoc BoardTasks do not have a structured artifact contract.
    #[test]
    fn pty_only_close_blocker_passes_for_non_delegated_tasks() {
        assert_eq!(
            pty_only_close_blocker(
                "freeform task description",
                /* has_durable_provider_final */ false,
                "OK done",
            ),
            None
        );
    }

    /// Delegated worker, no durable final, intermediate sentence in PTY → block.
    /// Mirrors the b5be6eed.../5599b07a.../a5ebf6c4... evidence.
    #[test]
    fn pty_only_close_blocker_blocks_intermediate_sentence_for_delegated_task() {
        let summary = "Now I have a complete picture. Let me share insights.";
        assert_eq!(
            pty_only_close_blocker(
                delegated_dispatch_description(),
                /* has_durable_provider_final */ false,
                summary,
            ),
            Some("missing-pty-final-artifact")
        );
    }

    /// Delegated worker, no durable final, but the PTY summary already has a
    /// structured artifact heading (e.g. `Findings`/`Verification`) → close
    /// is allowed because the artifact is on screen.
    #[test]
    fn pty_only_close_blocker_passes_when_structured_artifact_present() {
        let summary = "## Findings\n- ok\n\n## Verification\n- git status clean";
        assert_eq!(
            pty_only_close_blocker(
                delegated_dispatch_description(),
                /* has_durable_provider_final */ false,
                summary,
            ),
            None
        );
    }

    /// `## Swarm metadata` is honored the same way as `## Dispatch metadata`.
    #[test]
    fn pty_only_close_blocker_blocks_swarm_dispatch_without_artifact() {
        assert_eq!(
            pty_only_close_blocker(
                delegated_swarm_description(),
                /* has_durable_provider_final */ false,
                "narrating",
            ),
            Some("missing-pty-final-artifact")
        );
    }

    /// Existing acceptance-evidence markers (e.g. `verified`) also satisfy the
    /// structured-artifact check — preserves the prior write-scope-task path.
    #[test]
    fn pty_summary_with_acceptance_evidence_marker_passes() {
        assert!(pty_summary_has_structured_artifact(
            "All acceptance gates passed; verification done."
        ));
    }

    /// Durable provider summaries from a reused long-lived session can contain
    /// an older task's acceptance text. When the worker prompt asks for an
    /// explicit Findings/Evidence/Recommendations/Verification artifact, that
    /// stale acceptance text must not close the new task.
    #[test]
    fn output_contract_close_blocker_rejects_stale_structured_summary() {
        let stale = "## Changed files\n- services/deploy-center/.missiond/m10-convergence.lisp\n\n## Acceptance\n- M10 gate passed";
        assert_eq!(
            output_contract_close_blocker(delegated_context_pack_with_output_contract(), stale),
            Some("missing-output-contract-sections")
        );
    }

    #[test]
    fn output_contract_close_blocker_accepts_declared_sections() {
        let report = "# Context-Pack\n\n## Findings\n- one\n\n## Evidence\n- two\n\n## Recommendations\n- three\n\n## Verification\n- no edits";
        assert_eq!(
            output_contract_close_blocker(delegated_context_pack_with_output_contract(), report),
            None
        );
    }

    #[test]
    fn output_contract_close_blocker_accepts_memory_review_artifact_sections() {
        let report = "## Findings\n- Count reviewed: 7\n- Count selected for active memory: 0\n\nActive Memory Candidates\nNone\n\nSSOT-Workflow Backfill Candidates\nNone\n\nNeeds Human\nNone\n\nDiscard Rationale\nThe batch is procedural noise.\n\nVerification\nI only read the assigned batch files.";
        assert_eq!(
            output_contract_close_blocker(delegated_context_pack_with_output_contract(), report),
            None
        );
    }

    /// `is_delegated_worker_description` only fires on the V3 envelope
    /// markers — chat-style descriptions stay out of the gate.
    #[test]
    fn delegated_worker_detector_ignores_chat_descriptions() {
        assert!(!is_delegated_worker_description("just a chat note"));
        assert!(is_delegated_worker_description(
            "objective\n\n## Dispatch metadata\n- task_class: code"
        ));
        assert!(is_delegated_worker_description(
            "objective\n\n## Swarm metadata\n- target_projects: a=...\n"
        ));
    }
}

/// Scale-to-zero: release persistent slots that have been idle > IDLE_TIMEOUT.
/// The slot will be auto-respawned by ClaudeCodeSlotMgr::execute_persistent
/// when the next task arrives (lazy-spawn pattern).
async fn reap_idle_persistent_slots(state: &AppState, runtime_config: &AutopilotRuntimeConfig) {
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

        if idle_secs >= runtime_config.idle_persistent_slot_secs {
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
