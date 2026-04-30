use missiond_core::types::Plan;
use missiond_mcp::tools::error_codes;

use crate::handlers::knowledge::review_gate::{
    derive_plan_node_topic_hash, is_plan_node_review_action, ParsedReviewQuestionId,
};

use super::super::{DagNode, ParsedDag, ReviewGateKind};

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
    pub(in crate::handlers::knowledge::plan_dag) fn code(&self) -> &'static str {
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

    pub(in crate::handlers::knowledge::plan_dag) fn message(&self) -> String {
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
    parsed_qid: &ParsedReviewQuestionId,
    plan: &Plan,
    parsed_dag: &'a ParsedDag,
) -> std::result::Result<&'a DagNode, PlanNodeResumeError> {
    if parsed_qid.scope != "plan" || !is_plan_node_review_action(&parsed_qid.action) {
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
        let h = derive_plan_node_topic_hash(&n.id);
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
