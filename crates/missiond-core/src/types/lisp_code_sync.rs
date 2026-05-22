use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LispCodeSyncJob {
    pub id: Uuid,
    pub project_id: String,
    pub root_path: String,
    pub changed_path: String,
    pub content_hash: String,
    pub event_kind: String,
    pub status: String,
    pub attempts: i32,
    pub next_run_at: DateTime<Utc>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub checker_ok: Option<bool>,
    pub checker_command: Option<String>,
    pub checker_tail: Option<String>,
    pub sync_task_id: Option<String>,
    pub dedupe_key: String,
    pub storm_circuit: bool,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnqueueLispCodeSyncJob {
    pub project_id: String,
    pub root_path: String,
    pub changed_path: String,
    pub content_hash: String,
    pub event_kind: String,
    pub dedupe_key: String,
    pub storm_circuit: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LispCodeSyncQueueStats {
    pub queued: i64,
    pub running: i64,
    pub due: i64,
    pub failed: i64,
    pub oldest_due_age_secs: Option<i64>,
    pub active_leases: i64,
    pub batch_last_result: Option<String>,
}
