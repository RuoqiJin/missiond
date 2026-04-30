//! review_gate — event-bus aware review-gate emission for directive / plan /
//! workflow file-first artifacts.
//!
//! Lisp authority:
//!   - intent-flow.lisp ::
//!       F-intent-alignment-plan-execution-loop ::
//!         s3 alignment-review-gate + s5 plan-review-gate
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!       role alignment-review-gate / role plan-review-gate
//!   - intent-event-bus.lisp :: QuestionEvent
//!
//! Scope (wave-11 :: review gate event-aware code-alignment):
//!   - Pure helpers + an opt-in best-effort emitter.
//!   - Carries the deterministic question id derivation (so every artifact
//!     produces the same id from `(scope, id, version, action)` — caller can
//!     correlate Created → Resolved without persisting state).
//!   - Does NOT extend `QuestionEvent` payload (the existing `question_id`
//!     field already carries our deterministic id, and existing serde tests
//!     stay intact).
//!   - Does NOT implement human UI / wait-for-answer. The Created event is
//!     fire-and-forget; the manager surface returns immediately so callers
//!     are never blocked on a human gate. Gate resolution (Resolved /
//!     DecisionResolved) is also opt-in via `review_question_id`.
//!
//! Scope (wave-14 :: review gate auto-create v1):
//!   - Adds [`ReviewGatePolicy`] (`manual` / `emit_question` / `off`) and
//!     [`parse_review_gate_policy`] so directive / plan / workflow handlers
//!     can opt callers into automatic `QuestionEvent::Created` emission after
//!     a successful file-first artifact write — without changing the legacy
//!     opt-in `emit_review_question` boolean (which keeps working under the
//!     `manual` policy).
//!   - Adds [`auto_emit_review_question_after_artifact_write`], the
//!     post-write hook called from compile / distill paths AFTER
//!     `attempt_artifact_write` has spliced its `file_written` outcome. The
//!     hook only fires when policy=`emit_question` AND the splice declared
//!     `file_written=true`; otherwise it stamps `review_question_emitted=
//!     false` and surfaces the policy + reason so callers can observe what
//!     happened.
//!   - Deterministic id derivation is extended via
//!     [`derive_review_question_id_for_artifact`] which folds the artifact
//!     kind label, db id, version, and topic-or-file-path-hash into the same
//!     `review:<scope>:<id>:v<version>:<action>:<topic-hash>` envelope — same
//!     input always returns the same id, so retries / resolutions correlate
//!     even across daemon restarts.
//!   - Does NOT auto-approve, does NOT wait, does NOT mutate the persisted
//!     artifact. The hook is fire-and-forget on the bus side, and the file
//!     write success comes from the splice — we never overwrite the splice.
//!
//! Bus failure semantics (mirrors CLAUDE.md `feedback_fail_fast_no_fallback`):
//!   - The core action (compile persist / approve / archive / mark / supersede)
//!     never fails because of a side-channel bus error.
//!   - But we ALSO refuse to silently swallow it: when the publish call
//!     errors, the response carries a `review_question_warning` block with
//!     the error text plus the deterministic id, so downstream readers see a
//!     loud signal in the response payload AND in the daemon logs.

#[cfg(test)]
use missiond_core::event::events::QuestionEvent;
#[cfg(test)]
use serde_json::{json, Value};

mod auto_answer;
mod created;
mod llm_approval;
mod resolution;

#[cfg(test)]
use auto_answer::DESTRUCTIVE_REVIEW_ACTIONS;
#[allow(unused_imports)]
pub(crate) use auto_answer::{
    auto_answer_policy_was_explicit, evaluate_auto_answer_policy, is_destructive_review_action,
    parse_auto_answer_policy, stamp_auto_answer_payload, AutoAnswerOutcome, AutoAnswerPolicy,
    AutoAnswerStatus,
};

#[cfg(test)]
use resolution::event_kind_label;
#[allow(unused_imports)]
pub(crate) use resolution::{
    build_resolution_event, evaluate_review_automation, maybe_emit_review_question_resolved,
    parse_plan_node_resume_input, parse_resolution_review_question_id,
    parse_review_automation_policy, parse_review_question_id_struct, parse_review_resolution_input,
    parse_subscriber_resolution_string, plan_review_resolved_dispatch, resolution_wire_string,
    review_automation_policy_was_explicit, stamp_needs_changes_next_step, stamp_resolution_payload,
    stamp_review_automation_payload, validate_review_resolution_envelope, AutomationStatus,
    ParsedReviewQuestionId, PlanNodeResumeInput, ResolutionDecisionMeta, ResolutionInputError,
    ResolutionOutcome, ResolutionValidationError, ReviewAutomationContext, ReviewAutomationOutcome,
    ReviewAutomationPolicy, ReviewDecision, ReviewIdParseError, ReviewResolutionInput,
    ReviewResolvedDispatch,
};

#[allow(unused_imports)]
pub(crate) use created::{
    apply_compile_review_gates, auto_emit_review_question_after_artifact_write,
    derive_plan_node_review_question_id, derive_plan_node_topic_hash, derive_review_question_id,
    derive_review_question_id_for_artifact, is_plan_node_review_action,
    maybe_emit_review_question_created, parse_compile_review_gate, parse_review_gate_policy,
    review_gate_policy_was_explicit, AutoEmitDecision, CompileReviewGateRequest, ReviewGatePolicy,
    PLAN_NODE_REVIEW_DEFAULT_ACTION,
};
#[cfg(test)]
use created::{payload_says_file_written, stamp_policy, topic_hash_short};

#[allow(unused_imports)]
pub(crate) use llm_approval::{
    build_llm_auto_approve_proposal_system_prompt, build_llm_auto_approve_proposal_user_prompt,
    compute_proposal_hash, enforce_apply_gate_preflight, enforce_proposal_invariants,
    evaluate_llm_approve_apply_gate, llm_auto_approve_proposal_mode_was_explicit,
    parse_llm_approve_apply_gate_input, parse_llm_auto_approve_proposal,
    parse_llm_auto_approve_proposal_mode, stamp_llm_approve_apply_gate_payload,
    stamp_llm_auto_approve_proposal_payload, stamp_proposal_hash_payload, LlmApproveApplyGateInput,
    LlmApproveApplyGateOutcome, LlmApproveApplyStatus, LlmAutoApproveProposal,
    LlmAutoApproveProposalBundle, LlmAutoApproveProposalConfidence, LlmAutoApproveProposalMode,
    LlmAutoApproveProposalStatus, ProposalHashStatus, APPLY_GATE_INVALID_PARAM,
    APPLY_GATE_MISSING_PROPOSAL_HASH, APPLY_GATE_PROPOSAL_HASH_MISMATCH,
};
#[cfg(test)]
use llm_approval::{proposal_json_kind, strip_proposal_code_fence};

// ───────────────────────────────────────────────────────────────────────
// tests — pure helpers only (no bus, no DB).
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
