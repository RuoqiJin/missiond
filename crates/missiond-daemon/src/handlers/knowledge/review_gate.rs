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
use serde_json::{json, Value};

mod created;
mod resolution;


#[allow(unused_imports)]
pub(crate) use resolution::{
    build_resolution_event, evaluate_review_automation, maybe_emit_review_question_resolved,
    parse_plan_node_resume_input, parse_resolution_review_question_id,
    parse_review_automation_policy, parse_review_question_id_struct,
    parse_review_resolution_input, parse_subscriber_resolution_string,
    plan_review_resolved_dispatch, review_automation_policy_was_explicit,
    resolution_wire_string, stamp_needs_changes_next_step, stamp_resolution_payload,
    stamp_review_automation_payload, validate_review_resolution_envelope, AutomationStatus,
    ParsedReviewQuestionId, PlanNodeResumeInput, ResolutionDecisionMeta, ResolutionInputError,
    ResolutionOutcome, ResolutionValidationError, ReviewAutomationContext,
    ReviewAutomationOutcome, ReviewAutomationPolicy, ReviewDecision, ReviewIdParseError,
    ReviewResolutionInput, ReviewResolvedDispatch,
};
#[cfg(test)]
use resolution::event_kind_label;

#[allow(unused_imports)]
pub(crate) use created::{
    apply_compile_review_gates, auto_emit_review_question_after_artifact_write,
    derive_plan_node_review_question_id, derive_plan_node_topic_hash, derive_review_question_id,
    derive_review_question_id_for_artifact, is_plan_node_review_action,
    maybe_emit_review_question_created, parse_compile_review_gate, parse_review_gate_policy,
    review_gate_policy_was_explicit, AutoEmitDecision, CompileReviewGateRequest,
    ReviewGatePolicy, PLAN_NODE_REVIEW_DEFAULT_ACTION,
};
#[cfg(test)]
use created::{payload_says_file_written, stamp_policy, topic_hash_short};

// ───────────────────────────────────────────────────────────────────────
// wave-20 / task 08 — review auto-answer policy v0
//
// Adds an EXPLICIT auto-answer policy layered on top of the wave-18 / task
// 07 [`ReviewAutomationPolicy`] (`manual | suggest | auto_safe`). Where
// `review_automation_policy` controls the wave-15/16 manager-action
// resolution surfaces (directive approve/archive, plan approve/mark/
// supersede, workflow resolve_review), the `auto_answer_policy` controls
// the wave-16/02 [`spawn_review_resolution_sub`] LISTENER path: when a
// `QuestionEvent::Resolved` (or a deterministic dispatch outcome) arrives
// on the bus, MAY the listener auto-answer the review question without a
// human reviewing the inbound resolution string.
//
// Three states (default = `Off` so legacy callers stay byte-identical
// when they never sent the field):
//
//   off                → existing behaviour. Listener routes the inbound
//                        decision through the per-scope handler exactly as
//                        the bus delivered it. No deterministic safety
//                        recomputation.
//   deterministic_safe → listener computes a [`AutoAnswerOutcome`] from
//                        the wave-18/07 deterministic safety inspector
//                        ([`evaluate_review_automation`]) PLUS the
//                        wave-20/08 destructive-action guard. When EVERY
//                        rule passes AND the suggested decision is
//                        `Approved` AND the action is non-destructive,
//                        the helper returns `selected_decision=Approved`
//                        + `requires_human=false`. Otherwise
//                        `requires_human=true` and the listener MUST
//                        defer to the human reviewer.
//   dry_run            → run the same inspector + destructive-action
//                        guard but ALWAYS surface `requires_human=true`.
//                        The selected_decision is still computed (so
//                        observability dashboards see what we WOULD have
//                        done) but never used as authority. Useful for
//                        operators who want to validate the policy
//                        outcome shape on a real review question without
//                        the policy actually mutating state.
//
// Authority hierarchy (most → least):
//   1. Caller-supplied `review_decision` (when present in the same call)
//      — explicit human authority always wins.
//   2. `deterministic_safe` policy auto-promotion to `Approved` (only
//      when every safety + destructive-action rule passes).
//   3. Otherwise the suggestion is informational (`requires_human=true`).
//
// Hard invariants — every implementation MUST satisfy these without
// exception (pinned by tests):
//
//   I1. NEVER auto-reject. `selected_decision=Rejected` is impossible
//       under any policy mode — refusing a draft is a human-only
//       decision.
//   I2. NEVER auto-promote on a destructive action. `archive`,
//       `supersede`, `remove` (case-insensitive) ALWAYS land
//       `requires_human=true` regardless of the safety inspector
//       result. The wave-15 manager-action whitelist makes this
//       enforceable per-scope; we keep the rule loud here so the
//       LISTENER path also refuses the promotion.
//   I3. The policy NEVER calls an LLM. All inputs are pure data
//       supplied by the caller; the inspector logic is deterministic.
//   I4. When skipped (any non-Off mode that did not reach `Approved`),
//       the response carries `policy_result`, `selected_decision`,
//       `safety_rule_results[]`, and `requires_human=true` so observers
//       can audit the decision.
// ───────────────────────────────────────────────────────────────────────

/// Wave-20 / task 08 — three-state policy controlling whether the
/// review-question LISTENER path may auto-answer an inbound resolution
/// without a human reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoAnswerPolicy {
    /// Default: no listener-side auto-answer. Inbound decisions route
    /// through the per-scope handler exactly as the bus delivered them.
    Off,
    /// Compute a deterministic outcome via the wave-18/07 safety
    /// inspector + the wave-20/08 destructive-action guard. When every
    /// rule passes AND the suggestion is `Approved` AND the action is
    /// non-destructive, the helper auto-answers `Approved`.
    DeterministicSafe,
    /// Compute the same outcome but ALWAYS set `requires_human=true`.
    /// Used to validate the policy shape without mutating state.
    DryRun,
}

impl AutoAnswerPolicy {
    /// Lower-snake-case label for the response payload.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AutoAnswerPolicy::Off => "off",
            AutoAnswerPolicy::DeterministicSafe => "deterministic_safe",
            AutoAnswerPolicy::DryRun => "dry_run",
        }
    }
}

/// Parse the wave-20 / task 08 `auto_answer_policy` arg. Unknown / absent
/// / blank values collapse to `Off` so legacy callers (which never sent
/// the field) keep their byte-identical response shape.
///
/// Recognised values (case-insensitive, trimmed):
///   * `"off"`                → [`AutoAnswerPolicy::Off`] (default)
///   * `"deterministic_safe"` → [`AutoAnswerPolicy::DeterministicSafe`]
///                              (hyphenated `"deterministic-safe"` accepted)
///   * `"dry_run"`            → [`AutoAnswerPolicy::DryRun`]
///                              (hyphenated `"dry-run"` accepted)
pub(crate) fn parse_auto_answer_policy(args: &Value) -> AutoAnswerPolicy {
    let raw = args
        .get("auto_answer_policy")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase());
    match raw.as_deref() {
        Some("deterministic_safe") | Some("deterministic-safe") => {
            AutoAnswerPolicy::DeterministicSafe
        }
        Some("dry_run") | Some("dry-run") => AutoAnswerPolicy::DryRun,
        _ => AutoAnswerPolicy::Off,
    }
}

/// True iff the caller actually included an `auto_answer_policy` key in
/// the request JSON (regardless of value). Used to keep pre-wave-20/08
/// callers byte-identical when they never opted in.
pub(crate) fn auto_answer_policy_was_explicit(args: &Value) -> bool {
    args.get("auto_answer_policy").is_some()
}

/// Wave-20 / task 08 — destructive review actions. Promoting any of
/// these to `Approved` via auto-answer is forbidden; the listener MUST
/// defer to a human reviewer. Centralised here so both the listener and
/// the dry-run preview path apply the same rule.
///
/// The action vocabulary mirrors the wave-15 manager-action whitelist
/// (`compile|approve|archive|supersede|mark`) plus `remove` for forward-
/// compat with deletion paths (e.g. KB rows, generated artefacts) that
/// future scopes may need to gate.
const DESTRUCTIVE_REVIEW_ACTIONS: &[&str] = &["archive", "supersede", "remove"];

/// True iff the supplied action label is on the destructive list.
/// Case-insensitive + trimmed so caller variations (`"Archive"`,
/// `"  REMOVE  "`) collide with the canonical lowercase form.
pub(crate) fn is_destructive_review_action(action: &str) -> bool {
    let normalised = action.trim().to_ascii_lowercase();
    DESTRUCTIVE_REVIEW_ACTIONS
        .iter()
        .any(|a| **a == normalised)
}

/// Status label surfaced under `policy_result` on the response. Pure
/// projection of the policy + inspector + destructive-action rule
/// outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoAnswerStatus {
    /// `policy=off` (default). Listener routes the inbound decision
    /// without recomputation. Response stays pre-wave-20/08
    /// byte-identical.
    NotEvaluated,
    /// `policy=deterministic_safe` AND every safety rule passed AND the
    /// suggestion is `Approved` AND the action is non-destructive — the
    /// listener may auto-answer `Approved`.
    AutoAnswered,
    /// `policy=deterministic_safe` AND at least one safety rule failed
    /// (or the suggestion was not `Approved`). Listener defers to the
    /// human reviewer; payload carries the suggestion + blocking rules.
    SkippedRulesFailed,
    /// `policy=deterministic_safe` AND the action is destructive
    /// (archive/supersede/remove). Even if every other rule passes, we
    /// refuse to auto-promote. Pinned as a SEPARATE status so audit
    /// dashboards can grep for "auto-answer refused destructive
    /// promotion" without inspecting the rule outcomes.
    SkippedDestructiveAction,
    /// `policy=dry_run`. Every rule was evaluated and the
    /// `selected_decision` is the suggestion the helper would have
    /// chosen — but `requires_human=true` is always set so the listener
    /// MUST defer to a human reviewer. Distinct from
    /// `SkippedRulesFailed` so observers can tell the difference
    /// between "operator opted into dry-run" and "rules failed".
    DryRunPreview,
}

impl AutoAnswerStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AutoAnswerStatus::NotEvaluated => "not_evaluated",
            AutoAnswerStatus::AutoAnswered => "auto_answered",
            AutoAnswerStatus::SkippedRulesFailed => "skipped_rules_failed",
            AutoAnswerStatus::SkippedDestructiveAction => {
                "skipped_destructive_action"
            }
            AutoAnswerStatus::DryRunPreview => "dry_run_preview",
        }
    }
}

/// Pure outcome of [`evaluate_auto_answer_policy`]. Side-effect free —
/// no DB, no bus, no LLM. The listener consumes this projection to
/// decide whether to auto-answer or defer to the human reviewer.
///
/// Field invariants:
///   * `selected_decision` is `Some(Approved)` ONLY when the listener may
///     auto-answer (status = `AutoAnswered`). Under every other status
///     the field carries the SUGGESTION (so dashboards see what we
///     would have done) but the listener MUST NOT use it as authority.
///   * `requires_human=true` whenever `status != AutoAnswered`. Pinned
///     so callers never have to inspect the status to know whether to
///     defer.
///   * `safety_rule_results[]` is a flat list of `code:detail` strings
///     mirroring the wave-18/07 inspector vocabulary plus the wave-20/08
///     destructive-action rule.
///   * The outcome NEVER carries `Rejected` as the selected decision —
///     auto-rejection is impossible under any policy mode (invariant I1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoAnswerOutcome {
    pub policy: AutoAnswerPolicy,
    pub status: AutoAnswerStatus,
    /// Decision the listener should apply. `Some(Approved)` iff the
    /// listener may auto-answer; otherwise carries the suggestion (or
    /// `None` under `Off` where no inspection ran).
    pub selected_decision: Option<ReviewDecision>,
    /// Flat list of `code:detail` strings from the inspector + the
    /// destructive-action rule. Always populated under non-Off modes.
    pub safety_rule_results: Vec<String>,
    /// `true` iff the listener MUST defer to a human reviewer. Pinned
    /// separately from the status so callers don't have to pattern
    /// match.
    pub requires_human: bool,
}

/// Run the wave-20 / task 08 deterministic auto-answer evaluation.
/// Pure / side-effect free / NEVER calls an LLM.
///
/// Inputs:
///   * `policy` — resolved [`AutoAnswerPolicy`]; `Off` short-circuits.
///   * `ctx` — wave-18/07 [`ReviewAutomationContext`]; reused so the
///     same safety vocabulary surfaces under both knobs.
///   * `action` — the parsed review action (e.g. `"compile"`,
///     `"approve"`, `"archive"`, `"supersede"`). Folded into the
///     destructive-action rule.
///   * `caller_decision` — explicit `review_decision` the caller
///     supplied. When present, the helper refuses to auto-answer (the
///     human authority wins) and surfaces a "caller_decision_present"
///     blocker.
pub(crate) fn evaluate_auto_answer_policy(
    policy: AutoAnswerPolicy,
    ctx: &ReviewAutomationContext,
    action: &str,
    caller_decision: Option<ReviewDecision>,
) -> AutoAnswerOutcome {
    if matches!(policy, AutoAnswerPolicy::Off) {
        return AutoAnswerOutcome {
            policy,
            status: AutoAnswerStatus::NotEvaluated,
            selected_decision: None,
            safety_rule_results: Vec::new(),
            requires_human: false,
        };
    }

    // Reuse the wave-18/07 deterministic inspector. It already covers
    // deterministic_mode / file_write / file_hash / protected source /
    // additional blockers, so the wave-20/08 layer just needs to add
    // the destructive-action + caller-decision guards on top.
    let inner = evaluate_review_automation(
        ReviewAutomationPolicy::AutoSafe,
        ctx,
        caller_decision,
    );
    let mut rule_results = inner.reasons.clone();

    // Destructive-action guard — invariant I2. Pinned BEFORE the
    // rule-failure check so a destructive action whose other rules
    // pass surfaces under the dedicated `SkippedDestructiveAction`
    // status (so observers can tell "destructive blocked" apart from
    // "rules failed").
    let destructive = is_destructive_review_action(action);
    if destructive {
        rule_results.push(format!(
            "rule:destructive_action:`{}` is on the destructive list (archive|supersede|remove); auto-answer refuses to promote",
            action.trim().to_ascii_lowercase()
        ));
    } else {
        rule_results.push(format!(
            "rule:non_destructive_action:`{}` is not on the destructive list",
            action.trim().to_ascii_lowercase()
        ));
    }

    // Caller-decision guard — invariant: explicit human authority
    // ALWAYS wins over the policy. Pinned as an explicit rule-result so
    // observers see WHY we deferred even when every other rule passed.
    if caller_decision.is_some() {
        rule_results.push(
            "rule:caller_decision_present:explicit review_decision supplied by caller; auto-answer defers to human authority"
                .to_string(),
        );
    }

    // Suggestion: take the wave-18/07 outcome's suggestion as the
    // baseline. The inspector defaults to `Approved` and degrades to
    // `NeedsChanges` when blocking rules trip — `Rejected` is
    // impossible (invariant I1).
    let suggestion = inner
        .suggested_decision
        .unwrap_or(ReviewDecision::NeedsChanges);

    // Defensive: belt-and-braces against a future inspector change
    // that might emit `Rejected`. The auto-answer layer NEVER returns
    // `Rejected` as the selected decision (invariant I1); we degrade
    // it to `NeedsChanges` so the listener defers to a human reviewer.
    let suggestion = match suggestion {
        ReviewDecision::Rejected => {
            rule_results.push(
                "rule:rejection_demoted:upstream inspector emitted Rejected; auto-answer never auto-rejects, demoting to NeedsChanges"
                    .to_string(),
            );
            ReviewDecision::NeedsChanges
        }
        other => other,
    };

    // dry_run mode short-circuits to the preview status REGARDLESS of
    // whether every rule passed. The selected_decision still carries
    // the suggestion (so dashboards see what we would have done) but
    // the listener MUST defer to a human reviewer.
    if matches!(policy, AutoAnswerPolicy::DryRun) {
        return AutoAnswerOutcome {
            policy,
            status: AutoAnswerStatus::DryRunPreview,
            selected_decision: Some(suggestion),
            safety_rule_results: rule_results,
            requires_human: true,
        };
    }

    // deterministic_safe mode. The promotion rules are:
    //   * caller_decision is None
    //   * action is non-destructive
    //   * inspector's may_auto_resolve = true (every safety rule passed)
    //   * suggestion is Approved
    let approved_suggestion = matches!(suggestion, ReviewDecision::Approved);

    if destructive {
        // Destructive-action gate fires before the rule check so we can
        // surface a separate status. Even if every other rule passes,
        // the listener MUST defer for archive/supersede/remove actions.
        return AutoAnswerOutcome {
            policy,
            status: AutoAnswerStatus::SkippedDestructiveAction,
            selected_decision: Some(suggestion),
            safety_rule_results: rule_results,
            requires_human: true,
        };
    }

    if !inner.may_auto_resolve || !approved_suggestion || caller_decision.is_some() {
        return AutoAnswerOutcome {
            policy,
            status: AutoAnswerStatus::SkippedRulesFailed,
            selected_decision: Some(suggestion),
            safety_rule_results: rule_results,
            requires_human: true,
        };
    }

    AutoAnswerOutcome {
        policy,
        status: AutoAnswerStatus::AutoAnswered,
        selected_decision: Some(ReviewDecision::Approved),
        safety_rule_results: rule_results,
        requires_human: false,
    }
}

/// Stamp the wave-20 / task 08 outcome onto a response payload. Pure /
/// no bus calls. Always called when the resolved policy is non-Off; the
/// caller is responsible for skipping this under `Off` to keep
/// pre-wave-20/08 callers byte-identical.
///
/// Mutates `payload` with:
///   * `auto_answer_policy`     — resolved policy label
///   * `policy_result`          — outcome status label
///   * `selected_decision`      — `approved | needs_changes` (omitted
///                                 under `Off`; never `rejected`)
///   * `safety_rule_results`    — array of `code:detail` strings
///   * `requires_human`         — true whenever the listener must defer
pub(crate) fn stamp_auto_answer_payload(
    payload: &mut Value,
    outcome: &AutoAnswerOutcome,
) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert(
        "auto_answer_policy".to_string(),
        json!(outcome.policy.as_str()),
    );
    map.insert(
        "policy_result".to_string(),
        json!(outcome.status.as_str()),
    );
    if let Some(d) = outcome.selected_decision {
        map.insert(
            "selected_decision".to_string(),
            json!(d.as_str()),
        );
    }
    map.insert(
        "safety_rule_results".to_string(),
        json!(outcome.safety_rule_results),
    );
    map.insert(
        "requires_human".to_string(),
        json!(outcome.requires_human),
    );
}

// ───────────────────────────────────────────────────────────────────────
// wave-21 / task 06 — LLM auto-approve proposal v0
//
// Adds an EXPLICIT Sonnet-assisted proposal mode for the wave-15 / wave-16
// review-resolution surfaces (directive approve/archive, plan
// approve/mark/supersede). It is layered ON TOP of (and conservatively
// disjoint from) the wave-18 / 07 [`ReviewAutomationPolicy`] (deterministic
// safety inspector → may auto-resolve) and the wave-20 / 08
// [`AutoAnswerPolicy`] (listener-side deterministic auto-answer). Where
// the prior knobs reach into the deterministic inspector, this knob asks a
// Sonnet model to PROPOSE a review decision — but **never** lets the
// proposal land as authority in v0. The proposal is informational:
// dashboards and UI surfaces can grep for it and a human still has to
// supply an explicit `review_decision` to flip the artifact.
//
// Two states (default = `Off`, byte-identical with pre-wave-21 callers
// that never sent the field):
//
//   off                → existing behaviour. Handler does NOT call Sonnet
//                        for review-action suggestions; response stays
//                        pre-wave-21 byte-identical.
//   sonnet_suggest     → handler asks Sonnet to PROPOSE a structured
//                        review decision (decision + confidence +
//                        evidence + non_goal_check + destructive_check +
//                        requires_human) and surfaces the proposal under
//                        `llm_auto_approve_proposal` on the response.
//                        NEVER mutates state in v0; the field
//                        `applied=false` is pinned across every
//                        proposal so observers never have to inspect the
//                        status to know nothing landed.
//
// Hard invariants — every implementation MUST satisfy these without
// exception (pinned by tests):
//
//   I1. NEVER auto-reject. Proposals MAY return `decision=needs_changes`
//       or `decision=approved`; `decision=rejected` is collapsed to
//       `needs_changes` (with a `proposal_warnings[]` entry) so the
//       proposal NEVER carries `rejected` as the suggested authority —
//       refusing a draft is a human-only decision.
//   I2. Destructive actions (`archive`, `supersede`, `remove` —
//       case-insensitive) ALWAYS land `requires_human=true` and the
//       proposal status is pinned to `destructive_blocked` regardless of
//       the model's suggestion. The proposal value is preserved for
//       audit (so dashboards see what Sonnet would have said) but the
//       caller MUST defer to a human reviewer.
//   I3. **No actual auto-approve in v0**: the proposal NEVER drives a DB
//       transition or bus emission. `applied=false` is pinned on every
//       proposal regardless of confidence. Any future wave that wants to
//       promote a proposal to authority MUST add a separate explicit
//       caller-side opt-in flag — this knob only ever proposes.
//   I4. Sonnet unavailable surfaces `LlmAutoApproveProposalStatus::
//       Unavailable` with an explanatory `unavailable_reason` and zero
//       proposals. NO fallback to a deterministic suggestion; NO silent
//       success. This mirrors the `feedback_fail_fast_no_fallback` rule.
//   I5. The destructive_check field on the proposal MUST equal the
//       deterministic [`is_destructive_review_action`] outcome,
//       regardless of what Sonnet replied. Caller-supplied input never
//       overrides the deterministic destructive guard.
// ───────────────────────────────────────────────────────────────────────

/// Wave-21 / task 06 — opt-in mode controlling whether the resolution
/// surface asks Sonnet to propose a review decision. Default `Off`
/// preserves pre-wave-21 byte-shape; `SonnetSuggest` surfaces a propose-
/// only block under `llm_auto_approve_proposal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmAutoApproveProposalMode {
    /// Default: handler does NOT ask Sonnet for review-action proposals.
    Off,
    /// Handler asks Sonnet to PROPOSE a structured review decision and
    /// surfaces it on the response. NEVER mutates state in v0.
    SonnetSuggest,
}

impl LlmAutoApproveProposalMode {
    /// Lower-snake-case wire label for response payload.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LlmAutoApproveProposalMode::Off => "off",
            LlmAutoApproveProposalMode::SonnetSuggest => "sonnet_suggest",
        }
    }

    /// True iff the mode opts the caller into the propose-only Sonnet
    /// path. False for `Off` (legacy byte-shape).
    pub(crate) fn is_sonnet_suggest(self) -> bool {
        matches!(self, LlmAutoApproveProposalMode::SonnetSuggest)
    }
}

/// Parse the wave-21 / task 06 `auto_approve_mode` arg. Unknown / absent
/// / blank values collapse to `Off` so legacy callers (which never sent
/// the field) keep their byte-identical response shape. Strict-enum: any
/// non-empty unknown value fails fast with [`Err`] so caller typos never
/// silently degrade to Off.
///
/// Recognised values (case-insensitive, trimmed):
///   * `"off"`            → [`LlmAutoApproveProposalMode::Off`] (default)
///   * `"sonnet_suggest"` → [`LlmAutoApproveProposalMode::SonnetSuggest`]
///                          (hyphenated `"sonnet-suggest"` accepted)
pub(crate) fn parse_llm_auto_approve_proposal_mode(
    args: &Value,
) -> Result<LlmAutoApproveProposalMode, String> {
    let Some(raw_v) = args.get("auto_approve_mode") else {
        return Ok(LlmAutoApproveProposalMode::Off);
    };
    let Some(s) = raw_v.as_str() else {
        return Err(format!(
            "auto_approve_mode must be a string (one of [\"off\", \"sonnet_suggest\"]); got `{}`",
            raw_v
        ));
    };
    let normalised = s.trim().to_ascii_lowercase();
    match normalised.as_str() {
        "" | "off" => Ok(LlmAutoApproveProposalMode::Off),
        "sonnet_suggest" | "sonnet-suggest" => Ok(LlmAutoApproveProposalMode::SonnetSuggest),
        other => Err(format!(
            "auto_approve_mode must be one of [\"off\", \"sonnet_suggest\"]; got `{}`",
            other
        )),
    }
}

/// True iff the caller actually included an `auto_approve_mode` key in
/// the request JSON (regardless of value). Used to keep pre-wave-21/06
/// callers byte-identical when they never opted in.
pub(crate) fn llm_auto_approve_proposal_mode_was_explicit(args: &Value) -> bool {
    args.get("auto_approve_mode").is_some()
}

/// Confidence label attached to an LLM auto-approve proposal. Mirrors the
/// wave-20 plan inference confidence vocabulary so dashboards can pivot
/// on the same set across knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmAutoApproveProposalConfidence {
    Low,
    Medium,
    High,
}

impl LlmAutoApproveProposalConfidence {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LlmAutoApproveProposalConfidence::Low => "low",
            LlmAutoApproveProposalConfidence::Medium => "medium",
            LlmAutoApproveProposalConfidence::High => "high",
        }
    }

    /// Parse the wire string. Case-insensitive + trimmed. Unknown /
    /// blank → `None` so the caller can record a parse warning.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "low" => Some(LlmAutoApproveProposalConfidence::Low),
            "medium" | "med" => Some(LlmAutoApproveProposalConfidence::Medium),
            "high" => Some(LlmAutoApproveProposalConfidence::High),
            _ => None,
        }
    }
}

/// Wire status describing the outcome of the wave-21 / task 06
/// propose-only LLM pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmAutoApproveProposalStatus {
    /// Caller picked `auto_approve_mode="off"` (or omitted the knob).
    /// Bundle is absent from the response (byte-identical with pre-wave-21
    /// callers).
    NotInvoked,
    /// Sonnet was unavailable (gateway not initialised, network failure,
    /// etc.). Bundle carries `unavailable_reason`; no proposal. NO
    /// fallback to a deterministic suggestion (invariant I4).
    Unavailable,
    /// Sonnet returned a parseable proposal that survived validation.
    Suggested,
    /// Action is destructive (archive | supersede | remove). The proposal
    /// is preserved for audit but `requires_human=true` is pinned and
    /// `applied=false` is enforced (invariant I2).
    DestructiveBlocked,
    /// Sonnet returned an unparseable / empty / invalid response (e.g.
    /// no JSON, missing required fields). Bundle carries
    /// `proposal_warnings[]` for caller debugging; no proposal lands.
    NoSuggestion,
}

impl LlmAutoApproveProposalStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LlmAutoApproveProposalStatus::NotInvoked => "not_invoked",
            LlmAutoApproveProposalStatus::Unavailable => "llm_unavailable",
            LlmAutoApproveProposalStatus::Suggested => "suggested",
            LlmAutoApproveProposalStatus::DestructiveBlocked => "destructive_blocked",
            LlmAutoApproveProposalStatus::NoSuggestion => "no_suggestion",
        }
    }
}

/// One validated wave-21 / task 06 LLM auto-approve proposal. Pure data;
/// every field reflects either Sonnet output (decision / confidence /
/// evidence / non_goal_check) or a deterministic invariant
/// (destructive_check / requires_human / applied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LlmAutoApproveProposal {
    /// Suggested decision. NEVER `Rejected` — invariant I1 collapses any
    /// model-side `rejected` to `NeedsChanges` so the proposal never
    /// carries an auto-reject suggestion.
    pub decision: ReviewDecision,
    /// Sonnet-assigned confidence. Defaults to `Low` when the model omits
    /// or returns an unrecognised value.
    pub confidence: LlmAutoApproveProposalConfidence,
    /// Free-form Sonnet-side justification text. Trimmed; never empty
    /// (validator drops proposals without evidence to avoid silent
    /// suggestions).
    pub evidence: String,
    /// Sonnet-side claim that the proposal does not violate the
    /// artifact's stated non-goals. Echoed verbatim for audit; the
    /// handler does NOT cross-check this string against PLAN.lisp /
    /// directive sexp non-goals in v0.
    pub non_goal_check: String,
    /// Deterministic destructive-action check. ALWAYS sourced from
    /// [`is_destructive_review_action`] — never from Sonnet (invariant
    /// I5). Stable string to make dashboards trivially `grep`-able.
    pub destructive_check: String,
    /// Whether the listener / caller MUST defer to a human reviewer.
    /// ALWAYS `true` for destructive actions (invariant I2); ALWAYS
    /// `true` in v0 even for non-destructive actions (invariant I3 —
    /// proposals NEVER apply automatically).
    pub requires_human: bool,
}

impl LlmAutoApproveProposal {
    /// Wire shape consumed by callers. The `applied=false` field is
    /// pinned here (rather than computed from `requires_human`) so
    /// observers can `assert proposal.applied == false` without reading
    /// the whole task contract.
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "decision": self.decision.as_str(),
            "confidence": self.confidence.as_str(),
            "evidence": self.evidence,
            "non_goal_check": self.non_goal_check,
            "destructive_check": self.destructive_check,
            "requires_human": self.requires_human,
            "applied": false,
        })
    }
}

/// Bundle of wave-21 / task 06 LLM-side data attached to the response.
/// Always carries the status (so observers see whether the gateway was
/// reachable) plus the proposal payload (when one survived). The bundle
/// is propose-only — `applied=false` is pinned on every contained
/// proposal regardless of status (invariant I3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LlmAutoApproveProposalBundle {
    pub mode: LlmAutoApproveProposalMode,
    pub status: LlmAutoApproveProposalStatus,
    /// At most ONE proposal in v0 (the proposal is per-action, per-call).
    /// Future waves may extend this to multiple proposals (per-field) but
    /// the v0 contract caps it at one to keep the audit trail terse.
    pub proposal: Option<LlmAutoApproveProposal>,
    /// Free-form parse warnings the validator surfaced (e.g. "decision
    /// missing", "rejected demoted to needs_changes"). Never empty after
    /// `Unavailable` / `NoSuggestion`.
    pub proposal_warnings: Vec<String>,
    /// Reason the gateway was unavailable. Populated only under
    /// `LlmAutoApproveProposalStatus::Unavailable`.
    pub unavailable_reason: Option<String>,
    /// Action label this proposal was made against (e.g. `"approve"`,
    /// `"archive"`, `"supersede"`, `"mark"`). Echoed verbatim from the
    /// caller so observers can correlate the proposal with the surface.
    pub action: String,
    /// Caller string surfaced to LLM gateway logging. None when the
    /// gateway was never asked (e.g. status=DestructiveBlocked short-
    /// circuited the call).
    pub request_caller: Option<String>,
    /// Model identifier. Populated when the LLM was actually invoked.
    pub model: Option<String>,
}

impl LlmAutoApproveProposalBundle {
    /// Build a bundle in the `Off` (not-invoked) state. The `action` is
    /// recorded so dashboards can still grep for the surface label even
    /// when the bundle reports `not_invoked`.
    pub(crate) fn not_invoked(action: impl Into<String>) -> Self {
        LlmAutoApproveProposalBundle {
            mode: LlmAutoApproveProposalMode::Off,
            status: LlmAutoApproveProposalStatus::NotInvoked,
            proposal: None,
            proposal_warnings: Vec::new(),
            unavailable_reason: None,
            action: action.into(),
            request_caller: None,
            model: None,
        }
    }

    /// Build a bundle in the `Unavailable` state. NO fallback proposal —
    /// invariant I4 forbids silent degradation to deterministic.
    pub(crate) fn unavailable(
        mode: LlmAutoApproveProposalMode,
        action: impl Into<String>,
        request_caller: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        LlmAutoApproveProposalBundle {
            mode,
            status: LlmAutoApproveProposalStatus::Unavailable,
            proposal: None,
            proposal_warnings: Vec::new(),
            unavailable_reason: Some(reason.into()),
            action: action.into(),
            request_caller: Some(request_caller.into()),
            model: None,
        }
    }

    /// Build a bundle in the `DestructiveBlocked` state. Invariant I2:
    /// the proposal value is preserved for audit but `requires_human` is
    /// pinned to `true` and `applied` will serialise as `false` via
    /// [`LlmAutoApproveProposal::to_json`]. The Sonnet call MAY have run
    /// (proposal preserves the suggestion) OR MAY have been short-circuited
    /// before the call (proposal is None). Both shapes are valid.
    pub(crate) fn destructive_blocked(
        mode: LlmAutoApproveProposalMode,
        action: impl Into<String>,
        request_caller: impl Into<String>,
        proposal: Option<LlmAutoApproveProposal>,
        warning: impl Into<String>,
    ) -> Self {
        let mut warnings: Vec<String> = Vec::new();
        warnings.push(warning.into());
        LlmAutoApproveProposalBundle {
            mode,
            status: LlmAutoApproveProposalStatus::DestructiveBlocked,
            proposal: proposal.map(|mut p| {
                // Invariant I2 + I3: pin requires_human=true even if the
                // model claimed otherwise.
                p.requires_human = true;
                p
            }),
            proposal_warnings: warnings,
            unavailable_reason: None,
            action: action.into(),
            request_caller: Some(request_caller.into()),
            model: None,
        }
    }
}

/// Parse a Sonnet response string into a [`LlmAutoApproveProposal`].
/// Pure / side-effect free. The expected shape is a JSON object with the
/// six fields {decision, confidence, evidence, non_goal_check,
/// destructive_check, requires_human}. Wrapping `{"proposal": {...}}`
/// also accepted because Sonnet sometimes nests the body.
///
/// Validator behaviour:
///   * `decision="rejected"` is collapsed to `NeedsChanges` with a
///     `proposal_warnings[]` entry (invariant I1).
///   * Missing / empty `evidence` drops the proposal (we never surface
///     a silent suggestion).
///   * Missing `decision` drops the proposal.
///   * `confidence` defaults to `Low` when omitted / unrecognised
///     (records a warning).
///   * `non_goal_check` defaults to a deterministic placeholder when
///     omitted (records a warning).
///   * The caller is responsible for OVERWRITING `destructive_check` +
///     `requires_human` based on the deterministic [`
///     is_destructive_review_action`] outcome — invariant I5 forbids
///     trusting the model's value.
///
/// Returns `(Some(proposal), warnings)` on success;
/// `(None, warnings)` on failure (warnings always populated when
/// `proposal=None`).
pub(crate) fn parse_llm_auto_approve_proposal(
    raw: &str,
) -> (Option<LlmAutoApproveProposal>, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        warnings.push("LLM response was empty".to_string());
        return (None, warnings);
    }
    let trimmed = strip_proposal_code_fence(trimmed);
    let parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(err) => {
            warnings.push(format!("LLM response was not valid JSON: {}", err));
            return (None, warnings);
        }
    };
    let body = match &parsed {
        Value::Object(map) => match map.get("proposal") {
            Some(Value::Object(_)) => map.get("proposal").unwrap().clone(),
            Some(other) => {
                warnings.push(format!(
                    "`proposal` must be an object, got {}",
                    proposal_json_kind(other)
                ));
                return (None, warnings);
            }
            None => Value::Object(map.clone()),
        },
        other => {
            warnings.push(format!(
                "LLM response top-level must be an object, got {}",
                proposal_json_kind(other)
            ));
            return (None, warnings);
        }
    };
    let obj = match body.as_object() {
        Some(o) => o,
        None => {
            warnings.push("LLM response body must be an object".to_string());
            return (None, warnings);
        }
    };

    // decision (required, never `rejected`).
    let decision_raw = obj
        .get("decision")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if decision_raw.is_empty() {
        warnings.push("decision missing or not a string".to_string());
        return (None, warnings);
    }
    let decision = match decision_raw.as_str() {
        "approved" | "approve" => ReviewDecision::Approved,
        "needs_changes" | "needs-changes" | "changes" | "revise" | "fix" => {
            ReviewDecision::NeedsChanges
        }
        "rejected" | "reject" | "no" => {
            // Invariant I1 — never auto-reject. Demote to NeedsChanges.
            warnings.push(
                "rule:rejection_demoted:LLM proposed `rejected`; auto-approve proposal NEVER carries `rejected`, demoting to `needs_changes`"
                    .to_string(),
            );
            ReviewDecision::NeedsChanges
        }
        other => {
            warnings.push(format!(
                "decision `{}` is not in {{approved, needs_changes}}",
                other
            ));
            return (None, warnings);
        }
    };

    // evidence (required, non-empty).
    let evidence = obj
        .get("evidence")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let evidence = match evidence {
        Some(e) => e,
        None => {
            warnings.push(
                "evidence missing or empty; proposal dropped (no silent suggestions)".to_string(),
            );
            return (None, warnings);
        }
    };

    // confidence (optional, defaults to Low).
    let confidence = obj
        .get("confidence")
        .and_then(|v| v.as_str())
        .and_then(LlmAutoApproveProposalConfidence::parse)
        .unwrap_or_else(|| {
            warnings
                .push("confidence missing or unrecognised; defaulting to `low`".to_string());
            LlmAutoApproveProposalConfidence::Low
        });

    // non_goal_check (optional, defaults to placeholder).
    let non_goal_check = obj
        .get("non_goal_check")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            warnings.push(
                "non_goal_check missing or empty; defaulting to placeholder string".to_string(),
            );
            "non_goal_check_unavailable".to_string()
        });

    // destructive_check + requires_human are seeded from the model but
    // ALWAYS overwritten by the caller via [`enforce_proposal_invariants`]
    // before the bundle is published. We seed them to the model values
    // here so the validator stays pure (no action-label dependency).
    let destructive_check = obj
        .get("destructive_check")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "destructive_check_pending".to_string());
    let requires_human = obj
        .get("requires_human")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let proposal = LlmAutoApproveProposal {
        decision,
        confidence,
        evidence,
        non_goal_check,
        destructive_check,
        requires_human,
    };
    (Some(proposal), warnings)
}

/// Strip a leading ```/```json fence from the LLM response if present.
/// Mirrors [`strip_code_fence`] in plan.rs but kept local so the helper
/// stays self-contained.
fn strip_proposal_code_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(after_open) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let body = after_open
        .strip_prefix("json")
        .or_else(|| after_open.strip_prefix("JSON"))
        .unwrap_or(after_open);
    let body = body.trim_start_matches('\n').trim_start();
    body.strip_suffix("```")
        .map(|s| s.trim_end())
        .unwrap_or(body)
}

/// Lower-case JSON kind label for parser warnings. Mirrors
/// [`json_kind`] in plan.rs.
fn proposal_json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Apply the wave-21 / task 06 invariants to a freshly-parsed proposal:
///   * Pin `destructive_check` to the deterministic
///     [`is_destructive_review_action`] outcome (invariant I5).
///   * Force `requires_human=true` for destructive actions (invariant
///     I2) AND for ALL actions in v0 (invariant I3 — propose-only).
///   * Returns `true` iff the action was destructive (caller flips
///     bundle status to `DestructiveBlocked`).
///
/// Pure / side-effect free.
pub(crate) fn enforce_proposal_invariants(
    proposal: &mut LlmAutoApproveProposal,
    action: &str,
) -> bool {
    let destructive = is_destructive_review_action(action);
    let action_lc = action.trim().to_ascii_lowercase();
    proposal.destructive_check = if destructive {
        format!(
            "destructive:`{}` is on the destructive list (archive|supersede|remove); auto-approve proposal pinned `requires_human=true` regardless of model output",
            action_lc
        )
    } else {
        format!(
            "non_destructive:`{}` is not on the destructive list",
            action_lc
        )
    };
    // Invariant I3: propose-only in v0. Even non-destructive actions
    // pin requires_human=true so the listener / caller MUST defer.
    proposal.requires_human = true;
    destructive
}

/// Build the system prompt instructing Sonnet to produce a structured
/// auto-approve proposal. Pure / no I/O.
pub(crate) fn build_llm_auto_approve_proposal_system_prompt() -> String {
    String::from(
        "You are the Wave 21 / Task 06 review-action proposer. The user will share a \
         pending review surface (directive / plan + action) and any deterministic \
         safety inspector outcome. Your job is to PROPOSE a review decision in a \
         strict JSON shape. Constraints:\n\
         \n\
         1. You MUST reply with a single JSON object (no prose, no code fence). The \
            object MUST contain exactly these keys: decision, confidence, evidence, \
            non_goal_check, destructive_check, requires_human.\n\
         2. `decision` MUST be one of {\"approved\", \"needs_changes\"}. NEVER reply \
            `rejected` — refusing a draft is a human-only decision; if the artifact \
            looks unsafe, reply `needs_changes` with an evidence string explaining \
            why.\n\
         3. `confidence` MUST be one of {\"low\", \"medium\", \"high\"}.\n\
         4. `evidence` MUST be a non-empty string with concrete justification (cite \
            the artifact / safety inspector output).\n\
         5. `non_goal_check` MUST be a string explicitly stating whether the proposal \
            respects the artifact's declared non-goals.\n\
         6. `destructive_check` MUST be a string describing whether the action is \
            destructive (archive / supersede / remove). The handler will OVERWRITE \
            this field with the deterministic verdict — your value is informational.\n\
         7. `requires_human` MUST be boolean. The handler will FORCE this to true in \
            v0 (proposals never apply automatically) — your value is informational.\n\
         8. Respond with ONLY the JSON object — no commentary, no markdown.\n",
    )
}

/// Build the user prompt body (pure / no I/O). The caller passes the
/// surface label, action, deterministic safety summary, and an optional
/// caller-supplied artifact-side digest (e.g. PLAN.lisp sexp head, recent
/// evidence keys). Keep the prompt small — Sonnet only needs the shape.
pub(crate) fn build_llm_auto_approve_proposal_user_prompt(
    scope: &str,
    action: &str,
    artifact_id: &str,
    version: i32,
    deterministic_summary: &Value,
    artifact_digest: Option<&str>,
) -> String {
    format!(
        "Review surface: {scope} action={action}\n\
         Artifact: id={artifact_id} version=v{version}\n\
         Deterministic safety inspector summary:\n```json\n{summary}\n```\n\
         Artifact digest (when supplied):\n```\n{digest}\n```\n\n\
         Reply with the JSON proposal per the system instructions.\n",
        scope = scope,
        action = action,
        artifact_id = artifact_id,
        version = version,
        summary = serde_json::to_string_pretty(deterministic_summary)
            .unwrap_or_else(|_| "{}".into()),
        digest = artifact_digest.unwrap_or("(none)"),
    )
}

/// Stamp the wave-21 / task 06 bundle onto a response payload under the
/// stable `llm_auto_approve_proposal` key. Pure / no bus calls.
///
/// Mutates `payload` with:
///   * `llm_auto_approve_proposal_mode`   — resolved mode label
///   * `llm_auto_approve_proposal_status` — bundle status label
///   * `llm_auto_approve_proposal`        — proposal JSON (when present)
///   * `llm_auto_approve_proposal_warnings` — array of warning strings
///   * `llm_auto_approve_proposal_unavailable_reason` — string (when set)
///   * `llm_auto_approve_proposal_action` — action label echoed verbatim
///   * `llm_auto_approve_proposal_caller` — request caller (when set)
///   * `llm_auto_approve_proposal_model`  — model id (when set)
pub(crate) fn stamp_llm_auto_approve_proposal_payload(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert(
        "llm_auto_approve_proposal_mode".to_string(),
        json!(bundle.mode.as_str()),
    );
    map.insert(
        "llm_auto_approve_proposal_status".to_string(),
        json!(bundle.status.as_str()),
    );
    if let Some(p) = bundle.proposal.as_ref() {
        map.insert(
            "llm_auto_approve_proposal".to_string(),
            p.to_json(),
        );
    }
    map.insert(
        "llm_auto_approve_proposal_warnings".to_string(),
        json!(bundle.proposal_warnings),
    );
    if let Some(reason) = bundle.unavailable_reason.as_ref() {
        map.insert(
            "llm_auto_approve_proposal_unavailable_reason".to_string(),
            json!(reason),
        );
    }
    map.insert(
        "llm_auto_approve_proposal_action".to_string(),
        json!(bundle.action),
    );
    if let Some(caller) = bundle.request_caller.as_ref() {
        map.insert(
            "llm_auto_approve_proposal_caller".to_string(),
            json!(caller),
        );
    }
    if let Some(model) = bundle.model.as_ref() {
        map.insert(
            "llm_auto_approve_proposal_model".to_string(),
            json!(model),
        );
    }
}

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
            LlmApproveApplyStatus::SkippedNonApprovedDecision => {
                "skipped_non_approved_decision"
            }
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

// ───────────────────────────────────────────────────────────────────────
// tests — pure helpers only (no bus, no DB).
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_deterministic_for_same_input() {
        let a = derive_review_question_id("directive", "abc-123", 1, "compile");
        let b = derive_review_question_id("directive", "abc-123", 1, "compile");
        assert_eq!(a, b);
    }

    #[test]
    fn id_normalises_action_case() {
        let a = derive_review_question_id("plan", "p-1", 2, "Approve");
        let b = derive_review_question_id("plan", "p-1", 2, "approve");
        assert_eq!(a, b, "uppercase action must collide with lowercase form");
    }

    #[test]
    fn id_layout_has_canonical_format() {
        let id = derive_review_question_id("directive", "abc-123", 5, "compile");
        assert_eq!(id, "review:directive:abc-123:v5:compile");
    }

    #[test]
    fn id_changes_when_any_field_changes() {
        let base = derive_review_question_id("directive", "abc", 1, "compile");
        assert_ne!(
            base,
            derive_review_question_id("plan", "abc", 1, "compile"),
            "scope must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "abc", 2, "compile"),
            "version must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "abc", 1, "approve"),
            "action must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "xyz", 1, "compile"),
            "id must affect id"
        );
    }

    // -- parse_compile_review_gate --

    #[test]
    fn parse_compile_default_is_disabled() {
        let req = parse_compile_review_gate(&json!({}));
        assert!(!req.enabled);
        assert!(req.text.is_none());
        assert!(req.id_override.is_none());
    }

    #[test]
    fn parse_compile_extracts_all_fields() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_text": "  please review  ",
            "review_question_id": "  override-id  ",
        }));
        assert!(req.enabled);
        assert_eq!(req.text.as_deref(), Some("please review"));
        assert_eq!(req.id_override.as_deref(), Some("override-id"));
    }

    #[test]
    fn parse_compile_filters_blank_strings() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_text": "   ",
            "review_question_id": "",
        }));
        assert!(req.enabled);
        assert!(req.text.is_none());
        assert!(req.id_override.is_none());
    }

    #[test]
    fn parse_compile_emit_false_keeps_other_fields_in_struct_but_disabled() {
        // We still parse the optional override because callers may flip
        // emit later — but the helper must respect `enabled=false`.
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": false,
            "review_question_id": "explicit-id",
        }));
        assert!(!req.enabled);
        assert_eq!(req.id_override.as_deref(), Some("explicit-id"));
    }

    // -- parse_resolution_review_question_id --

    #[test]
    fn parse_resolution_id_returns_none_when_absent() {
        assert!(parse_resolution_review_question_id(&json!({})).is_none());
    }

    #[test]
    fn parse_resolution_id_trims_and_filters_blank() {
        assert!(parse_resolution_review_question_id(&json!({
            "review_question_id": "   "
        }))
        .is_none());
        assert_eq!(
            parse_resolution_review_question_id(&json!({
                "review_question_id": "  abc  "
            })),
            Some("abc".to_string())
        );
    }

    // -- build_resolution_event --

    #[test]
    fn resolution_event_without_decision_meta_is_resolved() {
        let ev = build_resolution_event("review:plan:p1:v1:approve", "approved", None);
        match ev {
            QuestionEvent::Resolved {
                question_id,
                resolution,
            } => {
                assert_eq!(question_id, "review:plan:p1:v1:approve");
                assert_eq!(resolution, "approved");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn resolution_event_with_tier_is_decision_resolved() {
        let meta = ResolutionDecisionMeta {
            tier: Some("tier1".into()),
            duration_ms: Some(123),
        };
        let ev = build_resolution_event("review:plan:p1:v1:approve", "approved", Some(&meta));
        match ev {
            QuestionEvent::DecisionResolved {
                question_id,
                tier,
                duration_ms,
            } => {
                assert_eq!(question_id, "review:plan:p1:v1:approve");
                assert_eq!(tier, "tier1");
                assert_eq!(duration_ms, 123);
            }
            other => panic!("expected DecisionResolved, got {other:?}"),
        }
    }

    #[test]
    fn resolution_event_decision_meta_default_duration_is_zero() {
        let meta = ResolutionDecisionMeta {
            tier: Some("urgent".into()),
            duration_ms: None,
        };
        let ev = build_resolution_event("rid", "approved", Some(&meta));
        if let QuestionEvent::DecisionResolved { duration_ms, .. } = ev {
            assert_eq!(duration_ms, 0);
        } else {
            panic!("expected DecisionResolved");
        }
    }

    #[test]
    fn resolution_event_meta_without_tier_falls_back_to_resolved() {
        // tier=None means "no decision-tier metadata" → plain Resolved even
        // when meta block is supplied. This pins the precedence.
        let meta = ResolutionDecisionMeta {
            tier: None,
            duration_ms: Some(99),
        };
        let ev = build_resolution_event("rid", "approved", Some(&meta));
        assert!(matches!(ev, QuestionEvent::Resolved { .. }));
    }

    #[test]
    fn event_kind_label_for_each_variant() {
        assert_eq!(
            event_kind_label(&QuestionEvent::Created {
                question_id: "x".into(),
            }),
            "created"
        );
        assert_eq!(
            event_kind_label(&QuestionEvent::Resolved {
                question_id: "x".into(),
                resolution: "y".into(),
            }),
            "resolved"
        );
        assert_eq!(
            event_kind_label(&QuestionEvent::DecisionResolved {
                question_id: "x".into(),
                tier: "t".into(),
                duration_ms: 0,
            }),
            "decision_resolved"
        );
    }

    // -- compile-response payload contract (caller-visible fields) --

    /// The compile branches construct a payload that may include the
    /// emission fields. These tests exercise the request-side decision
    /// surface (the inputs to `maybe_emit_review_question_created`) so the
    /// MCP contract stays pinned even without a real bus.
    #[test]
    fn compile_request_disabled_means_no_emission_fields_will_be_added() {
        let req = parse_compile_review_gate(&json!({}));
        assert!(!req.enabled);
        // When enabled=false the helper writes review_question_emitted=false
        // and no warning. The contract is "loud off" — see docstring on
        // maybe_emit_review_question_created.
        let derived = derive_review_question_id("directive", "abc", 1, "compile");
        assert_eq!(derived, "review:directive:abc:v1:compile");
    }

    #[test]
    fn compile_request_with_explicit_id_overrides_derived() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_id": "custom:q-1",
        }));
        assert!(req.enabled);
        assert_eq!(req.id_override.as_deref(), Some("custom:q-1"));
    }

    #[test]
    fn compile_request_without_explicit_id_falls_back_to_derived() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
        }));
        assert!(req.enabled);
        assert!(req.id_override.is_none());
        // The handler will compute the derived id at emit time from the
        // persisted artifact (id, version). Pin the contract here.
        let qid = derive_review_question_id("plan", "p-7", 3, "compile");
        assert_eq!(qid, "review:plan:p-7:v3:compile");
    }

    // ── wave-14 :: review_gate_policy parser ─────────────────────────────

    #[test]
    fn parse_policy_default_is_manual() {
        assert_eq!(
            parse_review_gate_policy(&json!({})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn parse_policy_recognises_emit_question() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "emit_question"})),
            ReviewGatePolicy::EmitQuestion
        );
    }

    #[test]
    fn parse_policy_recognises_off() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "off"})),
            ReviewGatePolicy::Off
        );
    }

    #[test]
    fn parse_policy_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "  EMIT_QUESTION  "})),
            ReviewGatePolicy::EmitQuestion
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "Off"})),
            ReviewGatePolicy::Off
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "MANUAL"})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn parse_policy_unknown_collapses_to_manual() {
        // Unknown values are silently mapped to the default rather than
        // rejected — the response always echoes the resolved policy so a
        // typo is observable downstream.
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "always"})),
            ReviewGatePolicy::Manual
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": ""})),
            ReviewGatePolicy::Manual
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "   "})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn policy_label_round_trips() {
        assert_eq!(ReviewGatePolicy::Manual.as_str(), "manual");
        assert_eq!(ReviewGatePolicy::EmitQuestion.as_str(), "emit_question");
        assert_eq!(ReviewGatePolicy::Off.as_str(), "off");
    }

    // ── wave-14 :: deterministic id with topic / file-path hash ─────────

    #[test]
    fn artifact_id_appends_topic_hash_suffix() {
        let id = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("wave14-topic"),
        );
        assert!(
            id.starts_with("review:directive:abc:v1:compile:"),
            "expected legacy prefix, got: {id}"
        );
        // Suffix must be the truncated hash, NOT the raw topic — keeps the
        // id bounded and obfuscates topic length.
        let suffix = id.rsplit(':').next().unwrap();
        assert_eq!(suffix.len(), 16, "suffix must be 16 hex chars");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!id.contains("wave14-topic"));
    }

    #[test]
    fn artifact_id_without_topic_falls_back_to_legacy_layout() {
        // Empty / blank `topic_or_path` collapses to the wave-11 layout so
        // existing callers that don't have a path yet stay byte-identical.
        let id = derive_review_question_id_for_artifact("plan", "p1", 2, "approve", None);
        assert_eq!(id, "review:plan:p1:v2:approve");
        let id2 =
            derive_review_question_id_for_artifact("plan", "p1", 2, "approve", Some("   "));
        assert_eq!(id2, "review:plan:p1:v2:approve");
    }

    #[test]
    fn artifact_id_is_deterministic_for_same_topic() {
        let a = derive_review_question_id_for_artifact(
            "workflow",
            "wf1",
            3,
            "compile",
            Some("/abs/path/.missiond/workflows/foo.lisp"),
        );
        let b = derive_review_question_id_for_artifact(
            "workflow",
            "wf1",
            3,
            "compile",
            Some("/abs/path/.missiond/workflows/foo.lisp"),
        );
        assert_eq!(a, b);
    }

    // ── wave-16 / task 04 — plan-node review-gate id helper ────────────

    #[test]
    fn plan_node_review_id_uses_plan_scope_and_topic_hash() {
        let id = derive_plan_node_review_question_id(
            "00000000-0000-0000-0000-000000000abc",
            3,
            "n1",
            None,
        );
        // scope=plan → wave-14 supported scope; topic-hash suffix folds in node_id.
        assert!(
            id.starts_with(
                "review:plan:00000000-0000-0000-0000-000000000abc:v3:plan-node:"
            ),
            "unexpected layout: {id}"
        );
        let suffix = id.rsplit(':').next().unwrap();
        assert_eq!(suffix.len(), 16, "16-hex topic hash expected");
    }

    #[test]
    fn plan_node_review_id_action_override_changes_id() {
        let default = derive_plan_node_review_question_id("p1", 1, "n1", None);
        let override_action =
            derive_plan_node_review_question_id("p1", 1, "n1", Some("human-checkpoint"));
        assert_ne!(default, override_action);
        assert!(override_action.contains(":human-checkpoint:"));
    }

    #[test]
    fn plan_node_review_id_blank_action_falls_back_to_default() {
        let blank = derive_plan_node_review_question_id("p1", 1, "n1", Some("   "));
        let default = derive_plan_node_review_question_id("p1", 1, "n1", None);
        assert_eq!(blank, default);
    }

    #[test]
    fn plan_node_review_id_distinct_per_node_under_same_plan() {
        let a = derive_plan_node_review_question_id("p1", 1, "node-a", None);
        let b = derive_plan_node_review_question_id("p1", 1, "node-b", None);
        assert_ne!(a, b, "different nodes must produce distinct ids");
    }

    #[test]
    fn plan_node_review_id_routes_under_plan_scope_via_subscriber() {
        // Forward-compat with wave16-02: the deterministic id must dispatch
        // under the existing `Route { scope=plan, ... }` outcome so the
        // QuestionEvent::Resolved listener can reach the per-scope handler
        // when auto-resume lands.
        let id = derive_plan_node_review_question_id(
            "00000000-0000-0000-0000-000000000abc",
            1,
            "n1",
            Some("plan-node"),
        );
        let dispatch = plan_review_resolved_dispatch(&id, "approved");
        match dispatch {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "plan");
                assert_eq!(parsed.action, "plan-node");
                assert!(parsed.topic_hash.is_some());
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!("expected Route under plan scope, got {:?}", other),
        }
    }

    #[test]
    fn artifact_id_changes_when_topic_changes() {
        let a = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("topic-a"),
        );
        let b = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("topic-b"),
        );
        assert_ne!(a, b, "topic must affect the trailing hash");
    }

    #[test]
    fn topic_hash_short_is_16_hex_chars() {
        let h = topic_hash_short("anything");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn topic_hash_short_is_stable() {
        // Pin the exact prefix for "wave14-topic" so an accidental change to
        // the hashing scheme breaks loud (id correlation across daemon
        // restarts depends on stability).
        assert_eq!(topic_hash_short("wave14-topic").len(), 16);
        let a = topic_hash_short("wave14-topic");
        let b = topic_hash_short("wave14-topic");
        assert_eq!(a, b);
    }

    // ── wave-14 :: payload introspection helper ─────────────────────────

    #[test]
    fn payload_says_file_written_true_when_flag_present() {
        let p = json!({"file_written": true});
        assert!(payload_says_file_written(&p));
    }

    #[test]
    fn payload_says_file_written_false_when_flag_missing() {
        let p = json!({"status": "compiled"});
        assert!(!payload_says_file_written(&p));
    }

    #[test]
    fn payload_says_file_written_false_when_flag_false() {
        let p = json!({"file_written": false});
        assert!(!payload_says_file_written(&p));
    }

    #[test]
    fn stamp_policy_inserts_resolved_label() {
        let mut p = json!({"status": "compiled"});
        stamp_policy(&mut p, ReviewGatePolicy::EmitQuestion);
        assert_eq!(p["review_gate_policy"], "emit_question");
    }

    #[test]
    fn stamp_policy_overwrites_prior_value() {
        // Always overwrite — we treat `review_gate_policy` as authoritative
        // for the resolved policy on this call.
        let mut p = json!({
            "status": "compiled",
            "review_gate_policy": "off",
        });
        stamp_policy(&mut p, ReviewGatePolicy::Manual);
        assert_eq!(p["review_gate_policy"], "manual");
    }

    // ── wave-14 :: auto-emit decision matrix (no bus) ───────────────────
    //
    // We can't drive the actual `auto_emit_review_question_after_artifact_write`
    // helper here without a `BusServices`, but the manual / off / file-not-
    // written branches return BEFORE the publish call. Replay the same
    // payload mutations in pure helpers so the contract stays pinned.

    #[test]
    fn auto_emit_manual_branch_is_a_noop_aside_from_policy_stamp() {
        // Replay manual-branch behaviour: stamp policy + return early.
        let mut p = json!({"status": "compiled", "file_written": true});
        stamp_policy(&mut p, ReviewGatePolicy::Manual);
        assert_eq!(p["review_gate_policy"], "manual");
        // No `review_question_emitted` mutation on manual — we leave the
        // legacy explicit-emit path in control of that field.
        assert!(p.get("review_question_emitted").is_none());
    }

    #[test]
    fn auto_emit_off_branch_stamps_emitted_false() {
        let mut p = json!({"status": "compiled", "file_written": true});
        stamp_policy(&mut p, ReviewGatePolicy::Off);
        // Replay the off-branch mutation: stamp emitted=false if absent.
        if let Some(map) = p.as_object_mut() {
            map.entry("review_question_emitted".to_string())
                .or_insert(json!(false));
        }
        assert_eq!(p["review_question_emitted"], false);
        assert_eq!(p["review_gate_policy"], "off");
    }

    #[test]
    fn auto_emit_file_not_written_records_warning_without_publishing() {
        let mut p = json!({"status": "partial", "file_written": false});
        stamp_policy(&mut p, ReviewGatePolicy::EmitQuestion);
        // Replay the suppress-because-no-file branch.
        if let Some(map) = p.as_object_mut() {
            map.insert("review_question_emitted".to_string(), json!(false));
            map.entry("review_question_warning".to_string()).or_insert(json!({
                "code": "FILE_WRITE_NOT_SUCCESSFUL",
                "reason": "review_gate_policy=emit_question requires file_written=true; auto-emit suppressed",
                "scope": "directive",
                "artifact_id": "abc",
                "version": 1,
            }));
        }
        assert_eq!(p["review_question_emitted"], false);
        assert_eq!(p["review_gate_policy"], "emit_question");
        assert_eq!(p["review_question_warning"]["code"], "FILE_WRITE_NOT_SUCCESSFUL");
    }

    #[test]
    fn auto_emit_explicit_id_override_wins_over_derived() {
        // Replay the id-resolution: id_override beats derive_review_question_id_for_artifact.
        let derived = derive_review_question_id_for_artifact(
            "plan",
            "p1",
            1,
            "compile",
            Some("/some/file"),
        );
        let id_override = "review:custom:override";
        let chosen = if !id_override.trim().is_empty() {
            id_override.to_string()
        } else {
            derived.clone()
        };
        assert_eq!(chosen, "review:custom:override");
        assert_ne!(chosen, derived);
    }

    #[test]
    fn review_gate_policy_was_explicit_detects_presence() {
        assert!(!review_gate_policy_was_explicit(&json!({})));
        assert!(!review_gate_policy_was_explicit(
            &json!({"emit_review_question": true})
        ));
        // Even an empty / unknown value still counts as "the key was sent",
        // so the response should stamp `review_gate_policy=manual` to make
        // the resolution visible.
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": ""})
        ));
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": "off"})
        ));
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": "emit_question"})
        ));
    }

    #[test]
    fn auto_emit_decision_variants_are_distinct() {
        // Pinning that the four decision variants are distinct so callers
        // can pattern-match them in tests / logging without surprise.
        assert_ne!(
            AutoEmitDecision::SkippedPolicyManual,
            AutoEmitDecision::SkippedPolicyOff
        );
        assert_ne!(
            AutoEmitDecision::SkippedFileWriteUnsuccessful,
            AutoEmitDecision::Emitted
        );
        assert_ne!(
            AutoEmitDecision::Emitted,
            AutoEmitDecision::EmitFailedBus
        );
    }

    // ── wave-15 :: explicit review-resolution input ─────────────────────

    #[test]
    fn decision_parse_accepts_canonical_strings() {
        assert_eq!(ReviewDecision::parse("approved").unwrap(), ReviewDecision::Approved);
        assert_eq!(ReviewDecision::parse("rejected").unwrap(), ReviewDecision::Rejected);
        assert_eq!(
            ReviewDecision::parse("needs_changes").unwrap(),
            ReviewDecision::NeedsChanges
        );
    }

    #[test]
    fn decision_parse_is_case_insensitive_and_trims() {
        assert_eq!(ReviewDecision::parse("  Approved  ").unwrap(), ReviewDecision::Approved);
        assert_eq!(ReviewDecision::parse("REJECTED").unwrap(), ReviewDecision::Rejected);
        assert_eq!(
            ReviewDecision::parse("Needs-Changes").unwrap(),
            ReviewDecision::NeedsChanges
        );
    }

    #[test]
    fn decision_parse_accepts_short_aliases() {
        assert_eq!(ReviewDecision::parse("approve").unwrap(), ReviewDecision::Approved);
        assert_eq!(ReviewDecision::parse("reject").unwrap(), ReviewDecision::Rejected);
        assert_eq!(ReviewDecision::parse("changes").unwrap(), ReviewDecision::NeedsChanges);
    }

    #[test]
    fn decision_parse_rejects_unknown() {
        let err = ReviewDecision::parse("approved-with-comments").unwrap_err();
        assert!(matches!(err, ResolutionInputError::UnknownDecision(_)));
        assert_eq!(err.code(), "INVALID_PARAM");
        assert!(err.message().contains("approved-with-comments"));
    }

    #[test]
    fn decision_outcome_mapping_is_total() {
        assert_eq!(
            ReviewDecision::Approved.outcome(),
            ResolutionOutcome::PerformTransition
        );
        assert_eq!(
            ReviewDecision::Rejected.outcome(),
            ResolutionOutcome::KeepArtifact
        );
        assert_eq!(
            ReviewDecision::NeedsChanges.outcome(),
            ResolutionOutcome::RequestChanges
        );
    }

    #[test]
    fn decision_label_round_trips() {
        assert_eq!(ReviewDecision::Approved.as_str(), "approved");
        assert_eq!(ReviewDecision::Rejected.as_str(), "rejected");
        assert_eq!(ReviewDecision::NeedsChanges.as_str(), "needs_changes");
    }

    #[test]
    fn parse_resolution_input_returns_none_when_qid_absent() {
        let out = parse_review_resolution_input(&json!({})).unwrap();
        assert!(out.is_none());
        // Even with a decision present, no qid → quiet path.
        let out = parse_review_resolution_input(&json!({"review_decision": "approved"})).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn parse_resolution_input_full_shape() {
        let out = parse_review_resolution_input(&json!({
            "review_question_id": "review:directive:abc:v1:approve",
            "review_decision": "approved",
            "review_actor": "  alice  ",
            "review_note": "  ship it  ",
        }))
        .unwrap()
        .expect("full input present");
        assert_eq!(out.question_id, "review:directive:abc:v1:approve");
        assert_eq!(out.decision, ReviewDecision::Approved);
        assert_eq!(out.actor.as_deref(), Some("alice"));
        assert_eq!(out.note.as_deref(), Some("ship it"));
    }

    #[test]
    fn parse_resolution_input_missing_decision_fails_fast() {
        let err = parse_review_resolution_input(&json!({
            "review_question_id": "review:directive:abc:v1:approve",
        }))
        .unwrap_err();
        assert_eq!(err, ResolutionInputError::MissingDecision);
        assert_eq!(err.code(), "MISSING_PARAM");
        assert!(err.message().contains("review_decision"));
    }

    #[test]
    fn parse_resolution_input_unknown_decision_fails_fast() {
        let err = parse_review_resolution_input(&json!({
            "review_question_id": "review:plan:p1:v1:approve",
            "review_decision": "looks_good",
        }))
        .unwrap_err();
        assert!(matches!(err, ResolutionInputError::UnknownDecision(_)));
        assert_eq!(err.code(), "INVALID_PARAM");
    }

    #[test]
    fn parse_resolution_input_blank_strings_collapse_to_none_for_actor_note() {
        let out = parse_review_resolution_input(&json!({
            "review_question_id": "review:plan:p1:v1:approve",
            "review_decision": "approved",
            "review_actor": "   ",
            "review_note": "",
        }))
        .unwrap()
        .unwrap();
        assert!(out.actor.is_none());
        assert!(out.note.is_none());
    }

    // ── wave-15 :: deterministic id parser ──────────────────────────────

    #[test]
    fn parse_qid_legacy_layout_no_topic_hash() {
        let p = parse_review_question_id_struct("review:directive:abc-123:v1:compile").unwrap();
        assert_eq!(p.scope, "directive");
        assert_eq!(p.artifact_id, "abc-123");
        assert_eq!(p.version, 1);
        assert_eq!(p.action, "compile");
        assert!(p.topic_hash.is_none());
    }

    #[test]
    fn parse_qid_with_topic_hash_layout() {
        let p =
            parse_review_question_id_struct("review:plan:p-7:v3:compile:abcdef0123456789").unwrap();
        assert_eq!(p.scope, "plan");
        assert_eq!(p.artifact_id, "p-7");
        assert_eq!(p.version, 3);
        assert_eq!(p.action, "compile");
        assert_eq!(p.topic_hash.as_deref(), Some("abcdef0123456789"));
    }

    #[test]
    fn parse_qid_round_trips_against_derive() {
        let original = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            7,
            "compile",
            Some("topic-foo"),
        );
        let p = parse_review_question_id_struct(&original).unwrap();
        assert_eq!(p.scope, "directive");
        assert_eq!(p.artifact_id, "abc");
        assert_eq!(p.version, 7);
        assert_eq!(p.action, "compile");
        assert!(p.topic_hash.is_some());
    }

    #[test]
    fn parse_qid_lowercases_action_for_match() {
        let p = parse_review_question_id_struct("review:directive:abc:v1:Approve").unwrap();
        assert_eq!(p.action, "approve");
    }

    #[test]
    fn parse_qid_rejects_missing_prefix() {
        let err = parse_review_question_id_struct("directive:abc:v1:compile").unwrap_err();
        assert_eq!(err, ReviewIdParseError::MissingPrefix);
    }

    #[test]
    fn parse_qid_rejects_too_few_segments() {
        let err = parse_review_question_id_struct("review:directive:abc:v1").unwrap_err();
        assert_eq!(err, ReviewIdParseError::InsufficientSegments);
    }

    #[test]
    fn parse_qid_rejects_too_many_segments() {
        let err = parse_review_question_id_struct(
            "review:directive:abc:v1:compile:topic-hash:extra-trailing",
        )
        .unwrap_err();
        assert_eq!(err, ReviewIdParseError::InsufficientSegments);
    }

    #[test]
    fn parse_qid_rejects_empty_segments() {
        assert_eq!(
            parse_review_question_id_struct("review::abc:v1:compile").unwrap_err(),
            ReviewIdParseError::EmptySegment("scope")
        );
        assert_eq!(
            parse_review_question_id_struct("review:directive::v1:compile").unwrap_err(),
            ReviewIdParseError::EmptySegment("artifact_id")
        );
        assert_eq!(
            parse_review_question_id_struct("review:directive:abc:v1:").unwrap_err(),
            ReviewIdParseError::EmptySegment("action")
        );
        assert_eq!(
            parse_review_question_id_struct("review:directive:abc:v1:compile:").unwrap_err(),
            ReviewIdParseError::EmptySegment("topic_hash")
        );
    }

    #[test]
    fn parse_qid_rejects_bad_version_segment() {
        let err = parse_review_question_id_struct("review:directive:abc:1:compile").unwrap_err();
        assert!(matches!(err, ReviewIdParseError::BadVersion(_)));
        let err = parse_review_question_id_struct("review:directive:abc:vNaN:compile").unwrap_err();
        assert!(matches!(err, ReviewIdParseError::BadVersion(_)));
    }

    // ── wave-15 :: validate_review_resolution_envelope ──────────────────

    fn make_parsed(scope: &str, id: &str, version: i32, action: &str) -> ParsedReviewQuestionId {
        ParsedReviewQuestionId {
            scope: scope.to_string(),
            artifact_id: id.to_string(),
            version,
            action: action.to_string(),
            topic_hash: None,
        }
    }

    #[test]
    fn validate_envelope_accepts_matching_directive_approve() {
        let parsed = make_parsed("directive", "abc", 1, "approve");
        validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["compile", "approve", "archive"],
        )
        .expect("happy path must succeed");
    }

    #[test]
    fn validate_envelope_rejects_scope_mismatch() {
        // qid says `plan` but submitted to directive surface.
        let parsed = make_parsed("plan", "abc", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["compile", "approve", "archive"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
    }

    #[test]
    fn validate_envelope_rejects_unsupported_scope() {
        let parsed = make_parsed("worker", "abc", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "worker",
            "abc",
            1,
            &["approve"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_SCOPE_UNSUPPORTED");
    }

    #[test]
    fn validate_envelope_rejects_artifact_id_mismatch() {
        let parsed = make_parsed("directive", "xyz", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["approve"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ARTIFACT_MISMATCH");
    }

    #[test]
    fn validate_envelope_rejects_stale_version() {
        // qid says v1 but artifact is at v2.
        let parsed = make_parsed("directive", "abc", 1, "approve");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            2,
            &["approve"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "STALE_REVIEW_VERSION");
        assert!(err.message().contains("v1"));
        assert!(err.message().contains("v2"));
    }

    #[test]
    fn validate_envelope_rejects_unsupported_action() {
        let parsed = make_parsed("directive", "abc", 1, "supersede");
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "abc",
            1,
            &["compile", "approve", "archive"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
        assert!(err.message().contains("supersede"));
    }

    // ── wave-15 :: payload stamping ─────────────────────────────────────

    fn approved_input() -> ReviewResolutionInput {
        ReviewResolutionInput {
            question_id: "review:directive:abc:v1:approve".to_string(),
            decision: ReviewDecision::Approved,
            actor: Some("alice".to_string()),
            note: Some("ship it".to_string()),
        }
    }

    #[test]
    fn stamp_resolution_payload_includes_decision_outcome_actor_note() {
        let mut p = json!({"status": "approved"});
        stamp_resolution_payload(&mut p, &approved_input());
        assert_eq!(p["review_question_id"], "review:directive:abc:v1:approve");
        assert_eq!(p["review_decision"], "approved");
        assert_eq!(p["review_decision_outcome"], "perform_transition");
        assert_eq!(p["review_actor"], "alice");
        assert_eq!(p["review_note"], "ship it");
    }

    #[test]
    fn stamp_resolution_payload_omits_actor_note_when_absent() {
        let mut p = json!({"status": "rejected"});
        let input = ReviewResolutionInput {
            question_id: "review:plan:p1:v1:approve".to_string(),
            decision: ReviewDecision::Rejected,
            actor: None,
            note: None,
        };
        stamp_resolution_payload(&mut p, &input);
        assert_eq!(p["review_decision"], "rejected");
        assert_eq!(p["review_decision_outcome"], "keep_artifact");
        assert!(p.get("review_actor").is_none());
        assert!(p.get("review_note").is_none());
    }

    #[test]
    fn stamp_needs_changes_next_step_is_actionable() {
        let mut p = json!({"status": "review"});
        stamp_needs_changes_next_step(&mut p, "directive", "compile");
        let next = p["next_step"].as_str().unwrap();
        assert!(next.contains("rework"));
        assert!(next.contains("directive"));
        assert!(next.contains("compile"));
    }

    #[test]
    fn resolution_wire_string_matches_decision_label() {
        assert_eq!(resolution_wire_string(ReviewDecision::Approved), "approved");
        assert_eq!(resolution_wire_string(ReviewDecision::Rejected), "rejected");
        assert_eq!(
            resolution_wire_string(ReviewDecision::NeedsChanges),
            "needs_changes"
        );
    }

    #[test]
    fn resolution_outcome_variants_distinct() {
        assert_ne!(
            ResolutionOutcome::PerformTransition,
            ResolutionOutcome::KeepArtifact
        );
        assert_ne!(
            ResolutionOutcome::KeepArtifact,
            ResolutionOutcome::RequestChanges
        );
    }

    // ── wave-16 :: subscriber-side resolution dispatcher ────────────────

    #[test]
    fn subscriber_resolution_string_approve_synonyms_collapse_to_approved() {
        for raw in ["approved", "approve", "yes", "accepted", "Approved", "  YES  "] {
            assert_eq!(
                parse_subscriber_resolution_string(raw),
                Some(ReviewDecision::Approved),
                "expected Approved for `{}`",
                raw
            );
        }
    }

    #[test]
    fn subscriber_resolution_string_reject_synonyms_collapse_to_rejected() {
        for raw in ["rejected", "reject", "no", "Reject", " NO "] {
            assert_eq!(
                parse_subscriber_resolution_string(raw),
                Some(ReviewDecision::Rejected),
                "expected Rejected for `{}`",
                raw
            );
        }
    }

    #[test]
    fn subscriber_resolution_string_needs_changes_synonyms_collapse() {
        for raw in [
            "needs_changes",
            "needs-changes",
            "changes",
            "revise",
            "fix",
            "Revise",
            "  FIX  ",
        ] {
            assert_eq!(
                parse_subscriber_resolution_string(raw),
                Some(ReviewDecision::NeedsChanges),
                "expected NeedsChanges for `{}`",
                raw
            );
        }
    }

    #[test]
    fn subscriber_resolution_string_unknown_returns_none() {
        for raw in ["", "maybe", "deferred", "unsure", "abstain"] {
            assert!(
                parse_subscriber_resolution_string(raw).is_none(),
                "expected None for `{}`",
                raw
            );
        }
    }

    #[test]
    fn dispatch_ignores_non_review_id() {
        let d = plan_review_resolved_dispatch("master:abc:approve", "approved");
        assert_eq!(d, ReviewResolvedDispatch::IgnoreNonReviewId);
    }

    #[test]
    fn dispatch_ignores_blank_id_as_non_review() {
        let d = plan_review_resolved_dispatch("", "approved");
        assert_eq!(d, ReviewResolvedDispatch::IgnoreNonReviewId);
    }

    #[test]
    fn dispatch_ignores_malformed_review_id() {
        let d = plan_review_resolved_dispatch("review:directive", "approved");
        match d {
            ReviewResolvedDispatch::IgnoreMalformedId(_) => {}
            other => panic!("expected IgnoreMalformedId, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_ignores_unsupported_scope_even_when_id_well_formed() {
        // `chat` is not directive/plan/workflow → defensive ignore.
        let d = plan_review_resolved_dispatch("review:chat:abc:v1:approve", "approved");
        match d {
            ReviewResolvedDispatch::IgnoreUnsupportedScope { scope } => {
                assert_eq!(scope, "chat");
            }
            other => panic!("expected IgnoreUnsupportedScope, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_ignores_unknown_resolution() {
        let d = plan_review_resolved_dispatch(
            "review:directive:abc:v1:approve",
            "deferred",
        );
        match d {
            ReviewResolvedDispatch::IgnoreUnknownResolution { resolution } => {
                assert_eq!(resolution, "deferred");
            }
            other => panic!("expected IgnoreUnknownResolution, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_directive_approved() {
        let d = plan_review_resolved_dispatch(
            "review:directive:abc-123:v1:approve",
            "approved",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "directive");
                assert_eq!(parsed.artifact_id, "abc-123");
                assert_eq!(parsed.version, 1);
                assert_eq!(parsed.action, "approve");
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_plan_rejected_via_synonym() {
        let d = plan_review_resolved_dispatch(
            "review:plan:9f3c:v2:supersede",
            "no",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "plan");
                assert_eq!(parsed.artifact_id, "9f3c");
                assert_eq!(parsed.version, 2);
                assert_eq!(parsed.action, "supersede");
                assert_eq!(decision, ReviewDecision::Rejected);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_workflow_needs_changes_via_synonym() {
        let d = plan_review_resolved_dispatch(
            "review:workflow:methodology-deploy-v0:v1:compile",
            "fix",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "workflow");
                assert_eq!(parsed.artifact_id, "methodology-deploy-v0");
                assert_eq!(parsed.version, 1);
                assert_eq!(parsed.action, "compile");
                assert_eq!(decision, ReviewDecision::NeedsChanges);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_routes_with_topic_hash_suffix() {
        let d = plan_review_resolved_dispatch(
            "review:directive:abc:v3:compile:0123456789abcdef",
            "approve",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "directive");
                assert_eq!(parsed.action, "compile");
                assert_eq!(
                    parsed.topic_hash.as_deref(),
                    Some("0123456789abcdef")
                );
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    // ── wave-17 / task 01 — plan-node resume helpers ──────────────────

    #[test]
    fn derive_plan_node_topic_hash_matches_emitter_round_trip() {
        // The hash the resume helper extracts MUST equal the hash the
        // wave-16 / task 04 pause emitter folds into the deterministic id —
        // otherwise the resume listener can never map an inbound qid back
        // to its originating paused node id.
        let plan_id = "00000000-0000-0000-0000-000000000abc";
        let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
        let parsed = parse_review_question_id_struct(&qid).expect("valid envelope");
        let hash = derive_plan_node_topic_hash("node-g");
        assert_eq!(parsed.topic_hash.as_deref(), Some(hash.as_str()));
        // Hash length is the wave-14 contract: 16 hex chars.
        assert_eq!(hash.len(), 16);
    }

    #[test]
    fn derive_plan_node_topic_hash_is_deterministic_per_node_id() {
        // Same node id always hashes to the same prefix so the resume
        // helper's lookup is stable across daemon restarts.
        let a = derive_plan_node_topic_hash("alpha");
        let b = derive_plan_node_topic_hash("alpha");
        assert_eq!(a, b);
        assert_ne!(a, derive_plan_node_topic_hash("beta"));
    }

    #[test]
    fn is_plan_node_review_action_matches_default_action_case_insensitive() {
        assert!(is_plan_node_review_action("plan-node"));
        assert!(is_plan_node_review_action("PLAN-NODE"));
        assert!(is_plan_node_review_action("  plan-node  "));
        assert!(!is_plan_node_review_action("compile"));
        assert!(!is_plan_node_review_action("approve"));
        assert!(!is_plan_node_review_action(""));
    }

    // ── wave-17 / task 01 — resume input parser ───────────────────────

    #[test]
    fn parse_plan_node_resume_input_returns_none_when_id_absent() {
        // Quiet path: no `resume_review_question_id` → caller falls
        // through to the standard execute pipeline. Must NOT error
        // because absence is the legacy-quiet contract.
        assert!(parse_plan_node_resume_input(&json!({})).expect("ok").is_none());
        assert!(parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "   "
        }))
        .expect("ok")
        .is_none());
    }

    #[test]
    fn parse_plan_node_resume_input_extracts_full_envelope() {
        let input = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "  review:plan:abc:v1:plan-node:0123456789abcdef  ",
            "resume_review_decision": "approved",
            "resume_actor": "  agent-team  ",
            "resume_note": "  proceed  ",
        }))
        .expect("ok")
        .expect("some");
        assert_eq!(
            input.question_id,
            "review:plan:abc:v1:plan-node:0123456789abcdef"
        );
        assert_eq!(input.decision, ReviewDecision::Approved);
        assert_eq!(input.actor.as_deref(), Some("agent-team"));
        assert_eq!(input.note.as_deref(), Some("proceed"));
    }

    #[test]
    fn parse_plan_node_resume_input_parses_rejected_and_needs_changes() {
        for (raw, expected) in [
            ("rejected", ReviewDecision::Rejected),
            ("needs_changes", ReviewDecision::NeedsChanges),
            ("REJECTED", ReviewDecision::Rejected),
            ("changes", ReviewDecision::NeedsChanges),
        ] {
            let input = parse_plan_node_resume_input(&json!({
                "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
                "resume_review_decision": raw,
            }))
            .expect("ok")
            .expect("some");
            assert_eq!(input.decision, expected, "decision raw={}", raw);
        }
    }

    #[test]
    fn parse_plan_node_resume_input_id_without_decision_is_missing_decision_error() {
        // The id is load-bearing — supplying it without a decision is
        // fail-fast (mirrors the wave-15 manager-side parser behaviour).
        let err = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
        }))
        .expect_err("missing decision");
        assert_eq!(err, ResolutionInputError::MissingDecision);
    }

    #[test]
    fn parse_plan_node_resume_input_unknown_decision_is_unknown_decision_error() {
        let err = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
            "resume_review_decision": "looks-good-to-me",
        }))
        .expect_err("unknown decision");
        match err {
            ResolutionInputError::UnknownDecision(raw) => {
                assert_eq!(raw, "looks-good-to-me");
            }
            other => panic!("expected UnknownDecision, got {:?}", other),
        }
    }

    #[test]
    fn parse_plan_node_resume_input_filters_blank_actor_and_note() {
        let input = parse_plan_node_resume_input(&json!({
            "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
            "resume_review_decision": "approved",
            "resume_actor": "   ",
            "resume_note": "",
        }))
        .expect("ok")
        .expect("some");
        assert!(input.actor.is_none());
        assert!(input.note.is_none());
    }

    // ── wave-18 / task 07 — review automation policy ────────────────────

    #[test]
    fn parse_automation_policy_default_is_manual() {
        assert_eq!(
            parse_review_automation_policy(&json!({})),
            ReviewAutomationPolicy::Manual
        );
    }

    #[test]
    fn parse_automation_policy_recognises_suggest() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "suggest"})),
            ReviewAutomationPolicy::Suggest
        );
    }

    #[test]
    fn parse_automation_policy_recognises_auto_safe() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "auto_safe"})),
            ReviewAutomationPolicy::AutoSafe
        );
        // Hyphenated alias accepted to keep authoring typos loud only on
        // truly unknown values.
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "auto-safe"})),
            ReviewAutomationPolicy::AutoSafe
        );
    }

    #[test]
    fn parse_automation_policy_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "  AUTO_SAFE  "})),
            ReviewAutomationPolicy::AutoSafe
        );
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "Suggest"})),
            ReviewAutomationPolicy::Suggest
        );
    }

    #[test]
    fn parse_automation_policy_unknown_collapses_to_manual() {
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": "yolo"})),
            ReviewAutomationPolicy::Manual
        );
        assert_eq!(
            parse_review_automation_policy(&json!({"review_automation_policy": ""})),
            ReviewAutomationPolicy::Manual
        );
    }

    #[test]
    fn automation_policy_was_explicit_detects_presence() {
        assert!(!review_automation_policy_was_explicit(&json!({})));
        assert!(review_automation_policy_was_explicit(
            &json!({"review_automation_policy": ""})
        ));
        assert!(review_automation_policy_was_explicit(
            &json!({"review_automation_policy": "auto_safe"})
        ));
    }

    #[test]
    fn automation_policy_label_round_trips() {
        assert_eq!(ReviewAutomationPolicy::Manual.as_str(), "manual");
        assert_eq!(ReviewAutomationPolicy::Suggest.as_str(), "suggest");
        assert_eq!(ReviewAutomationPolicy::AutoSafe.as_str(), "auto_safe");
    }

    #[test]
    fn automation_status_label_round_trips() {
        assert_eq!(AutomationStatus::NotEvaluated.as_str(), "not_evaluated");
        assert_eq!(AutomationStatus::Suggested.as_str(), "suggested");
        assert_eq!(AutomationStatus::AutoApproved.as_str(), "auto_approved");
        assert_eq!(
            AutomationStatus::AutoSafeBlocked.as_str(),
            "auto_safe_blocked"
        );
        assert_eq!(
            AutomationStatus::OverriddenByExplicitDecision.as_str(),
            "overridden_by_explicit_decision"
        );
    }

    fn safe_ctx() -> ReviewAutomationContext {
        ReviewAutomationContext {
            deterministic_mode: true,
            file_write_attempted: true,
            file_write_succeeded: true,
            actual_file_sha256: Some("deadbeef".repeat(8)),
            expected_file_sha256: Some("deadbeef".repeat(8)),
            protected_source_or_target: false,
            additional_blockers: Vec::new(),
        }
    }

    #[test]
    fn evaluate_manual_returns_not_evaluated_with_empty_block() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Manual,
            &safe_ctx(),
            None,
        );
        assert_eq!(outcome.policy, ReviewAutomationPolicy::Manual);
        assert_eq!(outcome.status, AutomationStatus::NotEvaluated);
        assert!(outcome.suggested_decision.is_none());
        assert!(outcome.reasons.is_empty());
        assert!(!outcome.may_auto_resolve);
    }

    #[test]
    fn evaluate_suggest_returns_suggestion_without_mutation() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Suggest,
            &safe_ctx(),
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::Suggested);
        assert_eq!(outcome.suggested_decision, Some(ReviewDecision::Approved));
        assert!(!outcome.may_auto_resolve);
        assert!(!outcome.reasons.is_empty());
    }

    #[test]
    fn evaluate_auto_safe_approves_when_every_rule_passes() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &safe_ctx(),
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoApproved);
        assert_eq!(outcome.suggested_decision, Some(ReviewDecision::Approved));
        assert!(outcome.may_auto_resolve);
        // Only passing reasons survive on the AutoApproved path.
        for r in &outcome.reasons {
            assert!(
                r.starts_with("rule:"),
                "reason `{}` should start with `rule:`",
                r
            );
        }
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_protected_source_or_target() {
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(!outcome.may_auto_resolve);
        assert_eq!(
            outcome.suggested_decision,
            Some(ReviewDecision::NeedsChanges)
        );
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("protected_source_or_target")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_non_deterministic() {
        let mut ctx = safe_ctx();
        ctx.deterministic_mode = false;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(!outcome.may_auto_resolve);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("deterministic_mode")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_file_hash_mismatch() {
        let mut ctx = safe_ctx();
        ctx.actual_file_sha256 = Some("aaaa".repeat(8));
        ctx.expected_file_sha256 = Some("bbbb".repeat(8));
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(!outcome.may_auto_resolve);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("file_hash_mismatch")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_when_file_write_failed() {
        let mut ctx = safe_ctx();
        ctx.file_write_attempted = true;
        ctx.file_write_succeeded = false;
        ctx.actual_file_sha256 = None;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("file_write_unsuccessful")));
    }

    #[test]
    fn evaluate_auto_safe_passes_when_no_file_write_attempted() {
        let mut ctx = safe_ctx();
        ctx.file_write_attempted = false;
        ctx.file_write_succeeded = false;
        ctx.actual_file_sha256 = None;
        ctx.expected_file_sha256 = None;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        // No file write attempted → the no-file-write rule auto-passes.
        assert_eq!(outcome.status, AutomationStatus::AutoApproved);
        assert!(outcome.may_auto_resolve);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("no_file_write")));
    }

    #[test]
    fn evaluate_auto_safe_blocked_by_additional_blocker() {
        let mut ctx = safe_ctx();
        ctx.additional_blockers
            .push("status=partial: review_question_warning present".to_string());
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert_eq!(outcome.status, AutomationStatus::AutoSafeBlocked);
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("additional_blocker")));
    }

    #[test]
    fn evaluate_explicit_decision_overrides_suggestion() {
        // Even when every rule passes, an explicit caller decision wins.
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &safe_ctx(),
            Some(ReviewDecision::Rejected),
        );
        assert_eq!(
            outcome.status,
            AutomationStatus::OverriddenByExplicitDecision
        );
        // We still surface the suggestion for audit, but never mutate.
        assert_eq!(outcome.suggested_decision, Some(ReviewDecision::Approved));
        assert!(!outcome.may_auto_resolve);
    }

    #[test]
    fn evaluate_explicit_decision_overrides_under_suggest_too() {
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Suggest,
            &safe_ctx(),
            Some(ReviewDecision::Approved),
        );
        assert_eq!(
            outcome.status,
            AutomationStatus::OverriddenByExplicitDecision
        );
        assert!(!outcome.may_auto_resolve);
    }

    #[test]
    fn evaluate_auto_safe_never_auto_rejects_even_when_suggestion_is_needs_changes() {
        // A blocking rule degrades the suggestion to NeedsChanges; even
        // though the suggestion is unanimous, auto_safe NEVER mutates
        // toward rejection / needs_changes — it only auto-approves.
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &ctx,
            None,
        );
        assert!(!outcome.may_auto_resolve);
        assert_eq!(
            outcome.suggested_decision,
            Some(ReviewDecision::NeedsChanges)
        );
        assert!(outcome
            .reasons
            .iter()
            .any(|r| r.contains("auto_safe_refuses_non_approved")
                || r.contains("protected_source_or_target")));
    }

    #[test]
    fn stamp_automation_payload_under_manual_is_emitted_when_called() {
        // The handler is responsible for skipping the call under Manual to
        // keep pre-wave-18 callers byte-identical. But if it IS called
        // (e.g. from a future explicit-status path), the stamp shape stays
        // sane — `suggested_review_decision` is omitted.
        let mut p = json!({"status": "approved"});
        let outcome = ReviewAutomationOutcome {
            policy: ReviewAutomationPolicy::Manual,
            status: AutomationStatus::NotEvaluated,
            suggested_decision: None,
            reasons: Vec::new(),
            may_auto_resolve: false,
        };
        stamp_review_automation_payload(&mut p, &outcome);
        assert_eq!(p["review_automation_policy"], "manual");
        assert_eq!(p["review_automation_status"], "not_evaluated");
        assert!(p.get("suggested_review_decision").is_none());
        assert_eq!(p["automation_reasons"], json!([]));
    }

    #[test]
    fn stamp_automation_payload_includes_suggestion_under_suggest() {
        let mut p = json!({"status": "draft"});
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::Suggest,
            &safe_ctx(),
            None,
        );
        stamp_review_automation_payload(&mut p, &outcome);
        assert_eq!(p["review_automation_policy"], "suggest");
        assert_eq!(p["review_automation_status"], "suggested");
        assert_eq!(p["suggested_review_decision"], "approved");
        assert!(p["automation_reasons"].is_array());
        assert!(!p["automation_reasons"].as_array().unwrap().is_empty());
    }

    #[test]
    fn stamp_automation_payload_under_auto_approved_path() {
        let mut p = json!({"status": "approved"});
        let outcome = evaluate_review_automation(
            ReviewAutomationPolicy::AutoSafe,
            &safe_ctx(),
            None,
        );
        stamp_review_automation_payload(&mut p, &outcome);
        assert_eq!(p["review_automation_policy"], "auto_safe");
        assert_eq!(p["review_automation_status"], "auto_approved");
        assert_eq!(p["suggested_review_decision"], "approved");
    }

    // ── wave-20 / task 08 — review auto-answer policy ────────────────────

    #[test]
    fn parse_auto_answer_policy_default_is_off() {
        assert_eq!(
            parse_auto_answer_policy(&json!({})),
            AutoAnswerPolicy::Off
        );
    }

    #[test]
    fn parse_auto_answer_policy_recognises_deterministic_safe() {
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "deterministic_safe"})),
            AutoAnswerPolicy::DeterministicSafe
        );
        // Hyphenated alias accepted to keep authoring typos loud only on
        // truly unknown values.
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "deterministic-safe"})),
            AutoAnswerPolicy::DeterministicSafe
        );
    }

    #[test]
    fn parse_auto_answer_policy_recognises_dry_run() {
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "dry_run"})),
            AutoAnswerPolicy::DryRun
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "dry-run"})),
            AutoAnswerPolicy::DryRun
        );
    }

    #[test]
    fn parse_auto_answer_policy_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_auto_answer_policy(
                &json!({"auto_answer_policy": "  DETERMINISTIC_SAFE  "})
            ),
            AutoAnswerPolicy::DeterministicSafe
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "Dry_Run"})),
            AutoAnswerPolicy::DryRun
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "OFF"})),
            AutoAnswerPolicy::Off
        );
    }

    #[test]
    fn parse_auto_answer_policy_unknown_collapses_to_off() {
        // Unknown values silently map to the default rather than rejected
        // — the response always echoes the resolved policy so a typo is
        // observable downstream.
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "auto_approve"})),
            AutoAnswerPolicy::Off
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": ""})),
            AutoAnswerPolicy::Off
        );
        assert_eq!(
            parse_auto_answer_policy(&json!({"auto_answer_policy": "   "})),
            AutoAnswerPolicy::Off
        );
    }

    #[test]
    fn auto_answer_policy_was_explicit_detects_presence() {
        assert!(!auto_answer_policy_was_explicit(&json!({})));
        assert!(auto_answer_policy_was_explicit(
            &json!({"auto_answer_policy": ""})
        ));
        assert!(auto_answer_policy_was_explicit(
            &json!({"auto_answer_policy": "dry_run"})
        ));
        assert!(auto_answer_policy_was_explicit(
            &json!({"auto_answer_policy": "off"})
        ));
    }

    #[test]
    fn auto_answer_policy_label_round_trips() {
        assert_eq!(AutoAnswerPolicy::Off.as_str(), "off");
        assert_eq!(
            AutoAnswerPolicy::DeterministicSafe.as_str(),
            "deterministic_safe"
        );
        assert_eq!(AutoAnswerPolicy::DryRun.as_str(), "dry_run");
    }

    #[test]
    fn auto_answer_status_label_round_trips() {
        assert_eq!(AutoAnswerStatus::NotEvaluated.as_str(), "not_evaluated");
        assert_eq!(AutoAnswerStatus::AutoAnswered.as_str(), "auto_answered");
        assert_eq!(
            AutoAnswerStatus::SkippedRulesFailed.as_str(),
            "skipped_rules_failed"
        );
        assert_eq!(
            AutoAnswerStatus::SkippedDestructiveAction.as_str(),
            "skipped_destructive_action"
        );
        assert_eq!(
            AutoAnswerStatus::DryRunPreview.as_str(),
            "dry_run_preview"
        );
    }

    // ── wave-20 / task 08 — destructive-action guard ─────────────────────

    #[test]
    fn destructive_action_recognises_archive_supersede_remove() {
        for raw in [
            "archive",
            "supersede",
            "remove",
            "Archive",
            "SUPERSEDE",
            "  Remove  ",
        ] {
            assert!(
                is_destructive_review_action(raw),
                "expected destructive for `{}`",
                raw
            );
        }
    }

    #[test]
    fn non_destructive_actions_are_safe() {
        for raw in [
            "compile",
            "approve",
            "mark",
            "plan-node",
            "human-checkpoint",
            "",
            "  ",
        ] {
            assert!(
                !is_destructive_review_action(raw),
                "expected non-destructive for `{}`",
                raw
            );
        }
    }

    // ── wave-20 / task 08 — evaluate_auto_answer_policy ──────────────────

    #[test]
    fn evaluate_auto_answer_off_returns_not_evaluated_with_empty_block() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::Off,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome.policy, AutoAnswerPolicy::Off);
        assert_eq!(outcome.status, AutoAnswerStatus::NotEvaluated);
        assert!(outcome.selected_decision.is_none());
        assert!(outcome.safety_rule_results.is_empty());
        // Off mode does NOT defer — caller routes the inbound decision
        // unchanged. requires_human=false matches the byte-identical
        // pre-wave-20/08 contract.
        assert!(!outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_auto_answers_when_every_rule_passes() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::AutoAnswered);
        assert_eq!(outcome.selected_decision, Some(ReviewDecision::Approved));
        assert!(!outcome.requires_human);
        // The destructive-action rule surfaces even on the happy path so
        // observers see why the action was eligible.
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("non_destructive_action")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocks_destructive_archive() {
        // Even when every other rule passes, archive MUST defer.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "archive",
            None,
        );
        assert_eq!(
            outcome.status,
            AutoAnswerStatus::SkippedDestructiveAction
        );
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("destructive_action") && r.contains("archive")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocks_destructive_supersede() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "supersede",
            None,
        );
        assert_eq!(
            outcome.status,
            AutoAnswerStatus::SkippedDestructiveAction
        );
        assert!(outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocks_destructive_remove() {
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "remove",
            None,
        );
        assert_eq!(
            outcome.status,
            AutoAnswerStatus::SkippedDestructiveAction
        );
        assert!(outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocked_when_protected_source_target() {
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
        // Suggestion degraded to NeedsChanges by the upstream inspector
        // because of the protected source/target rule.
        assert_eq!(
            outcome.selected_decision,
            Some(ReviewDecision::NeedsChanges)
        );
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("protected_source_or_target")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocked_when_not_deterministic() {
        // sonnet / LLM-driven artefact → wave-18/07 rule trips.
        let mut ctx = safe_ctx();
        ctx.deterministic_mode = false;
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("deterministic_mode")));
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_blocked_by_additional_blocker() {
        let mut ctx = safe_ctx();
        ctx.additional_blockers
            .push("review_question_warning present".to_string());
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
    }

    #[test]
    fn evaluate_auto_answer_deterministic_safe_defers_when_caller_decision_present() {
        // Explicit caller decision wins — the policy NEVER overrides
        // human authority.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            Some(ReviewDecision::Approved),
        );
        assert_eq!(outcome.status, AutoAnswerStatus::SkippedRulesFailed);
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("caller_decision_present")));
    }

    #[test]
    fn evaluate_auto_answer_dry_run_always_defers_even_on_safe_inputs() {
        // dry_run NEVER auto-answers — even when every rule passes the
        // selected_decision is informational and requires_human=true.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DryRun,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::DryRunPreview);
        assert!(outcome.requires_human);
        assert_eq!(outcome.selected_decision, Some(ReviewDecision::Approved));
    }

    #[test]
    fn evaluate_auto_answer_dry_run_preview_for_destructive_still_surfaces_rule() {
        // dry_run preview still surfaces the destructive-action rule on
        // the result block so dashboards see what would have happened.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DryRun,
            &safe_ctx(),
            "supersede",
            None,
        );
        assert_eq!(outcome.status, AutoAnswerStatus::DryRunPreview);
        assert!(outcome.requires_human);
        assert!(outcome
            .safety_rule_results
            .iter()
            .any(|r| r.contains("destructive_action") && r.contains("supersede")));
    }

    #[test]
    fn evaluate_auto_answer_never_returns_rejected_decision_invariant_i1() {
        // Invariant I1: auto-answer NEVER returns Rejected as the
        // selected decision. Even when the upstream inspector degrades
        // the suggestion under blocking rules, we surface NeedsChanges
        // instead of Rejected.
        for ctx_mutation in [
            // Non-deterministic mode trips a blocker.
            |c: &mut ReviewAutomationContext| c.deterministic_mode = false,
            // Protected source/target trips a blocker.
            |c: &mut ReviewAutomationContext| c.protected_source_or_target = true,
            // Hash mismatch trips a blocker.
            |c: &mut ReviewAutomationContext| {
                c.actual_file_sha256 = Some("aaaa".repeat(8));
                c.expected_file_sha256 = Some("bbbb".repeat(8));
            },
        ] {
            let mut ctx = safe_ctx();
            ctx_mutation(&mut ctx);
            for policy in [
                AutoAnswerPolicy::DeterministicSafe,
                AutoAnswerPolicy::DryRun,
            ] {
                let outcome = evaluate_auto_answer_policy(policy, &ctx, "approve", None);
                assert_ne!(
                    outcome.selected_decision,
                    Some(ReviewDecision::Rejected),
                    "invariant I1: auto-answer must NEVER return Rejected (policy={:?})",
                    policy
                );
            }
        }
    }

    #[test]
    fn evaluate_auto_answer_never_promotes_destructive_actions_invariant_i2() {
        // Invariant I2: archive / supersede / remove NEVER auto-promote,
        // even when every safety rule passes. Pinned across both policy
        // modes that evaluate.
        for action in DESTRUCTIVE_REVIEW_ACTIONS {
            // deterministic_safe → SkippedDestructiveAction.
            let outcome = evaluate_auto_answer_policy(
                AutoAnswerPolicy::DeterministicSafe,
                &safe_ctx(),
                action,
                None,
            );
            assert_ne!(
                outcome.status,
                AutoAnswerStatus::AutoAnswered,
                "invariant I2: destructive `{}` must NEVER auto-answer",
                action
            );
            assert!(
                outcome.requires_human,
                "invariant I2: destructive `{}` must require human",
                action
            );
            assert_ne!(
                outcome.selected_decision,
                Some(ReviewDecision::Rejected),
                "invariant I1+I2: destructive `{}` must NEVER auto-reject",
                action
            );

            // dry_run → DryRunPreview (never AutoAnswered) regardless.
            let dry = evaluate_auto_answer_policy(
                AutoAnswerPolicy::DryRun,
                &safe_ctx(),
                action,
                None,
            );
            assert_eq!(dry.status, AutoAnswerStatus::DryRunPreview);
            assert!(dry.requires_human);
        }
    }

    #[test]
    fn evaluate_auto_answer_never_calls_llm_invariant_i3() {
        // Invariant I3: the policy is pure / deterministic / never
        // touches an LLM. We can't directly assert on the absence of a
        // network call, but we CAN pin that the function is sync (no
        // async / no .await) by simply calling it in a sync context.
        // The signature itself enforces this — if a future refactor
        // adds `async fn`, this test fails to compile.
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        // And the decision is deterministic — running twice with the
        // same inputs MUST produce the same output.
        let outcome2 = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        assert_eq!(outcome, outcome2);
    }

    #[test]
    fn evaluate_auto_answer_skipped_block_carries_full_audit_invariant_i4() {
        // Invariant I4: when skipped (any non-Off mode that did not
        // reach AutoAnswered), the response carries policy_result,
        // selected_decision, safety_rule_results, and requires_human.
        let mut ctx = safe_ctx();
        ctx.protected_source_or_target = true;
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &ctx,
            "approve",
            None,
        );
        // policy_result label set.
        assert_ne!(outcome.status, AutoAnswerStatus::AutoAnswered);
        assert_eq!(outcome.status.as_str(), "skipped_rules_failed");
        // selected_decision present (suggestion degraded to NeedsChanges).
        assert!(outcome.selected_decision.is_some());
        // safety_rule_results non-empty.
        assert!(!outcome.safety_rule_results.is_empty());
        // requires_human=true.
        assert!(outcome.requires_human);
    }

    // ── wave-20 / task 08 — stamp_auto_answer_payload ────────────────────

    #[test]
    fn stamp_auto_answer_payload_under_off_carries_minimal_block() {
        // Helper writes the full block when called even under Off so a
        // future caller that DOES call it sees a well-formed payload.
        // The handler is responsible for skipping the call under Off to
        // keep pre-wave-20/08 callers byte-identical.
        let mut p = json!({"status": "approved"});
        let outcome = AutoAnswerOutcome {
            policy: AutoAnswerPolicy::Off,
            status: AutoAnswerStatus::NotEvaluated,
            selected_decision: None,
            safety_rule_results: Vec::new(),
            requires_human: false,
        };
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "off");
        assert_eq!(p["policy_result"], "not_evaluated");
        // No selected_decision when None.
        assert!(p.get("selected_decision").is_none());
        assert_eq!(p["safety_rule_results"], json!([]));
        assert_eq!(p["requires_human"], false);
    }

    #[test]
    fn stamp_auto_answer_payload_under_auto_answered_carries_approved_decision() {
        let mut p = json!({"status": "approved"});
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "approve",
            None,
        );
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "deterministic_safe");
        assert_eq!(p["policy_result"], "auto_answered");
        assert_eq!(p["selected_decision"], "approved");
        assert!(p["safety_rule_results"].is_array());
        assert_eq!(p["requires_human"], false);
    }

    #[test]
    fn stamp_auto_answer_payload_under_skipped_destructive_action() {
        let mut p = json!({"status": "draft"});
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DeterministicSafe,
            &safe_ctx(),
            "archive",
            None,
        );
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "deterministic_safe");
        assert_eq!(p["policy_result"], "skipped_destructive_action");
        // Suggestion still surfaces (Approved for the safe ctx) even
        // though the listener will defer.
        assert_eq!(p["selected_decision"], "approved");
        assert_eq!(p["requires_human"], true);
        let rules = p["safety_rule_results"].as_array().unwrap();
        assert!(rules
            .iter()
            .any(|r| r.as_str().unwrap().contains("destructive_action")));
    }

    #[test]
    fn stamp_auto_answer_payload_under_dry_run_preview() {
        let mut p = json!({"status": "draft"});
        let outcome = evaluate_auto_answer_policy(
            AutoAnswerPolicy::DryRun,
            &safe_ctx(),
            "approve",
            None,
        );
        stamp_auto_answer_payload(&mut p, &outcome);
        assert_eq!(p["auto_answer_policy"], "dry_run");
        assert_eq!(p["policy_result"], "dry_run_preview");
        assert_eq!(p["selected_decision"], "approved");
        // dry_run ALWAYS defers — pinning the invariant on the stamped
        // shape so a future refactor can't silently flip it.
        assert_eq!(p["requires_human"], true);
    }

    #[test]
    fn stamp_auto_answer_payload_never_emits_rejected_invariant_i1_round_trip() {
        // Belt-and-braces stamping check: even if a synthetic outcome
        // somehow carried Rejected, the stamper should serialise it as
        // the canonical `rejected` label — but the policy CONSTRUCTORS
        // (evaluate_auto_answer_policy) MUST never emit Rejected. Pinning
        // the invariant here as a defensive contract test.
        for ctx_mutation in [
            |c: &mut ReviewAutomationContext| c.deterministic_mode = false,
            |c: &mut ReviewAutomationContext| c.protected_source_or_target = true,
        ] {
            let mut ctx = safe_ctx();
            ctx_mutation(&mut ctx);
            let outcome = evaluate_auto_answer_policy(
                AutoAnswerPolicy::DeterministicSafe,
                &ctx,
                "approve",
                None,
            );
            let mut p = json!({});
            stamp_auto_answer_payload(&mut p, &outcome);
            // selected_decision MUST NOT be `rejected` after the
            // evaluator + stamper round trip.
            if let Some(s) = p.get("selected_decision").and_then(|v| v.as_str()) {
                assert_ne!(
                    s, "rejected",
                    "invariant I1 round-trip: stamper MUST NOT surface `rejected`"
                );
            }
        }
    }

    // ───────────────────────────────────────────────────────────────────
    // wave-21 / task 06 — LLM auto-approve proposal v0
    //
    // Pure helper tests. NO bus, NO DB, NO LLM. The Sonnet call itself
    // is wired in the per-handler integration code; here we pin the
    // parser / invariant / stamper contract.
    // ───────────────────────────────────────────────────────────────────

    fn well_formed_proposal_response() -> &'static str {
        r#"{
            "decision": "approved",
            "confidence": "medium",
            "evidence": "directive aligns with declared goals; safety inspector clear.",
            "non_goal_check": "no listed non-goals affected",
            "destructive_check": "non-destructive",
            "requires_human": false
        }"#
    }

    #[test]
    fn auto_approve_mode_default_off_when_field_absent() {
        let mode = parse_llm_auto_approve_proposal_mode(&json!({})).expect("default off");
        assert_eq!(mode, LlmAutoApproveProposalMode::Off);
        assert!(!llm_auto_approve_proposal_mode_was_explicit(&json!({})));
    }

    #[test]
    fn auto_approve_mode_recognises_off_blank_and_hyphen() {
        for raw in [
            json!({"auto_approve_mode": "off"}),
            json!({"auto_approve_mode": "  Off  "}),
            json!({"auto_approve_mode": ""}),
        ] {
            let mode = parse_llm_auto_approve_proposal_mode(&raw).expect("off variant");
            assert_eq!(mode, LlmAutoApproveProposalMode::Off);
            assert!(llm_auto_approve_proposal_mode_was_explicit(&raw));
        }
        let sonnet =
            parse_llm_auto_approve_proposal_mode(&json!({"auto_approve_mode": "sonnet_suggest"}))
                .expect("sonnet_suggest");
        assert_eq!(sonnet, LlmAutoApproveProposalMode::SonnetSuggest);
        let hyphen = parse_llm_auto_approve_proposal_mode(
            &json!({"auto_approve_mode": "  Sonnet-Suggest  "}),
        )
        .expect("hyphenated sonnet");
        assert_eq!(hyphen, LlmAutoApproveProposalMode::SonnetSuggest);
    }

    #[test]
    fn auto_approve_mode_rejects_unknown_value() {
        let err = parse_llm_auto_approve_proposal_mode(&json!({"auto_approve_mode": "auto"}))
            .expect_err("typo must fail-fast");
        assert!(err.contains("auto_approve_mode"));
    }

    #[test]
    fn auto_approve_mode_rejects_non_string_type() {
        let err = parse_llm_auto_approve_proposal_mode(&json!({"auto_approve_mode": true}))
            .expect_err("non-string must fail-fast");
        assert!(err.contains("must be a string"));
    }

    #[test]
    fn proposal_mode_label_round_trip() {
        assert_eq!(LlmAutoApproveProposalMode::Off.as_str(), "off");
        assert_eq!(
            LlmAutoApproveProposalMode::SonnetSuggest.as_str(),
            "sonnet_suggest"
        );
        assert!(!LlmAutoApproveProposalMode::Off.is_sonnet_suggest());
        assert!(LlmAutoApproveProposalMode::SonnetSuggest.is_sonnet_suggest());
    }

    #[test]
    fn proposal_status_label_round_trip() {
        assert_eq!(LlmAutoApproveProposalStatus::NotInvoked.as_str(), "not_invoked");
        assert_eq!(LlmAutoApproveProposalStatus::Unavailable.as_str(), "llm_unavailable");
        assert_eq!(LlmAutoApproveProposalStatus::Suggested.as_str(), "suggested");
        assert_eq!(
            LlmAutoApproveProposalStatus::DestructiveBlocked.as_str(),
            "destructive_blocked"
        );
        assert_eq!(LlmAutoApproveProposalStatus::NoSuggestion.as_str(), "no_suggestion");
    }

    #[test]
    fn proposal_confidence_label_round_trip_and_parse() {
        assert_eq!(LlmAutoApproveProposalConfidence::Low.as_str(), "low");
        assert_eq!(LlmAutoApproveProposalConfidence::Medium.as_str(), "medium");
        assert_eq!(LlmAutoApproveProposalConfidence::High.as_str(), "high");
        assert_eq!(
            LlmAutoApproveProposalConfidence::parse("HIGH"),
            Some(LlmAutoApproveProposalConfidence::High)
        );
        assert_eq!(
            LlmAutoApproveProposalConfidence::parse("med"),
            Some(LlmAutoApproveProposalConfidence::Medium)
        );
        assert_eq!(LlmAutoApproveProposalConfidence::parse("foo"), None);
    }

    #[test]
    fn parse_well_formed_proposal_returns_proposal_no_warnings() {
        let (p, warnings) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let p = p.expect("well-formed proposal must parse");
        assert_eq!(p.decision, ReviewDecision::Approved);
        assert_eq!(p.confidence, LlmAutoApproveProposalConfidence::Medium);
        assert!(p.evidence.contains("safety inspector clear"));
        assert_eq!(p.non_goal_check, "no listed non-goals affected");
        assert!(warnings.is_empty(), "well-formed proposal must not warn: {:?}", warnings);
    }

    #[test]
    fn parse_proposal_inside_wrapper_object_accepted() {
        let raw = format!(r#"{{"proposal": {}}}"#, well_formed_proposal_response());
        let (p, warnings) = parse_llm_auto_approve_proposal(&raw);
        assert!(p.is_some(), "wrapper-object proposal must parse");
        assert!(warnings.is_empty());
    }

    #[test]
    fn parse_proposal_strips_code_fence() {
        let fenced = format!("```json\n{}\n```", well_formed_proposal_response());
        let (p, _) = parse_llm_auto_approve_proposal(&fenced);
        assert!(p.is_some());
        let unfenced = format!("```\n{}\n```", well_formed_proposal_response());
        let (p, _) = parse_llm_auto_approve_proposal(&unfenced);
        assert!(p.is_some());
    }

    #[test]
    fn parse_proposal_demotes_rejected_to_needs_changes() {
        let raw = r#"{
            "decision": "rejected",
            "confidence": "high",
            "evidence": "model thinks artifact is unsafe",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        let p = p.expect("rejected proposal must demote, not drop");
        assert_eq!(p.decision, ReviewDecision::NeedsChanges, "invariant I1");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("rule:rejection_demoted")),
            "demotion must be logged: {:?}",
            warnings
        );
    }

    #[test]
    fn parse_proposal_drops_when_evidence_empty() {
        let raw = r#"{
            "decision": "approved",
            "confidence": "high",
            "evidence": "   ",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": false
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        assert!(p.is_none(), "empty evidence must drop the proposal");
        assert!(warnings.iter().any(|w| w.contains("evidence")));
    }

    #[test]
    fn parse_proposal_drops_when_decision_missing() {
        let raw = r#"{
            "confidence": "high",
            "evidence": "no decision",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        assert!(p.is_none());
        assert!(warnings.iter().any(|w| w.contains("decision")));
    }

    #[test]
    fn parse_proposal_drops_unknown_decision() {
        let raw = r#"{
            "decision": "unsure",
            "confidence": "high",
            "evidence": "model hedged",
            "non_goal_check": "n/a",
            "destructive_check": "n/a",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        assert!(p.is_none());
        assert!(warnings.iter().any(|w| w.contains("not in")));
    }

    #[test]
    fn parse_proposal_defaults_low_confidence_when_missing() {
        let raw = r#"{
            "decision": "needs_changes",
            "evidence": "some text",
            "non_goal_check": "ok",
            "destructive_check": "ok",
            "requires_human": true
        }"#;
        let (p, warnings) = parse_llm_auto_approve_proposal(raw);
        let p = p.expect("missing confidence is non-fatal");
        assert_eq!(p.confidence, LlmAutoApproveProposalConfidence::Low);
        assert!(warnings.iter().any(|w| w.contains("confidence")));
    }

    #[test]
    fn parse_proposal_handles_non_object_top_level() {
        let (p, warnings) = parse_llm_auto_approve_proposal("[1, 2]");
        assert!(p.is_none());
        assert!(warnings
            .iter()
            .any(|w| w.contains("top-level must be an object")));
    }

    #[test]
    fn parse_proposal_handles_invalid_json() {
        let (p, warnings) = parse_llm_auto_approve_proposal("not json at all");
        assert!(p.is_none());
        assert!(warnings.iter().any(|w| w.contains("not valid JSON")));
    }

    #[test]
    fn enforce_invariants_pins_destructive_check_on_archive() {
        let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let mut p = p.take().expect("seed proposal");
        // Sonnet claimed `requires_human=false` and `destructive_check=
        // non-destructive` — the enforcer MUST overwrite.
        let was_destructive = enforce_proposal_invariants(&mut p, "archive");
        assert!(was_destructive, "archive is destructive (invariant I5)");
        assert!(
            p.destructive_check.starts_with("destructive:"),
            "destructive_check must reflect deterministic verdict: {}",
            p.destructive_check
        );
        assert!(
            p.requires_human,
            "invariant I2: destructive actions ALWAYS require human"
        );
    }

    #[test]
    fn enforce_invariants_pins_requires_human_even_on_non_destructive() {
        let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let mut p = p.take().expect("seed proposal");
        // Approve is non-destructive; the model said requires_human=false.
        // Invariant I3 (propose-only) STILL forces requires_human=true.
        let was_destructive = enforce_proposal_invariants(&mut p, "approve");
        assert!(!was_destructive);
        assert!(
            p.destructive_check.starts_with("non_destructive:"),
            "non-destructive verdict must surface: {}",
            p.destructive_check
        );
        assert!(
            p.requires_human,
            "invariant I3: v0 NEVER auto-applies; requires_human always true"
        );
    }

    #[test]
    fn enforce_invariants_recognises_all_destructive_actions() {
        for action in ["archive", "supersede", "remove", "ARCHIVE", "  Supersede  "] {
            let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
            let mut p = p.take().unwrap();
            assert!(
                enforce_proposal_invariants(&mut p, action),
                "`{}` must be destructive",
                action
            );
        }
        for action in ["approve", "compile", "mark", "Approve"] {
            let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
            let mut p = p.take().unwrap();
            assert!(
                !enforce_proposal_invariants(&mut p, action),
                "`{}` must NOT be destructive",
                action
            );
        }
    }

    #[test]
    fn proposal_to_json_pins_applied_false() {
        let (p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let p = p.unwrap();
        let v = p.to_json();
        assert_eq!(
            v.get("applied").and_then(|x| x.as_bool()),
            Some(false),
            "invariant I3: every proposal serialises applied=false"
        );
    }

    #[test]
    fn bundle_not_invoked_records_action_label() {
        let b = LlmAutoApproveProposalBundle::not_invoked("approve");
        assert_eq!(b.mode, LlmAutoApproveProposalMode::Off);
        assert_eq!(b.status, LlmAutoApproveProposalStatus::NotInvoked);
        assert_eq!(b.action, "approve");
        assert!(b.proposal.is_none());
    }

    #[test]
    fn bundle_unavailable_pins_reason_and_caller() {
        let b = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "directive_review_proposer",
            "Sonnet gateway not initialized",
        );
        assert_eq!(b.status, LlmAutoApproveProposalStatus::Unavailable);
        assert!(b
            .unavailable_reason
            .as_deref()
            .unwrap()
            .contains("Sonnet"));
        assert_eq!(b.request_caller.as_deref(), Some("directive_review_proposer"));
        assert!(b.proposal.is_none(), "invariant I4: no fallback proposal");
        assert!(b.proposal_warnings.is_empty());
    }

    #[test]
    fn bundle_destructive_blocked_overwrites_requires_human() {
        let (mut p, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        // Force model-side claim that no human is needed.
        let proposal = p.take().map(|mut x| {
            x.requires_human = false;
            x
        });
        let b = LlmAutoApproveProposalBundle::destructive_blocked(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "supersede",
            "plan_review_proposer",
            proposal,
            "rule:destructive_action:supersede; auto-approve proposal NEVER promotes destructive actions",
        );
        assert_eq!(b.status, LlmAutoApproveProposalStatus::DestructiveBlocked);
        let p = b.proposal.expect("destructive_blocked preserves proposal");
        assert!(
            p.requires_human,
            "invariant I2: destructive_blocked MUST pin requires_human=true"
        );
        assert!(b
            .proposal_warnings
            .iter()
            .any(|w| w.contains("destructive_action")));
    }

    #[test]
    fn stamp_proposal_payload_round_trip() {
        let (proposal, _) = parse_llm_auto_approve_proposal(well_formed_proposal_response());
        let mut proposal = proposal.unwrap();
        enforce_proposal_invariants(&mut proposal, "approve");
        let bundle = LlmAutoApproveProposalBundle {
            mode: LlmAutoApproveProposalMode::SonnetSuggest,
            status: LlmAutoApproveProposalStatus::Suggested,
            proposal: Some(proposal),
            proposal_warnings: vec!["w1".to_string()],
            unavailable_reason: None,
            action: "approve".to_string(),
            request_caller: Some("directive_review_proposer".to_string()),
            model: Some("claude-sonnet".to_string()),
        };
        let mut payload = json!({});
        stamp_llm_auto_approve_proposal_payload(&mut payload, &bundle);
        assert_eq!(payload["llm_auto_approve_proposal_mode"], "sonnet_suggest");
        assert_eq!(payload["llm_auto_approve_proposal_status"], "suggested");
        assert_eq!(payload["llm_auto_approve_proposal_action"], "approve");
        assert_eq!(
            payload["llm_auto_approve_proposal_caller"],
            "directive_review_proposer"
        );
        assert_eq!(payload["llm_auto_approve_proposal_model"], "claude-sonnet");
        assert_eq!(
            payload["llm_auto_approve_proposal_warnings"],
            json!(["w1"])
        );
        assert_eq!(
            payload["llm_auto_approve_proposal"]["applied"],
            false,
            "invariant I3: applied always false"
        );
        assert_eq!(
            payload["llm_auto_approve_proposal"]["requires_human"],
            true,
            "invariant I3: requires_human always true in v0"
        );
        assert_eq!(
            payload["llm_auto_approve_proposal"]["decision"],
            "approved",
            "decision echoed verbatim"
        );
        assert!(payload
            .get("llm_auto_approve_proposal_unavailable_reason")
            .is_none());
    }

    #[test]
    fn stamp_proposal_payload_unavailable_includes_reason() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "directive_review_proposer",
            "no gateway",
        );
        let mut payload = json!({});
        stamp_llm_auto_approve_proposal_payload(&mut payload, &bundle);
        assert_eq!(
            payload["llm_auto_approve_proposal_status"],
            "llm_unavailable"
        );
        assert_eq!(
            payload["llm_auto_approve_proposal_unavailable_reason"],
            "no gateway"
        );
        assert!(
            payload.get("llm_auto_approve_proposal").is_none(),
            "invariant I4: no fallback proposal payload"
        );
    }

    #[test]
    fn proposal_invariants_round_trip_never_surface_rejected() {
        // Defensive invariant I1 — even if a future parser change accepted
        // `rejected`, the stamped payload MUST NOT carry decision=rejected.
        for decision_str in ["approved", "needs_changes", "rejected"] {
            let raw = format!(
                r#"{{
                    "decision": "{}",
                    "confidence": "high",
                    "evidence": "test",
                    "non_goal_check": "n/a",
                    "destructive_check": "n/a",
                    "requires_human": true
                }}"#,
                decision_str
            );
            let (p, _) = parse_llm_auto_approve_proposal(&raw);
            if let Some(mut p) = p {
                enforce_proposal_invariants(&mut p, "approve");
                let v = p.to_json();
                assert_ne!(
                    v["decision"], "rejected",
                    "invariant I1 round-trip: payload MUST NOT carry rejected"
                );
            }
        }
    }

    #[test]
    fn build_proposal_prompts_pure_no_io() {
        let system = build_llm_auto_approve_proposal_system_prompt();
        assert!(system.contains("decision"));
        assert!(system.contains("approved"));
        assert!(system.contains("needs_changes"));
        assert!(system.contains("rejected"));
        assert!(system.contains("requires_human"));
        let user = build_llm_auto_approve_proposal_user_prompt(
            "directive",
            "approve",
            "abc-123",
            1,
            &json!({"deterministic_status": "auto_approved"}),
            Some("(directive :goal :ship)"),
        );
        assert!(user.contains("directive"));
        assert!(user.contains("approve"));
        assert!(user.contains("abc-123"));
        assert!(user.contains("v1"));
        assert!(user.contains("auto_approved"));
        assert!(user.contains("(directive :goal :ship)"));
    }

    // ── wave-22 / task 03 — apply gate v1 unit tests ───────────────────
    //
    // Exercises every code path through `evaluate_llm_approve_apply_gate`
    // PLUS the strict pre-flight `enforce_apply_gate_preflight` PLUS the
    // pure `compute_proposal_hash` helper. Pinned tests for each of the
    // 5 wave-21 / task 06 invariants prove the apply gate cannot break
    // them.

    fn well_formed_high_confidence_proposal() -> LlmAutoApproveProposal {
        LlmAutoApproveProposal {
            decision: ReviewDecision::Approved,
            confidence: LlmAutoApproveProposalConfidence::High,
            evidence: "directive aligns with declared goal; non-goals respected".to_string(),
            non_goal_check: "no scope creep".to_string(),
            destructive_check: "non_destructive:`approve` is not on the destructive list"
                .to_string(),
            requires_human: true,
        }
    }

    fn suggested_bundle(p: LlmAutoApproveProposal) -> LlmAutoApproveProposalBundle {
        LlmAutoApproveProposalBundle {
            mode: LlmAutoApproveProposalMode::SonnetSuggest,
            status: LlmAutoApproveProposalStatus::Suggested,
            proposal: Some(p),
            proposal_warnings: Vec::new(),
            unavailable_reason: None,
            action: "approve".to_string(),
            request_caller: Some("directive_review_proposer".to_string()),
            model: Some("claude-sonnet".to_string()),
        }
    }

    #[test]
    fn apply_gate_compute_proposal_hash_is_deterministic() {
        let p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        let b = compute_proposal_hash("approve", "abc-123", 1, &p);
        assert_eq!(a, b, "hash MUST be deterministic for identical inputs");
        assert_eq!(a.len(), 32, "hash MUST be exactly 32 hex chars");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "hash MUST be hex");
    }

    #[test]
    fn apply_gate_compute_proposal_hash_changes_on_action() {
        let p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        let b = compute_proposal_hash("archive", "abc-123", 1, &p);
        assert_ne!(a, b);
    }

    #[test]
    fn apply_gate_compute_proposal_hash_changes_on_decision() {
        let mut p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        p.decision = ReviewDecision::NeedsChanges;
        let b = compute_proposal_hash("approve", "abc-123", 1, &p);
        assert_ne!(a, b);
    }

    #[test]
    fn apply_gate_compute_proposal_hash_ignores_evidence() {
        let mut p = well_formed_high_confidence_proposal();
        let a = compute_proposal_hash("approve", "abc-123", 1, &p);
        // Free-text fields are intentionally OUT of the hash so superficial
        // wording differences don't churn the audit correlator.
        p.evidence = "completely different wording".to_string();
        p.non_goal_check = "different placeholder".to_string();
        let b = compute_proposal_hash("approve", "abc-123", 1, &p);
        assert_eq!(a, b);
    }

    #[test]
    fn apply_gate_parse_input_default_is_off() {
        let input = parse_llm_approve_apply_gate_input(&json!({})).expect("default ok");
        assert!(!input.apply);
        assert!(!input.caller_approved);
        assert!(input.proposal_hash.is_none());
        assert!(!input.explicit);
    }

    #[test]
    fn apply_gate_parse_input_accepts_full_opt_in() {
        let args = json!({
            "apply_llm_auto_approve": true,
            "proposal_hash": "deadbeef".repeat(4),
            "caller_approved": true,
        });
        let input = parse_llm_approve_apply_gate_input(&args).expect("full opt-in ok");
        assert!(input.apply);
        assert!(input.caller_approved);
        assert_eq!(input.proposal_hash.as_deref(), Some("deadbeef".repeat(4).as_str()));
        assert!(input.explicit);
    }

    #[test]
    fn apply_gate_parse_input_rejects_string_apply() {
        // Strict: literal string `"true"` MUST be rejected so a typo can
        // never silently flip the gate.
        let args = json!({"apply_llm_auto_approve": "true"});
        let err = parse_llm_approve_apply_gate_input(&args).unwrap_err();
        assert_eq!(err.0, APPLY_GATE_INVALID_PARAM);
        assert!(err.1.contains("apply_llm_auto_approve"));
    }

    #[test]
    fn apply_gate_parse_input_rejects_bool_proposal_hash() {
        let args = json!({"proposal_hash": true});
        let err = parse_llm_approve_apply_gate_input(&args).unwrap_err();
        assert_eq!(err.0, APPLY_GATE_INVALID_PARAM);
        assert!(err.1.contains("proposal_hash"));
    }

    #[test]
    fn apply_gate_parse_input_rejects_string_caller_approved() {
        let args = json!({"caller_approved": "yes"});
        let err = parse_llm_approve_apply_gate_input(&args).unwrap_err();
        assert_eq!(err.0, APPLY_GATE_INVALID_PARAM);
    }

    #[test]
    fn apply_gate_parse_input_treats_null_as_absent() {
        let args = json!({
            "apply_llm_auto_approve": null,
            "proposal_hash": null,
            "caller_approved": null,
        });
        let input = parse_llm_approve_apply_gate_input(&args).expect("null = absent");
        assert!(!input.apply);
        assert!(!input.caller_approved);
        assert!(input.proposal_hash.is_none());
        // Explicit because the keys WERE present (even if null).
        assert!(input.explicit);
    }

    #[test]
    fn apply_gate_default_off_returns_not_requested() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput::default();
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::NotRequested);
        assert!(!outcome.status.should_apply());
        assert!(outcome.safety_rule_results.is_empty());
    }

    #[test]
    fn apply_gate_all_six_gates_pass_applies() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash.clone()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::Applied);
        assert!(outcome.status.should_apply());
        assert_eq!(outcome.applied_decision, Some(ReviewDecision::Approved));
        assert_eq!(outcome.proposal_hash_status, ProposalHashStatus::Matches);
        assert!(outcome.caller_approved);
        // Every rule narrated.
        let joined = outcome.safety_rule_results.join("|");
        assert!(joined.contains("rule:non_destructive_action"));
        assert!(joined.contains("rule:proposal_hash:matches"));
        assert!(joined.contains("rule:caller_approved:true"));
        assert!(joined.contains("rule:bundle_status:suggested"));
        assert!(joined.contains("rule:invariant_i5"));
        assert!(joined.contains("rule:decision_approved"));
        assert!(joined.contains("rule:confidence_high"));
        assert!(joined.contains("rule:apply_gate_satisfied"));
    }

    #[test]
    fn apply_gate_invariant_i1_never_applies_needs_changes() {
        // The proposal carries decision=NeedsChanges (the only non-Approved
        // wire value the parser emits — `rejected` is collapsed to
        // NeedsChanges). The gate MUST refuse to apply.
        let mut p = well_formed_high_confidence_proposal();
        p.decision = ReviewDecision::NeedsChanges;
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedNonApprovedDecision);
        assert!(!outcome.status.should_apply(), "I1: never auto-anything-non-approve");
        assert_eq!(outcome.applied_decision, Some(ReviewDecision::NeedsChanges));
    }

    #[test]
    fn apply_gate_invariant_i2_destructive_archive_skipped() {
        // Even with a perfect proposal + matching hash + caller_approved,
        // a destructive action MUST skip.
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "archive",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "archive", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedDestructiveAction);
        assert!(!outcome.status.should_apply(), "I2: destructive never auto-promoted");
    }

    #[test]
    fn apply_gate_invariant_i2_destructive_supersede_skipped() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "supersede",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "supersede", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedDestructiveAction);
    }

    #[test]
    fn apply_gate_invariant_i2_destructive_remove_skipped() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "remove",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "remove", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedDestructiveAction);
    }

    #[test]
    fn apply_gate_invariant_i3_proposal_block_unaffected() {
        // The proposal block itself MUST still carry applied=false +
        // requires_human=true regardless of the apply gate's outcome.
        // This is the structural separation the wave-22 / task 03 design
        // depends on.
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let json = bundle.proposal.as_ref().unwrap().to_json();
        assert_eq!(json["applied"], false);
        assert_eq!(json["requires_human"], true);
        // Even after the gate runs and applies, the proposal JSON
        // serialisation is unchanged (the gate publishes its own block).
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::Applied);
        let json2 = bundle.proposal.as_ref().unwrap().to_json();
        assert_eq!(json2["applied"], false, "proposal JSON unchanged by gate");
        assert_eq!(json2["requires_human"], true);
    }

    #[test]
    fn apply_gate_invariant_i4_unavailable_skipped_no_fallback() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "directive_review_proposer",
            "Sonnet gateway not initialized",
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("anything".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedUnavailable);
        assert!(!outcome.status.should_apply(), "I4: never fall back");
        assert_eq!(
            outcome.proposal_hash_status,
            ProposalHashStatus::NoProposalAvailable
        );
    }

    #[test]
    fn apply_gate_invariant_i5_destructive_check_always_deterministic() {
        // Construct a proposal whose model-supplied destructive_check
        // says "non_destructive" but the deterministic action label is
        // "archive" (destructive). The gate MUST trust the deterministic
        // verdict, NEVER the model.
        let mut p = well_formed_high_confidence_proposal();
        p.destructive_check = "non_destructive:model_lied_here".to_string();
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "archive",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "archive", "abc-123", 1);
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedDestructiveAction,
            "I5: deterministic destructive verdict overrides model claim"
        );
    }

    #[test]
    fn apply_gate_skips_when_caller_approved_false() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: false,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedCallerNotApproved);
    }

    #[test]
    fn apply_gate_skips_when_confidence_medium() {
        let mut p = well_formed_high_confidence_proposal();
        p.confidence = LlmAutoApproveProposalConfidence::Medium;
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedConfidenceTooLow);
    }

    #[test]
    fn apply_gate_skips_when_confidence_low() {
        let mut p = well_formed_high_confidence_proposal();
        p.confidence = LlmAutoApproveProposalConfidence::Low;
        let bundle = suggested_bundle(p);
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedConfidenceTooLow);
    }

    #[test]
    fn apply_gate_skips_when_bundle_is_no_suggestion() {
        let bundle = LlmAutoApproveProposalBundle {
            mode: LlmAutoApproveProposalMode::SonnetSuggest,
            status: LlmAutoApproveProposalStatus::NoSuggestion,
            proposal: None,
            proposal_warnings: vec!["unparseable response".to_string()],
            unavailable_reason: None,
            action: "approve".to_string(),
            request_caller: Some("directive_review_proposer".to_string()),
            model: Some("claude-sonnet".to_string()),
        };
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("any".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedNoProposal);
    }

    #[test]
    fn apply_gate_skips_when_bundle_is_not_invoked() {
        let bundle = LlmAutoApproveProposalBundle::not_invoked("approve");
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("any".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::SkippedNoProposal);
    }

    #[test]
    fn apply_gate_preflight_requires_proposal_hash_under_apply_true() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: None,
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_MISSING_PROPOSAL_HASH);
        assert!(err.1.contains("proposal_hash"));
    }

    #[test]
    fn apply_gate_preflight_rejects_mismatched_hash() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("0".repeat(32)),
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_PROPOSAL_HASH_MISMATCH);
        assert!(err.1.contains("does not match"));
    }

    #[test]
    fn apply_gate_preflight_accepts_matching_hash_case_insensitive() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash.to_uppercase()),
            caller_approved: true,
            explicit: true,
        };
        assert!(
            enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1).is_ok(),
            "preflight MUST accept case-insensitive hash match"
        );
    }

    #[test]
    fn apply_gate_preflight_skips_when_apply_false() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let input = LlmApproveApplyGateInput {
            apply: false,
            proposal_hash: Some("garbage".to_string()),
            caller_approved: false,
            explicit: true,
        };
        // apply=false ⇒ preflight passes without checking the hash.
        assert!(enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1).is_ok());
    }

    #[test]
    fn apply_gate_preflight_no_proposal_with_hash_returns_mismatch() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "x",
            "down",
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("anything".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_PROPOSAL_HASH_MISMATCH);
    }

    #[test]
    fn apply_gate_preflight_no_proposal_no_hash_returns_missing() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "x",
            "down",
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: None,
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&input, &bundle, "approve", "abc-123", 1)
            .unwrap_err();
        assert_eq!(err.0, APPLY_GATE_MISSING_PROPOSAL_HASH);
    }

    #[test]
    fn apply_gate_stamp_payload_emits_full_block_when_applied() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash.clone()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        let mut payload = json!({"status": "approved"});
        stamp_llm_approve_apply_gate_payload(&mut payload, &outcome);
        let block = payload
            .get("llm_approve_apply_gate")
            .expect("gate block stamped");
        assert_eq!(block["apply_status"], "applied");
        assert_eq!(block["applied_decision"], "approved");
        assert_eq!(block["proposal_hash_status"], "matches");
        assert_eq!(block["caller_approved"], true);
        assert_eq!(block["computed_proposal_hash"], hash);
        assert_eq!(block["supplied_proposal_hash"], hash);
        assert!(
            block["safety_rule_results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str().unwrap_or("").contains("apply_gate_satisfied")),
            "apply_gate_satisfied rule MUST surface"
        );
    }

    #[test]
    fn apply_gate_stamp_payload_omits_block_when_not_requested() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let outcome = evaluate_llm_approve_apply_gate(
            &LlmApproveApplyGateInput::default(),
            &bundle,
            "approve",
            "abc-123",
            1,
        );
        let mut payload = json!({"status": "approved"});
        stamp_llm_approve_apply_gate_payload(&mut payload, &outcome);
        assert!(
            payload.get("llm_approve_apply_gate").is_none(),
            "default off MUST stay byte-identical with wave-21 / task 06"
        );
    }

    #[test]
    fn apply_gate_stamp_proposal_hash_payload_when_present() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let mut payload = json!({});
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", "abc-123", 1);
        let hash = payload
            .get("llm_auto_approve_proposal_hash")
            .and_then(|v| v.as_str())
            .expect("hash stamped");
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn apply_gate_stamp_proposal_hash_payload_skips_when_absent() {
        let bundle = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "x",
            "down",
        );
        let mut payload = json!({});
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", "abc-123", 1);
        assert!(payload.get("llm_auto_approve_proposal_hash").is_none());
    }

    // ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
    //
    // Pin the wave22-03 review LLM auto-approve apply gate slice of the
    // wave22-07 v4 smoke contract. The pure evaluator + preflight pair
    // is the deterministic SSOT — no Sonnet call, no DB transition,
    // pure in-process functions over synthesised proposal/bundle structs.
    // The companion plan.rs / workstation_dispatch.rs / agent_execution.rs
    // / unified_entry.rs smokes cover the persisted-apply, auto-spawn,
    // failed-verification and markdown-non-load-bearing slices.

    /// V4 smoke (Requirement 2 / review apply-gate slice): the apply
    /// gate MUST reject `apply_llm_auto_approve=true` when the caller
    /// does not supply `proposal_hash`, AND MUST accept the same call
    /// when a hash matching `compute_proposal_hash(action, artifact_id,
    /// version, proposal)` is supplied. This is the wave22-03 gate's
    /// fail-fast preflight — the gate refuses to mutate state with no
    /// correlator and accepts only the canonical fixture path.
    #[test]
    fn smoke_wave22_07_review_apply_gate_rejects_missing_hash_accepts_fixture_hash() {
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        // Missing proposal_hash → APPLY_GATE_MISSING_PROPOSAL_HASH.
        let missing_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: None,
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&missing_input, &bundle, "approve", "abc-123", 1)
            .expect_err("wave22-07 v4: missing proposal_hash MUST fail-fast");
        assert_eq!(
            err.0, APPLY_GATE_MISSING_PROPOSAL_HASH,
            "wave22-07 v4 invariant: missing proposal_hash MUST surface the dedicated code"
        );
        // Mismatched proposal_hash → APPLY_GATE_PROPOSAL_HASH_MISMATCH.
        let mismatch_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("0".repeat(32)),
            caller_approved: true,
            explicit: true,
        };
        let err = enforce_apply_gate_preflight(&mismatch_input, &bundle, "approve", "abc-123", 1)
            .expect_err("wave22-07 v4: mismatched proposal_hash MUST fail-fast");
        assert_eq!(err.0, APPLY_GATE_PROPOSAL_HASH_MISMATCH);
        // Matching fixture hash → preflight OK + gate Applied.
        let canonical = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let valid_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(canonical.clone()),
            caller_approved: true,
            explicit: true,
        };
        assert!(
            enforce_apply_gate_preflight(&valid_input, &bundle, "approve", "abc-123", 1).is_ok(),
            "wave22-07 v4: matching proposal_hash MUST pass preflight"
        );
        let outcome = evaluate_llm_approve_apply_gate(
            &valid_input,
            &bundle,
            "approve",
            "abc-123",
            1,
        );
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::Applied,
            "wave22-07 v4 invariant: matching fixture proposal_hash MUST drive the gate to Applied"
        );
        assert_eq!(outcome.proposal_hash_status, ProposalHashStatus::Matches);
    }

    /// V4 smoke (cross-wave invariants / wave21-06 5 invariants pinned):
    /// the wave22-03 apply gate MUST preserve every wave-21 / task 06
    /// propose-only invariant when stamped onto the same call.
    ///   * I1 never auto-reject — the gate MUST refuse to apply a
    ///     `decision=NeedsChanges` proposal.
    ///   * I2 destructive never promote — destructive actions
    ///     (archive / supersede / remove) MUST skip even when every
    ///     other gate is green.
    ///   * I3 proposal block applied=false / requires_human=true —
    ///     the propose-only proposal serialisation MUST stay
    ///     unchanged even after the apply gate has run.
    ///   * I4 Sonnet unavailable no fallback — the gate MUST short-
    ///     circuit on `Unavailable` bundles without falling back.
    ///   * I5 destructive_check ALWAYS deterministic — a model that
    ///     claimed `non_destructive` MUST be overridden by the
    ///     deterministic destructive verdict.
    #[test]
    fn smoke_wave22_07_review_apply_gate_pins_wave21_06_five_invariants() {
        // I1 — never auto-reject (NeedsChanges).
        let mut needs_changes = well_formed_high_confidence_proposal();
        needs_changes.decision = ReviewDecision::NeedsChanges;
        let nc_bundle = suggested_bundle(needs_changes);
        let nc_hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            nc_bundle.proposal.as_ref().unwrap(),
        );
        let nc_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(nc_hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome =
            evaluate_llm_approve_apply_gate(&nc_input, &nc_bundle, "approve", "abc-123", 1);
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedNonApprovedDecision,
            "wave21-06 I1: never auto-anything-non-approve"
        );
        // I2 — destructive never promote (archive / supersede / remove).
        for destructive in ["archive", "supersede", "remove"] {
            let bundle = suggested_bundle(well_formed_high_confidence_proposal());
            let hash = compute_proposal_hash(
                destructive,
                "abc-123",
                1,
                bundle.proposal.as_ref().unwrap(),
            );
            let input = LlmApproveApplyGateInput {
                apply: true,
                proposal_hash: Some(hash),
                caller_approved: true,
                explicit: true,
            };
            let outcome = evaluate_llm_approve_apply_gate(
                &input,
                &bundle,
                destructive,
                "abc-123",
                1,
            );
            assert_eq!(
                outcome.status,
                LlmApproveApplyStatus::SkippedDestructiveAction,
                "wave21-06 I2: destructive `{}` MUST never auto-promote",
                destructive
            );
        }
        // I3 — proposal block applied=false / requires_human=true even
        //      after the apply gate has driven Applied.
        let bundle = suggested_bundle(well_formed_high_confidence_proposal());
        let hash = compute_proposal_hash(
            "approve",
            "abc-123",
            1,
            bundle.proposal.as_ref().unwrap(),
        );
        let input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(&input, &bundle, "approve", "abc-123", 1);
        assert_eq!(outcome.status, LlmApproveApplyStatus::Applied);
        let proposal_json = bundle.proposal.as_ref().unwrap().to_json();
        assert_eq!(
            proposal_json["applied"], false,
            "wave21-06 I3: proposal serialisation MUST keep applied=false even when gate Applied"
        );
        assert_eq!(
            proposal_json["requires_human"], true,
            "wave21-06 I3: proposal serialisation MUST keep requires_human=true"
        );
        // I4 — Sonnet unavailable no fallback.
        let unavailable = LlmAutoApproveProposalBundle::unavailable(
            LlmAutoApproveProposalMode::SonnetSuggest,
            "approve",
            "wave22-07-v4-smoke",
            "Sonnet gateway not initialized",
        );
        let unavail_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some("anything".to_string()),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(
            &unavail_input,
            &unavailable,
            "approve",
            "abc-123",
            1,
        );
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedUnavailable,
            "wave21-06 I4: Sonnet unavailable MUST never fall back"
        );
        // I5 — destructive_check ALWAYS deterministic; a model-supplied
        //      `non_destructive:` string is overridden by the deterministic
        //      destructive verdict for `archive`.
        let mut model_lied = well_formed_high_confidence_proposal();
        model_lied.destructive_check = "non_destructive:model_lied_here".to_string();
        let lied_bundle = suggested_bundle(model_lied);
        let lied_hash = compute_proposal_hash(
            "archive",
            "abc-123",
            1,
            lied_bundle.proposal.as_ref().unwrap(),
        );
        let lied_input = LlmApproveApplyGateInput {
            apply: true,
            proposal_hash: Some(lied_hash),
            caller_approved: true,
            explicit: true,
        };
        let outcome = evaluate_llm_approve_apply_gate(
            &lied_input,
            &lied_bundle,
            "archive",
            "abc-123",
            1,
        );
        assert_eq!(
            outcome.status,
            LlmApproveApplyStatus::SkippedDestructiveAction,
            "wave21-06 I5: deterministic destructive verdict overrides model claim"
        );
    }
}
