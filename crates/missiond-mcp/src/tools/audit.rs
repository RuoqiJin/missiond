use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Audit (Conversation Tool Call Analysis) =====
        ToolDefinition::new(
            "mission_audit_trace",
            "生成对话的工具调用审计轨迹（Markdown 格式）。紧凑摘要，适合发给其他 AI 审查。用 toolFilter 筛选特定工具，用 includeReasoning 包含 assistant 推理文本。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    },
                    "toolFilter": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "只看指定工具（如 [\"mission_kb_analyze\", \"mission_kb_forget\"]）"
                    },
                    "includeReasoning": {
                        "type": "boolean",
                        "description": "包含 assistant 推理文本（默认 false）"
                    }
                },
                "required": ["sessionId"]
            }),
        ),
        ToolDefinition::new(
            "mission_audit_detail",
            "获取单次工具调用的完整输入输出（下钻查看）。配合 mission_audit_trace 使用。",
            json!({
                "type": "object",
                "properties": {
                    "toolId": {
                        "type": "string",
                        "description": "tool_use_id（从 audit_trace 中获取）"
                    }
                },
                "required": ["toolId"]
            }),
        ),
        ToolDefinition::new(
            "mission_audit_stats",
            "获取会话的工具调用统计（按工具名分组，含成功/失败计数）。快速判断工作量和错误率。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    }
                },
                "required": ["sessionId"]
            }),
        ),

        ToolDefinition::new(
            "mission_audit_export",
            "一键导出 Board 任务的完整审计链：任务详情 + FlowContext + Board Notes + 所有关联会话及消息。输出 Markdown 格式，适合发给外部 AI 审查。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "Board 任务 ID"
                    },
                    "includeMessages": {
                        "type": "boolean",
                        "description": "是否包含关联会话的消息内容（默认 true）"
                    }
                },
                "required": ["taskId"]
            }),
        ),


    ]
}
