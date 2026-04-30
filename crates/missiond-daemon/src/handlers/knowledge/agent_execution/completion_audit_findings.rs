use chrono::Utc;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::path::Path;

use super::claim_lease::{parse_claims, parse_iso, scopes_overlap};
use super::completion_gates::{audit_scoped_commit_handoff, check_id_monotonic};
use super::lisp_syntax as sexp;
use super::log_counters::Counter;
use super::log_store::{list_block_summaries, parse_kv_pairs, LogFile};

pub(super) struct AuditFindings {
    pub(super) file: LogFile,
    pub(super) findings: Vec<Value>,
}

impl AuditFindings {
    pub(super) fn ok(&self) -> bool {
        self.findings.iter().all(|f| {
            f.get("severity")
                .and_then(|v| v.as_str())
                .map(|s| s != "error")
                .unwrap_or(true)
        })
    }

    pub(super) fn error_count(&self) -> u32 {
        self.findings
            .iter()
            .filter(|f| f.get("severity").and_then(|v| v.as_str()) == Some("error"))
            .count() as u32
    }
}

pub(super) fn collect_audit_findings(
    raw: String,
    path: &Path,
    execution_id: &str,
) -> std::result::Result<AuditFindings, ToolResult> {
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
            return Err(ToolResult::json_pretty(&json!({
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
            "detail": "id-counters block absent; mutating actions fall back to scan-max; run action=repair to materialize",
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
            let _ = comps;
        }
    }

    audit_scoped_commit_handoff(&file, &claims, &mut findings);

    Ok(AuditFindings { file, findings })
}
