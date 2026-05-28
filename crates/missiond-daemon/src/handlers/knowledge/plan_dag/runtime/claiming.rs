use std::collections::HashMap;

use anyhow::Result;
use serde_json::{json, Value};

use crate::engine::control_plane_kernel::{
    ClaimLeaseCommand, ControlPlaneKernel, ReleaseLeaseCommand,
};
use crate::state::AppState;

use super::super::claim_lease::{
    derive_node_claim_scopes, derive_plan_dag_claim_id, ClaimAcquire, ClaimRegistry, PlanDagClaim,
};
use super::super::lifecycle::{emit_evidence_claim_conflict, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult, NodeState};
use super::super::parser::{DagNode, FAILURE_POLICY_FAIL_FAST};
use super::super::scheduler::propagate_taint;
use super::claims::record_acquired_claim;

pub(super) enum DispatchClaimDecision {
    Dispatch,
    ConflictFailed { fail_fast_abort: bool },
}

pub(super) async fn prepare_dispatch_claim<'a>(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    claim_lease_secs: i64,
    claimer_name: &str,
    _enforce_claims: bool,
    claim_registry: &mut ClaimRegistry,
    active_claims_by_node: &mut HashMap<String, String>,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    results_by_id: &mut HashMap<String, NodeResult>,
    tainted_by: &mut HashMap<String, String>,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    outcome: &mut ExecutionOutcome,
) -> Result<DispatchClaimDecision> {
    let (scopes, scope_source) = derive_node_claim_scopes(node, plan_id);
    let claim_id = derive_plan_dag_claim_id(plan_id, &node.id, attempt);
    let acquire_outcome = acquire_plan_dag_work_leases(
        state,
        ctx,
        plan_id,
        node,
        attempt,
        claim_id,
        claimer_name,
        scopes,
        scope_source,
        claim_lease_secs,
    )
    .await?;

    match acquire_outcome {
        ClaimAcquire::Acquired(claim) => {
            claim_registry.record_acquired(claim.clone());
            record_acquired_claim(
                state,
                ctx,
                node,
                dispatch_strategy,
                attempt,
                &claim,
                lifecycle,
                active_claims_by_node,
                outcome,
            )
            .await;
            Ok(DispatchClaimDecision::Dispatch)
        }
        ClaimAcquire::Conflict {
            attempted_claim_id,
            attempted_scopes,
            attempted_scope_source,
            conflicting_claim_id,
            conflicting_claimer,
            conflicting_scope,
            offending_scope,
        } => Ok(record_strict_claim_conflict(
            state,
            ctx,
            node,
            dispatch_strategy,
            attempt,
            attempted_claim_id,
            attempted_scopes,
            attempted_scope_source,
            conflicting_claim_id,
            conflicting_claimer,
            conflicting_scope,
            offending_scope,
            lifecycle,
            results_by_id,
            tainted_by,
            succs,
            outcome,
        )
        .await),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn acquire_plan_dag_work_leases(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan_id: uuid::Uuid,
    node: &DagNode,
    attempt: u32,
    claim_id: String,
    claimer_name: &str,
    scopes: Vec<String>,
    scope_source: &'static str,
    claim_lease_secs: i64,
) -> Result<ClaimAcquire> {
    let acquired_at = chrono::Utc::now();
    let lease_expires_at = acquired_at + chrono::Duration::seconds(claim_lease_secs);
    let mut work_lease_ids = Vec::new();
    for scope in &scopes {
        let response = ControlPlaneKernel::new(state)
            .claim_lease_command(ClaimLeaseCommand {
                project_id: ctx.project_arg.map(str::to_string),
                task_id: Some(format!("plan-dag:{}:{}:{}", plan_id, node.id, attempt)),
                owner_id: claimer_name.to_string(),
                grant_id: None,
                subject_kind: "system".to_string(),
                subject_id: "plan-dag-scheduler".to_string(),
                scope_kind: plan_dag_scope_kind(scope_source).to_string(),
                scope_key: scope.clone(),
                lease_secs: claim_lease_secs,
                metadata: json!({
                    "schema": "missiond.plan-dag-work-lease.v1",
                    "source": "plan_dag_runtime",
                    "plan_id": plan_id,
                    "node_id": &node.id,
                    "attempt": attempt,
                    "plan_dag_claim_id": &claim_id,
                    "claim_scopes": &scopes,
                    "claim_scope_source": scope_source,
                    "claim_scope_key": scope,
                }),
                allow_system_bypass: true,
                bypass_reason: Some("plan DAG scheduler work_leases authority".to_string()),
            })
            .await?;
        if response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            if let Some(lease_id) = response
                .get("claim")
                .and_then(|claim| claim.get("id"))
                .and_then(Value::as_str)
            {
                work_lease_ids.push(lease_id.to_string());
            }
            continue;
        }
        release_partial_plan_dag_work_leases(state, claimer_name, &work_lease_ids).await;
        let conflict = response
            .get("conflict")
            .cloned()
            .unwrap_or_else(|| json!({}));
        return Ok(ClaimAcquire::Conflict {
            attempted_claim_id: claim_id,
            attempted_scopes: scopes.clone(),
            attempted_scope_source: scope_source,
            conflicting_claim_id: conflict_string(&conflict, "id", "unknown"),
            conflicting_claimer: conflict_string(&conflict, "owner_id", "unknown"),
            conflicting_scope: conflict_string(&conflict, "scope_key", "unknown"),
            offending_scope: scope.clone(),
        });
    }
    Ok(ClaimAcquire::Acquired(PlanDagClaim {
        claim_id,
        work_lease_ids,
        claimer: claimer_name.to_string(),
        scopes,
        scope_source,
        acquired_at,
        lease_expires_at,
        released_at: None,
    }))
}

async fn release_partial_plan_dag_work_leases(
    state: &AppState,
    claimer_name: &str,
    work_lease_ids: &[String],
) {
    for lease_id in work_lease_ids {
        let _ = ControlPlaneKernel::new(state)
            .release_lease_command(ReleaseLeaseCommand {
                claim_id: lease_id.clone(),
                owner_id: Some(claimer_name.to_string()),
                grant_id: None,
                subject_kind: "system".to_string(),
                subject_id: "plan-dag-scheduler".to_string(),
                details: json!({
                    "source": "plan_dag_runtime",
                    "reason": "partial plan DAG claim rollback",
                }),
                allow_system_bypass: true,
                bypass_reason: Some("plan DAG scheduler partial lease rollback".to_string()),
            })
            .await;
    }
}

fn plan_dag_scope_kind(scope_source: &'static str) -> &'static str {
    match scope_source {
        super::super::claim_lease::CLAIM_SCOPE_SOURCE_OWNED_FILES => "write_scope",
        super::super::claim_lease::CLAIM_SCOPE_SOURCE_SCOPE => "plan_dag_scope",
        _ => "plan_dag_node",
    }
}

fn conflict_string(conflict: &Value, key: &str, fallback: &str) -> String {
    conflict
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

#[allow(clippy::too_many_arguments)]
async fn record_strict_claim_conflict<'a>(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    attempted_claim_id: String,
    attempted_scopes: Vec<String>,
    attempted_scope_source: &'static str,
    conflicting_claim_id: String,
    conflicting_claimer: String,
    conflicting_scope: String,
    offending_scope: String,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    results_by_id: &mut HashMap<String, NodeResult>,
    tainted_by: &mut HashMap<String, String>,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    outcome: &mut ExecutionOutcome,
) -> DispatchClaimDecision {
    lifecycle.insert(node.id.clone(), NodeLifecycle::Failed);
    emit_evidence_claim_conflict(
        state,
        ctx,
        node,
        dispatch_strategy,
        attempt,
        &attempted_claim_id,
        &attempted_scopes,
        attempted_scope_source,
        &conflicting_claim_id,
        &conflicting_claimer,
        &conflicting_scope,
        &offending_scope,
        outcome,
    )
    .await;
    let reason = format!(
        "CLAIM_CONFLICT: scope `{}` overlaps active claim {} held by `{}` over `{}`",
        offending_scope, conflicting_claim_id, conflicting_claimer, conflicting_scope
    );
    let inner_payload = json!({
        "error": reason.clone(),
        "claim_status": "conflict",
        "attempted_claim_id": attempted_claim_id,
        "attempted_claim_scopes": attempted_scopes,
        "attempted_claim_scope_source": attempted_scope_source,
        "conflicting_claim_id": conflicting_claim_id,
        "conflicting_claimer": conflicting_claimer,
        "conflicting_scope": conflicting_scope,
        "offending_scope": offending_scope,
    });
    results_by_id.insert(
        node.id.clone(),
        NodeResult {
            id: node.id.clone(),
            target: node.target.clone(),
            state: NodeState::Failed { reason },
            dispatch_strategy: dispatch_strategy.to_string(),
            inner_payload,
            attempts_made: attempt,
            max_attempts: node.effective_max_attempts(),
            retry_skipped_non_retryable: true,
            rollback: None,
            acceptance: None,
        },
    );
    propagate_taint(node, succs, tainted_by);
    DispatchClaimDecision::ConflictFailed {
        fail_fast_abort: node.failure_policy == FAILURE_POLICY_FAIL_FAST,
    }
}
