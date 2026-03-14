use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Agent (merged: spawn + kill + restart + agents) =====
        ToolDefinition::new(
            "mission_agent",
            "Agent 进程管理。action: spawn(启动), kill(停止), restart(重启), list(查看所有状态)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["spawn", "kill", "restart", "list"],
                        "description": "spawn=启动, kill=停止, restart=重启, list=查看所有状态"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID (action=spawn/kill/restart 时必填)"
                    },
                    "visible": {
                        "type": "boolean",
                        "description": "是否打开终端窗口可观看 (action=spawn/restart 时可选)"
                    },
                    "autoRestart": {
                        "type": "boolean",
                        "description": "崩溃后自动重启 (action=spawn 时可选)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Information Query (KEEP AS-IS) =====
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
    ]
}
