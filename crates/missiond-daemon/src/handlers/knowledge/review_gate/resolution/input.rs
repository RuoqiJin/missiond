use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-15 :: explicit review-resolution input
//
// Wave-14 emits deterministic `QuestionEvent::Created` events for
// directive / plan / workflow file-first artifacts. Wave-15 closes the
// loop with an EXPLICIT (non-autonomous) resume path: when a human or
// operator resolves a review, the manager surfaces (approve / mark /
// supersede / archive) accept a structured input so MissionD can advance
// the artifact through its existing transition. This is NOT auto-approve
// and NOT a poll for a `QuestionEvent::Resolved` answer — the helper
// consumes ONLY the caller-supplied JSON.
//
// Input shape (all opt-in; absent → legacy quiet path):
//
//   review_question_id : string  — the deterministic id wave-14 produced
//                                  (`review:<scope>:<id>:v<version>:<action>[:<topic-hash>]`)
//   review_decision    : string  — "approved" | "rejected" | "needs_changes"
//   review_actor       : string  — free-form identity of the resolver
//                                  (echoed into the resolution event /
//                                  payload; never used for authentication)
//   review_note        : string  — free-form reason / next step (echoed)
//
// Validation pipeline (fail-fast):
//   1. parse_review_question_id_struct — refuses malformed envelopes
//   2. validate_resolution_envelope    — refuses scope mismatch / unsupported
//                                        scope / unsupported action / version
//                                        outside expected lifeline
//
// State transitions on `approved`: the manager surface performs its
// existing transition (e.g. `directive_approve`, `plan_update_status`).
// On `rejected` / `needs_changes`: the artifact STAYS non-approved. The
// payload surfaces the decision + reason so callers see the outcome.
//
// Bus failures are warnings (mirrors the wave-11/14 contract for
// `QuestionEvent::Resolved` emit). They never roll back the DB row.
// ───────────────────────────────────────────────────────────────────────

/// Explicit resolution decision attached to a review-question id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewDecision {
    /// Human / operator approved the artifact. The manager surface should
    /// run its existing approval transition.
    Approved,
    /// Human / operator rejected the artifact. The manager surface MUST
    /// keep the artifact non-approved and record the reason.
    Rejected,
    /// Human / operator wants the artifact reworked. The manager surface
    /// MUST keep the artifact in review/draft path and surface the
    /// next-step reason.
    NeedsChanges,
}

impl ReviewDecision {
    /// Lower-snake-case label for response payload + event resolution
    /// string.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ReviewDecision::Approved => "approved",
            ReviewDecision::Rejected => "rejected",
            ReviewDecision::NeedsChanges => "needs_changes",
        }
    }

    /// Parse the wire string. Case-insensitive + trimmed; we deliberately
    /// fail-fast on unknown values rather than collapsing to a default
    /// because the resolution decision is load-bearing for state
    /// transitions.
    pub(crate) fn parse(raw: &str) -> Result<Self, ResolutionInputError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "approved" | "approve" => Ok(ReviewDecision::Approved),
            "rejected" | "reject" => Ok(ReviewDecision::Rejected),
            "needs_changes" | "needs-changes" | "changes" => Ok(ReviewDecision::NeedsChanges),
            other => Err(ResolutionInputError::UnknownDecision(other.to_string())),
        }
    }
}

/// Structured resolution input pulled out of the request JSON.
#[derive(Debug, Clone)]
pub(crate) struct ReviewResolutionInput {
    pub(crate) question_id: String,
    pub(crate) decision: ReviewDecision,
    pub(crate) actor: Option<String>,
    pub(crate) note: Option<String>,
}

/// Errors returned while extracting the resolution input. We surface them
/// as structured `ToolError` codes at the handler boundary (so the bus
/// stays out of the failure path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolutionInputError {
    /// `review_question_id` was supplied but `review_decision` was missing.
    /// Caller must explicitly say what they decided — we never guess.
    MissingDecision,
    /// `review_decision` value is not in {approved, rejected,
    /// needs_changes}.
    UnknownDecision(String),
}

impl ResolutionInputError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            ResolutionInputError::MissingDecision => "MISSING_PARAM",
            ResolutionInputError::UnknownDecision(_) => "INVALID_PARAM",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            ResolutionInputError::MissingDecision => {
                "review_question_id supplied without `review_decision`; \
                 add review_decision=approved|rejected|needs_changes"
                    .to_string()
            }
            ResolutionInputError::UnknownDecision(raw) => format!(
                "review_decision `{}` is not in {{approved, rejected, needs_changes}}",
                raw
            ),
        }
    }
}

/// Pull `review_question_id`, `review_decision`, `review_actor`,
/// `review_note` out of an args JSON object.
///
/// Returns:
///   * `Ok(None)`      — `review_question_id` absent / blank → legacy
///                        quiet path; caller must skip resolution.
///   * `Ok(Some(...))` — full input; caller must validate it against the
///                        manager surface (scope / action / version) before
///                        mutating state.
///   * `Err(...)`      — `review_question_id` supplied but decision is
///                        missing / unknown. Caller must surface as
///                        structured error and refuse the action.
pub(crate) fn parse_review_resolution_input(
    args: &Value,
) -> Result<Option<ReviewResolutionInput>, ResolutionInputError> {
    let qid = parse_resolution_review_question_id(args);
    let Some(qid) = qid else {
        return Ok(None);
    };
    let decision_raw = args
        .get("review_decision")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let decision = match decision_raw {
        Some(s) => ReviewDecision::parse(&s)?,
        None => return Err(ResolutionInputError::MissingDecision),
    };
    let actor = args
        .get("review_actor")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let note = args
        .get("review_note")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(Some(ReviewResolutionInput {
        question_id: qid,
        decision,
        actor,
        note,
    }))
}

// ───────────────────────────────────────────────────────────────────────
// wave-17 :: PLAN-DAG paused-node resume input
//
// Wave-16 / task 04 added the `:review-gate "question-event"` per-node
// pause path; wave-17 / task 01 closes the loop with an EXPLICIT
// (non-autonomous) resume bridge for `mission_plan(action=execute)`.
//
// The resume input is intentionally a SEPARATE field-set from
// `review_question_id` / `review_decision` (the wave-15 manager-action
// resolution input above) so the same JSON request never accidentally
// triggers BOTH the manager-side approve transition AND the DAG-node
// resume helper. The ergonomic split also keeps the wave-15 contract
// byte-identical for callers that only used the manager actions.
//
// Input shape (all opt-in; absent → legacy quiet path):
//
//   resume_review_question_id : string  — the deterministic id wave-16 produced
//                                          (`review:plan:<plan_id>:v<version>:plan-node:<node-id-hash>`)
//   resume_review_decision    : string  — "approved" | "rejected" | "needs_changes"
//   resume_actor              : string  — free-form identity of the resolver
//   resume_note               : string  — free-form reason / next-step note
//
// This is NOT general auto-approve: only resumes nodes paused via the
// deterministic plan-node review id; non-plan-node ids in the field
// fail validation at the action_execute_resume call site.
// ───────────────────────────────────────────────────────────────────────

/// Wave-17 / task 01 — structured resume input pulled out of the
/// `mission_plan(action=execute)` request JSON. Same shape as
/// [`ReviewResolutionInput`] but lives under a distinct field set so
/// the wave-15 manager-side resolution input contract stays
/// byte-identical for callers that did not opt in to plan-node resume.
#[derive(Debug, Clone)]
pub(crate) struct PlanNodeResumeInput {
    pub(crate) question_id: String,
    pub(crate) decision: ReviewDecision,
    pub(crate) actor: Option<String>,
    pub(crate) note: Option<String>,
}

/// Wave-17 / task 01 — pull `resume_review_question_id`,
/// `resume_review_decision`, `resume_actor`, `resume_note` out of the
/// args JSON object.
///
/// Returns:
///   * `Ok(None)`      — no `resume_review_question_id` → caller falls
///                        through to the standard execute path.
///   * `Ok(Some(...))` — full input; caller must validate the envelope
///                        + paused-node mapping before dispatching.
///   * `Err(...)`      — id supplied without decision OR decision
///                        outside the {approved, rejected, needs_changes}
///                        whitelist. Caller must surface as structured
///                        error and refuse the action.
pub(crate) fn parse_plan_node_resume_input(
    args: &Value,
) -> Result<Option<PlanNodeResumeInput>, ResolutionInputError> {
    let qid = args
        .get("resume_review_question_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(qid) = qid else {
        return Ok(None);
    };
    let decision_raw = args
        .get("resume_review_decision")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let decision = match decision_raw {
        Some(s) => ReviewDecision::parse(&s)?,
        None => return Err(ResolutionInputError::MissingDecision),
    };
    let actor = args
        .get("resume_actor")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let note = args
        .get("resume_note")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(Some(PlanNodeResumeInput {
        question_id: qid,
        decision,
        actor,
        note,
    }))
}
