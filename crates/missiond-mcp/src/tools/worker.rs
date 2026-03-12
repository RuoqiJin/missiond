use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_workers",
            "列出所有后台 Worker 状态（running/paused）及统计信息（已处理/失败任务数、最后活跃时间）。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_worker_control",
            "运行时暂停/恢复后台 Worker（无需重启 daemon）。暂停时 Worker 在当前任务完成后挂起，恢复后自动处理积压。",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Worker 名称（如 translation_worker, briefing_worker, embedding, step_narrator）"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["pause", "resume"],
                        "description": "操作: pause(暂停), resume(恢复)"
                    }
                },
                "required": ["target", "action"]
            }),
        ),
    ]
}
