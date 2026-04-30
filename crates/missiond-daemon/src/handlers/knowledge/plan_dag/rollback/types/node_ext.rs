use super::super::super::DagNode;
use super::cascade::RollbackCascadeMode;
use super::policy::RollbackPolicy;

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
