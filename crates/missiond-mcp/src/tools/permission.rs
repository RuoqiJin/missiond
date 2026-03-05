use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
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

    ]
}
