//! Handler dispatch for MCP tool calls.
//! Routes tool names to domain-specific handler modules.
//!
//! Domain layout (Phase 3):
//!   knowledge/ — board, kb, skill, memory
//!   compute/   — pty, task, process, worker, cc_tasks, minimax, compute_slot
//!   comm/      — router_chat, question, conversation, timeline, audit, retrospective
//!   sysinfra/  — infra, permission, system, health, misc

mod comm;
mod compute;
mod knowledge;
mod sysinfra;

use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;

use crate::state::AppState;

// Re-export for external access (e.g. main.rs references retrospective::handle directly)
pub(crate) use comm::retrospective;

// Domain aliases for dispatch readability
use comm::{audit, codex_ops, conversation, question, router_chat, timeline};
use compute::{cc_tasks, compute_slot, flow_run, job, minimax, process, pty, task, task_delegate, worker};
use knowledge::{board, insight, intent, kb, memory, project, skill};
use sysinfra::{health, infra, misc, permission, system};

// @beacon: mcp
/// Dispatch a tool call to the appropriate handler module.
pub(crate) async fn dispatch_tool(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Consolidated tools (new names) =====
        "mission_task_submit" | "mission_task_query" | "mission_task_cancel" => {
            task::handle(state, name, args).await
        }
        "mission_agent" => process::handle(state, name, args).await,
        "mission_pty_spawn"
        | "mission_pty_send"
        | "mission_pty_read"
        | "mission_pty_signal"
        | "mission_pty_confirm"
        | "mission_pty_status"
        | "mission_pty_screenshot" => pty::handle(state, name, args).await,
        "mission_permission_query" | "mission_permission_mutate" => {
            permission::handle(state, name, args).await
        }
        "mission_cc_query" | "mission_cc_swarm" => cc_tasks::handle(state, name, args).await,
        "mission_skill_query"
        | "mission_skill_context"
        | "mission_skill_mutate"
        | "mission_skill_exec" => skill::handle(state, name, args).await,
        "mission_question"
        | "mission_decision_stats"
        | "mission_incident"
        | "mission_llm_trace"
        | "mission_gemini_auth" => question::handle(state, name, args).await,
        "mission_router_chat" | "mission_router_chat_manage" => {
            router_chat::handle(state, name, args).await
        }
        "mission_intent" => intent::handle(state, name, args).await,
        "mission_project" => project::handle(state, name, args).await,
        "mission_memory" => memory::handle(state, name, args).await,
        "mission_insight" => insight::handle(state, name, args).await,
        "mission_audit" => audit::handle(state, name, args).await,
        "mission_codex_ops" => codex_ops::handle(state, name, args).await,
        "mission_timeline" => timeline::handle(state, name, args).await,
        "mission_infra_query" | "mission_infra_ops" => infra::handle(state, name, args).await,
        "mission_worker" | "mission_control" => worker::handle(state, name, args).await,
        "mission_sys_logs" | "mission_sys_config" | "mission_daemon_update" => {
            system::handle(state, name, args).await
        }
        "mission_compute_slot" => compute_slot::handle(state, name, args).await,
        "mission_task_delegate" => task_delegate::handle(state, name, args).await,
        "mission_job_poll" => job::handle(state, name, args).await,
        "mission_flow_run" => flow_run::handle(state, name, args).await,

        // ===== Legacy names (backward compatibility) =====
        "mission_submit" | "mission_ask" | "mission_status" | "mission_cancel" | "mission_task"
        | "mission_task_ack" | "mission_task_track" => task::handle(state, name, args).await,
        "mission_spawn" | "mission_kill" | "mission_restart" | "mission_agents" => {
            process::handle(state, name, args).await
        }
        n if n.starts_with("mission_pty_") => pty::handle(state, n, args).await,
        n if n.starts_with("mission_permission_") => permission::handle(state, n, args).await,
        n if n.starts_with("mission_cc_") => cc_tasks::handle(state, n, args).await,
        n if n.starts_with("mission_skill_")
            || (n.starts_with("mission_context_") && n != "mission_context_around") =>
        {
            skill::handle(state, n, args).await
        }
        n if n.starts_with("mission_question_") => question::handle(state, n, args).await,
        n if n.starts_with("mission_router_chat") => router_chat::handle(state, n, args).await,
        n if n.starts_with("mission_memory_") => memory::handle(state, n, args).await,
        n if n.starts_with("mission_audit_") => audit::handle(state, n, args).await,
        n if n.starts_with("mission_timeline_") => timeline::handle(state, n, args).await,
        n if n.starts_with("mission_infra_")
            | (n == "mission_reachability")
            | (n == "mission_os_diagnose") =>
        {
            infra::handle(state, n, args).await
        }
        n if n.starts_with("mission_incident_")
            | (n == "mission_health")
            | (n == "mission_power_control") =>
        {
            health::handle(state, n, args).await
        }
        "mission_workers" | "mission_worker_control" => worker::handle(state, name, args).await,

        // ===== Unchanged tools =====
        n if n.starts_with("mission_kb_") => kb::handle(state, n, args).await,
        "mission_code_search" => kb::handle(state, name, args).await,
        n if n.starts_with("mission_beacon_") => kb::handle(state, n, args).await,
        n if n.starts_with("mission_board_") => board::handle(state, n, args).await,
        n if n.starts_with("mission_minimax_") || n.starts_with("mission_sonnet_") => {
            minimax::handle(state, n, args).await
        }
        n if n.starts_with("mission_conversation_")
            || n == "mission_agent_trajectory"
            || n == "mission_trigger_backfill"
            || n == "mission_embedding_stats"
            || n == "mission_embedding_ops"
            || n == "mission_habit_scan"
            || n == "mission_token_stats"
            || n == "mission_session_narrations"
            || n == "mission_activity_report"
            || n == "mission_message_search"
            || n == "mission_context_around"
            || n == "mission_user_message_index"
            || n == "mission_conversation_set_label"
            || n == "mission_conversation_delete_label" =>
        {
            conversation::handle(state, n, args).await
        }
        "mission_retrospective"
        | "mission_retrospective_list"
        | "mission_retrospective_backfill" => retrospective::handle(state, name, args).await,

        // ===== Misc (slots, inbox, etc.) =====
        "mission_slots"
        | "mission_inbox"
        | "mission_submit_phase_result"
        | "mission_slot_history"
        | "mission_jarvis_logs"
        | "mission_jarvis_trace"
        | "mission_gemini_trace"
        | "mission_gemini_stats"
        | "mission_gemini_content"
        | "mission_gemini_watch"
        | "mission_code_map_graph" => misc::handle(state, name, args).await,

        // ===== xjp proxy =====
        _ if name.starts_with("xjp_") => match state.xjp_mcp.call_tool(name, args).await {
            Ok(result) => Ok(result),
            Err(e) => {
                let mut res = ToolResult::text(format!("xjp-mcp proxy error: {}", e));
                res.is_error = Some(true);
                Ok(res)
            }
        },
        _ => Ok(ToolResult::structured_error(
            ToolError::new(error_codes::UNKNOWN_TOOL, format!("Unknown tool: {}", name))
                .with_suggestion(
                    "Use mission_slots to list available tools, or check tool name spelling",
                ),
        )),
    }
}
