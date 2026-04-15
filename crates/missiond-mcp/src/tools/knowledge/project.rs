use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_project",
            "项目管理。init: 注册新项目; list: 列出所有; get: 详情; context: 全景聚合; memories: Claude记忆; vault_sync: 参考项目lisp缓存; import_universe: 解析universe.intent.lisp批量注册; survey: 触发forge测绘生成intent.lisp",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["list", "get", "set_active", "sync", "init", "context", "memories", "vault_sync", "import_universe", "survey"], "default": "list"},
                    "id": {"type": "string", "description": "[get/set_active/init/context/memories/vault_sync/survey] 项目 ID"},
                    "path": {"type": "string", "description": "[init] 项目绝对路径"},
                    "slots": {"type": "array", "items": {"type": "string"}, "description": "[init] 关联工位 ID 列表"},
                    "active": {"type": "boolean", "description": "[set_active] 是否活跃", "default": true},
                    "file": {"type": "string", "description": "[memories] 指定记忆文件名读取全文"},
                    "manifest": {"type": "string", "description": "[import_universe] manifest 路径 (默认 ~/Projects/universe.intent.lisp)"},
                    "level": {"type": "string", "enum": ["L1", "L2", "L3"], "description": "[survey] 测绘粒度 (默认 L3)", "default": "L3"},
                    "check": {"type": "boolean", "description": "[survey] 仅检查是否过期，不调用 LLM"},
                    "dry_run": {"type": "boolean", "description": "[survey] 仅打印 prompt，不调用 LLM"}
                }
            }),
        ),
    ]
}
