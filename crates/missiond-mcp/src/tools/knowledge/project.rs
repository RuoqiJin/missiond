use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_project",
        "项目管理。resolve: 从项目名/别名/域名/URL/cwd 解析项目或未注册候选; init: 注册新项目; list: 列出所有; get: 详情; context: 全景聚合; memories: Claude记忆; universe: V3 service-runtime-universe部署拓扑; deployment_channels: 查询 build/runtime/frontend 部署通道; reconcile_deployment_channels: 检查 GitHub Actions/native runner/Deploy Center 通道漂移; reconcile: MissionD/deploy-center/Forge 三方项目身份一致性检查; vault_sync: 参考项目lisp缓存; import_universe: 解析universe.intent.lisp批量注册; survey: 触发forge测绘生成intent.lisp",
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "get", "resolve", "set_active", "sync", "init", "context", "memories", "universe", "deployment_channels", "reconcile_deployment_channels", "reconcile", "vault_sync", "import_universe", "survey"], "default": "list"},
                "id": {"type": "string", "description": "[get/set_active/init/context/memories/universe/deployment_channels/reconcile_deployment_channels/vault_sync/survey/resolve] 项目、服务 ID、Deploy Center slug、域名或 URL"},
                "project": {"type": "string", "description": "[get/resolve/context/deployment_channels/reconcile_deployment_channels] id alias for callers that use project-scoped tools"},
                "project_id": {"type": "string", "description": "[get/resolve/context/deployment_channels/reconcile_deployment_channels] id alias in snake_case"},
                "projectId": {"type": "string", "description": "[get/resolve/context/deployment_channels/reconcile_deployment_channels] id alias in camelCase"},
                "service": {"type": "string", "description": "[deployment_channels/reconcile_deployment_channels] service id, Deploy Center slug, container, domain, or URL filter"},
                "service_id": {"type": "string", "description": "[deployment_channels/reconcile_deployment_channels] service alias in snake_case"},
                "serviceId": {"type": "string", "description": "[deployment_channels/reconcile_deployment_channels] service alias in camelCase"},
                "query": {"type": "string", "description": "[resolve] 项目名、别名、域名、URL、服务名或用户原始描述"},
                "cwd": {"type": "string", "description": "[resolve] 当前工作目录，用于最长项目根匹配"},
                "path": {"type": "string", "description": "[init/resolve] 项目绝对路径或候选路径"},
                "slots": {"type": "array", "items": {"type": "string"}, "description": "[init] 关联工位 ID 列表"},
                "active": {"type": "boolean", "description": "[set_active] 是否活跃", "default": true},
                "file": {"type": "string", "description": "[memories] 指定记忆文件名读取全文"},
                "manifest": {"type": "string", "description": "[import_universe] manifest 路径 (默认 ~/Projects/universe.intent.lisp)"},
                "deployCenterRoot": {"type": "string", "description": "[reconcile] deploy-center canonical root override"},
                "forgeRoot": {"type": "string", "description": "[reconcile] Forge canonical root override"},
                "level": {"type": "string", "enum": ["L1", "L2", "L3"], "description": "[survey] 测绘粒度 (默认 L3)", "default": "L3"},
                "check": {"type": "boolean", "description": "[survey] 仅检查是否过期，不调用 LLM"},
                "dry_run": {"type": "boolean", "description": "[survey] 仅打印 prompt，不调用 LLM"},
                "include_observed": {"type": "boolean", "description": "[deployment_channels/reconcile_deployment_channels] 是否尝试读取 Deploy Center live observation，默认 true"},
                "include_unregistered_candidates": {"type": "boolean", "description": "[resolve] 未命中注册项目时返回候选根和注册方案", "default": true},
                "limit": {"type": "integer", "description": "[resolve] 返回候选上限", "default": 8}
            }
        }),
    )]
}
