use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_timeline_query",
            "按条件查询系统时间轴事件。支持 event_type/trace_id/时间范围筛选 + 分页。时间支持相对格式(1h/24h/7d)或 ISO datetime。",
            json!({
                "type": "object",
                "properties": {
                    "eventType": {
                        "type": "string",
                        "description": "事件类型过滤: slot_state_changed, task_lifecycle, gemini_request_completed, question_created, question_resolved, decision_made, memory_phase_changed, board_task_updated"
                    },
                    "traceId": {
                        "type": "string",
                        "description": "按 trace_id 过滤（因果链 ID）"
                    },
                    "since": {
                        "type": "string",
                        "description": "起始时间(含)。支持相对格式: 1h, 24h, 7d；或 ISO datetime"
                    },
                    "until": {
                        "type": "string",
                        "description": "结束时间(含)。格式同 since"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数(默认 50，最大 200)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "分页偏移(默认 0)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_timeline_trace",
            "查询完整因果链：同一 trace_id 的所有事件 + 关联的 gemini_requests 详细信息。用于理解\"为什么发生了 X\"。",
            json!({
                "type": "object",
                "properties": {
                    "traceId": {
                        "type": "string",
                        "description": "因果链 trace_id（通常是 task_id 或 question.task_id）"
                    }
                },
                "required": ["traceId"]
            }),
        ),
        ToolDefinition::new(
            "mission_timeline_stats",
            "时间轴统计：事件分布、trace 链数量、Gemini 延迟分布(p50/p90/p99)。",
            json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "string",
                        "description": "统计起始时间(1h/24h/7d 或 ISO datetime)"
                    },
                    "until": {
                        "type": "string",
                        "description": "统计结束时间"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_timeline_search",
            "关键字搜索时间轴（匹配 summary 和 payload）。在不知道 trace_id 时，用现象关键字找到相关事件。",
            json!({
                "type": "object",
                "properties": {
                    "keyword": {
                        "type": "string",
                        "description": "搜索关键字（匹配 summary 和 payload 内容）"
                    },
                    "since": {
                        "type": "string",
                        "description": "起始时间"
                    },
                    "until": {
                        "type": "string",
                        "description": "结束时间"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数(默认 20)"
                    }
                },
                "required": ["keyword"]
            }),
        ),
    ]
}
