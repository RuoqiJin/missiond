use serde::{Deserialize, Serialize};

/// A dynamically created slot with TTL lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicSlot {
    pub id: String,
    pub parent_slot_id: String,
    pub template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    /// JSON-serialized SlotConfig
    pub config: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminated_at: Option<String>,
    pub ttl_seconds: i64,
    pub expires_at: String,
    pub extend_count: i64,
}
