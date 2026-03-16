use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Worker (merged: workers + worker_control) =====
        ToolDefinition::new(
            "mission_worker",
            "后台 Worker 管理。action: list(列出状态及统计), control(暂停/恢复 Worker)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "control"],
                        "description": "list=列出所有 Worker 状态, control=暂停/恢复"
                    },
                    "target": {
                        "type": "string",
                        "description": "Worker 名称 (action=control 时必填，如 translation_worker, briefing_worker, embedding, step_narrator)"
                    },
                    "control_action": {
                        "type": "string",
                        "enum": ["pause", "resume"],
                        "description": "操作 (action=control 时必填): pause/resume"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
