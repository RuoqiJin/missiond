use super::super::{
    truncate_rollback_brief_preview, CascadeCompensationOutcome, RollbackDescriptor,
    RollbackPolicy, RollbackStatus,
};

/// wave-18 / task 04 — pure helper translating a workstation-dispatch
/// outcome into a single cascade compensation row. Decoupled from the
/// async cascade body so unit tests can pin every dispatch branch.
pub(super) fn map_dispatch_outcome_to_compensation(
    node_id: String,
    descriptor: RollbackDescriptor,
    outcome: super::super::super::super::workstation_dispatch::WorkstationDispatchOutcome,
) -> CascadeCompensationOutcome {
    use super::super::super::super::workstation_dispatch::WorkstationDispatchOutcome as O;
    match outcome {
        O::Dispatched {
            task_brief,
            task_brief_path,
            inner_payload,
            ..
        } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Dispatched,
            reason:
                "cascade dispatch-safe: workstation dispatch completed; inner handler returned Ok"
                    .to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
            task_brief_path,
            inner_payload: Some(inner_payload),
        },
        O::DryRun { task_brief } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Dispatched,
            reason: "cascade dispatch-safe: substrate ran dry_run (no real handler invoked)"
                .to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
            task_brief_path: None,
            inner_payload: None,
        },
        O::InnerError {
            task_brief,
            inner_payload,
        } => {
            let detail = inner_payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("cascade compensation inner handler returned error")
                .to_string();
            CascadeCompensationOutcome {
                node_id,
                policy: RollbackPolicy::Workstation,
                status: RollbackStatus::Failed,
                reason: format!(
                    "cascade dispatch-safe: workstation dispatch failed: {}",
                    detail
                ),
                objective: descriptor.objective,
                owned_files: descriptor.owned_files,
                acceptance_commands: descriptor.acceptance_commands,
                task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                task_brief_path: None,
                inner_payload: Some(inner_payload),
            }
        }
        O::SafeDescriptor { reason, task_brief } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Refused,
            reason: format!(
                "cascade dispatch-safe refused (substrate): {}",
                reason.detail()
            ),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: task_brief.as_deref().map(truncate_rollback_brief_preview),
            task_brief_path: None,
            inner_payload: None,
        },
    }
}
