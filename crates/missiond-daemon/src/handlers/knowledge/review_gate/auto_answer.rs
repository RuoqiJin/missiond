use serde_json::{json, Value};

use super::{
    evaluate_review_automation, ReviewAutomationContext, ReviewAutomationPolicy, ReviewDecision,
};

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
pub(super) const DESTRUCTIVE_REVIEW_ACTIONS: &[&str] = &["archive", "supersede", "remove"];

/// True iff the supplied action label is on the destructive list.
/// Case-insensitive + trimmed so caller variations (`"Archive"`,
/// `"  REMOVE  "`) collide with the canonical lowercase form.
pub(crate) fn is_destructive_review_action(action: &str) -> bool {
    let normalised = action.trim().to_ascii_lowercase();
    DESTRUCTIVE_REVIEW_ACTIONS.iter().any(|a| **a == normalised)
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
            AutoAnswerStatus::SkippedDestructiveAction => "skipped_destructive_action",
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
    let inner = evaluate_review_automation(ReviewAutomationPolicy::AutoSafe, ctx, caller_decision);
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
pub(crate) fn stamp_auto_answer_payload(payload: &mut Value, outcome: &AutoAnswerOutcome) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert(
        "auto_answer_policy".to_string(),
        json!(outcome.policy.as_str()),
    );
    map.insert("policy_result".to_string(), json!(outcome.status.as_str()));
    if let Some(d) = outcome.selected_decision {
        map.insert("selected_decision".to_string(), json!(d.as_str()));
    }
    map.insert(
        "safety_rule_results".to_string(),
        json!(outcome.safety_rule_results),
    );
    map.insert("requires_human".to_string(), json!(outcome.requires_human));
}
