use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-18 / task 07 — review automation policy v0
//
// Adds an explicit AUTONOMY policy layer for the wave-15 / wave-16
// resolution surfaces (directive approve/archive, plan approve/mark/
// supersede, workflow resolve_review). This is a separate dimension from
// the wave-14 `review_gate_policy` (`manual | emit_question | off`) which
// controls EMISSION; this knob controls RESOLUTION.
//
// Three states (default = `Manual` so legacy callers stay byte-identical
// when they never sent the field):
//
//   manual     → existing behaviour. Caller-supplied
//                `review_decision` is the only authority; absence stays
//                "no resolution".
//   suggest    → handler computes a `suggested_review_decision` from the
//                deterministic safety inspector and surfaces it on the
//                response. NO mutation. The caller still has to supply
//                an explicit `review_decision` to flip state. Useful for
//                review-tooling UIs that want a hint.
//   auto_safe  → handler MAY resolve automatically to `approved` ONLY
//                when ALL deterministic safety rules pass:
//                  1. caller explicitly opted in
//                     (`review_automation_policy="auto_safe"`)
//                  2. artifact came from a deterministic / dry-run
//                     producer mode
//                  3. no file write happened OR the file hash matches
//                     the expected hash (when the caller supplied one)
//                  4. no protected source / target involved
//                  5. no unresolved review conflicts on the response
//                     payload (e.g. `review_question_warning` block,
//                     `file_write_error`)
//                If any rule fails, the helper degrades to
//                `suggest` semantics — surfaces the suggestion + the
//                blocking reasons on `automation_reasons[]` and refuses
//                to mutate. NEVER calls an LLM.
//
// Authority precedence (most → least):
//   1. caller-supplied `review_decision` (explicit human choice always
//      wins; the policy NEVER overrides an explicit decision)
//   2. `auto_safe` policy auto-promoted to `Approved` when every safety
//      rule passes (suggested == Approved + no blocking reasons)
//   3. otherwise the suggestion is informational only
//
// Response invariants (always stamped when the policy was supplied):
//   * `review_automation_policy`     — resolved policy label
//   * `review_automation_status`     — outcome label (see `AutomationStatus`)
//   * `suggested_review_decision`    — the deterministic suggestion
//   * `automation_reasons`           — array of safety-rule outcome strings
//
// Bus & DB invariants:
//   * The wave-15 / wave-16 manager-side validators (scope match, stale
//     version, etc.) ALWAYS run BEFORE this policy fires. A failed
//     envelope short-circuits the whole resolution path; the automation
//     policy never gets the chance to mutate.
//   * `auto_safe` only ever auto-promotes to `Approved`. It NEVER
//     auto-rejects — refusing a draft is a human-only decision.
// ───────────────────────────────────────────────────────────────────────

/// Wave-18 / task 07 — three-state autonomy policy controlling whether
/// the explicit-resolution surfaces may auto-resolve based on the
/// deterministic safety inspector. Default `Manual` matches the
/// pre-wave-18 behaviour so legacy callers stay byte-identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewAutomationPolicy {
    /// Default: caller-supplied `review_decision` is the only authority.
    /// The policy fields are absent from the response (byte-identical
    /// with pre-wave-18 callers that never sent the knob).
    Manual,
    /// Compute a suggestion and surface it on the response. NEVER
    /// mutates state; the caller still has to supply an explicit
    /// `review_decision` to flip the artifact.
    Suggest,
    /// Compute a suggestion + the safety-rule outcomes; auto-promote
    /// to `Approved` when every rule passes. NEVER auto-rejects.
    AutoSafe,
}

impl ReviewAutomationPolicy {
    /// Lower-snake-case label for the response payload.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ReviewAutomationPolicy::Manual => "manual",
            ReviewAutomationPolicy::Suggest => "suggest",
            ReviewAutomationPolicy::AutoSafe => "auto_safe",
        }
    }
}

/// Parse the wave-18 `review_automation_policy` arg. Unknown / absent /
/// blank values collapse to `Manual` so legacy callers (which never sent
/// the field) keep their byte-identical response shape.
///
/// Recognised values (case-insensitive, trimmed):
///   * `"manual"`     → [`ReviewAutomationPolicy::Manual`] (default)
///   * `"suggest"`    → [`ReviewAutomationPolicy::Suggest`]
///   * `"auto_safe"`  → [`ReviewAutomationPolicy::AutoSafe`]
pub(crate) fn parse_review_automation_policy(args: &Value) -> ReviewAutomationPolicy {
    let raw = args
        .get("review_automation_policy")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase());
    match raw.as_deref() {
        Some("suggest") => ReviewAutomationPolicy::Suggest,
        Some("auto_safe") | Some("auto-safe") => ReviewAutomationPolicy::AutoSafe,
        _ => ReviewAutomationPolicy::Manual,
    }
}

/// True iff the caller actually included a `review_automation_policy`
/// key in the request JSON (regardless of value). Used to keep
/// pre-wave-18 callers byte-identical when they never opted in.
pub(crate) fn review_automation_policy_was_explicit(args: &Value) -> bool {
    args.get("review_automation_policy").is_some()
}

/// Caller-supplied safety inputs the deterministic inspector consults.
/// The handler builds this from artifact-side knowledge (mode, file
/// outcome, protected list, expected hash) before invoking
/// [`evaluate_review_automation`]. Pure data; no async / no DB.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReviewAutomationContext {
    /// `true` iff the artifact was produced in a deterministic / dry-run
    /// producer mode (e.g. directive `compiler_mode="dry_run"`,
    /// workflow `compile_mode="deterministic"`). LLM-driven artifacts
    /// (sonnet) leave this `false` and ALWAYS block `auto_safe`.
    pub deterministic_mode: bool,
    /// `true` iff a file write was attempted as part of this artifact's
    /// landing. When `false`, the "no-file-write" rule auto-passes.
    pub file_write_attempted: bool,
    /// `true` iff the file write succeeded (`AttemptOutcome::Written`).
    /// When `file_write_attempted=true` AND this is `false`, the
    /// "no-file-write OR matching hash" rule fails.
    pub file_write_succeeded: bool,
    /// On-disk SHA-256 the writer landed (when `file_write_succeeded`).
    pub actual_file_sha256: Option<String>,
    /// Caller-supplied expected SHA-256. When present, the inspector
    /// requires it to match `actual_file_sha256` exactly.
    pub expected_file_sha256: Option<String>,
    /// `true` iff EITHER the source OR the target of this artifact is on
    /// a protected list (e.g. capability_usage protected source/target).
    /// Mirrors the wave-5 `protected_target_policy="review-only"`
    /// semantics — a protected pair always blocks `auto_safe`.
    pub protected_source_or_target: bool,
    /// Free-form additional blocking reasons the handler already detected
    /// (e.g. `bus_publish_warning`, partial-status splice). Each entry
    /// surfaces verbatim under `automation_reasons[]` and blocks
    /// `auto_safe`.
    pub additional_blockers: Vec<String>,
}

/// Status label surfaced under `review_automation_status` on the
/// response. Pure projection of the policy + safety inspector outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationStatus {
    /// `policy=manual` (default). No automation involvement; response
    /// stays pre-wave-18 byte-identical.
    NotEvaluated,
    /// `policy=suggest`. Deterministic suggestion stamped; no mutation.
    Suggested,
    /// `policy=auto_safe`. Every safety rule passed; the handler MAY
    /// auto-promote to `Approved`.
    AutoApproved,
    /// `policy=auto_safe` but at least one safety rule failed. Stamps
    /// the suggestion + blocking reasons; refuses to mutate. The
    /// resolution path falls through to the explicit `review_decision`
    /// (or the legacy quiet path when absent).
    AutoSafeBlocked,
    /// `policy=suggest`/`auto_safe` but the caller ALSO supplied an
    /// explicit `review_decision`. The explicit decision wins; we still
    /// surface the suggestion for observability but never let the
    /// automation override.
    OverriddenByExplicitDecision,
}

impl AutomationStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AutomationStatus::NotEvaluated => "not_evaluated",
            AutomationStatus::Suggested => "suggested",
            AutomationStatus::AutoApproved => "auto_approved",
            AutomationStatus::AutoSafeBlocked => "auto_safe_blocked",
            AutomationStatus::OverriddenByExplicitDecision => "overridden_by_explicit_decision",
        }
    }
}

/// Pure outcome of [`evaluate_review_automation`]. The handler consumes
/// this projection to decide whether to mutate, what to splice into the
/// response, and what `automation_reasons[]` to surface. Side-effect
/// free — no DB, no bus, no LLM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewAutomationOutcome {
    pub policy: ReviewAutomationPolicy,
    pub status: AutomationStatus,
    /// The deterministic suggestion. Always populated under `Suggest` /
    /// `AutoSafe` (even on `AutoSafeBlocked`). `None` under `Manual`.
    pub suggested_decision: Option<ReviewDecision>,
    /// Reasons the inspector surfaces. Each entry is a short
    /// `code:detail` string ready to be splatted onto
    /// `automation_reasons[]`. Always populated under `Suggest` /
    /// `AutoSafe` (rule outcomes), empty under `Manual`.
    pub reasons: Vec<String>,
    /// `true` iff the handler should perform the auto-resolve transition
    /// (always `Approved` when set). False otherwise — including the
    /// `Suggest` and `AutoSafeBlocked` paths.
    pub may_auto_resolve: bool,
}

/// Run the deterministic safety inspector for the supplied policy + ctx
/// + caller-supplied explicit decision (if any). Pure / side-effect
/// free. The handler then:
///   * If `may_auto_resolve=true` AND the manager-side validators (run
///     elsewhere) succeeded, performs the existing approve transition.
///   * Otherwise, splices the outcome onto the response and falls back
///     to the legacy explicit-decision path.
///
/// `caller_decision` is the explicit `review_decision` the caller
/// supplied (if any). When present under a non-`Manual` policy, the
/// outcome short-circuits to `OverriddenByExplicitDecision` — explicit
/// human authority always wins.
pub(crate) fn evaluate_review_automation(
    policy: ReviewAutomationPolicy,
    ctx: &ReviewAutomationContext,
    caller_decision: Option<ReviewDecision>,
) -> ReviewAutomationOutcome {
    if matches!(policy, ReviewAutomationPolicy::Manual) {
        return ReviewAutomationOutcome {
            policy,
            status: AutomationStatus::NotEvaluated,
            suggested_decision: None,
            reasons: Vec::new(),
            may_auto_resolve: false,
        };
    }

    // Compute reasons + suggestion regardless of explicit override so the
    // observer payload still carries the audit trail.
    let (suggestion, blocking_reasons, passing_reasons) = inspect_safety_rules(ctx);

    if let Some(_decision) = caller_decision {
        // Explicit caller-supplied decision wins. Surface the
        // suggestion + the rule outcomes for audit, but never mutate
        // via the policy path.
        let mut reasons = passing_reasons;
        reasons.extend(blocking_reasons);
        return ReviewAutomationOutcome {
            policy,
            status: AutomationStatus::OverriddenByExplicitDecision,
            suggested_decision: Some(suggestion),
            reasons,
            may_auto_resolve: false,
        };
    }

    match policy {
        ReviewAutomationPolicy::Suggest => {
            let mut reasons = passing_reasons;
            reasons.extend(blocking_reasons);
            ReviewAutomationOutcome {
                policy,
                status: AutomationStatus::Suggested,
                suggested_decision: Some(suggestion),
                reasons,
                may_auto_resolve: false,
            }
        }
        ReviewAutomationPolicy::AutoSafe => {
            // auto_safe NEVER auto-rejects — refusing a draft is a
            // human-only decision. So even when the suggestion is
            // anything other than Approved we degrade to "blocked"
            // and never mutate.
            let approved_suggestion = matches!(suggestion, ReviewDecision::Approved);
            if blocking_reasons.is_empty() && approved_suggestion {
                ReviewAutomationOutcome {
                    policy,
                    status: AutomationStatus::AutoApproved,
                    suggested_decision: Some(suggestion),
                    reasons: passing_reasons,
                    may_auto_resolve: true,
                }
            } else {
                let mut reasons = passing_reasons;
                reasons.extend(blocking_reasons);
                if !approved_suggestion {
                    reasons.push(
                        "auto_safe_refuses_non_approved:auto_safe never auto-rejects; falling back to manual decision"
                            .to_string(),
                    );
                }
                ReviewAutomationOutcome {
                    policy,
                    status: AutomationStatus::AutoSafeBlocked,
                    suggested_decision: Some(suggestion),
                    reasons,
                    may_auto_resolve: false,
                }
            }
        }
        ReviewAutomationPolicy::Manual => unreachable!("Manual handled above"),
    }
}

/// Inspect the safety rules. Returns `(suggested_decision,
/// blocking_reasons, passing_reasons)`. Pure / side-effect free.
///
/// The suggestion defaults to `Approved` and degrades to `NeedsChanges`
/// when one of the soft rules trips. We never default to `Rejected` —
/// rejection requires explicit human input.
fn inspect_safety_rules(
    ctx: &ReviewAutomationContext,
) -> (ReviewDecision, Vec<String>, Vec<String>) {
    let mut blocking: Vec<String> = Vec::new();
    let mut passing: Vec<String> = Vec::new();

    // Rule 1 — deterministic / dry-run producer mode.
    if ctx.deterministic_mode {
        passing
            .push("rule:deterministic_mode:producer ran in deterministic/dry-run mode".to_string());
    } else {
        blocking.push(
            "rule:deterministic_mode:producer was not deterministic (e.g. sonnet/LLM-driven); auto_safe refuses to approve LLM output unattended"
                .to_string(),
        );
    }

    // Rule 2 — no file write OR matching expected hash.
    if !ctx.file_write_attempted {
        passing.push("rule:no_file_write:artifact did not touch the filesystem".to_string());
    } else if !ctx.file_write_succeeded {
        blocking.push(
            "rule:file_write_unsuccessful:file write was attempted but failed; refuse auto-approve until the on-disk SSOT lands"
                .to_string(),
        );
    } else {
        match (
            ctx.expected_file_sha256.as_deref(),
            ctx.actual_file_sha256.as_deref(),
        ) {
            (Some(expected), Some(actual)) if expected.eq_ignore_ascii_case(actual) => {
                passing.push(
                    "rule:file_hash_match:on-disk sha256 matches the expected value supplied by the caller"
                        .to_string(),
                );
            }
            (Some(expected), Some(actual)) => {
                blocking.push(format!(
                    "rule:file_hash_mismatch:expected sha256 `{}` but on-disk is `{}`",
                    short_hash(expected),
                    short_hash(actual)
                ));
            }
            (Some(_), None) => {
                blocking.push(
                    "rule:file_hash_unknown:caller supplied expected sha256 but writer did not surface an actual sha256"
                        .to_string(),
                );
            }
            (None, _) => {
                // No expected hash supplied → as long as the write
                // succeeded the rule passes (the caller can opt into
                // strict matching by supplying `expected_file_sha256`).
                passing.push(
                    "rule:file_write_succeeded:on-disk write landed (no expected sha256 supplied — strict matching disabled)"
                        .to_string(),
                );
            }
        }
    }

    // Rule 3 — protected source / target.
    if ctx.protected_source_or_target {
        blocking.push(
            "rule:protected_source_or_target:artifact involves a protected source or target; review-only policy applies"
                .to_string(),
        );
    } else {
        passing.push(
            "rule:no_protected_source_or_target:neither side is on the protected list".to_string(),
        );
    }

    // Rule 4 — additional caller-detected blockers (warnings, partial
    // status, unresolved conflicts surfaced upstream).
    for extra in &ctx.additional_blockers {
        let trimmed = extra.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("rule:") {
            blocking.push(trimmed.to_string());
        } else {
            blocking.push(format!("rule:additional_blocker:{}", trimmed));
        }
    }
    if ctx.additional_blockers.is_empty() {
        passing.push(
            "rule:no_additional_blockers:no caller-side warnings or partial-status conflicts on the response payload"
                .to_string(),
        );
    }

    // Rule 5 — explicit caller opt-in. The auto_safe policy itself IS
    // the opt-in; the rule is recorded so the audit trail is loud about
    // it. (The handler is responsible for not invoking the policy under
    // `Manual`.)
    passing
        .push("rule:caller_opt_in:caller explicitly supplied review_automation_policy".to_string());

    let suggestion = if blocking.is_empty() {
        ReviewDecision::Approved
    } else {
        // Soft degrade to NeedsChanges so suggest-only consumers see a
        // clear "rework" hint without us ever auto-rejecting.
        ReviewDecision::NeedsChanges
    };
    (suggestion, blocking, passing)
}

/// Truncate a hex digest to its leading 12 characters for log lines.
/// Empty / sub-12 inputs round-trip unchanged.
fn short_hash(s: &str) -> String {
    s.chars().take(12).collect()
}

/// Stamp the wave-18 / task 07 outcome onto the response payload. Pure
/// / no bus calls. Always called when the resolved policy is non-Manual
/// (the handler skips this for Manual to keep pre-wave-18 callers
/// byte-identical).
///
/// Mutates `payload` with:
///   * `review_automation_policy`     — resolved policy label
///   * `review_automation_status`     — outcome label
///   * `suggested_review_decision`    — `approved | rejected | needs_changes`
///                                       (omitted under `Manual`)
///   * `automation_reasons`           — array of `code:detail` strings
pub(crate) fn stamp_review_automation_payload(
    payload: &mut Value,
    outcome: &ReviewAutomationOutcome,
) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert(
        "review_automation_policy".to_string(),
        json!(outcome.policy.as_str()),
    );
    map.insert(
        "review_automation_status".to_string(),
        json!(outcome.status.as_str()),
    );
    if let Some(d) = outcome.suggested_decision {
        map.insert("suggested_review_decision".to_string(), json!(d.as_str()));
    }
    map.insert("automation_reasons".to_string(), json!(outcome.reasons));
}
