use serde::{Deserialize, Serialize};

// ============ Conversation Log ============

/// A conversation session (maps to a Claude Code JSONL file)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_id: Option<String>,
    pub source: String, // "claude_cli" | "pty"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonl_path: Option<String>,
    /// Parent conversation ID (for subagent sessions spawned by Task tool)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// Task ID: groups sessions that belong to the same logical task
    /// (survives Claude Code context compaction which creates new session IDs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub message_count: i64,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub status: String, // "active" | "completed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyzed_at: Option<String>,
    /// Deep analysis schema version (0 = never analyzed)
    #[serde(default)]
    pub analysis_version: i32,
    /// Retry count for deep analysis failures (capped at MAX_ANALYSIS_RETRIES)
    #[serde(default)]
    pub analysis_retries: i32,
    /// Checkpoint watermark: last message ID processed by incremental deep analysis
    #[serde(default)]
    pub deep_analyzed_message_id: i64,
    /// Conversation type: "pty" (default) or "router_chat" (Gemini sessions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_type: Option<String>,
    /// Classification: "user" | "meta" | "worker" | "subagent"
    #[serde(default = "default_conversation_type")]
    pub conversation_type: String,
    /// Last message write time (for compaction detection)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// LLM-generated conversation summary for semantic search
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_summary: Option<String>,
    /// Embedding provider identifier (e.g. "fastembed-bge-small-zh-v1.5")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_provider: Option<String>,
    /// JSON array of compaction fragment summaries (session timeline reconstruction)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_timeline: Option<String>,
    /// Timestamp when timeline was built (CAS guard against duplicate builds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_built_at: Option<String>,
}

fn default_conversation_type() -> String { "user".to_string() }

/// A message within a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String, // "user" | "assistant" | "tool_use" | "tool_result"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    /// Comma-separated tool names extracted from raw_content (for tool_use/tool_result messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    // ── Storage Layer (P0: Three-Layer Refactor) ──
    /// Original JSONL role before mapping (e.g., "user" before → "system")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_role: Option<String>,
    /// JSON array of content block types: ["text","tool_use","image"]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_types: Option<String>,
    /// Whether this message contains image data
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_image: bool,
    /// Whether this message contains tool_use blocks
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_tool_use: bool,
    /// Whether this message contains tool_result blocks
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_tool_result: bool,
    /// Token count from usage info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i64>,
    // ── Query-time computed fields (not persisted) ──
    /// Sequential message number within session (ROW_NUMBER)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    /// Human-readable role display name (Rust match mapping)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_display: Option<String>,
}

fn is_false(v: &bool) -> bool { !v }

/// Map DB role + flags to human-readable display name.
pub fn role_display(role: &str, has_tool_use: bool) -> &'static str {
    match (role, has_tool_use) {
        ("user", _) => "用户",
        ("assistant", true) => "工具调用",
        ("assistant", false) => "AI助理",
        ("tool_result", _) => "工具调用结果",
        ("thinking", _) => "思考",
        ("compact_summary", _) => "压缩摘要",
        ("system", _) => "系统",
        ("agent_user", _) => "代理用户",
        ("agent_assistant", _) => "代理助理",
        _ => "未知",
    }
}

/// A non-dialog system event from JSONL (turn_duration, compact_boundary, hook_progress, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEvent {
    pub id: i64,
    pub session_id: String,
    pub event_uuid: Option<String>,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_data: Option<String>,
    pub timestamp: String,
}

// ============ Conversation Tool Calls (Audit) ============

/// A structured tool call record extracted from JSONL tool_use/tool_result pairs.
/// Used for audit trail (Summary-to-Drilldown architecture).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub id: String,              // tool_use_id from Claude API
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<i64>, // FK to conversation_messages
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<String>,
    pub status: String,          // pending, success, error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub timestamp: String,
}

// ============ Slot Task History ============

/// A task dispatched to a slot by the daemon (for tracking history)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotTask {
    pub id: String,
    pub slot_id: String,
    /// Task type: "realtime_extract", "deep_analysis", "kb_gc", etc.
    pub task_type: String,
    /// Status: "pending", "running", "completed", "failed"
    pub status: String,
    /// First ~200 chars of the prompt sent to the slot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_summary: Option<String>,
    /// JSON array of source session IDs that triggered this task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sessions: Option<String>,
    /// Number of KB entries produced by this task
    #[serde(default)]
    pub output_count: i64,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The conversation ID created on the slot for this task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}
