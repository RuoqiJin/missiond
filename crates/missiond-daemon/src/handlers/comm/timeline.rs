use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::lenient;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_timeline
    if name == "mission_timeline" {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("query");
        return match action {
            "query" => handle_inner(state, "mission_timeline_query", args).await,
            "trace" => handle_inner(state, "mission_timeline_trace", args).await,
            "stats" => handle_inner(state, "mission_timeline_stats", args).await,
            "search" => handle_inner(state, "mission_timeline_search", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
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
                event_type: None, trace_id: None, since: None, until: None, limit: None, offset: None,
            });
            let limit = args.limit.unwrap_or(50).min(200);
            let offset = args.offset.unwrap_or(0);

            let rows = state.store.query_timeline_filtered(
                args.event_type.as_deref(),
                args.trace_id.as_deref(),
                args.since.as_deref(),
                args.until.as_deref(),
                limit,
                offset,
            ).await.map_err(|e| anyhow!("DB error: {}", e))?;

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

            let rows = state.store.query_timeline_stratified(
                &args.since,
                &args.until,
                per_type_limit,
                &type_limits,
            ).await.map_err(|e| anyhow!("DB error: {}", e))?;

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

            let rows = state.store.query_timeline_by_trace(&trace_id).await
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
                            if let Ok(gemini_rows) = state.store.gemini_log_query(
                                caller, Some(session_id), None, 1,
                            ).await {
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
            let args: Args = serde_json::from_value(args).unwrap_or(Args { since: None, until: None });

            let stats = state.store.query_timeline_stats(
                args.since.as_deref(),
                args.until.as_deref(),
            ).await.map_err(|e| anyhow!("DB error: {}", e))?;

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
            let limit = args.limit.unwrap_or(20).min(100);

            let rows = state.store.query_timeline_search(
                &args.keyword,
                args.since.as_deref(),
                args.until.as_deref(),
                limit,
            ).await.map_err(|e| anyhow!("DB error: {}", e))?;

            let events: Vec<Value> = rows.iter().map(timeline_row_to_json).collect();
            Ok(ToolResult::json(&json!({
                "keyword": args.keyword,
                "count": events.len(),
                "events": events,
            })))
        }

        _ => Err(anyhow!("Unknown timeline tool: {name}")),
    }
}

fn timeline_row_to_json(row: &missiond_core::db::TimelineRow) -> Value {
    let payload: Value = serde_json::from_str(&row.payload).unwrap_or(Value::String(row.payload.clone()));
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
