//! Flow node handlers — delegates to existing MissionD infrastructure.
//!
//! Each NodeType variant maps to a reusable daemon primitive:
//! - LlmCall  → llm_gateway (call_gemini_for_flow / call_sonnet_stateless)
//! - SlotTask → slot_orchestrator (spawn_tracked_slot + pty.send)
//! - McpTool  → handlers::dispatch_tool
//! - DaemonAction → inline Rust logic

use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::info;

use super::{FlowContext, GatherStrategy, NodeType, ParallelTaskSpec};
use crate::state::AppState;

/// Execute a single flow node, returning the output string.
pub(crate) async fn execute_node(
    state: &AppState,
    node_type: &NodeType,
    ctx: &FlowContext,
    task_id: &str,
) -> Result<String> {
    match node_type {
        NodeType::LlmCall {
            provider,
            prompt,
            max_tokens,
        } => {
            let resolved = ctx.resolve_vars(prompt);
            execute_llm_call(state, provider, &resolved, *max_tokens, task_id).await
        }
        NodeType::SlotTask {
            model,
            prompt,
            timeout_secs,
        } => {
            let resolved = ctx.resolve_vars(prompt);
            execute_slot_task(state, model, &resolved, *timeout_secs, task_id).await
        }
        NodeType::McpTool { tool_name, params } => {
            let resolved: serde_json::Value =
                serde_json::from_str(&ctx.resolve_vars(&params.to_string()))
                    .unwrap_or_else(|_| params.clone());
            execute_mcp_tool(state, tool_name, resolved).await
        }
        NodeType::DaemonAction { action, params } => {
            let resolved: serde_json::Value =
                serde_json::from_str(&ctx.resolve_vars(&params.to_string()))
                    .unwrap_or_else(|_| params.clone());
            execute_daemon_action(state, action, resolved, task_id).await
        }
        NodeType::ParallelSlotTasks {
            parallelism,
            tasks,
            gather,
            timeout_secs,
        } => {
            // POC: fire-and-forget semantics — 返回派发回执而非 slot 真实执行结果。
            // 真实结果回流（wait for submit_phase_result + per-task timeout wrapping）见 Phase 2。
            let _ = timeout_secs; // Phase 2: wrap each spawn with tokio::time::timeout
            execute_parallel_slot_tasks(state, *parallelism, tasks, gather, ctx).await
        }
    }
}

// ── LlmCall ──────────────────────────────────────────────────────────

async fn execute_llm_call(
    state: &AppState,
    provider: &str,
    prompt: &str,
    max_tokens: u32,
    task_id: &str,
) -> Result<String> {
    info!(provider, prompt_len = prompt.len(), "Flow: LlmCall");
    match provider {
        "gemini" => crate::llm_gateway::call_gemini_for_flow(state, task_id, prompt).await,
        "sonnet" => {
            crate::llm_gateway::call_sonnet_stateless(
                state,
                "You are an architecture reviewer.",
                prompt,
                max_tokens,
                "flow_node",
            )
            .await
        }
        _ => Err(anyhow!("Unknown LLM provider: {}", provider)),
    }
}

// ── SlotTask ─────────────────────────────────────────────────────────

/// Roles excluded from dynamic slot selection (meta/system agents).
const EXCLUDED_ROLES: &[&str] = &["memory", "supervisor", "strategy"];

async fn execute_slot_task(
    state: &AppState,
    _model: &str,
    prompt: &str,
    timeout_secs: u64,
    _task_id: &str,
) -> Result<String> {
    let slot = state
        .mission
        .list_slots()
        .into_iter()
        .find(|s| !EXCLUDED_ROLES.contains(&s.config.role.as_str()))
        .ok_or_else(|| anyhow!("No eligible slot for SlotTask"))?;

    let slot_id = slot.config.id.clone();
    info!(slot_id = %slot_id, prompt_len = prompt.len(), "Flow: SlotTask");

    // Ensure slot is running
    let needs_spawn = match state.pty.get_status(&slot_id).await {
        Some(info) => info.state == missiond_core::SessionState::Exited,
        None => true,
    };

    if needs_spawn {
        return Err(anyhow!(
            "Slot '{}' is not running. Start it first via mission_pty_spawn or slots.yaml auto_start.",
            slot_id
        ));
    }

    // Send prompt
    state
        .pty
        .send_fire_and_forget(&slot_id, prompt)
        .await
        .map_err(|e| anyhow!("Failed to send to slot {}: {}", slot_id, e))?;

    Ok(format!(
        "SlotTask dispatched to {} (timeout={}s)",
        slot_id, timeout_secs
    ))
}

// ── ParallelSlotTasks ────────────────────────────────────────────────

/// POC: fire-and-forget fan-out to multiple running slots with bounded parallelism.
/// Round-robins `tasks` across eligible (non-excluded, Running) slots, caps concurrent
/// `send_fire_and_forget` in-flight calls at `effective = min(parallelism, candidates, tasks)`.
///
/// Returns a JSON array of per-task dispatch receipts (string or null) as the node output.
/// Real completion signals must arrive via submit_phase_result — wired up in Phase 2.
async fn execute_parallel_slot_tasks(
    state: &AppState,
    parallelism: usize,
    tasks: &[ParallelTaskSpec],
    gather: &GatherStrategy,
    ctx: &FlowContext,
) -> Result<String> {
    if tasks.is_empty() {
        return Err(anyhow!("ParallelSlotTasks: no tasks supplied"));
    }

    // 1. Select eligible slots: non-excluded role AND currently Running.
    let all_slots = state.mission.list_slots();
    let mut candidates = Vec::new();
    for slot in all_slots {
        if EXCLUDED_ROLES.contains(&slot.config.role.as_str()) {
            continue;
        }
        let running = match state.pty.get_status(&slot.config.id).await {
            Some(info) => info.state != missiond_core::SessionState::Exited,
            None => false,
        };
        if running {
            candidates.push(slot);
        }
    }

    if candidates.is_empty() {
        return Err(anyhow!(
            "ParallelSlotTasks: no running, non-excluded slots available for fan-out"
        ));
    }

    // 2. Effective parallelism = min(parallelism, candidates, tasks), clamped to >= 1.
    let effective = std::cmp::min(std::cmp::min(parallelism, candidates.len()), tasks.len()).max(1);

    info!(
        tasks = tasks.len(),
        slots = candidates.len(),
        parallelism = effective,
        "Flow: ParallelSlotTasks fan-out"
    );

    // 3. Spawn: bounded by semaphore, one permit per in-flight send.
    let sem = Arc::new(Semaphore::new(effective));
    let mut set: JoinSet<std::result::Result<(usize, String), (usize, String)>> = JoinSet::new();

    for (idx, task_spec) in tasks.iter().enumerate() {
        let permit_sem = sem.clone();
        let pty = state.pty.clone();
        let prompt = ctx.resolve_vars(&task_spec.prompt);
        let task_id = task_spec.id.clone();
        // Round-robin slot assignment: a slot may receive multiple prompts,
        // slot-internal sequencing serializes them inside the PTY session.
        let slot_id = candidates[idx % candidates.len()].config.id.clone();

        set.spawn(async move {
            let _permit = match permit_sem.acquire_owned().await {
                Ok(p) => p,
                Err(e) => return Err((idx, format!("semaphore closed: {}", e))),
            };
            match pty.send_fire_and_forget(&slot_id, &prompt).await {
                Ok(()) => Ok((
                    idx,
                    format!("dispatched: task={} -> slot={}", task_id, slot_id),
                )),
                Err(e) => Err((
                    idx,
                    format!("send failed: task={} slot={} err={}", task_id, slot_id, e),
                )),
            }
        });
    }

    // 4. Gather: preserve task order in output.
    let mut results: Vec<Option<String>> = vec![None; tasks.len()];
    let mut errors: Vec<String> = Vec::new();
    while let Some(join_res) = set.join_next().await {
        match join_res {
            Ok(Ok((idx, out))) => results[idx] = Some(out),
            Ok(Err((_idx, msg))) => errors.push(msg),
            Err(join_err) => errors.push(format!("join error: {}", join_err)),
        }
    }

    let json_results = serde_json::to_string(&results)?;

    match gather {
        GatherStrategy::Aggregate => Ok(json_results),
        GatherStrategy::AllSuccess => {
            if results.iter().all(|r| r.is_some()) {
                Ok(json_results)
            } else {
                Err(anyhow!(
                    "ParallelSlotTasks all_success: {} failure(s): {:?}",
                    errors.len(),
                    errors
                ))
            }
        }
        GatherStrategy::AnySuccess => {
            if results.iter().any(|r| r.is_some()) {
                Ok(json_results)
            } else {
                Err(anyhow!(
                    "ParallelSlotTasks any_success: all {} tasks failed: {:?}",
                    tasks.len(),
                    errors
                ))
            }
        }
    }
}

// ── McpTool ──────────────────────────────────────────────────────────

async fn execute_mcp_tool(
    state: &AppState,
    tool_name: &str,
    params: serde_json::Value,
) -> Result<String> {
    info!(tool = tool_name, "Flow: McpTool");
    let result = crate::handlers::dispatch_tool(state, tool_name, params).await?;
    let text = result
        .content
        .iter()
        .map(|c| match c {
            missiond_mcp::tools::ToolContent::Text { text } => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if result.is_error == Some(true) {
        Err(anyhow!("MCP tool '{}' failed: {}", tool_name, text))
    } else {
        Ok(text)
    }
}

// ── DaemonAction ─────────────────────────────────────────────────────

async fn execute_daemon_action(
    state: &AppState,
    action: &str,
    params: serde_json::Value,
    task_id: &str,
) -> Result<String> {
    info!(action, "Flow: DaemonAction");
    match action {
        "read_intent_lisp" => {
            let project = params
                .get("project")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("read_intent_lisp: 'project' required"))?;
            let args = serde_json::json!({ "action": "read", "project": project });
            let result = crate::handlers::dispatch_tool(state, "mission_intent", args).await?;
            extract_tool_text(&result)
        }
        "close_flow" => {
            crate::engine::control_plane_kernel::ControlPlaneKernel::new(state)
                .complete_system_task(
                    crate::engine::control_plane_kernel::SystemTaskCompletionInput {
                        task_id: task_id.to_string(),
                        project_id: None,
                        producer_id: "flow_daemon_action".to_string(),
                        summary: "Flow daemon action requested completion.".to_string(),
                        content: Some("Flow daemon action completed.".to_string()),
                        raw_evidence: serde_json::json!({
                            "kind": "flow_daemon_action",
                            "action": "close_flow",
                            "task_id": task_id
                        }),
                        evidence_refs: vec![serde_json::json!({
                            "kind": "flow_daemon_action",
                            "action": "close_flow"
                        })],
                        result_status: "completed".to_string(),
                        metadata: serde_json::json!({
                            "daemon_action": "close_flow"
                        }),
                    },
                )
                .await?;
            info!(task_id, "Flow: task closed");
            Ok("Flow completed".to_string())
        }
        _ => Err(anyhow!("Unknown daemon action: {}", action)),
    }
}

fn extract_tool_text(result: &missiond_mcp::tools::ToolResult) -> Result<String> {
    let text = result
        .content
        .iter()
        .map(|c| match c {
            missiond_mcp::tools::ToolContent::Text { text } => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if result.is_error == Some(true) {
        Err(anyhow!("{}", text))
    } else {
        Ok(text)
    }
}
