use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Agent Questions (Pending Decisions) =====
        ToolDefinition::new(
            "mission_question_create",
            "Agent 遇到阻断问题时登记待决策项。持久化到 DB，显示在用户 Board UI。\
             Agent 可稍后用 mission_question_list 检查是否已回答。\
             设置 target=master 可触发 Decision Engine 自动决策。",
            json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "问题描述（简洁明确）"
                    },
                    "context": {
                        "type": "string",
                        "description": "补充上下文（已尝试方案、可选项、证据等）"
                    },
                    "taskId": {
                        "type": "string",
                        "description": "关联的 Board 任务 ID（可选）"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "提问的工位 ID（可选）"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "提问的会话 ID（可选）"
                    },
                    "target": {
                        "type": "string",
                        "enum": ["user", "master"],
                        "description": "决策目标：user(人类) 或 master(Decision Engine 自动决策)",
                        "default": "user"
                    },
                    "options": {
                        "type": "string",
                        "description": "结构化选项（JSON 数组，如 [\"方案A\", \"方案B\"]）"
                    },
                    "decisionType": {
                        "type": "string",
                        "enum": ["architecture", "implementation", "debug", "investigation", "risk", "preference"],
                        "description": "决策类型，影响路由：architecture→Gemini, debug→决策工位, preference→KB",
                        "default": "implementation"
                    }
                },
                "required": ["question"]
            }),
        ),
        ToolDefinition::new(
            "mission_question_list",
            "列出待决策问题。可按 status/target/limit 筛选。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态筛选: pending, answered, dismissed"
                    },
                    "target": {
                        "type": "string",
                        "description": "按决策目标筛选: user, master"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数上限"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_decision_stats",
            "Decision Engine 统计数据：各 Tier 命中率、待处理数、降级数",
            json!({
                "type": "object",
                "properties": {
                    "hours": {
                        "type": "integer",
                        "description": "统计时间窗口（小时），默认 24",
                        "default": 24
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_question_get",
            "获取单个待决策问题详情（含回答）",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "问题 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        ToolDefinition::new(
            "mission_question_answer",
            "回答 Agent 提出的待决策问题",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "问题 ID"
                    },
                    "answer": {
                        "type": "string",
                        "description": "回答/指示"
                    }
                },
                "required": ["id", "answer"]
            }),
        ),
        ToolDefinition::new(
            "mission_question_dismiss",
            "忽略/关闭一个待决策问题",
            json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "问题 ID"
                    }
                },
                "required": ["id"]
            }),
        ),
        // ── Jarvis Trace ──
        ToolDefinition::new(
            "mission_jarvis_logs",
            "查看最近 Jarvis chat completion 请求日志（环形缓冲区，最多 100 条）。用于调试 Router→MissionD→PTY 全链路。",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "number",
                        "description": "返回条数（默认 10，最大 100）"
                    },
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: in_progress / success / empty_response / error / slot_unavailable"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_jarvis_trace",
            "查看单条 Jarvis 请求的详细 trace（含 Router trace_id 关联、状态变迁、耗时）",
            json!({
                "type": "object",
                "properties": {
                    "trace_id": {
                        "type": "string",
                        "description": "Trace ID（不传则返回最新一条）"
                    }
                }
            }),
        ),
        // ── Gemini Request Log ──
        ToolDefinition::new(
            "mission_gemini_trace",
            "查询 Gemini API 调用日志。支持按 caller/session_id/status 筛选。排查 LLM 调用链路问题。",
            json!({
                "type": "object",
                "properties": {
                    "caller": {
                        "type": "string",
                        "description": "调用方: router_chat, flow_engine, kb_analyze, embedding",
                        "enum": ["router_chat", "flow_engine", "kb_analyze", "embedding"]
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Claude Code session ID"
                    },
                    "status": {
                        "type": "string",
                        "description": "请求状态: ok, error, timeout",
                        "enum": ["ok", "error", "timeout"]
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数(默认 20)",
                        "default": 20
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_gemini_stats",
            "Gemini API 调用统计（7天汇总：按 caller 分布、平均耗时、慢请求 Top 10）",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        // ── AIOps Incidents ──
        ToolDefinition::new(
            "mission_incident_test",
            "手动注入一条测试 Incident，触发 Reactor 处理流程（防抖→入库→创建 Board 任务）。用于验证 AIOps 管道。",
            json!({
                "type": "object",
                "properties": {
                    "severity": {
                        "type": "string",
                        "description": "严重等级: critical / high / warning（默认 warning）"
                    },
                    "title": {
                        "type": "string",
                        "description": "Incident 标题（默认 'Test incident'）"
                    },
                    "source": {
                        "type": "string",
                        "description": "来源: health_check / deploy_center / sentry / manual（默认 manual）"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "关联的 infra server ID（可选）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_incident_list",
            "列出最近的 AIOps Incident 记录（含关联的 Board 任务 ID）。",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "number",
                        "description": "返回条数（默认 20，最大 100）"
                    }
                }
            }),
        ),

    ]
}
