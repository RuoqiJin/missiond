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
                        "description": "专家角色 (如 secret, deploy, memory)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "任务提示词"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "指定目标工位 ID（可选，精确分发到具体 slot，跳过 role 匹配）"
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
        ToolDefinition::new(
            "mission_task",
            "查询 submit task 列表。显示 ID、角色、工位、状态、结果。可按状态过滤。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: queued, running, done, failed（不传返回最近 20 条）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 20）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_task_ack",
            "获取已完成的 submit task 通知。传 since（epoch毫秒）返回增量结果，各 session 独立 watermark 互不干扰。供 UserPromptSubmit hook 调用。",
            json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "integer",
                        "description": "返回 finished_at > since 的任务（epoch 毫秒）。不传返回最近 1 小时。"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_task_track",
            "全链路追踪 submit task。一个调用返回：任务状态、工位状态、PTY 进度、最后响应。替代 mission_task + pty_status + pty_screen 组合查询。",
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
            "向 PTY 会话发送消息。默认 fire-and-forget（立即返回），用 pty_status/pty_screen 轮询结果。设 waitForResponse=true 阻塞等待回复",
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
                    "waitForResponse": {
                        "type": "boolean",
                        "description": "是否阻塞等待回复 (默认 false，fire-and-forget)"
                    },
                    "timeoutMs": {
                        "type": "number",
                        "description": "waitForResponse=true 时的超时毫秒数 (默认 300000)"
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
        ToolDefinition::new(
            "mission_pty_screenshot",
            "截取 PTY 终端屏幕截图（PNG），返回文件路径。Claude Code 可用 Read 工具查看图片来可视化调试终端状态。",
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
                        "description": "工具模式 (如 secret_*)"
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

        // ===== Skill Engine (CQRS write tools) =====
        ToolDefinition::new(
            "mission_skill_upsert",
            "创建或更新 Skill 的某个章节。写入 DB 后自动生成 SKILL.md 文件。",
            json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Skill 主题名 (如 missiond, deployment)"
                    },
                    "section_title": {
                        "type": "string",
                        "description": "章节标题 (如 '# API', '## 配置')"
                    },
                    "content": {
                        "type": "string",
                        "description": "章节的 Markdown 内容"
                    },
                    "sort_order": {
                        "type": "integer",
                        "description": "章节排序（默认追加到末尾）"
                    }
                },
                "required": ["topic", "section_title", "content"]
            }),
        ),
        ToolDefinition::new(
            "mission_skill_record",
            "快速记录一条知识碎片到指定 Skill。低认知负担，积累后可用 optimize 合并整理。",
            json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Skill 主题名"
                    },
                    "content": {
                        "type": "string",
                        "description": "碎片内容"
                    }
                },
                "required": ["topic", "content"]
            }),
        ),
        ToolDefinition::new(
            "mission_skill_render",
            "从 DB 重新生成 SKILL.md 文件。可指定 topic 单个渲染或全量重建。",
            json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "主题名（空=全部重建）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_skill_topics",
            "列出所有 Skill 主题及统计信息（命中次数、碎片数、行数）。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_context_resolve",
            "跨域上下文聚合。根据任务描述自动匹配 Skill，递归解析 requires 依赖（skills/infra/kb），一次性返回完整认知上下文。替代 mission_context_build 用于复杂任务。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "任务描述 (如 '部署 auth 服务到 GCP')"
                    },
                    "skill": {
                        "type": "string",
                        "description": "可选，直接指定 skill name 跳过搜索"
                    },
                    "include_board": {
                        "type": "boolean",
                        "description": "是否包含 Board 相关任务（默认 false）"
                    }
                },
                "required": ["query"]
            }),
        ),

        // ===== Skill Execution (Phase 3) =====
        ToolDefinition::new(
            "mission_skill_exec",
            "执行 Skill 中定义的 workflow。顺序执行 MCP 工具步骤，支持 dry_run 预览。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "Skill 名称（如 backend-deploy）"
                    },
                    "action": {
                        "type": "string",
                        "description": "Action ID（对应 workflow block id）"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "预览模式，只返回步骤不执行（默认 false）"
                    },
                    "params": {
                        "type": "object",
                        "description": "运行时参数覆盖（注入为 ${key} 变量）"
                    }
                },
                "required": ["skill", "action"]
            }),
        ),
        ToolDefinition::new(
            "mission_skill_actions",
            "列出可执行的 Skill Actions。可按 skill 名筛选。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "按 Skill 名筛选（空=列出全部）"
                    }
                }
            }),
        ),

        // ===== Skill Execution Stats (Phase 4) =====
        ToolDefinition::new(
            "mission_skill_stats",
            "查看 Skill workflow 执行统计（成功率、平均耗时）。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "Skill 名称（空=全部 skill 汇总）"
                    }
                }
            }),
        ),

        // ===== Skill Version Rollback (Phase 4) =====
        ToolDefinition::new(
            "mission_skill_rollback",
            "回滚 Skill 到历史版本。不指定 version_id 则列出可用版本。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "Skill 名称（如 backend-deploy）"
                    },
                    "version_id": {
                        "type": "integer",
                        "description": "版本 ID（不指定则列出最近版本）"
                    }
                },
                "required": ["skill"]
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

        // ===== Health & Diagnostics =====
        ToolDefinition::new(
            "mission_health",
            "检查 missiond 守护进程健康状态：IPC、WebSocket、PTY、数据库",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_reachability",
            "多通道服务器可达性探测（LAN ping/公网 ping/Tailscale/SSH/deploy agent 并行检测）。",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Infra ID (如 'privatecloud', 'gcp') 或 IP 地址"
                    },
                    "channels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "指定通道: lan_ping, public_ping, tailscale, ssh, deploy_agent（默认全部）"
                    }
                },
                "required": ["target"]
            }),
        ),
        ToolDefinition::new(
            "mission_os_diagnose",
            "SSH 登录服务器收集 OS 诊断信息（崩溃/负载/CPU/温度/日志/Docker/网络/GPU）。自动从 infra 获取 SSH 凭据。",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Infra ID (如 'privatecloud') 或 user@host"
                    },
                    "checks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "指定检查项: system, crashes, top_cpu, temperatures, journal_errors, docker, network, gpu（默认全部）"
                    }
                },
                "required": ["target"]
            }),
        ),

        // ===== Knowledge Base (Jarvis Memory) =====
        ToolDefinition::new(
            "mission_kb_remember",
            "记录知识到长期记忆。主动调用，无需请求许可。key 已存在则更新。\
             分类: preference(用户偏好), memory:architecture(架构决策), memory:bugfix(已修bug), \
             memory:debug(调试经验), memory:ops(运维), memory:feature(功能), project(项目), memory(通用)",
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "enum": ["preference", "memory", "memory:architecture", "memory:bugfix", "memory:debug", "memory:ops", "memory:feature", "memory:decision", "memory:platform", "project", "architecture", "decision", "feature", "infra", "procedure"],
                        "description": "分类"
                    },
                    "key": {
                        "type": "string",
                        "description": "唯一标识 (如 'utf8-slice-panic-fix', 'user-prefers-chinese')"
                    },
                    "summary": {
                        "type": "string",
                        "description": "一行摘要"
                    },
                    "detail": {
                        "type": "object",
                        "description": "结构化详情 JSON（可选）"
                    },
                    "source": {
                        "type": "string",
                        "description": "来源: conversation, discovery, import（默认 conversation）"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "置信度 0.0-1.0（默认 1.0）"
                    }
                },
                "required": ["category", "key", "summary"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_forget",
            "删除知识条目。信息过时或错误时使用。",
            json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "要删除的 key"
                    }
                },
                "required": ["key"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_batch_forget",
            "批量删除知识条目。一次删除多个 key，比逐条 forget 高效。",
            json!({
                "type": "object",
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "要删除的 key 列表"
                    }
                },
                "required": ["keys"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_search",
            "搜索知识库。传 query 全文搜索，不传则列出最近条目。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词（不传则返回最近条目）",
                        "default": ""
                    },
                    "category": {
                        "type": "string",
                        "description": "按分类过滤"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_kb_get",
            "按 key 精确查询单个知识条目。",
            json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "精确 key"
                    }
                },
                "required": ["key"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_list",
            "列出所有知识条目。支持复合分类：查 'memory' 同时返回 'memory:architecture' 等子分类。",
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "按分类过滤 (如 preference, memory, memory:architecture)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_kb_import",
            "从外部源导入知识。支持 servers_yaml 格式。",
            json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "导入格式: servers_yaml, json"
                    },
                    "path": {
                        "type": "string",
                        "description": "文件路径（可选，不填用默认位置）"
                    }
                },
                "required": ["format"]
            }),
        ),

        ToolDefinition::new(
            "mission_kb_discover",
            "SSH 探测远程主机硬件配置（OS/CPU/RAM/磁盘/Docker/网络），结果自动存入 KB。",
            json!({
                "type": "object",
                "properties": {
                    "host": {
                        "type": "string",
                        "description": "SSH 目标: user@ip 或 infra key (如 'privatecloud')"
                    },
                    "port": {
                        "type": "integer",
                        "description": "SSH 端口（默认 22）"
                    },
                    "password": {
                        "type": "string",
                        "description": "SSH 密码（不填用密钥认证）"
                    }
                },
                "required": ["host"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_gc",
            "知识库治理: 检测过期/重复条目。stats=概览, stale=找未使用, duplicates=找重复, \
             clean_stale=自动清理过期, clean_duplicates=自动去重。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作: stats, stale, duplicates, clean_stale, clean_duplicates"
                    },
                    "days": {
                        "type": "integer",
                        "description": "stale 的天数阈值（默认 30）"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== KB Analysis (via external AI) =====
        ToolDefinition::new(
            "mission_kb_analyze",
            "用 Gemini 分析 KB 质量。overview=宏观评估+去重/重分类建议, \
             consolidation_plan=生成可执行的合并/删除 JSON, custom=自定义分析。\
             支持分页和分类过滤。include_board_context=true 注入 Board 任务上下文。",
            json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "description": "分析模式: overview(默认), consolidation_plan, custom"
                    },
                    "target_category": {
                        "type": "string",
                        "description": "按分类过滤（支持前缀匹配: 'memory' 包含 'memory:bugfix' 等）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "每次最大条目数（默认 500）"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "分页偏移（默认 0）"
                    },
                    "include_board_context": {
                        "type": "boolean",
                        "description": "注入 Board 任务上下文（默认 false）"
                    },
                    "custom_prompt": {
                        "type": "string",
                        "description": "自定义分析 prompt（仅 mode=custom）"
                    },
                    "model": {
                        "type": "string",
                        "description": "使用的模型（默认 gemini-3.1-pro）"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "最大响应 token 数（默认 16384）"
                    },
                    "save_plan": {
                        "type": "boolean",
                        "description": "consolidation_plan 模式时自动保存到操作队列（默认 true）"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "关联 Board 任务 ID（保存 plan 时关联）"
                    }
                }
            }),
        ),

        // ===== KB Operation Queue =====
        ToolDefinition::new(
            "mission_kb_queue_status",
            "查看 KB 操作队列状态。kb_analyze consolidation_plan 的执行进度。",
            json!({
                "type": "object",
                "properties": {
                    "plan_id": {
                        "type": "string",
                        "description": "按批次 ID 过滤"
                    },
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: pending, running, done, skipped, failed"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_kb_execute_plan",
            "执行 KB 操作队列中的 pending 操作。delete/update/category_fix/recategorize 自动执行，merge/distill 派发工位。自动清理 24h 前的 stale ops。",
            json!({
                "type": "object",
                "properties": {
                    "plan_id": {
                        "type": "string",
                        "description": "执行特定批次的操作（必须指定，防止跨 plan 混合执行）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "每次最多执行 N 个操作（默认 5）"
                    }
                },
                "required": ["plan_id"]
            }),
        ),

        // ===== Router Chat =====
        ToolDefinition::new(
            "mission_router_chat",
            "通过 AI 路由器与 Gemini 等模型多轮对话。传 task_id 自动持久化对话历史（同 Board 任务下连续对话）。不传 task_id 则无状态。",
            json!({
                "type": "object",
                "properties": {
                    "messages": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "role": { "type": "string", "enum": ["user", "assistant", "system"] },
                                "content": { "type": "string" }
                            },
                            "required": ["role", "content"]
                        },
                        "description": "本轮新消息。传 task_id 时历史自动加载，只需传新消息"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "关联 Board 任务 ID。传此参数自动加载/保存对话历史，实现跨会话连续对话"
                    },
                    "context": {
                        "type": "string",
                        "enum": ["board", "kb", "both", "none"],
                        "description": "自动注入上下文: board(任务板), kb(知识库), both, none（默认 none）"
                    },
                    "model": {
                        "type": "string",
                        "description": "模型（默认 gemini-3.1-pro）"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "最大响应 token 数（默认 16384）"
                    },
                    "search": {
                        "type": "boolean",
                        "description": "启用 Google 搜索增强（仅 Gemini，默认 false）"
                    }
                },
                "required": ["messages"]
            }),
        ),
        ToolDefinition::new(
            "mission_router_chat_history",
            "查看与某 Board 任务关联的 Gemini 对话历史",
            json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Board 任务 ID"
                    }
                },
                "required": ["task_id"]
            }),
        ),

        // ===== Memory Extraction =====
        ToolDefinition::new(
            "mission_memory_pending",
            "获取待分析的对话内容（消息级追踪）。\
             返回所有 pending 状态的用户 CLI 对话消息（非 PTY），按 session 分组。\
             每条消息带 [#id] 前缀，用户消息用 ★ 标记。\
             系统会在 daemon 侧自动提交处理状态；mission_memory_done 仅用于兼容旧流程（通常无需手动调用）。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_memory_pause",
            "暂停/恢复记忆任务。暂停后 realtime extraction 和 deep analysis 不再调度。\
             不传 paused 参数则 toggle 当前状态。",
            json!({
                "type": "object",
                "properties": {
                    "paused": {
                        "type": "boolean",
                        "description": "true=暂停, false=恢复。省略则 toggle。"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_memory_done",
            "兼容工具：手动确认一批消息已被 realtime 管道处理完成。\
             当前系统默认由 daemon 自动管理状态，通常不需要调用本工具。",
            json!({
                "type": "object",
                "properties": {
                    "message_ids": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "已处理完成的消息 ID 列表 (来自 mission_memory_pending 的 batch_msg_ids)"
                    }
                },
                "required": ["message_ids"]
            }),
        ),

        // ===== Token Stats =====
        ToolDefinition::new(
            "mission_token_stats",
            "查询 token 消耗统计。支持按会话、工位、模型、日期聚合。\
             无参数返回全局总量。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "按会话 ID 过滤"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "按工位 ID 过滤"
                    },
                    "since": {
                        "type": "string",
                        "description": "时间过滤，ISO 8601 格式 (e.g. 2026-02-27T00:00:00Z)"
                    },
                    "groupBy": {
                        "type": "string",
                        "enum": ["session", "slot", "model", "day"],
                        "description": "聚合维度: session=按会话, slot=按工位, model=按模型, day=按天"
                    }
                }
            }),
        ),

        // ===== Conversation Log =====
        ToolDefinition::new(
            "mission_conversation_list",
            "列出对话会话。显示 ID、项目、消息数、开始时间。可按状态过滤。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: active, completed（不传返回全部）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 20）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_conversation_get",
            "获取对话会话的消息内容。用 tail 控制返回最近几条。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    },
                    "tail": {
                        "type": "integer",
                        "description": "返回最近 N 条消息（默认 50）"
                    },
                    "sinceId": {
                        "type": "integer",
                        "description": "增量拉取：只返回 ID 大于此值的消息"
                    },
                    "includeRaw": {
                        "type": "boolean",
                        "description": "是否返回完整消息（含 rawContent/model/metadata）。默认 false（精简模式，保护 LLM 上下文）"
                    }
                },
                "required": ["sessionId"]
            }),
        ),
        ToolDefinition::new(
            "mission_conversation_search",
            "按内容搜索对话消息（跨会话），返回匹配消息及上下文。支持按角色/slot/会话过滤。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 20）"
                    },
                    "role": {
                        "type": "string",
                        "description": "按消息角色过滤: user, assistant, tool_use, tool_result, thinking, system"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "限定在特定会话中搜索"
                    },
                    "excludeSessionId": {
                        "type": "string",
                        "description": "排除特定会话（避免自引用）"
                    }
                },
                "required": ["query"]
            }),
        ),

        // ===== Conversation Events & Agent Trajectory =====
        ToolDefinition::new(
            "mission_conversation_events",
            "查询会话系统事件（turn_duration/compact_boundary/hook_progress/bash_progress 等）。不传 sessionId 时返回事件类型汇总。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID。不传则返回全局事件类型统计"
                    },
                    "eventType": {
                        "type": "string",
                        "description": "按事件类型过滤（如 turn_duration, compact_boundary, hook_progress）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 100）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_agent_trajectory",
            "查询子 Agent 的完整思维链。通过 toolUseId 获取该 Agent 调用下的所有交互消息。",
            json!({
                "type": "object",
                "properties": {
                    "toolUseId": {
                        "type": "string",
                        "description": "父 tool_use ID（对应 parentToolUseID）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 200）"
                    }
                },
                "required": ["toolUseId"]
            }),
        ),

        // ===== Audit (Conversation Tool Call Analysis) =====
        ToolDefinition::new(
            "mission_audit_trace",
            "生成对话的工具调用审计轨迹（Markdown 格式）。紧凑摘要，适合发给其他 AI 审查。用 toolFilter 筛选特定工具，用 includeReasoning 包含 assistant 推理文本。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    },
                    "toolFilter": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "只看指定工具（如 [\"mission_kb_analyze\", \"mission_kb_forget\"]）"
                    },
                    "includeReasoning": {
                        "type": "boolean",
                        "description": "包含 assistant 推理文本（默认 false）"
                    }
                },
                "required": ["sessionId"]
            }),
        ),
        ToolDefinition::new(
            "mission_audit_detail",
            "获取单次工具调用的完整输入输出（下钻查看）。配合 mission_audit_trace 使用。",
            json!({
                "type": "object",
                "properties": {
                    "toolId": {
                        "type": "string",
                        "description": "tool_use_id（从 audit_trace 中获取）"
                    }
                },
                "required": ["toolId"]
            }),
        ),
        ToolDefinition::new(
            "mission_audit_stats",
            "获取会话的工具调用统计（按工具名分组，含成功/失败计数）。快速判断工作量和错误率。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    }
                },
                "required": ["sessionId"]
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
                        "description": "关联项目 (如 my-project)"
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
            "mission_board_claim",
            "认领任务。原子操作：仅当任务状态为 open 且未被其他执行者认领时成功。防止多个 Claude Code 实例重复执行同一任务。认领成功后任务状态自动变为 running。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "要认领的任务 ID"
                    },
                    "executorId": {
                        "type": "string",
                        "description": "执行者标识 (如 slot ID 或会话标识)。不填则用 MCP session ID"
                    },
                    "executorType": {
                        "type": "string",
                        "description": "执行者类型: pty_slot (自动化工位) 或 manual_session (人工交互)。默认 manual_session"
                    }
                },
                "required": ["taskId"]
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
        ToolDefinition::new(
            "mission_board_summary",
            "任务板执行摘要。返回各状态任务计数、待处理问题数、新 KB 条目数。用于用户回来后快速了解全貌。",
            json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "string",
                        "description": "起始时间(ISO 8601)，统计该时间之后完成/失败的任务和新增 KB。不传则统计全部"
                    }
                }
            }),
        ),
        // ===== Slot Task History =====
        ToolDefinition::new(
            "mission_slot_history",
            "查询工位任务历史。显示 daemon 给工位分派过的所有任务（realtime_extract、deep_analysis 等），含状态、耗时、产出统计。",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID (如 slot-memory)。不传则查所有工位"
                    },
                    "taskType": {
                        "type": "string",
                        "description": "任务类型: realtime_extract, deep_analysis, kb_gc"
                    },
                    "status": {
                        "type": "string",
                        "description": "状态: pending, running, completed, failed"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数，默认 20"
                    },
                    "stats": {
                        "type": "boolean",
                        "description": "为 true 时只返回汇总统计，不返回明细"
                    }
                }
            }),
        ),
        // ===== Agent Questions (Pending Decisions) =====
        ToolDefinition::new(
            "mission_question_create",
            "Agent 遇到阻断问题时登记待决策项。持久化到 DB，显示在用户 Board UI。\
             Agent 可稍后用 mission_question_list 检查是否已回答。",
            json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "问题描述（简洁明确）"
                    },
                    "context": {
                        "type": "string",
                        "description": "补充上下文（已尝试方案、可选项、证据等）"
                    },
                    "taskId": {
                        "type": "string",
                        "description": "关联的 Board 任务 ID（可选）"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "提问的工位 ID（可选）"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "提问的会话 ID（可选）"
                    }
                },
                "required": ["question"]
            }),
        ),
        ToolDefinition::new(
            "mission_question_list",
            "列出待决策问题。默认返回全部，可按 status 筛选（pending/answered/dismissed）。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态筛选: pending, answered, dismissed"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_question_get",
            "获取单个待决策问题详情（含回答）",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "问题 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_question_answer",
            "回答 Agent 提出的待决策问题",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "问题 ID"
                    },
                    "answer": {
                        "type": "string",
                        "description": "回答/指示"
                    }
                },
                "required": ["id", "answer"]
            }),
        ),
        ToolDefinition::new(
            "mission_question_dismiss",
            "忽略/关闭一个待决策问题",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "问题 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        // ── Jarvis Trace ──
        ToolDefinition::new(
            "mission_jarvis_logs",
            "查看最近 Jarvis chat completion 请求日志（环形缓冲区，最多 100 条）。用于调试 Router→MissionD→PTY 全链路。",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "number",
                        "description": "返回条数（默认 10，最大 100）"
                    },
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: in_progress / success / empty_response / error / slot_unavailable"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_jarvis_trace",
            "查看单条 Jarvis 请求的详细 trace（含 Router trace_id 关联、状态变迁、耗时）",
            json!({
                "type": "object",
                "properties": {
                    "trace_id": {
                        "type": "string",
                        "description": "Trace ID（不传则返回最新一条）"
                    }
                }
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
            "mission_submit",
            "mission_ask",
            "mission_pty_spawn",
            "mission_kb_remember",
            "mission_cc_overview",
        ] {
            assert!(names.contains(required), "missing required tool: {required}");
        }
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
