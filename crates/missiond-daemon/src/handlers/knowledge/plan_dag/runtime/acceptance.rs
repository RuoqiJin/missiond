use std::collections::HashMap;

use missiond_core::types::Plan;
use serde_json::Value;

use crate::state::AppState;

use super::super::acceptance::{
    apply_acceptance_fan_in, derive_acceptance_pause_id, evaluate_node_acceptance,
    AcceptanceEvaluation, AcceptanceStatus,
};
use super::super::lifecycle::{emit_evidence_acceptance, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult, NodeState};
use super::super::parser::DagNode;

pub(super) struct SuccessAcceptanceOutcome {
    pub(super) acceptance: AcceptanceEvaluation,
    pub(super) acceptance_active: bool,
    pub(super) next_lifecycle: NodeLifecycle,
    pub(super) next_node_state: NodeState,
    pub(super) terminal_state_label: &'static str,
}

impl SuccessAcceptanceOutcome {
    pub(super) fn is_rejected(&self) -> bool {
        matches!(self.acceptance.status, AcceptanceStatus::Rejected)
    }
}

pub(super) async fn evaluate_success_acceptance(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    inner_payload: &Value,
    results_by_id: &HashMap<String, NodeResult>,
    outcome: &mut ExecutionOutcome,
) -> SuccessAcceptanceOutcome {
    let acceptance_base = evaluate_node_acceptance(node, inner_payload, true);
    let prior_results_view: HashMap<String, &NodeResult> =
        results_by_id.iter().map(|(k, v)| (k.clone(), v)).collect();
    let acceptance = apply_acceptance_fan_in(acceptance_base, node, &prior_results_view);
    let acceptance_active = !acceptance.is_inactive();
    if acceptance_active {
        emit_evidence_acceptance(
            state,
            ctx,
            node,
            dispatch_strategy,
            attempt,
            &acceptance,
            outcome,
        )
        .await;
    }
    let terminal_state_label = match acceptance.status {
        AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => "succeeded",
        AcceptanceStatus::Rejected => "failed",
        AcceptanceStatus::ManualRequired => "paused",
    };
    let next_node_state = match acceptance.status {
        AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => NodeState::Succeeded,
        AcceptanceStatus::Rejected => NodeState::Failed {
            reason: format!("acceptance_rejected: {}", acceptance.reason),
        },
        AcceptanceStatus::ManualRequired => {
            let question_id = derive_acceptance_pause_id(plan.id, plan.version, &node.id);
            NodeState::Paused {
                question_id,
                bus_publish_warning: None,
            }
        }
    };
    let next_lifecycle = match acceptance.status {
        AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => NodeLifecycle::Succeeded,
        AcceptanceStatus::Rejected => NodeLifecycle::Failed,
        AcceptanceStatus::ManualRequired => NodeLifecycle::Paused,
    };
    SuccessAcceptanceOutcome {
        acceptance,
        acceptance_active,
        next_lifecycle,
        next_node_state,
        terminal_state_label,
    }
}
