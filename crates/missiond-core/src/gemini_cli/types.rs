//! Gemini CLI native session types
//!
//! Mirrors the JSON structure written by Gemini CLI to ~/.gemini/tmp/*/chats/*.json

use serde::{Deserialize, Serialize};

// ============ Session-level ============

/// Root structure of a Gemini CLI session file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSession {
    pub session_id: String,
    pub project_hash: String,
    pub start_time: String,
    pub last_updated: String,
    pub messages: Vec<GeminiMessage>,
    #[serde(default)]
    pub kind: Option<String>,
}

// ============ Message-level ============

/// A single message in a Gemini CLI session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiMessage {
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub msg_type: String, // "user" | "gemini" | "info"
    /// String for gemini responses, array of content items for user messages.
    pub content: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<Vec<GeminiThought>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<GeminiTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<GeminiToolCall>>,
    /// UI-only field: subset of content for display (user messages with file refs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_content: Option<serde_json::Value>,
}

/// Gemini thinking step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiThought {
    pub subject: String,
    pub description: String,
    pub timestamp: String,
}

/// Token usage from Gemini API response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeminiTokens {
    #[serde(default)]
    pub input: i64,
    #[serde(default)]
    pub output: i64,
    #[serde(default)]
    pub cached: i64,
    #[serde(default)]
    pub thoughts: i64,
    #[serde(default)]
    pub tool: i64,
    #[serde(default)]
    pub total: i64,
}

// ============ Tool calls ============

/// A tool invocation within a gemini message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(default)]
    pub result: Vec<GeminiToolResult>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub result_display: Option<serde_json::Value>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub render_output_as_markdown: Option<bool>,
}

/// Wrapper for tool call results.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolResult {
    #[serde(default)]
    pub function_response: Option<GeminiFunctionResponse>,
}

/// Inner function response from a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub response: serde_json::Value,
}

// ============ Project mapping ============

/// ~/.gemini/projects.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiProjectsFile {
    /// path → project name
    pub projects: std::collections::HashMap<String, String>,
}
