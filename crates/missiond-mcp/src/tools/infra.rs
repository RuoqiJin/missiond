use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Infrastructure Registry =====
        ToolDefinition::new(
            "mission_infra_list",
            "列出基础设施服务器。可按 role (build/deploy/gpu) 或 provider (gcp/aliyun) 筛选。",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "按角色筛选 (如 build, deploy, gpu, vpn, production)"
                    },
                    "provider": {
                        "type": "string",
                        "description": "按云厂商筛选 (如 gcp, aliyun, self-hosted)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_infra_get",
            "获取单个服务器详情 (按 ID)。",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "服务器 ID (如 privatecloud, gcp, ecs)"
                    }
                },
                "required": ["id"]
            }),
        ),

        // ===== Health & Diagnostics =====
        ToolDefinition::new(
            "mission_health",
            "检查 missiond 守护进程健康状态：IPC、WebSocket、PTY、数据库",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_reachability",
            "多通道服务器可达性探测（LAN ping/公网 ping/Tailscale/SSH/deploy agent 并行检测）。",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Infra ID (如 'privatecloud', 'gcp') 或 IP 地址"
                    },
                    "channels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "指定通道: lan_ping, public_ping, tailscale, ssh, deploy_agent（默认全部）"
                    }
                },
                "required": ["target"]
            }),
        ),
        ToolDefinition::new(
            "mission_os_diagnose",
            "SSH 登录服务器收集 OS 诊断信息（崩溃/负载/CPU/温度/日志/Docker/网络/GPU）。自动从 infra 获取 SSH 凭据。",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Infra ID (如 'privatecloud') 或 user@host"
                    },
                    "checks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "指定检查项: system, crashes, top_cpu, temperatures, journal_errors, docker, network, gpu（默认全部）"
                    }
                },
                "required": ["target"]
            }),
        ),


    ]
}
