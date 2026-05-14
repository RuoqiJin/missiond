//! MissionD MCP tool directory.
//!
//! This read-only surface gives agents a small set of primary tool families
//! before they choose among the compatibility tools exposed by the MCP server.

use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    action: String,
    intent: Option<String>,
    query: Option<String>,
    tool: Option<String>,
    family: Option<String>,
    #[serde(default)]
    include_compatibility: bool,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct ToolFamily {
    id: &'static str,
    title: &'static str,
    tier: &'static str,
    danger_level: &'static str,
    primary_surface: &'static str,
    use_when: &'static str,
    current_tools: &'static [&'static str],
    intent_examples: &'static [&'static str],
    keywords: &'static [&'static str],
}

const FAMILIES: &[ToolFamily] = &[
    ToolFamily {
        id: "mission_board",
        title: "Board and decisions",
        tier: "primary",
        danger_level: "low-write",
        primary_surface: "mission_board_* compatibility tools",
        use_when: "Create, inspect, update, close, claim, retry, or note BoardTasks and questions.",
        current_tools: &[
            "mission_board_create",
            "mission_board_query",
            "mission_board_update",
            "mission_board_note_add",
            "mission_question",
            "mission_decision_stats",
        ],
        intent_examples: &[
            "create task",
            "close BoardTask",
            "answer decision",
            "add summary note",
        ],
        keywords: &[
            "mission_board",
            "task",
            "decision",
            "question",
            "note",
            "close",
            "claim",
            "retry",
        ],
    },
    ToolFamily {
        id: "mission_workflow",
        title: "Workflow, delegation, and shards",
        tier: "primary",
        danger_level: "writes-through-workers",
        primary_surface: "mission_workflow + mission_swarm_run + mission_task_delegate",
        use_when: "Run workflows, prepare context packs, dispatch investigators or implementers, and enforce accepted exact shards.",
        current_tools: &[
            "mission_workflow",
            "mission_swarm_run",
            "mission_task_delegate",
            "mission_flow_run",
        ],
        intent_examples: &[
            "run M6 wave",
            "delegate exact shard",
            "inspect workflow contract",
        ],
        keywords: &[
            "workflow",
            "swarm",
            "delegate",
            "shard",
            "m6",
            "implement",
            "investigate",
            "context-pack",
        ],
    },
    ToolFamily {
        id: "mission_workstation",
        title: "Workstations, PTY, and slots",
        tier: "primary",
        danger_level: "medium-runtime",
        primary_surface: "mission_slots + mission_pty_* + mission_compute_slot",
        use_when: "Inspect or operate Codex/ClaudeCode/Gemini slots, PTYs, model profiles, and runtime state.",
        current_tools: &[
            "mission_slots",
            "mission_pty_status",
            "mission_pty_read",
            "mission_pty_send",
            "mission_compute_slot",
            "mission_master_status",
        ],
        intent_examples: &["show active slots", "read Claude PTY", "check master state"],
        keywords: &[
            "slot",
            "pty",
            "workstation",
            "claude",
            "gemini",
            "codex",
            "worker",
            "terminal",
        ],
    },
    ToolFamily {
        id: "mission_context",
        title: "Grounding, conversation, timeline, and task evidence",
        tier: "primary",
        danger_level: "read-mostly",
        primary_surface: "mission_context_gather + mission_conversation_* + mission_timeline + mission_audit",
        use_when: "Gather grounded context across KB, SSOT, project registry, skills, infra, Board task records, conversations, timeline events, task result evidence, and tool-call traces.",
        current_tools: &[
            "mission_context_gather",
            "mission_conversation_query",
            "mission_conversation_get",
            "mission_message_search",
            "mission_timeline",
            "mission_audit",
        ],
        intent_examples: &[
            "ground user intent",
            "find worker final",
            "inspect tool calls",
            "wait for event",
        ],
        keywords: &[
            "context",
            "gather",
            "grounding",
            "intent",
            "conversation",
            "log",
            "timeline",
            "event",
            "evidence",
            "artifact",
            "final",
            "audit",
        ],
    },
    ToolFamily {
        id: "mission_memory",
        title: "Memory and KB governance",
        tier: "primary",
        danger_level: "knowledge-write",
        primary_surface: "mission_memory + mission_kb_*",
        use_when: "Review, query, archive, or promote long-term knowledge and memory candidates.",
        current_tools: &[
            "mission_memory",
            "mission_kb_query",
            "mission_kb_add",
            "mission_kb_update",
        ],
        intent_examples: &["review memory", "query active KB", "mark superseded"],
        keywords: &[
            "memory",
            "kb",
            "knowledge",
            "remember",
            "archive",
            "superseded",
        ],
    },
    ToolFamily {
        id: "mission_universe",
        title: "Projects, universe, and infrastructure identity",
        tier: "primary",
        danger_level: "read-mostly",
        primary_surface: "mission_project + mission_infra_query + mission_universe_graph",
        use_when: "Resolve project identity, SSOT paths, runtime targets, skill evidence, and infra capability boundaries.",
        current_tools: &[
            "mission_project",
            "mission_infra_query",
            "mission_universe_graph",
            "mission_skill_query",
        ],
        intent_examples: &[
            "where is router",
            "list registered projects",
            "find 12900kf evidence",
        ],
        keywords: &[
            "project", "universe", "infra", "server", "runtime", "skill", "registry", "root",
        ],
    },
    ToolFamily {
        id: "mission_ops",
        title: "System operations and deployment diagnostics",
        tier: "primary",
        danger_level: "operator-action",
        primary_surface: "mission_sys_* + mission_infra_ops + mission_power_control",
        use_when: "Inspect daemon health, logs, permissions, updates, deployment events, and operator actions.",
        current_tools: &[
            "mission_sys_logs",
            "mission_sys_config",
            "mission_daemon_update",
            "mission_infra_ops",
            "mission_power_control",
        ],
        intent_examples: &[
            "check daemon logs",
            "inspect deploy event",
            "restart after approval",
        ],
        keywords: &[
            "ops",
            "deploy",
            "daemon",
            "health",
            "permission",
            "power",
            "update",
            "ci",
        ],
    },
    ToolFamily {
        id: "mission_router",
        title: "Model router, embedding, rerank, and chat",
        tier: "primary",
        danger_level: "external-call",
        primary_surface: "mission_router_chat + mission_embedding_ops",
        use_when: "Use or inspect XJP router chat, embedding, rerank, model/provider health, and related configuration.",
        current_tools: &[
            "mission_router_chat",
            "mission_router_chat_manage",
            "mission_embedding_ops",
            "mission_embedding_stats",
        ],
        intent_examples: &[
            "test embedding",
            "check rerank provider",
            "route model call",
        ],
        keywords: &[
            "mission_router",
            "model",
            "embedding",
            "embed",
            "rerank",
            "qwen",
            "chat",
            "provider",
        ],
    },
    ToolFamily {
        id: "mission_tool_directory",
        title: "Tool selection and capability directory",
        tier: "primary",
        danger_level: "read-only",
        primary_surface: "mission_tool_directory",
        use_when: "Choose the right MissionD tool family before calling lower-level compatibility tools.",
        current_tools: &[
            "mission_tool_directory",
            "mission_capability_usage",
            "mission_codex_ops",
        ],
        intent_examples: &[
            "which tool should I use",
            "lookup mission_board_query",
            "explain workflow family",
        ],
        keywords: &[
            "tool",
            "mcp",
            "directory",
            "capability",
            "which tool",
            "lookup",
        ],
    },
];

pub(crate) async fn handle(_state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let args: Args = serde_json::from_value(args)?;
    match args.action.as_str() {
        "list" => Ok(ToolResult::json_pretty(&json!({
            "schema": "missiond.tool-directory.v1",
            "families": FAMILIES.iter().map(|family| render_family(*family, args.include_compatibility)).collect::<Vec<_>>(),
            "compatibilityNote": "Family ids are the preferred selection layer; currentTools remain callable MCP tools for compatibility."
        }))),
        "recommend" => {
            let intent = args.intent.or(args.query).unwrap_or_default();
            let matches = recommend(&intent);
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.tool-directory.v1",
                "intent": intent,
                "recommendations": matches.iter().map(|family| render_family(*family, true)).collect::<Vec<_>>(),
                "fallback": "If no recommendation is precise enough, ask mission_tool_directory(action=\"list\") and use the lowest-danger family that owns the surface."
            })))
        }
        "lookup" => {
            let tool = args.tool.ok_or_else(|| anyhow!("lookup requires tool"))?;
            let family = lookup_tool(&tool);
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.tool-directory.v1",
                "tool": tool,
                "family": family.map(|family| render_family(family, true)),
                "found": family.is_some()
            })))
        }
        "explain" => {
            let family_id = args
                .family
                .ok_or_else(|| anyhow!("explain requires family"))?;
            let family =
                find_family(&family_id).ok_or_else(|| anyhow!("unknown family: {}", family_id))?;
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.tool-directory.v1",
                "family": render_family(family, args.include_compatibility),
                "agentRule": "Use this family first, then call the listed compatibility tools only for the specific operation."
            })))
        }
        "deprecated" => {
            if let Some(tool) = args.tool {
                let family = lookup_tool(&tool);
                return Ok(ToolResult::json_pretty(&json!({
                    "schema": "missiond.tool-directory.v1",
                    "tool": tool,
                    "preferredFamily": family.map(|family| family.id),
                    "preferredSurface": family.map(|family| family.primary_surface),
                    "status": if family.is_some() { "compatibility-tool" } else { "unknown" }
                })));
            }
            let limit = args.limit.unwrap_or(50);
            let tools = FAMILIES
                .iter()
                .flat_map(|family| {
                    family.current_tools.iter().map(move |tool| {
                        json!({
                            "tool": tool,
                            "preferredFamily": family.id,
                            "preferredSurface": family.primary_surface,
                        })
                    })
                })
                .take(limit)
                .collect::<Vec<_>>();
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.tool-directory.v1",
                "tools": tools,
                "limit": limit
            })))
        }
        other => Ok(ToolResult::error(format!("Unknown action: {other}"))),
    }
}

fn render_family(family: ToolFamily, include_compatibility: bool) -> Value {
    let mut value = json!({
        "id": family.id,
        "title": family.title,
        "tier": family.tier,
        "dangerLevel": family.danger_level,
        "primarySurface": family.primary_surface,
        "useWhen": family.use_when,
        "intentExamples": family.intent_examples,
    });
    if include_compatibility {
        value["currentTools"] = json!(family.current_tools);
    }
    value
}

fn recommend(intent: &str) -> Vec<ToolFamily> {
    let normalized = intent.to_lowercase();
    let mut matches = FAMILIES
        .iter()
        .filter(|family| {
            family
                .keywords
                .iter()
                .any(|keyword| normalized.contains(keyword))
        })
        .copied()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches.push(find_family("mission_tool_directory").expect("tool-directory family exists"));
    }
    matches
}

fn lookup_tool(tool: &str) -> Option<ToolFamily> {
    FAMILIES
        .iter()
        .find(|family| family.current_tools.contains(&tool))
        .copied()
}

fn find_family(id: &str) -> Option<ToolFamily> {
    let normalized = if id.starts_with("mission_") {
        id.to_string()
    } else {
        format!("mission_{id}")
    };
    FAMILIES
        .iter()
        .find(|family| family.id == id || family.id == normalized)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommend_board_from_task_intent() {
        let matches = recommend("close the current BoardTask after adding a note");
        assert_eq!(matches[0].id, "mission_board");
    }

    #[test]
    fn lookup_raw_board_tool() {
        let family = lookup_tool("mission_board_query").expect("tool should be mapped");
        assert_eq!(family.id, "mission_board");
    }

    #[test]
    fn recommend_router_from_embedding_intent() {
        let matches = recommend("test qwen embedding and rerank through router");
        assert!(matches.iter().any(|family| family.id == "mission_router"));
    }
}
