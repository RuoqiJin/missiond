use serde_json::{json, Value};

use super::super::auto_answer::is_destructive_review_action;
use super::super::resolution::ReviewDecision;
use super::{
    proposal_json_kind, LlmAutoApproveProposal, LlmAutoApproveProposalBundle,
    LlmAutoApproveProposalConfidence, LlmAutoApproveProposalStatus,
};

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

/// Pure deterministic SHA-256 hash over the LOAD-BEARING fields of a
/// proposal. Truncated to the leading 32 hex chars (128 bits — way more
/// than enough collision resistance for an audit-trail correlator).
///
/// Inputs: action label, artifact id, artifact version, the proposal's
/// decision wire string, the proposal's confidence wire string, and the
/// proposal's deterministic destructive_check substring (we use the
/// "destructive:" / "non_destructive:" prefix only, not the full free
/// text, so the hash stays stable across superficial wording changes).
///
/// The hash is what the caller is expected to echo back via the
/// `proposal_hash` arg under `apply_llm_auto_approve=true`. Caller can
/// derive it themselves from the proposal block — we surface the same
/// value under `llm_auto_approve_proposal_hash` so dashboards can
/// `assert hash == derive(...)` directly.
pub(crate) fn compute_proposal_hash(
    action: &str,
    artifact_id: &str,
    version: i32,
    proposal: &LlmAutoApproveProposal,
) -> String {
    use sha2::{Digest, Sha256};
    // We hash a CANONICAL serialisation: lower-case action; trimmed
    // artifact id; ascii integer version; decision wire; confidence
    // wire; the destructive_check prefix (everything before the first
    // colon, lowercased). Any other proposal field (evidence /
    // non_goal_check) is intentionally OUT of the hash so superficial
    // text differences don't churn the audit correlator.
    let action_norm = action.trim().to_ascii_lowercase();
    let artifact_norm = artifact_id.trim();
    let destructive_prefix = proposal
        .destructive_check
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let payload = format!(
        "v1|{}|{}|{}|{}|{}|{}",
        action_norm,
        artifact_norm,
        version,
        proposal.decision.as_str(),
        proposal.confidence.as_str(),
        destructive_prefix,
    );
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full.chars().take(32).collect()
}

/// Pure outcome of [`evaluate_llm_approve_apply_gate`]. Side-effect free
/// — no DB, no bus, no LLM. The handler consumes this projection to
/// decide whether to run the existing `directive_approve` /
/// `plan_update_status(Approved)` transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LlmApproveApplyGateOutcome {
    /// Whether the caller opted into the gate at all. Echoes
    /// `LlmApproveApplyGateInput::apply` for audit symmetry.
    pub requested: bool,
    /// Wire status — the load-bearing signal for observers.
    pub status: LlmApproveApplyStatus,
    /// The decision the gate would have applied (always `Approved` when
    /// `status=Applied`; carries the proposal's decision under SKIP
    /// statuses so dashboards see what was offered). `None` when no
    /// proposal exists.
    pub applied_decision: Option<ReviewDecision>,
    /// Result of the proposal-hash comparison.
    pub proposal_hash_status: ProposalHashStatus,
    /// Hash the gate computed from the bundle (always populated when a
    /// proposal exists; None under Unavailable / NoSuggestion).
    pub computed_proposal_hash: Option<String>,
    /// Caller-supplied hash (echoed for audit symmetry).
    pub supplied_proposal_hash: Option<String>,
    /// Whether the caller flipped `caller_approved=true`.
    pub caller_approved: bool,
    /// Flat list of `code:detail` strings explaining every gate's
    /// outcome. Always populated under non-NotRequested statuses; the
    /// NotRequested status returns an empty vec so the response can
    /// omit the gate block entirely without losing audit detail.
    pub safety_rule_results: Vec<String>,
}

impl LlmApproveApplyGateOutcome {
    /// Build the wire shape consumed by the response payload. Always
    /// emits every field (with `null` for absent values) so observers
    /// can pivot on a stable shape regardless of which skip reason
    /// fired.
    pub(crate) fn to_response_json(&self) -> Value {
        json!({
            "requested": self.requested,
            "apply_status": self.status.as_str(),
            "applied_decision": self.applied_decision.map(|d| d.as_str()),
            "proposal_hash_status": self.proposal_hash_status.as_str(),
            "computed_proposal_hash": self.computed_proposal_hash.clone(),
            "supplied_proposal_hash": self.supplied_proposal_hash.clone(),
            "caller_approved": self.caller_approved,
            "safety_rule_results": self.safety_rule_results,
        })
    }
}

/// Pure evaluator of the wave-22 / task 03 apply-gate. Does NOT mutate
/// state, does NOT compare hashes against a strict-error code (the
/// hash-mismatch / hash-missing fail-fast path runs in
/// [`enforce_apply_gate_preflight`] BEFORE this evaluator). This helper
/// produces the structured outcome the response carries; the handler
/// reads `outcome.status.should_apply()` to decide whether to run the
/// DB transition.
///
/// Inputs:
///   * `input`     — caller-supplied gate args (parsed via
///                    [`parse_llm_approve_apply_gate_input`]).
///   * `bundle`    — proposal bundle from
///                    [`request_*_auto_approve_proposal`].
///   * `action`    — review action label (folded into hash + destructive
///                    rule).
///   * `artifact_id` / `version` — artifact identity (folded into hash).
pub(crate) fn evaluate_llm_approve_apply_gate(
    input: &LlmApproveApplyGateInput,
    bundle: &LlmAutoApproveProposalBundle,
    action: &str,
    artifact_id: &str,
    version: i32,
) -> LlmApproveApplyGateOutcome {
    let mut rule_results: Vec<String> = Vec::new();

    // Compute the hash + hash status up-front so observers always see
    // the deterministic verdict (regardless of whether the gate ran).
    let (computed_hash, hash_status) = match bundle.proposal.as_ref() {
        Some(p) => {
            let hash = compute_proposal_hash(action, artifact_id, version, p);
            let status = match input.proposal_hash.as_deref() {
                None => ProposalHashStatus::NotSupplied,
                Some(s) if s.eq_ignore_ascii_case(&hash) => ProposalHashStatus::Matches,
                Some(_) => ProposalHashStatus::Mismatch,
            };
            (Some(hash), status)
        }
        None => (None, ProposalHashStatus::NoProposalAvailable),
    };

    // G1 — caller opted in. Default short-circuit returns
    // `NotRequested` so the response stays byte-identical with
    // wave-21 / task 06 callers.
    if !input.apply {
        return LlmApproveApplyGateOutcome {
            requested: false,
            status: LlmApproveApplyStatus::NotRequested,
            applied_decision: bundle.proposal.as_ref().map(|p| p.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }

    // G4 (early) — destructive action ALWAYS skips, regardless of any
    // other gate outcome (invariant I2). Pinned BEFORE bundle status so
    // the rule-result list shows the deterministic refusal even if the
    // bundle is `DestructiveBlocked` (which already says the same thing
    // — but having BOTH layers loud means a future regression that
    // forgot to short-circuit destructive in `request_*` cannot sneak
    // through to apply).
    let destructive = is_destructive_review_action(action);
    if destructive {
        rule_results.push(format!(
            "rule:destructive_action:`{}` is on the destructive list (archive|supersede|remove); apply gate refuses to promote (invariant I2)",
            action.trim().to_ascii_lowercase()
        ));
    } else {
        rule_results.push(format!(
            "rule:non_destructive_action:`{}` is not on the destructive list",
            action.trim().to_ascii_lowercase()
        ));
    }

    // G2 — proposal hash status. Note the strict pre-flight in
    // `enforce_apply_gate_preflight` already rejected mismatch / missing
    // BEFORE we got here; surfacing the rule result keeps the audit
    // trail loud even in test paths that bypass the preflight.
    match hash_status {
        ProposalHashStatus::Matches => {
            rule_results.push("rule:proposal_hash:matches".to_string());
        }
        ProposalHashStatus::NotSupplied => {
            rule_results.push(
                "rule:proposal_hash:not_supplied (apply gate requires explicit hash echo)"
                    .to_string(),
            );
        }
        ProposalHashStatus::Mismatch => {
            rule_results.push(
                "rule:proposal_hash:mismatch (caller-supplied hash does not match bundle)"
                    .to_string(),
            );
        }
        ProposalHashStatus::NoProposalAvailable => {
            rule_results.push(
                "rule:proposal_hash:no_proposal_available (bundle has no proposal to hash)"
                    .to_string(),
            );
        }
    }

    // G3 — caller_approved double opt-in.
    if input.caller_approved {
        rule_results.push("rule:caller_approved:true".to_string());
    } else {
        rule_results.push(
            "rule:caller_approved:false (apply gate requires the explicit caller_approved=true confirmation)"
                .to_string(),
        );
    }

    // Bundle-status branches.
    match bundle.status {
        LlmAutoApproveProposalStatus::NotInvoked => {
            // The caller opted into the gate but the bundle was never
            // invoked (proposer mode is off / absent). This is a config
            // bug on the caller side — the gate cannot apply without a
            // proposal. We surface it as `skipped_no_proposal`.
            rule_results.push(
                "rule:bundle_status:not_invoked (proposer mode `off` / absent — no proposal to apply)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedNoProposal,
                applied_decision: None,
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::Unavailable => {
            // Invariant I4 — Sonnet unavailable; gate refuses to
            // synthesise a fallback.
            rule_results.push(
                "rule:bundle_status:unavailable (Sonnet gateway unavailable; gate refuses fallback per invariant I4)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedUnavailable,
                applied_decision: None,
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::NoSuggestion => {
            rule_results.push(
                "rule:bundle_status:no_suggestion (Sonnet returned an unparseable / empty response)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedNoProposal,
                applied_decision: None,
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::DestructiveBlocked => {
            // Invariant I2 (already pinned by G4 above) — we don't even
            // reach the proposal here in v0; the request_* helper short-
            // circuited. Belt-and-braces refusal.
            rule_results.push(
                "rule:bundle_status:destructive_blocked (invariant I2 short-circuited the proposer)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedDestructiveAction,
                applied_decision: bundle.proposal.as_ref().map(|p| p.decision),
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::Suggested => {
            rule_results.push(
                "rule:bundle_status:suggested (proposal survived parser + invariants)".to_string(),
            );
        }
    }

    let proposal = bundle.proposal.as_ref().expect(
        "Suggested status guarantees proposal is Some — see LlmAutoApproveProposalBundle invariants",
    );

    // G4 (deterministic re-check) — destructive action.
    if destructive {
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedDestructiveAction,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }

    // Invariant I5 cross-check — the proposal's destructive_check field
    // is informational, but if the model lied (claimed non_destructive
    // for an actually destructive action — unreachable here because
    // destructive short-circuits above, BUT we keep the check as a
    // belt-and-braces guard in case a future caller bypasses the
    // request_* helper) we surface the lie loudly.
    let model_says_destructive = proposal.destructive_check.starts_with("destructive:");
    if destructive != model_says_destructive {
        rule_results.push(format!(
            "rule:invariant_i5:deterministic_override (deterministic destructive={} but proposal.destructive_check=`{}`; gate trusts deterministic verdict)",
            destructive,
            proposal.destructive_check.split(':').next().unwrap_or("?"),
        ));
    } else {
        rule_results.push(format!(
            "rule:invariant_i5:proposal_matches_deterministic (destructive={})",
            destructive,
        ));
    }

    // G2 — hash status terminal check. Mismatch / missing = SKIP. Note
    // the strict pre-flight in `enforce_apply_gate_preflight` should
    // have already converted these into structured errors; surfacing
    // the SKIP here means the test path (which calls the evaluator
    // directly without the preflight) still gets a sane outcome.
    match hash_status {
        ProposalHashStatus::Matches => {}
        ProposalHashStatus::NotSupplied
        | ProposalHashStatus::Mismatch
        | ProposalHashStatus::NoProposalAvailable => {
            return LlmApproveApplyGateOutcome {
                requested: true,
                // Hash gate failures collapse to SkippedCallerNotApproved
                // ONLY when the strict preflight was bypassed (e.g. unit
                // tests). The preflight's structured error is the
                // production path; this fallback keeps the evaluator
                // pure even when it stands alone.
                status: LlmApproveApplyStatus::SkippedCallerNotApproved,
                applied_decision: Some(proposal.decision),
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
    }

    // G3 — caller_approved double opt-in.
    if !input.caller_approved {
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedCallerNotApproved,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }

    // G5 — decision must be Approved (invariant I1: never auto-reject;
    // we also never auto-anything-other-than-approve here).
    if !matches!(proposal.decision, ReviewDecision::Approved) {
        rule_results.push(format!(
            "rule:decision_not_approved (proposal.decision={}; gate only applies `approved`, never `needs_changes` per invariant I1)",
            proposal.decision.as_str(),
        ));
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedNonApprovedDecision,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }
    rule_results.push("rule:decision_approved (proposal.decision=approved)".to_string());

    // G6 — confidence must be High.
    if !matches!(proposal.confidence, LlmAutoApproveProposalConfidence::High) {
        rule_results.push(format!(
            "rule:confidence_too_low (proposal.confidence={}; gate requires `high`)",
            proposal.confidence.as_str(),
        ));
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedConfidenceTooLow,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }
    rule_results.push("rule:confidence_high (proposal.confidence=high)".to_string());

    // All gates passed.
    rule_results.push(
        "rule:apply_gate_satisfied (G1..G6 all green; handler may run the existing approve transition)"
            .to_string(),
    );
    LlmApproveApplyGateOutcome {
        requested: true,
        status: LlmApproveApplyStatus::Applied,
        applied_decision: Some(proposal.decision),
        proposal_hash_status: hash_status,
        computed_proposal_hash: computed_hash,
        supplied_proposal_hash: input.proposal_hash.clone(),
        caller_approved: input.caller_approved,
        safety_rule_results: rule_results,
    }
}

/// Strict pre-flight for the wave-22 / task 03 apply gate. Runs the
/// fail-fast hash-missing / hash-mismatch checks BEFORE any state
/// mutation. Returns `Ok(())` when:
///   * caller did not opt in (`apply=false`);
///   * caller opted in AND supplied a hash that matches.
/// Returns `Err((code, message))` for the two contract-mandated
/// structured errors:
///   * `APPLY_GATE_MISSING_PROPOSAL_HASH` — `apply=true` without a hash.
///   * `APPLY_GATE_PROPOSAL_HASH_MISMATCH` — `apply=true` with a hash
///     that does not match the bundle.
///
/// The handler converts the Err into [`ToolResult::structured_error`]
/// BEFORE running the existing `directive_approve` /
/// `plan_update_status` transition, satisfying the contract: "On
/// mismatch or missing proposal hash, return structured error and do
/// not mutate directive/plan/review state."
pub(crate) fn enforce_apply_gate_preflight(
    input: &LlmApproveApplyGateInput,
    bundle: &LlmAutoApproveProposalBundle,
    action: &str,
    artifact_id: &str,
    version: i32,
) -> std::result::Result<(), (String, String)> {
    if !input.apply {
        return Ok(());
    }
    // Without a proposal we cannot compute a hash; structured-error so
    // the caller knows to retry under `auto_approve_mode=sonnet_suggest`
    // first (or to drop the apply flag).
    let proposal = match bundle.proposal.as_ref() {
        Some(p) => p,
        None => {
            // Missing hash also implicitly fails for no-proposal bundles
            // — surface the more specific message so the caller knows
            // the proposal is the missing piece.
            if input.proposal_hash.is_none() {
                return Err((
                    APPLY_GATE_MISSING_PROPOSAL_HASH.to_string(),
                    format!(
                        "apply_llm_auto_approve=true requires `proposal_hash` AND a Sonnet proposal to apply against; bundle status `{}` carries no proposal",
                        bundle.status.as_str(),
                    ),
                ));
            }
            // Caller supplied a hash but the bundle has no proposal —
            // hash cannot match. Surface mismatch so dashboards see the
            // load-bearing reason.
            return Err((
                APPLY_GATE_PROPOSAL_HASH_MISMATCH.to_string(),
                format!(
                    "apply_llm_auto_approve=true with `proposal_hash` but bundle status `{}` carries no proposal to compare against",
                    bundle.status.as_str(),
                ),
            ));
        }
    };
    let hash = compute_proposal_hash(action, artifact_id, version, proposal);
    match input.proposal_hash.as_deref() {
        None => Err((
            APPLY_GATE_MISSING_PROPOSAL_HASH.to_string(),
            format!(
                "apply_llm_auto_approve=true requires `proposal_hash`; expected `{}` (echoed under `llm_auto_approve_proposal_hash` in the propose-only response)",
                hash,
            ),
        )),
        Some(s) if s.eq_ignore_ascii_case(&hash) => Ok(()),
        Some(s) => Err((
            APPLY_GATE_PROPOSAL_HASH_MISMATCH.to_string(),
            format!(
                "apply_llm_auto_approve=true with `proposal_hash=`{}`` does not match bundle hash `{}` (action=`{}` artifact=`{}` v{})",
                s, hash, action, artifact_id, version,
            ),
        )),
    }
}

/// Stamp the wave-22 / task 03 apply-gate outcome onto a response
/// payload under the stable `llm_approve_apply_gate` key. Pure / no
/// bus calls. Skipped when the gate was NotRequested so legacy callers
/// stay byte-identical with wave-21 / task 06.
pub(crate) fn stamp_llm_approve_apply_gate_payload(
    payload: &mut Value,
    outcome: &LlmApproveApplyGateOutcome,
) {
    if matches!(outcome.status, LlmApproveApplyStatus::NotRequested) {
        return;
    }
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert(
        "llm_approve_apply_gate".to_string(),
        outcome.to_response_json(),
    );
}

/// Augment the wave-21 / task 06 propose-only payload with the
/// deterministic proposal hash so callers can echo it back via
/// `proposal_hash` under `apply_llm_auto_approve=true` without having
/// to re-derive it themselves. Pure / no bus calls. Always runs
/// (regardless of mode) when a proposal is present so the wire shape
/// stays stable across runs.
pub(crate) fn stamp_proposal_hash_payload(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
    action: &str,
    artifact_id: &str,
    version: i32,
) {
    let Some(p) = bundle.proposal.as_ref() else {
        return;
    };
    let hash = compute_proposal_hash(action, artifact_id, version, p);
    if let Some(map) = payload.as_object_mut() {
        map.insert("llm_auto_approve_proposal_hash".to_string(), json!(hash));
    }
}
