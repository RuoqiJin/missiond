use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Infra Query (merged: infra_list + infra_get) =====
        ToolDefinition::new(
            "mission_infra_query",
            "基础设施查询。action: list(列出服务器，可按 role/provider 筛选), get(获取单个服务器详情)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "get"],
                        "description": "list=列出服务器, get=获取单个详情"
                    },
                    "id": {
                        "type": "string",
                        "description": "服务器 ID (action=get 时必填，如 privatecloud, gcp, ecs)"
                    },
                    "role": {
                        "type": "string",
                        "description": "按角色筛选 (action=list 时可选，如 build, deploy, gpu, vpn, production)"
                    },
                    "provider": {
                        "type": "string",
                        "description": "按云厂商筛选 (action=list 时可选，如 gcp, aliyun, self-hosted)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Infra Ops (merged: health + reachability + os_diagnose) =====
        ToolDefinition::new(
            "mission_infra_ops",
            "基础设施运维。action: health(检查 missiond 健康状态), reachability(多通道可达性探测), diagnose(SSH 登录收集 OS 诊断)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["health", "reachability", "diagnose"],
                        "description": "health=missiond 健康检查, reachability=可达性探测, diagnose=OS 诊断"
                    },
                    "target": {
                        "type": "string",
                        "description": "Infra ID 或 IP/user@host (action=reachability/diagnose 时必填)"
                    },
                    "channels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "指定探测通道 (action=reachability 时可选): lan_ping, public_ping, tailscale, ssh, deploy_agent"
                    },
                    "checks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "指定检查项 (action=diagnose 时可选): system, crashes, top_cpu, temperatures, journal_errors, docker, network, gpu"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
