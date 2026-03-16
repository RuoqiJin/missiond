use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Audit (merged: audit_trace + audit_detail + audit_stats + audit_export) =====
        ToolDefinition::new(
            "mission_audit",
            "对话工具调用审计。action: trace(生成审计轨迹), detail(单次调用详情), stats(工具调用统计), export(导出 Board 任务完整审计链)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["trace", "detail", "stats", "export"],
                        "description": "trace=审计轨迹, detail=调用详情, stats=调用统计, export=导出完整审计链"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID (action=trace/stats 时必填)"
                    },
                    "toolId": {
                        "type": "string",
                        "description": "tool_use_id (action=detail 时必填)"
                    },
                    "taskId": {
                        "type": "string",
                        "description": "Board 任务 ID (action=export 时必填)"
                    },
                    "toolFilter": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "只看指定工具 (action=trace 时可选)"
                    },
                    "includeReasoning": {
                        "type": "boolean",
                        "description": "包含 assistant 推理文本 (action=trace 时可选，默认 false)"
                    },
                    "includeMessages": {
                        "type": "boolean",
                        "description": "包含关联会话消息内容 (action=export 时可选，默认 true)"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
