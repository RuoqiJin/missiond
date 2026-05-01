use crate::state::AppState;
use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use super::claim_lease::parse_claims;
use super::completion_durability::summarize_durability;
use super::completion_records::parse_completions;
use super::log_store::{
    parse_kv_pairs, project_or_target_project, read_log_file, resolve_project_root, COMPANION_DIR,
    LEGACY_COMPANION_DIR,
};

pub(super) async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let parent_filter = args.get("parent_design").and_then(|v| v.as_str());
    let status_filter = args.get("status").and_then(|v| v.as_str());
    let scope_prefix = args.get("scope_prefix").and_then(|v| v.as_str());
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(50)
        .clamp(1, 500) as usize;

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let mut summaries: Vec<Value> = Vec::new();
    let mut seen = HashSet::new();
    let mut saw_dir = false;
    for (dir_name, legacy) in [(COMPANION_DIR, false), (LEGACY_COMPANION_DIR, true)] {
        let dir = root.join(dir_name);
        if !dir.exists() {
            continue;
        }
        saw_dir = true;
        for entry in std::fs::read_dir(&dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("lisp") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if !seen.insert(name.clone()) {
                continue;
            }
            let file = match read_log_file(&path) {
                Ok(f) => f,
                Err(_) => continue, // skip non-execution lisps
            };
            let meta = match file.find_block("meta") {
                Some(m) => parse_kv_pairs(&file.src, m.children()),
                None => HashMap::new(),
            };
            let parent = meta
                .get("parent-design")
                .or_else(|| meta.get("parent_design"))
                .or_else(|| meta.get("parent"))
                .cloned()
                .unwrap_or_default();
            let status = meta
                .get("status")
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            let scope = meta.get("scope").cloned().unwrap_or_default();
            // Workstation-dispatch metadata; legacy logs may omit it. Empty
            // string preserves a stable column shape for dashboards while
            // signalling "no record" cheaply.
            let dispatch = meta
                .get("dispatch-strategy")
                .map(|s| s.trim().trim_matches('"').to_string())
                .unwrap_or_default();
            let target_project = meta
                .get("target-project")
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty());

            if let Some(pf) = parent_filter {
                if !parent.contains(pf) {
                    continue;
                }
            }
            if let Some(sf) = status_filter {
                if !status.contains(sf) {
                    continue;
                }
            }
            if let Some(sp) = scope_prefix {
                if !scope.starts_with(sp) {
                    continue;
                }
            }

            let claims = parse_claims(&file);
            let active = claims.iter().filter(|c| c.status == "active").count();
            // Surface a thin durability snapshot per execution so dashboards can
            // tell at a glance whether scoped commits are flowing. Full per-row
            // details still live behind `mission_execution(action=status)` —
            // here we only carry counts + the latest commit_status to keep the
            // list payload small (intent-memory.lisp :: helper agent-execution-
            // coordination :: scoped-commit-contract :: invariants :inv-7).
            let completions = parse_completions(&file);
            let durability = summarize_durability(&completions);
            let mut row = json!({
                "execution_id": name,
                "path": path.display().to_string(),
                "storage_root": dir_name,
                "legacy_path": legacy,
                "parent_design": parent.trim_matches('"'),
                "status": status.trim_matches('"'),
                "scope": scope.trim_matches('"'),
                "active_claims": active,
                "claim_count": claims.len(),
                "dispatch_strategy": dispatch,
                "durability": durability,
            });
            if let Some(tp) = target_project {
                row["target_project"] = json!(tp);
            }
            summaries.push(row);
            if summaries.len() >= limit {
                break;
            }
        }
        if summaries.len() >= limit {
            break;
        }
    }
    if !saw_dir {
        return Ok(ToolResult::json_pretty(&json!({
            "executions": [],
            "hint": format!(
                "no {} or {} directory under {}",
                COMPANION_DIR,
                LEGACY_COMPANION_DIR,
                root.display()
            ),
        })));
    }

    summaries.sort_by(|a, b| {
        a["execution_id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["execution_id"].as_str().unwrap_or(""))
    });

    Ok(ToolResult::json_pretty(&json!({
        "executions": summaries,
        "count": summaries.len(),
    })))
}
