use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_context_gather",
            "高频上下文聚合入口：一次性查询 KB、SSOT intent、project registry、skill operational evidence、infra evidence、Board task records 和 bounded conversation logs，用于 intent.lisp/plan.lisp 前的事实补齐。",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "用户诉求、待查对象或 unknowns 的压缩查询"},
                    "source_profile": {
                        "type": "string",
                        "enum": ["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                        "default": "intent_default",
                        "description": "Authority-aware source profile. intent_default excludes conversation/infra/credentials; deploy_ops enables skill+infra+credential evidence; conversation_audit enables bounded conversations; full_debug preserves broad diagnostic behavior."
                    },
                    "sourceProfile": {
                        "type": "string",
                        "enum": ["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                        "default": "intent_default",
                        "description": "source_profile alias"
                    },
                    "project_id": {"type": "string", "description": "MissionD project id"},
                    "project": {"type": "string", "description": "project_id alias"},
                    "skill": {"type": "string", "description": "优先指定 skill topic"},
                    "infra_target": {"type": "string", "description": "优先指定 runtime target id"},
                    "infraTarget": {"type": "string", "description": "infra_target alias"},
                    "unknowns": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "本轮 intent grounding 判断出的未知信息"
                    },
                    "include_kb": {"type": "boolean", "default": true},
                    "include_ssot": {"type": "boolean", "default": true},
                    "include_project": {"type": "boolean", "default": true},
                    "include_skill": {"type": "boolean", "description": "Override profile skill evidence selection"},
                    "include_infra": {"type": "boolean", "description": "Override profile infra evidence selection"},
                    "include_board": {"type": "boolean", "default": true, "description": "Include active BoardTask search results as task-record evidence"},
                    "includeBoard": {"type": "boolean", "default": true, "description": "include_board alias"},
                    "include_conversations": {"type": "boolean", "description": "Override profile bounded durable conversation search selection; this is query-scoped evidence, not prompt preloading"},
                    "includeConversations": {"type": "boolean", "description": "include_conversations alias"},
                    "include_credentials": {"type": "boolean", "default": false, "description": "Include credential ref lane; disabled unless deploy_ops/full_debug or explicit opt-in"},
                    "includeCredentials": {"type": "boolean", "default": false, "description": "include_credentials alias"},
                    "include_raw_sources": {"type": "boolean", "default": false, "description": "Persist raw legacy sources into worker context pack; default artifact uses compact evidence_lanes"},
                    "includeRawSources": {"type": "boolean", "default": false, "description": "include_raw_sources alias"},
                    "conversation_time_range": {"type": "string", "default": "last_30d", "description": "Conversation search window: last_24h, last_7d, last_30d"},
                    "conversationTimeRange": {"type": "string", "default": "last_30d", "description": "conversation_time_range alias"},
                    "conversation_type": {"type": "string", "description": "Conversation read-model filter for conversation_audit/full_debug/include_conversations, e.g. user, codex_chat, gemini_chat, subagent, worker, all"},
                    "conversationType": {"type": "string", "description": "conversation_type alias"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 25, "default": 8}
                }
            }),
        ),
        ToolDefinition::new(
            "mission_context_boot",
            "Codex Boot Context Capsule：返回小型、可版本化、可校验的启动协作协议，供 resident Codex、Codex worker 或外部新对话在启动时获取 MissionD 的基本工作方式。",
            json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "可选 MissionD project id"},
                    "project": {"type": "string", "description": "project_id alias"},
                    "task_id": {"type": "string", "description": "可选 BoardTask/work-order id"},
                    "taskId": {"type": "string", "description": "task_id alias"},
                    "include_capsule": {"type": "boolean", "default": true, "description": "false 时仅返回 capsule metadata"}
                }
            }),
        ),
    ]
}
