use anyhow::Result;
use missiond_core::types::{BoardTask, ConversationEvent};
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
    #[serde(default)]
    limit: Option<i64>,
}

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
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
        "follow" | "status" => Ok(ToolResult::json_pretty(
            &status_payload(state, &args).await?,
        )),
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_interaction action `{other}`"),
            )
            .with_suggestion("valid actions: receive|confirm_intent|confirm_plan|follow|status"),
        )),
    }
}

async fn status_payload(state: &AppState, args: &InteractionArgs) -> Result<Value> {
    let limit = args.limit.unwrap_or(200).clamp(1, 1000);
    let task = if let Some(task_id) = args.task_id.as_deref() {
        state.store.get_board_task(task_id).await?
    } else {
        None
    };
    let resolved_interaction_id = args
        .interaction_id
        .clone()
        .or_else(|| task.as_ref().and_then(task_interaction_id));
    let events = if let Some(interaction_id) = resolved_interaction_id.as_deref() {
        state
            .store
            .get_interaction_events(interaction_id, limit)
            .await?
    } else {
        Vec::new()
    };
    let terminal_event = events
        .iter()
        .rev()
        .find(|event| is_terminal_interaction_event(event))
        .map(interaction_event_to_json);
    let phase = match (
        task.as_ref().map(|task| task.status.as_str()),
        events.is_empty(),
        terminal_event.is_some(),
    ) {
        (_, _, true) => "terminal_event_replayed",
        (Some(status), _, _) if status == "done" || status == "completed" => {
            "task_done_no_terminal_event"
        }
        (Some(status), _, _) if status == "failed" || status == "blocked" => {
            "task_terminal_no_interaction_final"
        }
        (Some(_), false, _) => "task_running_with_interaction_events",
        (Some(_), true, _) => "task_found_no_interaction_events",
        (None, false, _) => "interaction_events_replayed",
        (None, true, _) => "status_requires_task_or_interaction",
    };

    Ok(json!({
        "ok": true,
        "schema": "missiond.interaction-status.v1",
        "interaction_id": resolved_interaction_id,
        "task_id": args.task_id,
        "phase": phase,
        "task": task.as_ref().map(task_to_status_json),
        "events_count": events.len(),
        "events": events.iter().map(interaction_event_to_json).collect::<Vec<_>>(),
        "terminal_event": terminal_event,
        "next_action": status_next_action(phase),
    }))
}

fn task_to_status_json(task: &BoardTask) -> Value {
    json!({
        "id": &task.id,
        "title": &task.title,
        "status": task.status,
        "project": &task.project,
        "assignee": &task.assignee,
        "claim_executor_id": &task.claim_executor_id,
        "claim_executor_type": &task.claim_executor_type,
        "claimed_at": &task.claimed_at,
        "updated_at": &task.updated_at,
        "runtime_metadata": &task.runtime_metadata,
    })
}

fn task_interaction_id(task: &BoardTask) -> Option<String> {
    string_value(&task.runtime_metadata, "interaction_id").or_else(|| {
        task.runtime_metadata
            .get("metadata")
            .and_then(|value| string_value(value, "interaction_id"))
    })
}

fn interaction_event_to_json(event: &ConversationEvent) -> Value {
    let raw_json = event
        .raw_data
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    json!({
        "id": event.id,
        "session_id": &event.session_id,
        "event_uuid": &event.event_uuid,
        "event_type": &event.event_type,
        "event": event.event_type.strip_prefix("interaction.").unwrap_or(&event.event_type),
        "content": &event.content,
        "raw_data": raw_json.unwrap_or_else(|| event.raw_data.as_ref().map_or(Value::Null, |raw| Value::String(raw.clone()))),
        "timestamp": &event.timestamp,
    })
}

fn is_terminal_interaction_event(event: &ConversationEvent) -> bool {
    matches!(
        event.event_type.strip_prefix("interaction."),
        Some("final") | Some("diagnostic") | Some("result_artifact")
    )
}

fn string_value(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn status_next_action(phase: &str) -> &'static str {
    match phase {
        "terminal_event_replayed" => "render terminal_event/result artifact and close the client-side follow loop",
        "task_done_no_terminal_event" | "task_terminal_no_interaction_final" => {
            "inspect task runtime_metadata and stale-final audit; terminal task lacks a replayed interaction final"
        }
        "task_running_with_interaction_events" => {
            "continue mission_interaction(action=follow) or GET /interactions/v1/{interaction_id}/events"
        }
        "task_found_no_interaction_events" => {
            "wait for worker/task-result-artifact events or inspect BoardTask diagnostics"
        }
        "interaction_events_replayed" => {
            "continue replaying interaction events until result_artifact/final appears"
        }
        _ => "pass task_id or interaction_id returned by receive/BoardTask creation",
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
