use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_context_gather",
        "高频上下文聚合入口：一次性查询 KB、SSOT intent、project registry、skill operational evidence 和 infra evidence，用于 intent.lisp/plan.lisp 前的事实补齐。",
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "用户诉求、待查对象或 unknowns 的压缩查询"},
                "project_id": {"type": "string", "description": "MissionD project id"},
                "project": {"type": "string", "description": "project_id alias"},
                "skill": {"type": "string", "description": "优先指定 skill topic"},
                "infra_target": {"type": "string", "description": "优先指定 runtime target id"},
                "infraTarget": {"type": "string", "description": "infra_target alias"},
                "unknowns": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "本轮 intent grounding 判断出的未知信息"
                },
                "include_kb": {"type": "boolean", "default": true},
                "include_ssot": {"type": "boolean", "default": true},
                "include_project": {"type": "boolean", "default": true},
                "include_skill": {"type": "boolean", "default": true},
                "include_infra": {"type": "boolean", "default": true},
                "limit": {"type": "integer", "minimum": 1, "maximum": 25, "default": 8}
            }
        }),
    )]
}
