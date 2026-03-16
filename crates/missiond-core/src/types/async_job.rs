use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Async job status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AsyncJobStatus {
    Running,
    Completed,
    Failed,
}

/// An async job — long-running operation that returns immediately with a job ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncJob {
    pub id: String,
    pub tool_name: String,
    pub status: AsyncJobStatus,
    /// Result payload (set on completion).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error message (set on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

impl AsyncJob {
    pub fn new(id: String, tool_name: String) -> Self {
        AsyncJob {
            id,
            tool_name,
            status: AsyncJobStatus::Running,
            result: None,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        }
    }

    pub fn complete(&mut self, result: Value) {
        self.status = AsyncJobStatus::Completed;
        self.result = Some(result);
        self.completed_at = Some(chrono::Utc::now().to_rfc3339());
    }

    pub fn fail(&mut self, error: String) {
        self.status = AsyncJobStatus::Failed;
        self.error = Some(error);
        self.completed_at = Some(chrono::Utc::now().to_rfc3339());
    }
}
