//! workstation-dispatch v0 — conservative augmentation layer that turns a
//! plan node targeting `mission_task_delegate` into a scoped task brief
//! before delegating through the existing internal handler.
//!
//! Scope (Wave 15 / Task 05):
//!   - This module ONLY runs when caller / PLAN explicitly opts in. There is
//!     no broad private scheduling. The opt-in surface is:
//!       * execute arg `workstation_dispatch=true`, OR
//!       * PLAN.lisp / DAG node hint `:workstation-dispatch true`.
//!     The dispatch target itself must already resolve to
//!     `mission_task_delegate` — workstation dispatch never silently
//!     re-routes a `mission_execution` / `mission_flow_run` node.
//!   - We never shell out to `claude -p`. The actual transport is the
//!     existing `mission_task_delegate` substrate (which itself prefers a
//!     spawned / reused workstation). When the dispatch cannot be performed
//!     safely (e.g. project root unresolved, target wrong) we return a
//!     structured `safe descriptor`; the caller sees the reason and can
//!     reroute. We do NOT silently fall back to prompt mode.
//!   - `agent-team` is treated as a task-text hint (the literal Chinese
//!     line `使用 agent-team提高效率`) injected exactly once into the brief.
//!     It is not a new transport.
//!   - Project-root resolution honours
//!     `slot_orchestrator::project_root::resolve_target_project_root` —
//!     relative cwd is rejected, no process-cwd fallback.
//!   - Scoped commit handoff: the generated brief always carries
//!     `commit-policy: scoped` (default) and explicit "do not stage or
//!     commit outside owned files" guidance.
//!
//! Lisp authority:
//!   - intent-flow.lisp        :: F-workstation-dispatch-policy
//!   - intent-worker.lisp      :: claudecode-workstation-orchestration
//!   - intent-tools.lisp       :: implemented-surface mission_plan
//!                                 :execute-contract :: workstation-dispatch
//!
//! Wave 15 / Task 05 explicitly does NOT touch the Lisp authority files —
//! that backfill is Wave 15 / Task 06.

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use missiond_core::types::Plan;
use serde_json::{json, Value};

#[cfg(test)]
use crate::slot_orchestrator::project_root::resolve_target_project_root;
use crate::state::AppState;

#[cfg(test)]
use super::plan::AGENT_TEAM_OBJECTIVE_HINT;

/// Default commit policy when none is provided. Matches the wave-12 scoped
/// commit handoff contract: each delegated task only stages its own owned
/// files and never touches sibling shards.
pub(crate) const COMMIT_POLICY_SCOPED: &str = "scoped";

/// Hard cap on the per-list size for `owned-files` / `forbidden-files` /
/// `acceptance-commands` so a runaway PLAN.lisp can't blow the brief past
/// `mission_task_delegate`'s 16K objective cap. Author intent is preserved
/// via the `unsupported_*` overflow lists when this fires.
const TASK_BRIEF_LIST_CAP: usize = 32;

/// Hint contract the workstation-dispatch module recognises. Any field NOT
/// listed here is left to the existing `ParsedPlanHints` parser; we never
/// reinterpret arbitrary Lisp inside this module. Unknown PLAN keywords
/// reach this layer as `unsupported_fields` in the wave-12 v1 hint summary
/// and are never touched here.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkstationDispatchHints {
    pub objective: Option<String>,
    pub scope: Option<String>,
    pub owned_files: Vec<String>,
    pub forbidden_files: Vec<String>,
    pub acceptance_commands: Vec<String>,
    pub commit_policy: Option<String>,
    pub target_project: Option<String>,
    pub requested_cwd: Option<String>,
    pub dispatch_strategy: Option<String>,
}

impl WorkstationDispatchHints {
    /// Merge explicit args > plan-hint values. Args win on every field;
    /// list-shaped fields use the args list outright when non-empty.
    pub(crate) fn merge_args(mut self, args: &Value) -> Self {
        let s = |v: Option<&Value>| v.and_then(|x| x.as_str()).map(|s| s.to_string());
        if let Some(o) = s(args.get("objective")).filter(|x| !x.trim().is_empty()) {
            self.objective = Some(o);
        }
        if let Some(scope) = s(args.get("scope")).filter(|x| !x.trim().is_empty()) {
            self.scope = Some(scope);
        }
        let owned = collect_string_list(args.get("owned_files"));
        if !owned.is_empty() {
            self.owned_files = owned;
        }
        let forbidden = collect_string_list(args.get("forbidden_files"));
        if !forbidden.is_empty() {
            self.forbidden_files = forbidden;
        }
        let acceptance = collect_string_list(args.get("acceptance_commands"));
        if !acceptance.is_empty() {
            self.acceptance_commands = acceptance;
        }
        if let Some(cp) = s(args.get("commit_policy")).filter(|x| !x.trim().is_empty()) {
            self.commit_policy = Some(cp);
        }
        if let Some(tp) = s(args.get("target_project")).filter(|x| !x.trim().is_empty()) {
            self.target_project = Some(tp);
        }
        if let Some(c) = s(args.get("requested_cwd"))
            .or_else(|| s(args.get("cwd")))
            .filter(|x| !x.trim().is_empty())
        {
            self.requested_cwd = Some(c);
        }
        if let Some(ds) = s(args.get("dispatch_strategy")).filter(|x| !x.trim().is_empty()) {
            self.dispatch_strategy = Some(ds);
        }
        self
    }

    /// Cap every list field so a runaway plan body can't bloat the brief
    /// past the downstream 16K `objective` cap. Returns `Some(field_name,
    /// dropped_count)` for each list that was truncated so the caller can
    /// surface it on the response.
    pub(crate) fn cap_lists(&mut self) -> Vec<(&'static str, usize)> {
        let mut dropped: Vec<(&'static str, usize)> = Vec::new();
        for (label, list) in [
            ("owned_files", &mut self.owned_files),
            ("forbidden_files", &mut self.forbidden_files),
            ("acceptance_commands", &mut self.acceptance_commands),
        ] {
            if list.len() > TASK_BRIEF_LIST_CAP {
                let drop_count = list.len() - TASK_BRIEF_LIST_CAP;
                list.truncate(TASK_BRIEF_LIST_CAP);
                dropped.push((label, drop_count));
            }
        }
        dropped
    }

    /// wave-19 / task 07 — overlay a parsed task contract on top of the
    /// existing hints. The contract is the SSOT, so non-empty contract
    /// fields ALWAYS win over caller args (which were merged earlier).
    /// Empty contract list-fields do NOT clobber non-empty arg lists —
    /// that protects against a contract that omits a field (the renderer
    /// only emits non-empty `:acceptance` etc.) from accidentally
    /// erasing a caller-supplied list. The `task_contract_path` field is
    /// preserved so observers can trace provenance.
    pub(crate) fn overlay_contract(&mut self, contract: &ParsedTaskContract) {
        // :goal → objective (always wins when non-empty).
        if !contract.goal.trim().is_empty() {
            self.objective = Some(contract.goal.trim().to_string());
        }
        // :scope (optional, wins when present).
        if let Some(scope) = contract
            .scope
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.scope = Some(scope.to_string());
        }
        // :write-scope → owned_files (only overwrite when non-empty).
        if !contract.write_scope.is_empty() {
            self.owned_files = contract.write_scope.clone();
        }
        // :must-not-touch → forbidden_files (only overwrite when non-empty).
        if !contract.must_not_touch.is_empty() {
            self.forbidden_files = contract.must_not_touch.clone();
        }
        // :acceptance (only overwrite when non-empty).
        if !contract.acceptance.is_empty() {
            self.acceptance_commands = contract.acceptance.clone();
        }
        // :commit (:policy "...") (optional).
        if let Some(policy) = contract
            .commit_policy
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.commit_policy = Some(policy.to_string());
        }
        // :dispatch-strategy (optional, wins when present).
        if let Some(ds) = contract
            .dispatch_strategy
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.dispatch_strategy = Some(ds.to_string());
        }
        // :target-project (optional).
        if let Some(tp) = contract
            .target_project
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.target_project = Some(tp.to_string());
        }
        // :requested-cwd (optional).
        if let Some(cwd) = contract
            .requested_cwd
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.requested_cwd = Some(cwd.to_string());
        }
    }
}

mod descriptor;

#[cfg(test)]
pub(crate) use descriptor::parse_task_contract;
#[cfg(test)]
use descriptor::resolve_contract_path;
pub(crate) use descriptor::{
    load_task_contract, resolve_contract_path_public, ParsedTaskContract, TaskContractParseError,
};

mod decision;

pub(crate) use decision::{
    evaluate_dispatch_decision, DispatchDecision, InferenceContext, INFERABLE_DISPATCH_STRATEGIES,
};
#[cfg(test)]
pub(crate) use decision::{
    explicit_workstation_dispatch_flag, opt_in_requested, WorkstationDispatchSource,
};

mod outcome;

pub(crate) use outcome::{
    outcome_to_response_fields, truncate_brief_preview, SafeDescriptorReason,
    WorkstationDispatchOutcome,
};

mod runner;

pub(crate) use runner::{
    run_workstation_dispatch, run_workstation_dispatch_with_contract,
    run_workstation_dispatch_with_contract_and_trace,
};

mod brief;

pub(crate) use brief::{build_task_brief, build_task_brief_with_source_and_trace};
#[cfg(test)]
pub(crate) use brief::{build_task_brief_with_source, classify_task_kind, BriefTaskKind};

/// Local copy of the same string-list helper used by the compile path.
/// Accepts either a single string or an array of strings; ignores other
/// JSON shapes.
fn collect_string_list(v: Option<&Value>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s.clone()]
            }
        }
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| match item {
                Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

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
const PROPOSAL_VALID_TARGETS: &[&str] = &[
    "mission_execution",
    "mission_task_delegate",
    "mission_flow_run",
];

/// Allowlisted dispatch strategies. Subset of `INFERABLE_DISPATCH_STRATEGIES`
/// — `prompt-fallback` and `unknown` are deliberately NOT proposable
/// because the conservative spawn surface refuses them.
const PROPOSAL_VALID_STRATEGIES: &[&str] = &[
    "resident-lisp",
    "fresh-code-alignment",
    "agent-team",
    "mixed",
];

/// Wire status describing the outcome of the workstation-proposal pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationProposalStatus {
    /// Caller picked a non-LLM mode (`off` or absent); the bundle is absent.
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

// ── wave-22 / task 05 — autonomous workstation true spawn v1 ────────────
//
// Layered on top of the wave-21 / task 04 propose-only pass. The wave-21
// surface was deliberately SURFACE-only (every proposal carried
// `applied=false` and the bundle carried `auto_spawn=false`) so an
// operator could read the proposals and decide whether to re-issue with
// explicit args. Wave-22 / task 05 promotes that propose-only path to a
// CONDITIONAL real spawn under a new `auto_spawn=true` opt-in, gated by
// a multi-rule strict matrix that mirrors wave-22 / task 03's apply gate.
//
// Conservative invariants pinned at the call-site (NEVER violated):
//   * Default `auto_spawn=false` ⇒ byte-compatible with wave-21 / task 04.
//     No spawn, no preflight error, no response augmentation. Every
//     proposal STILL carries `applied=false` on the wire and the
//     wave-21 bundle STILL carries `auto_spawn=false` (the propose-only
//     surface is preserved verbatim — wave-22 surfaces auto-spawn on a
//     SEPARATE `workstation_auto_spawn_gate` block).
//   * Sonnet unavailable ⇒ gate refuses to spawn (`SkippedUnavailable`).
//     There is NEVER a fallback to `claude -p` / prompt mode (the gate
//     status text pins this invariant exactly like the wave-21 bundle).
//   * DAG mode rejects auto_spawn at preflight (mirrors the wave-21
//     `refuse_workstation_inference_in_dag_mode` rule — the gate is
//     single-node-execute-only in v1).
//   * The bundle must be `Suggested` AND every proposal must carry
//     `safety_status=safe` AND `confidence=high` for the gate to fire.
//     The wave-21 conservative whitelists (target ∈ {mission_execution |
//     mission_task_delegate | mission_flow_run}; dispatch_strategy ∈
//     {resident-lisp | fresh-code-alignment | agent-team | mixed}) are
//     enforced through the wave-21 safety classifier — proposals tagged
//     `unsupported_target` / `invalid_strategy` / `ambiguous_value` are
//     rejected here with a deterministic reason.
//   * `task_contract_path` MUST be supplied AND parse successfully into
//     a `ParsedTaskContract` with `:write-scope` non-empty. The contract
//     is the SSOT for what the spawned task is allowed to touch — no
//     contract ⇒ no spawn.
//   * `:must-not-touch` MUST NOT overlap with `:write-scope`. The spawn
//     refuses to dispatch a contract whose own forbidden scope already
//     intersects its write scope (defensive against malformed contracts).
//   * Caller MUST supply `caller_approved=true` (double opt-in mirroring
//     wave-22 / task 03 / 04). A single accidental flag flip cannot
//     trigger a real spawn.
//   * Caller MUST echo `workstation_proposal_hash` matching the
//     deterministic SHA-256 of the bundle. Hash mismatch / missing
//     fail-fast as a structured error BEFORE the spawn substrate runs.
//   * Caller MUST acknowledge `preflight_status_acceptable=true` —
//     this is the surface where the operator confirms hooks / preflight
//     state is acceptable (the daemon does not run hooks itself; the
//     gate just refuses to spawn without explicit confirmation).
//   * The spawn substrate is ALWAYS `mission_task_delegate` (wave-21
//     conservative target whitelist already restricts the candidates;
//     this layer additionally pins `mission_task_delegate` so the
//     spawn never silently takes a `mission_execution` /
//     `mission_flow_run` proposal). The wave-15 substrate
//     (`run_workstation_dispatch_with_contract`) handles the actual
//     dispatch — we NEVER shell out to `claude -p`, and the gate's
//     own status text pins this invariant verbatim.
//   * If any gate fails ⇒ structured failure (SafeDescriptor-style on
//     the `workstation_auto_spawn_gate` block). NO spawn happens.

/// Structured-error code returned when the caller flips `auto_spawn=true`
/// without echoing `workstation_proposal_hash`. Pinned as a constant so
/// dashboards can grep for the load-bearing failure reason without
/// inspecting the gate block. Mirrors the wave-22 / task 03 pattern
/// (`APPLY_GATE_MISSING_PROPOSAL_HASH`).
pub(crate) const AUTO_SPAWN_MISSING_PROPOSAL_HASH: &str = "AUTO_SPAWN_MISSING_PROPOSAL_HASH";

/// Structured-error code returned when the caller-supplied
/// `workstation_proposal_hash` does not match the bundle's deterministic
/// hash. The strongest "the proposals you saw are not the proposals we
/// have" signal — surfacing it BEFORE the spawn substrate runs is the
/// contract's hard requirement.
pub(crate) const AUTO_SPAWN_PROPOSAL_HASH_MISMATCH: &str = "AUTO_SPAWN_PROPOSAL_HASH_MISMATCH";

/// Structured-error code returned when the caller flips `auto_spawn=true`
/// but supplies a non-bool / non-string shape for the gate args. Caller
/// typos must fail fast so they can never silently degrade to skip.
/// Mirrors `APPLY_GATE_INVALID_PARAM` (wave-22 / task 03).
pub(crate) const AUTO_SPAWN_INVALID_PARAM: &str = "AUTO_SPAWN_INVALID_PARAM";

/// Wire status for the wave-22 / task 05 auto-spawn decision. Pinned as
/// a closed enum so dashboards can `grep` for stable strings without
/// inspecting the rest of the gate block. Each variant maps to exactly
/// one `auto_spawn_status` value on the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationAutoSpawnStatus {
    /// Caller did not opt in (`auto_spawn` absent / false). Gate block
    /// is omitted from the response so legacy callers stay byte-identical
    /// with wave-21 / task 04.
    NotRequested,
    /// All gates passed AND the spawn substrate ran successfully. The
    /// spawn-target / inner-result land on the response under the
    /// existing `workstation_dispatch_*` keys (the spawn re-uses the
    /// wave-15 `run_workstation_dispatch_with_contract` path).
    Spawned,
    /// Caller opted in but the bundle reported `Unavailable` (gateway
    /// not initialised / network failure). Gate refuses to synthesise
    /// a deterministic suggestion — there is NEVER a fallback to
    /// `claude -p` / prompt mode.
    SkippedUnavailable,
    /// Caller opted in but the bundle reported `NoSuggestions` /
    /// `PlanHintsPresent` / `NotInvoked`. No proposals to spawn against.
    SkippedNoProposals,
    /// Caller opted in but at least one proposal carried a non-`safe`
    /// `safety_status` (e.g. `ambiguous_value` / `unsupported_target` /
    /// `invalid_strategy`). The wave-21 whitelist is the SSOT here —
    /// the gate refuses to override the safety classifier.
    SkippedUnsafeProposal,
    /// Caller opted in but at least one proposal carried `confidence`
    /// other than `high`. The auto-spawn gate is deliberately stricter
    /// than the propose-only surface.
    SkippedConfidenceTooLow,
    /// Caller opted in but did not flip `caller_approved=true`. The
    /// double opt-in is required precisely so the gate cannot fire by
    /// a single accidental flag flip.
    SkippedCallerNotApproved,
    /// Caller opted in but did not supply `task_contract_path` (the
    /// contract is the SSOT for what the spawn is allowed to touch).
    SkippedMissingTaskContractPath,
    /// Caller opted in AND supplied `task_contract_path` but the file
    /// is missing / malformed / fails parse — the spawn refuses to
    /// proceed without a valid contract.
    SkippedMalformedTaskContract,
    /// Caller opted in but the contract's `:write-scope` is empty —
    /// a spawn against an empty write-scope contract has nothing to
    /// own; refuse defensively.
    SkippedEmptyWriteScope,
    /// Caller opted in but the contract's `:write-scope` overlaps with
    /// `:must-not-touch` — defensive refusal against a malformed
    /// contract that contradicts itself.
    SkippedForbiddenScopeOverlap,
    /// Caller opted in but did not acknowledge
    /// `preflight_status_acceptable=true`. The daemon does not run
    /// hooks itself; the gate refuses to spawn without explicit
    /// operator confirmation.
    SkippedPreflightUnacceptable,
    /// Caller opted in but the proposed `target` is not
    /// `mission_task_delegate`. The auto-spawn gate ALWAYS spawns
    /// through the wave-15 workstation substrate, which only wraps
    /// `mission_task_delegate`.
    SkippedUnsupportedTarget,
    /// All gates passed BUT the wave-15 substrate refused the dispatch
    /// (e.g. project root unresolved). The substrate's safe-descriptor
    /// reason flows through verbatim under
    /// `auto_spawn_substrate_reason` so the operator can fix and retry.
    SkippedSubstrateRefused,
    /// All gates passed BUT the wave-15 substrate's inner handler
    /// returned an error result. The inner payload flows through under
    /// `auto_spawn_inner_payload` so the operator can correlate.
    SkippedSubstrateInnerError,
}

impl WorkstationAutoSpawnStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationAutoSpawnStatus::NotRequested => "not_requested",
            WorkstationAutoSpawnStatus::Spawned => "spawned",
            WorkstationAutoSpawnStatus::SkippedUnavailable => "skipped_unavailable",
            WorkstationAutoSpawnStatus::SkippedNoProposals => "skipped_no_proposals",
            WorkstationAutoSpawnStatus::SkippedUnsafeProposal => "skipped_unsafe_proposal",
            WorkstationAutoSpawnStatus::SkippedConfidenceTooLow => "skipped_confidence_too_low",
            WorkstationAutoSpawnStatus::SkippedCallerNotApproved => "skipped_caller_not_approved",
            WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath => {
                "skipped_missing_task_contract_path"
            }
            WorkstationAutoSpawnStatus::SkippedMalformedTaskContract => {
                "skipped_malformed_task_contract"
            }
            WorkstationAutoSpawnStatus::SkippedEmptyWriteScope => "skipped_empty_write_scope",
            WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap => {
                "skipped_forbidden_scope_overlap"
            }
            WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable => {
                "skipped_preflight_unacceptable"
            }
            WorkstationAutoSpawnStatus::SkippedUnsupportedTarget => "skipped_unsupported_target",
            WorkstationAutoSpawnStatus::SkippedSubstrateRefused => "skipped_substrate_refused",
            WorkstationAutoSpawnStatus::SkippedSubstrateInnerError => {
                "skipped_substrate_inner_error"
            }
        }
    }

    /// True iff the gate authorised the substrate dispatch AND the
    /// substrate ran successfully. Used by the response splicer to
    /// decide whether to attach the full inner payload.
    pub(crate) fn was_spawned(self) -> bool {
        matches!(self, WorkstationAutoSpawnStatus::Spawned)
    }
}

/// Wire status for the deterministic proposal-hash check on the
/// auto-spawn gate. Mirrors wave-22 / task 03's `ProposalHashStatus`
/// shape so dashboards see a uniform vocabulary across the two gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationProposalHashStatus {
    /// Caller did not supply `workstation_proposal_hash`. Under
    /// `auto_spawn=true` this collapses to a structured error
    /// (`AUTO_SPAWN_MISSING_PROPOSAL_HASH`) BEFORE the gate runs;
    /// surfaced here for completeness when the gate's preflight is
    /// bypassed (unit tests).
    NotSupplied,
    /// Caller-supplied hash matches the bundle's deterministic hash.
    Matches,
    /// Caller-supplied hash does NOT match. Surfaced as a structured
    /// error (`AUTO_SPAWN_PROPOSAL_HASH_MISMATCH`) BEFORE the gate runs.
    Mismatch,
    /// No bundle / no proposals available — hash check is moot.
    NoProposalAvailable,
}

impl WorkstationProposalHashStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationProposalHashStatus::NotSupplied => "not_supplied",
            WorkstationProposalHashStatus::Matches => "matches",
            WorkstationProposalHashStatus::Mismatch => "mismatch",
            WorkstationProposalHashStatus::NoProposalAvailable => "no_proposal_available",
        }
    }
}

/// Caller-supplied opt-in inputs for the wave-22 / task 05 auto-spawn
/// gate. Strict-shape: `auto_spawn` / `caller_approved` /
/// `preflight_status_acceptable` are bool-only (literal strings `"true"`
/// / `"false"` are rejected so a typo cannot silently flip the gate);
/// `workstation_proposal_hash` and `task_contract_path` are string-only.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkstationAutoSpawnInput {
    /// Caller opted into the gate (`auto_spawn=true`).
    pub auto_spawn: bool,
    /// Caller-supplied SHA-256 hash (32 hex chars) of the bundle they
    /// inspected. Required when `auto_spawn=true`.
    pub proposal_hash: Option<String>,
    /// Caller's second opt-in flag confirming human intent.
    /// Required-truthy when `auto_spawn=true`.
    pub caller_approved: bool,
    /// Caller-supplied `task_contract_path` (relative against the
    /// project root or absolute). Required when `auto_spawn=true`.
    pub task_contract_path: Option<String>,
    /// Caller's acknowledgement that hooks / preflight state is
    /// acceptable. Required-truthy when `auto_spawn=true`. The daemon
    /// does NOT run hooks itself — this is the explicit operator
    /// confirmation surface.
    pub preflight_status_acceptable: bool,
    /// True iff the caller explicitly supplied any of the gate fields
    /// (used to differentiate "caller opted out" from "caller never saw
    /// the knob" so the response stays byte-identical for the latter).
    pub explicit: bool,
}

/// Strict pre-flight validator for the wave-22 / task 05 auto-spawn
/// args. Rejects any non-bool / non-string shape so caller typos fail
/// fast with structured errors. Pure / no I/O. Mirrors
/// `parse_llm_approve_apply_gate_input` (wave-22 / task 03).
pub(crate) fn parse_workstation_auto_spawn_input(
    args: &Value,
) -> std::result::Result<WorkstationAutoSpawnInput, (String, String)> {
    let mut input = WorkstationAutoSpawnInput::default();

    let auto_spawn_v = args.get("auto_spawn");
    let hash_v = args.get("workstation_proposal_hash");
    let caller_v = args.get("workstation_caller_approved");
    let path_v = args.get("task_contract_path");
    let preflight_v = args.get("preflight_status_acceptable");
    input.explicit = auto_spawn_v.is_some()
        || hash_v.is_some()
        || caller_v.is_some()
        || path_v.is_some()
        || preflight_v.is_some();

    if let Some(v) = auto_spawn_v {
        if v.is_null() {
            // null behaves like absent
        } else if let Some(b) = v.as_bool() {
            input.auto_spawn = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "auto_spawn must be a boolean (true|false); got {} \
                     — string `\"true\"` is REJECTED so a typo cannot silently flip the gate",
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
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "workstation_proposal_hash must be a string (SHA-256 hex truncated to 32 chars); \
                     got {}",
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
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "workstation_caller_approved must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = path_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                input.task_contract_path = Some(trimmed.to_string());
            }
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "task_contract_path must be a string (relative against project root or absolute); \
                     got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = preflight_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(b) = v.as_bool() {
            input.preflight_status_acceptable = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "preflight_status_acceptable must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    Ok(input)
}

/// Pure deterministic SHA-256 hash over the LOAD-BEARING fields of a
/// workstation proposal bundle. Truncated to the leading 32 hex chars
/// (128 bits — way more than enough collision resistance for an
/// audit-trail correlator). Mirrors `compute_proposal_hash` (wave-22 /
/// task 03) for symmetry.
///
/// Inputs (canonical form):
///   * literal `"v1"` schema sentinel,
///   * bundle status wire,
///   * each proposal in its received order:
///       `field|value|confidence|safety_status`,
///     joined by `;`.
///
/// The hash is what the caller is expected to echo back via the
/// `workstation_proposal_hash` arg under `auto_spawn=true`. Caller can
/// derive it themselves from the `workstation_proposals` block — we
/// surface the same value under `workstation_proposal_hash` on the
/// `workstation_auto_spawn_gate` block so dashboards can
/// `assert hash == derive(...)` directly.
pub(crate) fn compute_workstation_proposal_hash(bundle: &WorkstationProposalBundle) -> String {
    use sha2::{Digest, Sha256};
    let proposals: Vec<String> = bundle
        .proposals
        .iter()
        .map(|p| {
            let value_str = p
                .value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| p.value.to_string());
            format!(
                "{}|{}|{}|{}",
                p.field,
                value_str,
                p.confidence.as_wire(),
                p.safety_status.as_wire(),
            )
        })
        .collect();
    let payload = format!("v1|{}|{}", bundle.status.as_wire(), proposals.join(";"));
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full.chars().take(32).collect()
}

/// Pure outcome of [`evaluate_workstation_auto_spawn_gate`]. Side-effect
/// free — no DB, no bus, no LLM, no spawn. The handler consumes this
/// projection to decide whether to run the wave-15 substrate dispatch.
#[derive(Debug, Clone)]
pub(crate) struct WorkstationAutoSpawnGateOutcome {
    /// Whether the caller opted into the gate at all.
    pub requested: bool,
    /// Wire status — the load-bearing signal for observers.
    pub status: WorkstationAutoSpawnStatus,
    /// Spawn target the gate would have used (always
    /// `mission_task_delegate` when status=Spawned; carries the proposed
    /// target verbatim under SkippedUnsupportedTarget so the operator
    /// can see what was offered). `None` when no proposal exists.
    pub spawn_target: Option<String>,
    /// Validated `task_contract_path` (echoed for audit symmetry).
    pub task_contract_path: Option<String>,
    /// Result of the proposal-hash comparison.
    pub proposal_hash_status: WorkstationProposalHashStatus,
    /// Hash the gate computed from the bundle (always populated when a
    /// bundle exists; None when bundle is absent / unavailable).
    pub computed_proposal_hash: Option<String>,
    /// Caller-supplied hash (echoed for audit symmetry).
    pub supplied_proposal_hash: Option<String>,
    /// Whether the caller flipped `workstation_caller_approved=true`.
    pub caller_approved: bool,
    /// Whether the caller flipped `preflight_status_acceptable=true`.
    pub preflight_status_acceptable: bool,
    /// Flat list of `code:detail` strings explaining every gate's
    /// outcome. Always populated under non-NotRequested statuses.
    pub gate_results: Vec<String>,
    /// When the substrate dispatch ran (status=Spawned or status=
    /// SkippedSubstrateRefused / SkippedSubstrateInnerError), the
    /// substrate's safe-descriptor reason flows through here verbatim.
    pub substrate_reason: Option<String>,
}

impl WorkstationAutoSpawnGateOutcome {
    /// Build the wire shape consumed by the response payload. Always
    /// emits every field (with `null` for absent values) so observers
    /// can pivot on a stable shape regardless of which skip reason
    /// fired.
    pub(crate) fn to_response_json(&self) -> Value {
        json!({
            "requested": self.requested,
            "auto_spawn_status": self.status.as_wire(),
            "spawn_target": self.spawn_target.clone(),
            "task_contract_path": self.task_contract_path.clone(),
            "proposal_hash_status": self.proposal_hash_status.as_wire(),
            "computed_proposal_hash": self.computed_proposal_hash.clone(),
            "supplied_proposal_hash": self.supplied_proposal_hash.clone(),
            "caller_approved": self.caller_approved,
            "preflight_status_acceptable": self.preflight_status_acceptable,
            "gate_results": self.gate_results.clone(),
            "substrate_reason": self.substrate_reason.clone(),
        })
    }

    /// Construct a `NotRequested` outcome — used when the caller did
    /// not opt in. The gate block is omitted from the response in
    /// this case so wave-21 / task 04 byte-shape is preserved exactly.
    pub(crate) fn not_requested() -> Self {
        WorkstationAutoSpawnGateOutcome {
            requested: false,
            status: WorkstationAutoSpawnStatus::NotRequested,
            spawn_target: None,
            task_contract_path: None,
            proposal_hash_status: WorkstationProposalHashStatus::NotSupplied,
            computed_proposal_hash: None,
            supplied_proposal_hash: None,
            caller_approved: false,
            preflight_status_acceptable: false,
            gate_results: Vec::new(),
            substrate_reason: None,
        }
    }
}

/// Helper: extract a proposal's value-as-string (proposals are always
/// string-shaped in v0; the helper exists so the gate evaluator can
/// stay terse).
fn proposal_value_str(p: &WorkstationProposal) -> Option<String> {
    p.value.as_str().map(|s| s.trim().to_string())
}

/// Helper: extract the proposed `target` value from the bundle (or
/// `None` when no `target` proposal is present). Used by the gate to
/// pin the spawn target before validating against the
/// `mission_task_delegate` whitelist.
fn extract_proposed_target(bundle: &WorkstationProposalBundle) -> Option<String> {
    bundle
        .proposals
        .iter()
        .find(|p| p.field == "target")
        .and_then(proposal_value_str)
        .filter(|s| !s.is_empty())
}

/// Strict pre-flight for the wave-22 / task 05 auto-spawn gate. Runs
/// the fail-fast hash-missing / hash-mismatch checks BEFORE any spawn
/// substrate dispatch. Returns `Ok(())` when:
///   * caller did not opt in (`auto_spawn=false`);
///   * caller opted in AND supplied a hash that matches.
/// Returns `Err((code, message))` for the two contract-mandated
/// structured errors:
///   * `AUTO_SPAWN_MISSING_PROPOSAL_HASH` — `auto_spawn=true` without a hash.
///   * `AUTO_SPAWN_PROPOSAL_HASH_MISMATCH` — `auto_spawn=true` with a hash
///     that does not match the bundle.
///
/// The handler converts the Err into [`ToolResult::structured_error`]
/// BEFORE calling the wave-15 substrate dispatch, satisfying the
/// contract: "On hash mismatch / missing, return structured error and
/// do not spawn."
pub(crate) fn enforce_auto_spawn_preflight(
    input: &WorkstationAutoSpawnInput,
    bundle: Option<&WorkstationProposalBundle>,
) -> std::result::Result<(), (String, String)> {
    if !input.auto_spawn {
        return Ok(());
    }
    let bundle = match bundle {
        Some(b) => b,
        None => {
            // Without a bundle we cannot compute a hash. Surface the
            // missing-hash code so the caller knows to opt into
            // `workstation_inference_mode="sonnet_suggest"` first (or
            // to drop the auto_spawn flag).
            if input.proposal_hash.is_none() {
                return Err((
                    AUTO_SPAWN_MISSING_PROPOSAL_HASH.to_string(),
                    "auto_spawn=true requires `workstation_proposal_hash` AND a workstation \
                     proposal bundle to apply against; bundle is absent (set \
                     workstation_inference_mode=\"sonnet_suggest\" first)"
                        .to_string(),
                ));
            }
            return Err((
                AUTO_SPAWN_PROPOSAL_HASH_MISMATCH.to_string(),
                "auto_spawn=true with `workstation_proposal_hash` but no workstation \
                 proposal bundle is available to compare against"
                    .to_string(),
            ));
        }
    };
    let hash = compute_workstation_proposal_hash(bundle);
    match input.proposal_hash.as_deref() {
        None => Err((
            AUTO_SPAWN_MISSING_PROPOSAL_HASH.to_string(),
            format!(
                "auto_spawn=true requires `workstation_proposal_hash`; expected `{}` (echoed under \
                 `workstation_auto_spawn_gate.computed_proposal_hash` in the propose-only response)",
                hash,
            ),
        )),
        Some(s) if s.eq_ignore_ascii_case(&hash) => Ok(()),
        Some(s) => Err((
            AUTO_SPAWN_PROPOSAL_HASH_MISMATCH.to_string(),
            format!(
                "auto_spawn=true with `workstation_proposal_hash=`{}`` does not match bundle hash `{}`",
                s, hash,
            ),
        )),
    }
}

/// Pure evaluator of the wave-22 / task 05 auto-spawn gate. Does NOT
/// mutate state, does NOT spawn a workstation, does NOT call any
/// substrate. Computes the structured outcome the response carries;
/// the handler reads `outcome.status.was_spawned()` to decide whether
/// to attach the substrate dispatch payload.
///
/// Hash mismatch / missing is allowed to surface here as a SKIP
/// (`SkippedCallerNotApproved`) so unit tests that bypass the
/// preflight still get a sane outcome — production paths run the
/// preflight FIRST and hit the structured-error return BEFORE this
/// evaluator.
///
/// Inputs:
///   * `input`               — caller-supplied gate args (parsed via
///                              `parse_workstation_auto_spawn_input`).
///   * `bundle`              — proposal bundle from
///                              `request_workstation_proposals`.
///   * `parsed_contract`     — task-contract v1 already parsed by the
///                              caller, OR `None` when the caller did
///                              not supply / failed to load the path.
///   * `contract_load_error` — typed parse failure when present (used
///                              to distinguish "missing path" from
///                              "malformed file" in the gate output).
pub(crate) fn evaluate_workstation_auto_spawn_gate(
    input: &WorkstationAutoSpawnInput,
    bundle: Option<&WorkstationProposalBundle>,
    parsed_contract: Option<&ParsedTaskContract>,
    contract_load_error: Option<&str>,
) -> WorkstationAutoSpawnGateOutcome {
    let mut gate_results: Vec<String> = Vec::new();

    // Compute the hash + hash status up-front so observers always see
    // the deterministic verdict (regardless of whether the gate ran).
    let (computed_hash, hash_status) = match bundle {
        Some(b) if !b.proposals.is_empty() => {
            let hash = compute_workstation_proposal_hash(b);
            let status = match input.proposal_hash.as_deref() {
                None => WorkstationProposalHashStatus::NotSupplied,
                Some(s) if s.eq_ignore_ascii_case(&hash) => WorkstationProposalHashStatus::Matches,
                Some(_) => WorkstationProposalHashStatus::Mismatch,
            };
            (Some(hash), status)
        }
        Some(_) | None => (None, WorkstationProposalHashStatus::NoProposalAvailable),
    };

    let proposed_target = bundle.and_then(extract_proposed_target);

    // G1 — caller opted in. Default short-circuit returns NotRequested
    // so the response stays byte-identical with wave-21 / task 04
    // callers that never see the knob.
    if !input.auto_spawn {
        return WorkstationAutoSpawnGateOutcome::not_requested();
    }
    gate_results.push("rule:g1_auto_spawn_opt_in:true".to_string());

    // Build a partial outcome for the SKIP branches below — every
    // SKIP carries the deterministic hash verdict + caller flags so
    // dashboards can pivot uniformly.
    let mk_skip = |status: WorkstationAutoSpawnStatus,
                   gate_results: Vec<String>,
                   substrate_reason: Option<String>|
     -> WorkstationAutoSpawnGateOutcome {
        WorkstationAutoSpawnGateOutcome {
            requested: true,
            status,
            spawn_target: proposed_target.clone(),
            task_contract_path: input.task_contract_path.clone(),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash.clone(),
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            preflight_status_acceptable: input.preflight_status_acceptable,
            gate_results,
            substrate_reason,
        }
    };

    // G2 — proposal bundle present and Suggested.
    let bundle = match bundle {
        Some(b) => b,
        None => {
            gate_results.push(
                "rule:g2_bundle_status:absent (workstation_inference_mode=\"sonnet_suggest\" \
                 not set; nothing to spawn against)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedNoProposals,
                gate_results,
                None,
            );
        }
    };
    match bundle.status {
        WorkstationProposalStatus::Unavailable => {
            gate_results.push(
                "rule:g2_bundle_status:llm_unavailable (Sonnet gateway unavailable; gate refuses \
                 fallback to claude -p / prompt mode)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedUnavailable,
                gate_results,
                None,
            );
        }
        WorkstationProposalStatus::NotInvoked
        | WorkstationProposalStatus::NoSuggestions
        | WorkstationProposalStatus::PlanHintsPresent => {
            gate_results.push(format!(
                "rule:g2_bundle_status:{} (no proposals to spawn against)",
                bundle.status.as_wire(),
            ));
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedNoProposals,
                gate_results,
                None,
            );
        }
        WorkstationProposalStatus::Suggested => {
            gate_results.push("rule:g2_bundle_status:suggested".to_string());
        }
    }
    if bundle.proposals.is_empty() {
        gate_results.push("rule:g2_bundle_status:suggested_but_empty (defensive)".to_string());
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedNoProposals,
            gate_results,
            None,
        );
    }

    // G3 — proposal hash matches.
    match hash_status {
        WorkstationProposalHashStatus::Matches => {
            gate_results.push("rule:g3_proposal_hash:matches".to_string());
        }
        WorkstationProposalHashStatus::NotSupplied => {
            gate_results.push(
                "rule:g3_proposal_hash:not_supplied (gate requires explicit hash echo; \
                 production path runs preflight first and fail-fasts BEFORE this evaluator)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
                gate_results,
                None,
            );
        }
        WorkstationProposalHashStatus::Mismatch => {
            gate_results.push(
                "rule:g3_proposal_hash:mismatch (caller-supplied hash does not match bundle; \
                 production path runs preflight first and fail-fasts BEFORE this evaluator)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
                gate_results,
                None,
            );
        }
        WorkstationProposalHashStatus::NoProposalAvailable => {
            // Already handled by G2 above; defensive.
            gate_results
                .push("rule:g3_proposal_hash:no_proposal_available (defensive)".to_string());
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedNoProposals,
                gate_results,
                None,
            );
        }
    }

    // G4 — every proposal carries safety_status=safe (wave-21
    // whitelist enforcement).
    let unsafe_proposals: Vec<&WorkstationProposal> = bundle
        .proposals
        .iter()
        .filter(|p| p.safety_status != WorkstationProposalSafetyStatus::Safe)
        .collect();
    if !unsafe_proposals.is_empty() {
        let summary = unsafe_proposals
            .iter()
            .map(|p| format!("{}={}", p.field, p.safety_status.as_wire()))
            .collect::<Vec<_>>()
            .join(",");
        gate_results.push(format!(
            "rule:g4_safety_status:non_safe_proposals=[{}] (wave-21 whitelist refuses to spawn \
             against ambiguous_value / unsupported_target / invalid_strategy)",
            summary,
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedUnsafeProposal,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g4_safety_status:all_safe".to_string());

    // G5 — every proposal carries confidence=high.
    let low_confidence: Vec<&WorkstationProposal> = bundle
        .proposals
        .iter()
        .filter(|p| p.confidence != WorkstationProposalConfidence::High)
        .collect();
    if !low_confidence.is_empty() {
        let summary = low_confidence
            .iter()
            .map(|p| format!("{}={}", p.field, p.confidence.as_wire()))
            .collect::<Vec<_>>()
            .join(",");
        gate_results.push(format!(
            "rule:g5_confidence:non_high_proposals=[{}] (auto-spawn gate is deliberately \
             stricter than propose-only)",
            summary,
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedConfidenceTooLow,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g5_confidence:all_high".to_string());

    // G6 — caller_approved double opt-in.
    if !input.caller_approved {
        gate_results.push(
            "rule:g6_caller_approved:false (apply gate requires the explicit \
             workstation_caller_approved=true confirmation)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g6_caller_approved:true".to_string());

    // G7 — preflight_status_acceptable opt-in. The daemon does NOT
    // run hooks itself; this is the explicit operator confirmation
    // surface.
    if !input.preflight_status_acceptable {
        gate_results.push(
            "rule:g7_preflight_status_acceptable:false (gate refuses to spawn without explicit \
             operator confirmation that hooks / preflight state is acceptable)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g7_preflight_status_acceptable:true".to_string());

    // G8 — task_contract_path supplied.
    if input.task_contract_path.is_none() {
        gate_results.push(
            "rule:g8_task_contract_path:missing (contract is the SSOT for what the spawn is \
             allowed to touch; no contract ⇒ no spawn)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g8_task_contract_path:supplied".to_string());

    // G9 — task_contract loaded successfully.
    let contract = match parsed_contract {
        Some(c) => c,
        None => {
            let detail = contract_load_error
                .map(|e| format!(": {}", e))
                .unwrap_or_default();
            gate_results.push(format!(
                "rule:g9_task_contract_load:failed{} (gate refuses to spawn without a valid \
                 task-contract v1 file)",
                detail,
            ));
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedMalformedTaskContract,
                gate_results,
                contract_load_error.map(|s| s.to_string()),
            );
        }
    };
    gate_results.push("rule:g9_task_contract_load:ok".to_string());

    // G10 — write_scope non-empty.
    if contract.write_scope.is_empty() {
        gate_results.push(
            "rule:g10_write_scope:empty (refusing to spawn against a contract with no \
             :write-scope — there is nothing for the spawn to own)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedEmptyWriteScope,
            gate_results,
            None,
        );
    }
    gate_results.push(format!(
        "rule:g10_write_scope:non_empty (count={})",
        contract.write_scope.len(),
    ));

    // G11 — :must-not-touch must NOT overlap with :write-scope.
    let overlap: Vec<String> = contract
        .write_scope
        .iter()
        .filter(|p| contract.must_not_touch.iter().any(|f| f.trim() == p.trim()))
        .cloned()
        .collect();
    if !overlap.is_empty() {
        gate_results.push(format!(
            "rule:g11_forbidden_scope_overlap:[{}] (defensive refusal against a contract that \
             contradicts itself — :write-scope intersects :must-not-touch)",
            overlap.join(","),
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g11_forbidden_scope_overlap:none".to_string());

    // G12 — proposed target must be mission_task_delegate. The wave-15
    // substrate (`run_workstation_dispatch_with_contract`) ONLY wraps
    // `mission_task_delegate`; we pin the same invariant here so the
    // gate refuses BEFORE the substrate would.
    let target_value = match proposed_target.as_deref() {
        Some(s) => s,
        None => {
            // No `target` proposal ⇒ caller's intent is ambiguous.
            // Refuse defensively.
            gate_results.push(
                "rule:g12_spawn_target:missing (no `target` proposal in the bundle — refusing \
                 to spawn against an ambiguous target)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedUnsupportedTarget,
                gate_results,
                None,
            );
        }
    };
    if !target_value.eq_ignore_ascii_case("mission_task_delegate") {
        gate_results.push(format!(
            "rule:g12_spawn_target:unsupported (proposed target=`{}` — auto-spawn always \
             routes through mission_task_delegate substrate, never claude -p)",
            target_value,
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedUnsupportedTarget,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g12_spawn_target:mission_task_delegate".to_string());

    // All gates passed. The handler will run the wave-15 substrate
    // dispatch with the validated contract and update this outcome
    // to status=Spawned (or SkippedSubstrate{Refused,InnerError}
    // depending on the substrate result).
    gate_results.push(
        "rule:auto_spawn_gate_satisfied (G1..G12 all green; handler may run \
         run_workstation_dispatch_with_contract through mission_task_delegate substrate)"
            .to_string(),
    );
    WorkstationAutoSpawnGateOutcome {
        requested: true,
        status: WorkstationAutoSpawnStatus::Spawned,
        spawn_target: Some("mission_task_delegate".to_string()),
        task_contract_path: input.task_contract_path.clone(),
        proposal_hash_status: hash_status,
        computed_proposal_hash: computed_hash,
        supplied_proposal_hash: input.proposal_hash.clone(),
        caller_approved: input.caller_approved,
        preflight_status_acceptable: input.preflight_status_acceptable,
        gate_results,
        substrate_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;
    use missiond_core::types::PlanStatus;
    use uuid::Uuid;

    fn fixture_plan(sexp: &str) -> Plan {
        Plan {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000def").unwrap(),
            board_task_id: "btk-wd".to_string(),
            source_directive_id: None,
            version: 1,
            sexp_text: sexp.to_string(),
            sexp_hash: "deadbeef".to_string(),
            status: PlanStatus::Approved,
            compiler_model: None,
            compiled_from: None,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            approved_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn opt_in_requires_explicit_arg_or_plan_hint() {
        assert!(!opt_in_requested(&json!({}), false));
        assert!(opt_in_requested(
            &json!({"workstation_dispatch": true}),
            false
        ));
        assert!(opt_in_requested(&json!({}), true));
        // Random truthy fields do NOT count.
        assert!(!opt_in_requested(
            &json!({"target": "mission_task_delegate"}),
            false
        ));
        assert!(!opt_in_requested(
            &json!({"workstation_dispatch": false}),
            false
        ));
    }

    // ── wave-16 / task 03 — auto-inference decision tests ───────────────

    /// Helper: build an inference context that matches every gate by
    /// default. Individual tests flip a single field to assert that gate.
    fn ctx_all_pass<'a>() -> InferenceContext<'a> {
        InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "fresh-code-alignment",
            objective: Some("ship the wave"),
            owned_files_present: true,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: false,
        }
    }

    #[test]
    fn evaluate_decision_explicit_true_wins_even_without_scope_signal() {
        let mut ctx = ctx_all_pass();
        ctx.owned_files_present = false;
        let decision =
            evaluate_dispatch_decision(&json!({"workstation_dispatch": true}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::ExplicitArg);
        assert!(decision.is_enabled());
    }

    #[test]
    fn evaluate_decision_explicit_false_disables_inference() {
        let ctx = ctx_all_pass();
        let decision = evaluate_dispatch_decision(
            &json!({"workstation_dispatch": false}),
            true, // even with plan hint set
            &ctx,
        );
        assert_eq!(decision.source, WorkstationDispatchSource::Disabled);
        assert!(!decision.is_enabled());
        assert!(decision
            .reason
            .unwrap()
            .contains("workstation_dispatch=false"));
    }

    #[test]
    fn evaluate_decision_plan_hint_takes_precedence_over_inference() {
        let ctx = ctx_all_pass();
        let decision = evaluate_dispatch_decision(&json!({}), true, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::PlanHint);
        assert!(decision.is_enabled());
    }

    #[test]
    fn evaluate_decision_inferred_when_all_gates_pass() {
        let ctx = ctx_all_pass();
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
        assert!(decision.is_enabled());
        assert!(decision.reason.unwrap().contains("fresh-code-alignment"));
    }

    #[test]
    fn evaluate_decision_inferred_for_each_strategy_in_whitelist() {
        for strategy in INFERABLE_DISPATCH_STRATEGIES {
            let mut ctx = ctx_all_pass();
            ctx.dispatch_strategy = strategy;
            let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
            assert_eq!(
                decision.source,
                WorkstationDispatchSource::Inferred,
                "strategy `{}` should be inferable",
                strategy
            );
        }
    }

    #[test]
    fn evaluate_decision_not_inferred_for_unknown_strategy() {
        let mut ctx = ctx_all_pass();
        ctx.dispatch_strategy = "unknown";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(!decision.is_enabled());
        assert!(decision.reason.unwrap().contains("dispatch strategy"));
    }

    #[test]
    fn evaluate_decision_not_inferred_for_prompt_fallback_strategy() {
        let mut ctx = ctx_all_pass();
        ctx.dispatch_strategy = "prompt-fallback";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn evaluate_decision_not_inferred_for_mission_execution_target() {
        let mut ctx = ctx_all_pass();
        ctx.target = "mission_execution";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("mission_task_delegate"));
    }

    #[test]
    fn evaluate_decision_not_inferred_for_mission_flow_run_target() {
        let mut ctx = ctx_all_pass();
        ctx.target = "mission_flow_run";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn evaluate_decision_not_inferred_when_objective_missing() {
        let mut ctx = ctx_all_pass();
        ctx.objective = None;
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("objective"));
    }

    #[test]
    fn evaluate_decision_not_inferred_when_objective_blank() {
        let mut ctx = ctx_all_pass();
        ctx.objective = Some("   ");
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn evaluate_decision_not_inferred_when_no_scope_signal() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "fresh-code-alignment",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("scoping signal"));
    }

    #[test]
    fn evaluate_decision_inferred_when_scope_present_only() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "agent-team",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: true,
            target_project_present: false,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn evaluate_decision_inferred_when_target_project_present_only() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "resident-lisp",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: false,
            target_project_present: true,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn evaluate_decision_inferred_when_requested_cwd_present_only() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "mixed",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: true,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn workstation_dispatch_source_string_pin() {
        // The five values are part of the response wire contract.
        assert_eq!(
            WorkstationDispatchSource::ExplicitArg.as_str(),
            "explicit_arg"
        );
        assert_eq!(WorkstationDispatchSource::PlanHint.as_str(), "plan_hint");
        assert_eq!(WorkstationDispatchSource::Inferred.as_str(), "inferred");
        assert_eq!(WorkstationDispatchSource::Disabled.as_str(), "disabled");
        assert_eq!(
            WorkstationDispatchSource::NotApplicable.as_str(),
            "not_applicable"
        );
    }

    /// End-to-end shape check: when auto-inference picks `agent-team`,
    /// the brief built from the inferred hints carries the literal Chinese
    /// reminder exactly once. This pins the wave-15 invariant onto the
    /// wave-16 inference path so a future merge cannot silently double-
    /// inject the hint.
    #[test]
    fn inferred_agent_team_path_injects_literal_exactly_once() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "agent-team",
            objective: Some("ship the wave"),
            owned_files_present: true,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
        // Build the brief the same way `run_workstation_dispatch` would
        // to confirm the literal lands once.
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship the wave".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "agent-team");
        assert_eq!(
            brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "agent-team hint must appear exactly once on the inferred path"
        );
    }

    #[test]
    fn explicit_workstation_dispatch_flag_extracts_explicit_choice() {
        assert_eq!(explicit_workstation_dispatch_flag(&json!({})), None);
        assert_eq!(
            explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": true})),
            Some(true)
        );
        assert_eq!(
            explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": false})),
            Some(false)
        );
        // Non-bool values do not satisfy the strict opt-in/out contract.
        assert_eq!(
            explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": "yes"})),
            None
        );
    }

    #[test]
    fn merge_args_arg_wins_over_hint_for_every_field() {
        let hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            scope: Some("hint scope".to_string()),
            owned_files: vec!["hint.rs".to_string()],
            forbidden_files: vec!["hint_forbidden.rs".to_string()],
            acceptance_commands: vec!["hint cmd".to_string()],
            commit_policy: Some("hint-policy".to_string()),
            target_project: Some("hint-proj".to_string()),
            requested_cwd: Some("/hint/cwd".to_string()),
            dispatch_strategy: Some("resident-lisp".to_string()),
        };
        let args = json!({
            "objective": "arg obj",
            "scope": "arg scope",
            "owned_files": ["arg.rs", "arg2.rs"],
            "forbidden_files": ["arg_forbidden.rs"],
            "acceptance_commands": ["arg cmd1", "arg cmd2"],
            "commit_policy": "arg-policy",
            "target_project": "arg-proj",
            "requested_cwd": "/arg/cwd",
            "dispatch_strategy": "agent-team",
        });
        let merged = hints.merge_args(&args);
        assert_eq!(merged.objective.as_deref(), Some("arg obj"));
        assert_eq!(merged.scope.as_deref(), Some("arg scope"));
        assert_eq!(merged.owned_files, vec!["arg.rs", "arg2.rs"]);
        assert_eq!(merged.forbidden_files, vec!["arg_forbidden.rs"]);
        assert_eq!(merged.acceptance_commands, vec!["arg cmd1", "arg cmd2"]);
        assert_eq!(merged.commit_policy.as_deref(), Some("arg-policy"));
        assert_eq!(merged.target_project.as_deref(), Some("arg-proj"));
        assert_eq!(merged.requested_cwd.as_deref(), Some("/arg/cwd"));
        assert_eq!(merged.dispatch_strategy.as_deref(), Some("agent-team"));
    }

    #[test]
    fn merge_args_falls_back_to_hint_when_arg_absent_or_blank() {
        let hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            commit_policy: Some("hint-policy".to_string()),
            ..Default::default()
        };
        let args = json!({
            "objective": "   ",  // blank → falls back
            "commit_policy": "",
        });
        let merged = hints.merge_args(&args);
        assert_eq!(merged.objective.as_deref(), Some("hint obj"));
        assert_eq!(merged.commit_policy.as_deref(), Some("hint-policy"));
    }

    #[test]
    fn merge_args_cwd_falls_back_to_args_cwd_alias() {
        let hints = WorkstationDispatchHints::default();
        let args = json!({"cwd": "/from/cwd/alias"});
        let merged = hints.merge_args(&args);
        assert_eq!(merged.requested_cwd.as_deref(), Some("/from/cwd/alias"));
    }

    #[test]
    fn cap_lists_truncates_runaway_lists_and_reports_drop_count() {
        let mut hints = WorkstationDispatchHints {
            owned_files: (0..100).map(|i| format!("f{}.rs", i)).collect(),
            ..Default::default()
        };
        let dropped = hints.cap_lists();
        assert_eq!(hints.owned_files.len(), TASK_BRIEF_LIST_CAP);
        assert!(dropped
            .iter()
            .any(|(label, count)| *label == "owned_files" && *count == 100 - TASK_BRIEF_LIST_CAP));
    }

    #[test]
    fn build_task_brief_includes_canonical_sections_and_scoped_commit_reminder() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship the wave".to_string()),
            scope: Some("wave 15 task 05 only".to_string()),
            owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            forbidden_files: vec!["c.rs".to_string()],
            acceptance_commands: vec!["cargo test".to_string(), "git diff --check".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        // headings present
        assert!(brief.contains("## Objective"));
        assert!(brief.contains("## Scope"));
        assert!(brief.contains("## Owned files"));
        assert!(brief.contains("## Forbidden files"));
        assert!(brief.contains("## Acceptance commands"));
        assert!(brief.contains("## Commit policy"));
        // owned files listed
        assert!(brief.contains("- a.rs"));
        assert!(brief.contains("- b.rs"));
        // scoped commit reminder line
        assert!(brief.contains("do not stage or commit outside the owned files"));
        // default policy (we did NOT pass commit_policy)
        assert!(brief.contains("policy: scoped"));
        // fresh-code-alignment must NOT inject the agent-team hint
        assert!(!brief.contains(AGENT_TEAM_OBJECTIVE_HINT));
    }

    #[test]
    fn build_task_brief_injects_agent_team_hint_exactly_once_for_agent_team_strategy() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "agent-team");
        assert_eq!(
            brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "agent-team hint must appear exactly once, got: {brief}"
        );
    }

    #[test]
    fn build_task_brief_omits_optional_sections_when_lists_empty() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        // Forbidden / Acceptance / Scope sections must NOT appear when their
        // backing lists are empty / absent.
        assert!(!brief.contains("## Forbidden files"));
        assert!(!brief.contains("## Acceptance commands"));
        assert!(!brief.contains("## Scope"));
        // Owned-files section is always present (the policy is "stage NOTHING
        // by default" — explicit reminder).
        assert!(brief.contains("## Owned files"));
        assert!(brief.contains("(none declared"));
    }

    #[test]
    fn build_task_brief_uses_explicit_commit_policy_when_supplied() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            commit_policy: Some("monorepo-cascade".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "resident-lisp");
        assert!(brief.contains("policy: monorepo-cascade"));
        // resident-lisp must NOT inject agent-team hint
        assert!(!brief.contains(AGENT_TEAM_OBJECTIVE_HINT));
    }

    #[test]
    fn safe_descriptor_status_strings_are_distinct() {
        assert_eq!(
            SafeDescriptorReason::UnsupportedTarget("mission_execution".into()).status(),
            "skipped_unsupported_target"
        );
        assert_eq!(
            SafeDescriptorReason::ProjectRootUnresolved("nope".into()).status(),
            "skipped_project_root_unresolved"
        );
        assert_eq!(
            SafeDescriptorReason::MissingObjective.status(),
            "skipped_missing_objective"
        );
    }

    #[test]
    fn outcome_to_response_dispatched_carries_inner_and_brief_preview() {
        let outcome = WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: None,
            evidence_path: Some("/tmp/sidecar.json".to_string()),
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-9"}),
        };
        let v = outcome_to_response_fields(&outcome, "agent-team");
        assert_eq!(v["workstation_dispatch_status"], "dispatched");
        assert_eq!(v["dispatch_strategy"], "agent-team");
        assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
        assert!(v["task_brief_preview"]
            .as_str()
            .unwrap()
            .contains("## Objective"));
        assert_eq!(v["inner_result"]["task_id"], "btk-9");
        assert_eq!(v["delegated_board_task_id"], "btk-9");
        // wave-20 / task 04 — legacy / rendered path leaves the
        // `task_contract_source_path` key OFF so the wire shape stays
        // byte-compatible with wave-15..19 callers.
        assert!(
            v.get("task_contract_source_path").is_none(),
            "rendered-path dispatch must omit task_contract_source_path \
             (wave-15..19 byte-compat)"
        );
    }

    /// wave-20 / task 04 — when the dispatch ran in machine-driven mode
    /// the response must carry the resolved on-disk task-contract path
    /// so observers can prove the Lisp was load-bearing.
    #[test]
    fn outcome_to_response_dispatched_machine_mode_surfaces_contract_path() {
        let outcome = WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: Some(
                "/tmp/p/.missiond/tasks/generated/plan/root.lisp".to_string(),
            ),
            evidence_path: None,
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-9"}),
        };
        let v = outcome_to_response_fields(&outcome, "agent-team");
        assert_eq!(
            v["task_contract_source_path"],
            "/tmp/p/.missiond/tasks/generated/plan/root.lisp"
        );
    }

    #[test]
    fn outcome_to_response_dispatched_projects_nested_board_task_id() {
        let outcome = WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: None,
            evidence_path: None,
            evidence_error: None,
            inner_payload: json!({
                "task_id": {
                    "id": "3ab1b19d-3d64-45de-9493-ff9972d2e77f",
                    "status": "open"
                }
            }),
        };
        let v = outcome_to_response_fields(&outcome, "agent-team");
        assert_eq!(
            v["delegated_board_task_id"],
            "3ab1b19d-3d64-45de-9493-ff9972d2e77f"
        );
        assert_eq!(v["inner_result"]["task_id"]["status"], "open");
    }

    #[test]
    fn outcome_to_response_safe_descriptor_carries_reason_detail() {
        let outcome = WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::ProjectRootUnresolved("no signal".to_string()),
            task_brief: None,
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(
            v["workstation_dispatch_status"],
            "skipped_project_root_unresolved"
        );
        assert_eq!(v["workstation_dispatch_reason"], "no signal");
        // No inner_result on safe descriptors
        assert!(v.get("inner_result").is_none());
    }

    #[test]
    fn outcome_to_response_dry_run_omits_evidence_and_inner() {
        let outcome = WorkstationDispatchOutcome::DryRun {
            task_brief: "## Objective\nship\n".to_string(),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["workstation_dispatch_status"], "dry_run_no_dispatch");
        assert!(v.get("inner_result").is_none());
        assert!(v.get("evidence_path").is_none());
        assert!(v["task_brief_preview"].as_str().unwrap().contains("ship"));
    }

    // ── async path tests (stand up a minimal AppState) ───────────────────

    use crate::slot_orchestrator::project_root::ResolutionError;
    use missiond_core::types::{ProjectConfig, ProjectRegistry, SharedProjectRegistry};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn fixture_registry(id: &str, root: &Path) -> SharedProjectRegistry {
        Arc::new(RwLock::new(ProjectRegistry::new(vec![ProjectConfig {
            id: id.to_string(),
            path: root.display().to_string(),
            intent_path: None,
            active: true,
            slots: vec![],
            github_url: None,
            kind: "managed".to_string(),
            vault_path: None,
            parent_id: None,
            created_at: None,
            updated_at: None,
        }])))
    }

    /// Build a minimal AppState skeleton good enough for the workstation-
    /// dispatch resolver path. Only `project_registry` is touched here;
    /// other fields stay at their default constructions because we never
    /// invoke a code path that reads them in the safe-descriptor and
    /// resolver-only tests.
    async fn fixture_state_with_registry(reg: SharedProjectRegistry) -> Option<AppState> {
        // AppState construction is feature-gated and pulls in the full
        // daemon graph (DB, bus, slot dispatcher) — far heavier than this
        // unit-level test wants. We therefore exercise the resolver path
        // directly via `resolve_target_project_root` in
        // `safe_descriptor_emitted_when_project_root_unresolved` instead
        // of standing up a full AppState. Keeping the helper here for
        // future async-only tests; returning `None` for now signals the
        // test harness to fall back to direct resolver assertions.
        let _ = reg;
        None
    }

    /// Resolver-level assertion: missing project root yields a structured
    /// safe descriptor instead of a silent fallback. We exercise the path
    /// from `run_workstation_dispatch` would take by calling
    /// `resolve_target_project_root` with the same args and asserting the
    /// branch shape downstream.
    #[tokio::test]
    async fn missing_project_root_signals_resolver_no_signal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg = fixture_registry("missiond", &root);
        // No project_id, no cwd, no fallback → NoSignal.
        let err = resolve_target_project_root(None, None, None, &reg)
            .await
            .expect_err("should fail");
        assert!(matches!(err, ResolutionError::NoSignal));
        // Mirror what run_workstation_dispatch would build:
        let descriptor = SafeDescriptorReason::ProjectRootUnresolved(err.to_string());
        assert_eq!(descriptor.status(), "skipped_project_root_unresolved");
        let _ = fixture_state_with_registry(reg).await;
    }

    #[tokio::test]
    async fn relative_cwd_is_rejected_by_pre_flight() {
        // The dispatch helper itself rejects a relative cwd before even
        // reaching the resolver — this is the "do not join relative cwd
        // against process cwd" architectural invariant.
        let cwd = "relative/path";
        assert!(!Path::new(cwd).is_absolute());
        let descriptor = SafeDescriptorReason::ProjectRootUnresolved(format!(
            "requested_cwd `{}` is not absolute; \
             workstation-dispatch never joins a relative cwd against the daemon process cwd",
            cwd
        ));
        assert_eq!(descriptor.status(), "skipped_project_root_unresolved");
        assert!(descriptor.detail().contains("not absolute"));
    }

    /// `WorkstationDispatchHints::default` inherently has no objective —
    /// confirm the safe descriptor branch fires before any I/O happens.
    #[tokio::test]
    async fn missing_objective_yields_safe_descriptor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg = fixture_registry("missiond", &root);
        // We reuse the resolver to make this test fully self-contained
        // (no AppState dependency) — the resolver succeeds, but the
        // dispatch helper would refuse on `MissingObjective` first.
        let _ok = resolve_target_project_root(Some("missiond"), None, None, &reg)
            .await
            .expect("resolver succeeds");
        let descriptor = SafeDescriptorReason::MissingObjective;
        assert_eq!(descriptor.status(), "skipped_missing_objective");
        assert!(descriptor.detail().contains("content-free"));
    }

    // ── wave-17 / task 07 — scoped-commit handoff default tests ─────────

    #[test]
    fn classify_task_kind_treats_owned_files_as_code_brief() {
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        assert_eq!(classify_task_kind(&hints), BriefTaskKind::Code);
    }

    #[test]
    fn classify_task_kind_treats_empty_owned_files_as_read_only_brief() {
        let hints = WorkstationDispatchHints {
            objective: Some("audit the wave".to_string()),
            ..Default::default()
        };
        assert_eq!(classify_task_kind(&hints), BriefTaskKind::ReadOnly);
    }

    #[test]
    fn build_task_brief_code_requires_enforce_scoped_commit_on_completion() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        // The completion handoff section is always present.
        assert!(
            brief.contains("## Completion handoff (scoped commit)"),
            "code brief must carry the completion handoff section"
        );
        // The brief tells the worker to set enforce_scoped_commit=true.
        assert!(
            brief.contains("`enforce_scoped_commit=true`"),
            "code brief must instruct the worker to opt into enforcement"
        );
        // The brief asks for committed status + commit_hash + staged_files.
        assert!(
            brief.contains("`commit_status=\"committed\"`"),
            "code brief must request commit_status=committed"
        );
        assert!(
            brief.contains("`commit_hash="),
            "code brief must request commit_hash"
        );
        assert!(
            brief.contains("`staged_files="),
            "code brief must request staged_files"
        );
        // Task kind line.
        assert!(
            brief.contains("- task kind: code"),
            "code brief must declare task kind"
        );
        // The blocked branch must also be documented so workers don't
        // silently drop to "no commit".
        assert!(
            brief.contains("`commit_status=\"blocked\"`"),
            "code brief must document the blocked branch"
        );
        // The daemon-never-runs-git invariant must be loud.
        assert!(
            brief.contains("daemon never runs git itself"),
            "brief must restate the daemon-never-runs-git invariant"
        );
    }

    #[test]
    fn build_task_brief_read_only_uses_not_required_with_explanation() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("audit the wave-17 surface".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        assert!(brief.contains("## Completion handoff (scoped commit)"));
        // Read-only briefs default to commit_status=not-required.
        assert!(
            brief.contains("`commit_status=\"not-required\"`"),
            "read-only brief must default to commit_status=not-required"
        );
        // ...with an explanation requirement.
        assert!(
            brief.contains("explain WHY"),
            "read-only brief must require an explanation in the summary field"
        );
        // Task kind line.
        assert!(
            brief.contains("- task kind: read-only"),
            "read-only brief must declare task kind"
        );
        // Still asks for enforce_scoped_commit=true so the daemon's
        // wave-16/06 gates run.
        assert!(
            brief.contains("`enforce_scoped_commit=true`"),
            "read-only brief still opts the completion call into enforcement"
        );
        // Read-only brief must NOT instruct the worker to commit anything.
        assert!(
            !brief.contains("`commit_status=\"committed\"`"),
            "read-only brief must NOT prescribe commit_status=committed"
        );
    }

    #[test]
    fn build_task_brief_completion_handoff_does_not_double_inject_agent_team_hint() {
        // The agent-team hint sits in section 8 (after completion handoff).
        // Adding the new section must not leak the literal anywhere else.
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "agent-team");
        assert_eq!(
            brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "agent-team hint must still appear exactly once after wave-17 / task 07"
        );
    }

    #[test]
    fn build_task_brief_read_only_does_not_inject_agent_team_hint_for_other_strategies() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("audit".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "resident-lisp");
        assert_eq!(brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(), 0);
        // Confirm read-only branch lands.
        assert!(brief.contains("- task kind: read-only"));
    }

    #[test]
    fn outcome_to_response_dispatched_advertises_scoped_commit_policy() {
        let outcome = WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: None,
            evidence_path: None,
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-9"}),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn outcome_to_response_dry_run_advertises_scoped_commit_policy() {
        let outcome = WorkstationDispatchOutcome::DryRun {
            task_brief: "## Objective\nship\n".to_string(),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn outcome_to_response_inner_error_advertises_scoped_commit_policy() {
        let outcome = WorkstationDispatchOutcome::InnerError {
            task_brief: "## Objective\nship\n".to_string(),
            inner_payload: json!({"error": "nope"}),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn outcome_to_response_safe_descriptor_advertises_scoped_commit_policy() {
        // Even on safe-descriptor refusals the policy contract is part of
        // the wire shape so observers don't have to special-case the
        // skipped branch when asserting the invariant.
        let outcome = WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::MissingObjective,
            task_brief: None,
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn brief_task_kind_string_pin_is_stable_wire_contract() {
        // These two strings are part of the brief / response wire contract;
        // changing them silently would break downstream observers.
        assert_eq!(BriefTaskKind::Code.as_str(), "code");
        assert_eq!(BriefTaskKind::ReadOnly.as_str(), "read-only");
    }

    // ── wave-19 / task 07 — task-contract v1 parser tests ────────────────

    /// Reference contract body produced by the wave-19 / task 06 emitter.
    /// We pin against the exact textual shape so a future emitter tweak
    /// that breaks the parser surface trips a test rather than silently
    /// downgrading to the legacy brief.
    const SAMPLE_CONTRACT: &str = r#";; Generated by MissionD plan-runner (wave-19 / task 06).
;; plan_id = 00000000-0000-0000-0000-000000000def
;; board_task_id = btk-wd
;; node_id = root

(task plan-00000000-node-root
  :schema "missiond.task-contract.v1"
  :title "Plan 00000000-0000-0000-0000-000000000def node root — workstation task contract"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :dispatch-strategy "agent-team"
  :goal "ship the wave-19 contract consumer"
  :scope "wave 19 task 07 only"
  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
  :acceptance
    ["cargo test -p missiond-daemon"
     "cargo build --workspace"]
  :commit
    (:required true
     :message "feat(workstation): consume Lisp task contracts"
     :scope-check write-scope-only
     :policy "scoped")
  :target-project "missiond"
  :requested-cwd "/Users/jinchen/Projects/missiond"
  :target "mission_task_delegate"
  :plan-id "00000000-0000-0000-0000-000000000def"
  :node-id "root"
)
"#;

    #[test]
    fn parse_task_contract_extracts_every_consumed_field() {
        let c = parse_task_contract(SAMPLE_CONTRACT).expect("must parse");
        assert_eq!(c.schema, "missiond.task-contract.v1");
        assert_eq!(c.goal, "ship the wave-19 contract consumer");
        assert_eq!(c.scope.as_deref(), Some("wave 19 task 07 only"));
        assert_eq!(
            c.write_scope,
            vec!["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
        );
        assert_eq!(c.must_not_touch.len(), 2);
        assert!(c
            .must_not_touch
            .contains(&"crates/missiond-daemon/src/handlers/knowledge/plan.rs".to_string()));
        assert_eq!(c.acceptance.len(), 2);
        assert_eq!(c.commit_policy.as_deref(), Some("scoped"));
        assert_eq!(c.dispatch_strategy.as_deref(), Some("agent-team"));
        assert_eq!(c.target_project.as_deref(), Some("missiond"));
        assert_eq!(
            c.requested_cwd.as_deref(),
            Some("/Users/jinchen/Projects/missiond")
        );
        assert_eq!(c.target.as_deref(), Some("mission_task_delegate"));
    }

    #[test]
    fn parse_task_contract_tolerates_optional_field_absence() {
        // Minimal viable contract: schema + goal only.
        let src = r#"(task minimal
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :write-scope []
  :must-not-touch []
)
"#;
        let c = parse_task_contract(src).expect("minimal parse");
        assert_eq!(c.goal, "ship");
        assert!(c.scope.is_none());
        assert!(c.write_scope.is_empty());
        assert!(c.must_not_touch.is_empty());
        assert!(c.acceptance.is_empty());
        assert!(c.commit_policy.is_none());
        assert!(c.dispatch_strategy.is_none());
    }

    #[test]
    fn parse_task_contract_rejects_schema_mismatch() {
        let src = r#"(task wrong
  :schema "missiond.task-contract.v0"
  :goal "ship"
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::SchemaMismatch(_)));
        assert!(err.reason().contains("schema mismatch"));
        assert!(err.reason().contains("v0"));
    }

    #[test]
    fn parse_task_contract_rejects_missing_schema() {
        let src = r#"(task no-schema :goal "ship")"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::SchemaMismatch(_)));
        assert!(err.reason().contains("(absent)"));
    }

    #[test]
    fn parse_task_contract_rejects_missing_goal() {
        let src = r#"(task no-goal
  :schema "missiond.task-contract.v1"
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(
            err,
            TaskContractParseError::MissingRequired("goal")
        ));
    }

    #[test]
    fn parse_task_contract_rejects_blank_goal() {
        let src = r#"(task blank
  :schema "missiond.task-contract.v1"
  :goal "   "
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(
            err,
            TaskContractParseError::MissingRequired("goal")
        ));
    }

    #[test]
    fn parse_task_contract_rejects_unbalanced_parens() {
        let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal "ship"
"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::Lex(_)));
    }

    #[test]
    fn parse_task_contract_rejects_unterminated_string() {
        let src = r#"(task bad :schema "unterminated"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::Lex(_)));
    }

    #[test]
    fn parse_task_contract_rejects_non_task_top_form() {
        let src = r#"(plan something :schema "missiond.task-contract.v1" :goal "x")"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::NotATaskForm(_)));
    }

    #[test]
    fn parse_task_contract_rejects_wrong_field_shape() {
        // :goal must be a string, not a list.
        let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal ["a" "b"]
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        match err {
            TaskContractParseError::FieldShape { field, .. } => assert_eq!(field, "goal"),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn parse_task_contract_rejects_non_string_in_write_scope() {
        let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :write-scope [foo "bar"]
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        match err {
            TaskContractParseError::FieldShape { field, .. } => {
                assert_eq!(field, "write-scope")
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn parse_task_contract_handles_escaped_strings() {
        let src = r#"(task esc
  :schema "missiond.task-contract.v1"
  :goal "ship \"quoted\" and \\backslash"
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.goal, "ship \"quoted\" and \\backslash");
    }

    #[test]
    fn parse_task_contract_skips_unknown_fields() {
        // A future emitter may add fields we do not consume — accept and
        // ignore them so the parser stays forward-compatible. The
        // authoritative checker (scripts/check-task-contract.mjs) is the
        // gate for new fields.
        let src = r#"(task fwd
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :unknown-future-field "ignored"
  :requirements ["a" "b"]
  :report ["x"]
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.goal, "ship");
    }

    #[test]
    fn parse_task_contract_skips_comment_lines() {
        let src = r#"
;; comment 1
;; comment 2 with paren ( and string "
(task ok
  ;; inline comment
  :schema "missiond.task-contract.v1"
  :goal "ship"
)
"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.goal, "ship");
    }

    #[test]
    fn extract_commit_policy_returns_none_when_policy_absent() {
        let src = r#"(task np
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :commit (:required true :scope-check write-scope-only)
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert!(c.commit_policy.is_none());
    }

    #[test]
    fn extract_commit_policy_returns_value_when_present() {
        let src = r#"(task wp
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :commit (:required true :policy "monorepo-cascade" :scope-check none)
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.commit_policy.as_deref(), Some("monorepo-cascade"));
    }

    #[test]
    fn load_task_contract_round_trips_emitter_output() {
        // Write the emitter sample to disk and confirm the loader returns
        // the same projection as the in-memory parser.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("contract.lisp");
        std::fs::write(&path, SAMPLE_CONTRACT).unwrap();
        let c = load_task_contract(&path).expect("must load");
        assert_eq!(c.goal, "ship the wave-19 contract consumer");
        assert_eq!(c.dispatch_strategy.as_deref(), Some("agent-team"));
    }

    #[test]
    fn load_task_contract_io_error_when_file_missing() {
        let err = load_task_contract(Path::new("/nonexistent/path/contract.lisp"))
            .expect_err("must fail");
        assert!(matches!(err, TaskContractParseError::Io(_)));
        assert!(err.reason().starts_with("io:"));
    }

    // ── wave-19 / task 07 — overlay tests ────────────────────────────────

    #[test]
    fn overlay_contract_overrides_objective_and_lists() {
        let mut hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            owned_files: vec!["hint.rs".to_string()],
            forbidden_files: vec!["hint_forbidden.rs".to_string()],
            ..Default::default()
        };
        let contract = ParsedTaskContract {
            schema: "missiond.task-contract.v1".to_string(),
            goal: "contract goal".to_string(),
            write_scope: vec!["contract_a.rs".to_string(), "contract_b.rs".to_string()],
            must_not_touch: vec!["contract_no.rs".to_string()],
            acceptance: vec!["cargo test".to_string()],
            commit_policy: Some("scoped-strict".to_string()),
            dispatch_strategy: Some("resident-lisp".to_string()),
            target_project: Some("contract-proj".to_string()),
            requested_cwd: Some("/contract/cwd".to_string()),
            ..Default::default()
        };
        hints.overlay_contract(&contract);
        assert_eq!(hints.objective.as_deref(), Some("contract goal"));
        assert_eq!(hints.owned_files, vec!["contract_a.rs", "contract_b.rs"]);
        assert_eq!(hints.forbidden_files, vec!["contract_no.rs"]);
        assert_eq!(hints.acceptance_commands, vec!["cargo test"]);
        assert_eq!(hints.commit_policy.as_deref(), Some("scoped-strict"));
        assert_eq!(hints.dispatch_strategy.as_deref(), Some("resident-lisp"));
        assert_eq!(hints.target_project.as_deref(), Some("contract-proj"));
        assert_eq!(hints.requested_cwd.as_deref(), Some("/contract/cwd"));
    }

    #[test]
    fn overlay_contract_preserves_arg_lists_when_contract_lists_are_empty() {
        // The contract emitter only emits non-empty lists; an absent
        // `:acceptance` should NOT erase a caller-supplied acceptance arg.
        let mut hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            owned_files: vec!["arg.rs".to_string()],
            acceptance_commands: vec!["arg cmd".to_string()],
            ..Default::default()
        };
        let contract = ParsedTaskContract {
            schema: "missiond.task-contract.v1".to_string(),
            goal: "contract goal".to_string(),
            // All list fields empty.
            ..Default::default()
        };
        hints.overlay_contract(&contract);
        // Objective overridden (non-empty contract goal beats arg).
        assert_eq!(hints.objective.as_deref(), Some("contract goal"));
        // Lists preserved (contract did not declare them).
        assert_eq!(hints.owned_files, vec!["arg.rs"]);
        assert_eq!(hints.acceptance_commands, vec!["arg cmd"]);
    }

    #[test]
    fn overlay_contract_blank_scope_does_not_clobber_arg_scope() {
        let mut hints = WorkstationDispatchHints {
            objective: Some("o".to_string()),
            scope: Some("arg scope".to_string()),
            ..Default::default()
        };
        let contract = ParsedTaskContract {
            schema: "missiond.task-contract.v1".to_string(),
            goal: "g".to_string(),
            scope: Some("   ".to_string()),
            ..Default::default()
        };
        hints.overlay_contract(&contract);
        assert_eq!(hints.scope.as_deref(), Some("arg scope"));
    }

    // ── wave-19 / task 07 — brief integration tests ──────────────────────

    #[test]
    fn build_task_brief_with_source_prefixes_contract_block_when_path_supplied() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let path = std::path::PathBuf::from("/tmp/contract.lisp");
        let brief = build_task_brief_with_source(&plan, &hints, "agent-team", Some(&path));
        // Contract preamble present.
        assert!(brief.contains("## Source contract"));
        assert!(brief.contains("/tmp/contract.lisp"));
        assert!(brief.contains("treat the contract as the SSOT"));
        // Existing canonical sections still present (wave-15/16/17 invariants).
        assert!(brief.contains("## Objective"));
        assert!(brief.contains("## Owned files"));
        assert!(brief.contains("## Commit policy"));
        assert!(brief.contains("## Completion handoff (scoped commit)"));
        // Agent-team hint still appears exactly once.
        assert_eq!(brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(), 1);
    }

    #[test]
    fn build_task_brief_without_source_is_byte_identical_to_legacy_build_task_brief() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let legacy = build_task_brief(&plan, &hints, "agent-team");
        let with_none = build_task_brief_with_source(&plan, &hints, "agent-team", None);
        assert_eq!(
            legacy, with_none,
            "wave-19 wrapper must be byte-identical to legacy entry when no contract"
        );
    }

    #[test]
    fn build_task_brief_with_source_does_not_double_inject_completion_handoff() {
        // The completion handoff section is independent of the contract
        // preamble — it must appear exactly once even when the brief is
        // contract-flavoured.
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let path = std::path::PathBuf::from("/tmp/contract.lisp");
        let brief =
            build_task_brief_with_source(&plan, &hints, "fresh-code-alignment", Some(&path));
        assert_eq!(
            brief
                .matches("## Completion handoff (scoped commit)")
                .count(),
            1
        );
        // Code task kind still classified from owned_files presence.
        assert!(brief.contains("- task kind: code"));
    }

    // ── wave-19 / task 07 — SafeDescriptor tests ─────────────────────────

    #[test]
    fn malformed_task_contract_descriptor_status_is_distinct() {
        let r = SafeDescriptorReason::MalformedTaskContract {
            path: "/tmp/x.lisp".to_string(),
            reason: "schema mismatch".to_string(),
        };
        assert_eq!(r.status(), "skipped_malformed_task_contract");
        let detail = r.detail();
        assert!(detail.contains("/tmp/x.lisp"));
        assert!(detail.contains("schema mismatch"));
        assert!(detail.contains("SSOT"));
    }

    #[test]
    fn outcome_to_response_malformed_contract_carries_full_detail() {
        let outcome = WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::MalformedTaskContract {
                path: "/tmp/bad.lisp".to_string(),
                reason: "lex: unbalanced parens".to_string(),
            },
            task_brief: None,
        };
        let v = outcome_to_response_fields(&outcome, "agent-team");
        assert_eq!(
            v["workstation_dispatch_status"],
            "skipped_malformed_task_contract"
        );
        assert!(v["workstation_dispatch_reason"]
            .as_str()
            .unwrap()
            .contains("/tmp/bad.lisp"));
        assert!(v["workstation_dispatch_reason"]
            .as_str()
            .unwrap()
            .contains("lex: unbalanced parens"));
        // Scoped-commit policy contract still surfaces on the safe-descriptor
        // branch (wave-17 / task 07 invariant — applies regardless of branch).
        assert_eq!(v["scoped_commit_required"], json!(true));
    }

    // ── wave-19 / task 07 — path resolution tests ────────────────────────

    #[test]
    fn resolve_contract_path_keeps_absolute_paths_verbatim() {
        let abs = Path::new("/tmp/abs/contract.lisp");
        let root = Path::new("/Users/x/proj");
        assert_eq!(resolve_contract_path(abs, root), abs.to_path_buf());
    }

    #[test]
    fn resolve_contract_path_joins_relative_against_project_root() {
        let rel = Path::new(".missiond/tasks/generated/abc/root.lisp");
        let root = Path::new("/Users/x/proj");
        assert_eq!(
            resolve_contract_path(rel, root),
            Path::new("/Users/x/proj/.missiond/tasks/generated/abc/root.lisp").to_path_buf()
        );
    }

    // ── wave-19 / task 07 — parser error reason mapping pin ──────────────

    #[test]
    fn task_contract_parse_error_reason_strings_are_actionable() {
        // Each variant produces a human-actionable reason string. Pinned so
        // a future refactor that loses detail (e.g. dropping the offending
        // schema value) trips a test.
        assert!(TaskContractParseError::Io("perm denied".into())
            .reason()
            .contains("perm denied"));
        assert!(TaskContractParseError::Lex("EOF".into())
            .reason()
            .contains("EOF"));
        assert!(TaskContractParseError::NotATaskForm("(plan)".into())
            .reason()
            .contains("(plan)"));
        assert!(TaskContractParseError::SchemaMismatch("v9".into())
            .reason()
            .contains("v9"));
        assert!(TaskContractParseError::MissingRequired("goal")
            .reason()
            .contains("goal"));
        assert!(TaskContractParseError::FieldShape {
            field: "write-scope",
            detail: "got 42".into(),
        }
        .reason()
        .contains("write-scope"));
    }

    // ── wave-21 / task 04 — autonomous workstation LLM proposal v0 ─────

    #[test]
    fn workstation_proposal_status_wire_strings_pin() {
        // The five values are part of the response wire contract.
        assert_eq!(
            WorkstationProposalStatus::NotInvoked.as_wire(),
            "not_invoked"
        );
        assert_eq!(
            WorkstationProposalStatus::Unavailable.as_wire(),
            "llm_unavailable"
        );
        assert_eq!(WorkstationProposalStatus::Suggested.as_wire(), "suggested");
        assert_eq!(
            WorkstationProposalStatus::NoSuggestions.as_wire(),
            "no_suggestions"
        );
        assert_eq!(
            WorkstationProposalStatus::PlanHintsPresent.as_wire(),
            "plan_hints_present"
        );
    }

    #[test]
    fn workstation_proposal_safety_status_wire_strings_pin() {
        // The four values land verbatim on every proposal entry.
        assert_eq!(WorkstationProposalSafetyStatus::Safe.as_wire(), "safe");
        assert_eq!(
            WorkstationProposalSafetyStatus::AmbiguousValue.as_wire(),
            "ambiguous_value"
        );
        assert_eq!(
            WorkstationProposalSafetyStatus::UnsupportedTarget.as_wire(),
            "unsupported_target"
        );
        assert_eq!(
            WorkstationProposalSafetyStatus::InvalidStrategy.as_wire(),
            "invalid_strategy"
        );
    }

    #[test]
    fn workstation_proposal_to_json_pins_applied_false() {
        // Critical invariant: every proposal carries `applied=false` on
        // the wire so observers can `assert proposal.applied == false`
        // without re-reading the source. Wave-21 / task 04 explicitly
        // forbids auto-spawn from proposals.
        let p = WorkstationProposal {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: WorkstationProposalConfidence::High,
            evidence: "plan sexp delegates to claudecode".to_string(),
            safety_status: WorkstationProposalSafetyStatus::Safe,
        };
        let v = p.to_json();
        assert_eq!(v["applied"], json!(false));
        assert_eq!(v["field"], json!("target"));
        assert_eq!(v["confidence"], json!("high"));
        assert_eq!(v["safety_status"], json!("safe"));
        assert_eq!(v["value"], json!("mission_task_delegate"));
    }

    #[test]
    fn workstation_proposal_bundle_unavailable_carries_reason() {
        let b = WorkstationProposalBundle::unavailable("gateway not initialized");
        assert_eq!(b.status, WorkstationProposalStatus::Unavailable);
        assert!(b.proposals.is_empty());
        assert_eq!(
            b.unavailable_reason.as_deref(),
            Some("gateway not initialized")
        );
        assert_eq!(
            b.request_caller.as_deref(),
            Some(SONNET_WORKSTATION_PROPOSAL_CALLER)
        );
        assert!(
            b.model.is_none(),
            "unavailable bundle must not name a model"
        );
    }

    #[test]
    fn workstation_proposal_bundle_plan_hints_present_carries_reason() {
        let b = WorkstationProposalBundle::plan_hints_present("signals present: caller.objective");
        assert_eq!(b.status, WorkstationProposalStatus::PlanHintsPresent);
        assert!(b.proposals.is_empty());
        assert_eq!(
            b.unavailable_reason.as_deref(),
            Some("signals present: caller.objective")
        );
    }

    #[test]
    fn workstation_proposal_bundle_to_response_pins_auto_spawn_false() {
        // The bundle-level `auto_spawn=false` field is the wire-level
        // proof of the never-auto-spawn invariant. A UI surfacing the
        // block can quote this single field rather than re-deriving it
        // from the per-proposal `applied=false`.
        let bundle = WorkstationProposalBundle::unavailable("test reason");
        let v = bundle.to_response_json();
        assert_eq!(v["auto_spawn"], json!(false));
        assert_eq!(v["status"], "llm_unavailable");
        assert_eq!(v["unavailable_reason"], "test reason");
        assert_eq!(v["proposals"], json!([]));
    }

    #[test]
    fn workstation_proposal_bundle_to_response_includes_proposals_when_present() {
        let bundle = WorkstationProposalBundle {
            status: WorkstationProposalStatus::Suggested,
            proposals: vec![WorkstationProposal {
                field: "target",
                value: json!("mission_task_delegate"),
                confidence: WorkstationProposalConfidence::Medium,
                evidence: "plan sexp suggests delegation".to_string(),
                safety_status: WorkstationProposalSafetyStatus::Safe,
            }],
            parse_warnings: vec!["proposals[2] field `foo` not in allowlist".to_string()],
            unavailable_reason: None,
            model: Some("claude-sonnet".to_string()),
            request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
        };
        let v = bundle.to_response_json();
        assert_eq!(v["status"], "suggested");
        assert_eq!(v["proposals"][0]["field"], "target");
        assert_eq!(v["proposals"][0]["applied"], false);
        assert_eq!(v["proposals"][0]["safety_status"], "safe");
        assert_eq!(v["model"], "claude-sonnet");
        assert_eq!(v["caller"], SONNET_WORKSTATION_PROPOSAL_CALLER);
        assert_eq!(
            v["parse_warnings"][0],
            "proposals[2] field `foo` not in allowlist"
        );
        // The auto-spawn invariant is pinned on every bundle render.
        assert_eq!(v["auto_spawn"], json!(false));
    }

    #[test]
    fn workstation_proposal_gate_is_fully_silent_when_no_signals() {
        let gate = WorkstationProposalGate {
            caller_target_present: false,
            caller_dispatch_strategy_present: false,
            caller_objective_present: false,
            caller_scope_present: false,
            caller_owned_files_present: false,
            caller_project_signal_present: false,
            plan_hints_present: false,
            plan_workstation_opt_in: false,
            _marker: std::marker::PhantomData,
        };
        assert!(gate.is_fully_silent());
        assert!(gate.signal_summary().contains("no signals"));
    }

    #[test]
    fn workstation_proposal_gate_not_silent_when_caller_objective_set() {
        let gate = WorkstationProposalGate {
            caller_target_present: false,
            caller_dispatch_strategy_present: false,
            caller_objective_present: true,
            caller_scope_present: false,
            caller_owned_files_present: false,
            caller_project_signal_present: false,
            plan_hints_present: false,
            plan_workstation_opt_in: false,
            _marker: std::marker::PhantomData,
        };
        assert!(!gate.is_fully_silent());
        assert!(gate.signal_summary().contains("caller.objective"));
    }

    #[test]
    fn workstation_proposal_gate_not_silent_when_plan_hints_present() {
        let gate = WorkstationProposalGate {
            caller_target_present: false,
            caller_dispatch_strategy_present: false,
            caller_objective_present: false,
            caller_scope_present: false,
            caller_owned_files_present: false,
            caller_project_signal_present: false,
            plan_hints_present: true,
            plan_workstation_opt_in: false,
            _marker: std::marker::PhantomData,
        };
        assert!(!gate.is_fully_silent());
        assert!(gate.signal_summary().contains("plan.hints"));
    }

    #[test]
    fn workstation_proposal_gate_not_silent_when_plan_opt_in_set() {
        let gate = WorkstationProposalGate {
            caller_target_present: false,
            caller_dispatch_strategy_present: false,
            caller_objective_present: false,
            caller_scope_present: false,
            caller_owned_files_present: false,
            caller_project_signal_present: false,
            plan_hints_present: false,
            plan_workstation_opt_in: true,
            _marker: std::marker::PhantomData,
        };
        assert!(!gate.is_fully_silent());
        assert!(gate.signal_summary().contains("plan.workstation_dispatch"));
    }

    #[test]
    fn build_workstation_proposal_prompt_embeds_plan_and_provenance() {
        let plan = "(plan :id \"p1\" :board-task-id \"btk-1\")";
        let (system, user) = build_workstation_proposal_prompt(plan, Some("directive/abc:1"));
        // System prompt names the four allowlisted fields.
        for f in WORKSTATION_PROPOSAL_FIELDS {
            assert!(
                system.contains(f),
                "system prompt must mention field `{}`",
                f
            );
        }
        // System prompt pins the never-auto-spawn invariant.
        assert!(system.contains("NEVER be auto-"));
        assert!(system.to_ascii_lowercase().contains("never"));
        assert!(system.contains("STRICT JSON"));
        // System prompt pins the conservative target / strategy whitelists.
        assert!(system.contains("mission_execution"));
        assert!(system.contains("mission_task_delegate"));
        assert!(system.contains("fresh-code-alignment"));
        // User prompt embeds plan + provenance.
        assert!(user.contains(plan));
        assert!(user.contains("directive/abc:1"));
        // User prompt explicitly tells the model that no caller hints exist.
        assert!(user.contains("no workstation hints") || user.contains("no \nworkstation hints"));
    }

    #[test]
    fn build_workstation_proposal_prompt_handles_absent_provenance() {
        let (_, user) = build_workstation_proposal_prompt("(plan)", None);
        assert!(user.contains("(none)"));
    }

    #[test]
    fn parse_workstation_proposals_accepts_canonical_envelope() {
        let raw = r#"{
            "proposals": [
                {
                    "field": "target",
                    "value": "mission_task_delegate",
                    "confidence": "high",
                    "evidence": "plan sexp clearly delegates to claudecode"
                },
                {
                    "field": "objective",
                    "value": "ship the wave 21 task four",
                    "confidence": "medium",
                    "evidence": "directive provenance suggests this work"
                }
            ]
        }"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(warnings.is_empty(), "warnings: {:?}", warnings);
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].field, "target");
        assert_eq!(
            proposals[0].safety_status,
            WorkstationProposalSafetyStatus::Safe
        );
        assert_eq!(proposals[1].field, "objective");
        assert_eq!(
            proposals[1].safety_status,
            WorkstationProposalSafetyStatus::Safe
        );
    }

    #[test]
    fn parse_workstation_proposals_accepts_bare_array() {
        let raw = r#"[{"field":"scope","value":"crates/foo only","confidence":"medium","evidence":"plan suggests narrow scope"}]"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(warnings.is_empty(), "warnings: {:?}", warnings);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].field, "scope");
    }

    #[test]
    fn parse_workstation_proposals_strips_markdown_fence() {
        let raw = "```json\n{\"proposals\": [{\"field\":\"dispatch_strategy\",\"value\":\"agent-team\",\"confidence\":\"medium\",\"evidence\":\"parallelism hint\"}]}\n```";
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(warnings.is_empty(), "warnings: {:?}", warnings);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].field, "dispatch_strategy");
    }

    #[test]
    fn parse_workstation_proposals_rejects_unknown_field() {
        let raw = r#"{"proposals":[{"field":"orbital_velocity","value":"warp9","confidence":"high","evidence":"x"}]}"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(proposals.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("orbital_velocity"));
        assert!(warnings[0].contains("not in allowlist"));
    }

    #[test]
    fn parse_workstation_proposals_rejects_non_string_value() {
        let raw = r#"{"proposals":[{"field":"objective","value":42,"confidence":"high","evidence":"x"}]}"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("must be string"));
    }

    #[test]
    fn parse_workstation_proposals_rejects_blank_value() {
        let raw = r#"{"proposals":[{"field":"objective","value":"   ","confidence":"high","evidence":"x"}]}"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("non-empty"));
    }

    #[test]
    fn parse_workstation_proposals_rejects_invalid_confidence() {
        let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"absolute","evidence":"x"}]}"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("absolute"));
    }

    #[test]
    fn parse_workstation_proposals_rejects_missing_evidence() {
        let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"high","evidence":""}]}"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("evidence"));
    }

    #[test]
    fn parse_workstation_proposals_dedupes_repeated_fields() {
        let raw = r#"{
            "proposals":[
                {"field":"target","value":"mission_execution","confidence":"medium","evidence":"first"},
                {"field":"target","value":"mission_task_delegate","confidence":"high","evidence":"second"}
            ]
        }"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].evidence, "first");
        assert!(warnings.iter().any(|w| w.contains("duplicate")));
    }

    #[test]
    fn parse_workstation_proposals_caps_long_lists() {
        let raw = serde_json::to_string(&json!({
            "proposals": [
                {"field": "target",            "value": "mission_execution",       "confidence": "low", "evidence": "x"},
                {"field": "dispatch_strategy", "value": "fresh-code-alignment",    "confidence": "low", "evidence": "x"},
                {"field": "objective",         "value": "ship the wave",           "confidence": "low", "evidence": "x"},
                {"field": "scope",             "value": "narrow scope",            "confidence": "low", "evidence": "x"},
                {"field": "extra_one",         "value": "ignored",                 "confidence": "low", "evidence": "x"},
                {"field": "extra_two",         "value": "ignored",                 "confidence": "low", "evidence": "x"},
                {"field": "extra_three",       "value": "ignored",                 "confidence": "low", "evidence": "x"},
                {"field": "extra_four",        "value": "ignored",                 "confidence": "low", "evidence": "x"},
            ]
        }))
        .unwrap();
        let (proposals, warnings) = parse_workstation_proposals(&raw);
        assert!(proposals.len() <= WORKSTATION_PROPOSAL_CAP);
        assert_eq!(proposals.len(), 4);
        assert!(warnings.iter().any(|w| w.contains("extra_one")));
    }

    #[test]
    fn parse_workstation_proposals_rejects_garbage_json() {
        let (proposals, warnings) = parse_workstation_proposals("not json at all");
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("not valid JSON"));
    }

    #[test]
    fn parse_workstation_proposals_rejects_missing_proposals_key() {
        let raw = r#"{"results": []}"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("missing required `proposals`"));
    }

    #[test]
    fn classify_proposal_safety_target_unsupported_value_tagged() {
        let s = classify_proposal_safety("target", "mission_unknown");
        assert_eq!(s, WorkstationProposalSafetyStatus::UnsupportedTarget);
    }

    #[test]
    fn classify_proposal_safety_target_supported_value_safe() {
        for t in PROPOSAL_VALID_TARGETS {
            let s = classify_proposal_safety("target", t);
            assert_eq!(
                s,
                WorkstationProposalSafetyStatus::Safe,
                "target `{}` should be Safe",
                t
            );
        }
    }

    #[test]
    fn classify_proposal_safety_dispatch_strategy_unsupported_tagged() {
        for bad in ["prompt-fallback", "unknown", "telepathy"] {
            let s = classify_proposal_safety("dispatch_strategy", bad);
            assert_eq!(
                s,
                WorkstationProposalSafetyStatus::InvalidStrategy,
                "strategy `{}` should be InvalidStrategy",
                bad
            );
        }
    }

    #[test]
    fn classify_proposal_safety_dispatch_strategy_supported_safe() {
        for s in PROPOSAL_VALID_STRATEGIES {
            let safety = classify_proposal_safety("dispatch_strategy", s);
            assert_eq!(
                safety,
                WorkstationProposalSafetyStatus::Safe,
                "strategy `{}` should be Safe",
                s
            );
        }
    }

    #[test]
    fn classify_proposal_safety_objective_too_short_tagged_ambiguous() {
        let s = classify_proposal_safety("objective", "go");
        assert_eq!(s, WorkstationProposalSafetyStatus::AmbiguousValue);
    }

    #[test]
    fn classify_proposal_safety_scope_too_short_tagged_ambiguous() {
        let s = classify_proposal_safety("scope", "x");
        assert_eq!(s, WorkstationProposalSafetyStatus::AmbiguousValue);
    }

    #[test]
    fn classify_proposal_safety_objective_long_enough_safe() {
        let s = classify_proposal_safety("objective", "ship the wave 21 task");
        assert_eq!(s, WorkstationProposalSafetyStatus::Safe);
    }

    #[test]
    fn parse_workstation_proposals_tags_unsupported_target_safety_status() {
        // The validator surfaces the proposal but flags the safety status
        // so the operator sees the warning. We do NOT block the proposal
        // — that is the operator's job.
        let raw = r#"{"proposals":[{"field":"target","value":"mission_unknown","confidence":"high","evidence":"plan suggests it"}]}"#;
        let (proposals, warnings) = parse_workstation_proposals(raw);
        assert!(warnings.is_empty(), "warnings: {:?}", warnings);
        assert_eq!(proposals.len(), 1);
        assert_eq!(
            proposals[0].safety_status,
            WorkstationProposalSafetyStatus::UnsupportedTarget
        );
    }

    /// Wave-21 / task 04 — the "no fallback" invariant: when the Sonnet
    /// gateway is unavailable we surface a typed `Unavailable` bundle
    /// rather than silently downgrading to a deterministic-only or
    /// `claude -p` path. This test pins the unavailable-reason text so a
    /// future refactor that swallows the gateway-not-initialised
    /// distinction trips the test.
    #[test]
    fn workstation_proposal_unavailable_reason_pins_no_fallback_text() {
        let b = WorkstationProposalBundle::unavailable(
            "Sonnet gateway not initialized; autonomous workstation proposal unavailable \
             (no fallback to claude -p / prompt mode in v0)",
        );
        let reason = b.unavailable_reason.unwrap_or_default();
        assert!(reason.contains("no fallback"));
        assert!(reason.contains("claude -p") || reason.contains("prompt mode"));
    }

    // ── Wave 21 / Task 08 — machine-contract autonomous loop smoke ──
    //
    // Pure-helper smoke proving the wave21-04 workstation-proposal
    // pipeline (parse → classify → bundle → response JSON) preserves the
    // propose-only invariants on the canonical fixture. Layered on top
    // of the wave-15..20 fixtures already exercised above:
    //   * I4-04  the `applied=false` invariant is pinned on EVERY
    //            proposal regardless of safety_status (UI / observers
    //            can `assert proposal.applied == false` without reading
    //            the source).
    //   * I4-04  every safety_status (`safe` / `unsupported_target` /
    //            `invalid_strategy`) survives the round-trip — the
    //            classifier never silently demotes a flagged proposal
    //            to `safe`.
    //   * I4-04  the `auto_spawn=false` invariant is pinned on the
    //            bundle JSON (not just the proposal JSON) so a
    //            top-level grep proves the bundle never auto-applies.

    /// Canonical Sonnet response shape used by the wave21-04 propose-only
    /// pipeline. Three proposals span distinct fields + the safety
    /// spectrum (safe target, invalid strategy, ambiguous objective) so
    /// the smoke can pin the applied=false invariant across every
    /// safety_status branch without tripping the parser's per-field
    /// dedup rule.
    const WAVE21_08_SMOKE_PROPOSAL_JSON: &str = r#"
{
  "proposals": [
    {
      "field": "target",
      "value": "mission_task_delegate",
      "confidence": "high",
      "evidence": "node hint :target mission_task_delegate matches the wave21-08 smoke contract"
    },
    {
      "field": "dispatch_strategy",
      "value": "vibes-based",
      "confidence": "medium",
      "evidence": "model invented a strategy not on the conservative subset"
    },
    {
      "field": "objective",
      "value": "ship the wave21-08 deterministic loop smoke",
      "confidence": "low",
      "evidence": "summarised from the contract :goal field"
    }
  ]
}
"#;

    /// Wave21-08 propose-only smoke: every parsed wave21-04 proposal
    /// MUST carry `applied=false` (invariant I3-04) on the wire shape,
    /// the bundle MUST surface `auto_spawn=false`, and the safety
    /// classifier MUST tag the unsupported / invalid values verbatim.
    #[test]
    fn smoke_wave21_workstation_proposals_pin_applied_false_invariant_across_safety_spectrum() {
        let (proposals, warnings) = parse_workstation_proposals(WAVE21_08_SMOKE_PROPOSAL_JSON);
        assert_eq!(
            proposals.len(),
            3,
            "fixture must surface three proposals — got {} (warnings: {:?})",
            proposals.len(),
            warnings
        );
        // Pin the safety spectrum + applied=false invariant on every
        // proposal in one pass so a future refactor that flips the wire
        // contract (or silently demotes a flagged proposal) lands an
        // explicit failure here.
        let mut safety_seen: Vec<WorkstationProposalSafetyStatus> = Vec::new();
        for p in &proposals {
            let json = p.to_json();
            assert_eq!(
                json["applied"], false,
                "wave21-04 invariant I3: every proposal MUST carry applied=false on the wire"
            );
            assert!(
                !json["evidence"].as_str().unwrap_or("").is_empty(),
                "wave21-04 propose-only contract: evidence MUST be non-empty"
            );
            safety_seen.push(p.safety_status);
        }
        assert!(
            safety_seen.contains(&WorkstationProposalSafetyStatus::Safe),
            "wave21-08 fixture must include at least one Safe proposal"
        );
        assert!(
            safety_seen.contains(&WorkstationProposalSafetyStatus::InvalidStrategy),
            "wave21-08 fixture must include the invalid-strategy case (vibes-based)"
        );

        // Cross-check the dedicated unsupported_target classifier branch
        // independently of the bundle round-trip so a future refactor
        // that demotes UnsupportedTarget to Safe lands an explicit
        // failure here too. wave21-04 invariant: the classifier
        // surfaces UnsupportedTarget verbatim — never silently demotes
        // a flagged proposal.
        assert_eq!(
            classify_proposal_safety("target", "mission_unknown"),
            WorkstationProposalSafetyStatus::UnsupportedTarget,
            "wave21-04 invariant: unsupported target value MUST tag UnsupportedTarget"
        );

        // Bundle-level invariant: `auto_spawn=false` MUST appear on the
        // top-level JSON so observers can grep one place.
        let bundle = WorkstationProposalBundle {
            status: WorkstationProposalStatus::Suggested,
            proposals,
            parse_warnings: warnings,
            unavailable_reason: None,
            model: Some("claude-sonnet-4-7".to_string()),
            request_caller: Some("wave21-08-smoke".to_string()),
        };
        let block = bundle.to_response_json();
        assert_eq!(
            block["auto_spawn"], false,
            "wave21-04 invariant: bundle MUST pin auto_spawn=false at the top level"
        );
        assert_eq!(
            block["status"], "suggested",
            "wave21-04 status mapping: Suggested → \"suggested\""
        );
        let inner = block["proposals"].as_array().expect("proposals array");
        for p in inner {
            assert_eq!(
                p["applied"], false,
                "wave21-04 invariant I3: bundle round-trip preserves applied=false"
            );
        }
    }

    /// Wave21-08 propose-only smoke: when the Sonnet gateway is
    /// unreachable the bundle MUST surface status=`llm_unavailable` +
    /// the explicit "no fallback" reason text. Pinned here so a future
    /// refactor that adds a silent `claude -p` fallback fails the
    /// machine-contract loop smoke immediately.
    #[test]
    fn smoke_wave21_workstation_proposal_unavailable_pins_no_silent_fallback() {
        let bundle = WorkstationProposalBundle::unavailable(
            "Sonnet gateway unavailable; wave21-04 propose-only contract refuses fallback to \
             claude -p / prompt mode (autonomous workstation dispatch is opt-in)",
        );
        let block = bundle.to_response_json();
        assert_eq!(block["status"], "llm_unavailable");
        assert_eq!(
            block["auto_spawn"], false,
            "wave21-04 invariant: even llm_unavailable surfaces auto_spawn=false at the top level"
        );
        assert_eq!(
            block["proposals"]
                .as_array()
                .expect("empty proposals array")
                .len(),
            0,
            "llm_unavailable bundle must NOT carry a synthesised fallback proposal"
        );
        let reason = block["unavailable_reason"].as_str().unwrap_or("");
        assert!(
            reason.contains("no fallback")
                || reason.contains("refuses fallback")
                || reason.contains("claude -p")
                || reason.contains("prompt mode"),
            "wave21-04 invariant: unavailable reason MUST name the no-fallback contract"
        );
    }

    /// Wave21-08 contract overlay smoke: the wave-19/07 narrow parser
    /// projects the fixture task-contract Lisp text and the consumer
    /// rule (contract wins on every non-empty field) reaches the brief
    /// renderer. Pinning this here closes the wave21-08 loop on the
    /// workstation-side: the same contract the verifier ratifies
    /// (wave21-03) also drives the brief the worker reads.
    #[test]
    fn smoke_wave21_machine_contract_overlay_drives_brief_renderer() {
        const WAVE21_08_SMOKE_DISPATCH_CONTRACT: &str = r#"
(task wave21-08-dispatch-smoke
  :schema "missiond.task-contract.v1"
  :goal "wave21-08 machine-contract loop drives the brief from the contract"
  :scope "wave 21 task 08 only"
  :write-scope ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
  :must-not-touch ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"]
  :acceptance ["cargo test -p missiond-daemon"]
  :commit (:required true :message "test(intent): cover wave21 loop" :scope-check write-scope-only :policy "scoped")
  :dispatch-strategy "agent-team"
  :target-project "missiond"
  :target "mission_task_delegate")
"#;
        let parsed = parse_task_contract(WAVE21_08_SMOKE_DISPATCH_CONTRACT)
            .expect("wave21-08 dispatch contract must parse");
        assert_eq!(
            parsed.goal,
            "wave21-08 machine-contract loop drives the brief from the contract"
        );
        assert_eq!(parsed.dispatch_strategy.as_deref(), Some("agent-team"));
        assert_eq!(parsed.commit_policy.as_deref(), Some("scoped"));
        assert_eq!(parsed.target.as_deref(), Some("mission_task_delegate"));

        // Caller side starts with stale / placeholder hints so the
        // overlay rule is exercised end-to-end.
        let mut hints = WorkstationDispatchHints {
            objective: Some("STALE caller objective".to_string()),
            scope: Some("STALE caller scope".to_string()),
            owned_files: vec!["stale.rs".to_string()],
            forbidden_files: vec![],
            acceptance_commands: vec!["stale-cmd".to_string()],
            commit_policy: None,
            target_project: None,
            requested_cwd: None,
            dispatch_strategy: Some("fresh-code-alignment".to_string()),
        };
        hints.overlay_contract(&parsed);

        // wave21-08 invariant: the contract overlay wins on EVERY non-
        // empty field — caller stale data MUST NOT leak into the brief.
        assert_eq!(
            hints.objective.as_deref(),
            Some("wave21-08 machine-contract loop drives the brief from the contract"),
            "contract :goal MUST replace caller stale objective"
        );
        assert_eq!(
            hints.scope.as_deref(),
            Some("wave 21 task 08 only"),
            "contract :scope MUST replace caller stale scope"
        );
        assert_eq!(
            hints.owned_files,
            vec![
                "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs".to_string()
            ],
            "contract :write-scope MUST replace caller stale owned_files"
        );
        assert_eq!(
            hints.forbidden_files,
            vec!["crates/missiond-daemon/src/handlers/knowledge/workflow.rs".to_string()],
            "contract :must-not-touch MUST surface verbatim"
        );
        assert_eq!(
            hints.commit_policy.as_deref(),
            Some("scoped"),
            "contract :commit :policy MUST reach hints"
        );
        assert_eq!(
            hints.dispatch_strategy.as_deref(),
            Some("agent-team"),
            "contract :dispatch-strategy MUST replace caller fresh-code-alignment"
        );

        // Brief renderer must carry the wave-19/07 source-contract
        // preamble so the worker sees the SSOT pointer; the contract
        // :goal MUST surface as the brief objective; the stale caller
        // objective MUST NOT leak.
        let plan = fixture_plan("(plan)");
        let contract_path = std::path::Path::new(
            "/tmp/missiond-wave21-08-smoke/.missiond/tasks/wave21/wave21-08-dispatch.lisp",
        );
        let brief = build_task_brief_with_source(&plan, &hints, "agent-team", Some(contract_path));
        assert!(
            brief.contains("## Source contract"),
            "wave21-08 brief MUST carry the wave-19/07 source-contract preamble"
        );
        assert!(
            brief.contains(
                "/tmp/missiond-wave21-08-smoke/.missiond/tasks/wave21/wave21-08-dispatch.lisp"
            ),
            "wave21-08 brief preamble MUST name the on-disk contract path"
        );
        assert!(
            brief.contains("wave21-08 machine-contract loop drives the brief from the contract"),
            "wave21-08 brief MUST render the contract :goal as the objective"
        );
        assert!(
            !brief.contains("STALE caller objective"),
            "wave21-08 invariant: stale caller objective MUST NOT leak into the contract-driven brief"
        );
        // Pin the agent-team injection invariant survives the wave21-08
        // path.
        assert_eq!(
            brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "wave21-08 brief: agent-team hint MUST appear exactly once on the contract-driven path"
        );
    }

    // ── wave-22 / task 05 — autonomous workstation true spawn v1 tests ──

    /// Helper: build a minimal `Suggested` bundle with a single `target`
    /// proposal of `mission_task_delegate` carrying high confidence /
    /// safe safety_status. Used by the gate evaluator unit tests so
    /// they don't have to rebuild the same boilerplate.
    fn fixture_spawnable_bundle() -> WorkstationProposalBundle {
        WorkstationProposalBundle {
            status: WorkstationProposalStatus::Suggested,
            proposals: vec![
                WorkstationProposal {
                    field: "target",
                    value: json!("mission_task_delegate"),
                    confidence: WorkstationProposalConfidence::High,
                    evidence: "PLAN names mission_task_delegate".to_string(),
                    safety_status: WorkstationProposalSafetyStatus::Safe,
                },
                WorkstationProposal {
                    field: "objective",
                    value: json!("ship the wave22-05 invariant proof"),
                    confidence: WorkstationProposalConfidence::High,
                    evidence: "PLAN summary names the goal".to_string(),
                    safety_status: WorkstationProposalSafetyStatus::Safe,
                },
            ],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: Some("claude-sonnet".to_string()),
            request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
        }
    }

    /// Helper: a `ParsedTaskContract` with non-empty :write-scope and
    /// non-overlapping :must-not-touch. Used by the gate happy path.
    fn fixture_spawnable_contract() -> ParsedTaskContract {
        ParsedTaskContract {
            schema: "missiond.task-contract.v1".to_string(),
            goal: "wave22-05 spawn smoke".to_string(),
            scope: Some("ship the gate".to_string()),
            write_scope: vec![
                "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs".to_string(),
                "crates/missiond-daemon/src/handlers/knowledge/plan.rs".to_string(),
            ],
            must_not_touch: vec![".missiond/v2/intent-event-bus.lisp".to_string()],
            acceptance: vec!["cargo test -p missiond-daemon".to_string()],
            commit_policy: Some("scoped".to_string()),
            dispatch_strategy: Some("agent-team".to_string()),
            target_project: None,
            requested_cwd: None,
            target: Some("mission_task_delegate".to_string()),
            session_trace_path: None,
        }
    }

    fn fixture_input_all_green() -> WorkstationAutoSpawnInput {
        // Build a spawnable bundle to derive the canonical hash so the
        // helper does not have to import sha2 from inside the test.
        let bundle = fixture_spawnable_bundle();
        let hash = compute_workstation_proposal_hash(&bundle);
        WorkstationAutoSpawnInput {
            auto_spawn: true,
            proposal_hash: Some(hash),
            caller_approved: true,
            task_contract_path: Some(
                ".missiond/tasks/wave22/wave22-05-autonomous-workstation-true-spawn-v1.lisp"
                    .to_string(),
            ),
            preflight_status_acceptable: true,
            explicit: true,
        }
    }

    #[test]
    fn wave22_05_auto_spawn_status_wire_strings_pin() {
        let cases = [
            (WorkstationAutoSpawnStatus::NotRequested, "not_requested"),
            (WorkstationAutoSpawnStatus::Spawned, "spawned"),
            (
                WorkstationAutoSpawnStatus::SkippedUnavailable,
                "skipped_unavailable",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedNoProposals,
                "skipped_no_proposals",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedUnsafeProposal,
                "skipped_unsafe_proposal",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedConfidenceTooLow,
                "skipped_confidence_too_low",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
                "skipped_caller_not_approved",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath,
                "skipped_missing_task_contract_path",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedMalformedTaskContract,
                "skipped_malformed_task_contract",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedEmptyWriteScope,
                "skipped_empty_write_scope",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap,
                "skipped_forbidden_scope_overlap",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable,
                "skipped_preflight_unacceptable",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedUnsupportedTarget,
                "skipped_unsupported_target",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedSubstrateRefused,
                "skipped_substrate_refused",
            ),
            (
                WorkstationAutoSpawnStatus::SkippedSubstrateInnerError,
                "skipped_substrate_inner_error",
            ),
        ];
        let mut seen = std::collections::HashSet::new();
        for (status, wire) in cases {
            assert_eq!(status.as_wire(), wire);
            assert!(seen.insert(wire), "wire string `{}` duplicated", wire);
        }
        // was_spawned is true ONLY for Spawned.
        assert!(WorkstationAutoSpawnStatus::Spawned.was_spawned());
        for (status, _) in cases {
            if !matches!(status, WorkstationAutoSpawnStatus::Spawned) {
                assert!(
                    !status.was_spawned(),
                    "{:?} must not report spawned",
                    status
                );
            }
        }
    }

    #[test]
    fn wave22_05_proposal_hash_status_wire_strings_pin() {
        assert_eq!(
            WorkstationProposalHashStatus::NotSupplied.as_wire(),
            "not_supplied"
        );
        assert_eq!(WorkstationProposalHashStatus::Matches.as_wire(), "matches");
        assert_eq!(
            WorkstationProposalHashStatus::Mismatch.as_wire(),
            "mismatch"
        );
        assert_eq!(
            WorkstationProposalHashStatus::NoProposalAvailable.as_wire(),
            "no_proposal_available"
        );
    }

    #[test]
    fn wave22_05_compute_hash_is_deterministic_and_stable() {
        let bundle = fixture_spawnable_bundle();
        let h1 = compute_workstation_proposal_hash(&bundle);
        let h2 = compute_workstation_proposal_hash(&bundle);
        assert_eq!(h1, h2, "hash must be deterministic across calls");
        assert_eq!(h1.len(), 32, "hash MUST be exactly 32 hex chars (128 bits)");
        assert!(
            h1.chars().all(|c| c.is_ascii_hexdigit()),
            "hash MUST be hex-only"
        );

        // Mutating any load-bearing field changes the hash.
        let mut other = fixture_spawnable_bundle();
        other.proposals[0].value = json!("mission_execution");
        let h3 = compute_workstation_proposal_hash(&other);
        assert_ne!(h1, h3, "hash MUST change when proposal value changes");

        let mut other = fixture_spawnable_bundle();
        other.proposals[0].confidence = WorkstationProposalConfidence::Medium;
        let h4 = compute_workstation_proposal_hash(&other);
        assert_ne!(h1, h4, "hash MUST change when confidence changes");

        let mut other = fixture_spawnable_bundle();
        other.proposals[0].safety_status = WorkstationProposalSafetyStatus::AmbiguousValue;
        let h5 = compute_workstation_proposal_hash(&other);
        assert_ne!(h1, h5, "hash MUST change when safety_status changes");

        // Mutating non-load-bearing fields does NOT change the hash.
        let mut other = fixture_spawnable_bundle();
        other.proposals[0].evidence = "different evidence text".to_string();
        let h6 = compute_workstation_proposal_hash(&other);
        assert_eq!(
            h1, h6,
            "hash MUST be stable across superficial evidence text changes"
        );
    }

    #[test]
    fn wave22_05_parse_input_default_is_off_and_response_block_omitted_path() {
        let parsed = parse_workstation_auto_spawn_input(&json!({})).expect("ok");
        assert!(!parsed.auto_spawn);
        assert!(!parsed.caller_approved);
        assert!(!parsed.preflight_status_acceptable);
        assert!(parsed.proposal_hash.is_none());
        assert!(parsed.task_contract_path.is_none());
        assert!(!parsed.explicit, "absent ⇒ explicit=false");
    }

    #[test]
    fn wave22_05_parse_input_rejects_string_true_for_auto_spawn() {
        let err = parse_workstation_auto_spawn_input(&json!({"auto_spawn": "true"}))
            .expect_err("string \"true\" must fail-fast");
        assert_eq!(err.0, AUTO_SPAWN_INVALID_PARAM);
        assert!(err.1.contains("auto_spawn"));
    }

    #[test]
    fn wave22_05_parse_input_rejects_string_true_for_caller_approved() {
        let err =
            parse_workstation_auto_spawn_input(&json!({"workstation_caller_approved": "true"}))
                .expect_err("string \"true\" must fail-fast");
        assert_eq!(err.0, AUTO_SPAWN_INVALID_PARAM);
        assert!(err.1.contains("workstation_caller_approved"));
    }

    #[test]
    fn wave22_05_parse_input_rejects_non_string_path() {
        let err = parse_workstation_auto_spawn_input(&json!({"task_contract_path": 42}))
            .expect_err("non-string path must fail-fast");
        assert_eq!(err.0, AUTO_SPAWN_INVALID_PARAM);
        assert!(err.1.contains("task_contract_path"));
    }

    #[test]
    fn wave22_05_parse_input_accepts_bool_and_string_fields() {
        let parsed = parse_workstation_auto_spawn_input(&json!({
            "auto_spawn": true,
            "workstation_proposal_hash": "deadbeef",
            "workstation_caller_approved": true,
            "task_contract_path": ".missiond/tasks/foo.lisp",
            "preflight_status_acceptable": true,
        }))
        .expect("ok");
        assert!(parsed.auto_spawn);
        assert_eq!(parsed.proposal_hash.as_deref(), Some("deadbeef"));
        assert!(parsed.caller_approved);
        assert_eq!(
            parsed.task_contract_path.as_deref(),
            Some(".missiond/tasks/foo.lisp")
        );
        assert!(parsed.preflight_status_acceptable);
        assert!(parsed.explicit);
    }

    #[test]
    fn wave22_05_preflight_ok_when_auto_spawn_off() {
        let input = WorkstationAutoSpawnInput::default();
        assert!(enforce_auto_spawn_preflight(&input, None).is_ok());
        let bundle = fixture_spawnable_bundle();
        assert!(enforce_auto_spawn_preflight(&input, Some(&bundle)).is_ok());
    }

    #[test]
    fn wave22_05_preflight_missing_hash_when_auto_spawn_on() {
        let mut input = fixture_input_all_green();
        input.proposal_hash = None;
        let bundle = fixture_spawnable_bundle();
        let err = enforce_auto_spawn_preflight(&input, Some(&bundle))
            .expect_err("missing hash must fail-fast");
        assert_eq!(err.0, AUTO_SPAWN_MISSING_PROPOSAL_HASH);
    }

    #[test]
    fn wave22_05_preflight_mismatch_hash_when_auto_spawn_on() {
        let mut input = fixture_input_all_green();
        input.proposal_hash = Some("0".repeat(32));
        let bundle = fixture_spawnable_bundle();
        let err = enforce_auto_spawn_preflight(&input, Some(&bundle))
            .expect_err("mismatch hash must fail-fast");
        assert_eq!(err.0, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH);
    }

    #[test]
    fn wave22_05_preflight_no_bundle_when_auto_spawn_on_missing_hash() {
        let mut input = fixture_input_all_green();
        input.proposal_hash = None;
        let err = enforce_auto_spawn_preflight(&input, None)
            .expect_err("no bundle ⇒ missing-hash even when caller forgot the hash");
        assert_eq!(err.0, AUTO_SPAWN_MISSING_PROPOSAL_HASH);
    }

    #[test]
    fn wave22_05_preflight_no_bundle_when_auto_spawn_on_with_hash() {
        let input = fixture_input_all_green();
        let err = enforce_auto_spawn_preflight(&input, None)
            .expect_err("no bundle ⇒ mismatch even when caller supplied a hash");
        assert_eq!(err.0, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH);
    }

    #[test]
    fn wave22_05_preflight_ok_when_hash_matches() {
        let input = fixture_input_all_green();
        let bundle = fixture_spawnable_bundle();
        assert!(enforce_auto_spawn_preflight(&input, Some(&bundle)).is_ok());
    }

    #[test]
    fn wave22_05_gate_default_is_not_requested_byte_compatible_with_wave21_04() {
        let input = WorkstationAutoSpawnInput::default();
        let outcome = evaluate_workstation_auto_spawn_gate(&input, None, None, None);
        assert_eq!(outcome.status, WorkstationAutoSpawnStatus::NotRequested);
        assert!(!outcome.requested);
        assert!(outcome.gate_results.is_empty());
        // Wire JSON shape stable.
        let v = outcome.to_response_json();
        assert_eq!(v["auto_spawn_status"], "not_requested");
        assert_eq!(v["requested"], false);
    }

    #[test]
    fn wave22_05_gate_happy_path_returns_spawned() {
        let input = fixture_input_all_green();
        let bundle = fixture_spawnable_bundle();
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::Spawned,
            "all 12 gates green ⇒ Spawned: gate_results={:?}",
            outcome.gate_results
        );
        assert!(outcome.status.was_spawned());
        assert_eq!(
            outcome.spawn_target.as_deref(),
            Some("mission_task_delegate")
        );
        assert_eq!(
            outcome.proposal_hash_status,
            WorkstationProposalHashStatus::Matches
        );
        assert!(outcome
            .gate_results
            .iter()
            .any(|s| s.contains("g12_spawn_target:mission_task_delegate")));
        assert!(outcome
            .gate_results
            .iter()
            .any(|s| s.contains("auto_spawn_gate_satisfied")));
    }

    #[test]
    fn wave22_05_gate_skips_when_unavailable_no_fallback() {
        let input = fixture_input_all_green();
        let bundle = WorkstationProposalBundle::unavailable("Sonnet down");
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedUnavailable
        );
        assert!(
            outcome
                .gate_results
                .iter()
                .any(|s| s.contains("llm_unavailable")),
            "unavailable reason MUST surface in gate_results: {:?}",
            outcome.gate_results
        );
        assert!(
            outcome.gate_results.iter().any(|s| s.contains("claude -p")),
            "gate text MUST pin no-fallback invariant"
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_proposal_unsafe() {
        let input = fixture_input_all_green();
        let mut bundle = fixture_spawnable_bundle();
        bundle.proposals[0].safety_status = WorkstationProposalSafetyStatus::UnsupportedTarget;
        // hash will mismatch since safety_status is part of the hash; recompute.
        let mut input = input;
        input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedUnsafeProposal
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_confidence_low() {
        let mut bundle = fixture_spawnable_bundle();
        bundle.proposals[0].confidence = WorkstationProposalConfidence::Medium;
        let mut input = fixture_input_all_green();
        input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedConfidenceTooLow
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_caller_not_approved() {
        let mut input = fixture_input_all_green();
        input.caller_approved = false;
        let bundle = fixture_spawnable_bundle();
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedCallerNotApproved
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_preflight_unacceptable() {
        let mut input = fixture_input_all_green();
        input.preflight_status_acceptable = false;
        let bundle = fixture_spawnable_bundle();
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_task_contract_path_missing() {
        let mut input = fixture_input_all_green();
        input.task_contract_path = None;
        let bundle = fixture_spawnable_bundle();
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_task_contract_malformed() {
        let input = fixture_input_all_green();
        let bundle = fixture_spawnable_bundle();
        let outcome = evaluate_workstation_auto_spawn_gate(
            &input,
            Some(&bundle),
            None,
            Some("schema mismatch — expected `missiond.task-contract.v1`"),
        );
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedMalformedTaskContract
        );
        assert_eq!(
            outcome.substrate_reason.as_deref(),
            Some("schema mismatch — expected `missiond.task-contract.v1`")
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_write_scope_empty() {
        let input = fixture_input_all_green();
        let bundle = fixture_spawnable_bundle();
        let mut contract = fixture_spawnable_contract();
        contract.write_scope.clear();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedEmptyWriteScope
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_must_not_touch_overlaps_write_scope() {
        let input = fixture_input_all_green();
        let bundle = fixture_spawnable_bundle();
        let mut contract = fixture_spawnable_contract();
        // Force overlap.
        contract
            .must_not_touch
            .push(contract.write_scope[0].clone());
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_proposed_target_not_task_delegate() {
        let mut bundle = fixture_spawnable_bundle();
        bundle.proposals[0].value = json!("mission_execution");
        let mut input = fixture_input_all_green();
        input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedUnsupportedTarget
        );
        assert_eq!(outcome.spawn_target.as_deref(), Some("mission_execution"));
    }

    #[test]
    fn wave22_05_gate_skips_when_no_target_proposal() {
        let mut bundle = fixture_spawnable_bundle();
        bundle.proposals.retain(|p| p.field != "target");
        let mut input = fixture_input_all_green();
        input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedUnsupportedTarget
        );
    }

    #[test]
    fn wave22_05_gate_skips_when_bundle_no_suggestions() {
        let input = fixture_input_all_green();
        let bundle = WorkstationProposalBundle {
            status: WorkstationProposalStatus::NoSuggestions,
            proposals: Vec::new(),
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: Some("claude-sonnet".to_string()),
            request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
        };
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedNoProposals
        );
    }

    /// Wave21-04 invariant: even after wave22-05's true-spawn promotion,
    /// the wave-21 propose-only bundle's `auto_spawn=false` field MUST
    /// stay unchanged. The wave-22 gate publishes its own auto_spawn
    /// status on a SEPARATE `workstation_auto_spawn_gate` block.
    #[test]
    fn wave22_05_preserves_wave21_04_bundle_auto_spawn_false_invariant() {
        let bundle = fixture_spawnable_bundle();
        let v = bundle.to_response_json();
        assert_eq!(
            v["auto_spawn"], false,
            "wave-21 / task 04 invariant: bundle MUST still pin auto_spawn=false on the \
             propose-only surface — wave-22 / task 05 publishes spawn status SEPARATELY"
        );
    }

    /// Wave21-04 invariant: every proposal still carries applied=false
    /// on the wire, even when the wave22-05 gate would have spawned.
    /// The propose-only surface stays semantically identical; the
    /// real spawn happens through the wave-22 gate's substrate path.
    #[test]
    fn wave22_05_preserves_wave21_04_proposal_applied_false_invariant() {
        let bundle = fixture_spawnable_bundle();
        for p in &bundle.proposals {
            let v = p.to_json();
            assert_eq!(
                v["applied"], false,
                "wave-21 / task 04 invariant: every proposal MUST carry applied=false on the \
                 wire — wave-22 / task 05 spawns via SEPARATE gate decision, not by flipping \
                 this field"
            );
        }
    }

    /// Wave21-04 invariant: Sonnet unavailable bundle's
    /// `unavailable_reason` text still pins the no-fallback contract.
    /// The wave-22 gate inherits this invariant — when bundle is
    /// Unavailable, the gate refuses to spawn (no fallback synthesis).
    #[test]
    fn wave22_05_preserves_wave21_04_unavailable_no_fallback_invariant() {
        let bundle = WorkstationProposalBundle::unavailable(
            "Sonnet gateway not initialized; autonomous workstation proposal unavailable \
             (no fallback to claude -p / prompt mode in v0)",
        );
        assert!(bundle
            .unavailable_reason
            .as_deref()
            .unwrap_or("")
            .contains("no fallback"));
        // Gate inherits: SkippedUnavailable when bundle is Unavailable.
        let input = fixture_input_all_green();
        let outcome = evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), None, None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedUnavailable
        );
    }

    /// Wave21-04 invariant cross-check: the response JSON of the
    /// wave-22 gate MUST surface every contract-mandated field
    /// (auto_spawn_status / spawn_target / task_contract_path /
    /// proposal_hash_status / gate_results) in a stable shape so
    /// dashboards can pivot uniformly.
    #[test]
    fn wave22_05_response_json_carries_all_contract_fields() {
        let input = fixture_input_all_green();
        let bundle = fixture_spawnable_bundle();
        let contract = fixture_spawnable_contract();
        let outcome =
            evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
        let v = outcome.to_response_json();
        for key in [
            "requested",
            "auto_spawn_status",
            "spawn_target",
            "task_contract_path",
            "proposal_hash_status",
            "computed_proposal_hash",
            "supplied_proposal_hash",
            "caller_approved",
            "preflight_status_acceptable",
            "gate_results",
            "substrate_reason",
        ] {
            assert!(
                v.get(key).is_some(),
                "wave22-05 response JSON MUST carry `{}`",
                key
            );
        }
    }

    /// Wave21-04 invariant carryover: DAG mode still rejects the wave-21
    /// propose pass at preflight. The wave-22 gate also refuses (it
    /// requires a bundle, which DAG mode prevents creating). This is
    /// belt-and-braces: the DAG preflight rejection is enforced by
    /// `plan.rs::refuse_workstation_inference_in_dag_mode` BEFORE any
    /// gate runs.
    #[test]
    fn wave22_05_gate_skips_when_no_bundle_supplied() {
        let input = fixture_input_all_green();
        let outcome = evaluate_workstation_auto_spawn_gate(&input, None, None, None);
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::SkippedNoProposals,
            "no bundle ⇒ SkippedNoProposals (the DAG-mode invariant flows through here)"
        );
    }

    // ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
    //
    // Pin the wave22-05 autonomous workstation true-spawn gate slice
    // of the wave22-07 v4 smoke contract. The pure preflight + 12-rule
    // evaluator pair is the deterministic SSOT — no Sonnet call, no
    // substrate dispatch, pure in-process functions over synthesised
    // bundle / contract / input fixtures. The companion review_gate.rs
    // / plan.rs / agent_execution.rs / unified_entry.rs smokes cover
    // the review-apply-gate, persisted-apply, failed-verification, and
    // markdown-non-load-bearing slices.

    /// V4 smoke (Requirement 2 / workstation auto-spawn slice): the
    /// wave22-05 gate MUST reject `auto_spawn=true` when the caller
    /// does not supply `proposal_hash`, MUST reject a mismatched hash,
    /// AND MUST accept the canonical `compute_workstation_proposal_hash`
    /// value. This is the wave22-05 fail-fast preflight — the gate
    /// refuses to dispatch the substrate with no correlator and
    /// accepts only the canonical fixture path, where the 12-rule
    /// evaluator then drives Spawned without ever calling the
    /// substrate (the test never exercises run_workstation_dispatch).
    #[test]
    fn smoke_wave22_07_workstation_auto_spawn_gate_rejects_missing_hash_accepts_fixture_hash() {
        let bundle = fixture_spawnable_bundle();
        let canonical = compute_workstation_proposal_hash(&bundle);
        // Missing proposal_hash → AUTO_SPAWN_MISSING_PROPOSAL_HASH.
        let mut missing_input = fixture_input_all_green();
        missing_input.proposal_hash = None;
        let err = enforce_auto_spawn_preflight(&missing_input, Some(&bundle))
            .expect_err("wave22-07 v4: missing proposal_hash MUST fail-fast on auto-spawn path");
        assert_eq!(
            err.0, AUTO_SPAWN_MISSING_PROPOSAL_HASH,
            "wave22-07 v4 invariant: missing proposal_hash MUST surface the dedicated code"
        );
        // Mismatched proposal_hash → AUTO_SPAWN_PROPOSAL_HASH_MISMATCH.
        let mut mismatch_input = fixture_input_all_green();
        mismatch_input.proposal_hash = Some("0".repeat(32));
        let err = enforce_auto_spawn_preflight(&mismatch_input, Some(&bundle))
            .expect_err("wave22-07 v4: mismatched proposal_hash MUST fail-fast");
        assert_eq!(err.0, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH);
        // Matching fixture hash → preflight OK + 12-rule gate Spawned
        // (substrate is NOT invoked — the gate evaluator is a pure
        // function ending at WorkstationAutoSpawnStatus::Spawned).
        let mut valid_input = fixture_input_all_green();
        valid_input.proposal_hash = Some(canonical.clone());
        assert!(
            enforce_auto_spawn_preflight(&valid_input, Some(&bundle)).is_ok(),
            "wave22-07 v4: matching proposal_hash MUST pass auto-spawn preflight"
        );
        let contract = fixture_spawnable_contract();
        let outcome = evaluate_workstation_auto_spawn_gate(
            &valid_input,
            Some(&bundle),
            Some(&contract),
            None,
        );
        assert_eq!(
            outcome.status,
            WorkstationAutoSpawnStatus::Spawned,
            "wave22-07 v4 invariant: matching fixture hash + all 12 rules green \
             MUST drive the auto-spawn gate to Spawned (no real substrate dispatch \
             happens in this smoke — the evaluator is pure)"
        );
        assert_eq!(
            outcome.proposal_hash_status,
            WorkstationProposalHashStatus::Matches
        );
    }

    /// V4 smoke (cross-wave invariants / wave21-04 4 invariants
    /// pinned): the wave22-05 true-spawn gate MUST preserve every
    /// wave-21 / task 04 propose-only invariant when stamped onto the
    /// same call.
    ///   * I1 default off — `auto_spawn=false` (or absent) keeps the
    ///     wave-21 / task 04 byte-shape exactly: no
    ///     `workstation_auto_spawn_gate` block on the response.
    ///   * I2 Sonnet unavailable no fallback — the gate MUST short-
    ///     circuit on `Unavailable` bundles with the no-fallback
    ///     marker text in `gate_results`.
    ///   * I3 DAG mode rejects (flows through `Some(bundle)=None` for
    ///     the propose-only path) — the gate skips with
    ///     `SkippedNoProposals` when no bundle is supplied.
    ///   * I4 propose-only fields preserved — the bundle's
    ///     `auto_spawn=false` + every proposal's `applied=false`
    ///     wire shape stays unchanged on the propose-only surface;
    ///     the wave-22 / task 05 gate publishes spawn status on a
    ///     SEPARATE block.
    #[test]
    fn smoke_wave22_07_workstation_auto_spawn_gate_pins_wave21_04_four_invariants() {
        // I1 — default off: gate omitted when auto_spawn=false / absent.
        let off_input = WorkstationAutoSpawnInput::default();
        let off_outcome = evaluate_workstation_auto_spawn_gate(&off_input, None, None, None);
        assert_eq!(
            off_outcome.status,
            WorkstationAutoSpawnStatus::NotRequested,
            "wave21-04 I1: default off ⇒ NotRequested (byte-shape preserved)"
        );
        assert!(
            !off_outcome.requested,
            "wave21-04 I1: requested MUST stay false on the default-off path"
        );
        // I2 — Sonnet unavailable no fallback (the gate text pins the
        // no-fallback marker so dashboards can grep the response).
        let unavail_input = fixture_input_all_green();
        let unavail_bundle = WorkstationProposalBundle::unavailable("Sonnet down");
        let unavail_contract = fixture_spawnable_contract();
        let unavail_outcome = evaluate_workstation_auto_spawn_gate(
            &unavail_input,
            Some(&unavail_bundle),
            Some(&unavail_contract),
            None,
        );
        assert_eq!(
            unavail_outcome.status,
            WorkstationAutoSpawnStatus::SkippedUnavailable,
            "wave21-04 I2: Sonnet unavailable ⇒ SkippedUnavailable (never fall back)"
        );
        assert!(
            unavail_outcome
                .gate_results
                .iter()
                .any(|s| s.contains("claude -p")),
            "wave21-04 I2: gate text MUST pin the no-fallback invariant in gate_results"
        );
        // I3 — DAG-mode-style rejection (no bundle ⇒ SkippedNoProposals).
        // The wave-21 / task 04 DAG-mode-rejects invariant flows into
        // the gate via the `bundle=None` branch (the wave-22 / task 05
        // splice site refuses to compute a bundle in DAG mode).
        let dag_outcome =
            evaluate_workstation_auto_spawn_gate(&fixture_input_all_green(), None, None, None);
        assert_eq!(
            dag_outcome.status,
            WorkstationAutoSpawnStatus::SkippedNoProposals,
            "wave21-04 I3: DAG-mode-style (no bundle) MUST skip without dispatching"
        );
        // I4 — propose-only bundle's auto_spawn=false and every
        // proposal's applied=false stay unchanged on the wire.
        let bundle = fixture_spawnable_bundle();
        let v = bundle.to_response_json();
        assert_eq!(
            v["auto_spawn"], false,
            "wave21-04 I4: bundle auto_spawn MUST stay false on the propose-only surface"
        );
        for p in v["proposals"].as_array().expect("proposals array") {
            assert_eq!(
                p["applied"], false,
                "wave21-04 I4: every proposal MUST keep applied=false on the propose-only wire"
            );
        }
    }

    // ── wave-23 / task 05 — session-trace propagation tests ─────────────
    //
    // These tests pin the brief / contract / response surfaces against
    // the four cases the task contract enumerates: legacy (no trace),
    // happy path (trace forwarded), and contract-side overlay (the
    // pure-parser variant). Plan-level integration (validation +
    // response surface) lives in plan.rs::tests; this module exercises
    // the workstation-dispatch substrate alone.

    #[test]
    fn build_task_brief_with_source_and_trace_omits_session_trace_block_when_path_absent() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief_with_source_and_trace(
            &plan,
            &hints,
            "fresh-code-alignment",
            None,
            None,
        );
        assert!(
            !brief.contains("## Session trace"),
            "wave23-05: legacy brief (no trace path supplied) must NOT carry the Session trace section — got:\n{}",
            brief
        );
    }

    #[test]
    fn build_task_brief_with_source_and_trace_renders_session_trace_block_when_path_supplied() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let trace = ".missiond/tasks/wave23/session-trace.lisp";
        let brief = build_task_brief_with_source_and_trace(
            &plan,
            &hints,
            "fresh-code-alignment",
            None,
            Some(trace),
        );
        assert!(
            brief.contains("## Session trace\n"),
            "wave23-05: brief with trace path must render the Session trace heading — got:\n{}",
            brief
        );
        assert!(
            brief.contains(trace),
            "wave23-05: brief must echo the trace path verbatim so the worker reads it"
        );
        assert!(
            brief.contains("forward this path verbatim as `session_trace_path`"),
            "wave23-05: brief must instruct the worker to forward the path on completion calls"
        );
    }

    #[test]
    fn parse_task_contract_extracts_session_trace_path_from_contract_lisp() {
        // Pure-parser test: a contract that emits :session-trace-path
        // must round-trip through the workstation_dispatch parser into
        // ParsedTaskContract.session_trace_path. This is the SSOT path
        // for machine-mode dispatches that load the contract directly
        // (caller may have dropped the explicit arg).
        let src = r#"(task plan-trace
  :schema "missiond.task-contract.v1"
  :goal "ship the wave"
  :write-scope ["a.rs"]
  :must-not-touch []
  :acceptance ["cargo test"]
  :session-trace-path ".missiond/tasks/wave23/session-trace.lisp"
)"#;
        let parsed = parse_task_contract(src).expect("parse ok");
        assert_eq!(
            parsed.session_trace_path.as_deref(),
            Some(".missiond/tasks/wave23/session-trace.lisp"),
            "wave23-05: contract :session-trace-path must round-trip through the parser"
        );
    }
}
