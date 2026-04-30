// ───────────────────────────────────────────────────────────────────────
// Stage labels — keep these as constants so tests can pin them and so the
// flow narrative in intent-flow.lisp stays trivially greppable.
// ───────────────────────────────────────────────────────────────────────

// `pipeline_stage` values returned in the response payload. These line up
// 1:1 with `F-intent-alignment-plan-execution-loop` stage ids in
// `intent-flow.lisp`. We expose them as constants because both the
// planner and its tests pin these strings.
pub(crate) const MESSAGE_INTAKE: &str = "s1_message_intake";
pub(crate) const DIRECTIVE_REVIEW_GATE: &str = "s3_alignment_review_gate";
pub(crate) const PLAN_AUTHORING: &str = "s4_plan_authoring";
pub(crate) const PLAN_REVIEW_GATE: &str = "s5_plan_review_gate";
pub(crate) const EXECUTION_RUNNER: &str = "s6_execution_runner";
/// `flow_ref` echoed on every response so callers can correlate a unified
/// entry response back to the canonical flow narrative.
pub(super) const FLOW_REF: &str = "F-intent-alignment-plan-execution-loop";
