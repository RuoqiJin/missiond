use super::*;

// V3 invariant anchor: apply-gate mode must never auto-approve without
// explicit caller approval and a matching proposal hash.

// ───────────────────────────────────────────────────────────────────────
// wave-22 / task 03 — LLM auto-approve apply gate v1
//
// Layered conservatively on top of wave-21 / task 06 (propose-only). The
// new `apply_llm_auto_approve` knob is OPT-IN (default `false`); when
// the caller flips it AND supplies a matching `proposal_hash` AND the
// proposal cleared every safety rule, the gate promotes the proposal's
// `decision=approved` into an actual DB transition (analogous to the
// caller having supplied an explicit `review_decision=approved`).
//
// The wave-21 / task 06 hard invariants stay PINNED — this wave does
// NOT relax any of them. The proposal value itself still carries
// `applied=false` + `requires_human=true` (those are properties of the
// PROPOSAL surface, not the apply gate). The new apply-gate surface is
// a SEPARATE structured block under `llm_approve_apply_gate` that
// records WHETHER the gate fired AND WHY.
//
// 6 strict gate conditions (all must pass to apply):
//
//   G1. `apply_llm_auto_approve=true` (the explicit per-call opt-in
//       referenced in wave-21 / task 06 invariant I3 — "any future wave
//       promoting proposals to authority MUST add a separate explicit
//       caller-side opt-in flag"). Default false ⇒ byte-identical with
//       wave-21 / task 06.
//   G2. `proposal_hash` supplied AND matches the bundle's deterministic
//       hash (SHA-256 over decision|confidence|destructive_check|
//       action|artifact_id|version, truncated to 32 hex chars). Missing
//       hash ⇒ structured error `APPLY_GATE_MISSING_PROPOSAL_HASH`.
//       Mismatch ⇒ structured error `APPLY_GATE_PROPOSAL_HASH_MISMATCH`.
//       Both fail-fast BEFORE any state mutation (per the contract:
//       "On mismatch or missing proposal hash, return structured error
//       and do not mutate directive/plan/review state.").
//   G3. `caller_approved=true` (a SECOND explicit opt-in field that
//       confirms the human intent — having two flags makes accidental
//       opt-in by config-file mishap virtually impossible).
//   G4. The `action` is non-destructive per the deterministic
//       `is_destructive_review_action` helper (NOT the proposal's
//       destructive_check field, which is informational only — see I5).
//   G5. The proposal's `decision == ReviewDecision::Approved`. We never
//       auto-apply `needs_changes` or any other non-Approved state. By
//       contract: "Never auto-reject. Never apply archive/supersede/
//       remove/destructive actions."
//   G6. The proposal's `confidence == LlmAutoApproveProposalConfidence::
//       High`. Medium / Low confidence proposals always SKIP. This is
//       deliberately stricter than the wave-21 / task 05 plan-inference
//       gate (which allows medium) because LLM review approval is a
//       higher-stakes surface than field inference.
//
// Wave-21 / task 06 invariant carry-over (PROVED PRESERVED by tests):
//
//   I1. Never auto-reject. The gate ONLY ever applies `decision=approved`;
//       any other decision falls through to G5 = SKIP. Rejected can
//       never reach this code (the parser already demoted it to
//       needs_changes per I1).
//   I2. Destructive never auto-promote. G4 fails for archive/supersede/
//       remove. The apply_status surfaces as
//       `skipped_destructive_action` with the deterministic verdict.
//   I3. The PROPOSAL still carries `applied=false` + `requires_human=
//       true`. The gate's `apply_status="applied"` lives on a SEPARATE
//       surface (`llm_approve_apply_gate`); the proposal block stays
//       byte-identical with wave-21 / task 06.
//   I4. Sonnet unavailable / NoSuggestion ⇒ no proposal exists ⇒ G5
//       fails (no decision to apply) ⇒ apply_status is
//       `skipped_no_proposal` / `skipped_unavailable`. NEVER falls back
//       to a deterministic synthesised proposal.
//   I5. The gate cross-checks the proposal's `destructive_check` field
//       against the deterministic helper. If the model lied (returned
//       non_destructive for an actually destructive action), the gate
//       defers to the DETERMINISTIC verdict (G4). The proposal's lie
//       surfaces in `safety_rule_results[]` as a deterministic_override
//       entry.
//
// The gate is a PURE evaluator — `evaluate_llm_approve_apply_gate` does
// no I/O. The handler reads `apply_status` from the outcome to decide
// whether to run the existing `directive_approve` / `plan_update_status`
// transition. This keeps the wave-15 / wave-18 / wave-21 layered
// suggestion machinery untouched.
//
// Lisp authority forward reference (Wave 22 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                         s3 alignment-review-gate (apply gate v1)
//   - intent-tools.lisp :: implemented-surface mission_directive ::
//                         :execute-contract :apply-llm-auto-approve-gate
//   - intent-tools.lisp :: implemented-surface mission_plan ::
//                         :execute-contract :apply-llm-auto-approve-gate
// ───────────────────────────────────────────────────────────────────────

/// Structured error code returned when the caller flips
/// `apply_llm_auto_approve=true` but omits the required `proposal_hash`.
/// Pinned to the conservative posture: the gate refuses to silently
/// proceed without the hash so callers can never accidentally apply a
/// proposal they have not actually inspected.
pub(crate) const APPLY_GATE_MISSING_PROPOSAL_HASH: &str = "APPLY_GATE_MISSING_PROPOSAL_HASH";

/// Structured error code returned when the caller-supplied
/// `proposal_hash` does not match the bundle's deterministic hash. This
/// is the strongest "the proposal you saw is not the proposal we have"
/// signal — surfacing it BEFORE any DB mutation is the contract's
/// hard requirement.
pub(crate) const APPLY_GATE_PROPOSAL_HASH_MISMATCH: &str = "APPLY_GATE_PROPOSAL_HASH_MISMATCH";

/// Structured error code returned when the caller flips
/// `apply_llm_auto_approve=true` but supplies a non-bool / non-string
/// shape for `caller_approved` / `proposal_hash`. Caller typos must
/// fail-fast so they can never silently degrade to skip.
pub(crate) const APPLY_GATE_INVALID_PARAM: &str = "APPLY_GATE_INVALID_PARAM";

/// Wire status for the wave-22 / task 03 apply-gate decision. Pinned
/// as a closed enum so dashboards can `grep` for stable strings without
/// inspecting the rest of the gate block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmApproveApplyStatus {
    /// Caller did not opt in (`apply_llm_auto_approve` absent / false).
    /// Gate block is omitted from the response so legacy callers stay
    /// byte-identical with wave-21 / task 06.
    NotRequested,
    /// All 6 gates passed AND the handler ran the DB transition. Wire
    /// label is the load-bearing signal observers pivot on.
    Applied,
    /// Caller opted in but the bundle reported `Unavailable` (gateway
    /// not initialised / network failure). Gate refuses to synthesise
    /// a deterministic suggestion (invariant I4).
    SkippedUnavailable,
    /// Caller opted in but the bundle reported `NoSuggestion` (Sonnet
    /// returned an unparseable / empty response). No proposal to apply.
    SkippedNoProposal,
    /// Caller opted in but the action is destructive (invariant I2).
    /// Pinned as a SEPARATE status so observers can grep for "destructive
    /// blocked" apart from "rules failed".
    SkippedDestructiveAction,
    /// Caller opted in but the proposal's decision is not `Approved`
    /// (e.g. `NeedsChanges`). Invariant I1 — never auto-reject; this
    /// status covers "never auto-anything-other-than-approve" too.
    SkippedNonApprovedDecision,
    /// Caller opted in but the proposal's confidence is `Medium` or
    /// `Low`. The wave-22 gate is deliberately stricter than wave-21 /
    /// task 05.
    SkippedConfidenceTooLow,
    /// Caller opted in but did not flip `caller_approved=true`. The
    /// double opt-in is required precisely so the gate cannot fire by
    /// a single accidental flag flip.
    SkippedCallerNotApproved,
}

impl LlmApproveApplyStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LlmApproveApplyStatus::NotRequested => "not_requested",
            LlmApproveApplyStatus::Applied => "applied",
            LlmApproveApplyStatus::SkippedUnavailable => "skipped_unavailable",
            LlmApproveApplyStatus::SkippedNoProposal => "skipped_no_proposal",
            LlmApproveApplyStatus::SkippedDestructiveAction => "skipped_destructive_action",
            LlmApproveApplyStatus::SkippedNonApprovedDecision => "skipped_non_approved_decision",
            LlmApproveApplyStatus::SkippedConfidenceTooLow => "skipped_confidence_too_low",
            LlmApproveApplyStatus::SkippedCallerNotApproved => "skipped_caller_not_approved",
        }
    }

    /// True iff the gate authorised the handler to run the existing
    /// `directive_approve` / `plan_update_status(Approved)` transition.
    pub(crate) fn should_apply(self) -> bool {
        matches!(self, LlmApproveApplyStatus::Applied)
    }
}

/// Wire status for the deterministic proposal-hash check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProposalHashStatus {
    /// Caller did not supply `proposal_hash`. Under
    /// `apply_llm_auto_approve=true` this collapses to a structured
    /// error (`APPLY_GATE_MISSING_PROPOSAL_HASH`) BEFORE the gate runs;
    /// surfaced under propose-only paths for completeness.
    NotSupplied,
    /// Caller-supplied hash matches the bundle's deterministic hash.
    Matches,
    /// Caller-supplied hash does NOT match. Surfaced as a structured
    /// error (`APPLY_GATE_PROPOSAL_HASH_MISMATCH`) BEFORE the gate runs.
    Mismatch,
    /// No proposal exists (Unavailable / NoSuggestion / DestructiveBlocked
    /// short-circuit). Hash check is moot.
    NoProposalAvailable,
}

impl ProposalHashStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProposalHashStatus::NotSupplied => "not_supplied",
            ProposalHashStatus::Matches => "matches",
            ProposalHashStatus::Mismatch => "mismatch",
            ProposalHashStatus::NoProposalAvailable => "no_proposal_available",
        }
    }
}

/// Caller-supplied opt-in inputs for the wave-22 / task 03 apply gate.
/// Strict-shape: `apply` / `caller_approved` are bool-only (the literal
/// strings `"true"` / `"false"` are rejected so a typo cannot silently
/// flip the gate); `proposal_hash` is string-only.
#[derive(Debug, Clone, Default)]
pub(crate) struct LlmApproveApplyGateInput {
    /// Caller opted into the gate (`apply_llm_auto_approve=true`).
    pub apply: bool,
    /// Caller-supplied SHA-256 hash (truncated) of the proposal they
    /// inspected. Required when `apply=true`.
    pub proposal_hash: Option<String>,
    /// Caller's second opt-in flag confirming human intent.
    /// Required-truthy when `apply=true`.
    pub caller_approved: bool,
    /// True iff the caller explicitly supplied any of the gate fields
    /// (used to differentiate "caller opted out" from "caller never saw
    /// the knob" so the response stays byte-identical for the latter).
    pub explicit: bool,
}

/// Strict pre-flight validator for the wave-22 / task 03 apply-gate
/// args. Rejects any non-bool / non-string shape so caller typos fail
/// fast with structured errors. Pure / no I/O.
pub(crate) fn parse_llm_approve_apply_gate_input(
    args: &Value,
) -> std::result::Result<LlmApproveApplyGateInput, (String, String)> {
    let mut input = LlmApproveApplyGateInput::default();

    let apply_v = args.get("apply_llm_auto_approve");
    let hash_v = args.get("proposal_hash");
    let caller_v = args.get("caller_approved");
    input.explicit = apply_v.is_some() || hash_v.is_some() || caller_v.is_some();

    if let Some(v) = apply_v {
        if v.is_null() {
            // null behaves like absent for back-compat with callers who
            // serialize an explicit null.
        } else if let Some(b) = v.as_bool() {
            input.apply = b;
        } else {
            return Err((
                APPLY_GATE_INVALID_PARAM.to_string(),
                format!(
                    "apply_llm_auto_approve must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = hash_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                input.proposal_hash = Some(trimmed.to_string());
            }
        } else {
            return Err((
                APPLY_GATE_INVALID_PARAM.to_string(),
                format!(
                    "proposal_hash must be a string (SHA-256 hex truncated to 32 chars); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = caller_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(b) = v.as_bool() {
            input.caller_approved = b;
        } else {
            return Err((
                APPLY_GATE_INVALID_PARAM.to_string(),
                format!(
                    "caller_approved must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    Ok(input)
}
