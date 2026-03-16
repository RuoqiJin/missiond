use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Consolidated: Conversation Query =====
        ToolDefinition::new(
            "mission_conversation_query",
            "对话统一查询。通过 action 区分操作：\
             list(列出会话), get(消息内容), search(对话搜索), \
             message_search(消息级搜索), context(上下文锚定), events(系统事件)。\
             不传 action 时默认 list。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "get", "search", "message_search", "context", "events"],
                        "description": "操作类型: list(会话列表), get(消息内容), search(hybrid/fts/semantic 搜索), message_search(消息级搜索), context(上下文锚定), events(系统事件)。默认 list"
                    },

                    // ── list 参数 ──
                    "status": {
                        "type": "string",
                        "description": "[list] 按状态过滤: active, completed（不传返回全部）"
                    },
                    "conversationType": {
                        "type": "string",
                        "description": "[list] 按类型过滤: user, worker, meta, system(meta+worker), all"
                    },
                    "taskId": {
                        "type": "string",
                        "description": "[list] 按 Board 任务 ID 过滤，返回该任务关联的所有会话"
                    },

                    // ── get 参数 ──
                    "sessionId": {
                        "type": "string",
                        "description": "[get/search/events] 会话 ID。get 必传；search 限定会话内搜索；events 不传则返回全局统计"
                    },
                    "tail": {
                        "type": "integer",
                        "description": "[get] 返回最近 N 条消息（默认 50）"
                    },
                    "sinceId": {
                        "type": "integer",
                        "description": "[get] 增量拉取：只返回 ID 大于此值的消息"
                    },
                    "includeRaw": {
                        "type": "boolean",
                        "description": "[get] 是否返回完整消息（含 rawContent/model/metadata）。默认 false"
                    },

                    // ── search / message_search 共用参数 ──
                    "query": {
                        "type": "string",
                        "description": "[search/message_search] 搜索关键词（支持中英混合）"
                    },
                    "queryMode": {
                        "type": "string",
                        "enum": ["hybrid", "fts", "semantic"],
                        "description": "[search] 搜索模式：hybrid(默认,FTS+Embedding), fts(精确关键词), semantic(语义相似度)"
                    },
                    "timeRange": {
                        "type": "string",
                        "enum": ["last_24h", "last_7d", "last_30d"],
                        "description": "[search/message_search] 时间范围过滤"
                    },
                    "project": {
                        "type": "string",
                        "description": "[search] 按项目过滤"
                    },
                    "excludeSessionId": {
                        "type": "string",
                        "description": "[search] 排除特定会话（避免自引用）"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "[search] 跳过前 N 条结果（分页用）"
                    },

                    // ── message_search 参数 ──
                    "role": {
                        "type": "string",
                        "enum": ["user", "assistant", "tool_result", "system", "thinking"],
                        "description": "[message_search] 过滤消息角色"
                    },
                    "toolName": {
                        "type": "string",
                        "description": "[message_search] 过滤工具名（如 Bash, Read, mission_kb_search）"
                    },

                    // ── context 参数 ──
                    "messageId": {
                        "type": "integer",
                        "description": "[context] 锚点消息 ID（来自 message_search 结果）"
                    },
                    "before": {
                        "type": "integer",
                        "description": "[context] 锚点前取几条消息（默认 3，最大 50）"
                    },
                    "after": {
                        "type": "integer",
                        "description": "[context] 锚点后取几条消息（默认 5，最大 50）"
                    },

                    // ── events 参数 ──
                    "eventType": {
                        "type": "string",
                        "description": "[events] 按事件类型过滤（如 turn_duration, compact_boundary, hook_progress）"
                    },

                    // ── 共用参数 ──
                    "limit": {
                        "type": "integer",
                        "description": "[list/search/message_search/events] 最大返回数"
                    },
                    "since": {
                        "type": "string",
                        "description": "[list] 起始时间过滤(ISO datetime/纯日期/相对格式 1h/24h/7d)"
                    },
                    "until": {
                        "type": "string",
                        "description": "[list] 结束时间过滤(格式同 since)"
                    }
                }
            }),
        ),

        // ===== Consolidated: Conversation Analyze =====
        ToolDefinition::new(
            "mission_conversation_analyze",
            "对话分析。通过 action 区分：\
             retrospective(会话复盘,quick/detailed/full 三级深度), \
             trajectory(子 Agent 思维链), \
             activity(活动报告,日报/周报)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["retrospective", "trajectory", "activity"],
                        "description": "分析类型: retrospective(会话复盘), trajectory(子 Agent 思维链), activity(活动报告)"
                    },

                    // ── retrospective 参数 ──
                    "sessionId": {
                        "type": "string",
                        "description": "[retrospective] 要复盘的会话 ID"
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["quick", "detailed", "full"],
                        "description": "[retrospective] 分析深度: quick(默认,纯 SQL) / detailed(+ 文件热力图/服务器分布/错误恢复链) / full(detailed + Gemini 根因分析)"
                    },

                    // ── trajectory 参数 ──
                    "toolUseId": {
                        "type": "string",
                        "description": "[trajectory] 父 tool_use ID（对应 parentToolUseID）"
                    },

                    // ── activity 参数 ──
                    "since": {
                        "type": "string",
                        "description": "[activity] 起始时间(ISO datetime/纯日期/相对格式 1h/24h/7d)"
                    },
                    "until": {
                        "type": "string",
                        "description": "[activity] 结束时间(格式同 since，不传则为当前时间)"
                    },

                    // ── 共用参数 ──
                    "limit": {
                        "type": "integer",
                        "description": "[trajectory] 最大返回数（默认 200）"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Consolidated: Retrospective Manage =====
        ToolDefinition::new(
            "mission_retrospective_manage",
            "复盘管理。通过 action 区分：\
             list(已完成复盘列表), \
             backfill(批量回填,分析指定时间以来的所有会话)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "backfill"],
                        "description": "操作类型: list(列出已完成复盘), backfill(批量回填复盘)"
                    },

                    // ── list 参数 ──
                    "limit": {
                        "type": "integer",
                        "description": "[list] 最大返回数（默认 10）"
                    },

                    // ── backfill 参数 ──
                    "since": {
                        "type": "string",
                        "description": "[backfill] 起始时间(ISO datetime，如 2026-03-11T18:00:00)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Consolidated: Embedding Ops =====
        ToolDefinition::new(
            "mission_embedding_ops",
            "Embedding 操作。通过 action 区分：\
             stats(全系统覆盖率统计,含 provider 分布/缓存大小), \
             backfill(触发 KB→Skill→对话 Embedding 回填,含 stale 重刷)。\
             可多次调用 backfill 查看剩余量。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["stats", "backfill"],
                        "description": "操作类型: stats(覆盖率统计), backfill(触发回填)"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
