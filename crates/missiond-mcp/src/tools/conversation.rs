use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Conversation Log =====
        ToolDefinition::new(
            "mission_conversation_list",
            "列出对话会话。显示 ID、项目、消息数、开始时间。可按状态、类型、任务 ID 过滤。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: active, completed（不传返回全部）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 20）"
                    },
                    "conversationType": {
                        "type": "string",
                        "description": "按类型过滤: user, worker, meta, system(meta+worker), all"
                    },
                    "taskId": {
                        "type": "string",
                        "description": "按 Board 任务 ID 过滤，返回该任务关联的所有会话（PTY、Gemini 咨询等）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_conversation_get",
            "获取对话会话的消息内容。用 tail 控制返回最近几条。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    },
                    "tail": {
                        "type": "integer",
                        "description": "返回最近 N 条消息（默认 50）"
                    },
                    "sinceId": {
                        "type": "integer",
                        "description": "增量拉取：只返回 ID 大于此值的消息"
                    },
                    "includeRaw": {
                        "type": "boolean",
                        "description": "是否返回完整消息（含 rawContent/model/metadata）。默认 false（精简模式，保护 LLM 上下文）"
                    }
                },
                "required": ["sessionId"]
            }),
        ),
        ToolDefinition::new(
            "mission_conversation_search",
            "混合语义搜索对话（FTS5 + Embedding RRF 双路融合）。返回会话级结果，含摘要、匹配分数和示例消息。传 sessionId 时退化为单会话消息搜索。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词（支持中英混合，语义搜索自动启用）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回会话数（默认 10）"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "限定在特定会话中搜索（退化为消息级搜索）"
                    },
                    "excludeSessionId": {
                        "type": "string",
                        "description": "排除特定会话（避免自引用）"
                    }
                },
                "required": ["query"]
            }),
        ),

        ToolDefinition::new(
            "mission_trigger_backfill",
            "触发全系统 Embedding 回填（KB 知识库 → Skill 技能 → 对话日志）。后台 Worker 按批处理，包括 provider 切换后的 stale 重刷。返回各系统 missing/stale 统计。可多次调用查看剩余量。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_embedding_stats",
            "查看全系统 Embedding 覆盖率统计（KB / Skill / 对话），含 provider 分布、缓存大小。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),

        // ===== Conversation Events & Agent Trajectory =====
        ToolDefinition::new(
            "mission_conversation_events",
            "查询会话系统事件（turn_duration/compact_boundary/hook_progress/bash_progress 等）。不传 sessionId 时返回事件类型汇总。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID。不传则返回全局事件类型统计"
                    },
                    "eventType": {
                        "type": "string",
                        "description": "按事件类型过滤（如 turn_duration, compact_boundary, hook_progress）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 100）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_agent_trajectory",
            "查询子 Agent 的完整思维链。通过 toolUseId 获取该 Agent 调用下的所有交互消息。",
            json!({
                "type": "object",
                "properties": {
                    "toolUseId": {
                        "type": "string",
                        "description": "父 tool_use ID（对应 parentToolUseID）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 200）"
                    }
                },
                "required": ["toolUseId"]
            }),
        ),


    ]
}
