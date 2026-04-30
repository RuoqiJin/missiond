use serde_json::{json, Value};

use super::super::DagNode;

impl DagNode {
    /// wave-17 / task 04 — typed projection of `:rollback-policy`.
    /// Returns `None` when the author did not declare a policy OR wrote
    /// an unrecognised value (the parser also pushes unrecognised
    /// values into `unsupported_fields` so the typo is loud).
    pub(in crate::handlers::knowledge::plan_dag) fn rollback_policy_kind(
        &self,
    ) -> Option<RollbackPolicy> {
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
    pub(in crate::handlers::knowledge::plan_dag) fn rollback_cascade_kind(
        &self,
    ) -> Option<RollbackCascadeMode> {
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
    pub(in crate::handlers::knowledge::plan_dag) fn has_active_rollback_cascade(&self) -> bool {
        matches!(
            self.rollback_cascade_kind(),
            Some(RollbackCascadeMode::Plan) | Some(RollbackCascadeMode::DispatchSafe)
        )
    }

    /// wave-17 / task 04 — true iff this node opted into ANY rollback
    /// hint (policy / objective / owned files / acceptance commands).
    /// Used to skip the rollback evaluator entirely on the wave-13
    /// byte-shape path (no hints declared → no rollback evidence row).
    pub(in crate::handlers::knowledge::plan_dag) fn has_rollback_hints(&self) -> bool {
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
pub(in crate::handlers::knowledge::plan_dag) enum RollbackPolicy {
    None,
    Descriptor,
    Workstation,
}

impl RollbackPolicy {
    pub(in crate::handlers::knowledge::plan_dag) fn as_wire(self) -> &'static str {
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
    pub(in crate::handlers::knowledge::plan_dag) fn parse(raw: &str) -> Option<Self> {
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
pub(in crate::handlers::knowledge::plan_dag) enum RollbackStatus {
    NotRequested,
    DescriptorReady,
    Dispatched,
    Refused,
    Failed,
}

impl RollbackStatus {
    pub(in crate::handlers::knowledge::plan_dag) fn as_wire(self) -> &'static str {
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
pub(in crate::handlers::knowledge::plan_dag) struct RollbackEvaluation {
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
    pub(in crate::handlers::knowledge::plan_dag) fn is_inactive(&self) -> bool {
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
    pub(in crate::handlers::knowledge::plan_dag) fn to_json(&self) -> Value {
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
pub(in crate::handlers::knowledge::plan_dag) enum RollbackCascadeMode {
    None,
    Plan,
    DispatchSafe,
}

impl RollbackCascadeMode {
    pub(in crate::handlers::knowledge::plan_dag) fn as_wire(self) -> &'static str {
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
    pub(in crate::handlers::knowledge::plan_dag) fn parse(raw: &str) -> Option<Self> {
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
pub(in crate::handlers::knowledge::plan_dag) struct CascadeCompensationOutcome {
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
    pub(in crate::handlers::knowledge::plan_dag) fn to_json(&self) -> Value {
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
pub(in crate::handlers::knowledge::plan_dag) struct CascadeRollbackOutcome {
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
    pub(in crate::handlers::knowledge::plan_dag) fn is_inactive(&self) -> bool {
        matches!(self.mode, RollbackCascadeMode::None) && self.compensations.is_empty()
    }

    /// Project the outcome as a JSON block suitable for
    /// `node_results[].rollback.cascade` / `evidence.rollback.cascade`.
    pub(in crate::handlers::knowledge::plan_dag) fn to_json(&self) -> Value {
        let comps: Vec<Value> = self.compensations.iter().map(|c| c.to_json()).collect();
        json!({
            "mode": self.mode.as_wire(),
            "cascade_root": self.cascade_root,
            "reason": self.reason,
            "compensations": comps,
        })
    }
}
