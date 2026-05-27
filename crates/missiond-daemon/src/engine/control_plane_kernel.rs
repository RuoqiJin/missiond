use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::{json, Value};

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
        self.state
            .shared_memory
            .handle_action(&json!({
                "action": "job_event",
                "task_id": task_id,
                "agent_id": producer_id,
                "event_kind": "observation.recorded",
                "payload": payload
            }))
            .await
    }

    pub(crate) async fn write_completion_artifact(
        &self,
        input: TaskCompletionEvidenceInput,
    ) -> Result<crate::engine::task_completion_evidence::TaskCompletionEvidenceResult> {
        // Control-plane ABI anchor: shared memory action `"action": "task_result_put"`.
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
        self.state
            .shared_memory
            .handle_action(&json!({
                "action": "worker_settle",
                "task_id": task_id,
                "status": "done",
                "artifact_hash": artifact_hash,
                "summary": summary,
                "slot_id": producer_id
            }))
            .await
    }

    pub(crate) async fn claim_lease(
        &self,
        task_id: &str,
        owner_id: &str,
        scope_kind: &str,
        scope_key: &str,
    ) -> Result<Value> {
        self.state
            .shared_memory
            .handle_action(&json!({
                "action": "claim",
                "task_id": task_id,
                "owner_id": owner_id,
                "scope_kind": scope_kind,
                "scope_key": scope_key,
                "metadata": {
                    "source": "control-plane-kernel"
                }
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
        self.state
            .shared_memory
            .handle_action(&json!({
                "action": "capability_check",
                "task_id": task_id,
                "operation": operation,
                "scope_kind": scope_kind,
                "scope_key": scope_key
            }))
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
            .handle_action(&json!({
                "action": "job_event",
                "task_id": task_id,
                "event_kind": "observation.recorded",
                "payload": {
                    "schema": "missiond.board-task-view-projection-request.v1",
                    "projected_status": projected_status,
                    "projection": payload
                }
            }))
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
        let mut runtime_metadata = task.runtime_metadata.clone();
        if runtime_metadata
            .as_object()
            .is_none_or(|obj| obj.is_empty())
        {
            runtime_metadata = json!({
                "schema": "missiond.runtime-task-metadata.v1",
                "source": "control-plane-kernel",
                "task_contract_id": format!("board-task:{task_id}"),
                "dispatch_metadata": {
                    "project_id": project_id,
                    "task_id": task_id,
                    "read_scope": [],
                    "write_scope": [],
                    "must_not_touch": [],
                    "control_state": "runtime_metadata"
                }
            });
        }
        let capability_grant_ids = self
            .state
            .shared_memory
            .grant_task_capabilities(
                Some(project_id.as_str()),
                input.task_id.as_str(),
                "system",
                input.producer_id.as_str(),
                &[],
                &[],
                &[],
                "control-plane-kernel",
            )
            .await?;
        runtime_metadata["capability_grant_ids"] = json!(capability_grant_ids.clone());
        runtime_metadata["sandbox_profile"] = json!("system-no-sandbox");
        if let Some(dispatch) = runtime_metadata
            .get_mut("dispatch_metadata")
            .and_then(Value::as_object_mut)
        {
            dispatch.insert(
                "capability_grant_ids".to_string(),
                json!(capability_grant_ids),
            );
            dispatch.insert("sandbox_profile".to_string(), json!("system-no-sandbox"));
        }
        let _ = self
            .state
            .store
            .update_board_task(
                &input.task_id,
                &missiond_core::types::UpdateBoardTaskInput {
                    runtime_metadata: Some(runtime_metadata),
                    ..Default::default()
                },
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
