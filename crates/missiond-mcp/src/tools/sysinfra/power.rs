use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Power Control (Epic 3: 算力经济学) =====
        ToolDefinition::new(
            "mission_power_control",
            "物理服务器电源管控。唤醒 (WoL/gcloud start)、休眠、查询状态。用于 slot-ops 按需启停 GPU 等算力资源。",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "目标服务器 ID (如 win-3090ti, ecs, gcp-prod)"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["wake", "suspend", "status"],
                        "description": "操作: wake (唤醒), suspend (休眠), status (查状态)"
                    }
                },
                "required": ["target", "action"]
            }),
        ),

    ]
}
