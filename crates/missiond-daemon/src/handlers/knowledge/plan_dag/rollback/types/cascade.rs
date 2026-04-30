use serde_json::{json, Value};

use super::evaluation::RollbackStatus;
use super::policy::RollbackPolicy;

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
