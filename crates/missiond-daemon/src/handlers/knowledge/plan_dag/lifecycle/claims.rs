use super::super::claim_lease::PlanDagClaim;
use super::super::{DagNode, ExecutionOutcome};
use super::{publish_plan_node_state_change, EvidenceCtx};
use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;
use serde_json::json;

/// wave-17 / task 02 — emit a `pending -> claimed` evidence row + bus
/// event for a node whose canonical work_leases claim was successfully
/// registered. The transition always runs BEFORE `ready -> running` so
/// observers can pivot on the claim metadata without reconstructing it
/// from the running row.
///
/// `claim_status` is one of:
///   * `"acquired"` — work_leases accepted the claim with no active
///                    conflicting holder.
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claimed(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    claim: &PlanDagClaim,
    claim_status: &str,
    outcome: &mut ExecutionOutcome,
) {
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "pending",
        "claimed",
        Some(format!(
            "claim:{}:{}:{}",
            claim.claim_id, claim.claimer, claim_status
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> claimed")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("claim_id", json!(claim.claim_id))
    .with_extra("work_lease_ids", json!(&claim.work_lease_ids))
    .with_extra("claimer", json!(claim.claimer))
    .with_extra("claim_scopes", json!(claim.scopes))
    .with_extra("claim_scope_source", json!(claim.scope_source))
    .with_extra("claim_acquired_at", json!(claim.acquired_at_iso()))
    .with_extra(
        "claim_lease_expires_at",
        json!(claim.lease_expires_at_iso()),
    )
    .with_extra("claim_status", json!(claim_status));
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->claimed evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 02 — emit a `claimed -> released` evidence row and
/// best-effort bus event after the wave loop reaches a terminal state
/// for the node and releases its registry record. Stamps the
/// `released_at` ISO timestamp + the original lease bounds so audit
/// dashboards can compute the actual hold duration without rejoining
/// the prior `pending -> claimed` row.
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claim_released(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    claim: &PlanDagClaim,
    terminal_state: &str,
    outcome: &mut ExecutionOutcome,
) {
    let released_iso = claim
        .released_at_iso()
        .unwrap_or_else(|| claim.acquired_at_iso());
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "claimed",
        "released",
        Some(format!(
            "release:{}:{}:after-{}",
            claim.claim_id, claim.claimer, terminal_state
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("claimed -> released")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("claim_id", json!(claim.claim_id))
    .with_extra("work_lease_ids", json!(&claim.work_lease_ids))
    .with_extra("claimer", json!(claim.claimer))
    .with_extra("claim_scopes", json!(claim.scopes))
    .with_extra("claim_scope_source", json!(claim.scope_source))
    .with_extra("claim_acquired_at", json!(claim.acquired_at_iso()))
    .with_extra(
        "claim_lease_expires_at",
        json!(claim.lease_expires_at_iso()),
    )
    .with_extra("claim_released_at", json!(released_iso))
    .with_extra("claim_terminal_state", json!(terminal_state));
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: claimed->released evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 02 — emit a `pending -> failed` evidence row for a
/// node refused at the canonical work_leases claim gate. The inner
/// handler is NEVER invoked; the node fails fast with a structured
/// `CLAIM_CONFLICT` reason so audit dashboards can pivot on the
/// dedicated `claim_conflict` skip tag.
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_claim_conflict(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    attempted_claim_id: &str,
    attempted_scopes: &[String],
    attempted_scope_source: &str,
    conflicting_claim_id: &str,
    conflicting_claimer: &str,
    conflicting_scope: &str,
    offending_scope: &str,
    outcome: &mut ExecutionOutcome,
) {
    let reason = format!(
        "CLAIM_CONFLICT: scope `{}` overlaps active claim {} held by `{}` over `{}`",
        offending_scope, conflicting_claim_id, conflicting_claimer, conflicting_scope
    );
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "pending",
        "failed",
        Some(reason.clone()),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> failed")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("skip_reason", json!("claim_conflict"))
    .with_extra("claim_status", json!("conflict"))
    .with_extra("attempted_claim_id", json!(attempted_claim_id))
    .with_extra("attempted_claim_scopes", json!(attempted_scopes))
    .with_extra(
        "attempted_claim_scope_source",
        json!(attempted_scope_source),
    )
    .with_extra("conflicting_claim_id", json!(conflicting_claim_id))
    .with_extra("conflicting_claimer", json!(conflicting_claimer))
    .with_extra("conflicting_scope", json!(conflicting_scope))
    .with_extra("offending_scope", json!(offending_scope))
    .with_extra("inner_error", json!({ "error": reason }));
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->failed (claim conflict) evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}
