use std::time::Duration;

use serde_json::{json, Value};

use crate::engine::shared_memory::TaskResultPutRequest;

#[allow(dead_code)]
pub(crate) const EVIDENCE_REQUIRED: &str = "EVIDENCE_REQUIRED";
pub(crate) const EVIDENCE_WRITE_FAILED: &str = "COMPLETION_ARTIFACT_WRITE_FAILED";
pub(crate) const EVIDENCE_WRITE_TIMEOUT: &str = "EVIDENCE_WRITE_TIMEOUT";

pub(crate) const DEFAULT_EVIDENCE_WRITE_TIMEOUT: Duration = Duration::from_secs(20);

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

impl TaskCompletionEvidenceInput {
    pub(crate) fn into_task_result_put_request(self) -> TaskResultPutRequest {
        let slot_id = self.slot_id.clone();
        let conversation_id = self.conversation_id.clone();
        let project_id = self.project_id.unwrap_or_else(|| "missiond".to_string());
        let details = self.json.clone();
        let content = self
            .content
            .clone()
            .map(Value::String)
            .filter(|value| !value.is_null())
            .unwrap_or_else(|| details.clone());
        let raw_evidence = self.raw_evidence.unwrap_or_else(|| {
            json!({
                "kind": "task_completion_evidence_input",
                "payload": details.clone()
            })
        });
        let evidence_refs = self.evidence_refs.unwrap_or_else(|| {
            json!([{
                "kind": "task_completion_evidence_input",
                "storage": "inline_raw_evidence"
            }])
        });
        let evidence_refs_vec = match evidence_refs {
            Value::Array(values) => values,
            Value::Null => Vec::new(),
            value => vec![value],
        };
        let subject_kind = self.subject_kind.unwrap_or_else(|| {
            if slot_id.is_some() {
                "worker".to_string()
            } else if conversation_id.is_some() {
                "conversation".to_string()
            } else {
                "task".to_string()
            }
        });
        let subject_id = self
            .subject_id
            .or_else(|| slot_id.clone())
            .or_else(|| conversation_id.clone())
            .unwrap_or_else(|| self.task_id.clone());
        let producer = self.producer.unwrap_or_else(|| {
            json!({
                "kind": "worker-completion-producer",
                "provider": self.provider,
                "slot_id": slot_id,
                "conversation_id": conversation_id
            })
        });
        let created_at = self
            .created_at
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        let allow_system_bypass = matches!(subject_kind.as_str(), "system" | "daemon")
            || (subject_kind == "operator" && self.confirm.unwrap_or(false));

        let raw_args = json!({
            "task_id": self.task_id,
            "project_id": project_id,
            "slot_id": slot_id,
            "conversation_id": conversation_id,
            "provider": self.provider,
            "result_status": self.result_status,
            "summary": self.summary,
            "content": content,
            "json": details,
            "accepted_shard_id": self.accepted_shard_id,
            "attempt_id": self.attempt_id,
            "capability_grant_id": self.capability_grant_id,
            "subject_kind": subject_kind,
            "subject_id": subject_id,
            "confirm": self.confirm,
            "producer": producer,
            "raw_evidence": raw_evidence,
            "evidence_refs": evidence_refs_vec,
            "created_at": created_at
        });

        TaskResultPutRequest {
            task_id: raw_args
                .get("task_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            project_id: raw_args
                .get("project_id")
                .and_then(Value::as_str)
                .unwrap_or("missiond")
                .to_string(),
            slot_id: raw_args
                .get("slot_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            conversation_id: raw_args
                .get("conversation_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            provider: raw_args
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            result_status: raw_args
                .get("result_status")
                .and_then(Value::as_str)
                .unwrap_or("completed")
                .to_string(),
            summary: raw_args
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("Worker result artifact")
                .to_string(),
            content: raw_args.get("content").cloned().unwrap_or(Value::Null),
            details: raw_args.get("json").cloned().unwrap_or_else(|| json!({})),
            accepted_shard_id: raw_args
                .get("accepted_shard_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            attempt_id: raw_args
                .get("attempt_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            grant_id: raw_args
                .get("capability_grant_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            subject_kind: raw_args
                .get("subject_kind")
                .and_then(Value::as_str)
                .unwrap_or("task")
                .to_string(),
            subject_id: raw_args
                .get("subject_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            producer: raw_args
                .get("producer")
                .cloned()
                .unwrap_or_else(|| json!({})),
            raw_evidence: raw_args.get("raw_evidence").cloned(),
            evidence_refs: raw_args
                .get("evidence_refs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            has_explicit_evidence: true,
            created_at: raw_args
                .get("created_at")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            allow_system_bypass,
            raw_args,
        }
    }
}

#[allow(dead_code)]
pub(crate) fn suggested_task_result_put(task_id: &str) -> String {
    format!(
        "mission_shared_memory(action=\"task_result_put\", task_id=\"{task_id}\", result_status=\"completed\", ...)"
    )
}
