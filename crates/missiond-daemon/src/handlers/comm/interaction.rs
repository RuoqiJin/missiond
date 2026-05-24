use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

#[derive(Debug, Default, Deserialize)]
struct InteractionArgs {
    action: String,
    #[serde(default)]
    interaction_id: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    external_user_id: Option<String>,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    message: Option<Value>,
    #[serde(default)]
    attachments: Vec<Value>,
    #[serde(default)]
    metadata: Option<Value>,
    #[serde(default, alias = "taskId")]
    task_id: Option<String>,
    #[serde(default)]
    intent_artifact_id: Option<String>,
    #[serde(default)]
    plan_artifact_id: Option<String>,
}

pub(crate) async fn handle(_state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let args: InteractionArgs = serde_json::from_value(args)?;
    match args.action.as_str() {
        "receive" => Ok(ToolResult::json_pretty(&receive_payload(args))),
        "confirm_intent" => Ok(ToolResult::json_pretty(&json!({
            "ok": true,
            "schema": "missiond.interaction-confirmation.v1",
            "interaction_id": args.interaction_id,
            "phase": "awaiting_plan",
            "confirm_payload": {
                "missiond_intent_confirmed": true,
                "missiond_intent_artifact_id": args.intent_artifact_id
            },
            "next_action": "send the returned confirm_payload through /interactions/v1/messages metadata, then review plan_draft"
        }))),
        "confirm_plan" => Ok(ToolResult::json_pretty(&json!({
            "ok": true,
            "schema": "missiond.interaction-confirmation.v1",
            "interaction_id": args.interaction_id,
            "phase": "ready_to_dispatch",
            "confirm_payload": {
                "missiond_intent_confirmed": true,
                "missiond_plan_confirmed": true,
                "missiond_intent_artifact_id": args.intent_artifact_id,
                "missiond_plan_artifact_id": args.plan_artifact_id
            },
            "next_action": "send the returned confirm_payload through /interactions/v1/messages metadata to create BoardTask"
        }))),
        "follow" | "status" => Ok(ToolResult::json_pretty(&json!({
            "ok": true,
            "schema": "missiond.interaction-status.v1",
            "interaction_id": args.interaction_id,
            "task_id": args.task_id,
            "phase": if args.task_id.is_some() { "follow_task_result" } else { "status_requires_task_or_interaction" },
            "next_action": if args.task_id.is_some() {
                "use /interactions/v1/messages metadata.missiond_follow_task_id or mission_board_query(action=get) to observe the task"
            } else {
                "pass task_id or interaction_id returned by receive/BoardTask creation"
            }
        }))),
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_interaction action `{other}`"),
            )
            .with_suggestion("valid actions: receive|confirm_intent|confirm_plan|follow|status"),
        )),
    }
}

fn receive_payload(args: InteractionArgs) -> Value {
    let interaction_id = args
        .interaction_id
        .unwrap_or_else(|| format!("ix-{}", uuid::Uuid::new_v4().simple()));
    let channel = args.channel.unwrap_or_else(|| "web".to_string());
    let message_chars = args
        .message
        .as_ref()
        .and_then(|value| match value {
            Value::String(text) => Some(text.chars().count()),
            Value::Object(map) => map
                .get("text")
                .or_else(|| map.get("content"))
                .and_then(Value::as_str)
                .map(|text| text.chars().count()),
            _ => None,
        })
        .unwrap_or(0);
    json!({
        "ok": true,
        "schema": "missiond.interaction-envelope.v1",
        "interaction_id": interaction_id,
        "phase": "received",
        "channel": channel,
        "external_user_id": args.external_user_id,
        "conversation_id": args.conversation_id,
        "message_chars": message_chars,
        "attachments_count": args.attachments.len(),
        "metadata": args.metadata.unwrap_or_else(|| json!({})),
        "required_chain": [
            "auth",
            "permission_context",
            "mission_context_gather",
            "intent_draft",
            "intent_confirmation",
            "plan_draft",
            "plan_confirmation",
            "BoardTask",
            "task-result-artifact"
        ],
        "next_action": "send this envelope to POST /interactions/v1/messages for the live channel adapter, or continue with confirm_intent/confirm_plan after reviewing artifacts"
    })
}
