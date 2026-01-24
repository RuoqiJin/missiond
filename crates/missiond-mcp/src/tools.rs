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

/// Generate all tool definitions
pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        // ===== Task Operations =====
        ToolDefinition::new(
            "mission_submit",
            "异步提交任务给专家 Agent",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "专家角色 (如 secret, deploy)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "任务提示词"
                    }
                },
                "required": ["role", "prompt"]
            }),
        ),
        ToolDefinition::new(
            "mission_ask",
            "同步询问专家（提交 + 等待结果）",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "专家角色"
                    },
                    "question": {
                        "type": "string",
                        "description": "问题"
                    },
                    "timeoutMs": {
                        "type": "number",
                        "description": "超时毫秒数 (默认 120000)"
                    }
                },
                "required": ["role", "question"]
            }),
        ),
        ToolDefinition::new(
            "mission_status",
            "查询任务状态",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["taskId"]
            }),
        ),
        ToolDefinition::new(
            "mission_cancel",
            "取消任务",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["taskId"]
            }),
        ),
        // ===== Process Control =====
        ToolDefinition::new(
            "mission_spawn",
            "启动工位 Agent 进程",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "visible": {
                        "type": "boolean",
                        "description": "是否打开终端窗口可观看"
                    },
                    "autoRestart": {
                        "type": "boolean",
                        "description": "崩溃后自动重启"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_kill",
            "停止 Agent 进程",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_restart",
            "重启 Agent 进程",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "visible": {
                        "type": "boolean",
                        "description": "是否打开终端窗口"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_agents",
            "查看所有 Agent 状态",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        // ===== Information Query =====
        ToolDefinition::new(
            "mission_slots",
            "列出所有工位配置",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_inbox",
            "获取收件箱消息",
            json!({
                "type": "object",
                "properties": {
                    "unreadOnly": {
                        "type": "boolean",
                        "description": "仅未读 (默认 true)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "最大条数 (默认 10)"
                    }
                }
            }),
        ),
        // ===== PTY Interactive Sessions =====
        ToolDefinition::new(
            "mission_pty_spawn",
            "启动 PTY 交互式会话（像人一样操作 Claude Code）",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "autoRestart": {
                        "type": "boolean",
                        "description": "崩溃后自动重启"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_pty_send",
            "向 PTY 会话发送消息并等待回复",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "message": {
                        "type": "string",
                        "description": "发送的消息"
                    },
                    "timeoutMs": {
                        "type": "number",
                        "description": "超时毫秒数 (默认 300000)"
                    }
                },
                "required": ["slotId", "message"]
            }),
        ),
        ToolDefinition::new(
            "mission_pty_kill",
            "停止 PTY 会话",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_pty_screen",
            "获取 PTY 屏幕内容",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "lines": {
                        "type": "number",
                        "description": "获取最后 N 行 (不填返回全部)"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_pty_history",
            "获取 PTY 对话历史",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_pty_status",
            "获取 PTY 会话状态",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID (不填返回所有)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_pty_confirm",
            "发送确认响应（用于工具使用确认对话框）",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "response": {
                        "oneOf": [
                            { "type": "boolean", "description": "true=确认, false=拒绝" },
                            { "type": "number", "description": "选项编号 (1, 2, 3)" },
                            { "type": "string", "description": "直接输入的响应" }
                        ],
                        "description": "确认响应"
                    }
                },
                "required": ["slotId", "response"]
            }),
        ),
        ToolDefinition::new(
            "mission_pty_interrupt",
            "发送 Ctrl+C 中断信号",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        ToolDefinition::new(
            "mission_pty_logs",
            "获取 PTY 日志文件路径（用于 tail -f 实时查看）",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    }
                },
                "required": ["slotId"]
            }),
        ),
        // ===== Permission Management =====
        ToolDefinition::new(
            "mission_permission_get",
            "获取权限配置",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_permission_set_role",
            "设置角色权限规则",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "角色名称"
                    },
                    "rule": {
                        "type": "object",
                        "properties": {
                            "auto_allow": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "自动允许的工具模式"
                            },
                            "require_confirm": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "需要确认的工具模式"
                            },
                            "deny": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "拒绝的工具模式"
                            }
                        }
                    }
                },
                "required": ["role", "rule"]
            }),
        ),
        ToolDefinition::new(
            "mission_permission_set_slot",
            "设置工位权限规则",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "rule": {
                        "type": "object",
                        "properties": {
                            "auto_allow": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "自动允许的工具模式"
                            },
                            "require_confirm": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "需要确认的工具模式"
                            },
                            "deny": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "拒绝的工具模式"
                            }
                        }
                    }
                },
                "required": ["slotId", "rule"]
            }),
        ),
        ToolDefinition::new(
            "mission_permission_add_auto_allow",
            "添加自动允许规则",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "角色名称 (与 slotId 二选一)"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID (与 role 二选一)"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "工具模式 (如 xjp_secret_*)"
                    }
                },
                "required": ["pattern"]
            }),
        ),
        ToolDefinition::new(
            "mission_permission_reload",
            "重新加载权限配置文件",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        // ===== Claude Code Tasks Monitoring =====
        ToolDefinition::new(
            "mission_cc_sessions",
            "列出 Claude Code 会话及其 Tasks 状态",
            json!({
                "type": "object",
                "properties": {
                    "projectPath": {
                        "type": "string",
                        "description": "筛选特定项目路径"
                    },
                    "activeOnly": {
                        "type": "boolean",
                        "description": "仅显示活跃会话 (默认 true)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_cc_tasks",
            "获取指定会话的 Tasks 列表",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    },
                    "projectPath": {
                        "type": "string",
                        "description": "项目路径 (返回该项目所有会话的 Tasks)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_cc_overview",
            "获取所有 Claude Code 会话的 Tasks 概览统计",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_cc_in_progress",
            "获取所有正在进行中的任务 (跨会话)",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_cc_trigger_swarm",
            "通过 PTY 触发 Claude Code 的 Swarm 模式并行执行任务",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "PTY 工位 ID"
                    },
                    "tasks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "要执行的任务列表"
                    },
                    "teammateCount": {
                        "type": "number",
                        "description": "并行 Agent 数量 (默认 3)"
                    },
                    "timeoutMs": {
                        "type": "number",
                        "description": "超时毫秒数 (默认 600000)"
                    }
                },
                "required": ["slotId", "tasks"]
            }),
        ),
    ]
}

/// Get tool by name
pub fn get_tool(name: &str) -> Option<ToolDefinition> {
    all_tools().into_iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_tools_count() {
        let tools = all_tools();
        // Task: 4, Process: 4, Query: 2, PTY: 9, Permission: 5, CC: 5 = 29
        assert_eq!(tools.len(), 29);
    }

    #[test]
    fn test_get_tool() {
        assert!(get_tool("mission_submit").is_some());
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
