//! Handler dispatch for MCP tool calls.
//! Routes tool names to domain-specific handler modules.
//!
//! Domain layout (Phase 3):
//!   knowledge/ — board, kb, skill, memory
//!   compute/   — pty, task, process, worker, cc_tasks, minimax, slot, compute_slot
//!   comm/      — router_chat, question, conversation, timeline, audit, retrospective
//!   sysinfra/  — infra, permission, power, system, health, misc

mod comm;
mod compute;
pub(crate) mod knowledge;
mod sysinfra;

use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;

use crate::state::AppState;

// Re-export for external access (e.g. main.rs references retrospective::handle directly)
pub(crate) use comm::retrospective;

// Domain aliases for dispatch readability
use comm::{
    audit, capability_usage, codex_ops, codex_replay, conversation, question, router_chat,
    timeline, tool_directory,
};
use compute::{
    cc_tasks, compute_slot, flow_run, forge, job, minimax, process, pty, slot, task, task_delegate,
    worker,
};
use knowledge::{
    agent_execution, board, cascade, context_gather, directive, insight, intent, kb, memory, plan,
    project, request, shared_memory, skill, workflow,
};
use sysinfra::{global_instruction, health, infra, misc, permission, power, system};

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
        "mission_context_boot" | "mission_context_gather" => {
            context_gather::handle(state, name, args).await
        }
        "mission_question"
        | "mission_decision_stats"
        | "mission_incident"
        | "mission_llm_trace"
        | "mission_gemini_auth" => question::handle(state, name, args).await,
        "mission_router_chat" | "mission_router_chat_manage" => {
            router_chat::handle(state, name, args).await
        }
        "mission_intent" => intent::handle(state, name, args).await,
        "mission_request" => request::handle(state, name, args).await,
        "mission_directive" => directive::handle(state, name, args).await,
        "mission_plan" => plan::handle(state, name, args).await,
        "mission_workflow" => workflow::handle(state, name, args).await,
        "mission_execution" => agent_execution::handle(state, name, args).await,
        "mission_capability_usage" => capability_usage::handle(state, name, args).await,
        "mission_tool_directory" => tool_directory::handle(state, name, args).await,
        "mission_project" => project::handle(state, name, args).await,
        "mission_shared_memory" | "mission_context_slice" | "mission_claim_status" => {
            shared_memory::handle(state, name, args).await
        }
        "mission_memory" => memory::handle(state, name, args).await,
        "mission_insight" => insight::handle(state, name, args).await,
        "mission_audit" => audit::handle(state, name, args).await,
        "mission_codex_ops" => codex_ops::handle(state, name, args).await,
        "mission_codex_replay" => codex_replay::handle(state, name, args).await,
        "mission_timeline" => timeline::handle(state, name, args).await,
        "mission_infra_query" | "mission_infra_ops" => infra::handle(state, name, args).await,
        "mission_worker" | "mission_control" => worker::handle(state, name, args).await,
        "mission_sys_logs" | "mission_sys_config" | "mission_daemon_update" => {
            system::handle(state, name, args).await
        }
        "mission_power_control" => power::handle(state, name, args).await,
        "mission_global_instruction" => global_instruction::handle(state, name, args).await,
        "mission_compute_slot" => compute_slot::handle(state, name, args).await,
        "mission_task_delegate" | "mission_swarm_run" => {
            task_delegate::handle(state, name, args).await
        }
        "mission_job_poll" => job::handle(state, name, args).await,
        "mission_flow_run" => flow_run::handle(state, name, args).await,
        "mission_forge_build" | "mission_forge_lint" => forge::handle(state, name, args).await,
        "mission_universe_graph"
        | "mission_cascade_plan"
        | "mission_cascade_trigger"
        | "mission_cascade_lint" => cascade::handle(state, name, args).await,

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
        n if n.starts_with("mission_incident_") => question::handle(state, n, args).await,
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
        n if n == "mission_health" => health::handle(state, n, args).await,
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
            || n == "mission_activity_report"
            || n == "mission_retrospective_manage"
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

        // ===== Compute runtime slot/process control =====
        "mission_slots"
        | "mission_inbox"
        | "mission_slot_history"
        | "mission_pause"
        | "mission_master_status"
        | "mission_convergence_status"
        | "mission_nightly_evolution" => slot::handle(state, name, args).await,

        // ===== Legacy LLM trace aliases now owned by comm/question =====
        "mission_jarvis_logs"
        | "mission_jarvis_trace"
        | "mission_gemini_trace"
        | "mission_gemini_stats"
        | "mission_gemini_content"
        | "mission_gemini_watch" => question::handle(state, name, args).await,

        // ===== Misc (legacy support adapters) =====
        "mission_submit_phase_result" | "mission_code_map_graph" => {
            misc::handle(state, name, args).await
        }

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
