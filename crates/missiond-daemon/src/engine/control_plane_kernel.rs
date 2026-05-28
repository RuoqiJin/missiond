use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::{json, Value};

use crate::engine::shared_memory::{CapabilityCheckRequest, ClaimRequest, WorkerSettleRequest};
use crate::engine::task_completion_evidence::{
    TaskCompletionEvidenceInput, TaskCompletionEvidenceWriter,
};
use crate::state::AppState;

#[derive(Debug, Clone)]
pub(crate) struct SystemTaskCompletionInput {
    pub task_id: String,
    pub project_id: Option<String>,
    pub producer_id: String,
    pub summary: String,
    pub content: Option<String>,
    pub raw_evidence: Value,
    pub evidence_refs: Vec<Value>,
    pub result_status: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct SettleTaskCommand {
    pub task_id: String,
    pub project_id: Option<String>,
    pub slot_id: Option<String>,
    pub conversation_id: Option<String>,
    pub artifact_hash: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub attempt_id: Option<String>,
    pub allow_system_bypass: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RecordObservationCommand {
    pub task_id: String,
    pub project_id: Option<String>,
    pub producer_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct StartAttemptCommand {
    pub task_id: String,
    pub project_id: Option<String>,
    pub attempt_id: String,
    pub agent_id: String,
    pub worker_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaimLeaseCommand {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub owner_id: String,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub scope_kind: String,
    pub scope_key: String,
    pub lease_secs: i64,
    pub metadata: Value,
    pub allow_system_bypass: bool,
    pub bypass_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReleaseLeaseCommand {
    pub claim_id: String,
    pub owner_id: Option<String>,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub details: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct RequireCapabilityCommand {
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub operation: String,
    pub scope_kind: String,
    pub scope_key: String,
    pub task_id: Option<String>,
    pub allow_system_bypass: bool,
    pub bypass_reason: Option<String>,
    pub details: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct GrantTaskCapabilitiesCommand {
    pub project_id: Option<String>,
    pub task_id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub read_scope: Vec<String>,
    pub write_scope: Vec<String>,
    pub must_not_touch: Vec<String>,
    pub issuer: String,
}

pub(crate) struct ControlPlaneKernel<'a> {
    state: &'a AppState,
}

impl<'a> ControlPlaneKernel<'a> {
    pub(crate) fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub(crate) async fn record_observation(
        &self,
        task_id: &str,
        producer_id: &str,
        payload: Value,
    ) -> Result<Value> {
        self.record_observation_command(RecordObservationCommand {
            task_id: task_id.to_string(),
            project_id: None,
            producer_id: producer_id.to_string(),
            payload,
        })
        .await
    }

    pub(crate) async fn record_observation_command(
        &self,
        command: RecordObservationCommand,
    ) -> Result<Value> {
        self.state
            .shared_memory
            .record_job_event_typed(
                command.task_id.as_str(),
                command.project_id.as_deref(),
                command.producer_id.as_str(),
                "observation.recorded",
                command.payload,
            )
            .await
    }

    pub(crate) async fn start_attempt_command(
        &self,
        command: StartAttemptCommand,
    ) -> Result<Value> {
        self.state
            .shared_memory
            .job_event_typed(json!({
                "task_id": command.task_id,
                "project_id": command.project_id,
                "event_kind": "attempt.started",
                "attempt_id": command.attempt_id,
                "agent_id": command.agent_id,
                "worker_id": command.worker_id,
                "payload": command.payload,
            }))
            .await
    }

    pub(crate) async fn write_completion_artifact(
        &self,
        input: TaskCompletionEvidenceInput,
    ) -> Result<crate::engine::task_completion_evidence::TaskCompletionEvidenceResult> {
        // Control-plane ABI anchor: typed task-result artifact command.
        TaskCompletionEvidenceWriter::new(self.state.shared_memory.clone())
            .write_bounded(input)
            .await
    }

    pub(crate) async fn settle_task(
        &self,
        task_id: &str,
        artifact_hash: &str,
        summary: &str,
        producer_id: &str,
    ) -> Result<Value> {
        self.settle_task_command(SettleTaskCommand {
            task_id: task_id.to_string(),
            project_id: None,
            slot_id: Some(producer_id.to_string()),
            conversation_id: None,
            artifact_hash: Some(artifact_hash.to_string()),
            status: "done".to_string(),
            summary: Some(summary.to_string()),
            grant_id: None,
            subject_kind: "system".to_string(),
            subject_id: producer_id.to_string(),
            attempt_id: None,
            allow_system_bypass: true,
        })
        .await
    }

    pub(crate) async fn settle_task_command(&self, command: SettleTaskCommand) -> Result<Value> {
        self.state
            .shared_memory
            .settle_worker_command(WorkerSettleRequest {
                task_id: command.task_id,
                project_id: command.project_id,
                slot_id: command.slot_id,
                conversation_id: command.conversation_id,
                artifact_hash: command.artifact_hash,
                status: command.status,
                summary: command.summary,
                grant_id: command.grant_id,
                subject_kind: command.subject_kind,
                subject_id: command.subject_id,
                attempt_id: command.attempt_id,
                allow_system_bypass: command.allow_system_bypass,
            })
            .await
    }

    pub(crate) async fn claim_lease(
        &self,
        task_id: &str,
        owner_id: &str,
        scope_kind: &str,
        scope_key: &str,
    ) -> Result<Value> {
        self.claim_lease_command(ClaimLeaseCommand {
            project_id: None,
            task_id: Some(task_id.to_string()),
            owner_id: owner_id.to_string(),
            grant_id: None,
            subject_kind: "system".to_string(),
            subject_id: "control-plane-kernel".to_string(),
            scope_kind: scope_kind.to_string(),
            scope_key: scope_key.to_string(),
            lease_secs: 1800,
            metadata: json!({
                "source": "control-plane-kernel"
            }),
            allow_system_bypass: true,
            bypass_reason: Some("internal control-plane kernel lease authority".to_string()),
        })
        .await
    }

    pub(crate) async fn claim_lease_command(&self, command: ClaimLeaseCommand) -> Result<Value> {
        self.state
            .shared_memory
            .claim_lease_typed(ClaimRequest {
                project_id: command.project_id,
                task_id: command.task_id,
                owner_id: command.owner_id,
                grant_id: command.grant_id,
                subject_kind: command.subject_kind,
                subject_id: command.subject_id,
                scope_kind: command.scope_kind,
                scope_key: command.scope_key,
                lease_secs: command.lease_secs,
                metadata: command.metadata,
                allow_system_bypass: command.allow_system_bypass,
                bypass_reason: command.bypass_reason,
            })
            .await
    }

    pub(crate) async fn release_lease_command(
        &self,
        command: ReleaseLeaseCommand,
    ) -> Result<Value> {
        self.state
            .shared_memory
            .release_typed(json!({
                "claim_id": command.claim_id,
                "owner_id": command.owner_id,
                "grant_id": command.grant_id,
                "subject_kind": command.subject_kind,
                "subject_id": command.subject_id,
                "details": command.details,
            }))
            .await
    }

    pub(crate) async fn require_capability(
        &self,
        task_id: &str,
        operation: &str,
        scope_kind: &str,
        scope_key: &str,
    ) -> Result<Value> {
        let grant_id = self
            .require_capability_command(RequireCapabilityCommand {
                grant_id: None,
                subject_kind: "system".to_string(),
                subject_id: "control-plane-kernel".to_string(),
                operation: operation.to_string(),
                scope_kind: scope_kind.to_string(),
                scope_key: scope_key.to_string(),
                task_id: Some(task_id.to_string()),
                allow_system_bypass: true,
                bypass_reason: Some("internal control-plane kernel authority".to_string()),
                details: json!({}),
            })
            .await?;
        Ok(json!({
            "schema": "missiond.capability-check.v1",
            "ok": true,
            "grant_id": grant_id,
            "task_id": task_id,
            "operation": operation,
            "scope_kind": scope_kind,
            "scope_key": scope_key
        }))
    }

    pub(crate) async fn require_capability_command(
        &self,
        command: RequireCapabilityCommand,
    ) -> Result<String> {
        self.state
            .shared_memory
            .require_capability(CapabilityCheckRequest {
                grant_id: command.grant_id,
                subject_kind: command.subject_kind,
                subject_id: command.subject_id,
                operation: command.operation,
                scope_kind: command.scope_kind,
                scope_key: command.scope_key,
                task_id: command.task_id,
                allow_system_bypass: command.allow_system_bypass,
                bypass_reason: command.bypass_reason,
                details: command.details,
            })
            .await
    }

    pub(crate) async fn grant_task_capabilities_command(
        &self,
        command: GrantTaskCapabilitiesCommand,
    ) -> Result<Vec<String>> {
        self.state
            .shared_memory
            .grant_task_capabilities(
                command.project_id.as_deref(),
                command.task_id.as_str(),
                command.subject_kind.as_str(),
                command.subject_id.as_str(),
                &command.read_scope,
                &command.write_scope,
                &command.must_not_touch,
                command.issuer.as_str(),
            )
            .await
    }

    pub(crate) async fn project_board_view(
        &self,
        task_id: &str,
        projected_status: &str,
        payload: Value,
    ) -> Result<Value> {
        self.state
            .shared_memory
            .record_job_event_typed(
                task_id,
                None,
                "control-plane-kernel",
                "observation.recorded",
                json!({
                    "schema": "missiond.board-task-view-projection-request.v1",
                    "projected_status": projected_status,
                    "projection": payload
                }),
            )
            .await
    }

    pub(crate) async fn complete_system_task(
        &self,
        input: SystemTaskCompletionInput,
    ) -> Result<Value> {
        let task = self
            .state
            .store
            .get_board_task(&input.task_id)
            .await?
            .ok_or_else(|| anyhow!("BoardTask {} not found", input.task_id))?;
        let project_id = input
            .project_id
            .clone()
            .or_else(|| task.project.clone())
            .unwrap_or_else(|| "missiond".to_string());
        let task_id = input.task_id.clone();
        let runtime_metadata = json!({
            "schema": "missiond.runtime-task-metadata.v1",
            "source": "control-plane-kernel",
            "task_contract_id": format!("board-task:{task_id}"),
            "dispatch_metadata": {
                "project_id": project_id,
                "task_id": task_id,
                "read_scope": [],
                "write_scope": [],
                "must_not_touch": [],
                "control_state": "task_contracts",
                "authority": "system"
            },
            "sandbox_profile": "system-no-sandbox"
        });
        self.state
            .shared_memory
            .ensure_task_contract_from_metadata(
                input.task_id.as_str(),
                Some(project_id.as_str()),
                &runtime_metadata,
            )
            .await?;

        let created_at = Utc::now().to_rfc3339();
        let evidence_refs = if input.evidence_refs.is_empty() {
            vec![json!({
                "kind": "system_fact",
                "producer_id": input.producer_id,
                "created_at": created_at
            })]
        } else {
            input.evidence_refs.clone()
        };
        let artifact = self
            .write_completion_artifact(TaskCompletionEvidenceInput {
                task_id: input.task_id.clone(),
                project_id: Some(project_id.clone()),
                slot_id: None,
                conversation_id: None,
                provider: input.producer_id.clone(),
                result_status: input.result_status.clone(),
                summary: input.summary.clone(),
                content: input
                    .content
                    .clone()
                    .or_else(|| Some(input.summary.clone())),
                json: input.metadata.clone(),
                accepted_shard_id: None,
                attempt_id: None,
                capability_grant_id: None,
                subject_kind: Some("system".to_string()),
                subject_id: Some(input.producer_id.clone()),
                confirm: Some(true),
                producer: Some(json!({
                    "kind": "system",
                    "id": input.producer_id,
                    "created_at": created_at,
                    "source": "control-plane-kernel"
                })),
                raw_evidence: Some(input.raw_evidence.clone()),
                evidence_refs: Some(Value::Array(evidence_refs)),
                created_at: Some(created_at),
            })
            .await?;
        let settle = self
            .settle_task(
                input.task_id.as_str(),
                artifact.artifact_hash.as_str(),
                input.summary.as_str(),
                input.producer_id.as_str(),
            )
            .await?;
        Ok(json!({
            "schema": "missiond.control-plane-kernel.system-completion.v1",
            "ok": true,
            "task_id": input.task_id,
            "artifact_hash": artifact.artifact_hash,
            "artifact": artifact.response,
            "settle": settle
        }))
    }
}
