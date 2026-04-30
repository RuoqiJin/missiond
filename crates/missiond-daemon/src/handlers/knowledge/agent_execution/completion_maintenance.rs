use anyhow::Result;
use chrono::Utc;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};

use crate::state::AppState;

use super::claim_lease::{parse_claims, parse_iso, scopes_overlap};
use super::completion_gates::{audit_scoped_commit_handoff, check_id_monotonic};
use super::lisp_syntax as sexp;
use super::log_counters::Counter;
use super::log_dispatch::read_dispatch_metadata_from_log;
use super::log_store::{
    companion_path, list_block_summaries, parse_kv_pairs, project_or_target_project, require_str,
    resolve_project_root, LogFile,
};
use super::log_surface::emit_execution_event;

pub(super) use super::completion_repair::action_repair;

// ───────────────────────────────────────────────────────────────────────
// action: audit — paren balance + ID monotonic + claim overlap + stale +
//                 completion coverage + open-issue owners
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_audit(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let raw = std::fs::read_to_string(&path)?;
    let mut findings: Vec<Value> = Vec::new();

    if let Err(e) = sexp::check_balance(&raw) {
        findings.push(json!({
            "severity": "error",
            "kind": "paren-imbalance",
            "detail": e.to_string(),
        }));
    }

    let file = match LogFile::parse(raw) {
        Ok(f) => f,
        Err(e) => {
            findings.push(json!({
                "severity": "error",
                "kind": "parse-failed",
                "detail": e.to_string(),
            }));
            return Ok(ToolResult::json_pretty(&json!({
                "execution_id": execution_id,
                "path": path.display().to_string(),
                "ok": false,
                "findings": findings,
            })));
        }
    };

    if file.find_block("id-counters").is_none() {
        findings.push(json!({
            "severity": "warn",
            "kind": "missing-id-counters",
            "detail": "id-counters block absent; mutating actions fall back to scan-max — run action=repair to materialize",
        }));
    }

    for counter in [
        Counter::Claim,
        Counter::Deviation,
        Counter::Decision,
        Counter::Issue,
        Counter::Completion,
    ] {
        check_id_monotonic(&file, counter, &mut findings);
    }

    let claims = parse_claims(&file);
    let now = Utc::now();
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                findings.push(json!({
                    "severity": "warn",
                    "kind": "stale-claim",
                    "claim_id": c.id,
                    "claimer": c.claimer,
                    "lease_expires_at": c.lease_expires_at,
                    "detail": "lease expired with no release/heartbeat",
                }));
            }
        }
    }

    // Active claim overlaps.
    for (i, a) in claims.iter().enumerate() {
        if a.status != "active" {
            continue;
        }
        for b in claims.iter().skip(i + 1) {
            if b.status != "active" {
                continue;
            }
            if scopes_overlap(&a.scope, &b.scope) {
                findings.push(json!({
                    "severity": "error",
                    "kind": "claim-overlap",
                    "left": a.id,
                    "right": b.id,
                    "scope_left": a.scope,
                    "scope_right": b.scope,
                }));
            }
        }
    }

    // Open-issue owners.
    let issues_block = file.find_block("issues");
    if let Some(block) = issues_block {
        for child in block.children().iter().skip(1) {
            let kvs = parse_kv_pairs(&file.src, child.children());
            let status = kvs
                .get("status")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_else(|| "open".to_string());
            if status == "resolved" || status == "closed" {
                continue;
            }
            let owner = kvs
                .get("owner")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_default();
            if owner.is_empty() {
                let head = child.head_atom().unwrap_or("?");
                findings.push(json!({
                    "severity": "warn",
                    "kind": "open-issue-no-owner",
                    "issue_id": head,
                }));
            }
        }
    }

    // Completion coverage: each phase referenced by a completion should have
    // a phase entry. We just check the inverse — phases marked completed in
    // phase-tracker should have at least one COMP entry referencing them.
    let phase_tracker = file
        .find_block("phase-tracker")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();
    if let Some(current) = phase_tracker.get("current-phase") {
        if current.trim().trim_matches('"') != "nil" && !current.trim().is_empty() {
            let comps = list_block_summaries(&file, "completions", |kvs, head| {
                Some(json!({
                    "id": head,
                    "phase": kvs.get("phase").cloned().unwrap_or_default(),
                }))
            });
            let _ = comps; // informational only — no failing assertion yet
        }
    }

    // ── scoped-commit handoff durability checks ──────────────────────
    // intent-memory.lisp :: helper agent-execution-coordination ::
    // scoped-commit-contract + invariants :inv-7 — every completion that
    // claims to have committed must carry a real commit_hash, every
    // blocked completion must explain itself, and staged_files must stay
    // inside the claim scope (the active or most-recently-released claim
    // owned by the same agent). These are read-only audit findings — the
    // daemon never executes git itself; the writer agent is responsible
    // for the actual commit. See task-file
    // wave12-01-mission-execution-scoped-commit-handoff.md.
    audit_scoped_commit_handoff(&file, &claims, &mut findings);

    let ok = findings.iter().all(|f| {
        f.get("severity")
            .and_then(|v| v.as_str())
            .map(|s| s != "error")
            .unwrap_or(true)
    });

    let error_count = findings
        .iter()
        .filter(|f| f.get("severity").and_then(|v| v.as_str()) == Some("error"))
        .count() as u32;
    // Wave 20 / Task 09 — surface the workstation-dispatch trio on the
    // audit + stale-claim events. Audit is read-only so we don't write
    // back to the file; the meta block we observe is whatever the latest
    // writer left there.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Audited {
            execution_id: execution_id.to_string(),
            ok,
            findings_count: findings.len() as u32,
            error_count,
            dispatch_strategy: meta.dispatch_strategy.clone(),
            target_project: meta.target_project.clone(),
            requested_cwd: meta.requested_cwd.clone(),
        },
    )
    .await;
    for f in &findings {
        if f.get("kind").and_then(|v| v.as_str()) != Some("stale-claim") {
            continue;
        }
        let claim_id = f
            .get("claim_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let claimer = f
            .get("claimer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let lease_expires_at = f
            .get("lease_expires_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        emit_execution_event(
            state,
            ExecutionEvent::StaleClaim {
                execution_id: execution_id.to_string(),
                claim_id,
                claimer,
                lease_expires_at,
                dispatch_strategy: meta.dispatch_strategy.clone(),
                target_project: meta.target_project.clone(),
                requested_cwd: meta.requested_cwd.clone(),
            },
        )
        .await;
    }

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "ok": ok,
        "findings": findings,
    })))
}
