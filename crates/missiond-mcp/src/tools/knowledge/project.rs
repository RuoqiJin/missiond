use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_project",
            "项目管理。list: 列出所有项目; get: 获取详情; set_active: 设置活跃状态; sync: 扫描 ~/.claude/projects/ 自动发现注册",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["list", "get", "set_active", "sync"], "default": "list"},
                    "id": {"type": "string", "description": "[get/set_active] 项目 ID"},
                    "active": {"type": "boolean", "description": "[set_active] 是否活跃", "default": true}
                }
            }),
        ),
    ]
}
