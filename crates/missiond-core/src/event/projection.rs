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
//! * `event_type` column = `"<domain>::<kind>"` (legible for AI consumers).
//!   The WS live stream still emits the 52-arm v1 wire_type via
//!   `daemon/src/bus/ws_bridge.rs::v2_logged_to_v1_wire_format`; catch-up
//!   here prefers the projection format. Mixing is OK because catch-up is
//!   always client-requested and the browser code handles both shapes.
//! * `summary` comes from `payload_inline.preview` / `.summary` if present,
//!   else empty.
//! * Search uses Postgres FTS on `payload_inline::text` (index added by
//!   migration `20260420100000_event_log_fts.sql`).

#![cfg(feature = "postgres")]

use std::collections::HashMap;

use sqlx::PgPool;

use crate::db::error::{DbError, DbResult};
use crate::db::shared::{LatencyStats, TimelineRow, TimelineStats};

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
        let event_type = format!("{}::{}", self.domain, self.kind);
        let (summary, payload_str) = project_payload(&self.payload_inline);
        TimelineRow {
            seq: self.seq,
            trace_id: self.trace_id.map(|u| u.to_string()),
            span_id: self.span_id.map(|u| u.to_string()),
            parent_span_id: self.parent_span_id.map(|u| u.to_string()),
            event_type,
            summary,
            payload: payload_str,
            created_at: self.ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}

/// Extract a legible `summary` from the variant-wrapped payload.
/// v2 payload shape is `{"VariantName": {...fields...}}` (externally tagged).
/// We peek into the single variant and prefer `summary` > `preview` > `title`.
fn project_payload(payload: &Option<serde_json::Value>) -> (Option<String>, String) {
    let Some(payload) = payload else {
        return (None, "null".to_string());
    };
    // Unwrap the externally-tagged variant if present.
    let inner = payload
        .as_object()
        .and_then(|obj| obj.values().next())
        .unwrap_or(payload);
    let summary = inner
        .get("summary")
        .or_else(|| inner.get("preview"))
        .or_else(|| inner.get("title"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (summary, payload.to_string())
}

/// Parse `event_type` (projection form `"domain::kind"`) back into
/// `(domain, kind)` parts for SQL filtering. Returns `None` for either side
/// if the legacy wire_type (no `::` separator) is passed — caller should
/// fall back to ILIKE match on domain or payload.
fn parse_event_type_filter(event_type: &str) -> (Option<String>, Option<String>) {
    match event_type.split_once("::") {
        Some((d, k)) => (Some(d.to_string()), Some(k.to_string())),
        None => (None, None),
    }
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
    Ok(rows.into_iter().map(RawEventLogRow::into_timeline_row).collect())
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
        conditions.push(format!("ts >= ${}", param_idx));
        param_idx += 1;
        since_val = Some(parse_since(s));
    }
    if let Some(u) = until {
        conditions.push(format!("ts <= ${}", param_idx));
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
    Ok(rows.into_iter().map(RawEventLogRow::into_timeline_row).collect())
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
             WHERE ts >= $1 AND ts <= $2
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
    Ok(rows.into_iter().map(RawEventLogRow::into_timeline_row).collect())
}

/// All events sharing a trace_id, in causal order.
pub async fn query_timeline_by_trace(
    pool: &PgPool,
    trace_id: &str,
) -> DbResult<Vec<TimelineRow>> {
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
    Ok(rows.into_iter().map(RawEventLogRow::into_timeline_row).collect())
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
        conditions.push(format!("ts >= ${}", idx));
        idx += 1;
        since_val = Some(parse_since(s));
    }
    if let Some(u) = until {
        conditions.push(format!("ts <= ${}", idx));
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
         ORDER BY seq DESC LIMIT 100".to_string()
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
    let duration_rows: Vec<(Option<i64>,)> =
        q.fetch_all(pool).await.map_err(DbError::from)?;
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
        conditions.push(format!("ts >= ${}", idx));
        idx += 1;
        since_val = Some(parse_since(s));
    }
    if let Some(u) = until {
        conditions.push(format!("ts <= ${}", idx));
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
    Ok(rows.into_iter().map(RawEventLogRow::into_timeline_row).collect())
}
