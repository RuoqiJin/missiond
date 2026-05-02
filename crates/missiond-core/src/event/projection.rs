//! Timeline projection — event_log → legacy timeline shape.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` v1.3.0 §4.6 event_log
//! declares `:ssot-declaration "event_log = timeline SSOT"` and introduces
//! the `read-ui-projection` access-pattern. This module is the canonical
//! implementation of that projection: it reads `event_log` rows and shapes
//! them to the TimelineRow / TimelineStats format that the mission_timeline
//! MCP tool + WS catch-up protocol expect.
//!
//! Design:
//!
//! * `event_type` column = v1 wire_type (e.g. `"board_task_created"`), produced
//!   by [`crate::event::wire_format::v2_payload_to_v1_shape`]. This is the
//!   **SSOT** shared with the live `ws_bridge` path — catch-up replay and
//!   live push emit byte-identical envelopes. Previously catch-up returned
//!   `"domain::kind"` and live returned the v1 wire_type, which broke the
//!   frontend on reconnect; now both converge on v1 wire_type.
//! * `payload` column = v1-shape JSON string (also from the SSOT mapper).
//!   This differs from the raw `payload_inline` (which is externally-tagged
//!   `{"VariantName": {...}}`). Callers that need the raw shape should
//!   query `event_log` directly.
//! * `summary` is extracted from the raw `payload_inline` (the
//!   externally-tagged shape) **before** wire-mapping, so the
//!   `summary / preview / title` heuristic still works.
//! * Search uses Postgres FTS on `payload_inline::text` (index added by
//!   migration `20260420100000_event_log_fts.sql`).

#![cfg(feature = "postgres")]

use std::collections::HashMap;

use sqlx::PgPool;

use crate::db::error::{DbError, DbResult};
use crate::db::shared::{LatencyStats, TimelineRow, TimelineStats};
use crate::event::wire_format::v2_payload_to_v1_shape;

/// Parse relative time strings like "10min", "1h", "24h", "7d" into timestamp strings.
/// Mirrors the old `pg/timeline.rs` helper so the mission_timeline MCP + catch-up
/// query semantics stay identical.
fn parse_relative_time(s: &str) -> String {
    let s = s.trim();
    if let Some(mins) = s.strip_suffix("min").and_then(|v| v.parse::<i64>().ok()) {
        return format!(
            "{}",
            (chrono::Utc::now() - chrono::Duration::minutes(mins)).format("%Y-%m-%d %H:%M:%S")
        );
    }
    if let Some(hours) = s.strip_suffix('h').and_then(|v| v.parse::<i64>().ok()) {
        return format!(
            "{}",
            (chrono::Utc::now() - chrono::Duration::hours(hours)).format("%Y-%m-%d %H:%M:%S")
        );
    }
    if let Some(days) = s.strip_suffix('d').and_then(|v| v.parse::<i64>().ok()) {
        return format!(
            "{}",
            (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%d %H:%M:%S")
        );
    }
    if s.contains('T') {
        return s.replace('T', " ").chars().take(19).collect();
    }
    s.to_string()
}

fn parse_since(s: &str) -> String {
    let ts = parse_relative_time(s);
    if ts.len() == 10 {
        format!("{} 00:00:00", ts)
    } else {
        ts
    }
}

fn parse_until(s: &str) -> String {
    let ts = parse_relative_time(s);
    if ts.len() == 10 {
        format!("{} 23:59:59", ts)
    } else {
        ts
    }
}

/// Shape of a raw event_log row needed for projection.
#[derive(sqlx::FromRow)]
struct RawEventLogRow {
    seq: i64,
    domain: String,
    kind: String,
    payload_inline: Option<serde_json::Value>,
    trace_id: Option<uuid::Uuid>,
    span_id: Option<uuid::Uuid>,
    parent_span_id: Option<uuid::Uuid>,
    ts: chrono::DateTime<chrono::Utc>,
}

impl RawEventLogRow {
    fn into_timeline_row(self) -> TimelineRow {
        // Extract summary from the raw externally-tagged payload BEFORE wire
        // mapping — the v1-shape payload is flat and won't retain the
        // heuristic keys reliably.
        let summary = extract_summary(&self.payload_inline);

        // Map `(domain, kind, raw_payload)` through the SSOT v1-wire mapper.
        // This is the same transform that the live `ws_bridge` path applies,
        // so catch-up + live emit byte-identical envelopes.
        let raw_payload = self.payload_inline.unwrap_or(serde_json::Value::Null);
        let (wire_type, v1_payload) =
            v2_payload_to_v1_shape(&self.domain, &self.kind, &raw_payload);

        TimelineRow {
            seq: self.seq,
            trace_id: self.trace_id.map(|u| u.to_string()),
            span_id: self.span_id.map(|u| u.to_string()),
            parent_span_id: self.parent_span_id.map(|u| u.to_string()),
            event_type: wire_type.to_string(),
            summary,
            payload: v1_payload.to_string(),
            created_at: self.ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}

/// Extract a legible `summary` from the variant-wrapped payload.
/// v2 payload shape is `{"VariantName": {...fields...}}` (externally tagged).
/// We peek into the single variant and prefer `summary` > `preview` > `title`.
fn extract_summary(payload: &Option<serde_json::Value>) -> Option<String> {
    let payload = payload.as_ref()?;
    let inner = payload
        .as_object()
        .and_then(|obj| obj.values().next())
        .unwrap_or(payload);
    inner
        .get("summary")
        .or_else(|| inner.get("preview"))
        .or_else(|| inner.get("title"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Parse `event_type` input for SQL filtering on `event_log.domain` and
/// `event_log.kind`. Accepts three forms (in order of preference):
///
/// 1. `"domain::kind"` (internal code, e.g. `"message::logged"`) — splits
///    on `::` and returns both parts.
/// 2. v1 wire_type (e.g. `"board_task_created"`) — looked up via
///    [`crate::event::wire_format::v1_wire_type_to_v2_parts`]. Returns
///    `(domain, kind?)`; the `kind?` may be `None` for many-to-one wire
///    types that need domain-only match.
/// 3. Anything else — returns `(None, None)` and the caller treats the
///    input as a literal kind match (legacy best-effort).
fn parse_event_type_filter(event_type: &str) -> (Option<String>, Option<String>) {
    if let Some((d, k)) = event_type.split_once("::") {
        return (Some(d.to_string()), Some(k.to_string()));
    }
    if let Some((d, k)) = crate::event::wire_format::v1_wire_type_to_v2_parts(event_type) {
        return (Some(d.to_string()), k.map(|s| s.to_string()));
    }
    (None, None)
}

// ─────────────────────────────────────────────────────────────────────────
// Public projection API — mirrors the old TimelineStore read methods.
// ─────────────────────────────────────────────────────────────────────────

/// SELECT MAX(seq) FROM event_log.
pub async fn timeline_latest_seq(pool: &PgPool) -> DbResult<i64> {
    let (seq,): (Option<i64>,) = sqlx::query_as("SELECT MAX(seq) FROM event_log")
        .fetch_one(pool)
        .await
        .map_err(DbError::from)?;
    Ok(seq.unwrap_or(0))
}

/// Events with seq > since_seq, ordered ASC (catch-up replay).
pub async fn query_timeline_since(
    pool: &PgPool,
    since_seq: i64,
    limit: usize,
) -> DbResult<Vec<TimelineRow>> {
    let rows: Vec<RawEventLogRow> = sqlx::query_as(
        "SELECT seq, domain, kind, payload_inline, trace_id, span_id, parent_span_id, ts
         FROM event_log WHERE seq > $1 ORDER BY seq ASC LIMIT $2",
    )
    .bind(since_seq)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    .map_err(DbError::from)?;
    Ok(rows
        .into_iter()
        .map(RawEventLogRow::into_timeline_row)
        .collect())
}

/// Filtered query: by event_type ("domain::kind"), trace_id, time range.
pub async fn query_timeline_filtered(
    pool: &PgPool,
    event_type: Option<&str>,
    trace_id: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<TimelineRow>> {
    let mut conditions: Vec<String> = Vec::new();
    let mut domain_val: Option<String> = None;
    let mut kind_val: Option<String> = None;
    let mut tid_val: Option<uuid::Uuid> = None;
    let mut since_val: Option<String> = None;
    let mut until_val: Option<String> = None;
    let mut param_idx: u32 = 1;

    if let Some(et) = event_type {
        let (d, k) = parse_event_type_filter(et);
        if let Some(d) = d {
            conditions.push(format!("domain = ${}", param_idx));
            param_idx += 1;
            domain_val = Some(d);
        }
        if let Some(k) = k {
            conditions.push(format!("kind = ${}", param_idx));
            param_idx += 1;
            kind_val = Some(k);
        }
        if domain_val.is_none() && kind_val.is_none() {
            // Legacy wire_type — match on kind as best-effort.
            conditions.push(format!("kind = ${}", param_idx));
            param_idx += 1;
            kind_val = Some(et.to_string());
        }
    }
    if let Some(tid) = trace_id {
        let uuid = uuid::Uuid::parse_str(tid)
            .map_err(|e| DbError::Other(format!("trace_id not a UUID: {e}")))?;
        conditions.push(format!("trace_id = ${}", param_idx));
        param_idx += 1;
        tid_val = Some(uuid);
    }
    if let Some(s) = since {
        // event_log.ts is timestamptz; bound parameters arrive as text and must be cast
        // explicitly or PG raises `operator does not exist: timestamp with time zone >= text`.
        conditions.push(format!("ts >= ${}::timestamptz", param_idx));
        param_idx += 1;
        since_val = Some(parse_since(s));
    }
    if let Some(u) = until {
        conditions.push(format!("ts <= ${}::timestamptz", param_idx));
        param_idx += 1;
        until_val = Some(parse_until(u));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT seq, domain, kind, payload_inline, trace_id, span_id, parent_span_id, ts
         FROM event_log {} ORDER BY seq DESC LIMIT ${} OFFSET ${}",
        where_clause,
        param_idx,
        param_idx + 1
    );

    let mut q = sqlx::query_as::<_, RawEventLogRow>(&sql);
    if let Some(v) = domain_val {
        q = q.bind(v);
    }
    if let Some(v) = kind_val {
        q = q.bind(v);
    }
    if let Some(v) = tid_val {
        q = q.bind(v);
    }
    if let Some(v) = since_val {
        q = q.bind(v);
    }
    if let Some(v) = until_val {
        q = q.bind(v);
    }
    q = q.bind(limit).bind(offset);

    let rows = q.fetch_all(pool).await.map_err(DbError::from)?;
    Ok(rows
        .into_iter()
        .map(RawEventLogRow::into_timeline_row)
        .collect())
}

/// Stratified: up to `per_type_limit` per (domain, kind) within [since, until].
pub async fn query_timeline_stratified(
    pool: &PgPool,
    since: &str,
    until: &str,
    per_type_limit: i64,
    type_limits: &HashMap<String, i64>,
) -> DbResult<Vec<TimelineRow>> {
    let ts_since = parse_since(since);
    let ts_until = parse_until(until);

    // Per-type CASE expression: keyed by "domain::kind".
    let limit_expr = if type_limits.is_empty() {
        format!("{}", per_type_limit)
    } else {
        let mut cases = String::from("CASE (domain || '::' || kind) ");
        for (et, lim) in type_limits {
            cases.push_str(&format!("WHEN '{}' THEN {} ", et.replace('\'', "''"), lim));
        }
        cases.push_str(&format!("ELSE {} END", per_type_limit));
        cases
    };

    let sql = format!(
        "SELECT seq, domain, kind, payload_inline, trace_id, span_id, parent_span_id, ts
         FROM (
             SELECT *, ROW_NUMBER() OVER (PARTITION BY domain, kind ORDER BY seq DESC) as rn
             FROM event_log
             WHERE ts >= $1::timestamptz AND ts <= $2::timestamptz
         ) sub
         WHERE rn <= ({})
         ORDER BY seq DESC",
        limit_expr
    );

    let rows: Vec<RawEventLogRow> = sqlx::query_as(&sql)
        .bind(&ts_since)
        .bind(&ts_until)
        .fetch_all(pool)
        .await
        .map_err(DbError::from)?;
    Ok(rows
        .into_iter()
        .map(RawEventLogRow::into_timeline_row)
        .collect())
}

/// All events sharing a trace_id, in causal order.
pub async fn query_timeline_by_trace(pool: &PgPool, trace_id: &str) -> DbResult<Vec<TimelineRow>> {
    let uuid = uuid::Uuid::parse_str(trace_id)
        .map_err(|e| DbError::Other(format!("trace_id not a UUID: {e}")))?;
    let rows: Vec<RawEventLogRow> = sqlx::query_as(
        "SELECT seq, domain, kind, payload_inline, trace_id, span_id, parent_span_id, ts
         FROM event_log WHERE trace_id = $1 ORDER BY seq ASC",
    )
    .bind(uuid)
    .fetch_all(pool)
    .await
    .map_err(DbError::from)?;
    Ok(rows
        .into_iter()
        .map(RawEventLogRow::into_timeline_row)
        .collect())
}

/// Stats: total count + per-type distribution + trace stats + Gemini latency.
pub async fn query_timeline_stats(
    pool: &PgPool,
    since: Option<&str>,
    until: Option<&str>,
) -> DbResult<TimelineStats> {
    let mut conditions: Vec<String> = Vec::new();
    let mut since_val: Option<String> = None;
    let mut until_val: Option<String> = None;
    let mut idx: u32 = 1;
    if let Some(s) = since {
        // event_log.ts is timestamptz; cast text-bound parameter explicitly.
        conditions.push(format!("ts >= ${}::timestamptz", idx));
        idx += 1;
        since_val = Some(parse_since(s));
    }
    if let Some(u) = until {
        conditions.push(format!("ts <= ${}::timestamptz", idx));
        // idx += 1; // unused after last
        until_val = Some(parse_until(u));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let total_sql = format!("SELECT COUNT(*) FROM event_log {}", where_clause);
    let mut q = sqlx::query_as::<_, (i64,)>(&total_sql);
    if let Some(ref v) = since_val {
        q = q.bind(v);
    }
    if let Some(ref v) = until_val {
        q = q.bind(v);
    }
    let (total_events,) = q.fetch_one(pool).await.map_err(DbError::from)?;

    let by_type_sql = format!(
        "SELECT (domain || '::' || kind) as event_type, COUNT(*) as n
         FROM event_log {}
         GROUP BY domain, kind ORDER BY n DESC",
        where_clause
    );
    let mut q = sqlx::query_as::<_, (String, i64)>(&by_type_sql);
    if let Some(ref v) = since_val {
        q = q.bind(v);
    }
    if let Some(ref v) = until_val {
        q = q.bind(v);
    }
    let by_type: Vec<(String, i64)> = q.fetch_all(pool).await.map_err(DbError::from)?;

    let traced_where = if where_clause.is_empty() {
        "WHERE trace_id IS NOT NULL".to_string()
    } else {
        format!("{} AND trace_id IS NOT NULL", where_clause)
    };
    let traced_sql = format!(
        "SELECT COUNT(*), COUNT(DISTINCT trace_id) FROM event_log {}",
        traced_where
    );
    let mut q = sqlx::query_as::<_, (i64, i64)>(&traced_sql);
    if let Some(ref v) = since_val {
        q = q.bind(v);
    }
    if let Some(ref v) = until_val {
        q = q.bind(v);
    }
    let (traced_events, unique_traces) = q.fetch_one(pool).await.map_err(DbError::from)?;

    // Gemini latency — v2 records it via LlmEvent/LegacyGeminiRequestCompleted inside
    // payload_inline.LegacyGeminiRequestCompleted.duration_ms.
    let gemini_sql = if where_clause.is_empty() {
        "SELECT CAST(payload_inline->'LegacyGeminiRequestCompleted'->>'duration_ms' AS BIGINT)
         FROM event_log
         WHERE domain = 'llm' AND kind = 'legacy_gemini_request_completed'
         ORDER BY seq DESC LIMIT 100"
            .to_string()
    } else {
        format!(
            "SELECT CAST(payload_inline->'LegacyGeminiRequestCompleted'->>'duration_ms' AS BIGINT)
             FROM event_log {}
             AND domain = 'llm' AND kind = 'legacy_gemini_request_completed'
             ORDER BY seq DESC LIMIT 100",
            where_clause
        )
    };
    let mut q = sqlx::query_as::<_, (Option<i64>,)>(&gemini_sql);
    if let Some(ref v) = since_val {
        q = q.bind(v);
    }
    if let Some(ref v) = until_val {
        q = q.bind(v);
    }
    let duration_rows: Vec<(Option<i64>,)> = q.fetch_all(pool).await.map_err(DbError::from)?;
    let durations: Vec<i64> = duration_rows.into_iter().filter_map(|r| r.0).collect();
    let gemini_latency = if durations.is_empty() {
        None
    } else {
        let mut sorted = durations.clone();
        sorted.sort();
        let len = sorted.len();
        let avg = sorted.iter().sum::<i64>() / len as i64;
        Some(LatencyStats {
            count: len as i64,
            avg_ms: avg,
            p50_ms: sorted[len / 2],
            p90_ms: sorted[(len as f64 * 0.9) as usize],
            p99_ms: sorted[(len as f64 * 0.99).min((len - 1) as f64) as usize],
        })
    };

    Ok(TimelineStats {
        total_events,
        by_type,
        traced_events,
        unique_traces,
        gemini_latency,
    })
}

/// Keyword search over payload_inline using FTS index (added by migration
/// `20260420100000_event_log_fts.sql`). Not an ILIKE scan — frozen lisp
/// §4.6 event_log FTS requirement is explicit.
pub async fn query_timeline_search(
    pool: &PgPool,
    keyword: &str,
    since: Option<&str>,
    until: Option<&str>,
    limit: i64,
) -> DbResult<Vec<TimelineRow>> {
    let mut conditions: Vec<String> = Vec::new();
    conditions.push(
        "to_tsvector('simple', coalesce(payload_inline::text, '')) @@ plainto_tsquery('simple', $1)"
            .to_string(),
    );
    let mut since_val: Option<String> = None;
    let mut until_val: Option<String> = None;
    let mut idx: u32 = 2;
    if let Some(s) = since {
        // event_log.ts is timestamptz; cast text-bound parameter explicitly.
        conditions.push(format!("ts >= ${}::timestamptz", idx));
        idx += 1;
        since_val = Some(parse_since(s));
    }
    if let Some(u) = until {
        conditions.push(format!("ts <= ${}::timestamptz", idx));
        idx += 1;
        until_val = Some(parse_until(u));
    }

    let sql = format!(
        "SELECT seq, domain, kind, payload_inline, trace_id, span_id, parent_span_id, ts
         FROM event_log WHERE {} ORDER BY seq DESC LIMIT ${}",
        conditions.join(" AND "),
        idx
    );

    let mut q = sqlx::query_as::<_, RawEventLogRow>(&sql).bind(keyword);
    if let Some(v) = since_val {
        q = q.bind(v);
    }
    if let Some(v) = until_val {
        q = q.bind(v);
    }
    q = q.bind(limit);
    let rows = q.fetch_all(pool).await.map_err(DbError::from)?;
    Ok(rows
        .into_iter()
        .map(RawEventLogRow::into_timeline_row)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    /// Construct a synthetic event_log row bypassing the DB — sufficient to
    /// exercise the pure projection mapping.
    fn make_raw(domain: &str, kind: &str, payload: serde_json::Value) -> RawEventLogRow {
        RawEventLogRow {
            seq: 100,
            domain: domain.to_string(),
            kind: kind.to_string(),
            payload_inline: Some(payload),
            trace_id: None,
            span_id: Some(uuid::Uuid::nil()),
            parent_span_id: None,
            ts: Utc.with_ymd_and_hms(2026, 4, 19, 0, 0, 0).unwrap(),
        }
    }

    /// The **SSOT test** — proves catch-up emits v1 wire_type, not
    /// `"domain::kind"`. If this ever regresses to `"board::task_created"`
    /// the frontend live/catch-up drift is back.
    #[test]
    fn catch_up_emits_v1_wire_type_not_domain_kind() {
        let raw = make_raw(
            "board",
            "task_created",
            json!({
                "TaskCreated": {
                    "task_id": "t-1",
                    "title": "hello",
                    "category": "code"
                }
            }),
        );
        let row = raw.into_timeline_row();
        assert_eq!(
            row.event_type, "board_task_created",
            "catch-up should emit v1 wire_type, matching live stream"
        );
        assert!(
            !row.event_type.contains("::"),
            "event_type must NOT contain `::` — drift detected"
        );

        // Payload should also be v1 shape (flat keys + `action`), not
        // externally-tagged.
        let payload_val: serde_json::Value = serde_json::from_str(&row.payload).unwrap();
        assert_eq!(payload_val.get("task_id").unwrap(), "t-1");
        assert_eq!(payload_val.get("title").unwrap(), "hello");
        assert_eq!(payload_val.get("action").unwrap(), "created");
        assert!(
            payload_val.get("TaskCreated").is_none(),
            "payload must NOT contain externally-tagged variant name"
        );
    }

    #[test]
    fn catch_up_slot_state_changed_wire_type() {
        let raw = make_raw(
            "slot",
            "became_idle",
            json!({ "BecameIdle": { "slot_id": "slot-a" } }),
        );
        let row = raw.into_timeline_row();
        assert_eq!(row.event_type, "slot_state_changed");
    }

    #[test]
    fn catch_up_message_logged_routes_by_role() {
        let raw = make_raw(
            "message",
            "logged",
            json!({
                "Logged": {
                    "message_id": 1,
                    "session_id": "s-1",
                    "role": "user",
                    "content_chars": 5,
                    "preview": "hello"
                }
            }),
        );
        let row = raw.into_timeline_row();
        // Role-based wire_type routing preserved.
        assert_eq!(row.event_type, "user_message");
    }

    #[test]
    fn summary_still_extracted_from_raw_payload() {
        let raw = make_raw(
            "slot",
            "task_dispatched",
            json!({
                "TaskDispatched": {
                    "slot_id": "slot-a",
                    "task_id": "t-1",
                    "purpose": "code-review",
                    "prompt_chars": 100,
                    "preview": "Please review the diff",
                    "cited_kb_ids": []
                }
            }),
        );
        let row = raw.into_timeline_row();
        assert_eq!(row.summary.as_deref(), Some("Please review the diff"));
    }

    #[test]
    fn parse_event_type_filter_handles_all_input_forms() {
        // Form 1: "domain::kind" — internal code style.
        assert_eq!(
            parse_event_type_filter("message::logged"),
            (Some("message".into()), Some("logged".into()))
        );

        // Form 2: v1 wire_type — frontend style.
        assert_eq!(
            parse_event_type_filter("board_task_created"),
            (Some("board".into()), Some("task_created".into()))
        );

        // Form 2 many-to-one: domain-only match.
        assert_eq!(
            parse_event_type_filter("slot_state_changed"),
            (Some("slot".into()), None)
        );
        assert_eq!(
            parse_event_type_filter("user_message"),
            (Some("message".into()), Some("logged".into()))
        );

        // Form 3: unknown — caller falls back to best-effort kind match.
        assert_eq!(
            parse_event_type_filter("totally_unknown_type"),
            (None, None)
        );
    }
}
