use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_project",
            "项目管理。init: 注册新项目(自动发现 git/intent/backfill); list: 列出所有项目; get: 获取详情; set_active: 设置活跃状态; sync: 扫描 ~/.claude/projects/ 自动发现注册",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["list", "get", "set_active", "sync", "init"], "default": "list"},
                    "id": {"type": "string", "description": "[get/set_active/init] 项目 ID（init 时可选，默认从路径推导）"},
                    "path": {"type": "string", "description": "[init] 项目绝对路径"},
                    "slots": {"type": "array", "items": {"type": "string"}, "description": "[init] 关联工位 ID 列表"},
                    "active": {"type": "boolean", "description": "[set_active] 是否活跃", "default": true}
                }
            }),
        ),
    ]
}
