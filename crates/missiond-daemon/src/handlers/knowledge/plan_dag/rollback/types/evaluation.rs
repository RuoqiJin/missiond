use serde_json::{json, Value};

use super::cascade::CascadeRollbackOutcome;
use super::policy::RollbackPolicy;

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
