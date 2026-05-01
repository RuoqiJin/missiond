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

use serde_json::Value;

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

mod auto_spawn;
mod descriptor;
mod proposal;

#[cfg(test)]
pub(crate) use descriptor::parse_task_contract;
#[cfg(test)]
use descriptor::resolve_contract_path;
pub(crate) use descriptor::{
    load_task_contract, resolve_contract_path_public, ParsedTaskContract, TaskContractParseError,
};

#[cfg(test)]
pub(crate) use auto_spawn::{
    compute_workstation_proposal_hash, WorkstationProposalHashStatus, AUTO_SPAWN_INVALID_PARAM,
    AUTO_SPAWN_MISSING_PROPOSAL_HASH, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH,
};
pub(crate) use auto_spawn::{
    enforce_auto_spawn_preflight, evaluate_workstation_auto_spawn_gate,
    parse_workstation_auto_spawn_input, WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnInput,
    WorkstationAutoSpawnStatus,
};
#[cfg(test)]
pub(crate) use proposal::{
    build_workstation_proposal_prompt, classify_proposal_safety, parse_workstation_proposals,
    WorkstationProposal, WorkstationProposalConfidence, WorkstationProposalSafetyStatus,
    WorkstationProposalStatus, PROPOSAL_VALID_STRATEGIES, PROPOSAL_VALID_TARGETS,
    SONNET_WORKSTATION_PROPOSAL_CALLER, WORKSTATION_PROPOSAL_CAP, WORKSTATION_PROPOSAL_FIELDS,
};
pub(crate) use proposal::{
    request_workstation_proposals, WorkstationProposalBundle, WorkstationProposalGate,
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

pub(crate) use brief::{
    build_task_brief, build_task_brief_with_source_and_trace, workstation_execution_id,
};
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

#[cfg(test)]
mod tests;
