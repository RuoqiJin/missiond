//! Durable Autopilot workflow role contracts.
//!
//! The legacy `autopilot.rs` tick loop remains the runtime implementation in
//! Phase 1. These role types define the seams used to extract scheduling,
//! dispatch, per-task run ownership, and maintenance without changing external
//! BoardTask behavior.

use missiond_domain::ids::{BoardTaskId, SlotId, WorkflowRunId};

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

impl AutopilotScheduler {
    #[allow(dead_code)]
    pub(crate) fn for_reason(reason: AutopilotWakeReason) -> Self {
        Self { reason }
    }

    #[allow(dead_code)]
    pub(crate) fn maintenance_tick() -> Self {
        Self {
            reason: AutopilotWakeReason::MaintenanceTick,
        }
    }
}

#[allow(dead_code)]
pub(crate) struct BoardTaskDispatcher {
    pub(crate) executor_id: String,
}

impl BoardTaskDispatcher {
    #[allow(dead_code)]
    pub(crate) fn for_executor(executor_id: impl Into<String>) -> Self {
        Self {
            executor_id: executor_id.into(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn claim_executor_type(&self) -> &'static str {
        "pty_slot"
    }
}

#[allow(dead_code)]
pub(crate) struct DispatchRunActor {
    pub(crate) run_id: WorkflowRunId,
    pub(crate) task_id: BoardTaskId,
    pub(crate) slot_id: SlotId,
}

impl DispatchRunActor {
    pub(crate) fn new(task_id: impl Into<String>, slot_id: impl Into<String>) -> Self {
        Self {
            run_id: WorkflowRunId::new(format!("dispatch-{}", uuid::Uuid::new_v4())),
            task_id: BoardTaskId::new(task_id),
            slot_id: SlotId::new(slot_id),
        }
    }

    /// Classify the current `pty.send` failure path without mutating durable
    /// Board state. The caller still owns the concrete note/retry writes.
    pub(crate) fn failure_policy_for_send_error(error_chain: &str) -> DispatchRunFailurePolicy {
        let lower = error_chain.to_ascii_lowercase();
        if lower.contains("timeout")
            && lower.contains("waiting for")
            && lower.contains("response from slot")
        {
            return DispatchRunFailurePolicy::DoNotCloseWithoutTaskResultArtifact;
        }
        if error_chain.contains("Cannot send message in state:") {
            DispatchRunFailurePolicy::ReleaseClaim
        } else {
            DispatchRunFailurePolicy::PreserveTaskWithBlocker
        }
    }
}

#[allow(dead_code)]
pub(crate) struct MaintenanceRunner {
    pub(crate) worker_id: String,
}

impl MaintenanceRunner {
    #[allow(dead_code)]
    pub(crate) fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn durable_tasks(&self) -> [&'static str; 6] {
        [
            "stale-lease-recovery",
            "slot-watchdog",
            "fts-dirty-rebuild",
            "claude-md-sync",
            "learning-tick",
            "operator-health-sample",
        ]
    }
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

    #[test]
    fn dispatch_run_send_failure_policy_releases_only_transient_slot_state() {
        assert_eq!(
            DispatchRunActor::failure_policy_for_send_error("Cannot send message in state: Busy"),
            DispatchRunFailurePolicy::ReleaseClaim
        );
        assert_eq!(
            DispatchRunActor::failure_policy_for_send_error("provider crashed"),
            DispatchRunFailurePolicy::PreserveTaskWithBlocker
        );
        assert_eq!(
            DispatchRunActor::failure_policy_for_send_error(
                "Timeout (600000ms) waiting for Codex response from slot slot-codex-review-worker"
            ),
            DispatchRunFailurePolicy::DoNotCloseWithoutTaskResultArtifact
        );
    }
}
