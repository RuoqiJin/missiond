use missiond_core::types::Plan;

use super::super::{
    build_rollback_descriptor, truncate_rollback_brief_preview, CascadeCompensationOutcome,
    DagNode, RollbackStatus,
};

/// wave-18 / task 04 — pure helper that builds a `plan`-mode cascade
/// outcome for a single compensation node. Records intent + brief
/// preview but never dispatches. Decoupled so unit tests can pin the
/// shape without standing up an `AppState`.
pub(in crate::handlers::knowledge::plan_dag) fn build_compensation_plan_entry(
    plan: &Plan,
    node: &DagNode,
) -> CascadeCompensationOutcome {
    let descriptor = build_rollback_descriptor(node);
    let policy = descriptor.policy;
    let hints = descriptor.to_workstation_hints(node);
    let strategy = node.dispatch_strategy.as_deref().unwrap_or("unknown");
    let preview = if descriptor.objective.is_some() {
        Some(truncate_rollback_brief_preview(
            &super::super::super::super::workstation_dispatch::build_task_brief(
                plan, &hints, strategy,
            ),
        ))
    } else {
        None
    };
    CascadeCompensationOutcome {
        node_id: node.id.clone(),
        policy,
        status: RollbackStatus::DescriptorReady,
        reason: "cascade plan: compensation node recorded; no dispatch performed".to_string(),
        objective: descriptor.objective,
        owned_files: descriptor.owned_files,
        acceptance_commands: descriptor.acceptance_commands,
        task_brief_preview: preview,
        task_brief_path: None,
        inner_payload: None,
    }
}
