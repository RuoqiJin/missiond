use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::engine::shared_memory::SharedMemoryService;

#[allow(dead_code)]
pub(crate) const EVIDENCE_REQUIRED: &str = "EVIDENCE_REQUIRED";
pub(crate) const EVIDENCE_WRITE_FAILED: &str = "COMPLETION_ARTIFACT_WRITE_FAILED";
pub(crate) const EVIDENCE_WRITE_TIMEOUT: &str = "EVIDENCE_WRITE_TIMEOUT";

const DEFAULT_EVIDENCE_WRITE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub(crate) struct TaskCompletionEvidenceWriter {
    shared_memory: Arc<SharedMemoryService>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskCompletionEvidenceInput {
    pub task_id: String,
    pub project_id: Option<String>,
    pub slot_id: Option<String>,
    pub conversation_id: Option<String>,
    pub provider: String,
    pub result_status: String,
    pub summary: String,
    pub content: Option<String>,
    pub json: Value,
    pub accepted_shard_id: Option<String>,
    pub attempt_id: Option<String>,
    pub capability_grant_id: Option<String>,
    pub subject_kind: Option<String>,
    pub subject_id: Option<String>,
    pub confirm: Option<bool>,
    pub producer: Option<Value>,
    pub raw_evidence: Option<Value>,
    pub evidence_refs: Option<Value>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskCompletionEvidenceResult {
    pub artifact_hash: String,
    pub response: Value,
}

impl TaskCompletionEvidenceWriter {
    pub(crate) fn new(shared_memory: Arc<SharedMemoryService>) -> Self {
        Self {
            shared_memory,
            timeout: DEFAULT_EVIDENCE_WRITE_TIMEOUT,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub(crate) async fn write(
        &self,
        input: TaskCompletionEvidenceInput,
    ) -> Result<TaskCompletionEvidenceResult> {
        let task_id = input.task_id.clone();
        let payload = input.into_task_result_put_args();
        let request = self
            .shared_memory
            .task_result_put_request_from_args(&payload)?;
        let response = self
            .shared_memory
            .task_result_put_command(request)
            .await
            .map_err(|err| anyhow!("{EVIDENCE_WRITE_FAILED}: task_id={task_id}: {err}"))?;
        let artifact_hash = response
            .get("artifact_hash")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow!("{EVIDENCE_WRITE_FAILED}: task_id={task_id}: task_result_put returned no artifact_hash")
            })?
            .to_string();
        Ok(TaskCompletionEvidenceResult {
            artifact_hash,
            response,
        })
    }

    pub(crate) async fn write_bounded(
        &self,
        input: TaskCompletionEvidenceInput,
    ) -> Result<TaskCompletionEvidenceResult> {
        let task_id = input.task_id.clone();
        tokio::time::timeout(self.timeout, self.write(input))
            .await
            .map_err(|_| {
                anyhow!(
                    "{EVIDENCE_WRITE_TIMEOUT}: task_id={task_id}: task_result_put exceeded {:?}",
                    self.timeout
                )
            })?
    }
}

impl TaskCompletionEvidenceInput {
    fn into_task_result_put_args(self) -> Value {
        let raw_evidence = self.raw_evidence.unwrap_or_else(|| {
            json!({
                "kind": "task_completion_evidence_input",
                "payload": self.json.clone()
            })
        });
        let evidence_refs = self.evidence_refs.unwrap_or_else(|| {
            json!([{
                "kind": "task_completion_evidence_input",
                "storage": "inline_raw_evidence"
            }])
        });
        json!({
            "task_id": self.task_id,
            "project_id": self.project_id,
            "slot_id": self.slot_id,
            "conversation_id": self.conversation_id,
            "provider": self.provider,
            "result_status": self.result_status,
            "summary": self.summary,
            "content": self.content,
            "json": self.json,
            "accepted_shard_id": self.accepted_shard_id,
            "attempt_id": self.attempt_id,
            "capability_grant_id": self.capability_grant_id,
            "subject_kind": self.subject_kind,
            "subject_id": self.subject_id,
            "confirm": self.confirm,
            "producer": self.producer,
            "raw_evidence": raw_evidence,
            "evidence_refs": evidence_refs,
            "created_at": self.created_at
        })
    }
}

#[allow(dead_code)]
pub(crate) fn suggested_task_result_put(task_id: &str) -> String {
    format!(
        "mission_shared_memory(action=\"task_result_put\", task_id=\"{task_id}\", result_status=\"completed\", ...)"
    )
}
