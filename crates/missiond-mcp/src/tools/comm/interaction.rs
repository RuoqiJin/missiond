use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_interaction",
        "统一外部交互入口 facade：receive/confirm_intent/confirm_plan/follow/status。Web、iOS、微信桥接和外部服务都应先进入 InteractionEnvelope，再走 Auth 权限、grounding、intent/plan gate、BoardTask 和 task-result-artifact。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["receive", "confirm_intent", "confirm_plan", "follow", "status"]
                },
                "interaction_id": {"type": "string"},
                "channel": {"type": "string", "description": "web | ios | jarvis | wechat | service"},
                "external_user_id": {"type": "string"},
                "auth_token": {"type": "string", "description": "Auth bearer/service token; secrets should normally be passed through HTTP Authorization, not stored."},
                "conversation_id": {"type": "string"},
                "message": {"description": "User/external message text or object"},
                "attachments": {"type": "array", "items": {"type": "object"}},
                "metadata": {"type": "object"},
                "task_id": {"type": "string", "description": "BoardTask id for follow/status"},
                "intent_artifact_id": {"type": "string"},
                "plan_artifact_id": {"type": "string"}
            }
        }),
    )]
}
