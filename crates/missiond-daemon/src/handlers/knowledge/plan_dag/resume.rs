use anyhow::Result;
use missiond_core::types::{Plan, PlanStatus};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::state::AppState;

use super::super::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use super::{
    build_validated_dag, dispatch_node, emit_evidence_finished, emit_evidence_running,
    publish_plan_node_state_change, DagNode, DispatchOutcome, EvidenceCtx, ExecutionOutcome,
    ParsedDag, ReviewGateKind, TaskContractDispatchCtx, PLAN_NODE_DEFAULT_ATTEMPT,
};

/// Wave-17 / task 01 — pure failure modes for the resume validator. Each
/// variant maps to a structured `ToolError` at the action_execute_resume
/// boundary so callers see actionable error codes instead of opaque
/// anyhow strings. Listener-side bridge logs the same vocabulary for
/// observability without surfacing tool errors to the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge) enum PlanNodeResumeError {
    /// Supplied id failed the wave-14 envelope parser. Caller surfaces
    /// `REVIEW_ID_MALFORMED`.
    IdMalformed { detail: String },
    /// Supplied id is not a plan-node review (either scope != "plan" or
    /// action != "plan-node"). Resume only handles the deterministic
    /// plan-node path; other ids must travel through the wave-15
    /// manager-side resolution input.
    NotPlanNodeId { scope: String, action: String },
    /// Supplied id targets a different plan than the one being executed.
    PlanIdMismatch { expected: String, actual: String },
    /// Supplied id targets a different plan version than the one in the
    /// DB. The author must rebuild the resume request against the
    /// current plan version (the original paused-node lifecycle was
    /// stamped against an older PLAN.lisp shape).
    StaleVersion { expected: i32, actual_in_id: i32 },
    /// `topic_hash` slot is empty — wave-16 / task 04 always populates
    /// it. Defensive against authors hand-rolling a malformed id.
    MissingTopicHash,
    /// `topic_hash` did not map to any node carrying
    /// `:review-gate "question-event"` in the current PLAN.lisp. Either
    /// the node was renamed / removed since pause, or the gate hint
    /// was stripped — either way we refuse to dispatch a phantom node.
    NoMatchingPausedNode { topic_hash: String },
    /// `topic_hash` matched more than one paused-eligible node. Should
    /// be impossible (SHA-256 collision space is 64 bits over the node
    /// ids in a single plan) but kept loud so a future bug surfaces.
    AmbiguousPausedNode {
        topic_hash: String,
        candidates: Vec<String>,
    },
    /// PLAN.lisp body failed `build_validated_dag` (e.g. cycle / unknown
    /// target). Resume cannot revive a node from an unparseable plan.
    DagBuildFailed { detail: String },
    /// Plan status is outside the executable set (`approved` /
    /// `executing`). Authors must re-approve / re-mark the plan before
    /// resuming a paused node.
    PlanStatusNotExecutable { status: String },
}

impl PlanNodeResumeError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            PlanNodeResumeError::IdMalformed { .. } => "REVIEW_ID_MALFORMED",
            PlanNodeResumeError::NotPlanNodeId { .. } => "REVIEW_ACTION_UNSUPPORTED",
            PlanNodeResumeError::PlanIdMismatch { .. } => "REVIEW_ARTIFACT_MISMATCH",
            PlanNodeResumeError::StaleVersion { .. } => "STALE_REVIEW_VERSION",
            PlanNodeResumeError::MissingTopicHash => "REVIEW_ID_MALFORMED",
            PlanNodeResumeError::NoMatchingPausedNode { .. } => error_codes::NOT_FOUND,
            PlanNodeResumeError::AmbiguousPausedNode { .. } => error_codes::INVALID_PARAM,
            PlanNodeResumeError::DagBuildFailed { .. } => error_codes::INVALID_PARAM,
            PlanNodeResumeError::PlanStatusNotExecutable { .. } => error_codes::INVALID_PARAM,
        }
    }

    pub(super) fn message(&self) -> String {
        match self {
            PlanNodeResumeError::IdMalformed { detail } => {
                format!("resume_review_question_id is malformed: {}", detail)
            }
            PlanNodeResumeError::NotPlanNodeId { scope, action } => format!(
                "resume_review_question_id must encode scope=plan and action=plan-node \
                 (got scope=`{}` action=`{}`); use review_question_id + review_decision \
                 for manager-side resolution",
                scope, action
            ),
            PlanNodeResumeError::PlanIdMismatch { expected, actual } => format!(
                "resume_review_question_id targets plan `{}` but execute called against plan `{}`",
                actual, expected
            ),
            PlanNodeResumeError::StaleVersion {
                expected,
                actual_in_id,
            } => format!(
                "resume_review_question_id targets version `v{}` but plan is at `v{}` \
                 — recompile / re-pause the gate against the current version",
                actual_in_id, expected
            ),
            PlanNodeResumeError::MissingTopicHash => {
                "resume_review_question_id is missing the trailing :node-hash segment".to_string()
            }
            PlanNodeResumeError::NoMatchingPausedNode { topic_hash } => format!(
                "no node carrying `:review-gate \"question-event\"` in the current plan \
                 maps to node-hash `{}` — either the node was renamed/removed since the \
                 pause emitted, or the gate hint was stripped",
                topic_hash
            ),
            PlanNodeResumeError::AmbiguousPausedNode {
                topic_hash,
                candidates,
            } => format!(
                "node-hash `{}` matched more than one paused-eligible node ({:?}); \
                 SHA-256 collision over node ids — this should never happen",
                topic_hash, candidates
            ),
            PlanNodeResumeError::DagBuildFailed { detail } => {
                format!("plan.sexp_text failed DAG validation: {}", detail)
            }
            PlanNodeResumeError::PlanStatusNotExecutable { status } => format!(
                "plan status `{}` is not executable; approve / mark to executing first",
                status
            ),
        }
    }
}

/// Wave-17 / task 01 — pure validator. Locates the unique paused-eligible
/// node a resume request targets WITHOUT touching DB or bus. Pulled out
/// of `action_execute_resume` so unit tests can pin the matrix
/// (id-malformed / plan-id mismatch / stale version / hash miss / hash
/// matched a non-paused node) without standing up an `AppState`.
///
/// The "paused-eligible" predicate is `review_gate_kind() == QuestionEvent`
/// — the same predicate the wave-16 / task 04 scheduler used at dispatch
/// time. This is the only signal we have because paused state is per-call
/// (no DB column); the resume helper is therefore stateless on the
/// execute side.
pub(in crate::handlers::knowledge) fn validate_resume_request<'a>(
    parsed_qid: &super::super::review_gate::ParsedReviewQuestionId,
    plan: &Plan,
    parsed_dag: &'a ParsedDag,
) -> std::result::Result<&'a DagNode, PlanNodeResumeError> {
    if parsed_qid.scope != "plan"
        || !super::super::review_gate::is_plan_node_review_action(&parsed_qid.action)
    {
        return Err(PlanNodeResumeError::NotPlanNodeId {
            scope: parsed_qid.scope.clone(),
            action: parsed_qid.action.clone(),
        });
    }
    if parsed_qid.artifact_id != plan.id.to_string() {
        return Err(PlanNodeResumeError::PlanIdMismatch {
            expected: plan.id.to_string(),
            actual: parsed_qid.artifact_id.clone(),
        });
    }
    if parsed_qid.version != plan.version {
        return Err(PlanNodeResumeError::StaleVersion {
            expected: plan.version,
            actual_in_id: parsed_qid.version,
        });
    }
    let topic_hash = parsed_qid
        .topic_hash
        .as_deref()
        .ok_or(PlanNodeResumeError::MissingTopicHash)?;
    let mut matches: Vec<&DagNode> = Vec::new();
    for n in &parsed_dag.nodes {
        if !matches!(n.review_gate_kind(), ReviewGateKind::QuestionEvent) {
            continue;
        }
        let h = super::super::review_gate::derive_plan_node_topic_hash(&n.id);
        if h == topic_hash {
            matches.push(n);
        }
    }
    match matches.len() {
        0 => Err(PlanNodeResumeError::NoMatchingPausedNode {
            topic_hash: topic_hash.to_string(),
        }),
        1 => Ok(matches[0]),
        _ => Err(PlanNodeResumeError::AmbiguousPausedNode {
            topic_hash: topic_hash.to_string(),
            candidates: matches.iter().map(|n| n.id.clone()).collect(),
        }),
    }
}

/// Wave-17 / task 01 — single-node resume entrypoint invoked from
/// `mission_plan(action=execute)` when the caller supplies the
/// `resume_review_question_id` field set. Performs the validate /
/// dispatch / evidence cycle for ONE paused node and surfaces the
/// outcome on the response payload.
///
/// Only the targeted node is touched. Downstream nodes that were
/// pending after the original paused dispatch stay pending — the
/// caller is expected to drive a follow-up `mission_plan(execute)`
/// call to advance them. This conservative scope matches the wave-17
/// brief: "Only resume existing paused node state. No broad PLAN
/// reinterpretation."
pub(in crate::handlers::knowledge) async fn action_execute_resume(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    input: super::super::review_gate::PlanNodeResumeInput,
) -> Result<ToolResult> {
    use super::super::review_gate::{parse_review_question_id_struct, ReviewDecision};

    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return Ok(resume_error_to_tool_result(
            PlanNodeResumeError::PlanStatusNotExecutable {
                status: plan.status.as_str().to_string(),
            },
        ));
    }

    let parsed_qid = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(resume_error_to_tool_result(
                PlanNodeResumeError::IdMalformed {
                    detail: e.message(),
                },
            ))
        }
    };

    let (parsed_dag, _order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => {
            return Ok(resume_error_to_tool_result(
                PlanNodeResumeError::DagBuildFailed {
                    detail: format!("{:?}", e),
                },
            ))
        }
    };

    let node = match validate_resume_request(&parsed_qid, plan, &parsed_dag) {
        Ok(n) => n.clone(),
        Err(e) => return Ok(resume_error_to_tool_result(e)),
    };

    let dispatch_strategy = node
        .dispatch_strategy
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let ctx = EvidenceCtx {
        plan_id: plan.id,
        plan_version: plan.version,
        project_arg: args.get("project").and_then(|v| v.as_str()),
        cwd_arg: args.get("cwd").and_then(|v| v.as_str()),
        target_project_arg: args.get("target_project").and_then(|v| v.as_str()),
    };

    // Evidence is recorded for EVERY decision (approved/rejected/
    // needs_changes) so the audit trail captures the resume even when
    // we refuse to dispatch.
    let mut outcome = ExecutionOutcome::default();
    let resume_event_ref = emit_resume_decision_evidence(
        state,
        &ctx,
        plan,
        &node,
        &dispatch_strategy,
        &input,
        &mut outcome,
    )
    .await;

    let mut payload = json!({
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "node_id": node.id,
        "review_question_id": input.question_id,
        "review_decision": input.decision.as_str(),
        "review_resume": true,
    });
    if let Some(actor) = input.actor.as_deref() {
        payload["resume_actor"] = json!(actor);
    }
    if let Some(note) = input.note.as_deref() {
        payload["resume_note"] = json!(note);
    }
    if let Some(ref_event_id) = resume_event_ref.as_deref() {
        payload["resume_event_id"] = json!(ref_event_id);
    }

    match input.decision {
        ReviewDecision::Approved => {
            // Fresh attempt 1: paused is non-terminal (no failed
            // attempt was consumed by the gate emit), so the resume
            // dispatch is conceptually a brand-new run of the node.
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
            // wave-19 / task 06 — resume path also honours the
            // task-contract knobs from the resume call. We re-build
            // the ctx from `args` because the paused-node code path
            // never threaded the original DAG run's ctx through —
            // and that's the right semantic: the resume request is a
            // fresh execute call so it gets to set its own contract
            // policy.
            let resume_task_contract_ctx = match TaskContractDispatchCtx::from_args(args) {
                Ok(c) => c,
                Err(err) => return Ok(err),
            };
            let dispatch_outcome = match dispatch_node(
                state.clone(),
                plan.clone(),
                node.clone(),
                resume_task_contract_ctx,
            )
            .await
            {
                Ok(o) => o,
                Err(e) => {
                    return Err(e);
                }
            };
            let DispatchOutcome {
                node_id: _,
                target,
                dispatch_strategy: ds,
                inner_payload,
                classification,
                non_retryable: _,
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
            payload["status"] = json!(if succeeded {
                "resume_dispatched"
            } else {
                "resume_failed"
            });
            payload["target"] = json!(target);
            payload["dispatch_strategy"] = json!(ds);
            payload["inner_result"] = inner_payload;
            payload["attempt"] = json!(attempt);
            if let Err(reason) = classification {
                payload["reason"] = json!(reason);
            }
        }
        ReviewDecision::Rejected => {
            payload["status"] = json!("resume_rejected");
            payload["target"] = json!(node.target);
            payload["dispatch_strategy"] = json!(dispatch_strategy);
            payload["next_step"] = json!(format!(
                "node `{}` remains paused; recompile the plan or supply \
                 review_decision=approved to dispatch it",
                node.id
            ));
        }
        ReviewDecision::NeedsChanges => {
            payload["status"] = json!("resume_needs_changes");
            payload["target"] = json!(node.target);
            payload["dispatch_strategy"] = json!(dispatch_strategy);
            payload["next_step"] = json!(format!(
                "rework node `{}` per `resume_note`, recompile the plan, \
                 then resume against the new node-hash",
                node.id
            ));
        }
    }

    if let Some(p) = outcome.evidence_path.as_deref() {
        payload["evidence_path"] = json!(p);
    }
    if let Some(e) = outcome.evidence_error.as_deref() {
        payload["evidence_error"] = json!(e);
    }
    if !outcome.bus_publish_warnings.is_empty() {
        payload["bus_publish_warnings"] = json!(outcome.bus_publish_warnings);
    }

    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-17 / task 01 — convert a resume validation failure into the
/// canonical `ToolResult::structured_error` shape so the
/// `mission_plan(action=execute)` boundary always surfaces actionable
/// error codes.
fn resume_error_to_tool_result(err: PlanNodeResumeError) -> ToolResult {
    ToolResult::structured_error(ToolError::new(err.code(), err.message()))
}

/// Wave-17 / task 01 — emit a single `paused -> review_resolved`
/// evidence row capturing the resume decision (approved / rejected /
/// needs_changes) for the audit trail. Always runs, regardless of
/// whether we go on to dispatch the node, so the row records the
/// human / operator intent even when the decision keeps the node
/// paused.
///
/// Returns `Some(event_id)` when a `PlanNodeStateChanged` lifecycle
/// event was published (or fell back to the deterministic id) so the
/// caller can splice it onto the response under `resume_event_id`.
/// Returns `None` only when the `EventRef` builder yielded an
/// `unavailable` ref — currently unreachable but kept loose so the
/// helper stays decoupled from ref-availability assumptions.
async fn emit_resume_decision_evidence(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    dispatch_strategy: &str,
    input: &super::super::review_gate::PlanNodeResumeInput,
    outcome: &mut ExecutionOutcome,
) -> Option<String> {
    let to_state = match input.decision {
        super::super::review_gate::ReviewDecision::Approved => "resume_approved",
        super::super::review_gate::ReviewDecision::Rejected => "resume_rejected",
        super::super::review_gate::ReviewDecision::NeedsChanges => "resume_needs_changes",
    };
    let attempt = PLAN_NODE_DEFAULT_ATTEMPT;
    let (event_ref, lifecycle_warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "paused",
        to_state,
        Some(format!(
            "review_resume:{}:qid={}",
            input.decision.as_str(),
            input.question_id
        )),
    )
    .await;
    if let Some(w) = lifecycle_warning {
        outcome.bus_publish_warnings.push(w);
    }
    let event_id = event_ref.event_id.clone();

    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition(format!("paused -> {}", to_state))
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("review_resume", json!(true))
    .with_extra("review_question_id", json!(input.question_id))
    .with_extra("review_decision", json!(input.decision.as_str()))
    .with_extra("plan_version", json!(plan.version));
    if let Some(actor) = input.actor.as_deref() {
        entry = entry.with_extra("resume_actor", json!(actor));
    }
    if let Some(note) = input.note.as_deref() {
        entry = entry.with_extra("resume_note", json!(note));
    }
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
            plan_id = %ctx.plan_id,
            node_id = %node.id,
            decision = %input.decision.as_str(),
            error = %error,
            "DAG resume: paused->review_* evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
    event_id
}

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
    parsed_qid: &super::super::review_gate::ParsedReviewQuestionId,
    decision: super::super::review_gate::ReviewDecision,
) -> PlanNodeResumeListenerOutcome {
    let id = match uuid::Uuid::parse_str(&parsed_qid.artifact_id) {
        Ok(u) => u,
        Err(e) => {
            return PlanNodeResumeListenerOutcome::ArtifactIdNotUuid {
                artifact_id: parsed_qid.artifact_id.clone(),
                error: e.to_string(),
            }
        }
    };
    let plan = match state.store.plan_get(id).await {
        Ok(Some(p)) => p,
        Ok(None) => return PlanNodeResumeListenerOutcome::NotFound { artifact_id: id },
        Err(e) => {
            return PlanNodeResumeListenerOutcome::DispatchError {
                plan_id: id,
                detail: format!("plan_get: {}", e),
            }
        }
    };
    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return PlanNodeResumeListenerOutcome::ValidationRejected {
            plan_id: id,
            code: error_codes::INVALID_PARAM,
            message: format!("plan status `{}` is not executable", plan.status.as_str()),
        };
    }
    let (parsed_dag, _order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => {
            return PlanNodeResumeListenerOutcome::DagBuildFailed {
                plan_id: id,
                detail: format!("{:?}", e),
            }
        }
    };
    let node = match validate_resume_request(parsed_qid, &plan, &parsed_dag) {
        Ok(n) => n.clone(),
        Err(e) => {
            return PlanNodeResumeListenerOutcome::ValidationRejected {
                plan_id: id,
                code: e.code(),
                message: e.message(),
            }
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
    let input = super::super::review_gate::PlanNodeResumeInput {
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
        super::super::review_gate::ReviewDecision::Approved => {
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
                    }
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
        super::super::review_gate::ReviewDecision::Rejected => {
            PlanNodeResumeListenerOutcome::KeptPaused {
                plan_id: id,
                node_id: node.id.clone(),
                decision: "rejected",
            }
        }
        super::super::review_gate::ReviewDecision::NeedsChanges => {
            PlanNodeResumeListenerOutcome::KeptPaused {
                plan_id: id,
                node_id: node.id.clone(),
                decision: "needs_changes",
            }
        }
    }
}
