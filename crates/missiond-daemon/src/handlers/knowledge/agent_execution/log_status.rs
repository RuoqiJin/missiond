use crate::state::AppState;
use anyhow::Result;
use chrono::Utc;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};

use super::claim_lease::{parse_claims, parse_iso};
use super::completion_durability::summarize_durability;
use super::completion_records::parse_completions;
use super::log_store::{
    companion_path, json_strip_quotes, list_block_summaries, parse_kv_pairs,
    project_or_target_project, read_log_file, require_str, resolve_project_root,
};

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
