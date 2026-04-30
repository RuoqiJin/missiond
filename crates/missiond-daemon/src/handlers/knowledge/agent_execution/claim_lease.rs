use crate::state::AppState;
use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};

use super::log_counters::{allocate_id, Counter};
use super::log_store::{
    append_to_block, companion_path, lisp_quote_string, now_iso, project_or_target_project,
    read_log_file, require_str, resolve_project_root, touch_last_updated, write_log_file,
};
use super::log_surface::{emit_execution_event, read_dispatch_metadata_from_log};

pub(super) use super::claim_heartbeat::action_heartbeat;
pub(super) use super::claim_records::{find_claim_node, parse_claims, parse_iso, ClaimRecord};
pub(super) use super::claim_release::action_release;

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
