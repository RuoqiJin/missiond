//! Wave-19 explicit workflow distill auto-chain recorder.
//!
//! Kept below auto_chain.rs so the public workflow surface still imports
//! one facade while V3 can pin the recorder as its own implementation slice.

use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::state::AppState;

use super::super::super::{evidence_collector, plan as plan_handler};
use super::super::distill::evidence_sidecar_path;
use super::super::resolve_project_root_from_args;

// ───────────────────────────────────────────────────────────────────────
// wave-19 / task 09 :: cross-plan distill auto-chain
//
// `apply_distill_chain` in plan.rs already records cross-plan distill chains
// when callers opt in via `distill_chain_id / distill_chain_mode /
// distill_chain_name`. Wave-19 adds an opt-in counterpart on the workflow
// surface itself so a direct `mission_workflow(action=distill, …)` caller
// can record a chain entry WITHOUT supplying an explicit `chain_id`. The
// id is derived deterministically from the workflow context the caller
// already has in scope:
//
//     project_root + plan_id + (workflow_name | workflow_id) + evidence_sha256
//
// Hard constraints (mirror the wave-19 / task 09 brief):
//   - DEFAULT `auto_chain=false`. Existing callers see byte-identical
//     response shapes; only opt-in payloads carry the `auto_chain` block.
//   - NEVER calls Sonnet implicitly. The auto-chain hook runs AFTER the
//     caller-chosen distill mode (dry_run or sonnet) and only RECORDS the
//     chain entry — it never re-invokes the distiller.
//   - Sidecar is append-only (uses `evidence_collector::append`); no
//     migration, no overwrite, no schema change.
//   - Failures (sidecar write / project resolution) collapse to a partial
//     `auto_chain` block carrying `evidence_error` so the original distill
//     payload stays durable.
// ───────────────────────────────────────────────────────────────────────

/// Source tag stamped on the auto-chain evidence row so consumers can
/// distinguish the wave-19 workflow-side recorder from the wave-18
/// plan_dag-side recorder (`source::PLAN_DAG_NODE_DISPATCH`).
pub(in crate::handlers::knowledge::workflow) const AUTO_CHAIN_EVIDENCE_SOURCE: &str =
    "workflow_distill_auto_chain";

/// Evidence `kind` tag — same wire form as plan.rs's `CHAIN_RECORD_KIND`
/// so a single audit query (`kind="distill_chain_record"`) sees BOTH the
/// wave-18 plan_dag-driven entries AND the wave-19 workflow-driven entries.
pub(in crate::handlers::knowledge::workflow) const AUTO_CHAIN_EVIDENCE_KIND: &str =
    "distill_chain_record";

/// Status surfaced under `auto_chain.status` on the response — kept as
/// constants so audit / test consumers can pin the wire form.
pub(in crate::handlers::knowledge::workflow) const AUTO_CHAIN_STATUS_RECORDED: &str = "recorded";
pub(in crate::handlers::knowledge::workflow) const AUTO_CHAIN_STATUS_RECORD_FAILED: &str =
    "record_failed";
pub(in crate::handlers::knowledge::workflow) const AUTO_CHAIN_STATUS_RESOLVE_FAILED: &str =
    "resolve_failed";

/// Deterministic id source label that mirrors plan.rs's
/// `chain_id_source` taxonomy (`explicit_arg` / `derived_from_plan_id`).
pub(in crate::handlers::knowledge::workflow) const AUTO_CHAIN_ID_SOURCE_DERIVED: &str =
    "derived_from_workflow_context";

/// Returns true when the caller opted into the auto-chain hook. Strict
/// boolean check on `auto_chain` so a missing / non-bool field collapses
/// to false (byte-compat with pre-wave-19 callers).
pub(in crate::handlers::knowledge::workflow) fn auto_chain_requested(args: &Value) -> bool {
    args.get("auto_chain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Optional caller-supplied chain name (free-form, e.g.
/// `"wave19-finalize-loop"`). Echoed into the chain block + evidence row
/// so dashboards can group runs without parsing the deterministic id.
pub(in crate::handlers::knowledge::workflow) fn parse_auto_chain_name(
    args: &Value,
) -> Option<String> {
    args.get("auto_chain_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Derive the deterministic auto-chain id from the workflow context.
///
/// Inputs (in canonical concatenation order):
///
///   1. `project_root`        — registry-resolved, absolute. Anchors the
///                              chain to the project so two projects
///                              cannot collide on the same plan_id.
///   2. `plan_id`             — the source plan UUID.
///   3. `workflow_anchor`     — the persisted `workflow_id` UUID when the
///                              distill row landed; else the workflow
///                              `name` arg; else the literal string
///                              `<unnamed>`. Provides a stable anchor
///                              across re-runs that produce the same
///                              workflow row / name.
///   4. `evidence_sha256`     — sha256 of the on-disk evidence sidecar
///                              when present; the literal string
///                              `<no-evidence>` otherwise. Lets a fresh
///                              evidence record (e.g. additional rows
///                              appended between distill runs) bucket
///                              into a NEW chain id without forcing the
///                              caller to roll an explicit value.
///
/// The four components are joined with a single `\u{1f}` (US — unit
/// separator) so no caller-supplied substring can collide with the
/// delimiter. The sha256 of the resulting blob is hex-encoded and
/// prefixed with `chain:auto:wf-` to mirror plan.rs's `chain:auto:plan-…`
/// shape; the `wf-` namespace prefix lets audit queries pivot on the
/// recorder origin without re-deriving it from the evidence row.
pub(in crate::handlers::knowledge::workflow) fn derive_auto_chain_id(
    project_root: &Path,
    plan_id: uuid::Uuid,
    workflow_anchor: &str,
    evidence_sha256: &str,
) -> String {
    let project_canonical = project_root.display().to_string();
    let blob = format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        project_canonical, plan_id, workflow_anchor, evidence_sha256
    );
    let mut hasher = Sha256::new();
    hasher.update(blob.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{:02x}", byte));
    }
    format!("chain:auto:wf-{}", hex)
}

/// Compute the sha256 hex of an on-disk evidence sidecar. Returns
/// `Some(hex)` when the file exists AND the bytes hash cleanly; `None`
/// when the sidecar is absent or unreadable. The auto-chain id derivation
/// substitutes the literal `<no-evidence>` for the `None` case so
/// missing-evidence runs still hash to a stable id.
pub(in crate::handlers::knowledge::workflow) fn compute_evidence_sha256(
    path: &Path,
) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return None,
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{:02x}", byte));
    }
    Some(hex)
}

/// Pick the workflow anchor for chain-id derivation. Persisted distill
/// rows expose a fresh UUID via `payload["workflow_id"]`; the dry-run /
/// non-persist path falls back to the caller-supplied `name` (which is
/// the workflow UNIQUE key) or the literal `<unnamed>` so the hash stays
/// well-defined.
pub(in crate::handlers::knowledge::workflow) fn pick_workflow_anchor(
    workflow_id: Option<&str>,
    name: &str,
) -> String {
    if let Some(id) = workflow_id.map(str::trim).filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "<unnamed>".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Splice an `auto_chain` block onto the distill response payload. The
/// block always carries `requested` / `status` / (when applicable)
/// `chain_id` / `chain_id_source` / `chain_id_inputs`. Optional fields
/// (`chain_name`, `evidence_path`, `evidence_error`) are only added when
/// present. Mirrors `plan.rs::attach_distill_chain_to_payload`'s
/// "always-stable shape" contract.
pub(in crate::handlers::knowledge::workflow) fn attach_auto_chain_to_payload(
    payload: &mut Value,
    block: Value,
) {
    if let Some(obj) = payload.as_object_mut() {
        // Top-level shortcuts so callers can pivot without descending into
        // the block — mirrors `distill_chain_status` on plan.rs.
        if let Some(status) = block.get("status").and_then(|v| v.as_str()) {
            obj.insert("auto_chain_status".to_string(), json!(status));
        }
        if let Some(id) = block.get("chain_id").and_then(|v| v.as_str()) {
            obj.insert("auto_chain_id".to_string(), json!(id));
        }
        obj.insert("auto_chain".to_string(), block);
    }
}

/// Compose the auto-chain block surfaced under `payload.auto_chain`.
pub(in crate::handlers::knowledge::workflow) fn build_auto_chain_block(
    requested: bool,
    status: &str,
    chain_id: Option<&str>,
    chain_name: Option<&str>,
    chain_id_inputs: Option<Value>,
    evidence_path: Option<&str>,
    evidence_error: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "status": status,
        "chain_id_source": AUTO_CHAIN_ID_SOURCE_DERIVED,
    });
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    if let Some(name) = chain_name {
        block["chain_name"] = json!(name);
    }
    if let Some(inputs) = chain_id_inputs {
        block["chain_id_inputs"] = inputs;
    }
    if let Some(p) = evidence_path {
        block["evidence_path"] = json!(p);
    }
    if let Some(e) = evidence_error {
        block["evidence_error"] = json!(e);
    }
    block
}

/// Post-distill hook. Returns the original `inner` ToolResult unchanged
/// when `auto_chain=false` (byte-compat); otherwise re-projects the inner
/// payload, computes the deterministic chain id, appends a chain-record
/// evidence row, and splices an `auto_chain` block onto the response.
///
/// Failure surface (per CLAUDE.md fail-fast policy):
///   - Project root resolution failure → `auto_chain.status="resolve_failed"`
///     (the original distill payload is preserved; no chain id is derived
///     because the project root anchors the hash).
///   - Sidecar append failure          → `auto_chain.status="record_failed"`
///     with `evidence_error` set; the chain id IS still derived and
///     surfaced so callers can retry / re-record manually.
///   - Inner result is a structured error → skip auto-chain entirely
///     (matches plan.rs's `apply_distill_chain` policy: never overwrite
///     an error envelope with chain side-effects).
pub(in crate::handlers::knowledge::workflow) async fn maybe_apply_auto_chain(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
) -> ToolResult {
    if !auto_chain_requested(args) {
        return inner;
    }
    if inner.is_error.unwrap_or(false) {
        return inner;
    }

    // Re-project the inner payload so we can splice the chain block
    // alongside the existing distill fields without rebuilding the whole
    // response shape. We borrow plan.rs's `tool_result_payload` helper
    // (`pub(super)`) so the projection rule stays consistent across both
    // chain recorders. Distill always emits a JSON text part; the helper
    // collapses anything else to `Value::Null` / `Value::String`, in
    // which case we return the inner result untouched (no silent payload
    // synthesis).
    let mut payload = plan_handler::tool_result_payload(&inner);
    if !payload.is_object() {
        return inner;
    }

    let chain_name = parse_auto_chain_name(args);

    // Project resolution: we need the canonical absolute root to hash
    // into the chain id AND to anchor the sidecar append.
    let project_root = match resolve_project_root_from_args(state, args).await {
        Ok(p) => p,
        Err(reason) => {
            let block = build_auto_chain_block(
                true,
                AUTO_CHAIN_STATUS_RESOLVE_FAILED,
                None,
                chain_name.as_deref(),
                None,
                None,
                Some(&reason),
            );
            attach_auto_chain_to_payload(&mut payload, block);
            return ToolResult::json_pretty(&payload);
        }
    };

    // Pull the persisted workflow_id from the distill payload when
    // available (persist=true path); fall back to the caller-supplied
    // `name` so dry-run / non-persist runs still produce a stable
    // anchor.
    let workflow_id_str = payload
        .get("workflow_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let workflow_anchor = pick_workflow_anchor(workflow_id_str.as_deref(), name);

    // Hash the on-disk evidence sidecar (when present). Missing sidecar
    // collapses to the literal `<no-evidence>` placeholder so the chain
    // id stays well-defined for callers that distil without an evidence
    // gate (e.g. `allow_missing_evidence=true`).
    let evidence_path = evidence_sidecar_path(&project_root, plan.id);
    let evidence_sha = compute_evidence_sha256(&evidence_path);
    let evidence_sha_for_id = evidence_sha
        .clone()
        .unwrap_or_else(|| "<no-evidence>".to_string());

    let chain_id = derive_auto_chain_id(
        &project_root,
        plan.id,
        &workflow_anchor,
        &evidence_sha_for_id,
    );

    // Inputs surfaced verbatim on the response so audit consumers can
    // re-derive the chain id without scraping the daemon's source. Names
    // mirror the function signature so the mapping is obvious.
    let chain_id_inputs = json!({
        "project_root": project_root.display().to_string(),
        "plan_id": plan.id.to_string(),
        "workflow_anchor": workflow_anchor,
        "evidence_sha256": evidence_sha_for_id,
    });

    // Append exactly ONE chain-record evidence row. Schema mirrors
    // plan.rs's wave-18 chain-record entry so a single audit query
    // (`kind="distill_chain_record"`) sees both recorders' rows; the
    // `source` tag distinguishes which surface produced it.
    let mut entry = evidence_collector::EvidenceEntry::new(
        AUTO_CHAIN_EVIDENCE_SOURCE,
        AUTO_CHAIN_EVIDENCE_KIND,
    )
    .with_state_transition("workflow_distill_auto_chain_appended")
    .with_extra("event_kind", json!("workflow_distill_auto_chain"))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("plan_version", json!(plan.version))
    .with_extra("chain_id", json!(chain_id))
    .with_extra("chain_id_source", json!(AUTO_CHAIN_ID_SOURCE_DERIVED))
    .with_extra("chain_id_inputs", chain_id_inputs.clone())
    .with_extra("workflow_anchor", json!(workflow_anchor))
    .with_extra("workflow_name_arg", json!(name))
    .with_extra(
        "distill_mode",
        payload
            .get("distill_mode")
            .cloned()
            .unwrap_or_else(|| Value::String("dry_run".to_string())),
    );
    if let Some(label) = chain_name.as_deref() {
        entry = entry.with_extra("chain_name", json!(label));
    }
    if let Some(id) = workflow_id_str.as_deref() {
        entry = entry.with_extra("workflow_id", json!(id));
    }
    if let Some(sha) = evidence_sha.as_deref() {
        entry = entry.with_extra("evidence_sha256", json!(sha));
    }

    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    let outcome = evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;

    let (evidence_path_str, evidence_error) = match outcome {
        evidence_collector::AppendOutcome::Written { path, .. } => {
            (Some(path.display().to_string()), None)
        }
        evidence_collector::AppendOutcome::Failed { error } => {
            tracing::warn!(
                plan_id = %plan.id,
                chain_id = %chain_id,
                error = %error,
                "workflow auto_chain: evidence sidecar append failed"
            );
            (None, Some(error))
        }
    };

    let status = if evidence_error.is_some() {
        AUTO_CHAIN_STATUS_RECORD_FAILED
    } else {
        AUTO_CHAIN_STATUS_RECORDED
    };

    let block = build_auto_chain_block(
        true,
        status,
        Some(&chain_id),
        chain_name.as_deref(),
        Some(chain_id_inputs),
        evidence_path_str.as_deref(),
        evidence_error.as_deref(),
    );
    attach_auto_chain_to_payload(&mut payload, block);
    ToolResult::json_pretty(&payload)
}
