use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCommitOutboxInput {
    pub operation_key: String,
    pub surface: String,
    pub request_id: Option<String>,
    pub project_id: Option<String>,
    pub artifact_kind: String,
    pub artifact_path: String,
    pub artifact_sha256: Option<String>,
    pub db_table: Option<String>,
    pub db_row_id: Option<String>,
    pub event_id: Option<String>,
    pub event_seq: Option<i64>,
    pub payload: Value,
}

#[cfg_attr(feature = "postgres", derive(sqlx::FromRow))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCommitOutboxRecord {
    pub id: Uuid,
    pub operation_key: String,
    pub surface: String,
    pub request_id: Option<String>,
    pub project_id: Option<String>,
    pub artifact_kind: String,
    pub artifact_path: String,
    pub artifact_sha256: Option<String>,
    pub db_table: Option<String>,
    pub db_row_id: Option<String>,
    pub event_id: Option<String>,
    pub event_seq: Option<i64>,
    pub status: String,
    pub attempt_count: i32,
    pub last_error: Option<String>,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}
