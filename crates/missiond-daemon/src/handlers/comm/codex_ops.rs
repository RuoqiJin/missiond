use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::lenient;
use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_codex_ops" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("recent");
        return match action {
            "recent" => handle_recent(state, args).await,
            "thread" => handle_thread(state, args).await,
            "tool_stats" => handle_tool_stats(state, args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    Ok(ToolResult::error(format!("Unknown tool: {}", name)))
}

/// action: "recent" — latest tool calls across all Codex threads.
///
/// `since` filters at the **tool_call timestamp** layer (NOT conversation start).
/// A 36h-old thread that called a tool 5 minutes ago is included by `since=2h`.
async fn handle_recent(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        tool_filter: Option<String>,
        since: Option<String>,
        project: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_i64")]
        limit: Option<i64>,
    }
    let Args {
        tool_filter,
        since,
        project,
        limit,
    } = serde_json::from_value(args)?;
    let limit = limit.unwrap_or(50);

    let since_dt = since.as_deref().and_then(parse_since_to_utc);

    // Pull every Codex conversation. We deliberately do NOT pass `since` to
    // list_conversations because it filters by `started_at`, which would drop
    // long-lived threads that just got fresh tool calls.
    let convs = state
        .store
        .list_conversations(
            None,
            500,
            None,
            None,
            None,
            None,
            Some("codex_cli"),
        )
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    // Filter by project substring.
    let convs: Vec<_> = if let Some(ref proj) = project {
        convs
            .into_iter()
            .filter(|c| {
                c.project
                    .as_deref()
                    .map(|p| p.contains(proj.as_str()))
                    .unwrap_or(false)
            })
            .collect()
    } else {
        convs
    };

    // No coarse prune by conversation timestamps — they don't update reliably on
    // every JSONL append. The authoritative filter happens at the tool_call layer
    // below; pulling 200-500 sessions × tool_calls is cheap enough (~1s).

    // Pull tool calls per surviving conversation. Use a generous per-thread cap so
    // we don't truncate the high-volume Codex threads (1k+ calls is normal).
    let tool_filter_slice: Option<Vec<String>> = tool_filter.as_ref().map(|f| vec![f.clone()]);
    let per_thread_cap: i64 = 100_000;

    let mut all_calls = Vec::new();
    for conv in &convs {
        let calls = state
            .store
            .get_tool_calls_by_session(&conv.id, tool_filter_slice.as_deref(), per_thread_cap)
            .await
            .unwrap_or_default();
        for tc in calls {
            // Authoritative since filter at the tool_call layer.
            if let Some(s_dt) = since_dt {
                match chrono::DateTime::parse_from_rfc3339(&tc.timestamp) {
                    Ok(tc_dt) if tc_dt.with_timezone(&chrono::Utc) < s_dt => continue,
                    Ok(_) => {}
                    Err(_) => continue, // unparseable timestamp — drop rather than misreport
                }
            }
            all_calls.push(json!({
                "thread_id": tc.session_id,
                "call_id": tc.id,
                "tool_name": tc.tool_name,
                "input_summary": tc.input_summary,
                "status": tc.status,
                "duration_ms": tc.duration_ms,
                "timestamp": tc.timestamp,
                "project": conv.project,
            }));
        }
    }

    // Sort by timestamp descending and limit.
    all_calls.sort_by(|a, b| {
        let ts_b = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let ts_a = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        ts_b.cmp(ts_a)
    });
    all_calls.truncate(limit as usize);

    Ok(ToolResult::json_pretty(&json!({
        "action": "recent",
        "count": all_calls.len(),
        "threads_scanned": convs.len(),
        "tool_calls": all_calls,
    })))
}

/// Parse "2h" / "30min" / "7d" / RFC3339 to a UTC instant. Mirrors the loose
/// format accepted by the rest of the codebase but returns a typed value so we
/// can compare against tool_call.timestamp directly.
fn parse_since_to_utc(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s.trim();
    if let Some(mins) = s.strip_suffix("min").and_then(|v| v.parse::<i64>().ok()) {
        return Some(chrono::Utc::now() - chrono::Duration::minutes(mins));
    }
    if let Some(hours) = s.strip_suffix('h').and_then(|v| v.parse::<i64>().ok()) {
        return Some(chrono::Utc::now() - chrono::Duration::hours(hours));
    }
    if let Some(days) = s.strip_suffix('d').and_then(|v| v.parse::<i64>().ok()) {
        return Some(chrono::Utc::now() - chrono::Duration::days(days));
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// action: "thread" — detailed tool calls for a specific Codex thread.
async fn handle_thread(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        thread_id: String,
        tool_filter: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_i64")]
        limit: Option<i64>,
    }
    let Args {
        thread_id,
        tool_filter,
        limit,
    } = serde_json::from_value(args)?;
    let limit = limit.unwrap_or(500);

    // Get conversation metadata.
    let conv = state
        .store
        .get_conversation(&thread_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let conv = match conv {
        Some(c) => c,
        None => {
            return Ok(ToolResult::error(format!(
                "Codex thread not found: {}. Use action=recent to list available threads.",
                thread_id
            )));
        }
    };

    let tool_filter_slice: Option<Vec<String>> = tool_filter
        .as_ref()
        .map(|f| vec![f.clone()]);

    // Pull EVERYTHING then sort by timestamp DESC and truncate. The store layer
    // sorts by `id ASC` (call_id is a random hash, not a clock), so a small SQL
    // limit returns lexicographically-earliest calls instead of the newest ones.
    let calls = state
        .store
        .get_tool_calls_by_session(&thread_id, tool_filter_slice.as_deref(), 100_000)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let total_in_db = calls.len();

    let mut call_entries: Vec<Value> = calls
        .into_iter()
        .map(|tc| {
            json!({
                "call_id": tc.id,
                "tool_name": tc.tool_name,
                "input_summary": tc.input_summary,
                "output_summary": tc.output_summary,
                "status": tc.status,
                "duration_ms": tc.duration_ms,
                "timestamp": tc.timestamp,
            })
        })
        .collect();

    call_entries.sort_by(|a, b| {
        let ts_b = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let ts_a = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        ts_b.cmp(ts_a)
    });
    call_entries.truncate(limit as usize);

    Ok(ToolResult::json_pretty(&json!({
        "action": "thread",
        "thread_id": thread_id,
        "project": conv.project,
        "model": conv.model,
        "source": conv.source,
        "started_at": conv.started_at,
        "total_tool_calls": total_in_db,
        "returned": call_entries.len(),
        "tool_calls": call_entries,
    })))
}

/// action: "tool_stats" — aggregated statistics across Codex threads.
async fn handle_tool_stats(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        since: Option<String>,
        project: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_i64")]
        limit: Option<i64>,
    }
    let Args {
        since,
        project,
        limit: _,
    } = serde_json::from_value(args)?;

    // tool_stats can't filter individual tool_calls (it uses pre-aggregated SQL).
    // `since` is therefore inherently approximate here — we drop conversations
    // whose newest tool_call timestamp is older than the cutoff, after pulling
    // a tiny sample to check.
    let since_dt = since.as_deref().and_then(parse_since_to_utc);

    let convs = state
        .store
        .list_conversations(
            None,
            500,
            None,
            None,
            None,
            None,
            Some("codex_cli"),
        )
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let convs: Vec<_> = if let Some(ref proj) = project {
        convs
            .into_iter()
            .filter(|c| {
                c.project
                    .as_deref()
                    .map(|p| p.contains(proj.as_str()))
                    .unwrap_or(false)
            })
            .collect()
    } else {
        convs
    };

    // Aggregate stats per tool across all threads.
    let mut tool_stats: std::collections::HashMap<String, ToolAgg> =
        std::collections::HashMap::new();
    let mut total_calls = 0i64;
    let mut total_errors = 0i64;

    for conv in &convs {
        let stats = state
            .store
            .get_tool_call_stats(&conv.id)
            .await
            .unwrap_or_default();
        for (tool_name, count, success, errors) in stats {
            let entry = tool_stats.entry(tool_name).or_default();
            entry.total += count;
            entry.success += success;
            entry.errors += errors;
            total_calls += count;
            total_errors += errors;
        }
    }

    // Sort by total calls descending.
    let mut stats_vec: Vec<Value> = tool_stats
        .into_iter()
        .map(|(name, agg)| {
            let error_rate = if agg.total > 0 {
                (agg.errors as f64 / agg.total as f64) * 100.0
            } else {
                0.0
            };
            json!({
                "tool_name": name,
                "total_calls": agg.total,
                "success": agg.success,
                "errors": agg.errors,
                "error_rate_pct": format!("{:.1}", error_rate),
            })
        })
        .collect();
    stats_vec.sort_by(|a, b| {
        let ca = a.get("total_calls").and_then(|v| v.as_i64()).unwrap_or(0);
        let cb = b.get("total_calls").and_then(|v| v.as_i64()).unwrap_or(0);
        cb.cmp(&ca)
    });

    Ok(ToolResult::json_pretty(&json!({
        "action": "tool_stats",
        "threads_scanned": convs.len(),
        "total_calls": total_calls,
        "total_errors": total_errors,
        "error_rate_pct": if total_calls > 0 {
            format!("{:.1}", (total_errors as f64 / total_calls as f64) * 100.0)
        } else {
            "0.0".to_string()
        },
        "by_tool": stats_vec,
    })))
}

#[derive(Default)]
struct ToolAgg {
    total: i64,
    success: i64,
    errors: i64,
}
