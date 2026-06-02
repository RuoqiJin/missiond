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
    /// Knowledge type: rule, fact, goal, state. Inferred from category prefix.
    /// Rules (policy/preference) → always apply. Facts (memory/architecture) → context.
    /// Goals (feature/project) → aspirational. State (ops/debug) → current status.
    #[serde(default = "default_kb_type")]
    pub kb_type: String,
    /// Postgres FTS snippet with highlighted hit context (search results only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_snippet: Option<String>,
    /// Working Memory scope: None=global KB, Some(task_id)=scratchpad for that task.
    /// Scratchpad entries only visible to their owning task, not global retrieval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_task_id: Option<String>,
    /// Darwinian utility score [0.0, 1.0]. Rises on hits, decays over time.
    /// Drives dynamic GC: low-utility entries get pruned regardless of age.
    #[serde(default = "default_utility_score")]
    pub utility_score: f64,
    /// Project scope: None = global knowledge, Some(id) = project-specific.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

fn default_utility_score() -> f64 {
    0.5
}

fn default_kb_type() -> String {
    "fact".to_string()
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
    #[serde(default)]
    pub project_id: Option<String>,
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

/// Current or historical review overlay for a knowledge entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeReviewState {
    pub id: String,
    pub knowledge_id: String,
    pub state: String,
    pub batch_id: String,
    pub reviewer: String,
    pub rationale: String,
    pub evidence_refs: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    pub confidence: f64,
    pub reviewed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
    pub is_current: bool,
}

/// Input for writing a non-destructive review overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeReviewInput {
    pub knowledge_id: String,
    pub state: String,
    pub batch_id: String,
    pub reviewer: String,
    pub rationale: String,
    #[serde(default = "default_evidence_refs")]
    pub evidence_refs: serde_json::Value,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default = "default_review_confidence")]
    pub confidence: f64,
    #[serde(default)]
    pub applied_at: Option<String>,
}

fn default_evidence_refs() -> serde_json::Value {
    serde_json::Value::Array(vec![])
}

fn default_review_confidence() -> f64 {
    0.8
}

// ============ KB Operation Queue Types ============

/// Input for saving a KB operation (from consolidation plan)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KBOperation {
    pub operation: String,
    pub target_keys: Vec<String>,
    pub rationale: Option<String>,
}

// ============ Knowledge Graph (Explicit Edges) ============

/// A directed edge between two KB entries for multi-hop reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KBEdge {
    pub source_id: String,
    pub target_id: String,
    /// derives_from, contradicts, supersedes, related_to
    pub relation_type: String,
    pub weight: f64,
    pub created_at: String,
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

// ============ Evidence Lane Read Models ============

/// Compact searchable evidence item. This is a projection over raw sources, not
/// a promotion into long-term KB truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceItemInput {
    pub id: String,
    pub lane_id: String,
    pub source_type: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    pub title: String,
    pub summary: String,
    pub authority_class: String,
    pub validity: String,
    pub privacy_class: String,
    pub freshness: String,
    #[serde(default)]
    pub score: Option<f64>,
    pub raw_policy: String,
    #[serde(default = "default_evidence_refs")]
    pub evidence_refs: serde_json::Value,
    #[serde(default = "default_evidence_metadata")]
    pub metadata: serde_json::Value,
}

/// Query contract for lane-first compact evidence retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSearchInput {
    pub query: String,
    #[serde(default)]
    pub allowed_lanes: Vec<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub include_global: bool,
    #[serde(default = "default_evidence_search_limit")]
    pub limit: i64,
}

fn default_evidence_search_limit() -> i64 {
    12
}

/// Persistent metric row for one mission_context_gather call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextGatherRunInput {
    pub id: String,
    pub query: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    pub source_profile: String,
    #[serde(default = "default_evidence_metadata")]
    pub lane_counts: serde_json::Value,
    #[serde(default = "default_evidence_metadata")]
    pub metrics: serde_json::Value,
    pub raw_sources_included: bool,
    pub credential_opt_in: bool,
    pub conversation_opt_in: bool,
    #[serde(default)]
    pub resolver_source: Option<String>,
    #[serde(default)]
    pub runtime_root_consistent: Option<bool>,
    #[serde(default)]
    pub artifact_hash: Option<String>,
    #[serde(default = "default_evidence_refs")]
    pub diagnostics: serde_json::Value,
}

fn default_evidence_metadata() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}
