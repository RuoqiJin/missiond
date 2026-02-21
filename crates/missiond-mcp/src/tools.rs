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
            "启动 PTY 交互式会话（像人一样操作 Claude Code）。默认异步返回，不等待 Idle。可通过 mcpConfigPath 注入 MCP 工具配置。",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID"
                    },
                    "waitForIdle": {
                        "type": "boolean",
                        "description": "是否等待 Claude 就绪 (默认 false，立即返回)"
                    },
                    "timeoutSecs": {
                        "type": "number",
                        "description": "等待 Idle 超时秒数 (默认 60，仅 waitForIdle=true 时生效)"
                    },
                    "autoRestart": {
                        "type": "boolean",
                        "description": "崩溃后自动重启"
                    },
                    "mcpConfigPath": {
                        "type": "string",
                        "description": "MCP 配置文件路径 (JSON)，传给 claude --mcp-config。不填则使用 slots.yaml 中的 mcpConfig"
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

        // ===== Skill Knowledge Hub =====
        ToolDefinition::new(
            "mission_skill_list",
            "列出所有已索引的 Skill（知识库条目）。返回 name、description、aka、路径。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_skill_search",
            "按关键词搜索 Skill。支持 name/aka/description 模糊匹配。返回匹配的 Skill 元数据。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词 (如 deploy, auth, 部署)"
                    }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::new(
            "mission_context_build",
            "根据任务关键词自动匹配相关 Skill 并生成 [Context] 块。用于 Agent 派任务前自动注入上下文。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "任务关键词 (如 '部署 auth 服务')"
                    }
                },
                "required": ["query"]
            }),
        ),

        // ===== Infrastructure Registry =====
        ToolDefinition::new(
            "mission_infra_list",
            "列出基础设施服务器。可按 role (build/deploy/gpu) 或 provider (gcp/aliyun) 筛选。",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "按角色筛选 (如 build, deploy, gpu, vpn, production)"
                    },
                    "provider": {
                        "type": "string",
                        "description": "按云厂商筛选 (如 gcp, aliyun, self-hosted)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_infra_get",
            "获取单个服务器详情 (按 ID)。",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "服务器 ID (如 privatecloud, gcp, ecs)"
                    }
                },
                "required": ["id"]
            }),
        ),

        // ===== Health =====
        ToolDefinition::new(
            "mission_health",
            "检查 missiond 守护进程健康状态：IPC、WebSocket、PTY、数据库",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),

        // ===== Knowledge Base (Jarvis Memory) =====
        ToolDefinition::new(
            "mission_kb_remember",
            "Record knowledge discovered during conversation. Call PROACTIVELY when you learn: \
             server IPs, user preferences, project structures, procedures, decisions. \
             Do NOT ask permission — just record. If the key already exists, it will be updated. \
             Examples: user mentions a server → category 'infra'; user corrects your approach → 'preference'; \
             you SSH into a machine → 'discovery'; deployment succeeds → 'procedure'; \
             a key decision is made → 'memory'.",
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Knowledge category (open-ended): infra, project, preference, memory, procedure, credential, relation, ..."
                    },
                    "key": {
                        "type": "string",
                        "description": "Unique key within category (e.g. 'privatecloud', 'commit-style', 'wss-migration-2026-02')"
                    },
                    "summary": {
                        "type": "string",
                        "description": "One-line human-readable summary"
                    },
                    "detail": {
                        "type": "object",
                        "description": "Structured detail as JSON (optional). For relations: {from, to, type}"
                    },
                    "source": {
                        "type": "string",
                        "description": "How this was learned: conversation, discovery, import (default: conversation)"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "Confidence 0.0-1.0 (default: 1.0). Lower for inferred knowledge."
                    }
                },
                "required": ["category", "key", "summary"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_forget",
            "Delete a knowledge entry by key. Use when information is confirmed outdated or wrong.",
            json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The key to delete"
                    }
                },
                "required": ["key"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_search",
            "Search the knowledge base BEFORE guessing or asking the user. \
             If the user mentions a server, project, or concept — search here first. \
             This is your long-term memory across all conversations.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Full-text search query"
                    },
                    "category": {
                        "type": "string",
                        "description": "Filter by category (optional)"
                    }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_get",
            "Get a single knowledge entry by exact key.",
            json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The exact key to look up"
                    }
                },
                "required": ["key"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_list",
            "List all knowledge entries, optionally filtered by category. Use to see what's known.",
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Filter by category (e.g. infra, project, preference)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_kb_import",
            "Import knowledge from external sources. Currently supports 'servers_yaml' format \
             to migrate servers.yaml into KB entries.",
            json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "Import format: servers_yaml, json"
                    },
                    "path": {
                        "type": "string",
                        "description": "File path (optional, uses default location if omitted)"
                    }
                },
                "required": ["format"]
            }),
        ),

        ToolDefinition::new(
            "mission_kb_discover",
            "Probe a remote host via SSH to discover its specs (OS, CPU, RAM, disk, docker containers, \
             network). Call this PROACTIVELY after you SSH into a new server or when the user mentions \
             a server you haven't profiled yet. Results are automatically saved to KB as category='infra'. \
             Accepts host (user@ip or infra key), optional port and password. If no password, tries SSH key auth.",
            json!({
                "type": "object",
                "properties": {
                    "host": {
                        "type": "string",
                        "description": "SSH target: user@ip, ip, or infra registry key (e.g. 'privatecloud', 'root@106.15.2.17')"
                    },
                    "port": {
                        "type": "integer",
                        "description": "SSH port (default 22)"
                    },
                    "password": {
                        "type": "string",
                        "description": "SSH password (if not provided, uses key-based auth)"
                    }
                },
                "required": ["host"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_gc",
            "Knowledge governance: detect stale entries, duplicates, and show KB health stats. \
             Use action='stats' for overview, 'stale' to find unused entries (default 30 days), \
             'duplicates' to find potential duplicate keys within the same category. \
             Call periodically to keep the knowledge base clean and accurate.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Governance action: stats, stale, duplicates"
                    },
                    "days": {
                        "type": "integer",
                        "description": "For 'stale' action: number of days threshold (default 30)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Memory Extraction =====
        ToolDefinition::new(
            "mission_memory_pending",
            "获取待分析的对话内容。返回自上次分析后积累的用户 CLI 对话（非 PTY），\
             按 session 分组。调用后自动更新转发时间戳，避免重复分析。\
             仅供 memory slot 使用。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_memory_pending_user",
            "获取待分析的用户消息（仅 role=user）。独立于 mission_memory_pending，\
             用于从用户原话中提取偏好、纠正、决策等高价值记忆。\
             调用后自动更新 user_voice 转发时间戳。仅供 memory slot 使用。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),

        // ===== Conversation Log =====
        ToolDefinition::new(
            "mission_conversation_list",
            "List conversation sessions stored in the database. Shows session ID, project, message count, \
             start time, and analysis status. Use status filter to find active or completed sessions.",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "Filter by status: active, completed (omit for all)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of conversations to return (default 20)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_conversation_get",
            "Get messages from a conversation session. Returns the conversation metadata and messages. \
             Use tail parameter to control how many recent messages to return.",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "Conversation session ID"
                    },
                    "tail": {
                        "type": "integer",
                        "description": "Number of recent messages to return (default 50)"
                    },
                    "sinceId": {
                        "type": "integer",
                        "description": "Return messages with ID greater than this (for incremental fetch)"
                    }
                },
                "required": ["sessionId"]
            }),
        ),
        ToolDefinition::new(
            "mission_conversation_search",
            "Search conversation messages by content. Searches across all sessions. \
             Returns matching messages with session context.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (substring match)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 20)"
                    }
                },
                "required": ["query"]
            }),
        ),

        // ===== Board Tasks (Personal Task Board) =====
        ToolDefinition::new(
            "mission_board_list",
            "列出个人任务板上的所有任务。返回树形结构（含子任务）。可按状态筛选。默认隐藏 hidden 任务（续费等手动处理项）。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态筛选: open, done (不传则返回全部)"
                    },
                    "includeHidden": {
                        "type": "boolean",
                        "description": "是否包含隐藏任务（续费等手动处理项）。默认 false"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_board_create",
            "在个人任务板上创建新任务。支持子任务（通过 parentId）。",
            json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "任务标题"
                    },
                    "description": {
                        "type": "string",
                        "description": "任务描述"
                    },
                    "priority": {
                        "type": "string",
                        "description": "优先级: high, medium, low (默认 medium)"
                    },
                    "category": {
                        "type": "string",
                        "description": "分类: deploy, dev, infra, test, other (默认 other)"
                    },
                    "project": {
                        "type": "string",
                        "description": "关联项目 (如 xiaojinpro-backend)"
                    },
                    "server": {
                        "type": "string",
                        "description": "关联服务器 (如 私有云, ECS, GCP)"
                    },
                    "dueDate": {
                        "type": "string",
                        "description": "截止日期 (YYYY-MM-DD)"
                    },
                    "parentId": {
                        "type": "string",
                        "description": "父任务 ID (创建子任务时使用)"
                    },
                    "assignee": {
                        "type": "string",
                        "description": "分配的 PTY 工位 ID (如 slot-coder-1)，用于自动执行"
                    },
                    "autoExecute": {
                        "type": "boolean",
                        "description": "是否在 due_date 到达时自动执行 (需要 assignee)"
                    },
                    "promptTemplate": {
                        "type": "string",
                        "description": "自动执行时的 prompt 模板 (不填则用 title + description)"
                    },
                    "hidden": {
                        "type": "boolean",
                        "description": "隐藏任务（续费等手动处理项），不在默认列表中显示"
                    }
                },
                "required": ["title"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_update",
            "更新任务板上的任务。可修改标题、状态、优先级等任意字段。",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    },
                    "title": {
                        "type": "string",
                        "description": "新标题"
                    },
                    "description": {
                        "type": "string",
                        "description": "新描述"
                    },
                    "status": {
                        "type": "string",
                        "description": "新状态: open, done"
                    },
                    "priority": {
                        "type": "string",
                        "description": "新优先级: high, medium, low"
                    },
                    "category": {
                        "type": "string",
                        "description": "新分类: deploy, dev, infra, test, other"
                    },
                    "project": {
                        "type": "string",
                        "description": "新关联项目"
                    },
                    "server": {
                        "type": "string",
                        "description": "新关联服务器"
                    },
                    "dueDate": {
                        "type": "string",
                        "description": "新截止日期"
                    },
                    "parentId": {
                        "type": "string",
                        "description": "新父任务 ID"
                    },
                    "assignee": {
                        "type": "string",
                        "description": "分配的 PTY 工位 ID"
                    },
                    "autoExecute": {
                        "type": "boolean",
                        "description": "是否自动执行"
                    },
                    "promptTemplate": {
                        "type": "string",
                        "description": "自动执行 prompt 模板"
                    },
                    "hidden": {
                        "type": "boolean",
                        "description": "隐藏任务（续费等手动处理项）"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_get",
            "获取任务板上单个任务的详细信息",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_delete",
            "删除任务板上的任务（级联删除所有子任务）",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_toggle",
            "切换任务状态 (open ↔ done)",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_board_note_add",
            "为任务添加进度笔记。用于记录阶段进度、完成摘要或一般备注。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID"
                    },
                    "content": {
                        "type": "string",
                        "description": "笔记内容（支持 markdown）"
                    },
                    "noteType": {
                        "type": "string",
                        "description": "笔记类型: progress (进度更新), summary (完成摘要), note (一般备注)。默认 note"
                    },
                    "author": {
                        "type": "string",
                        "description": "作者标识 (如 claude-code, user)"
                    }
                },
                "required": ["taskId", "content"]
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
        assert_eq!(tools.len(), 54);
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
