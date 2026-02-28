//! Types for Claude CLI stream-json output
//!
//! Mirrors the TypeScript definitions in packages/runner/src/types.ts

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Runner configuration options
pub struct RunOptions {
    /// The prompt to send
    pub prompt: String,
    /// Working directory
    pub cwd: Option<PathBuf>,
    /// Resume session ID
    pub session_id: Option<String>,
    /// MCP config file path
    pub mcp_config: Option<PathBuf>,
    /// Timeout duration (default: 5 minutes)
    pub timeout: Duration,
    /// Progress callback
    pub on_progress: Option<ProgressCallback>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            cwd: None,
            session_id: None,
            mcp_config: None,
            timeout: Duration::from_secs(5 * 60),
            on_progress: None,
        }
    }
}

/// Progress callback type
pub type ProgressCallback = Box<dyn Fn(StreamEvent) + Send + Sync>;

/// Execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    /// Session ID (can be used for --resume)
    pub session_id: String,
    /// Final result text
    pub result: String,
    /// Whether the result is an error
    pub is_error: bool,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// API call duration in milliseconds
    pub duration_api_ms: u64,
    /// Number of turns
    pub num_turns: u32,
    /// Total cost in USD
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    /// Token usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Stream event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamEvent {
    System(SystemEvent),
    Assistant(AssistantEvent),
    User(UserEvent),
    Result(ResultEvent),
}

/// System event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub subtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Assistant event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantEvent {
    pub message: AssistantMessage,
}

/// Assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
}

/// User event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEvent {
    pub message: UserMessage,
}

/// User message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: String,
}

/// Result event from stream-json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultEvent {
    pub subtype: String,
    pub session_id: String,
    pub result: String,
    pub is_error: bool,
    pub duration_ms: u64,
    pub duration_api_ms: u64,
    pub num_turns: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<RawUsage>,
}

/// Raw usage from Claude CLI (snake_case)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Content block in assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Runner errors
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("Task cancelled")]
    Cancelled,

    #[error("Task timed out after {0:?}")]
    Timeout(Duration),

    #[error("Claude CLI failed: {0}")]
    CliFailed(String),

    #[error("No result from Claude CLI: {0}")]
    NoResult(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}
