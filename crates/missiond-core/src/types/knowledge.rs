use serde::{Deserialize, Serialize};

// ============ Knowledge Base (Jarvis Memory) ============

/// A knowledge entry in the KB
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEntry {
    pub id: String,
    pub category: String,
    pub key: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    pub source: String,
    pub confidence: f64,
    pub access_count: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_task_id: Option<String>,
}

/// Input for remembering (upserting) knowledge
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KBRememberInput {
    pub category: String,
    pub key: String,
    pub summary: String,
    #[serde(default)]
    pub detail: Option<serde_json::Value>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// Result of a kb_remember operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KBRememberResult {
    pub entry: KnowledgeEntry,
    /// "created", "updated", "merged"
    pub action: String,
    /// If merged, the key of the entry that was merged into
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_key: Option<String>,
    /// Similarity score if merged
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
}

// ============ KB Operation Queue Types ============

/// Input for saving a KB operation (from consolidation plan)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KBOperation {
    pub operation: String,
    pub target_keys: Vec<String>,
    pub rationale: Option<String>,
}

/// Row from kb_operation_queue table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KBOperationRow {
    pub id: String,
    pub plan_id: String,
    pub task_id: Option<String>,
    pub operation: String,
    pub target_keys: String,
    pub rationale: Option<String>,
    pub status: String,
    pub priority: i32,
    pub result: Option<String>,
    pub created_at: String,
    pub executed_at: Option<String>,
    pub error: Option<String>,
}
