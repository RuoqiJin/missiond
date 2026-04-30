use missiond_core::types::Plan;
use serde_json::{json, Value};

use crate::state::AppState;

mod cascade;

#[allow(unused_imports)]
pub(super) use cascade::{
    build_compensation_plan_entry, compute_compensation_order, run_cascade_rollback,
};

use super::DagNode;

impl DagNode {
    /// wave-17 / task 04 — typed projection of `:rollback-policy`.
    /// Returns `None` when the author did not declare a policy OR wrote
    /// an unrecognised value (the parser also pushes unrecognised
    /// values into `unsupported_fields` so the typo is loud).
    pub(super) fn rollback_policy_kind(&self) -> Option<RollbackPolicy> {
        let raw = self.rollback_policy.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        RollbackPolicy::parse(raw)
    }

    /// wave-18 / task 04 — typed projection of `:rollback-cascade`.
    /// Returns `None` when the author did not declare a cascade mode OR
    /// wrote an unrecognised value (the parser ALSO pushes unrecognised
    /// values into `unsupported_fields` so the typo is loud). The
    /// scheduler treats `None` as `RollbackCascadeMode::None` (the safe
    /// default — cascade pass skipped).
    pub(super) fn rollback_cascade_kind(&self) -> Option<RollbackCascadeMode> {
        let raw = self.rollback_cascade.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        RollbackCascadeMode::parse(raw)
    }

    /// wave-18 / task 04 — true iff this node opted into the cascade
    /// rollback evaluator (any `:rollback-cascade` value other than
    /// `"none"`). Used by the scheduler to decide whether to run the
    /// cascade pass after the per-node `run_rollback`.
    pub(super) fn has_active_rollback_cascade(&self) -> bool {
        matches!(
            self.rollback_cascade_kind(),
            Some(RollbackCascadeMode::Plan) | Some(RollbackCascadeMode::DispatchSafe)
        )
    }

    /// wave-17 / task 04 — true iff this node opted into ANY rollback
    /// hint (policy / objective / owned files / acceptance commands).
    /// Used to skip the rollback evaluator entirely on the wave-13
    /// byte-shape path (no hints declared → no rollback evidence row).
    pub(super) fn has_rollback_hints(&self) -> bool {
        let policy_present = self
            .rollback_policy
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let objective_present = self
            .rollback_objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let owned_present = self
            .rollback_owned_files_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let acceptance_present = self
            .rollback_acceptance_commands_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        // wave-18 / task 04 — cascade hints also count: a node that
        // declares `:rollback-cascade` / `:compensates` / `:rollback-after`
        // but no `:rollback-policy` should still surface its rollback
        // intent through the response so audit can pin the cascade plan.
        let cascade_present = self
            .rollback_cascade
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let compensates_present = self
            .compensates
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        // wave-19 / task 10 — forward `:compensate-node` refs are also
        // a rollback hint (declared on the failing node side); surface
        // them through `node_hint_summary` for the same audit reasons.
        let compensate_node_present = self
            .compensate_node
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let rollback_after_present = !self.rollback_after.is_empty();
        policy_present
            || objective_present
            || owned_present
            || acceptance_present
            || cascade_present
            || compensates_present
            || compensate_node_present
            || rollback_after_present
    }
}

/// wave-17 / task 04 — typed projection of `:rollback-policy` for the
/// conservative rollback descriptor pass. Resolved on the parser side
/// so the runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `None`        — author wrote `"none"` (or omitted the policy
///                      entirely; absence on `DagNode::rollback_policy`
///                      is the SAME as `None`). Preserves the existing
///                      failure behaviour: failed node propagates taint
///                      per `:failure-policy`, no rollback descriptor
///                      is emitted.
///   * `Descriptor`  — record / surface a structured rollback
///                      descriptor (objective + owned files +
///                      acceptance commands + brief preview) on the
///                      response and evidence row. **Never dispatches.**
///                      Use this when the author wants downstream
///                      observers / humans to know what a rollback
///                      WOULD do without authorising the scheduler to
///                      execute it.
///   * `Workstation` — opt into automatic rollback dispatch through
///                      the existing wave-15 workstation-dispatch
///                      substrate. The scheduler ONLY dispatches when
///                      every safety condition holds (resolved target
///                      project, non-empty rollback objective, at
///                      least one owned file, dispatch strategy is on
///                      the inferable whitelist). Otherwise the row
///                      surfaces as `refused` with the failing
///                      condition spelled out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RollbackPolicy {
    None,
    Descriptor,
    Workstation,
}

impl RollbackPolicy {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            RollbackPolicy::None => "none",
            RollbackPolicy::Descriptor => "descriptor",
            RollbackPolicy::Workstation => "workstation",
        }
    }

    /// Parse a raw `:rollback-policy` value into a typed mode. Trims
    /// and lowercases the input; unknown values yield `None` (the
    /// parser also pushes them onto `unsupported_fields` so the typo
    /// surfaces in `node_hint_summary`).
    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Some(RollbackPolicy::None),
            "descriptor" => Some(RollbackPolicy::Descriptor),
            "workstation" => Some(RollbackPolicy::Workstation),
            _ => None,
        }
    }
}

/// wave-17 / task 04 — outcome of the conservative rollback pass.
/// Drives whether the failed node carries a rollback descriptor on the
/// response, whether a rollback task was dispatched, and (when refused)
/// the condition that failed.
///
/// Wire vocabulary is fixed so audit dashboards can pivot on a single
/// string:
///   * `not_requested`     — no rollback hints declared OR
///                            `:rollback-policy "none"`. Default for
///                            failed nodes that did not opt in.
///   * `descriptor_ready`  — `:rollback-policy "descriptor"`. The
///                            descriptor is recorded on the response /
///                            evidence; **no dispatch happened**.
///   * `dispatched`        — `:rollback-policy "workstation"` AND
///                            every safety gate passed AND the
///                            rollback dispatch ran. The inner
///                            payload + brief preview ride on the row.
///   * `refused`           — `:rollback-policy "workstation"` was
///                            requested but at least one safety gate
///                            failed. The reason carries the failing
///                            condition. **No dispatch happened.**
///                            SafeDescriptor refusals from the
///                            underlying substrate also collapse here.
///   * `failed`            — `:rollback-policy "workstation"` was
///                            dispatched but the inner handler
///                            returned an error. The inner payload's
///                            error message is captured on the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RollbackStatus {
    NotRequested,
    DescriptorReady,
    Dispatched,
    Refused,
    Failed,
}

impl RollbackStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            RollbackStatus::NotRequested => "not_requested",
            RollbackStatus::DescriptorReady => "descriptor_ready",
            RollbackStatus::Dispatched => "dispatched",
            RollbackStatus::Refused => "refused",
            RollbackStatus::Failed => "failed",
        }
    }
}

/// wave-17 / task 04 — pure result of evaluating a node's rollback
/// hints. `policy=None` + `status=NotRequested` is the inactive
/// default; the wave loop suppresses the rollback evidence + response
/// surfacing whenever this evaluation is inactive so the wave-13
/// byte shape stays untouched for plans that did not opt in.
#[derive(Debug, Clone)]
pub(super) struct RollbackEvaluation {
    pub policy: RollbackPolicy,
    pub status: RollbackStatus,
    /// Reason the gate / dispatch landed where it did. Always populated
    /// (even for the `not_requested` branch) so the audit row carries a
    /// human-readable explanation of the decision.
    pub reason: String,
    /// Resolved rollback objective (may be empty when no hint declared).
    pub objective: Option<String>,
    /// Resolved owned-files list. Surfaced verbatim into the descriptor
    /// + brief.
    pub owned_files: Vec<String>,
    /// Resolved acceptance-commands list. Surfaced verbatim — NEVER
    /// executed by the scheduler.
    pub acceptance_commands: Vec<String>,
    /// Trimmed task-brief preview when the substrate built one.
    /// `None` for `not_requested` and for `refused` paths that
    /// short-circuited before brief construction.
    pub task_brief_preview: Option<String>,
    /// File path the brief was mirrored to (currently always `None` —
    /// substrate does not yet write the brief to disk; kept on the
    /// shape so a future enhancement can fill it in without breaking
    /// the wire contract).
    pub task_brief_path: Option<String>,
    /// Inner dispatch payload from `run_workstation_dispatch` when the
    /// rollback was actually dispatched. `None` for descriptor-only,
    /// not-requested, and refused paths.
    pub inner_payload: Option<Value>,
    /// wave-18 / task 04 — cascade rollback outcome for THIS failed node.
    /// `None` when the node did not opt into cascading (default — the
    /// wave-17 / task 04 byte shape is preserved). `Some(out)` carries
    /// the resolved cascade mode + ordered compensation outcomes; the
    /// scheduler stamps it onto `node_results[].rollback.cascade` so
    /// callers see the cascade plan + dispatch / refusal results without
    /// re-deriving from evidence.
    pub cascade: Option<CascadeRollbackOutcome>,
}

impl RollbackEvaluation {
    /// Convenience: this evaluation produced no rollback signal at all.
    /// Used by the scheduler to skip the rollback-evidence emit and
    /// preserve the v2 byte-shape.
    pub(super) fn is_inactive(&self) -> bool {
        matches!(self.policy, RollbackPolicy::None)
            && matches!(self.status, RollbackStatus::NotRequested)
            && self.objective.is_none()
            && self.owned_files.is_empty()
            && self.acceptance_commands.is_empty()
            // wave-18 / task 04 — a cascade-only opt-in (no node-local
            // rollback hints) MUST still surface so observers can pin
            // the cascade plan. Treat any active cascade as a signal.
            && self
                .cascade
                .as_ref()
                .map(|c| c.is_inactive())
                .unwrap_or(true)
    }

    /// Project the evaluation as a JSON block suitable for
    /// `node_results[].rollback` / `evidence.rollback`. Stable shape
    /// — every field is always present so consumers don't have to
    /// branch on absence.
    pub(super) fn to_json(&self) -> Value {
        let mut v = json!({
            "policy": self.policy.as_wire(),
            "status": self.status.as_wire(),
            "reason": self.reason,
            "objective": self.objective,
            "owned_files": self.owned_files,
            "acceptance_commands": self.acceptance_commands,
            "acceptance_commands_executed": false,
        });
        if let Some(preview) = self.task_brief_preview.as_deref() {
            v["task_brief_preview"] = json!(preview);
        }
        if let Some(p) = self.task_brief_path.as_deref() {
            v["task_brief_path"] = json!(p);
        }
        if let Some(inner) = self.inner_payload.clone() {
            v["inner_result"] = inner;
        }
        // wave-18 / task 04 — cascade outcome rides on the same JSON
        // block so observers can pin `rollback.cascade.compensations[]`
        // without descending into a separate evidence row. Quiet when
        // the cascade evaluator never produced an observable signal so
        // the wave-17 / task 04 byte-shape stays untouched for plans
        // that did not opt into cascading.
        if let Some(cascade) = self.cascade.as_ref() {
            if !cascade.is_inactive() {
                v["cascade"] = cascade.to_json();
            }
        }
        v
    }
}

/// wave-18 / task 04 — typed projection of `:rollback-cascade` for the
/// conservative cascade rollback evaluator. Resolved on the parser side
/// so the runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `None`         — author wrote `"none"` OR omitted the value
///                       entirely. Cascade pass skipped; only the
///                       wave-17 / task 04 node-local rollback runs.
///                       This is the safe default — preserves the
///                       byte shape for plans that did not opt into
///                       cascading.
///   * `Plan`         — cascade evaluator computes the ordered list of
///                       compensation nodes (every plan node carrying
///                       `:compensates "<this-failed-node>"`) and
///                       records the plan on the response + evidence
///                       row. **NEVER dispatches.** Use this when the
///                       author wants downstream observers / humans to
///                       see what compensation WOULD be required without
///                       authorising the scheduler to execute it.
///   * `DispatchSafe` — cascade evaluator computes the same plan AND,
///                       for every compensation node whose own
///                       rollback safety gates pass, dispatches it
///                       through the wave-15 workstation substrate.
///                       Refusals are recorded but the cascade itself
///                       is NEVER retried — SafeDescriptor / safety-gate
///                       refusals stay refusals (mirrors the wave-17 /
///                       task 04 non-retryable invariant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RollbackCascadeMode {
    None,
    Plan,
    DispatchSafe,
}

impl RollbackCascadeMode {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            RollbackCascadeMode::None => "none",
            RollbackCascadeMode::Plan => "plan",
            RollbackCascadeMode::DispatchSafe => "dispatch-safe",
        }
    }

    /// Parse a raw `:rollback-cascade` value. Trims + lowercases; both
    /// `_` and `-` separators are accepted so authors can write either
    /// `dispatch_safe` or `dispatch-safe`. Unknown values yield `None`
    /// (the parser also pushes them onto `unsupported_fields`).
    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Some(RollbackCascadeMode::None),
            "plan" => Some(RollbackCascadeMode::Plan),
            "dispatch-safe" | "dispatch_safe" => Some(RollbackCascadeMode::DispatchSafe),
            _ => None,
        }
    }
}

/// wave-18 / task 04 — outcome of the cascade rollback pass for a single
/// compensation node. Captures whether the node was just recorded
/// (`plan` mode), dispatched through the substrate (`dispatch-safe` +
/// safety passed + dispatch ok), refused (any safety / substrate
/// refusal), or failed (substrate dispatched but inner handler errored).
///
/// Wire vocabulary mirrors [`RollbackStatus`] so audit dashboards can
/// pivot on the same string vocabulary across both single-node and
/// cascade evaluations.
#[derive(Debug, Clone)]
pub(super) struct CascadeCompensationOutcome {
    /// Plan id of the compensation node (matches `DagNode::id`).
    pub node_id: String,
    /// Resolved policy of THIS compensation node (NOT the cascade root).
    /// `None` when the compensation node carried no `:rollback-policy`
    /// — the cascade evaluator treats that as `Descriptor` for the
    /// purpose of recording intent.
    pub policy: RollbackPolicy,
    /// Final per-compensation-node status. Vocabulary:
    ///   * `descriptor_ready` — `plan` mode OR `dispatch-safe` mode but
    ///                          the compensation node is descriptor-only
    ///                          (`:rollback-policy "descriptor"`).
    ///   * `dispatched`       — `dispatch-safe` mode AND every safety
    ///                          gate passed AND inner handler returned Ok.
    ///   * `refused`          — `dispatch-safe` mode AND at least one
    ///                          safety gate failed (or substrate refused).
    ///                          Non-retryable.
    ///   * `failed`           — `dispatch-safe` mode AND the substrate
    ///                          dispatched but the inner handler returned
    ///                          an error. Non-retryable.
    pub status: RollbackStatus,
    /// Human-readable explanation of the per-compensation-node decision.
    pub reason: String,
    /// Resolved objective for this compensation node (may be empty).
    pub objective: Option<String>,
    /// Resolved owned-files list for this compensation node.
    pub owned_files: Vec<String>,
    /// Resolved acceptance commands surfaced verbatim — NEVER executed.
    pub acceptance_commands: Vec<String>,
    /// Trimmed task-brief preview when the substrate / pure helper built
    /// one. `None` for pure-plan-mode entries.
    pub task_brief_preview: Option<String>,
    /// File path the brief was mirrored to (currently always `None` —
    /// substrate does not yet write the brief to disk; kept for shape
    /// compatibility with the node-local rollback evaluation).
    pub task_brief_path: Option<String>,
    /// Inner dispatch payload from `run_workstation_dispatch` when the
    /// compensation was actually dispatched.
    pub inner_payload: Option<Value>,
}

impl CascadeCompensationOutcome {
    pub(super) fn to_json(&self) -> Value {
        let mut v = json!({
            "node_id": self.node_id,
            "policy": self.policy.as_wire(),
            "status": self.status.as_wire(),
            "reason": self.reason,
            "objective": self.objective,
            "owned_files": self.owned_files,
            "acceptance_commands": self.acceptance_commands,
            "acceptance_commands_executed": false,
        });
        if let Some(p) = self.task_brief_preview.as_deref() {
            v["task_brief_preview"] = json!(p);
        }
        if let Some(p) = self.task_brief_path.as_deref() {
            v["task_brief_path"] = json!(p);
        }
        if let Some(inner) = self.inner_payload.clone() {
            v["inner_result"] = inner;
        }
        v
    }
}

/// wave-18 / task 04 — top-level outcome of the cascade rollback pass
/// for a single failed (cascade root) node. Carries the resolved cascade
/// mode + the ordered list of compensation outcomes so observers can
/// audit "which compensation nodes were planned / dispatched / refused"
/// without re-walking the prior nodes.
///
/// `is_inactive()` is true iff the cascade evaluator was either skipped
/// entirely (no compensation nodes found AND mode=None) OR ran but
/// produced no observable signal — the wave loop suppresses the cascade
/// surface in that case so the wave-17 / task 04 byte shape stays untouched
/// for plans that did not opt into cascading.
#[derive(Debug, Clone)]
pub(super) struct CascadeRollbackOutcome {
    pub mode: RollbackCascadeMode,
    /// Cascade root: the failed node id whose compensation is being planned.
    pub cascade_root: String,
    /// Compensation nodes in resolved cascade order. Empty when no plan
    /// node carries `:compensates "<cascade_root>"`.
    pub compensations: Vec<CascadeCompensationOutcome>,
    /// Human-readable explanation of the cascade-level decision (e.g.
    /// "cascade plan recorded; 2 compensation nodes",
    /// "no compensation nodes declared", etc.).
    pub reason: String,
}

impl CascadeRollbackOutcome {
    /// Convenience: this outcome produced no observable cascade signal.
    /// Used by the scheduler to decide whether to surface the cascade
    /// block on the response / evidence row.
    pub(super) fn is_inactive(&self) -> bool {
        matches!(self.mode, RollbackCascadeMode::None) && self.compensations.is_empty()
    }

    /// Project the outcome as a JSON block suitable for
    /// `node_results[].rollback.cascade` / `evidence.rollback.cascade`.
    pub(super) fn to_json(&self) -> Value {
        let comps: Vec<Value> = self.compensations.iter().map(|c| c.to_json()).collect();
        json!({
            "mode": self.mode.as_wire(),
            "cascade_root": self.cascade_root,
            "reason": self.reason,
            "compensations": comps,
        })
    }
}

/// wave-17 / task 04 — pure helper that derives the rollback descriptor
/// data (objective + owned files + acceptance commands + resolved
/// policy) from the node hints. Decoupled from any IO so unit tests can
/// pin the shape without standing up an `AppState`.
///
/// Returns the resolved policy and the descriptor payload. The actual
/// dispatch decision (refused vs dispatched) belongs to the wave loop —
/// this helper only produces the inputs.
pub(super) fn build_rollback_descriptor(node: &DagNode) -> RollbackDescriptor {
    let policy = node.rollback_policy_kind().unwrap_or(RollbackPolicy::None);
    let objective = node
        .rollback_objective
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let owned_files =
        super::super::plan::split_lisp_string_list(node.rollback_owned_files_raw.as_deref());
    let acceptance_commands = super::super::plan::split_lisp_string_list(
        node.rollback_acceptance_commands_raw.as_deref(),
    );
    RollbackDescriptor {
        policy,
        objective,
        owned_files,
        acceptance_commands,
    }
}

/// wave-17 / task 04 — descriptor inputs derived from a node's
/// `:rollback-*` hints. Does NOT carry any decision yet; the wave loop
/// (or the test fixtures) consume this to evaluate safety + dispatch.
#[derive(Debug, Clone)]
pub(super) struct RollbackDescriptor {
    pub policy: RollbackPolicy,
    pub objective: Option<String>,
    pub owned_files: Vec<String>,
    pub acceptance_commands: Vec<String>,
}

impl RollbackDescriptor {
    /// Project the descriptor as a `WorkstationDispatchHints` value the
    /// substrate consumes. The rollback brief reuses the wave-15
    /// task-brief shape so observers see the same headings as a
    /// forward task brief.
    pub(super) fn to_workstation_hints(
        &self,
        node: &DagNode,
    ) -> super::super::workstation_dispatch::WorkstationDispatchHints {
        super::super::workstation_dispatch::WorkstationDispatchHints {
            objective: self.objective.clone(),
            // Free-form scope explains the rollback intent so the
            // delegated agent never confuses a rollback brief with a
            // forward brief.
            scope: Some(format!(
                "rollback for failed plan-DAG node `{}` (target=`{}`)",
                node.id, node.target
            )),
            owned_files: self.owned_files.clone(),
            // Forbidden files for the rollback brief mirror any forward
            // forbidden hints so the rollback agent inherits the same
            // safety boundary.
            forbidden_files: super::super::plan::split_lisp_string_list(
                node.forbidden_files_raw.as_deref(),
            ),
            acceptance_commands: self.acceptance_commands.clone(),
            commit_policy: node.commit_policy.clone().or(Some("scoped".to_string())),
            target_project: node.target_project.clone(),
            requested_cwd: node.requested_cwd.clone(),
            // Rollback dispatch reuses the forward dispatch strategy so
            // the same workstation backend handles both.
            dispatch_strategy: node.dispatch_strategy.clone(),
        }
    }

    /// Determine whether the descriptor satisfies every safety
    /// requirement to dispatch a rollback through the workstation
    /// substrate. Pure: no side effects. Returns `Ok(())` when safe,
    /// `Err(reason)` with the human-readable failing condition
    /// otherwise. The reason vocabulary is stable so dashboards can
    /// pivot on it.
    pub(super) fn safety_check_for_workstation(
        &self,
        node: &DagNode,
    ) -> std::result::Result<(), String> {
        // 1. Objective must be present + non-empty.
        let has_obj = self
            .objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_obj {
            return Err(
                "rollback workstation dispatch requires :rollback-objective (non-empty)"
                    .to_string(),
            );
        }
        // 2. At least one owned file must be declared. Workstation
        //    dispatch with no owned files would let the rollback agent
        //    touch arbitrary parts of the tree — the exact thing the
        //    scoped-commit invariant exists to prevent.
        if self.owned_files.is_empty() {
            return Err(
                "rollback workstation dispatch requires :rollback-owned-files (>= 1 entry)"
                    .to_string(),
            );
        }
        // 3. Project must be resolvable. We check the static signal
        //    (target_project / requested_cwd present); the substrate
        //    re-validates via `resolve_target_project_root` so absence
        //    of either signal would always result in
        //    `SafeDescriptorReason::ProjectRootUnresolved`. Catching
        //    it here turns the refusal into a friendlier
        //    "no project signal" message rather than a downstream
        //    resolver error.
        let has_project = node
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let has_cwd = node
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_project && !has_cwd {
            return Err(
                "rollback workstation dispatch requires :target-project or :requested-cwd \
                 to resolve a project root"
                    .to_string(),
            );
        }
        // 4. Dispatch strategy must be on the inferable whitelist —
        //    `unknown` / `prompt-fallback` are forward-only paths and
        //    are not safe to ride for a destructive rollback.
        let strat = node
            .dispatch_strategy
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        if !super::super::workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES.contains(&strat) {
            return Err(format!(
                "rollback workstation dispatch requires :dispatch-strategy on the safe \
                 whitelist {:?}; got `{}`",
                super::super::workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES,
                strat
            ));
        }
        Ok(())
    }
}

/// wave-17 / task 04 — pure helper composing the descriptor + the
/// safety check + a static decision (no IO). Intended for unit tests
/// that pin "given hints X, what status / reason would the wave loop
/// land on BEFORE dispatch?". The wave loop always re-runs the
/// safety check before invoking the substrate so this helper and the
/// runtime cannot drift.
pub(super) fn pre_dispatch_rollback_decision(node: &DagNode) -> RollbackEvaluation {
    let descriptor = build_rollback_descriptor(node);
    match descriptor.policy {
        RollbackPolicy::None => RollbackEvaluation {
            policy: RollbackPolicy::None,
            status: if node.has_rollback_hints() {
                // Author declared SOME rollback hint but explicitly
                // wrote `:rollback-policy "none"`. Surface as
                // `not_requested` (the explicit-none decision dominates)
                // so the response stays quiet.
                RollbackStatus::NotRequested
            } else {
                RollbackStatus::NotRequested
            },
            reason: if node.has_rollback_hints() {
                "rollback policy explicitly set to none; no rollback dispatch".to_string()
            } else {
                "no rollback hints declared".to_string()
            },
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        },
        RollbackPolicy::Descriptor => RollbackEvaluation {
            policy: RollbackPolicy::Descriptor,
            status: RollbackStatus::DescriptorReady,
            reason: "descriptor mode: rollback intent recorded; no dispatch performed".to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        },
        RollbackPolicy::Workstation => match descriptor.safety_check_for_workstation(node) {
            Ok(()) => RollbackEvaluation {
                policy: RollbackPolicy::Workstation,
                status: RollbackStatus::Refused,
                reason:
                    "workstation mode passed pre-dispatch safety; runtime will attempt dispatch"
                        .to_string(),
                objective: descriptor.objective,
                owned_files: descriptor.owned_files,
                acceptance_commands: descriptor.acceptance_commands,
                task_brief_preview: None,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            },
            Err(detail) => RollbackEvaluation {
                policy: RollbackPolicy::Workstation,
                status: RollbackStatus::Refused,
                reason: format!("rollback workstation dispatch refused: {}", detail),
                objective: descriptor.objective,
                owned_files: descriptor.owned_files,
                acceptance_commands: descriptor.acceptance_commands,
                task_brief_preview: None,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            },
        },
    }
}

/// wave-17 / task 04 — execute the conservative rollback pass for a
/// just-failed node. Pure async wrapper over the descriptor /
/// safety-check / optional-dispatch pipeline so the wave loop's
/// final-failure branch can call a single helper.
///
/// Behaviour matrix (matches the wave-17 / task 04 brief):
///   * No rollback hints OR `:rollback-policy "none"` →
///     `RollbackEvaluation { status: NotRequested, ... }` and the
///     scheduler skips the rollback evidence emit entirely.
///   * `:rollback-policy "descriptor"` → fully-populated descriptor
///     evaluation with `status=DescriptorReady`, no dispatch attempt.
///   * `:rollback-policy "workstation"` + safety check fails →
///     `status=Refused` with the failing condition spelled out, no
///     dispatch attempt. SafeDescriptor refusals from the substrate
///     also collapse to `Refused`.
///   * `:rollback-policy "workstation"` + safety check passes →
///     dispatch via `run_workstation_dispatch`. On success
///     `status=Dispatched` (with brief preview + inner payload). On
///     inner-handler error `status=Failed` with the error message on
///     the reason. SafeDescriptor refusals (which can still surface
///     even after the static safety check passes — e.g. resolver
///     reports a non-existent project root) become `Refused` so the
///     non-retryable refusal vocabulary stays consistent across all
///     workstation-substrate consumers.
pub(super) async fn run_rollback(
    state: &AppState,
    plan: &Plan,
    node: &DagNode,
) -> RollbackEvaluation {
    let descriptor = build_rollback_descriptor(node);
    match descriptor.policy {
        RollbackPolicy::None => RollbackEvaluation {
            policy: RollbackPolicy::None,
            status: RollbackStatus::NotRequested,
            reason: if node.has_rollback_hints() {
                "rollback policy explicitly set to none; no rollback dispatch".to_string()
            } else {
                "no rollback hints declared".to_string()
            },
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        },
        RollbackPolicy::Descriptor => {
            // Build the descriptor brief locally so observers see the
            // same shape they would for a forward task brief, but
            // NEVER dispatch.
            let hints = descriptor.to_workstation_hints(node);
            let strategy = node.dispatch_strategy.as_deref().unwrap_or("unknown");
            let preview = if descriptor.objective.is_some() {
                Some(truncate_rollback_brief_preview(
                    &super::super::workstation_dispatch::build_task_brief(plan, &hints, strategy),
                ))
            } else {
                None
            };
            RollbackEvaluation {
                policy: RollbackPolicy::Descriptor,
                status: RollbackStatus::DescriptorReady,
                reason: "descriptor mode: rollback intent recorded; no dispatch performed"
                    .to_string(),
                objective: descriptor.objective.clone(),
                owned_files: descriptor.owned_files.clone(),
                acceptance_commands: descriptor.acceptance_commands.clone(),
                task_brief_preview: preview,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            }
        }
        RollbackPolicy::Workstation => {
            // Run the static safety check first so a refusal here
            // never touches the substrate. SafeDescriptor refusals
            // are non-retryable per the wave-15 contract.
            if let Err(reason) = descriptor.safety_check_for_workstation(node) {
                return RollbackEvaluation {
                    policy: RollbackPolicy::Workstation,
                    status: RollbackStatus::Refused,
                    reason: format!("rollback workstation dispatch refused: {}", reason),
                    objective: descriptor.objective,
                    owned_files: descriptor.owned_files,
                    acceptance_commands: descriptor.acceptance_commands,
                    task_brief_preview: None,
                    task_brief_path: None,
                    inner_payload: None,
                    cascade: None,
                };
            }
            // Static safety passed — dispatch through the substrate.
            // The substrate may STILL refuse (e.g. cwd not absolute,
            // project registry miss); we map every SafeDescriptor
            // refusal back to `Refused` so the non-retryable
            // vocabulary stays consistent.
            let hints = descriptor.to_workstation_hints(node);
            let strategy = node.dispatch_strategy.as_deref().unwrap_or("unknown");
            let outcome = super::super::workstation_dispatch::run_workstation_dispatch(
                state,
                plan,
                "mission_task_delegate",
                strategy,
                hints,
                false,
            )
            .await;
            match outcome {
                super::super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched {
                    task_brief,
                    task_brief_path,
                    inner_payload,
                    ..
                } => RollbackEvaluation {
                    policy: RollbackPolicy::Workstation,
                    status: RollbackStatus::Dispatched,
                    reason: "rollback workstation dispatch completed; inner handler returned Ok"
                        .to_string(),
                    objective: descriptor.objective,
                    owned_files: descriptor.owned_files,
                    acceptance_commands: descriptor.acceptance_commands,
                    task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                    task_brief_path,
                    inner_payload: Some(inner_payload),
                    cascade: None,
                },
                super::super::workstation_dispatch::WorkstationDispatchOutcome::DryRun { task_brief } => {
                    // The wave loop never asks for dry_run on rollback
                    // (we always pass dry_run=false above). Defensive:
                    // if a future caller flips the knob we surface as
                    // dispatched with no inner payload so observers
                    // don't see a missing variant.
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Dispatched,
                        reason: "rollback dispatched in dry_run mode (no real handler invoked)"
                            .to_string(),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                        task_brief_path: None,
                        inner_payload: None,
                        cascade: None,
                    }
                }
                super::super::workstation_dispatch::WorkstationDispatchOutcome::InnerError {
                    task_brief,
                    inner_payload,
                } => {
                    let detail = inner_payload
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rollback inner handler returned error")
                        .to_string();
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Failed,
                        reason: format!("rollback workstation dispatch failed: {}", detail),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                        task_brief_path: None,
                        inner_payload: Some(inner_payload),
                        cascade: None,
                    }
                }
                super::super::workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor {
                    reason,
                    task_brief,
                } => {
                    // Substrate-side safety refusal — collapse to
                    // Refused so the wave loop treats it as
                    // non-retryable (mirrors wave-15 / task 05).
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Refused,
                        reason: format!(
                            "rollback workstation dispatch refused (substrate): {}",
                            reason.detail()
                        ),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: task_brief
                            .as_deref()
                            .map(truncate_rollback_brief_preview),
                        task_brief_path: None,
                        inner_payload: None,
                        cascade: None,
                    }
                }
            }
        }
    }
}

/// wave-17 / task 04 — local copy of the workstation-dispatch preview
/// truncation so the rollback evaluation block surfaces a humane
/// preview without taking a dep on the substrate's private helper.
/// Same MAX (800 chars) so previews look identical across surfaces.
fn truncate_rollback_brief_preview(brief: &str) -> String {
    const MAX: usize = 800;
    if brief.len() <= MAX {
        return brief.to_string();
    }
    let mut end = MAX;
    while end > 0 && !brief.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &brief[..end])
}
