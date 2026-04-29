use crate::state::AppState;
use anyhow::{anyhow, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use super::lisp_syntax::Node;
use super::log_store::{
    allocate_id, append_to_block, companion_path, lisp_quote_string, now_iso, parse_kv_pairs,
    project_or_target_project, read_log_file, require_str, resolve_project_root,
    touch_last_updated, update_kv_in_node, write_log_file, Counter, LogFile,
};
use super::log_surface::{emit_execution_event, read_dispatch_metadata_from_log};

pub(super) const DEFAULT_LEASE_SECS: i64 = 1800;
pub(super) const MAX_LEASE_SECS: i64 = 24 * 3600;

pub(super) fn scopes_overlap(a: &str, b: &str) -> bool {
    scopes_overlap_pure(a, b)
}

/// wave-17 / task 02 — pure scope-overlap predicate exposed to the
/// PLAN DAG scheduler so claim-lease conflict detection reuses the
/// exact semantics established by wave12-01 (agent_execution::action_claim)
/// and wave16-06 (enforce_scoped_commit_completion).
///
/// Same prefix-match contract: empty strings never overlap; strings match if
/// they are equal OR one is a prefix of the other. Re-exporting this from the
/// facade keeps the `plan_dag.rs` dependency stable while the implementation
/// now lives under the V3 claim-lease surface.
pub(in crate::handlers::knowledge) fn scopes_overlap_pure(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.starts_with(b) || b.starts_with(a)
}

// ───────────────────────────────────────────────────────────────────────
// claim helpers
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(super) struct ClaimRecord {
    pub(super) id: String,
    pub(super) claimer: String,
    pub(super) scope: String,
    pub(super) phase: Option<String>,
    pub(super) lease_expires_at: Option<String>,
    pub(super) heartbeat_at: Option<String>,
    pub(super) status: String,
}

pub(super) fn parse_claims(file: &LogFile) -> Vec<ClaimRecord> {
    let block = match file.find_block("claims") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for child in block.children().iter().skip(1) {
        let head = child.head_atom().unwrap_or("");
        let kvs = parse_kv_pairs(&file.src, child.children());
        // Two flavors: head is the id, or `:id <ID>` is inline.
        let id = if head.starts_with(['C', 'c'])
            && head.len() > 1
            && head[1..].chars().all(|c| c.is_ascii_digit())
        {
            head.to_string()
        } else if let Some(v) = kvs.get("id").or_else(|| kvs.get("claim-id")).cloned() {
            v.trim().to_string()
        } else {
            // Legacy unnumbered claim — keep but with synthetic id.
            format!("claim@{}", child.start)
        };
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| {
                if kvs.get("released-at").is_some() {
                    "released".to_string()
                } else {
                    "active".to_string()
                }
            });
        out.push(ClaimRecord {
            id,
            claimer: kvs
                .get("claimer")
                .or_else(|| kvs.get("agent"))
                .cloned()
                .unwrap_or_default(),
            scope: kvs.get("scope").cloned().unwrap_or_default(),
            phase: kvs.get("phase").cloned(),
            lease_expires_at: kvs.get("lease-expires-at").cloned(),
            heartbeat_at: kvs.get("heartbeat-at").cloned(),
            status,
        });
    }
    out
}

pub(super) fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    let t = s.trim().trim_matches('"');
    DateTime::parse_from_rfc3339(t)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

pub(super) fn find_claim_node<'a>(file: &'a LogFile, claim_id: &str) -> Option<&'a Node> {
    let block = file.find_block("claims")?;
    for child in block.children().iter().skip(1) {
        if child.head_atom() == Some(claim_id) {
            return Some(child);
        }
        let kvs = parse_kv_pairs(&file.src, child.children());
        if let Some(id) = kvs.get("id").or_else(|| kvs.get("claim-id")) {
            if id.trim().trim_matches('"') == claim_id {
                return Some(child);
            }
        }
    }
    None
}

// ───────────────────────────────────────────────────────────────────────
// action: claim / heartbeat / release
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_claim(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claimer = match require_str(args, "claimer_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let scope = match require_str(args, "scope") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let phase = args.get("phase").and_then(|v| v.as_str()).unwrap_or("");
    let lease_secs = args
        .get("lease_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LEASE_SECS)
        .clamp(60, MAX_LEASE_SECS);

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    // Conflict check: any active claim with overlapping scope.
    let now = Utc::now();
    let claims = parse_claims(&file);
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        // Treat lease-expired claims as soft-released for conflict purposes
        // (still surfaced in audit as stale).
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                continue;
            }
        }
        if scopes_overlap(&c.scope, scope) {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    "CLAIM_CONFLICT",
                    format!(
                        "scope `{}` overlaps active claim {} held by `{}` over `{}`",
                        scope, c.id, c.claimer, c.scope
                    ),
                )
                .with_suggestion(
                    "wait for release/heartbeat expiry, narrow scope, or contact the claimer",
                ),
            ));
        }
    }

    let claim_id = allocate_id(&mut file, Counter::Claim)?;
    let acquired = now_iso();
    let expires =
        (now + chrono::Duration::seconds(lease_secs)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let entry = format!(
        "    ({id}\n      :claimer {claimer}\n      :scope {scope}\n      :phase {phase}\n      :acquired-at {acquired}\n      :lease-expires-at {expires}\n      :heartbeat-at {acquired}\n      :status \"active\")",
        id = claim_id,
        claimer = lisp_quote_string(claimer),
        scope = lisp_quote_string(scope),
        phase = lisp_quote_string(phase),
        acquired = lisp_quote_string(&acquired),
        expires = lisp_quote_string(&expires),
    );
    append_to_block(&mut file, "claims", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Surface the workstation-dispatch trio on the live event so consumers
    // can correlate this claim against the dispatch context without
    // re-loading the companion log. We read the trio from the same
    // post-write `file` handle so the meta block we observe is the one
    // just persisted (the claim append doesn't touch meta beyond
    // `:last-updated-at`, which we ignore here).
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Claimed {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.clone(),
            claimer: claimer.to_string(),
            scope: scope.to_string(),
            phase: phase.to_string(),
            lease_expires_at: expires.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "claimed",
        "claim_id": claim_id,
        "claimer": claimer,
        "scope": scope,
        "phase": phase,
        "acquired_at": acquired,
        "lease_expires_at": expires,
    })))
}

pub(super) async fn action_heartbeat(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claim_id = match require_str(args, "claim_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claimer = match require_str(args, "claimer_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let lease_secs = args
        .get("lease_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LEASE_SECS)
        .clamp(60, MAX_LEASE_SECS);

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    let claim_node = match find_claim_node(&file, claim_id) {
        Some(n) => n.clone(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("claim {} not found", claim_id),
                )
                .with_suggestion("use action=status to list active claims"),
            ))
        }
    };

    let kvs = parse_kv_pairs(&file.src, claim_node.children());
    let owner = kvs
        .get("claimer")
        .or_else(|| kvs.get("agent"))
        .cloned()
        .unwrap_or_default();
    if owner.trim_matches('"') != claimer {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "CLAIM_WRONG_OWNER",
                format!("claim {} owned by `{}`, not `{}`", claim_id, owner, claimer),
            )
            .with_suggestion("use the original claimer_name or run action=audit"),
        ));
    }

    let now = Utc::now();
    let now_s = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let expires =
        (now + chrono::Duration::seconds(lease_secs)).to_rfc3339_opts(SecondsFormat::Secs, true);

    update_kv_in_node(
        &mut file,
        &claim_node,
        "heartbeat-at",
        &lisp_quote_string(&now_s),
    )?;
    let claim_node2 = find_claim_node(&file, claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("claim node vanished after heartbeat update"))?;
    update_kv_in_node(
        &mut file,
        &claim_node2,
        "lease-expires-at",
        &lisp_quote_string(&expires),
    )?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Wave 20 / Task 09 — surface the workstation-dispatch trio on the
    // live event so a long-lived heartbeat stream stays correlatable
    // against the dispatch context. The same projection rationale as
    // `action_claim` / `action_complete`: read the trio from the
    // post-write `file` handle so the meta block we observe is the one
    // just persisted.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Heartbeat {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.to_string(),
            claimer: claimer.to_string(),
            heartbeat_at: now_s.clone(),
            lease_expires_at: expires.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "heartbeat",
        "claim_id": claim_id,
        "heartbeat_at": now_s,
        "lease_expires_at": expires,
    })))
}

pub(super) async fn action_release(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claim_id = match require_str(args, "claim_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claimer = match require_str(args, "claimer_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let summary = args.get("summary").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    let claim_node = match find_claim_node(&file, claim_id) {
        Some(n) => n.clone(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("claim {} not found", claim_id),
                )
                .with_suggestion("use action=status to list active claims"),
            ))
        }
    };

    let kvs = parse_kv_pairs(&file.src, claim_node.children());
    let owner = kvs
        .get("claimer")
        .or_else(|| kvs.get("agent"))
        .cloned()
        .unwrap_or_default();
    if owner.trim_matches('"') != claimer {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "CLAIM_WRONG_OWNER",
                format!("claim {} owned by `{}`, not `{}`", claim_id, owner, claimer),
            )
            .with_suggestion("use the original claimer_name or run action=audit"),
        ));
    }

    let now = now_iso();
    update_kv_in_node(
        &mut file,
        &claim_node,
        "released-at",
        &lisp_quote_string(&now),
    )?;
    let claim_node2 = find_claim_node(&file, claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("claim node vanished after release update"))?;
    update_kv_in_node(
        &mut file,
        &claim_node2,
        "status",
        &lisp_quote_string("released"),
    )?;
    if !summary.is_empty() {
        let claim_node3 = find_claim_node(&file, claim_id)
            .cloned()
            .ok_or_else(|| anyhow!("claim node vanished after status update"))?;
        update_kv_in_node(
            &mut file,
            &claim_node3,
            "summary",
            &lisp_quote_string(summary),
        )?;
    }
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Wave 20 / Task 09 — same dispatch-metadata projection rationale as
    // `action_claim`. `Released` completes the pair with `Claimed`, so
    // claim-lifetime aggregators can join the two events without
    // re-loading the companion log.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Released {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.to_string(),
            claimer: claimer.to_string(),
            released_at: now.clone(),
            summary: if summary.is_empty() {
                None
            } else {
                Some(summary.to_string())
            },
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "released",
        "claim_id": claim_id,
        "released_at": now,
        "summary": summary,
    })))
}
