use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Permission Query (merged: permission_get + permission_learned_list) =====
        ToolDefinition::new(
            "mission_permission_query",
            "权限查询。action: get(获取权限配置), learned_list(列出已学习的权限规则)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "learned_list"],
                        "description": "get=获取权限配置, learned_list=列出已学习的权限规则"
                    },
                    "scopeType": {
                        "type": "string",
                        "enum": ["role", "slot"],
                        "description": "过滤作用域类型 (action=learned_list 时可选)"
                    },
                    "scopeId": {
                        "type": "string",
                        "description": "过滤作用域 ID（角色名或工位 ID）(action=learned_list 时可选)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Permission Mutate (merged: set_role + set_slot + auto_allow + reload + revoke) =====
        ToolDefinition::new(
            "mission_permission_mutate",
            "权限写操作。action: set_role(设角色权限), set_slot(设工位权限), auto_allow(添加自动允许), reload(重载配置), revoke(撤销已学习权限)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["set_role", "set_slot", "auto_allow", "reload", "revoke"],
                        "description": "set_role=设角色权限, set_slot=设工位权限, auto_allow=添加自动允许, reload=重载配置, revoke=撤销已学习权限"
                    },
                    "role": {
                        "type": "string",
                        "description": "角色名称 (action=set_role/auto_allow 时可用)"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID (action=set_slot/auto_allow 时可用)"
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
                        },
                        "description": "权限规则 (action=set_role/set_slot 时必填)"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "工具模式 (action=auto_allow 时必填，如 secret_*)"
                    },
                    "scopeType": {
                        "type": "string",
                        "enum": ["role", "slot"],
                        "description": "作用域类型 (action=revoke 时必填)"
                    },
                    "scopeId": {
                        "type": "string",
                        "description": "作用域 ID (action=revoke 时必填)"
                    },
                    "toolPattern": {
                        "type": "string",
                        "description": "工具名称或模式 (action=revoke 时必填)"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
