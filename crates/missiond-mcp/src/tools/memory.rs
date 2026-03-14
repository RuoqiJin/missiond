use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Memory (merged: memory_pending + memory_pause + token_stats) =====
        ToolDefinition::new(
            "mission_memory",
            "记忆与 Token 管理。action: pending(获取待分析对话), pause(暂停/恢复记忆任务), token_stats(Token 消耗统计)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["pending", "pause", "token_stats"],
                        "description": "pending=待分析对话, pause=暂停/恢复记忆, token_stats=Token 消耗统计"
                    },
                    "paused": {
                        "type": "boolean",
                        "description": "true=暂停, false=恢复，省略则 toggle (action=pause 时使用)"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "按会话 ID 过滤 (action=token_stats 时可选)"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "按工位 ID 过滤 (action=token_stats 时可选)"
                    },
                    "since": {
                        "type": "string",
                        "description": "时间过滤，ISO 8601 格式 (action=token_stats 时可选)"
                    },
                    "groupBy": {
                        "type": "string",
                        "enum": ["session", "slot", "model", "day"],
                        "description": "聚合维度 (action=token_stats 时可选): session/slot/model/day"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
