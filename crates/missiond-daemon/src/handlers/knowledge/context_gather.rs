use anyhow::Result;
use missiond_mcp::tools::{ToolContent, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf};

use crate::handlers::comm::conversation;
use crate::handlers::sysinfra::infra;
use crate::state::AppState;

use super::{board, intent, kb, project, skill};

#[derive(Debug, Deserialize)]
struct ContextGatherArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default, alias = "infraTarget")]
    infra_target: Option<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    unknowns: Vec<String>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_kb: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_ssot: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_project: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_skill: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_infra: Option<bool>,
    #[serde(
        default,
        alias = "includeBoard",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_board: Option<bool>,
    #[serde(
        default,
        alias = "includeConversations",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_conversations: Option<bool>,
    #[serde(default, alias = "conversationTimeRange")]
    conversation_time_range: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Default, Deserialize)]
struct ContextBootArgs {
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default, alias = "taskId")]
    task_id: Option<String>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_capsule: Option<bool>,
}

const CODEX_BOOT_CONTEXT_REL: &str = ".missiond/v3/evidence/codex-boot-context.lisp";
const CODEX_BOOT_CONTEXT_FALLBACK: &str =
    include_str!("../../../../../.missiond/v3/evidence/codex-boot-context.lisp");

fn default_limit() -> usize {
    8
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_context_boot" {
        return handle_context_boot(args);
    }

    let args: ContextGatherArgs = serde_json::from_value(args)?;
    let query = normalized_query(&args);
    if query.is_empty() && args.project_id.is_none() && args.skill.is_none() {
        return Ok(ToolResult::error(
            "mission_context_gather requires query, project/project_id, skill, infra_target, or unknowns",
        ));
    }

    let limit = args.limit.clamp(1, 25);
    let mut sources = serde_json::Map::new();
    let mut diagnostics = Vec::new();

    if args.include_project.unwrap_or(true) {
        let payload = if let Some(project_id) = args.project_id.as_deref() {
            json!({"action": "get", "id": project_id})
        } else {
            json!({"action": "list"})
        };
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "project_registry",
            project::handle(state, "mission_project", payload).await,
        );
    }

    if args.include_ssot.unwrap_or(true) {
        let payload = if let Some(project_id) = args.project_id.as_deref() {
            json!({"action": "summary", "project": project_id})
        } else {
            json!({"action": "list"})
        };
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "ssot",
            intent::handle(state, "mission_intent", payload).await,
        );
    }

    if args.include_kb.unwrap_or(true) {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "kb",
            kb::handle(
                state,
                "mission_kb_query",
                json!({
                    "action": "search",
                    "query": query,
                    "project": args.project_id,
                    "limit": limit,
                    "include_archived": false
                }),
            )
            .await,
        );
    }

    if args.include_skill.unwrap_or(true) {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "skill_context",
            skill::handle(
                state,
                "mission_skill_context",
                json!({
                    "action": "resolve",
                    "query": query,
                    "project_id": args.project_id,
                    "skill": args.skill,
                    "include_kb": false,
                    "include_board": false
                }),
            )
            .await,
        );
    }

    if args.include_infra.unwrap_or(true) {
        let infra_payload = if let Some(target_id) = args.infra_target.as_deref() {
            json!({"action": "get", "id": target_id})
        } else {
            json!({"action": "skill_evidence", "skill": args.skill, "target_id": args.infra_target, "limit": limit})
        };
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "infra",
            infra::handle(state, "mission_infra_query", infra_payload).await,
        );
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "credential_refs",
            infra::handle(
                state,
                "mission_infra_query",
                json!({
                    "action": "credential_refs",
                    "target_id": args.infra_target,
                    "skill": args.skill,
                    "limit": limit
                }),
            )
            .await,
        );
    }

    if args.include_board.unwrap_or(true) {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "board_tasks",
            board::handle(
                state,
                "mission_board_query",
                json!({
                    "action": "search",
                    "query": query,
                    "project": args.project_id,
                    "scope": "active",
                    "limit": limit
                }),
            )
            .await,
        );
    }

    if args.include_conversations.unwrap_or(true) {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "conversation_logs",
            conversation::handle(
                state,
                "mission_conversation_query",
                json!({
                    "action": "search",
                    "query": query,
                    "project": args.project_id,
                    "limit": limit,
                    "time_range": args
                        .conversation_time_range
                        .as_deref()
                        .unwrap_or("last_30d"),
                    "query_mode": "hybrid"
                }),
            )
            .await,
        );
    }

    let evidence_refs = collect_evidence_refs(&sources);
    let unresolved = args
        .unknowns
        .iter()
        .filter(|unknown| !unknown.trim().is_empty())
        .map(|unknown| {
            json!({
                "unknown": unknown,
                "status": "needs_synthesis",
                "hint": "Review sources and promote to resolved/evidence_gap in the context-pack."
            })
        })
        .collect::<Vec<_>>();

    Ok(ToolResult::json_pretty(&json!({
        "ok": diagnostics.is_empty(),
        "schema": "missiond.context-gather.v1",
        "query": query,
        "project_id": args.project_id,
        "skill": args.skill,
        "infra_target": args.infra_target,
        "unknowns": args.unknowns,
        "sources": Value::Object(sources),
        "evidence_refs": evidence_refs,
        "unresolved": unresolved,
        "diagnostics": diagnostics,
        "next_action": "Synthesize grounded intent. If intent is confirmed, assign a plan-authoring worker to compile plan.lisp from the confirmed intent plus tool/resource inventory."
    })))
}

fn handle_context_boot(args: Value) -> Result<ToolResult> {
    let args: ContextBootArgs = serde_json::from_value(args).unwrap_or_default();
    let (capsule, source_path) = read_codex_boot_context();
    let include_capsule = args.include_capsule.unwrap_or(true);
    let capsule_len = capsule.chars().count();
    Ok(ToolResult::json_pretty(&json!({
        "ok": true,
        "schema": "missiond.codex-boot-context.v1",
        "project_id": args.project_id,
        "task_id": args.task_id,
        "source_path": source_path,
        "capsule_chars": capsule_len,
        "layers": ["L0-always-on", "L1-current-task", "L2-grounded-facts", "L3-cold-evidence"],
        "capsule": if include_capsule { Value::String(capsule) } else { Value::Null },
        "next_action": "Use this boot capsule as the collaboration protocol. For missing task/project facts, call mission_context_gather with explicit unknowns instead of preloading broad KB or logs."
    })))
}

fn read_codex_boot_context() -> (String, String) {
    for candidate in codex_boot_context_candidates() {
        if let Ok(text) = fs::read_to_string(&candidate) {
            return (text, candidate.display().to_string());
        }
    }
    (
        CODEX_BOOT_CONTEXT_FALLBACK.to_string(),
        "compiled-fallback:.missiond/v3/evidence/codex-boot-context.lisp".to_string(),
    )
}

fn codex_boot_context_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = env::var("MISSIOND_CODEX_BOOT_CONTEXT") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(root) = env::var("MISSIOND_PROJECT_ROOT") {
        candidates.push(PathBuf::from(root).join(CODEX_BOOT_CONTEXT_REL));
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join(CODEX_BOOT_CONTEXT_REL));
    }
    candidates.push(PathBuf::from("/Users/jinchen/Projects/missiond").join(CODEX_BOOT_CONTEXT_REL));
    candidates
}

fn normalized_query(args: &ContextGatherArgs) -> String {
    args.query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let joined = args
                .unknowns
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        })
        .or_else(|| args.project_id.clone())
        .or_else(|| args.skill.clone())
        .or_else(|| args.infra_target.clone())
        .unwrap_or_default()
}

fn insert_subcall(
    sources: &mut serde_json::Map<String, Value>,
    diagnostics: &mut Vec<Value>,
    key: &str,
    result: Result<ToolResult>,
) {
    match result {
        Ok(result) => {
            let is_error = result.is_error.unwrap_or(false);
            let value = tool_result_to_value(result);
            if is_error {
                diagnostics.push(json!({"source": key, "error": value}));
            } else {
                sources.insert(key.to_string(), value);
            }
        }
        Err(err) => diagnostics.push(json!({"source": key, "error": err.to_string()})),
    }
}

fn tool_result_to_value(result: ToolResult) -> Value {
    let Some(ToolContent::Text { text }) = result.content.into_iter().next() else {
        return Value::Null;
    };
    serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "text": text }))
}

fn collect_evidence_refs(sources: &serde_json::Map<String, Value>) -> Vec<Value> {
    let mut refs = Vec::new();
    collect_evidence_refs_inner(&Value::Object(sources.clone()), "$", &mut refs);
    refs.truncate(60);
    refs
}

fn collect_evidence_refs_inner(value: &Value, path: &str, refs: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            for key in [
                "path",
                "intentPath",
                "intent_path",
                "file_path",
                "source_path",
                "sourcePath",
                "source_file",
                "id",
                "key",
            ] {
                if let Some(v) = map.get(key).and_then(Value::as_str) {
                    if !v.is_empty() {
                        refs.push(json!({"source_path": path, "field": key, "value": v}));
                    }
                }
            }
            for (k, v) in map {
                collect_evidence_refs_inner(v, &format!("{path}.{k}"), refs);
                if refs.len() >= 60 {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                collect_evidence_refs_inner(item, &format!("{path}[{idx}]"), refs);
                if refs.len() >= 60 {
                    return;
                }
            }
        }
        _ => {}
    }
}
