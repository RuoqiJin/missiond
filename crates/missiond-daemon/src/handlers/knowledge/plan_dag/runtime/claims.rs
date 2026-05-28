use std::collections::HashMap;

use serde_json::json;

use crate::engine::control_plane_kernel::{ControlPlaneKernel, ReleaseLeaseCommand};
use crate::state::AppState;

use super::super::claim_lease::{ClaimRegistry, PlanDagClaim};
use super::super::lifecycle::{emit_evidence_claim_released, emit_evidence_claimed, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle};
use super::super::parser::DagNode;

pub(super) async fn record_acquired_claim(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    claim: &PlanDagClaim,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    active_claims_by_node: &mut HashMap<String, String>,
    outcome: &mut ExecutionOutcome,
) {
    lifecycle.insert(node.id.clone(), NodeLifecycle::Claimed);
    emit_evidence_claimed(
        state,
        ctx,
        node,
        dispatch_strategy,
        attempt,
        claim,
        "acquired",
        outcome,
    )
    .await;
    active_claims_by_node.insert(node.id.clone(), claim.claim_id.clone());
}

pub(super) async fn release_claim_if_recorded(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    terminal_state_label: &str,
    claim_registry: &mut ClaimRegistry,
    active_claims_by_node: &mut HashMap<String, String>,
    outcome: &mut ExecutionOutcome,
) {
    if let Some(claim_id) = active_claims_by_node.remove(&node.id) {
        if let Some(released) = claim_registry.release(&claim_id, chrono::Utc::now()) {
            for lease_id in &released.work_lease_ids {
                let _ = ControlPlaneKernel::new(state)
                    .release_lease_command(ReleaseLeaseCommand {
                        claim_id: lease_id.clone(),
                        owner_id: Some(released.claimer.clone()),
                        grant_id: None,
                        subject_kind: "system".to_string(),
                        subject_id: "plan-dag-scheduler".to_string(),
                        details: json!({
                            "source": "plan_dag_runtime",
                            "plan_dag_claim_id": &released.claim_id,
                            "node_id": &node.id,
                            "terminal_state": terminal_state_label,
                        }),
                        allow_system_bypass: true,
                        bypass_reason: Some(
                            "plan DAG scheduler releases work_leases authority".to_string(),
                        ),
                    })
                    .await;
            }
            emit_evidence_claim_released(
                state,
                ctx,
                node,
                dispatch_strategy,
                attempt,
                &released,
                terminal_state_label,
                outcome,
            )
            .await;
        }
    }
}
