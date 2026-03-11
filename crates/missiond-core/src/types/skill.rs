use serde::{Deserialize, Serialize};

// ============ Skill Engine Types ============

/// Skill topic metadata (maps to skill_topics table)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillTopic {
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aka: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    pub file_path: String,
    pub hit_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_hit_at: Option<String>,
    pub fragment_count: i64,
    pub total_lines: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// Phase 2: JSON-serialized SkillRequires (dependency declarations)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_json: Option<String>,
    /// Phase 3: JSON-serialized Vec<SkillAction> (executable actions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions_json: Option<String>,
    /// Phase 4: JSON-serialized Vec<ContextHook> (pre-flight probes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_hooks_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Skill block (section or fragment)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBlock {
    pub id: String,
    pub topic: String,
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub content: String,
    pub sort_order: i32,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Skill execution statistics (aggregated per action)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExecutionStat {
    pub action_id: String,
    pub total: i64,
    pub successes: i64,
    pub failures: i64,
    pub avg_duration_ms: Option<f64>,
    pub last_run: String,
}

/// Skill version snapshot (for rollback)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersion {
    pub id: i64,
    pub topic: String,
    pub content: String,
    pub checksum: String,
    pub created_at: String,
}

/// Skill FTS search result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSearchResult {
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A credential stored alongside a knowledge entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: String,
    pub knowledge_id: String,
    pub name: String,
    pub value_encrypted: String,
    pub created_at: String,
    pub updated_at: String,
}
