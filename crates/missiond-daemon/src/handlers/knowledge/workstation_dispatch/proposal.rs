use serde_json::{json, Value};

use crate::state::AppState;

// ── wave-21 / task 04 — autonomous workstation LLM proposal v0 ─────────
//
// Scope: when caller / PLAN.lisp / DAG node carry NO workstation hints at
// all (no objective, no scope, no owned files, no target_project, no
// requested_cwd, no dispatch_strategy hint, no `:workstation-dispatch`
// flag) AND `workstation_inference_mode="sonnet_suggest"` is set, we ask
// Sonnet to PROPOSE the four core workstation fields:
//
//   * `target`            — one of {mission_execution | mission_task_delegate
//                            | mission_flow_run}.
//   * `dispatch_strategy` — one of {resident-lisp | fresh-code-alignment |
//                            agent-team | mixed}.
//   * `objective`         — non-empty string (~one paragraph max).
//   * `scope`             — non-empty string (additional bounds / files).
//
// The proposals are validated, tagged with a `safety_status` (Safe /
// AmbiguousValue / UnsupportedTarget / InvalidStrategy), and surfaced on
// the response under `workstation_proposals[]`. They are NEVER auto-
// applied: the dispatch path stays exactly as the wave-15..20 deterministic
// engine resolved it. Operator reads the proposals and decides whether to
// re-issue with explicit args.
//
// Conservative invariants (never violated):
//   * Default mode `off` ⇒ byte-compatible with wave-15..20. No prompt
//     emission, no Sonnet call, no response augmentation.
//   * Sonnet unavailable ⇒ `WorkstationProposalStatus::Unavailable` with
//     a typed reason; we NEVER fall back to `claude -p` or prompt mode.
//   * Each proposal carries `applied=false` on the wire so observers can
//     `assert proposal.applied == false` without re-reading the contract.
//   * Auto-spawn boundary: this layer NEVER calls
//     `run_workstation_dispatch*` based on proposals. It is a SUGGESTION
//     surface only.
//   * DAG mode rejects sonnet_suggest at the plan.rs preflight (see
//     `refuse_workstation_inference_in_dag_mode`); v0 is single-node only.

/// Allowlisted workstation fields the LLM may propose. Mirrors the four
/// core knobs the wave-15 dispatcher consumes when no PLAN hints exist.
pub(crate) const WORKSTATION_PROPOSAL_FIELDS: &[&str] =
    &["target", "dispatch_strategy", "objective", "scope"];

/// Hard cap on proposals so a runaway model can't blow the response payload.
/// Four fields × one proposal each is the canonical case; cap at 6 leaves
/// headroom for an alternative/variant without unbounded growth.
pub(crate) const WORKSTATION_PROPOSAL_CAP: usize = 6;

/// Token budget for the Sonnet workstation-proposal call. Prompts are
/// compact (just a system contract + plan sexp + provenance), so 1024
/// tokens is plenty for four short proposals with justifications.
const SONNET_WORKSTATION_PROPOSAL_MAX_TOKENS: u32 = 1024;

/// Caller string surfaced to the LLM gateway logging. Distinct from
/// `plan_field_inference` (wave-20 / task 07) so observers can tell the
/// two passes apart on the trace surface.
pub(crate) const SONNET_WORKSTATION_PROPOSAL_CALLER: &str = "workstation_dispatch_proposal";

/// Hard-coded model id for the response surface. Mirrors the literal used
/// by `plan.rs::SONNET_COMPILER_MODEL`; we keep a local copy rather than
/// re-export so the workstation layer stays free of plan internals.
const SONNET_WORKSTATION_PROPOSAL_MODEL: &str = "claude-sonnet";

/// Allowlisted target values. Mirrors the wave-15 plan-runner whitelist.
pub(crate) const PROPOSAL_VALID_TARGETS: &[&str] = &[
    "mission_execution",
    "mission_task_delegate",
    "mission_flow_run",
];

/// Allowlisted dispatch strategies. Subset of `INFERABLE_DISPATCH_STRATEGIES`
/// — `prompt-fallback` and `unknown` are deliberately NOT proposable
/// because the conservative spawn surface refuses them.
pub(crate) const PROPOSAL_VALID_STRATEGIES: &[&str] = &[
    "resident-lisp",
    "fresh-code-alignment",
    "agent-team",
    "mixed",
];

/// Wire status describing the outcome of the workstation-proposal pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationProposalStatus {
    /// Caller picked a non-LLM mode (`off` or absent); the bundle is absent.
    #[allow(dead_code)]
    NotInvoked,
    /// LLM was unavailable (gateway not initialised, network failure, etc.).
    /// Bundle carries a reason; no proposals.
    Unavailable,
    /// LLM responded with at least one valid proposal.
    Suggested,
    /// LLM responded but no proposal survived validation (zero usable
    /// fields). Bundle may carry parse_warnings to explain why.
    NoSuggestions,
    /// Caller / PLAN already supplied workstation hints, so the proposal
    /// pass was skipped (we never override existing signal). The bundle
    /// reports this so the response surface stays uniform.
    PlanHintsPresent,
}

impl WorkstationProposalStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationProposalStatus::NotInvoked => "not_invoked",
            WorkstationProposalStatus::Unavailable => "llm_unavailable",
            WorkstationProposalStatus::Suggested => "suggested",
            WorkstationProposalStatus::NoSuggestions => "no_suggestions",
            WorkstationProposalStatus::PlanHintsPresent => "plan_hints_present",
        }
    }
}

/// Per-proposal safety classification. Pure annotation — never blocks the
/// proposal from being surfaced (the operator decides). `Safe` means the
/// proposed value passes the wave-15 allowlist for that field; the other
/// variants explain WHY the operator should think twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationProposalSafetyStatus {
    /// Value passes the wave-15 allowlist for this field.
    Safe,
    /// Value is non-empty but ambiguous (e.g. objective shorter than the
    /// minimum useful length, or scope that looks like a placeholder).
    AmbiguousValue,
    /// `target` value is not in the wave-15 whitelist.
    UnsupportedTarget,
    /// `dispatch_strategy` value is not in the conservative subset.
    InvalidStrategy,
}

impl WorkstationProposalSafetyStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationProposalSafetyStatus::Safe => "safe",
            WorkstationProposalSafetyStatus::AmbiguousValue => "ambiguous_value",
            WorkstationProposalSafetyStatus::UnsupportedTarget => "unsupported_target",
            WorkstationProposalSafetyStatus::InvalidStrategy => "invalid_strategy",
        }
    }
}

/// Confidence vocabulary mirroring wave-20 / task 07. Local copy so the
/// workstation layer stays decoupled from plan-internal types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationProposalConfidence {
    High,
    Medium,
    Low,
}

impl WorkstationProposalConfidence {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationProposalConfidence::High => "high",
            WorkstationProposalConfidence::Medium => "medium",
            WorkstationProposalConfidence::Low => "low",
        }
    }
}

/// One validated workstation proposal. The `field` is interned to a static
/// allowlist string so downstream consumers can switch on it cheaply.
#[derive(Debug, Clone)]
pub(crate) struct WorkstationProposal {
    pub field: &'static str,
    pub value: Value,
    pub confidence: WorkstationProposalConfidence,
    pub evidence: String,
    pub safety_status: WorkstationProposalSafetyStatus,
}

impl WorkstationProposal {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "field": self.field,
            "value": self.value.clone(),
            "confidence": self.confidence.as_wire(),
            "evidence": self.evidence,
            "safety_status": self.safety_status.as_wire(),
            // Pin the never-applied invariant so observers can `assert
            // proposal.applied == false` without reading the source.
            // Wave-21 / task 04 explicitly forbids auto-spawn from
            // proposals; this field is the wire-level proof.
            "applied": false,
        })
    }
}

/// Bundle of LLM-side data for the workstation proposal pass. Always
/// carries the status (so observers see whether the gateway was reachable)
/// plus the validated proposals. `parse_warnings[]` records per-field
/// validation drops for caller debugging without aborting the response.
#[derive(Debug, Clone)]
pub(crate) struct WorkstationProposalBundle {
    pub status: WorkstationProposalStatus,
    pub proposals: Vec<WorkstationProposal>,
    pub parse_warnings: Vec<String>,
    pub unavailable_reason: Option<String>,
    pub model: Option<String>,
    pub request_caller: Option<String>,
}

impl WorkstationProposalBundle {
    /// Construct an `Unavailable` bundle with a typed reason. Used when
    /// the Sonnet gateway is not initialised or the call failed — we
    /// NEVER silently fall back to deterministic-only or prompt mode.
    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        WorkstationProposalBundle {
            status: WorkstationProposalStatus::Unavailable,
            proposals: Vec::new(),
            parse_warnings: Vec::new(),
            unavailable_reason: Some(reason.into()),
            model: None,
            request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
        }
    }

    /// Construct a `PlanHintsPresent` bundle. Surfaced so the response
    /// shape stays uniform when caller / PLAN already supplied signals
    /// and the proposal pass was therefore skipped.
    pub(crate) fn plan_hints_present(reason: impl Into<String>) -> Self {
        WorkstationProposalBundle {
            status: WorkstationProposalStatus::PlanHintsPresent,
            proposals: Vec::new(),
            parse_warnings: Vec::new(),
            unavailable_reason: Some(reason.into()),
            model: None,
            request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
        }
    }

    /// Build the JSON block surfaced under `workstation_proposals` on the
    /// response. Always carries every list (empty when nothing fired) so
    /// observers can pivot on a stable shape.
    pub(crate) fn to_response_json(&self) -> Value {
        let proposals: Vec<Value> = self.proposals.iter().map(|p| p.to_json()).collect();
        let mut block = json!({
            "status": self.status.as_wire(),
            "proposals": proposals,
            "parse_warnings": self.parse_warnings.clone(),
            // Pin the "never auto-spawn" invariant on the bundle level
            // too so a UI surfacing the block can quote the field
            // directly without re-deriving it.
            "auto_spawn": false,
        });
        if let Some(reason) = self.unavailable_reason.as_deref() {
            block["unavailable_reason"] = json!(reason);
        }
        if let Some(model) = self.model.as_deref() {
            block["model"] = json!(model);
        }
        if let Some(caller) = self.request_caller.as_deref() {
            block["caller"] = json!(caller);
        }
        block
    }
}

/// Inputs feeding the "are caller / PLAN hints absent?" predicate. Pure
/// projection of the merged hints + caller args so the gate function can
/// stay testable without standing up a full execute pipeline.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkstationProposalGate<'a> {
    /// True when caller passed `target=...` explicitly.
    pub caller_target_present: bool,
    /// True when caller passed `dispatch_strategy=...` explicitly.
    pub caller_dispatch_strategy_present: bool,
    /// True when caller passed `objective=...` explicitly.
    pub caller_objective_present: bool,
    /// True when caller passed `scope=...` explicitly.
    pub caller_scope_present: bool,
    /// True when caller passed `owned_files=...` (any non-empty list).
    pub caller_owned_files_present: bool,
    /// True when caller passed `target_project=...` or `requested_cwd=...`.
    pub caller_project_signal_present: bool,
    /// True when PLAN.lisp surfaced `:objective` / `:summary` / `:scope` /
    /// any of the workstation knobs the wave-15 hints parser consumes.
    pub plan_hints_present: bool,
    /// True when PLAN.lisp / node carried `:workstation-dispatch true`.
    pub plan_workstation_opt_in: bool,
    /// Phantom binding so the lifetime is non-trivial — matches the
    /// caller side that holds &str refs into args / hints structs.
    pub _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> WorkstationProposalGate<'a> {
    /// True iff every signal source is silent — caller passed no relevant
    /// args AND PLAN.lisp surfaced no relevant hints AND PLAN did not opt
    /// into workstation dispatch.
    pub(crate) fn is_fully_silent(&self) -> bool {
        !self.caller_target_present
            && !self.caller_dispatch_strategy_present
            && !self.caller_objective_present
            && !self.caller_scope_present
            && !self.caller_owned_files_present
            && !self.caller_project_signal_present
            && !self.plan_hints_present
            && !self.plan_workstation_opt_in
    }

    /// Human-readable reason naming which signals fired. Surfaced as the
    /// `unavailable_reason` of a `PlanHintsPresent` bundle so the operator
    /// can see exactly which slot triggered the skip.
    pub(crate) fn signal_summary(&self) -> String {
        let mut hits: Vec<&'static str> = Vec::new();
        if self.caller_target_present {
            hits.push("caller.target");
        }
        if self.caller_dispatch_strategy_present {
            hits.push("caller.dispatch_strategy");
        }
        if self.caller_objective_present {
            hits.push("caller.objective");
        }
        if self.caller_scope_present {
            hits.push("caller.scope");
        }
        if self.caller_owned_files_present {
            hits.push("caller.owned_files");
        }
        if self.caller_project_signal_present {
            hits.push("caller.project_signal");
        }
        if self.plan_hints_present {
            hits.push("plan.hints");
        }
        if self.plan_workstation_opt_in {
            hits.push("plan.workstation_dispatch");
        }
        if hits.is_empty() {
            "no signals present".to_string()
        } else {
            format!("signals present: {}", hits.join(", "))
        }
    }
}

/// Compose the system + user prompts for the Sonnet workstation-proposal
/// call. Pure function so the unit tests can lock the prompt shape.
///
/// The system prompt pins:
///   * the four allowlisted fields,
///   * the strict JSON schema,
///   * the never-auto-spawn invariant (the model is told its proposals
///     will be SURFACED ONLY, not executed),
///   * the conservative target / dispatch-strategy allowlists.
///
/// The user prompt embeds the PLAN sexp + the directive provenance string
/// so the model has enough context to ground its proposals.
pub(crate) fn build_workstation_proposal_prompt(
    plan_sexp: &str,
    compiled_from: Option<&str>,
) -> (String, String) {
    let system = String::from(
        "You are MissionD's autonomous workstation proposal assistant. Inspect the supplied \
         PLAN.lisp sexp and the directive provenance string. The PLAN does NOT carry any \
         workstation dispatch hints; the operator wants you to PROPOSE values for the four \
         core workstation fields:\n\n\
         - `target`            — one of: mission_execution | mission_task_delegate | mission_flow_run.\n\
         - `dispatch_strategy` — one of: resident-lisp | fresh-code-alignment | agent-team | mixed.\n\
         - `objective`         — concise non-empty string (one paragraph max) describing the work.\n\
         - `scope`             — concise non-empty string declaring additional bounds or files.\n\n\
         Reply with STRICT JSON ONLY (no Markdown fences, no prose) matching this shape:\n\
         {\n  \"proposals\": [\n    {\n      \"field\": \"<one of the four fields>\",\n      \"value\": <string>,\n      \"confidence\": \"high\"|\"medium\"|\"low\",\n      \"evidence\": \"<one short sentence justifying the proposal>\"\n    }\n  ]\n}\n\n\
         Rules:\n\
         - The proposals will be SURFACED to the operator for review and will NEVER be auto-\
           applied or auto-spawn a workstation. There is no execution side effect — your job is \
           to suggest a starting point, not to dispatch.\n\
         - Omit a field rather than fabricate one. An empty proposals array is a valid response.\n\
         - `target` must be in the listed whitelist. Anything else will be tagged `unsupported_target`.\n\
         - `dispatch_strategy` must be in the listed whitelist. `prompt-fallback` and `unknown` \
           are deliberately excluded — never propose them.\n\
         - Confidence `high` is reserved for unambiguous evidence. When in doubt, use `medium` or omit.\n\
         - Never include keys outside the listed schema.",
    );
    let user = format!(
        "PLAN.lisp sexp:\n```lisp\n{plan}\n```\n\ncompiled_from: {compiled}\n\nThe caller passed no \
         workstation hints (no objective, no scope, no owned files, no target_project, no requested_cwd, \
         no `:workstation-dispatch` flag). Propose values for any of the four fields you can ground \
         in the PLAN sexp / provenance.",
        plan = plan_sexp,
        compiled = compiled_from.unwrap_or("(none)"),
    );
    (system, user)
}

/// Validate a Sonnet response into a list of [`WorkstationProposal`]
/// entries. Accepts `{"proposals": [{...}, ...]}` (canonical) OR a bare
/// top-level array (model sometimes elides the wrapper). Rejected
/// proposals land on `parse_warnings[]` so the caller can audit what
/// survived. Pure function — no IO.
pub(crate) fn parse_workstation_proposals(raw: &str) -> (Vec<WorkstationProposal>, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let trimmed = raw.trim();
    let trimmed = strip_proposal_code_fence(trimmed);
    let parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(err) => {
            warnings.push(format!("LLM response was not valid JSON: {}", err));
            return (Vec::new(), warnings);
        }
    };
    let raw_proposals: Vec<Value> = match &parsed {
        Value::Array(arr) => arr.clone(),
        Value::Object(map) => match map.get("proposals") {
            Some(Value::Array(arr)) => arr.clone(),
            Some(other) => {
                warnings.push(format!(
                    "`proposals` must be an array, got {}",
                    proposal_json_kind(other)
                ));
                return (Vec::new(), warnings);
            }
            None => {
                warnings.push("LLM response object missing required `proposals` array".to_string());
                return (Vec::new(), warnings);
            }
        },
        other => {
            warnings.push(format!(
                "LLM response top-level must be array or object, got {}",
                proposal_json_kind(other)
            ));
            return (Vec::new(), warnings);
        }
    };
    let mut out: Vec<WorkstationProposal> = Vec::new();
    let mut seen_fields: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    for (idx, raw) in raw_proposals.iter().enumerate() {
        if out.len() >= WORKSTATION_PROPOSAL_CAP {
            warnings.push(format!(
                "proposal cap of {} reached; dropping remaining entries",
                WORKSTATION_PROPOSAL_CAP
            ));
            break;
        }
        let obj = match raw.as_object() {
            Some(o) => o,
            None => {
                warnings.push(format!(
                    "proposals[{}] must be an object, got {}",
                    idx,
                    proposal_json_kind(raw)
                ));
                continue;
            }
        };
        let field_raw = obj
            .get("field")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .unwrap_or("");
        let field = match WORKSTATION_PROPOSAL_FIELDS
            .iter()
            .find(|allowed| allowed.eq_ignore_ascii_case(field_raw))
            .copied()
        {
            Some(f) => f,
            None => {
                warnings.push(format!(
                    "proposals[{}] field `{}` not in allowlist",
                    idx, field_raw
                ));
                continue;
            }
        };
        if seen_fields.contains(field) {
            warnings.push(format!(
                "proposals[{}] duplicate field `{}` ignored",
                idx, field
            ));
            continue;
        }
        let value_raw = match obj.get("value") {
            Some(v) => v.clone(),
            None => {
                warnings.push(format!("proposals[{}] missing required `value`", idx));
                continue;
            }
        };
        // All four fields are string-shaped in v0; reject non-string
        // values rather than coerce silently.
        let value_str = match value_raw.as_str() {
            Some(s) => s.trim().to_string(),
            None => {
                warnings.push(format!(
                    "proposals[{}] value for `{}` must be string, got {}",
                    idx,
                    field,
                    proposal_json_kind(&value_raw)
                ));
                continue;
            }
        };
        if value_str.is_empty() {
            warnings.push(format!(
                "proposals[{}] value for `{}` must be non-empty string",
                idx, field
            ));
            continue;
        }
        let confidence = match obj.get("confidence").and_then(|v| v.as_str()) {
            Some(s) => match s.trim().to_ascii_lowercase().as_str() {
                "high" => WorkstationProposalConfidence::High,
                "medium" => WorkstationProposalConfidence::Medium,
                "low" => WorkstationProposalConfidence::Low,
                other => {
                    warnings.push(format!(
                        "proposals[{}] confidence `{}` not in [high, medium, low]",
                        idx, other
                    ));
                    continue;
                }
            },
            None => {
                warnings.push(format!("proposals[{}] missing required `confidence`", idx));
                continue;
            }
        };
        let evidence = obj
            .get("evidence")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if evidence.is_empty() {
            warnings.push(format!(
                "proposals[{}] missing required `evidence` justification",
                idx
            ));
            continue;
        }
        let safety_status = classify_proposal_safety(field, &value_str);
        seen_fields.insert(field);
        out.push(WorkstationProposal {
            field,
            value: json!(value_str),
            confidence,
            evidence,
            safety_status,
        });
    }
    (out, warnings)
}

/// Classify the safety status of a proposed value against the wave-15
/// allowlists. Pure function — no IO. Never blocks the proposal from
/// being surfaced; it just annotates so the operator can pivot on it.
///
/// Rules:
///   * `target` not in `PROPOSAL_VALID_TARGETS`           → `UnsupportedTarget`.
///   * `dispatch_strategy` not in `PROPOSAL_VALID_STRATEGIES` → `InvalidStrategy`.
///   * `objective` shorter than 8 characters              → `AmbiguousValue`.
///   * `scope` shorter than 4 characters                  → `AmbiguousValue`.
///   * Otherwise                                          → `Safe`.
pub(crate) fn classify_proposal_safety(
    field: &str,
    value: &str,
) -> WorkstationProposalSafetyStatus {
    let trimmed = value.trim();
    match field {
        "target" => {
            if PROPOSAL_VALID_TARGETS
                .iter()
                .any(|t| t.eq_ignore_ascii_case(trimmed))
            {
                WorkstationProposalSafetyStatus::Safe
            } else {
                WorkstationProposalSafetyStatus::UnsupportedTarget
            }
        }
        "dispatch_strategy" => {
            if PROPOSAL_VALID_STRATEGIES
                .iter()
                .any(|s| s.eq_ignore_ascii_case(trimmed))
            {
                WorkstationProposalSafetyStatus::Safe
            } else {
                WorkstationProposalSafetyStatus::InvalidStrategy
            }
        }
        "objective" => {
            if trimmed.chars().count() < 8 {
                WorkstationProposalSafetyStatus::AmbiguousValue
            } else {
                WorkstationProposalSafetyStatus::Safe
            }
        }
        "scope" => {
            if trimmed.chars().count() < 4 {
                WorkstationProposalSafetyStatus::AmbiguousValue
            } else {
                WorkstationProposalSafetyStatus::Safe
            }
        }
        // Defensive default: unknown fields cannot be reached because
        // the allowlist filter runs before this function.
        _ => WorkstationProposalSafetyStatus::AmbiguousValue,
    }
}

/// Strip a Markdown code fence (```json ... ``` or ``` ... ```) if the
/// model wrapped its JSON output. Local copy of the strip helper used by
/// the wave-20 / task 07 plan-field surface; we duplicate it so the
/// workstation layer stays decoupled from plan internals.
fn strip_proposal_code_fence(s: &str) -> &str {
    let s = s.trim();
    let stripped = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```JSON"))
        .or_else(|| s.strip_prefix("```"));
    let Some(rest) = stripped else {
        return s;
    };
    let rest = rest.trim_start_matches('\n');
    let rest = rest.strip_suffix("```").unwrap_or(rest);
    rest.trim()
}

/// Short json kind name for diagnostics.
pub(super) fn proposal_json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Run the Sonnet workstation-proposal call. Returns a
/// [`WorkstationProposalBundle`] in every code path so the caller can
/// pivot on the bundle status without branching on `Result`.
///
/// Sonnet unavailability surfaces as `WorkstationProposalStatus::Unavailable`
/// with an explanatory reason — NEVER as a silent fallback to prompt mode
/// or `claude -p`. This mirrors the wave-20 / task 07 invariant for the
/// plan-field surface.
///
/// This function NEVER spawns a workstation. It is a SUGGESTION pass only.
pub(crate) async fn request_workstation_proposals(
    state: &AppState,
    plan_sexp: &str,
    compiled_from: Option<&str>,
) -> WorkstationProposalBundle {
    let Some(sonnet) = state.sonnet.as_ref() else {
        return WorkstationProposalBundle::unavailable(
            "Sonnet gateway not initialized; autonomous workstation proposal unavailable \
             (no fallback to claude -p / prompt mode in v0)",
        );
    };
    let (system, user) = build_workstation_proposal_prompt(plan_sexp, compiled_from);
    let messages = vec![
        crate::minimax_client::ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        crate::minimax_client::ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ];
    let raw = match sonnet
        .call_interactive(
            messages,
            Some(SONNET_WORKSTATION_PROPOSAL_MAX_TOKENS),
            SONNET_WORKSTATION_PROPOSAL_CALLER,
        )
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return WorkstationProposalBundle::unavailable(format!(
                "Sonnet workstation-proposal call failed: {} \
                 (no fallback to claude -p / prompt mode in v0)",
                err
            ));
        }
    };
    let (proposals, parse_warnings) = parse_workstation_proposals(&raw);
    let status = if proposals.is_empty() {
        WorkstationProposalStatus::NoSuggestions
    } else {
        WorkstationProposalStatus::Suggested
    };
    WorkstationProposalBundle {
        status,
        proposals,
        parse_warnings,
        unavailable_reason: None,
        model: Some(SONNET_WORKSTATION_PROPOSAL_MODEL.to_string()),
        request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
    }
}
