use missiond_core::types::PlanStatus;
use missiond_mcp::tools::error_codes;

use crate::handlers::knowledge::review_gate::{
    ParsedReviewQuestionId, PlanNodeResumeInput, ReviewDecision,
};
use crate::state::AppState;

use super::super::{
    build_validated_dag_from_contract_json, dispatch_node, emit_evidence_finished,
    emit_evidence_running, DispatchOutcome, EvidenceCtx, ExecutionOutcome, TaskContractDispatchCtx,
    PLAN_NODE_DEFAULT_ATTEMPT,
};
use super::evidence::emit_resume_decision_evidence;
use super::validation::validate_resume_request;

/// Wave-17 / task 01 — outcome of the listener-side resume bridge,
/// used by `bus::v2_subscribers` to log a structured signal for every
/// inbound plan-node Resolved event without surfacing tool errors on
/// the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanNodeResumeListenerOutcome {
    /// Approved decision — re-dispatched the paused node.
    Dispatched {
        plan_id: uuid::Uuid,
        node_id: String,
        succeeded: bool,
    },
    /// Rejected decision — node stays paused, evidence row recorded.
    KeptPaused {
        plan_id: uuid::Uuid,
        node_id: String,
        decision: &'static str,
    },
    /// Envelope's artifact_id failed to parse as a UUID.
    ArtifactIdNotUuid { artifact_id: String, error: String },
    /// Plan row not found for the qid's artifact_id.
    NotFound { artifact_id: uuid::Uuid },
    /// Validation rejected the resume (id mismatch, hash miss, etc.)
    /// — kept loud for observability.
    ValidationRejected {
        plan_id: uuid::Uuid,
        code: &'static str,
        message: String,
    },
    /// Plan row exists but `plan.sexp_text` failed DAG validation.
    DagBuildFailed { plan_id: uuid::Uuid, detail: String },
    /// Underlying DB / dispatch raised an error.
    DispatchError { plan_id: uuid::Uuid, detail: String },
}

/// Wave-17 / task 01 — listener-side bridge. Called from
/// `bus::v2_subscribers::handle_review_resolved` when the inbound
/// envelope's scope is `plan` AND action is `plan-node`. Performs the
/// same validate / dispatch / evidence cycle as the explicit
/// `action_execute_resume` entry point, but takes the parsed envelope
/// + decision directly (no `args` JSON) and surfaces a structured
/// outcome instead of a `ToolResult`.
///
/// Side effects mirror the explicit path: at most one node dispatch +
/// the evidence rows (`paused -> resume_*` + `ready -> running` +
/// `running -> {succeeded|failed}` for the approved branch). No bus
/// publish of a downstream Resolved event — the inbound Resolved we
/// just consumed IS the downstream signal.
pub(crate) async fn handle_review_resolved_plan_node_event(
    state: &AppState,
    parsed_qid: &ParsedReviewQuestionId,
    decision: ReviewDecision,
) -> PlanNodeResumeListenerOutcome {
    let id = match uuid::Uuid::parse_str(&parsed_qid.artifact_id) {
        Ok(u) => u,
        Err(e) => {
            return PlanNodeResumeListenerOutcome::ArtifactIdNotUuid {
                artifact_id: parsed_qid.artifact_id.clone(),
                error: e.to_string(),
            };
        }
    };
    let plan = match state.store.plan_get(id).await {
        Ok(Some(p)) => p,
        Ok(None) => return PlanNodeResumeListenerOutcome::NotFound { artifact_id: id },
        Err(e) => {
            return PlanNodeResumeListenerOutcome::DispatchError {
                plan_id: id,
                detail: format!("plan_get: {}", e),
            };
        }
    };
    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return PlanNodeResumeListenerOutcome::ValidationRejected {
            plan_id: id,
            code: error_codes::INVALID_PARAM,
            message: format!("plan status `{}` is not executable", plan.status.as_str()),
        };
    }
    let (parsed_dag, _order) = match build_validated_dag_from_contract_json(&plan.contract_json) {
        Ok(v) => v,
        Err(e) => {
            return PlanNodeResumeListenerOutcome::DagBuildFailed {
                plan_id: id,
                detail: format!("{:?}", e),
            };
        }
    };
    let node = match validate_resume_request(parsed_qid, &plan, &parsed_dag) {
        Ok(n) => n.clone(),
        Err(e) => {
            return PlanNodeResumeListenerOutcome::ValidationRejected {
                plan_id: id,
                code: e.code(),
                message: e.message(),
            };
        }
    };
    let dispatch_strategy = node
        .dispatch_strategy
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let ctx = EvidenceCtx {
        plan_id: plan.id,
        plan_version: plan.version,
        project_arg: None,
        cwd_arg: None,
        target_project_arg: None,
    };
    let input = PlanNodeResumeInput {
        question_id: format!(
            "review:{}:{}:v{}:{}",
            parsed_qid.scope, parsed_qid.artifact_id, parsed_qid.version, parsed_qid.action
        ) + &parsed_qid
            .topic_hash
            .as_deref()
            .map(|h| format!(":{}", h))
            .unwrap_or_default(),
        decision,
        actor: None,
        note: None,
    };
    let mut outcome = ExecutionOutcome::default();
    let _ = emit_resume_decision_evidence(
        state,
        &ctx,
        &plan,
        &node,
        &dispatch_strategy,
        &input,
        &mut outcome,
    )
    .await;
    match decision {
        ReviewDecision::Approved => {
            let attempt = PLAN_NODE_DEFAULT_ATTEMPT;
            emit_evidence_running(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                attempt,
                &mut outcome,
            )
            .await;
            // wave-19 / task 06 — listener-driven resumes never see
            // caller args (they fire from a bus event), so the
            // task-contract emitter defaults to Off here. Callers
            // that want a contract emitted must hit the explicit
            // `mission_plan(action=execute, resume_review_question_id=...)`
            // path so they can pass `task_contract_mode`.
            let dispatch_outcome = match dispatch_node(
                state.clone(),
                plan.clone(),
                node.clone(),
                TaskContractDispatchCtx::off(),
            )
            .await
            {
                Ok(o) => o,
                Err(e) => {
                    return PlanNodeResumeListenerOutcome::DispatchError {
                        plan_id: id,
                        detail: e.to_string(),
                    };
                }
            };
            let DispatchOutcome {
                inner_payload,
                classification,
                dispatch_strategy: ds,
                ..
            } = dispatch_outcome;
            let succeeded = classification.is_ok();
            emit_evidence_finished(
                state,
                &ctx,
                &node,
                &ds,
                &inner_payload,
                succeeded,
                attempt,
                &mut outcome,
            )
            .await;
            PlanNodeResumeListenerOutcome::Dispatched {
                plan_id: id,
                node_id: node.id.clone(),
                succeeded,
            }
        }
        ReviewDecision::Rejected => PlanNodeResumeListenerOutcome::KeptPaused {
            plan_id: id,
            node_id: node.id.clone(),
            decision: "rejected",
        },
        ReviewDecision::NeedsChanges => PlanNodeResumeListenerOutcome::KeptPaused {
            plan_id: id,
            node_id: node.id.clone(),
            decision: "needs_changes",
        },
    }
}
