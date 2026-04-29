use serde_json::Value;

/// Whether the caller / plan opted into workstation-dispatch v0. This is
/// the legacy back-compat helper kept so existing tests / callers keep
/// reading the same boolean. New code goes through `evaluate_dispatch_decision`
/// so the response can surface the source + inference reason.
pub(crate) fn opt_in_requested(args: &Value, plan_hint_workstation_dispatch: bool) -> bool {
    let arg_flag = args
        .get("workstation_dispatch")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    arg_flag || plan_hint_workstation_dispatch
}

/// wave-16 / task 03 — the resolved source of a workstation-dispatch
/// decision. Surfaced verbatim on the response under
/// `workstation_dispatch_source` so callers can route on the provenance
/// without re-deriving it.
///
/// Semantics:
///   * `ExplicitArg`   — caller passed `workstation_dispatch=true` (and
///                       passed every safety gate). Wave-15 behaviour.
///   * `PlanHint`      — PLAN.lisp / DAG node carried `:workstation-dispatch
///                       true` (and explicit arg was absent or true).
///                       Wave-15 behaviour.
///   * `Inferred`      — caller set neither flag, but the resolved target +
///                       dispatch strategy + objective + at least one
///                       scoping signal all matched the conservative
///                       auto-inference rule. Wave-16 behaviour.
///   * `Disabled`      — caller passed `workstation_dispatch=false`.
///                       Auto-inference is suppressed.
///   * `NotApplicable` — none of the above; fall through to the legacy
///                       plan-runner internal dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationDispatchSource {
    ExplicitArg,
    PlanHint,
    Inferred,
    Disabled,
    NotApplicable,
}

impl WorkstationDispatchSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            WorkstationDispatchSource::ExplicitArg => "explicit_arg",
            WorkstationDispatchSource::PlanHint => "plan_hint",
            WorkstationDispatchSource::Inferred => "inferred",
            WorkstationDispatchSource::Disabled => "disabled",
            WorkstationDispatchSource::NotApplicable => "not_applicable",
        }
    }
}

/// Resolved decision: should this dispatch run through the workstation
/// substrate? Carries the source + (when relevant) the reason text the
/// inference engine attached. The reason is response-facing only — it does
/// NOT change the dispatch path.
#[derive(Debug, Clone)]
pub(crate) struct DispatchDecision {
    pub source: WorkstationDispatchSource,
    pub reason: Option<String>,
}

impl DispatchDecision {
    fn enabled(source: WorkstationDispatchSource, reason: Option<String>) -> Self {
        Self { source, reason }
    }

    fn off(source: WorkstationDispatchSource, reason: Option<String>) -> Self {
        Self { source, reason }
    }

    /// True iff this decision routes through the workstation-dispatch
    /// substrate. Only the explicit/plan-hint/inferred branches do; the
    /// disabled and not-applicable branches stay on the legacy plan-runner.
    pub(crate) fn is_enabled(&self) -> bool {
        matches!(
            self.source,
            WorkstationDispatchSource::ExplicitArg
                | WorkstationDispatchSource::PlanHint
                | WorkstationDispatchSource::Inferred
        )
    }
}

/// wave-16 / task 03 — the strategies the auto-inference engine accepts.
/// Strictly a sub-list of `VALID_DISPATCH_STRATEGIES` from plan.rs:
/// `unknown` and `prompt-fallback` are intentionally excluded so a node
/// without a real dispatch hint stays on the legacy plan-runner path.
pub(crate) const INFERABLE_DISPATCH_STRATEGIES: &[&str] = &[
    "fresh-code-alignment",
    "resident-lisp",
    "agent-team",
    "mixed",
];

/// Hint context the inference engine reads. Only the conservative subset
/// of fields that actually scope a workstation task — the fully merged
/// hint set is built later via `WorkstationDispatchHints::merge_args` once
/// we know the decision is "go".
pub(crate) struct InferenceContext<'a> {
    /// Resolved target (already normalised to one of `mission_execution |
    /// mission_task_delegate | mission_flow_run`).
    pub target: &'a str,
    /// Already-canonicalised dispatch strategy (one of
    /// `VALID_DISPATCH_STRATEGIES` in plan.rs, including `unknown`).
    pub dispatch_strategy: &'a str,
    /// Final objective text (caller arg with PLAN.lisp / node fallback).
    pub objective: Option<&'a str>,
    /// Scoping signal #1 — declared owned files (post-merge).
    pub owned_files_present: bool,
    /// Scoping signal #2 — free-form scope string.
    pub scope_present: bool,
    /// Scoping signal #3 — explicit `target_project` (caller arg or hint).
    pub target_project_present: bool,
    /// Scoping signal #4 — explicit `requested_cwd` (caller arg or hint).
    pub requested_cwd_present: bool,
}

/// Read the caller's explicit `workstation_dispatch` knob. `None` means
/// "no explicit choice" (auto-inference is allowed); `Some(true)` /
/// `Some(false)` mean "explicit on" / "explicit off".
pub(crate) fn explicit_workstation_dispatch_flag(args: &Value) -> Option<bool> {
    args.get("workstation_dispatch").and_then(|v| v.as_bool())
}

/// Decide whether to route through workstation-dispatch.
///
/// Precedence (highest first):
///   1. `workstation_dispatch=false` arg → `Disabled` (suppresses inference).
///   2. `workstation_dispatch=true` arg  → `ExplicitArg`.
///   3. PLAN.lisp / node `:workstation-dispatch true` → `PlanHint`.
///   4. Auto-inference (all five conditions) → `Inferred`.
///   5. Otherwise → `NotApplicable`.
///
/// Conditions for `Inferred` (ALL must hold):
///   a. resolved target is `mission_task_delegate`
///   b. dispatch strategy is one of `INFERABLE_DISPATCH_STRATEGIES`
///   c. objective is non-empty
///   d. at least one scoping signal is present
///      (owned_files | scope | target_project | requested_cwd)
///   e. caller did not set `workstation_dispatch=false`
///
/// `mission_execution` and `mission_flow_run` are NEVER auto-inferred —
/// auto-inference only ever wraps the task_delegate substrate.
pub(crate) fn evaluate_dispatch_decision(
    args: &Value,
    plan_hint_workstation_dispatch: bool,
    ctx: &InferenceContext<'_>,
) -> DispatchDecision {
    let explicit = explicit_workstation_dispatch_flag(args);

    // 1. Explicit `false` short-circuits everything.
    if explicit == Some(false) {
        return DispatchDecision::off(
            WorkstationDispatchSource::Disabled,
            Some(
                "workstation_dispatch=false suppresses both opt-in and auto-inference".to_string(),
            ),
        );
    }

    // 2. Explicit `true` is honoured even if a safety gate would later
    //    refuse — the wave-15 behaviour returns a SafeDescriptor and the
    //    caller sees it. We do NOT silently downgrade to NotApplicable.
    if explicit == Some(true) {
        return DispatchDecision::enabled(
            WorkstationDispatchSource::ExplicitArg,
            Some("caller passed workstation_dispatch=true".to_string()),
        );
    }

    // 3. PLAN.lisp / node hint.
    if plan_hint_workstation_dispatch {
        return DispatchDecision::enabled(
            WorkstationDispatchSource::PlanHint,
            Some("PLAN.lisp / node carried :workstation-dispatch true".to_string()),
        );
    }

    // 4. Auto-inference. Each gate produces a deterministic skip-reason
    //    so the response can explain why we did NOT auto-enable.

    // a. Target must be mission_task_delegate.
    if ctx.target != "mission_task_delegate" {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(format!(
                "auto-inference only wraps mission_task_delegate; resolved target is `{}`",
                ctx.target
            )),
        );
    }

    // b. Dispatch strategy must be in the inferable subset.
    if !INFERABLE_DISPATCH_STRATEGIES.contains(&ctx.dispatch_strategy) {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(format!(
                "auto-inference requires a known workstation dispatch strategy ({:?}); got `{}`",
                INFERABLE_DISPATCH_STRATEGIES, ctx.dispatch_strategy
            )),
        );
    }

    // c. Objective must be non-empty.
    let has_objective = ctx.objective.map(|s| !s.trim().is_empty()).unwrap_or(false);
    if !has_objective {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(
                "auto-inference requires a non-empty objective (caller arg or PLAN.lisp hint)"
                    .to_string(),
            ),
        );
    }

    // d. At least one scoping signal.
    let any_scope = ctx.owned_files_present
        || ctx.scope_present
        || ctx.target_project_present
        || ctx.requested_cwd_present;
    if !any_scope {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(
                "auto-inference requires at least one scoping signal: owned_files, scope, \
                 target_project, or requested_cwd"
                    .to_string(),
            ),
        );
    }

    DispatchDecision::enabled(
        WorkstationDispatchSource::Inferred,
        Some(format!(
            "inferred from target=mission_task_delegate, dispatch_strategy=`{}`, non-empty objective, scoping signals present",
            ctx.dispatch_strategy
        )),
    )
}
