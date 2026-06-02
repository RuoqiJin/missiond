use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_context_gather",
            "高频上下文聚合入口：profile-first 查询 runtime_truth、project_ssot、reviewed_kb、active_board、support_refs，以及可选 skill_evidence / conversation_audit / cold_archive，并返回 compact evidence_lanes、evidence_items、support_catalog 和 context_noise_metrics。",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "用户诉求、待查对象或 unknowns 的压缩查询"},
                    "source_profile": {
                        "type": "string",
                        "enum": ["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                        "default": "intent_default",
                        "description": "Evidence lane profile. intent_default allows runtime_truth/project_ssot/reviewed_kb/active_board/support_refs; deploy_ops adds scoped skill_evidence and redacted credential refs; conversation_audit adds bounded conversation evidence; full_debug preserves raw/cold forensics."
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
                    "include_raw_sources": {"type": "boolean", "default": false, "description": "Include raw legacy sources in response/context pack; default artifact uses compact evidence_lanes/evidence_items/support_catalog only"},
                    "includeRawSources": {"type": "boolean", "default": false, "description": "include_raw_sources alias"},
                    "persist": {"type": "boolean", "default": false, "description": "Persist a worker context artifact/capsule. This also writes compact context_gather_runs/evidence_items read-model projections."},
                    "persist_read_model": {"type": "boolean", "default": true, "description": "Persist compact context_gather_runs/evidence_items read-model projections without creating a worker context artifact. persist=true forces this on."},
                    "persistReadModel": {"type": "boolean", "default": true, "description": "persist_read_model alias"},
                    "conversation_time_range": {"type": "string", "default": "last_30d", "description": "Conversation search window: last_24h, last_7d, last_30d"},
                    "conversationTimeRange": {"type": "string", "default": "last_30d", "description": "conversation_time_range alias"},
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
