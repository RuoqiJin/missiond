use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
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

    ]
}
