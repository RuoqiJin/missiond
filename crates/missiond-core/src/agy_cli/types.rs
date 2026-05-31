//! Antigravity (`agy`) CLI native transcript types.
//!
//! Mirrors the per-step JSONL written by Antigravity to
//! `~/.gemini/antigravity-cli/brain/<conversationId>/.system_generated/logs/transcript_full.jsonl`.
//!
//! Each line is one planner/tool/user step. A step carries EITHER `content`
//! (text) OR `tool_calls` (a planner tool invocation), plus an optional
//! `thinking` field. `type` names the producer (USER_INPUT, PLANNER_RESPONSE,
//! LIST_DIRECTORY, VIEW_FILE, GREP_SEARCH, RUN_COMMAND, CONVERSATION_HISTORY, …).

use serde::{Deserialize, Serialize};

/// One step line in `transcript_full.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgyStep {
    #[serde(default)]
    pub step_index: i64,
    /// Producer source: "USER_EXPLICIT" | "MODEL" | "SYSTEM".
    #[serde(default)]
    pub source: String,
    /// Step type, e.g. "USER_INPUT" | "PLANNER_RESPONSE" | "LIST_DIRECTORY".
    #[serde(rename = "type", default)]
    pub step_type: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    /// Text payload for user/assistant/tool-result steps. Usually a string.
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    /// Planner reasoning attached to a step.
    #[serde(default)]
    pub thinking: Option<serde_json::Value>,
    /// Planner tool invocations (the model deciding to call a tool). `id` is
    /// absent in Antigravity's format, so callers synthesize a stable id.
    #[serde(default)]
    pub tool_calls: Option<Vec<AgyToolCall>>,
}

/// A planner tool invocation. Antigravity records only `name` + `args`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgyToolCall {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

/// A parsed Antigravity session (one conversationId / brain dir).
#[derive(Debug, Clone)]
pub struct AgySession {
    pub session_id: String,
    pub steps: Vec<AgyStep>,
}
