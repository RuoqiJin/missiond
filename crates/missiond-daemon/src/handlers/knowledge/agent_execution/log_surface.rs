use crate::state::AppState;
use anyhow::{anyhow, Result};
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::warn;

use super::claim_lease::parse_claims;
use super::completion_durability::summarize_durability;
use super::completion_records::parse_completions;
use super::lisp_syntax as sexp;
use super::log_dispatch::{build_opened_event, normalize_dispatch_strategy};
use super::log_store::{
    companion_path, parse_kv_pairs, project_or_target_project, read_log_file,
    render_canonical_template, require_str, resolve_project_root, COMPANION_DIR,
};
use super::session_trace::{
    append_session_trace_event, resolve_session_trace_path, resolve_trace_task_id,
    sanitize_trace_backend, TraceEvent, TraceKind,
};

/// Forward an `ExecutionEvent` to the v2 bus and log (but never propagate)
/// publish failures. Companion-log writes are already durable on disk; the
/// bus event is a live projection.
pub(super) async fn emit_execution_event(state: &AppState, ev: ExecutionEvent) {
    if let Err(e) = state.bus.publish_execution(ev).await {
        warn!(error = %e, "failed to publish ExecutionEvent (companion log already durable)");
    }
}

pub(super) async fn action_open(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let parent_design = match require_str(args, "parent_design") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let scope = match require_str(args, "scope") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let owner = args
        .get("owner")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let dispatch_strategy =
        normalize_dispatch_strategy(args.get("dispatch_strategy").and_then(|v| v.as_str()));
    let target_project = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let requested_cwd = args
        .get("requested_cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);

    if path.exists() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "EXECUTION_EXISTS",
                format!("companion log already exists at {}", path.display()),
            )
            .with_suggestion("use action=status to inspect, or pick a different execution_id"),
        ));
    }

    let body = render_canonical_template(
        execution_id,
        parent_design,
        scope,
        owner,
        dispatch_strategy,
        target_project,
        requested_cwd,
    );
    sexp::check_balance(&body).map_err(|e| anyhow!("template paren balance broken: {}", e))?;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, body.as_bytes())?;

    // intent-worker.lisp :: claudecode-workstation-orchestration ::
    // execution-strategy-record asks for dispatch metadata to be surfaced on
    // the live ExecutionEvent::Opened projection alongside the durable
    // companion-log meta block. The companion log remains the source of
    // truth (per planned-event-extensions :: ExecutionEvent :: rationale);
    // these optional fields are skipped on serialize when absent so legacy
    // Opened consumers stay byte-identical.
    let event = build_opened_event(
        execution_id,
        parent_design,
        scope,
        owner,
        path.display().to_string(),
        dispatch_strategy,
        target_project,
        requested_cwd,
    );
    emit_execution_event(state, event).await;

    let mut response = json!({
        "status": "opened",
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "parent_design": parent_design,
        "scope": scope,
        "owner": owner,
        "dispatch_strategy": dispatch_strategy,
    });
    if let Some(tp) = target_project {
        response["target_project"] = json!(tp);
    }
    if let Some(cwd) = requested_cwd {
        response["requested_cwd"] = json!(cwd);
    }

    // wave23-04 — opt-in session-trace append. When the caller threads
    // `session_trace_path` we emit a `dispatch` event capturing this
    // open as the first fact in the task's trace. Best-effort: failures
    // surface as `trace_warning` without aborting the open result.
    if let Some(trace_path) = resolve_session_trace_path(args, &root) {
        match resolve_trace_task_id(args, &root, execution_id) {
            Some(task_id) => {
                let backend = sanitize_trace_backend(owner);
                let ev = TraceEvent {
                    task: task_id,
                    backend,
                    kind: TraceKind::Dispatch,
                    summary: format!(
                        "mission_execution(action=open) execution_id={} parent_design={} dispatch_strategy={}",
                        execution_id, parent_design, dispatch_strategy
                    ),
                    agent: None,
                    files: None,
                    commit_hash: None,
                    report_path: None,
                };
                if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                    response["trace_warning"] = json!(w.to_string());
                }
            }
            None => {
                response["trace_warning"] = json!(format!(
                    "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                    execution_id
                ));
            }
        }
    }

    Ok(ToolResult::json_pretty(&response))
}

// ───────────────────────────────────────────────────────────────────────
// action: list
// ───────────────────────────────────────────────────────────────────────

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
    let dir = root.join(COMPANION_DIR);
    let mut summaries: Vec<Value> = Vec::new();
    if !dir.exists() {
        return Ok(ToolResult::json_pretty(&json!({
            "executions": [],
            "hint": format!("no {} directory under {}", COMPANION_DIR, root.display()),
        })));
    }

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
