use missiond_core::event::events::QuestionEvent;
use serde_json::{json, Value};
use tracing::warn;

use crate::bus::BusServices;

// ───────────────────────────────────────────────────────────────────────
// Decision-time review gate (Resolved / DecisionResolved)
//
// approve / archive / mark / supersede call this when the caller passes
// `review_question_id`. The handler always succeeds first (DB mutation)
// and then attempts the publish. We never block the DB outcome on a bus
// success.
// ───────────────────────────────────────────────────────────────────────

/// Parse a single `review_question_id` field for the resolution path.
/// Returns `None` when absent / blank — the resolution emit is opt-in.
pub(crate) fn parse_resolution_review_question_id(args: &Value) -> Option<String> {
    args.get("review_question_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Optional decision metadata for `QuestionEvent::DecisionResolved`. When
/// `tier` is provided, the helper publishes `DecisionResolved` instead of
/// `Resolved` so the upstream router can attribute the decision tier.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolutionDecisionMeta {
    pub(crate) tier: Option<String>,
    pub(crate) duration_ms: Option<u64>,
}

/// Pure constructor: build the `QuestionEvent` that the resolution helper
/// would emit for a given `(qid, resolution, decision_meta)` triple. Split
/// out so tests can assert event shape without touching a real bus.
pub(crate) fn build_resolution_event(
    qid: &str,
    resolution: &str,
    decision: Option<&ResolutionDecisionMeta>,
) -> QuestionEvent {
    match decision.and_then(|d| d.tier.as_deref()) {
        Some(tier) => QuestionEvent::DecisionResolved {
            question_id: qid.to_string(),
            tier: tier.to_string(),
            duration_ms: decision.and_then(|d| d.duration_ms).unwrap_or(0),
        },
        None => QuestionEvent::Resolved {
            question_id: qid.to_string(),
            resolution: resolution.to_string(),
        },
    }
}

/// Pure event-kind label for response payload. Mirrors the
/// `DomainEvent::kind` impl on `QuestionEvent` but is callable from a
/// borrowed reference without touching the trait.
pub(super) fn event_kind_label(ev: &QuestionEvent) -> &'static str {
    match ev {
        QuestionEvent::DecisionResolved { .. } => "decision_resolved",
        QuestionEvent::Resolved { .. } => "resolved",
        QuestionEvent::Created { .. } => "created",
    }
}

/// Best-effort `QuestionEvent::Resolved` (or `DecisionResolved` when
/// `decision.tier.is_some()`) emit after a control action (approve /
/// archive / mark / supersede) committed.
///
/// Mutates `payload`:
///   * `review_question_resolved` (bool) — true on success
///   * `review_question_id`       (string) — echoed back so the caller can
///     correlate with the original Created event
///   * `review_question_warning`  (object) — only when publish errored
///
/// Resolution is OPT-IN at the caller. When `qid.is_none()`, this helper is
/// a no-op (no payload mutation) so legacy callers that never knew about
/// the gate stay byte-identical.
pub(crate) async fn maybe_emit_review_question_resolved(
    payload: &mut Value,
    bus: &BusServices,
    qid: Option<&str>,
    resolution: &str,
    decision: Option<&ResolutionDecisionMeta>,
) {
    let Some(qid) = qid else {
        return;
    };
    let qid = qid.to_string();

    let ev = build_resolution_event(&qid, resolution, decision);
    let kind = event_kind_label(&ev);

    match bus.publish_question(ev).await {
        Ok(_) => {
            payload["review_question_resolved"] = json!(true);
            payload["review_question_id"] = json!(qid);
            payload["review_question_kind"] = json!(kind);
        }
        Err(e) => {
            warn!(
                question_id = %qid,
                resolution = resolution,
                kind = kind,
                error = %e,
                "review-gate: QuestionEvent resolved publish failed; DB action already committed"
            );
            payload["review_question_resolved"] = json!(false);
            payload["review_question_id"] = json!(qid);
            payload["review_question_kind"] = json!(kind);
            payload["review_question_warning"] = json!({
                "code": "BUS_PUBLISH_FAILED",
                "reason": format!("{:#}", e),
                "question_id": qid,
                "resolution": resolution,
                "kind": kind,
            });
        }
    }
}

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
    pub(super) fn parse(raw: &str) -> Result<Self, ResolutionInputError> {
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

/// Parsed envelope of a wave-14 deterministic review-question id. Layout:
///   `review:<scope>:<artifact_id>:v<version>:<action>[:<topic-hash>]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedReviewQuestionId {
    pub(crate) scope: String,
    pub(crate) artifact_id: String,
    pub(crate) version: i32,
    pub(crate) action: String,
    /// Optional 16-hex-char topic hash. Wave-14 layout suffix; wave-11
    /// layout has no suffix and this field stays None.
    pub(crate) topic_hash: Option<String>,
}

/// Errors returned by [`parse_review_question_id_struct`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewIdParseError {
    /// id does not start with the literal `review:` prefix.
    MissingPrefix,
    /// id is missing one of the mandatory `<scope>:<id>:v<version>:<action>`
    /// segments.
    InsufficientSegments,
    /// Mandatory segment is empty (e.g. `review::abc:v1:compile`).
    EmptySegment(&'static str),
    /// `v<version>` segment is malformed (missing `v` prefix or non-numeric
    /// version).
    BadVersion(String),
}

impl ReviewIdParseError {
    pub(crate) fn message(&self) -> String {
        match self {
            ReviewIdParseError::MissingPrefix => {
                "review_question_id must start with `review:` prefix".to_string()
            }
            ReviewIdParseError::InsufficientSegments => {
                "review_question_id must look like `review:<scope>:<id>:v<version>:<action>[:<topic-hash>]`"
                    .to_string()
            }
            ReviewIdParseError::EmptySegment(seg) => {
                format!("review_question_id has empty `{}` segment", seg)
            }
            ReviewIdParseError::BadVersion(raw) => format!(
                "review_question_id version segment `{}` must be `v<int>` (e.g. `v1`)",
                raw
            ),
        }
    }
}

/// Parse a wave-14 deterministic review-question id back into its parts.
/// Pure / side-effect free; does not consult the DB.
///
/// Recognised shapes:
///   `review:<scope>:<artifact_id>:v<version>:<action>`
///   `review:<scope>:<artifact_id>:v<version>:<action>:<topic-hash>`
pub(crate) fn parse_review_question_id_struct(
    qid: &str,
) -> Result<ParsedReviewQuestionId, ReviewIdParseError> {
    let trimmed = qid.trim();
    let body = trimmed
        .strip_prefix("review:")
        .ok_or(ReviewIdParseError::MissingPrefix)?;
    let segs: Vec<&str> = body.split(':').collect();
    if segs.len() < 4 {
        return Err(ReviewIdParseError::InsufficientSegments);
    }
    if segs.len() > 5 {
        // We don't accept `v<v>:<action>:<hash>:<extra>` — extra colons inside
        // the action / hash were never produced by wave-14.
        return Err(ReviewIdParseError::InsufficientSegments);
    }
    let scope = segs[0].trim();
    if scope.is_empty() {
        return Err(ReviewIdParseError::EmptySegment("scope"));
    }
    let artifact_id = segs[1].trim();
    if artifact_id.is_empty() {
        return Err(ReviewIdParseError::EmptySegment("artifact_id"));
    }
    let version_seg = segs[2].trim();
    let version_num = version_seg
        .strip_prefix('v')
        .ok_or_else(|| ReviewIdParseError::BadVersion(version_seg.to_string()))?;
    let version: i32 = version_num
        .parse()
        .map_err(|_| ReviewIdParseError::BadVersion(version_seg.to_string()))?;
    let action = segs[3].trim();
    if action.is_empty() {
        return Err(ReviewIdParseError::EmptySegment("action"));
    }
    let topic_hash = if segs.len() == 5 {
        let hash = segs[4].trim();
        if hash.is_empty() {
            return Err(ReviewIdParseError::EmptySegment("topic_hash"));
        }
        Some(hash.to_string())
    } else {
        None
    };
    Ok(ParsedReviewQuestionId {
        scope: scope.to_string(),
        artifact_id: artifact_id.to_string(),
        version,
        action: action.to_ascii_lowercase(),
        topic_hash,
    })
}

/// Errors returned by [`validate_review_resolution_envelope`]. Each
/// variant maps to an MCP `ToolError` code via [`Self::code`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolutionValidationError {
    /// The parsed id's scope does not match the manager surface (e.g. a
    /// `directive` id submitted to the plan handler).
    ScopeMismatch {
        expected: &'static str,
        actual: String,
    },
    /// The parsed id's artifact_id does not match the action's artifact
    /// (e.g. `directive_id=abc` but qid encodes `xyz`).
    ArtifactIdMismatch { expected: String, actual: String },
    /// The parsed id's version does not match the artifact's current
    /// version (or the version the caller passed for transition).
    StaleVersion {
        expected: i32,
        actual_in_id: i32,
        artifact_id: String,
    },
    /// The parsed id's action is not in the action whitelist for this
    /// manager surface (e.g. `:supersede` arrived at the directive
    /// handler).
    UnsupportedAction {
        scope: &'static str,
        action: String,
        allowed: &'static [&'static str],
    },
    /// The parsed id's scope is not one of the wave-14 surfaces (i.e. not
    /// directive/plan/workflow). Defensive — shouldn't happen for ids we
    /// produced, but keeps malformed third-party ids loud.
    UnsupportedScope { scope: String },
}

impl ResolutionValidationError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            ResolutionValidationError::StaleVersion { .. } => "STALE_REVIEW_VERSION",
            ResolutionValidationError::ScopeMismatch { .. } => "REVIEW_SCOPE_MISMATCH",
            ResolutionValidationError::ArtifactIdMismatch { .. } => "REVIEW_ARTIFACT_MISMATCH",
            ResolutionValidationError::UnsupportedAction { .. } => "REVIEW_ACTION_UNSUPPORTED",
            ResolutionValidationError::UnsupportedScope { .. } => "REVIEW_SCOPE_UNSUPPORTED",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            ResolutionValidationError::ScopeMismatch { expected, actual } => format!(
                "review_question_id scope `{}` does not match manager surface `{}`",
                actual, expected
            ),
            ResolutionValidationError::ArtifactIdMismatch { expected, actual } => format!(
                "review_question_id artifact `{}` does not match request artifact `{}`",
                actual, expected
            ),
            ResolutionValidationError::StaleVersion {
                expected,
                actual_in_id,
                artifact_id,
            } => format!(
                "review_question_id targets version `v{}` but artifact `{}` is at version `v{}`",
                actual_in_id, artifact_id, expected
            ),
            ResolutionValidationError::UnsupportedAction {
                scope,
                action,
                allowed,
            } => format!(
                "review_question_id action `{}` is not allowed on scope `{}` (valid: {})",
                action,
                scope,
                allowed.join("|")
            ),
            ResolutionValidationError::UnsupportedScope { scope } => format!(
                "review_question_id scope `{}` is not supported (valid: directive|plan|workflow)",
                scope
            ),
        }
    }
}

/// All wave-14 surfaces that we accept review-question ids for. Defensive
/// check — wave-15 handler-side validators repeat the per-surface match
/// (so they can pin the action whitelist), but this guards against
/// third-party ids whose scope is not one we ever produced.
const WAVE14_SUPPORTED_SCOPES: &[&str] = &["directive", "plan", "workflow"];

/// Validate a parsed review-question id against the manager surface that
/// received it. Pure / side-effect free; does not consult the DB. The
/// caller is responsible for sourcing `current_artifact_version` (e.g.
/// from `directive_get_version_chain` head) before invoking this.
///
/// `allowed_actions` is the manager-side action whitelist (e.g.
/// `&["compile", "approve", "archive"]` for the directive surface). The
/// id's `action` must be in this list — we refuse to resolve a directive
/// surface against a `:supersede` qid even if other validators pass.
pub(crate) fn validate_review_resolution_envelope(
    parsed: &ParsedReviewQuestionId,
    expected_scope: &'static str,
    expected_artifact_id: &str,
    current_artifact_version: i32,
    allowed_actions: &'static [&'static str],
) -> Result<(), ResolutionValidationError> {
    if !WAVE14_SUPPORTED_SCOPES.contains(&parsed.scope.as_str()) {
        return Err(ResolutionValidationError::UnsupportedScope {
            scope: parsed.scope.clone(),
        });
    }
    if parsed.scope != expected_scope {
        return Err(ResolutionValidationError::ScopeMismatch {
            expected: expected_scope,
            actual: parsed.scope.clone(),
        });
    }
    if parsed.artifact_id != expected_artifact_id {
        return Err(ResolutionValidationError::ArtifactIdMismatch {
            expected: expected_artifact_id.to_string(),
            actual: parsed.artifact_id.clone(),
        });
    }
    if parsed.version != current_artifact_version {
        return Err(ResolutionValidationError::StaleVersion {
            expected: current_artifact_version,
            actual_in_id: parsed.version,
            artifact_id: parsed.artifact_id.clone(),
        });
    }
    if !allowed_actions.contains(&parsed.action.as_str()) {
        return Err(ResolutionValidationError::UnsupportedAction {
            scope: expected_scope,
            action: parsed.action.clone(),
            allowed: allowed_actions,
        });
    }
    Ok(())
}

/// What the manager surface should DO after the resolution input passes
/// validation. Pure projection of `decision`:
///
///   * `Approved`     → `PerformTransition` — caller runs its existing
///                       state transition (e.g. `directive_approve`),
///                       then emits `Resolved/approved`.
///   * `Rejected`     → `KeepArtifact` — caller skips the transition,
///                       leaves the artifact non-approved, emits
///                       `Resolved/rejected` with the actor / note as the
///                       reason.
///   * `NeedsChanges` → `RequestChanges` — caller skips the transition,
///                       leaves the artifact in the review/draft path,
///                       emits `Resolved/needs_changes`, surfaces a
///                       `next_step` recommendation in the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionOutcome {
    PerformTransition,
    KeepArtifact,
    RequestChanges,
}

impl ReviewDecision {
    pub(crate) fn outcome(self) -> ResolutionOutcome {
        match self {
            ReviewDecision::Approved => ResolutionOutcome::PerformTransition,
            ReviewDecision::Rejected => ResolutionOutcome::KeepArtifact,
            ReviewDecision::NeedsChanges => ResolutionOutcome::RequestChanges,
        }
    }
}

/// Stamp the resolution decision + actor + note onto the response
/// payload. Always called after the manager surface decides what to do
/// (whether or not the DB transition fired). Pure / no bus calls.
///
/// Mutates `payload` with:
///   * `review_question_id`        (string) — echoed back so callers can
///     correlate with the original Created event.
///   * `review_decision`           (string) — `approved|rejected|needs_changes`.
///   * `review_decision_outcome`   (string) — `perform_transition|keep_artifact|request_changes`.
///   * `review_actor`              (string) — when supplied.
///   * `review_note`               (string) — when supplied.
pub(crate) fn stamp_resolution_payload(payload: &mut Value, input: &ReviewResolutionInput) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert("review_question_id".to_string(), json!(input.question_id));
    map.insert(
        "review_decision".to_string(),
        json!(input.decision.as_str()),
    );
    map.insert(
        "review_decision_outcome".to_string(),
        json!(match input.decision.outcome() {
            ResolutionOutcome::PerformTransition => "perform_transition",
            ResolutionOutcome::KeepArtifact => "keep_artifact",
            ResolutionOutcome::RequestChanges => "request_changes",
        }),
    );
    if let Some(actor) = input.actor.as_ref() {
        map.insert("review_actor".to_string(), json!(actor));
    }
    if let Some(note) = input.note.as_ref() {
        map.insert("review_note".to_string(), json!(note));
    }
}

/// Stamp the response payload for a `RequestChanges` (needs_changes)
/// resolution: surface a default `next_step` so the caller knows what to
/// do next.
pub(crate) fn stamp_needs_changes_next_step(payload: &mut Value, scope: &str, action: &str) {
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "next_step".to_string(),
            json!(format!(
                "rework the {scope} draft per `review_note`, then re-run `mission_{scope}(action={action})` against the new version"
            )),
        );
    }
}

/// Build the structured event resolution string for a given decision.
/// Centralises the wire vocabulary so directive / plan / workflow speak
/// the same thing on the bus.
pub(crate) fn resolution_wire_string(decision: ReviewDecision) -> &'static str {
    decision.as_str()
}

// ───────────────────────────────────────────────────────────────────────
// wave-16 :: subscriber-side resolution dispatcher
//
// The wave-16 `QuestionEvent::Resolved` listener (see
// `crates/missiond-daemon/src/bus/v2_subscribers.rs::spawn_review_resolution_sub`)
// consumes Resolved events and routes deterministic `review:*` ids back
// through the same explicit-resolution validators each manager handler
// already owns. This block is the PURE planner the subscriber uses so the
// dispatch logic is testable without spinning a bus / DB.
//
// Conservatism contract:
//   * Non-review ids → `IgnoreNonReviewId`. We never auto-act on
//     decision-engine answers for non-review questions.
//   * Malformed `review:*` ids → `IgnoreMalformedId`. Surface the parse
//     error so observability sees a loud signal; do NOT mutate.
//   * Resolution string outside the recognised vocabulary →
//     `IgnoreUnknownResolution`. Wider than the strict `ReviewDecision::parse`
//     vocabulary so wave-14 emitters / decision-engine tier output that
//     uses synonyms (`yes` / `accepted` / `fix` / `revise` / etc.) still
//     resolves cleanly.
//   * Recognised review id + recognised resolution → `Route` carrying the
//     parsed envelope and decision. The subscriber applies the per-scope
//     side-effect (no double-publish; the inbound Resolved is the
//     downstream signal).
// ───────────────────────────────────────────────────────────────────────

/// Dispatch outcome for a `(question_id, resolution)` pair pulled off the
/// `QuestionEvent::Resolved` topic. Pure / side-effect free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewResolvedDispatch {
    /// `question_id` does not start with the `review:` prefix — wave-16
    /// subscriber ignores it (other consumers may handle it).
    IgnoreNonReviewId,
    /// `question_id` started with `review:` but the envelope was malformed
    /// (missing segments / bad version / empty segment / unsupported
    /// scope). Subscriber logs a warning and acks; never mutates.
    IgnoreMalformedId(ReviewIdParseError),
    /// `question_id` was a recognised wave-14 id but the envelope's scope
    /// is not one of `directive | plan | workflow`. Defensive against
    /// third-party emitters that reuse the `review:` prefix.
    IgnoreUnsupportedScope { scope: String },
    /// Resolution string did not map to any known decision after the
    /// conservative vocabulary expansion. Subscriber logs a warning and
    /// acks; never mutates.
    IgnoreUnknownResolution { resolution: String },
    /// Recognised review id + recognised resolution → subscriber may
    /// route through the per-scope handler (`directive::resolve_from_event`
    /// / `plan::resolve_from_event` / `workflow::resolve_from_event`).
    Route {
        parsed: ParsedReviewQuestionId,
        decision: ReviewDecision,
    },
}

/// Conservative resolution-string mapper for the subscriber path. WIDER
/// vocabulary than `ReviewDecision::parse` (which is the strict explicit
/// API):
///
///   * `approved | approve | yes | accepted` → `Approved`
///   * `rejected | reject | no`              → `Rejected`
///   * `needs_changes | needs-changes | changes | revise | fix`
///                                            → `NeedsChanges`
///   * anything else                          → `None` (caller logs a
///     warning and acks the bus message; no mutation)
///
/// Case-insensitive + trimmed. Returns `None` rather than `Err` because
/// the subscriber never fails — it just ignores unknown vocabulary.
pub(crate) fn parse_subscriber_resolution_string(raw: &str) -> Option<ReviewDecision> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "approved" | "approve" | "yes" | "accepted" => Some(ReviewDecision::Approved),
        "rejected" | "reject" | "no" => Some(ReviewDecision::Rejected),
        "needs_changes" | "needs-changes" | "changes" | "revise" | "fix" => {
            Some(ReviewDecision::NeedsChanges)
        }
        _ => None,
    }
}

/// Pure planner: decide what the `QuestionEvent::Resolved` subscriber
/// should do with a `(question_id, resolution)` pair. Does NOT touch DB
/// or bus.
pub(crate) fn plan_review_resolved_dispatch(
    question_id: &str,
    resolution: &str,
) -> ReviewResolvedDispatch {
    if !question_id.trim_start().starts_with("review:") {
        return ReviewResolvedDispatch::IgnoreNonReviewId;
    }
    let parsed = match parse_review_question_id_struct(question_id) {
        Ok(p) => p,
        Err(e) => return ReviewResolvedDispatch::IgnoreMalformedId(e),
    };
    if !WAVE14_SUPPORTED_SCOPES.contains(&parsed.scope.as_str()) {
        return ReviewResolvedDispatch::IgnoreUnsupportedScope {
            scope: parsed.scope.clone(),
        };
    }
    let decision = match parse_subscriber_resolution_string(resolution) {
        Some(d) => d,
        None => {
            return ReviewResolvedDispatch::IgnoreUnknownResolution {
                resolution: resolution.to_string(),
            }
        }
    };
    ReviewResolvedDispatch::Route { parsed, decision }
}
