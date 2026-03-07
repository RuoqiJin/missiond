//! Handler dispatch for MCP tool calls.
//! Routes tool names to domain-specific handler modules.

mod task;
mod process;
mod pty;
mod permission;
mod cc_tasks;
mod kb;
mod router_chat;
mod memory;
mod conversation;
mod audit;
mod board;
mod skill;
mod infra;
mod question;
mod misc;
mod health;
mod timeline;
mod minimax;

use anyhow::Result;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;

/// Dispatch a tool call to the appropriate handler module.
pub(crate) async fn dispatch_tool(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Prefix-based routing =====
        n if n.starts_with("mission_pty_") => pty::handle(state, n, args).await,
        n if n.starts_with("mission_permission_") => permission::handle(state, n, args).await,
        n if n.starts_with("mission_cc_") => cc_tasks::handle(state, n, args).await,
        n if n.starts_with("mission_kb_") => kb::handle(state, n, args).await,
        n if n.starts_with("mission_router_chat") => router_chat::handle(state, n, args).await,
        n if n.starts_with("mission_memory_") => memory::handle(state, n, args).await,
        n if n.starts_with("mission_board_") => board::handle(state, n, args).await,
        n if n.starts_with("mission_skill_") || n.starts_with("mission_context_") => skill::handle(state, n, args).await,
        n if n.starts_with("mission_question_") || n == "mission_decision_stats" => question::handle(state, n, args).await,
        n if n.starts_with("mission_conversation_") || n == "mission_agent_trajectory"
            || n == "mission_trigger_backfill" || n == "mission_embedding_stats"
            || n == "mission_token_stats" => conversation::handle(state, n, args).await,
        n if n.starts_with("mission_timeline_") => timeline::handle(state, n, args).await,
        n if n.starts_with("mission_minimax_") => minimax::handle(state, n, args).await,
        n if n.starts_with("mission_audit_") => audit::handle(state, n, args).await,
        n if n.starts_with("mission_infra_") || n == "mission_reachability"
            || n == "mission_os_diagnose" => infra::handle(state, n, args).await,
        n if n.starts_with("mission_incident_") || n == "mission_health"
            || n == "mission_power_control" => health::handle(state, n, args).await,

        // ===== Explicit name routing =====
        "mission_submit" | "mission_ask" | "mission_status" | "mission_cancel"
            | "mission_task" | "mission_task_ack" | "mission_task_track"
            => task::handle(state, name, args).await,
        "mission_spawn" | "mission_kill" | "mission_restart" | "mission_agents"
            => process::handle(state, name, args).await,
        "mission_slots" | "mission_inbox" | "mission_submit_phase_result"
            | "mission_slot_history" | "mission_jarvis_logs" | "mission_jarvis_trace"
            | "mission_gemini_trace" | "mission_gemini_stats" | "mission_gemini_content"
            => misc::handle(state, name, args).await,

        // ===== xjp proxy =====
        _ if name.starts_with("xjp_") => {
            match state.xjp_mcp.call_tool(name, args).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    let mut res = ToolResult::text(format!("xjp-mcp proxy error: {}", e));
                    res.is_error = Some(true);
                    Ok(res)
                }
            }
        }
        _ => {
            let mut res = ToolResult::text(format!("Unknown tool: {}", name));
            res.is_error = Some(true);
            Ok(res)
        }
    }
}
