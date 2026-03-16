//! MCP Tool definitions
//!
//! This module defines all available MCP tools and their schemas.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Tool definition following MCP schema
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    /// Tool name (e.g., "mission_submit")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: Value,
}

impl ToolDefinition {
    /// Create a new tool definition
    pub fn new(name: impl Into<String>, description: impl Into<String>, input_schema: Value) -> Self {
        ToolDefinition {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// Permission rule for role/slot
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_allow: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_confirm: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny: Option<Vec<String>>,
}

/// Tool result content type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum ToolContent {
    Text { text: String },
}

/// Tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl ToolResult {
    /// Create a successful text result
    pub fn text(text: impl Into<String>) -> Self {
        ToolResult {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: None,
        }
    }

    /// Create a successful JSON result
    pub fn json<T: Serialize>(value: &T) -> Self {
        let text = serde_json::to_string(value).unwrap_or_else(|e| {
            json!({ "error": e.to_string() }).to_string()
        });
        ToolResult::text(text)
    }

    /// Create a pretty-printed JSON result
    pub fn json_pretty<T: Serialize>(value: &T) -> Self {
        let text = serde_json::to_string_pretty(value).unwrap_or_else(|e| {
            json!({ "error": e.to_string() }).to_string()
        });
        ToolResult::text(text)
    }

    /// Create an error result
    pub fn error(message: impl Into<String>) -> Self {
        ToolResult {
            content: vec![ToolContent::Text {
                text: json!({ "error": message.into() }).to_string(),
            }],
            is_error: Some(true),
        }
    }
}


mod task;
mod process;
mod pty;
mod permission;
mod cc_tasks;
mod skill;
mod infra;
mod kb;
mod router_chat;
mod memory;
mod conversation;
mod audit;
mod board;
mod slot;
mod question;
mod power;
mod timeline;
mod minimax;
mod worker;
mod system;

/// Generate all tool definitions
pub fn all_tools() -> Vec<ToolDefinition> {
    let mut tools = Vec::new();
    tools.extend(task::definitions());
    tools.extend(process::definitions());
    tools.extend(pty::definitions());
    tools.extend(permission::definitions());
    tools.extend(cc_tasks::definitions());
    tools.extend(skill::definitions());
    tools.extend(infra::definitions());
    tools.extend(kb::definitions());
    tools.extend(router_chat::definitions());
    tools.extend(memory::definitions());
    tools.extend(conversation::definitions());
    tools.extend(audit::definitions());
    tools.extend(board::definitions());
    tools.extend(slot::definitions());
    tools.extend(question::definitions());
    tools.extend(power::definitions());
    tools.extend(timeline::definitions());
    tools.extend(minimax::definitions());
    tools.extend(worker::definitions());
    tools.extend(system::definitions());
    tools
}

/// Get tool by name
pub fn get_tool(name: &str) -> Option<ToolDefinition> {
    all_tools().into_iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_all_tools_count() {
        let tools = all_tools();
        assert!(!tools.is_empty());

        let mut names = HashSet::new();
        for tool in &tools {
            assert!(
                names.insert(tool.name.clone()),
                "duplicate tool name found: {}",
                tool.name
            );
        }

        for required in [
            "mission_task_submit",
            "mission_task_query",
            "mission_pty_spawn",
            "mission_kb_remember",
            "mission_cc_query",
        ] {
            assert!(names.contains(required), "missing required tool: {required}");
        }
    }

    #[test]
    fn test_get_tool() {
        assert!(get_tool("mission_task_submit").is_some());
        assert!(get_tool("mission_pty_send").is_some());
        assert!(get_tool("unknown_tool").is_none());
    }

    #[test]
    fn test_tool_result_json() {
        let result = ToolResult::json(&serde_json::json!({"key": "value"}));
        match &result.content[0] {
            ToolContent::Text { text } => {
                assert!(text.contains("key"));
            }
        }
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("Something went wrong");
        assert_eq!(result.is_error, Some(true));
    }
}
