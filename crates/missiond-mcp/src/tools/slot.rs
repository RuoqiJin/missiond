use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Slot Task History =====
        ToolDefinition::new(
            "mission_slot_history",
            "查询工位任务历史。显示 daemon 给工位分派过的所有任务（realtime_extract、deep_analysis 等），含状态、耗时、产出统计。",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "工位 ID (如 slot-memory)。不传则查所有工位"
                    },
                    "taskType": {
                        "type": "string",
                        "description": "任务类型: realtime_extract, deep_analysis, kb_gc"
                    },
                    "status": {
                        "type": "string",
                        "description": "状态: pending, running, completed, failed"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数，默认 20"
                    },
                    "stats": {
                        "type": "boolean",
                        "description": "为 true 时只返回汇总统计，不返回明细"
                    }
                }
            }),
        ),

    ]
}
