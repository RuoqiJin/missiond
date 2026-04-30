use crate::handlers::knowledge::{plan, workstation_dispatch};

use super::super::DagNode;
use super::RollbackPolicy;
#[cfg(test)]
use super::{RollbackEvaluation, RollbackStatus};

/// wave-17 / task 04 — pure helper that derives the rollback descriptor
/// data (objective + owned files + acceptance commands + resolved
/// policy) from the node hints. Decoupled from any IO so unit tests can
/// pin the shape without standing up an `AppState`.
///
/// Returns the resolved policy and the descriptor payload. The actual
/// dispatch decision (refused vs dispatched) belongs to the wave loop —
/// this helper only produces the inputs.
pub(in crate::handlers::knowledge::plan_dag) fn build_rollback_descriptor(
    node: &DagNode,
) -> RollbackDescriptor {
    let policy = node.rollback_policy_kind().unwrap_or(RollbackPolicy::None);
    let objective = node
        .rollback_objective
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let owned_files = plan::split_lisp_string_list(node.rollback_owned_files_raw.as_deref());
    let acceptance_commands =
        plan::split_lisp_string_list(node.rollback_acceptance_commands_raw.as_deref());
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
pub(in crate::handlers::knowledge::plan_dag) struct RollbackDescriptor {
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
    pub(in crate::handlers::knowledge::plan_dag) fn to_workstation_hints(
        &self,
        node: &DagNode,
    ) -> workstation_dispatch::WorkstationDispatchHints {
        workstation_dispatch::WorkstationDispatchHints {
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
            forbidden_files: plan::split_lisp_string_list(node.forbidden_files_raw.as_deref()),
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
    pub(in crate::handlers::knowledge::plan_dag) fn safety_check_for_workstation(
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
        if !workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES.contains(&strat) {
            return Err(format!(
                "rollback workstation dispatch requires :dispatch-strategy on the safe \
                 whitelist {:?}; got `{}`",
                workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES,
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
#[cfg(test)]
pub(in crate::handlers::knowledge::plan_dag) fn pre_dispatch_rollback_decision(
    node: &DagNode,
) -> RollbackEvaluation {
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
