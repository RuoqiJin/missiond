use std::collections::HashMap;

use crate::state::AppState;

use super::super::claim_lease::ClaimRegistry;
use super::super::lifecycle::{emit_evidence_claim_released, EvidenceCtx};
use super::super::outcome::ExecutionOutcome;
use super::super::parser::DagNode;

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
