use serde_json::{json, Value};

use super::super::auto_answer::is_destructive_review_action;
use super::super::resolution::ReviewDecision;

// V3 invariant anchor: review LLM proposal mode must never auto-approve.

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
            warnings.push("confidence missing or unrecognised; defaulting to `low`".to_string());
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
pub(in crate::handlers::knowledge::review_gate) fn strip_proposal_code_fence(raw: &str) -> &str {
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
pub(in crate::handlers::knowledge::review_gate) fn proposal_json_kind(v: &Value) -> &'static str {
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
        summary =
            serde_json::to_string_pretty(deterministic_summary).unwrap_or_else(|_| "{}".into()),
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
        map.insert("llm_auto_approve_proposal".to_string(), p.to_json());
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
        map.insert("llm_auto_approve_proposal_model".to_string(), json!(model));
    }
}
