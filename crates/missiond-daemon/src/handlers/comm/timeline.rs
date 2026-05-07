use anyhow::{anyhow, Result};
use missiond_core::event::events::{BoardEvent, SlotEvent, SystemEvent, TaskEvent};
use missiond_core::event::DomainEvent;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::context::v3_blueprint_runtime::ConversationIngestionRuntimeConfig;
use crate::lenient;
use crate::state::AppState;

fn load_conversation_config() -> Result<ConversationIngestionRuntimeConfig> {
    ConversationIngestionRuntimeConfig::load_for_current_dir()
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_timeline
    if name == "mission_timeline" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("query");
        return match action {
            "query" => handle_inner(state, "mission_timeline_query", args).await,
            "trace" => handle_inner(state, "mission_timeline_trace", args).await,
            "stats" => handle_inner(state, "mission_timeline_stats", args).await,
            "search" => handle_inner(state, "mission_timeline_search", args).await,
            "wait" => handle_inner(state, "mission_timeline_wait", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    let config = load_conversation_config()?;
    match name {
        "mission_timeline_query" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                event_type: Option<String>,
                trace_id: Option<String>,
                since: Option<String>,
                until: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                offset: Option<i64>,
            }
            let args: Args = serde_json::from_value(args).unwrap_or(Args {
                event_type: None,
                trace_id: None,
                since: None,
                until: None,
                limit: None,
                offset: None,
            });
            let limit = config.timeline_query_limit(args.limit);
            let offset = args.offset.unwrap_or(0);

            let rows = state
                .store
                .query_timeline_filtered(
                    args.event_type.as_deref(),
                    args.trace_id.as_deref(),
                    args.since.as_deref(),
                    args.until.as_deref(),
                    limit,
                    offset,
                )
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            let events: Vec<Value> = rows.iter().map(timeline_row_to_json).collect();
            Ok(ToolResult::json(&json!({
                "count": events.len(),
                "offset": offset,
                "events": events,
            })))
        }

        "mission_timeline_stratified" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                since: String,
                until: String,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                per_type_limit: Option<i64>,
                #[serde(default)]
                type_limits: Option<std::collections::HashMap<String, i64>>,
            }
            let args: Args = serde_json::from_value(args)?;
            let per_type_limit = args.per_type_limit.unwrap_or(80);
            let type_limits = args.type_limits.unwrap_or_default();

            let rows = state
                .store
                .query_timeline_stratified(&args.since, &args.until, per_type_limit, &type_limits)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            let events: Vec<Value> = rows.iter().map(timeline_row_to_json).collect();
            Ok(ToolResult::json(&json!({
                "count": events.len(),
                "events": events,
            })))
        }

        "mission_timeline_trace" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                trace_id: String,
            }
            let Args { trace_id } = serde_json::from_value(args)?;

            let rows = state
                .store
                .query_timeline_by_trace(&trace_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if rows.is_empty() {
                return Ok(ToolResult::json(&json!({
                    "trace_id": trace_id,
                    "count": 0,
                    "events": [],
                    "hint": "No events found for this trace_id. Use mission_timeline_search to find the correct trace_id."
                })));
            }

            // Enrich gemini_request_completed events with full data from gemini_requests table
            let mut events: Vec<Value> = Vec::with_capacity(rows.len());
            for row in &rows {
                let mut ev = timeline_row_to_json(row);
                if row.event_type == "gemini_request_completed" {
                    // Try to find matching gemini_request by created_at proximity
                    if let Ok(payload) = serde_json::from_str::<Value>(&row.payload) {
                        let caller = payload.get("caller").and_then(|v| v.as_str());
                        let session_id = payload.get("session_id").and_then(|v| v.as_str());
                        if let Some(session_id) = session_id {
                            if let Ok(gemini_rows) = state
                                .store
                                .gemini_log_query(caller, Some(session_id), None, 1)
                                .await
                            {
                                if let Some(gemini_detail) = gemini_rows.first() {
                                    ev["gemini_detail"] = gemini_detail.clone();
                                }
                            }
                        }
                    }
                }
                events.push(ev);
            }

            Ok(ToolResult::json_pretty(&json!({
                "trace_id": trace_id,
                "count": events.len(),
                "time_range": {
                    "first": rows.first().map(|r| &r.created_at),
                    "last": rows.last().map(|r| &r.created_at),
                },
                "events": events,
            })))
        }

        "mission_timeline_stats" => {
            #[derive(Deserialize)]
            struct Args {
                since: Option<String>,
                until: Option<String>,
            }
            let args: Args = serde_json::from_value(args).unwrap_or(Args {
                since: None,
                until: None,
            });

            let stats = state
                .store
                .query_timeline_stats(args.since.as_deref(), args.until.as_deref())
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            Ok(ToolResult::json_pretty(&stats))
        }

        "mission_timeline_search" => {
            #[derive(Deserialize)]
            struct Args {
                keyword: String,
                since: Option<String>,
                until: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
            }
            let args: Args = serde_json::from_value(args)?;
            let limit = config.timeline_search_limit(args.limit);

            let rows = state
                .store
                .query_timeline_search(
                    &args.keyword,
                    args.since.as_deref(),
                    args.until.as_deref(),
                    limit,
                )
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            let events: Vec<Value> = rows.iter().map(timeline_row_to_json).collect();
            Ok(ToolResult::json(&json!({
                "keyword": args.keyword,
                "count": events.len(),
                "events": events,
            })))
        }

        "mission_timeline_wait" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                domain: String,
                kind: Option<String>,
                task_id: Option<String>,
                slot_id: Option<String>,
                status: Option<String>,
                service_id: Option<String>,
                event_id: Option<String>,
                event_kind: Option<String>,
                project_id: Option<String>,
                correlation_id: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                timeout_ms: Option<i64>,
            }
            let args: Args = serde_json::from_value(args)?;
            let domain = args.domain.trim().to_ascii_lowercase();
            let timeout_ms = args.timeout_ms.unwrap_or(30_000).clamp(100, 300_000) as u64;
            let filter = WaitFilter {
                kind: args.kind,
                task_id: args.task_id,
                slot_id: args.slot_id,
                status: args.status,
                service_id: args.service_id,
                event_id: args.event_id,
                event_kind: args.event_kind,
                project_id: args.project_id,
                correlation_id: args.correlation_id,
            };
            let timeout = std::time::Duration::from_millis(timeout_ms);
            let wait_result = match domain.as_str() {
                "board" | "board_task" | "boardtask" => {
                    let rx = state.bus.dispatcher.topic::<BoardEvent>().subscribe();
                    wait_board_event(rx, &filter, timeout).await
                }
                "slot" => {
                    let rx = state.bus.dispatcher.topic::<SlotEvent>().subscribe();
                    wait_slot_event(rx, &filter, timeout).await
                }
                "task" => {
                    let rx = state.bus.dispatcher.topic::<TaskEvent>().subscribe();
                    wait_task_event(rx, &filter, timeout).await
                }
                "system" => {
                    let rx = state.bus.dispatcher.topic::<SystemEvent>().subscribe();
                    wait_system_event(rx, &filter, timeout).await
                }
                other => {
                    return Ok(ToolResult::error(format!(
                        "Unknown wait domain: {other}. Expected board|slot|task|system"
                    )));
                }
            };

            Ok(ToolResult::json(&wait_result))
        }

        _ => Err(anyhow!("Unknown timeline tool: {name}")),
    }
}

async fn wait_board_event(
    rx: broadcast::Receiver<Arc<BoardEvent>>,
    args: &WaitFilter,
    timeout: std::time::Duration,
) -> Value {
    wait_for_event(rx, timeout, |event| {
        if !kind_matches(args.kind(), event.kind()) {
            return None;
        }
        if let Some(task_id) = args.task_id() {
            if board_task_id(event) != Some(task_id) {
                return None;
            }
        }
        if let Some(status) = args.status() {
            if !board_status_matches(event, status) {
                return None;
            }
        }
        serde_json::to_value(event).ok()
    })
    .await
}

async fn wait_slot_event(
    rx: broadcast::Receiver<Arc<SlotEvent>>,
    args: &WaitFilter,
    timeout: std::time::Duration,
) -> Value {
    wait_for_event(rx, timeout, |event| {
        if !kind_matches(args.kind(), event.kind()) {
            return None;
        }
        if let Some(slot_id) = args.slot_id() {
            if slot_event_slot_id(event) != Some(slot_id) {
                return None;
            }
        }
        if let Some(task_id) = args.task_id() {
            if slot_event_task_id(event) != Some(task_id) {
                return None;
            }
        }
        if let Some(status) = args.status() {
            if !slot_status_matches(event, status) {
                return None;
            }
        }
        serde_json::to_value(event).ok()
    })
    .await
}

async fn wait_task_event(
    rx: broadcast::Receiver<Arc<TaskEvent>>,
    args: &WaitFilter,
    timeout: std::time::Duration,
) -> Value {
    wait_for_event(rx, timeout, |event| {
        if !kind_matches(args.kind(), event.kind()) {
            return None;
        }
        if let Some(task_id) = args.task_id() {
            if task_event_task_id(event) != Some(task_id) {
                return None;
            }
        }
        serde_json::to_value(event).ok()
    })
    .await
}

async fn wait_system_event(
    rx: broadcast::Receiver<Arc<SystemEvent>>,
    args: &WaitFilter,
    timeout: std::time::Duration,
) -> Value {
    wait_for_event(rx, timeout, |event| {
        if !kind_matches(args.kind(), event.kind()) {
            return None;
        }
        match event {
            SystemEvent::ExternalServiceEvent {
                service_id,
                event_id,
                event_kind,
                payload_json,
                ..
            } => {
                if let Some(expected) = args.service_id() {
                    if service_id != expected {
                        return None;
                    }
                }
                if let Some(expected) = args.event_id() {
                    if event_id != expected {
                        return None;
                    }
                }
                if let Some(expected) = args.event_kind() {
                    if event_kind != expected {
                        return None;
                    }
                }
                if let Some(expected) = args.project_id() {
                    if !external_payload_field_matches(payload_json, "project_id", expected) {
                        return None;
                    }
                }
                if let Some(expected) = args.correlation_id() {
                    if !external_payload_field_matches(payload_json, "correlation_id", expected) {
                        return None;
                    }
                }
            }
            SystemEvent::ContextualCommitDetected { slot_id, .. }
            | SystemEvent::ToolCompleted { slot_id, .. } => {
                if let Some(expected) = args.slot_id() {
                    if slot_id.as_deref() != Some(expected) {
                        return None;
                    }
                }
            }
            _ => {
                if args.service_id().is_some()
                    || args.event_id().is_some()
                    || args.event_kind().is_some()
                    || args.project_id().is_some()
                    || args.correlation_id().is_some()
                    || args.slot_id().is_some()
                {
                    return None;
                }
            }
        }
        serde_json::to_value(event).ok()
    })
    .await
}

async fn wait_for_event<T, F>(
    mut rx: broadcast::Receiver<Arc<T>>,
    timeout: std::time::Duration,
    mut matches: F,
) -> Value
where
    T: missiond_core::event::DomainEvent + serde::Serialize + Send + Sync + 'static,
    F: FnMut(&T) -> Option<Value>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return json!({
                "ok": false,
                "status": "timeout",
                "domain": format!("{:?}", T::domain()),
                "timeoutMs": timeout.as_millis(),
                "diagnostic": "eventbus-wait-timeout; callers may use bounded polling fallback"
            });
        }
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Ok(event)) => {
                if let Some(payload) = matches(&event) {
                    return json!({
                        "ok": true,
                        "status": "matched",
                        "domain": format!("{:?}", T::domain()),
                        "kind": event.kind(),
                        "event": payload,
                    });
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(skipped))) => {
                return json!({
                    "ok": false,
                    "status": "lagged",
                    "domain": format!("{:?}", T::domain()),
                    "skipped": skipped,
                    "diagnostic": "eventbus-wait-lagged; use timeline query fallback from durable log"
                });
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                return json!({
                    "ok": false,
                    "status": "closed",
                    "domain": format!("{:?}", T::domain()),
                    "diagnostic": "eventbus-wait-channel-closed"
                });
            }
            Err(_) => {}
        }
    }
}

struct WaitFilter {
    kind: Option<String>,
    task_id: Option<String>,
    slot_id: Option<String>,
    status: Option<String>,
    service_id: Option<String>,
    event_id: Option<String>,
    event_kind: Option<String>,
    project_id: Option<String>,
    correlation_id: Option<String>,
}

impl WaitFilter {
    fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }
    fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }
    fn slot_id(&self) -> Option<&str> {
        self.slot_id.as_deref()
    }
    fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }
    fn service_id(&self) -> Option<&str> {
        self.service_id.as_deref()
    }
    fn event_id(&self) -> Option<&str> {
        self.event_id.as_deref()
    }
    fn event_kind(&self) -> Option<&str> {
        self.event_kind.as_deref()
    }
    fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }
    fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }
}

fn external_payload_field_matches(payload_json: &str, field: &str, expected: &str) -> bool {
    let Ok(payload) = serde_json::from_str::<Value>(payload_json) else {
        return false;
    };
    payload
        .get("_envelope")
        .and_then(|v| v.get(field))
        .or_else(|| payload.get(field))
        .and_then(|v| v.as_str())
        .map(|actual| actual == expected)
        .unwrap_or(false)
}

fn kind_matches(expected: Option<&str>, actual: &str) -> bool {
    expected
        .map(|kind| kind.eq_ignore_ascii_case(actual))
        .unwrap_or(true)
}

fn board_task_id(event: &BoardEvent) -> Option<&str> {
    match event {
        BoardEvent::TaskCreated { task_id, .. }
        | BoardEvent::StatusChanged { task_id, .. }
        | BoardEvent::NoteAdded { task_id, .. }
        | BoardEvent::Claimed { task_id, .. }
        | BoardEvent::Deleted { task_id, .. }
        | BoardEvent::Updated { task_id, .. } => Some(task_id),
    }
}

fn board_status_matches(event: &BoardEvent, expected: &str) -> bool {
    match event {
        BoardEvent::StatusChanged { new_status, .. } => new_status.eq_ignore_ascii_case(expected),
        BoardEvent::Updated { status, .. } => status.eq_ignore_ascii_case(expected),
        _ => false,
    }
}

fn slot_event_slot_id(event: &SlotEvent) -> Option<&str> {
    match event {
        SlotEvent::BecameIdle { slot_id }
        | SlotEvent::StateChanged { slot_id, .. }
        | SlotEvent::TaskDispatched { slot_id, .. }
        | SlotEvent::Stuck { slot_id, .. } => Some(slot_id),
    }
}

fn slot_event_task_id(event: &SlotEvent) -> Option<&str> {
    match event {
        SlotEvent::TaskDispatched { task_id, .. } => task_id.as_deref(),
        _ => None,
    }
}

fn slot_status_matches(event: &SlotEvent, expected: &str) -> bool {
    match event {
        SlotEvent::BecameIdle { .. } => expected.eq_ignore_ascii_case("idle"),
        SlotEvent::StateChanged { new_state, .. } => new_state.eq_ignore_ascii_case(expected),
        SlotEvent::Stuck { .. } => expected.eq_ignore_ascii_case("stuck"),
        SlotEvent::TaskDispatched { .. } => expected.eq_ignore_ascii_case("dispatched"),
    }
}

fn task_event_task_id(event: &TaskEvent) -> Option<&str> {
    match event {
        TaskEvent::Created { task_id } | TaskEvent::Completed { task_id } => Some(task_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_wait_predicates_match_status_and_task() {
        let event = BoardEvent::StatusChanged {
            task_id: "task-1".to_string(),
            old_status: "running".to_string(),
            new_status: "done".to_string(),
        };
        assert_eq!(board_task_id(&event), Some("task-1"));
        assert!(board_status_matches(&event, "done"));
        assert!(!board_status_matches(&event, "blocked"));
        assert!(kind_matches(Some("status_changed"), event.kind()));
    }

    #[test]
    fn slot_wait_predicates_match_dispatched_task_and_idle() {
        let dispatched = SlotEvent::TaskDispatched {
            slot_id: "slot-a".to_string(),
            task_id: Some("task-2".to_string()),
            purpose: "boardtask".to_string(),
            prompt_chars: 42,
            preview: "do work".to_string(),
            cited_kb_ids: vec![],
        };
        assert_eq!(slot_event_slot_id(&dispatched), Some("slot-a"));
        assert_eq!(slot_event_task_id(&dispatched), Some("task-2"));
        assert!(slot_status_matches(&dispatched, "dispatched"));

        let idle = SlotEvent::BecameIdle {
            slot_id: "slot-a".to_string(),
        };
        assert!(slot_status_matches(&idle, "idle"));
    }

    #[test]
    fn task_wait_predicates_match_submit_queue_task_id() {
        let event = TaskEvent::Completed {
            task_id: "legacy-task".to_string(),
        };
        assert_eq!(task_event_task_id(&event), Some("legacy-task"));
        assert!(kind_matches(Some("completed"), event.kind()));
    }

    #[test]
    fn external_payload_field_matches_envelope_or_legacy_payload() {
        let payload = r#"{"_envelope":{"project_id":"auth","correlation_id":"run-1"}}"#;
        assert!(external_payload_field_matches(
            payload,
            "project_id",
            "auth"
        ));
        assert!(external_payload_field_matches(
            r#"{"project_id":"router"}"#,
            "project_id",
            "router"
        ));
        assert!(!external_payload_field_matches(
            payload,
            "project_id",
            "router"
        ));
    }
}

fn timeline_row_to_json(row: &missiond_core::db::TimelineRow) -> Value {
    let payload: Value =
        serde_json::from_str(&row.payload).unwrap_or(Value::String(row.payload.clone()));
    json!({
        "seq": row.seq,
        "event_type": row.event_type,
        "trace_id": row.trace_id,
        "span_id": row.span_id,
        "parent_span_id": row.parent_span_id,
        "summary": row.summary,
        "payload": payload,
        "created_at": row.created_at,
    })
}
