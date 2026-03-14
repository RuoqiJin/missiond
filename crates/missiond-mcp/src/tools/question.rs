use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Question (merged: question_create + question_list + question_get + question_answer + question_dismiss) =====
        ToolDefinition::new(
            "mission_question",
            "Agent 待决策问题管理。action: create(登记问题), list(列出问题), get(获取详情), answer(回答问题), dismiss(忽略/关闭)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "list", "get", "answer", "dismiss"],
                        "description": "create=登记问题, list=列出, get=获取详情, answer=回答, dismiss=忽略"
                    },
                    "id": {
                        "type": "string",
                        "description": "问题 ID (action=get/answer/dismiss 时必填)"
                    },
                    "question": {
                        "type": "string",
                        "description": "问题描述 (action=create 时必填)"
                    },
                    "context": {
                        "type": "string",
                        "description": "补充上下文 (action=create 时可选)"
                    },
                    "taskId": {
                        "type": "string",
                        "description": "关联的 Board 任务 ID (action=create 时可选)"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "提问的工位 ID (action=create 时可选)"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "提问的会话 ID (action=create 时可选)"
                    },
                    "target": {
                        "type": "string",
                        "enum": ["user", "master"],
                        "description": "决策目标: user(人类) 或 master(Decision Engine)。action=create 默认 user，action=list 可用于筛选"
                    },
                    "options": {
                        "type": "string",
                        "description": "结构化选项 JSON 数组 (action=create 时可选，如 [\"方案A\", \"方案B\"])"
                    },
                    "decisionType": {
                        "type": "string",
                        "enum": ["architecture", "implementation", "debug", "investigation", "risk", "preference"],
                        "description": "决策类型 (action=create 时可选)，影响路由：architecture→Gemini, debug→决策工位, preference→KB"
                    },
                    "answer": {
                        "type": "string",
                        "description": "回答/指示 (action=answer 时必填)"
                    },
                    "status": {
                        "type": "string",
                        "description": "按状态筛选: pending, answered, dismissed (action=list 时可选)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数上限 (action=list 时可选)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== LLM Trace (merged: gemini_trace + gemini_stats + gemini_watch + jarvis_logs + jarvis_trace) =====
        ToolDefinition::new(
            "mission_llm_trace",
            "LLM 调用链路追踪。action: gemini_trace(Gemini 调用日志), gemini_stats(7天统计), gemini_watch(可用性监测), jarvis_logs(Jarvis 请求日志), jarvis_trace(Jarvis 详细 trace)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["gemini_trace", "gemini_stats", "gemini_watch", "jarvis_logs", "jarvis_trace"],
                        "description": "gemini_trace=Gemini 调用日志, gemini_stats=统计, gemini_watch=可用性监测, jarvis_logs=Jarvis 日志, jarvis_trace=Jarvis trace"
                    },
                    "caller": {
                        "type": "string",
                        "enum": ["router_chat", "flow_engine", "kb_analyze", "embedding"],
                        "description": "调用方 (action=gemini_trace 时可选)"
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Claude Code session ID (action=gemini_trace 时可选)"
                    },
                    "status": {
                        "type": "string",
                        "description": "请求状态过滤 (action=gemini_trace: ok/error/timeout, action=jarvis_logs: in_progress/success/empty_response/error/slot_unavailable)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回条数 (action=gemini_trace 默认 20, action=jarvis_logs 默认 10)"
                    },
                    "watch_action": {
                        "type": "string",
                        "enum": ["start", "stop", "status"],
                        "description": "监测操作 (action=gemini_watch 时必填): start/stop/status"
                    },
                    "trace_id": {
                        "type": "string",
                        "description": "Trace ID (action=jarvis_trace 时可选，不传返回最新)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Decision Stats (KEEP AS-IS) =====
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

        // ===== Incident (merged: incident_test + incident_list) =====
        ToolDefinition::new(
            "mission_incident",
            "AIOps Incident 管理。action: test(手动注入测试 Incident), list(列出最近记录)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["test", "list"],
                        "description": "test=手动注入测试 Incident, list=列出最近记录"
                    },
                    "severity": {
                        "type": "string",
                        "description": "严重等级: critical/high/warning (action=test 时可选，默认 warning)"
                    },
                    "title": {
                        "type": "string",
                        "description": "Incident 标题 (action=test 时可选，默认 'Test incident')"
                    },
                    "source": {
                        "type": "string",
                        "description": "来源: health_check/deploy_center/sentry/manual (action=test 时可选，默认 manual)"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "关联的 infra server ID (action=test 时可选)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "返回条数 (action=list 时可选，默认 20，最大 100)"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
