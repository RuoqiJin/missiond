//! MissionD MCP tool directory.
//!
//! This read-only surface gives agents a small set of primary tool families
//! before they choose among the compatibility tools exposed by the MCP server.

use std::fs;

use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::helpers::missiond_project_root;
use crate::state::AppState;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    action: String,
    intent: Option<String>,
    query: Option<String>,
    #[serde(default, alias = "entry_id")]
    entry_id: Option<String>,
    project: Option<String>,
    #[serde(default, alias = "project_id")]
    project_id: Option<String>,
    surface: Option<String>,
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

const AGENT_SLICES_REL: &str = ".missiond/v3/runtime/compiled/compiled-agent-slices.json";
const PROJECT_AGENT_NAVIGATION_REL: &str =
    ".missiond/v3/runtime/compiled/compiled-project-agent-navigation.json";

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
            "mission_pty_key",
            "mission_pty_text",
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
        "guide" => Ok(ToolResult::json_pretty(&guide_tool_directory(&args))),
        other => Ok(ToolResult::error(format!("Unknown action: {other}"))),
    }
}

fn guide_tool_directory(args: &Args) -> Value {
    guide_from_agent_slices(args, read_agent_slices())
}

fn guide_from_agent_slices(args: &Args, compiled: Result<Value>) -> Value {
    let intent = args
        .intent
        .clone()
        .or_else(|| args.query.clone())
        .unwrap_or_default();
    let project = guide_project(args);
    if project != "missiond" {
        return project_agent_navigation_guide(&project, &intent, read_project_agent_navigation());
    }
    let compiled = match compiled {
        Ok(value) => value,
        Err(err) => {
            return json!({
                "schema": "missiond.tool-directory-guide.v1",
                "ok": false,
                "diagnostic": {
                    "code": "AGENT_SLICES_UNAVAILABLE",
                    "message": format!("cannot read {AGENT_SLICES_REL}: {err}"),
                    "recovery": "Run node scripts/compile-v3-runtime.mjs --write, then retry mission_tool_directory(action=\"guide\")."
                },
                "project": project,
                "intent": intent,
                "fallbackRecommendations": recommend(&intent).iter().map(|family| render_family(*family, true)).collect::<Vec<_>>(),
            });
        }
    };
    let entries = compiled
        .pointer("/payload/entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let matches = guide_matches(&entries, args, &intent);
    let selected = matches.first().map(|(_, entry)| entry.clone());
    let alternatives = matches
        .iter()
        .skip(1)
        .take(2)
        .map(|(score, entry)| render_guide_summary(entry, *score))
        .collect::<Vec<_>>();
    match selected {
        Some(entry) => {
            let primary_family_id = string_field(&entry, "primaryFamily")
                .or_else(|| string_field(&entry, "primary_family"))
                .unwrap_or("mission_tool_directory");
            let read_first = entry.get("readFirst").cloned().unwrap_or_else(|| json!([]));
            let write_scope = entry
                .get("writeScope")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let must_not_touch = entry
                .get("mustNotTouch")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let checks = entry.get("checks").cloned().unwrap_or_else(|| json!([]));
            let authority_notes = entry
                .get("authorityNotes")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let source_refs = entry
                .get("sourceRefs")
                .cloned()
                .unwrap_or_else(|| json!([]));
            json!({
                "schema": "missiond.tool-directory-guide.v1",
                "ok": true,
                "project": project,
                "intent": intent,
                "entryId": args.entry_id.as_deref(),
                "surface": args.surface.as_deref(),
                "selectedEntry": entry.clone(),
                "alternatives": alternatives,
                "primaryFamily": find_family(primary_family_id).map(|family| render_family(family, true)),
                "readFirst": read_first,
                "writeScope": write_scope,
                "mustNotTouch": must_not_touch,
                "checks": checks,
                "authorityNotes": authority_notes,
                "sourceRefs": source_refs,
                "source": {
                    "agentSlices": AGENT_SLICES_REL,
                    "sourceHash": compiled.get("source_hash").cloned().unwrap_or(Value::Null),
                }
            })
        }
        None => json!({
            "schema": "missiond.tool-directory-guide.v1",
            "ok": false,
            "project": project,
            "diagnostic": {
                "code": "AGENT_ENTRY_NOT_FOUND",
                "message": "No agent entry matched entry_id, surface, or intent.",
                "recovery": "Call mission_tool_directory(action=\"guide\", entry_id=\"modify-semantic-ir-ssot\") for an exact card, or use action=\"recommend\" for family selection."
            },
            "intent": intent,
            "entryId": args.entry_id.as_deref(),
            "surface": args.surface.as_deref(),
            "fallbackRecommendations": recommend(&intent).iter().map(|family| render_family(*family, true)).collect::<Vec<_>>(),
        }),
    }
}

fn read_agent_slices() -> Result<Value> {
    let path = missiond_project_root().join(AGENT_SLICES_REL);
    let text = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn read_project_agent_navigation() -> Result<Value> {
    let path = missiond_project_root().join(PROJECT_AGENT_NAVIGATION_REL);
    let text = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn project_agent_navigation_guide(project: &str, intent: &str, compiled: Result<Value>) -> Value {
    match compiled {
        Ok(value) => {
            let selected = value
                .pointer("/payload/projects")
                .and_then(Value::as_array)
                .and_then(|projects| {
                    projects
                        .iter()
                        .find(|entry| {
                            entry.get("projectId").and_then(Value::as_str) == Some(project)
                        })
                        .cloned()
                });
            match selected {
                Some(entry) => json!({
                    "schema": "missiond.tool-directory-guide.v1",
                    "ok": true,
                    "project": project,
                    "intent": intent,
                    "selectedEntry": entry.clone(),
                    "alternatives": [],
                    "primaryFamily": find_family("mission_universe").map(|family| render_family(family, true)),
                    "readFirst": entry.get("readFirst").cloned().unwrap_or_else(|| json!([])),
                    "writeScope": entry.get("writeScope").cloned().unwrap_or_else(|| json!([])),
                    "mustNotTouch": entry.get("mustNotTouch").cloned().unwrap_or_else(|| json!([])),
                    "checks": entry.get("checks").cloned().unwrap_or_else(|| json!([])),
                    "authorityNotes": entry.get("authorityNotes").cloned().unwrap_or_else(|| json!([])),
                    "sourceRefs": entry.get("sourceRefs").cloned().unwrap_or_else(|| json!([])),
                    "coverageDiagnostic": {
                        "coverageState": entry.get("coverageState").cloned().unwrap_or(Value::Null),
                        "rule": "non-MissionD project navigation is read-only and suggestion-only"
                    },
                    "source": {
                        "projectAgentNavigation": PROJECT_AGENT_NAVIGATION_REL,
                        "sourceHash": value.get("source_hash").cloned().unwrap_or(Value::Null)
                    }
                }),
                None => json!({
                    "schema": "missiond.tool-directory-guide.v1",
                    "ok": false,
                    "project": project,
                    "intent": intent,
                    "diagnostic": {
                        "code": "PROJECT_AGENT_ENTRY_NOT_FOUND",
                        "message": "No registered project navigation card matched the requested project.",
                        "recovery": "Call mission_agent_navigation(action=\"suggest_entries\", project=...) or mission_project(action=\"list\")."
                    },
                    "fallbackRecommendations": recommend(intent).iter().map(|family| render_family(*family, true)).collect::<Vec<_>>(),
                }),
            }
        }
        Err(err) => json!({
            "schema": "missiond.tool-directory-guide.v1",
            "ok": false,
            "project": project,
            "intent": intent,
            "diagnostic": {
                "code": "PROJECT_AGENT_NAVIGATION_UNAVAILABLE",
                "message": format!("cannot read {PROJECT_AGENT_NAVIGATION_REL}: {err}"),
                "recovery": "Run node scripts/compile-v3-runtime.mjs --write."
            },
            "fallbackRecommendations": recommend(intent).iter().map(|family| render_family(*family, true)).collect::<Vec<_>>(),
        }),
    }
}

fn guide_project(args: &Args) -> String {
    args.project
        .as_deref()
        .or(args.project_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("missiond")
        .to_string()
}

fn guide_matches(entries: &[Value], args: &Args, intent: &str) -> Vec<(i64, Value)> {
    if let Some(entry_id) = args
        .entry_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let exact = entries
            .iter()
            .filter(|entry| string_field(entry, "id") == Some(entry_id))
            .cloned()
            .map(|entry| (10_000, entry))
            .collect::<Vec<_>>();
        if !exact.is_empty() {
            return exact;
        }
    }
    if let Some(surface) = args
        .surface
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let surface_matches = entries
            .iter()
            .filter(|entry| array_contains(entry.get("surfaces"), surface))
            .cloned()
            .map(|entry| (5_000, entry))
            .collect::<Vec<_>>();
        if !surface_matches.is_empty() {
            return surface_matches;
        }
    }
    let normalized = intent.to_lowercase();
    let mut scored = entries
        .iter()
        .filter_map(|entry| {
            let score = score_entry(entry, &normalized);
            (score > 0).then(|| (score, entry.clone()))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            string_field(&a.1, "id")
                .unwrap_or("")
                .cmp(string_field(&b.1, "id").unwrap_or(""))
        })
    });
    scored.truncate(3);
    scored
}

fn score_entry(entry: &Value, normalized_intent: &str) -> i64 {
    let mut score = 0;
    if let Some(id) = string_field(entry, "id") {
        if normalized_intent.contains(&id.to_lowercase()) {
            score += 100;
        }
    }
    if let Some(label) = string_field(entry, "label") {
        for token in label.to_lowercase().split(|ch: char| !ch.is_alphanumeric()) {
            if token.len() >= 4 && normalized_intent.contains(token) {
                score += 10;
            }
        }
    }
    for keyword in string_array(entry.get("intentKeywords")) {
        let keyword = keyword.to_lowercase();
        if normalized_intent.contains(&keyword) {
            score += 25 + keyword.len() as i64;
        }
    }
    for surface in string_array(entry.get("surfaces")) {
        if normalized_intent.contains(&surface.to_lowercase()) {
            score += 20;
        }
    }
    score
}

fn render_guide_summary(entry: &Value, score: i64) -> Value {
    json!({
        "id": string_field(entry, "id"),
        "label": string_field(entry, "label"),
        "primaryFamily": string_field(entry, "primaryFamily"),
        "surfaces": entry.get("surfaces").cloned().unwrap_or_else(|| json!([])),
        "score": score,
    })
}

fn string_field<'a>(entry: &'a Value, key: &str) -> Option<&'a str> {
    entry.get(key).and_then(Value::as_str)
}

fn string_array(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn array_contains(value: Option<&Value>, needle: &str) -> bool {
    string_array(value).iter().any(|item| *item == needle)
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

    #[test]
    fn guide_by_exact_entry_id() {
        let args = Args {
            action: "guide".to_string(),
            entry_id: Some("modify-plan-execution".to_string()),
            ..Args::default()
        };
        let value = guide_from_agent_slices(&args, Ok(test_agent_slices()));
        assert_eq!(
            value.pointer("/selectedEntry/id").and_then(Value::as_str),
            Some("modify-plan-execution")
        );
    }

    #[test]
    fn guide_by_intent_scores_plan_execution() {
        let args = Args {
            action: "guide".to_string(),
            intent: Some("change plan execution".to_string()),
            ..Args::default()
        };
        let value = guide_from_agent_slices(&args, Ok(test_agent_slices()));
        assert_eq!(
            value.pointer("/selectedEntry/id").and_then(Value::as_str),
            Some("modify-plan-execution")
        );
    }

    #[test]
    fn guide_by_intent_scores_autopilot_completion_over_board_backend() {
        let args = Args {
            action: "guide".to_string(),
            intent: Some("我要修改 autopilot 的 BoardTask 完成判定".to_string()),
            ..Args::default()
        };
        let value = guide_from_agent_slices(&args, Ok(test_agent_slices()));
        assert_eq!(
            value.pointer("/selectedEntry/id").and_then(Value::as_str),
            Some("modify-workstation-autopilot")
        );
    }

    #[test]
    fn guide_missing_agent_slices_returns_diagnostic() {
        let args = Args {
            action: "guide".to_string(),
            intent: Some("change plan execution".to_string()),
            ..Args::default()
        };
        let value = guide_from_agent_slices(&args, Err(anyhow!("missing compiled artifact")));
        assert_eq!(
            value.pointer("/diagnostic/code").and_then(Value::as_str),
            Some("AGENT_SLICES_UNAVAILABLE")
        );
        assert!(value
            .pointer("/fallbackRecommendations")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty()));
    }

    fn test_agent_slices() -> Value {
        json!({
            "schema_version": "missiond.compiled-agent-slices.v1",
            "source_hash": "test",
            "payload": {
                "entries": [
                    {
                        "id": "modify-board-backend",
                        "label": "Modify Board backend",
                        "primaryFamily": "mission_board",
                        "intentKeywords": ["board", "boardtask", "board backend"],
                        "surfaces": ["mission_board"],
                        "readFirst": [],
                        "writeScope": [],
                        "mustNotTouch": [],
                        "checks": [],
                        "authorityNotes": [],
                        "sourceRefs": []
                    },
                    {
                        "id": "modify-plan-execution",
                        "label": "Modify plan execution",
                        "primaryFamily": "mission_workflow",
                        "intentKeywords": ["plan execution", "execute plan"],
                        "surfaces": ["mission_plan"],
                        "readFirst": ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"],
                        "writeScope": ["crates/missiond-daemon/src/handlers/knowledge/plan/**"],
                        "mustNotTouch": ["workstation provider spawning"],
                        "checks": ["node scripts/check-v3-plan-execution-isomorphism.mjs"],
                        "authorityNotes": ["Navigation only"],
                        "sourceRefs": []
                    },
                    {
                        "id": "modify-workstation-autopilot",
                        "label": "Modify workstation autopilot",
                        "primaryFamily": "mission_workstation",
                        "intentKeywords": ["autopilot", "boardtask completion", "boardtask 完成", "完成判定", "worker dispatch"],
                        "surfaces": ["autopilot-runtime"],
                        "readFirst": [],
                        "writeScope": [],
                        "mustNotTouch": [],
                        "checks": [],
                        "authorityNotes": [],
                        "sourceRefs": []
                    }
                ]
            }
        })
    }
}
