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
                    },
                    "since": {
                        "type": "string",
                        "description": "起始时间过滤(ISO datetime/纯日期/相对格式 1h/24h/7d)"
                    },
                    "until": {
                        "type": "string",
                        "description": "结束时间过滤(格式同 since)"
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
            "搜索对话。默认 hybrid 模式(FTS5+Embedding RRF)。返回会话级结果+匹配片段(FTS snippet)。找特定报错/代码名用 fts 模式；找解决思路用 semantic 模式。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词（支持中英混合）"
                    },
                    "queryMode": {
                        "type": "string",
                        "enum": ["hybrid", "fts", "semantic"],
                        "description": "搜索模式：hybrid(默认,FTS+Embedding), fts(精确关键词), semantic(语义相似度)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回会话数（默认 10）"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "跳过前 N 条结果（分页用）"
                    },
                    "timeRange": {
                        "type": "string",
                        "enum": ["last_24h", "last_7d", "last_30d"],
                        "description": "时间范围过滤"
                    },
                    "project": {
                        "type": "string",
                        "description": "按项目过滤"
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

        // ===== Retrospective Analysis =====
        ToolDefinition::new(
            "mission_retrospective",
            "会话复盘分析。quick 模式纯 Rust SQL 聚合（零 LLM），返回工具频次/时间黑洞/重复模式/N-Gram 弯路/高错误率。full 模式额外调 Gemini 做根因分析。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "要复盘的会话 ID"
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["quick", "detailed", "full"],
                        "description": "分析深度: quick(默认,纯 SQL) / detailed(+ 文件热力图/服务器分布/错误恢复链) / full(detailed + Gemini 根因分析)"
                    }
                },
                "required": ["sessionId"]
            }),
        ),

        ToolDefinition::new(
            "mission_retrospective_backfill",
            "批量回填复盘：分析指定时间以来的所有会话（无阈值过滤）。后台逐个执行 L1(detailed)+L2(MiniMax)，高严重度自动建 Board 任务。跳过已有复盘结果的会话。",
            json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "string",
                        "description": "起始时间(ISO datetime，如 2026-03-11T18:00:00)"
                    }
                },
                "required": ["since"]
            }),
        ),

        ToolDefinition::new(
            "mission_retrospective_list",
            "列出已完成的复盘结果。显示会话 ID、触发原因、分析时间。用于查看后台 RetroWorker 的自动复盘产出。",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 10）"
                    }
                }
            }),
        ),

        // ===== Activity Report =====
        ToolDefinition::new(
            "mission_activity_report",
            "活动报告：一次调用返回时间范围内的对话统计、Board 任务流转、Timeline 事件分布。支持日报/周报。",
            json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "string",
                        "description": "起始时间(ISO datetime/纯日期/相对格式 1h/24h/7d)"
                    },
                    "until": {
                        "type": "string",
                        "description": "结束时间(格式同 since，不传则为当前时间)"
                    }
                },
                "required": ["since"]
            }),
        ),

    ]
}
