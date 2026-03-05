use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
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

    ]
}
