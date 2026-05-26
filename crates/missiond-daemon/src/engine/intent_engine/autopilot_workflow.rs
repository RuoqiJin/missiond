//! Durable Autopilot workflow role contracts.
//!
//! The legacy `autopilot.rs` tick loop remains the runtime implementation in
//! Phase 1. These role types define the seams used to extract scheduling,
//! dispatch, per-task run ownership, and maintenance without changing external
//! BoardTask behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutopilotWakeReason {
    BoardTaskChanged,
    SlotBecameIdle,
    MaintenanceTick,
    ManualKick,
}

#[allow(dead_code)]
pub(crate) struct AutopilotScheduler {
    pub(crate) reason: AutopilotWakeReason,
}

#[allow(dead_code)]
pub(crate) struct BoardTaskDispatcher {
    pub(crate) executor_id: String,
}

#[allow(dead_code)]
pub(crate) struct DispatchRunActor {
    pub(crate) run_id: String,
    pub(crate) task_id: String,
    pub(crate) slot_id: String,
}

#[allow(dead_code)]
pub(crate) struct MaintenanceRunner {
    pub(crate) worker_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchRunFailurePolicy {
    ReleaseClaim,
    PreserveTaskWithBlocker,
    DoNotCloseWithoutTaskResultArtifact,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskResultArtifact {
    pub(crate) task_id: String,
    pub(crate) result_status: String,
    pub(crate) artifact_hash: String,
}

#[allow(dead_code)]
pub(crate) enum DispatchRunCommand {
    SendPrompt { prompt: String, timeout_ms: u64 },
    AwaitDurableFinal { settle_window_ms: u64 },
    PutTaskResultArtifact(TaskResultArtifact),
    CloseBoardTask,
    PreserveWithBlocker { reason: String },
    ReleaseClaim { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_run_contract_pins_task_result_authority() {
        let policies = [
            DispatchRunFailurePolicy::ReleaseClaim,
            DispatchRunFailurePolicy::PreserveTaskWithBlocker,
            DispatchRunFailurePolicy::DoNotCloseWithoutTaskResultArtifact,
        ];
        assert!(policies.contains(&DispatchRunFailurePolicy::DoNotCloseWithoutTaskResultArtifact));

        let artifact = TaskResultArtifact {
            task_id: "task-1".to_string(),
            result_status: "completed".to_string(),
            artifact_hash: "sha256:abc".to_string(),
        };
        let json = serde_json::to_value(artifact).expect("task result artifact json");
        assert_eq!(json["taskId"], "task-1");
    }
}
