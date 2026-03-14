use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Timeline (merged: timeline_query + timeline_trace + timeline_stats + timeline_search) =====
        ToolDefinition::new(
            "mission_timeline",
            "系统时间轴。action: query(按条件查询事件), trace(查完整因果链), stats(事件统计), search(关键字搜索)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["query", "trace", "stats", "search"],
                        "description": "query=条件查询, trace=因果链, stats=统计, search=关键字搜索"
                    },
                    "eventType": {
                        "type": "string",
                        "description": "事件类型过滤 (action=query 时可选): slot_state_changed, task_lifecycle, gemini_request_completed, question_created, question_resolved, decision_made, memory_phase_changed, board_task_updated"
                    },
                    "traceId": {
                        "type": "string",
                        "description": "因果链 trace_id (action=query 可选, action=trace 时必填)"
                    },
                    "since": {
                        "type": "string",
                        "description": "起始时间，支持相对格式(1h/24h/7d)或 ISO datetime"
                    },
                    "until": {
                        "type": "string",
                        "description": "结束时间，格式同 since"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数 (action=query 默认 50 最大 200, action=search 默认 20)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "分页偏移 (action=query 时可选，默认 0)"
                    },
                    "keyword": {
                        "type": "string",
                        "description": "搜索关键字 (action=search 时必填，匹配 summary 和 payload)"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
