use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Memory Extraction =====
        ToolDefinition::new(
            "mission_memory_pending",
            "获取待分析的对话内容（消息级追踪）。\
             返回所有 pending 状态的用户 CLI 对话消息（非 PTY），按 session 分组。\
             每条消息带 [#id] 前缀，用户消息用 ★ 标记。\
             系统会在 daemon 侧自动提交处理状态；mission_memory_done 仅用于兼容旧流程（通常无需手动调用）。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_memory_pause",
            "暂停/恢复记忆任务。暂停后 realtime extraction 和 deep analysis 不再调度。\
             不传 paused 参数则 toggle 当前状态。",
            json!({
                "type": "object",
                "properties": {
                    "paused": {
                        "type": "boolean",
                        "description": "true=暂停, false=恢复。省略则 toggle。"
                    }
                }
            }),
        ),
        // ===== Token Stats =====
        ToolDefinition::new(
            "mission_token_stats",
            "查询 token 消耗统计。支持按会话、工位、模型、日期聚合。\
             无参数返回全局总量。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "按会话 ID 过滤"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "按工位 ID 过滤"
                    },
                    "since": {
                        "type": "string",
                        "description": "时间过滤，ISO 8601 格式 (e.g. 2026-02-27T00:00:00Z)"
                    },
                    "groupBy": {
                        "type": "string",
                        "enum": ["session", "slot", "model", "day"],
                        "description": "聚合维度: session=按会话, slot=按工位, model=按模型, day=按天"
                    }
                }
            }),
        ),


    ]
}
