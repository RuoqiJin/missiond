use crate::state::AppState;
use anyhow::{anyhow, Result};
use chrono::Utc;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::warn;

use super::claim_lease::{parse_claims, parse_iso};
use super::completion_records::{parse_completions, summarize_durability};
use super::lisp_syntax as sexp;
use super::log_store::{
    allocate_id, append_to_block, companion_path, json_strip_quotes, lisp_quote_string,
    list_block_summaries, now_iso, parse_kv_pairs, project_or_target_project, read_log_file,
    render_canonical_template, require_str, resolve_project_root, touch_last_updated,
    write_log_file, Counter, LogFile, COMPANION_DIR,
};
use super::session_trace::{
    append_session_trace_event, resolve_session_trace_path, resolve_trace_task_id,
    sanitize_trace_backend, TraceEvent, TraceKind,
};

/// Canonical workstation-dispatch strategies surfaced by intent-tools.lisp ::
/// implemented-surface mission_execution :: :workstation-dispatch-record. Kept
/// in sync with `plan.rs::VALID_DISPATCH_STRATEGIES`; unknown / empty inputs
/// normalize to `DEFAULT_DISPATCH_STRATEGY` so legacy callers keep working.
const VALID_DISPATCH_STRATEGIES: &[&str] = &[
    "resident-lisp",
    "fresh-code-alignment",
    "agent-team",
    "mixed",
    "prompt-fallback",
    "unknown",
];
pub(super) const DEFAULT_DISPATCH_STRATEGY: &str = "unknown";

/// Normalize an optional dispatch strategy string against the canonical set.
/// Unknown / empty values fall back to `DEFAULT_DISPATCH_STRATEGY` (`"unknown"`)
/// without erroring; we never hard-fail open() on a strategy mismatch because
/// upstream dispatchers may legitimately surface novel labels we then audit.
pub(super) fn normalize_dispatch_strategy(raw: Option<&str>) -> &'static str {
    let v = raw.unwrap_or("").trim();
    if v.is_empty() {
        return DEFAULT_DISPATCH_STRATEGY;
    }
    for &known in VALID_DISPATCH_STRATEGIES {
        if known == v {
            return known;
        }
    }
    DEFAULT_DISPATCH_STRATEGY
}

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

// ───────────────────────────────────────────────────────────────────────
// action: deviate / decide / issue / complete
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_deviate(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let lisp_said = match require_str(args, "lisp_said") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let actually_found = match require_str(args, "actually_found") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let reason = match require_str(args, "reason") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let approved_by = args
        .get("approved_by")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
    let phase = args.get("phase").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Deviation)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :phase {phase}\n      :date {date}\n      :lisp-said {lisp_said}\n      :actually-found {actually_found}\n      :reason {reason}\n      :approved-by {approved_by}\n      :status \"open\")",
        id = id,
        phase = lisp_quote_string(phase),
        date = lisp_quote_string(&date),
        lisp_said = lisp_quote_string(lisp_said),
        actually_found = lisp_quote_string(actually_found),
        reason = lisp_quote_string(reason),
        approved_by = lisp_quote_string(approved_by),
    );
    append_to_block(&mut file, "deviations", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Wave 20 / Task 09 — surface the workstation-dispatch trio so a
    // deviation observer can route on dispatch context without re-loading
    // the companion log. Read from the post-write `file` handle.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::DeviationRecorded {
            execution_id: execution_id.to_string(),
            deviation_id: id.clone(),
            phase: phase.to_string(),
            approved_by: approved_by.to_string(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "deviation_id": id,
        "phase": phase,
        "approved_by": approved_by,
    })))
}

pub(super) async fn action_decide(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let context = match require_str(args, "context") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let chosen = match require_str(args, "chosen") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let rationale = match require_str(args, "rationale") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let decided_by = match require_str(args, "decided_by") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let options = args.get("options").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Decision)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :context {context}\n      :options {options}\n      :chosen {chosen}\n      :rationale {rationale}\n      :decided-by {decided_by}\n      :at {date})",
        id = id,
        context = lisp_quote_string(context),
        options = lisp_quote_string(options),
        chosen = lisp_quote_string(chosen),
        rationale = lisp_quote_string(rationale),
        decided_by = lisp_quote_string(decided_by),
        date = lisp_quote_string(&date),
    );
    append_to_block(&mut file, "decisions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Wave 20 / Task 09 — surface the workstation-dispatch trio so a
    // decision observer can route on dispatch context without re-loading
    // the companion log. Read from the post-write `file` handle.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::DecisionRecorded {
            execution_id: execution_id.to_string(),
            decision_id: id.clone(),
            decided_by: decided_by.to_string(),
            at: date.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "decision_id": id,
        "decided_by": decided_by,
        "at": date,
    })))
}

pub(super) async fn action_issue(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let severity = args
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let desc = match require_str(args, "desc") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let resolution_path = args
        .get("resolution_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let owner = args.get("owner").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Issue)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :severity {severity}\n      :desc {desc}\n      :resolution-path {res}\n      :owner {owner}\n      :at {date}\n      :status \"open\")",
        id = id,
        severity = lisp_quote_string(severity),
        desc = lisp_quote_string(desc),
        res = lisp_quote_string(resolution_path),
        owner = lisp_quote_string(owner),
        date = lisp_quote_string(&date),
    );
    append_to_block(&mut file, "issues", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Wave 20 / Task 09 — surface the workstation-dispatch trio so an
    // issue observer can route on dispatch context without re-loading
    // the companion log. Read from the post-write `file` handle.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::IssueRecorded {
            execution_id: execution_id.to_string(),
            issue_id: id.clone(),
            severity: severity.to_string(),
            owner: owner.to_string(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "issue_id": id,
        "severity": severity,
        "owner": owner,
    })))
}

// ───────────────────────────────────────────────────────────────────────
// action: status — meta + active claims + open issues + unresolved deviations
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_status(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let file = read_log_file(&path)?;

    let meta = file
        .find_block("meta")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();
    let counters = file
        .find_block("id-counters")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();
    let phase_tracker = file
        .find_block("phase-tracker")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();

    let claims = parse_claims(&file);
    let now = Utc::now();
    let active_claims: Vec<Value> = claims
        .iter()
        .filter(|c| c.status == "active")
        .map(|c| {
            let stale = c
                .lease_expires_at
                .as_deref()
                .and_then(parse_iso)
                .map(|exp| exp < now)
                .unwrap_or(false);
            json!({
                "id": c.id,
                "claimer": c.claimer,
                "scope": c.scope,
                "phase": c.phase,
                "lease_expires_at": c.lease_expires_at,
                "heartbeat_at": c.heartbeat_at,
                "stale": stale,
            })
        })
        .collect();

    let unresolved = list_block_summaries(&file, "deviations", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(json!({
                "id": head.to_string(),
                "phase": kvs.get("phase").cloned().unwrap_or_default(),
                "lisp_said": kvs.get("lisp-said").or_else(|| kvs.get("lisp_said")).cloned().unwrap_or_default(),
                "actually_did": kvs.get("actually-found").or_else(|| kvs.get("actually-did")).cloned().unwrap_or_default(),
                "approved_by": kvs.get("approved-by").or_else(|| kvs.get("approved_by")).cloned().unwrap_or_default(),
                "status": status,
            }))
        }
    });

    let open_issues = list_block_summaries(&file, "issues", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(json!({
                "id": head.to_string(),
                "severity": kvs.get("severity").cloned().unwrap_or_default(),
                "desc": kvs.get("desc").cloned().unwrap_or_default(),
                "owner": kvs.get("owner").cloned().unwrap_or_default(),
                "status": status,
            }))
        }
    });

    let latest_decisions = list_block_summaries(&file, "decisions", |kvs, head| {
        Some(json!({
            "id": head.to_string(),
            "context": kvs.get("context").cloned().unwrap_or_default(),
            "chosen": kvs.get("chosen").cloned().unwrap_or_default(),
            "decided_by": kvs.get("decided-by").or_else(|| kvs.get("decided_by")).cloned().unwrap_or_default(),
            "at": kvs.get("at").cloned().unwrap_or_default(),
        }))
    });

    // ── completion durability projection ───────────────────────────
    // intent-memory.lisp :: helper agent-execution-coordination :: completions
    // gained `changed_files / staged_files / commit_hash / commit_status /
    // commit_blocker` for the scoped-commit handoff. Surface them in
    // `completed_phases` (legacy keys preserved) and roll them up into a
    // dedicated `durability` block so dashboards can show "still pending /
    // blocked / fully durable" without re-parsing the companion log.
    let completion_records = parse_completions(&file);
    let completed_phases: Vec<Value> = completion_records
        .iter()
        .map(|c| {
            let mut row = json!({
                "id": c.id,
                "phase": c.phase,
                "agent": c.agent,
                "at": c.at,
            });
            if let Some(list) = &c.changed_files {
                row["changed_files"] = json!(list);
            }
            if let Some(list) = &c.staged_files {
                row["staged_files"] = json!(list);
            }
            if let Some(hash) = &c.commit_hash {
                row["commit_hash"] = json!(hash);
            }
            if let Some(status_val) = &c.commit_status {
                row["commit_status"] = json!(status_val);
            }
            if let Some(blocker) = &c.commit_blocker {
                row["commit_blocker"] = json!(blocker);
            }
            // wave-19 / task 08 — task-contract metadata projection.
            // Same skip-on-absent semantics as the scoped-commit fields
            // above so legacy completed_phases entries stay shape-stable.
            if let Some(tcp) = &c.task_contract_path {
                row["task_contract_path"] = json!(tcp);
            }
            if let Some(trp) = &c.task_report_path {
                row["task_report_path"] = json!(trp);
            }
            if let Some(vs) = &c.verifier_status {
                row["verifier_status"] = json!(vs);
            }
            if let Some(vn) = &c.verifier_notes {
                row["verifier_notes"] = json!(vn);
            }
            row
        })
        .collect();

    let durability = summarize_durability(&completion_records);

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "meta": json_strip_quotes(meta),
        "id_counters": json_strip_quotes(counters),
        "phase_tracker": json_strip_quotes(phase_tracker),
        "active_claims": active_claims,
        "unresolved_deviations": unresolved,
        "open_issues": open_issues,
        "latest_decisions": latest_decisions,
        "completed_phases": completed_phases,
        "durability": durability,
    })))
}

/// Build an `ExecutionEvent::Opened` payload from the inputs `action_open`
/// has already validated and normalized. Centralizing the construction
/// keeps the dispatch-metadata mapping (intent-worker.lisp ::
/// claudecode-workstation-orchestration :: execution-strategy-record)
/// in one testable place — the runtime caller and the unit tests stay in
/// lock-step on which open args land in which event slot.
///
/// `dispatch_strategy` always resolves to a canonical string via
/// `normalize_dispatch_strategy`. We surface it on the event verbatim so
/// downstream auditors observe the same label that lives in the companion
/// log meta block. `target_project` / `requested_cwd` are forwarded only
/// when the open args carry them — `Option::is_none` skip-serialize keeps
/// the wire form byte-identical to the legacy 5-field shape otherwise.
pub(super) fn build_opened_event(
    execution_id: &str,
    parent_design: &str,
    scope: &str,
    owner: &str,
    path: String,
    dispatch_strategy: &str,
    target_project: Option<&str>,
    requested_cwd: Option<&str>,
) -> ExecutionEvent {
    ExecutionEvent::Opened {
        execution_id: execution_id.to_string(),
        parent_design: parent_design.to_string(),
        scope: scope.to_string(),
        owner: owner.to_string(),
        path,
        dispatch_strategy: Some(dispatch_strategy.to_string()),
        target_project: target_project.map(|s| s.to_string()),
        requested_cwd: requested_cwd.map(|s| s.to_string()),
    }
}

/// Single tuple of the workstation-dispatch trio surfaced on every
/// `ExecutionEvent` variant that carries dispatch context. Sourced from the
/// companion-log meta block so consumers don't have to re-load the file to
/// correlate the event against its dispatch strategy / target project /
/// requested cwd. All three fields are `None` when the meta block omits the
/// corresponding `:key`, which lets the legacy companion logs (pre-wave12-01)
/// emit cleanly with the default skip-serialize wire form.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DispatchMeta {
    pub(super) dispatch_strategy: Option<String>,
    pub(super) target_project: Option<String>,
    pub(super) requested_cwd: Option<String>,
}

/// Read the workstation-dispatch trio (`:dispatch-strategy` /
/// `:target-project` / `:requested-cwd`) from the companion-log meta block.
///
/// Mirrors the parsing path used by `action_list` so the live event stream
/// and the dashboard list view see identical strings. Quoted-string atoms
/// have their outer quotes stripped via `trim_matches('"')` to match the
/// downstream contract; whitespace-only values collapse to `None` so a
/// caller that wrote `:target-project ""` doesn't surface a confusing empty
/// label on the bus.
///
/// Returns `DispatchMeta::default()` when the file has no meta block — the
/// caller emits the event without metadata in that case, matching what
/// legacy producers serialized before the trio was added.
pub(super) fn read_dispatch_metadata_from_log(file: &LogFile) -> DispatchMeta {
    let Some(block) = file.find_block("meta") else {
        return DispatchMeta::default();
    };
    let meta = parse_kv_pairs(&file.src, block.children());
    let read = |key: &str| -> Option<String> {
        meta.get(key)
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
    };
    DispatchMeta {
        dispatch_strategy: read("dispatch-strategy"),
        target_project: read("target-project"),
        requested_cwd: read("requested-cwd"),
    }
}
