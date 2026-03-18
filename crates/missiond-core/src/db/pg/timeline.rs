//! TimelineStore — PostgreSQL implementation.

use std::collections::HashMap;
use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::{TimelineStore, TimelineRow, TimelineStats, LatencyStats};
use super::PgMissionStore;

/// Parse relative time strings like "10min", "1h", "24h", "7d" into timestamp strings.
/// Also normalizes ISO datetime (T separator -> space) to match DB storage format.
fn parse_relative_time(s: &str) -> String {
    let s = s.trim();
    if let Some(mins) = s.strip_suffix("min").and_then(|v| v.parse::<i64>().ok()) {
        return format!("{}", (chrono::Utc::now() - chrono::Duration::minutes(mins)).format("%Y-%m-%d %H:%M:%S"));
    }
    if let Some(hours) = s.strip_suffix('h').and_then(|v| v.parse::<i64>().ok()) {
        return format!("{}", (chrono::Utc::now() - chrono::Duration::hours(hours)).format("%Y-%m-%d %H:%M:%S"));
    }
    if let Some(days) = s.strip_suffix('d').and_then(|v| v.parse::<i64>().ok()) {
        return format!("{}", (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%d %H:%M:%S"));
    }
    // ISO datetime with T separator
    if s.contains('T') {
        return s.replace('T', " ").chars().take(19).collect();
    }
    s.to_string()
}

fn parse_since(s: &str) -> String {
    let ts = parse_relative_time(s);
    if ts.len() == 10 { format!("{} 00:00:00", ts) } else { ts }
}

fn parse_until(s: &str) -> String {
    let ts = parse_relative_time(s);
    if ts.len() == 10 { format!("{} 23:59:59", ts) } else { ts }
}

/// Helper to extract a TimelineRow from a sqlx::Row.
fn row_to_timeline(row: &sqlx::postgres::PgRow) -> TimelineRow {
    use sqlx::Row;
    TimelineRow {
        seq: row.get("seq"),
        trace_id: row.get("trace_id"),
        span_id: row.get("span_id"),
        parent_span_id: row.get("parent_span_id"),
        event_type: row.get("event_type"),
        summary: row.get("summary"),
        payload: row.get("payload"),
        created_at: row.get("created_at"),
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl TimelineStore for PgMissionStore {
    async fn insert_timeline_batch(&self, entries: &[(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)]) -> DbResult<Vec<i64>> {
        let mut seqs = Vec::with_capacity(entries.len());
        for (trace_id, span_id, parent_span_id, event_type, summary, payload) in entries {
            let (seq,): (i64,) = sqlx::query_as(
                "INSERT INTO system_timeline (trace_id, span_id, parent_span_id, event_type, summary, payload)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 RETURNING seq"
            )
            .bind(trace_id)
            .bind(span_id)
            .bind(parent_span_id)
            .bind(event_type)
            .bind(summary)
            .bind(payload)
            .fetch_one(&self.pool)
            .await?;
            seqs.push(seq);
        }
        Ok(seqs)
    }

    async fn query_timeline_since(&self, since_seq: i64, limit: usize) -> DbResult<Vec<TimelineRow>> {
        let rows = sqlx::query(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE seq > $1 ORDER BY seq ASC LIMIT $2"
        )
        .bind(since_seq)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_timeline).collect())
    }

    async fn timeline_latest_seq(&self) -> DbResult<i64> {
        let (seq,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(seq), 0) FROM system_timeline"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(seq)
    }

    async fn cleanup_timeline_ttl(&self, days: i64) -> DbResult<usize> {
        let cutoff = format!("{}", (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%d %H:%M:%S"));
        let result = sqlx::query(
            "DELETE FROM system_timeline WHERE created_at < $1"
        )
        .bind(&cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn find_timeline_needing_briefing(&self, min_content_chars: usize, limit: usize) -> DbResult<Vec<(i64, String, String, Option<String>)>> {
        // PostgreSQL uses payload::json->>'content_chars' or cast(payload::json->>'content_chars' as int)
        // to extract JSON fields instead of json_extract().
        let rows: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
            "SELECT seq, event_type, payload, summary FROM system_timeline
             WHERE event_type IN ('user_message', 'assistant_message', 'thinking_message')
               AND CAST(payload::json->>'content_chars' AS INTEGER) > $1
               AND (summary IS NULL OR summary = payload::json->>'preview')
             ORDER BY seq DESC LIMIT $2"
        )
        .bind(min_content_chars as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn update_timeline_summary(&self, seq: i64, summary: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE system_timeline SET summary = $1 WHERE seq = $2"
        )
        .bind(summary)
        .bind(seq)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn query_timeline_filtered(
        &self,
        event_type: Option<&str>,
        trace_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        // Build dynamic query with numbered params
        let mut conditions = Vec::new();
        let mut param_idx = 1u32;

        // We'll collect param values and bind them dynamically.
        // Since sqlx doesn't support fully dynamic params easily, we build the SQL
        // with all possible params and use Option binding.
        // Simpler approach: build SQL string and use separate queries per combo.
        // For correctness with dynamic params, we use a manual approach.

        let mut et_val = None;
        let mut tid_val = None;
        let mut since_val = None;
        let mut until_val = None;

        if let Some(et) = event_type {
            conditions.push(format!("event_type = ${}", param_idx));
            param_idx += 1;
            et_val = Some(et.to_string());
        }
        if let Some(tid) = trace_id {
            conditions.push(format!("trace_id = ${}", param_idx));
            param_idx += 1;
            tid_val = Some(tid.to_string());
        }
        if let Some(s) = since {
            let ts = parse_since(s);
            conditions.push(format!("created_at >= ${}", param_idx));
            param_idx += 1;
            since_val = Some(ts);
        }
        if let Some(u) = until {
            let ts = parse_until(u);
            conditions.push(format!("created_at <= ${}", param_idx));
            param_idx += 1;
            until_val = Some(ts);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline {} ORDER BY seq DESC LIMIT ${} OFFSET ${}",
            where_clause, param_idx, param_idx + 1
        );

        // Build query and bind dynamically in order
        let mut query = sqlx::query(&sql);
        if let Some(ref v) = et_val { query = query.bind(v); }
        if let Some(ref v) = tid_val { query = query.bind(v); }
        if let Some(ref v) = since_val { query = query.bind(v); }
        if let Some(ref v) = until_val { query = query.bind(v); }
        query = query.bind(limit).bind(offset);

        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.iter().map(row_to_timeline).collect())
    }

    async fn query_timeline_stratified(
        &self,
        since: &str,
        until: &str,
        per_type_limit: i64,
        type_limits: &HashMap<String, i64>,
    ) -> DbResult<Vec<TimelineRow>> {
        let ts_since = parse_since(since);
        let ts_until = parse_until(until);

        // Build per-type CASE expression for custom limits
        let limit_expr = if type_limits.is_empty() {
            format!("{}", per_type_limit)
        } else {
            let mut cases = String::from("CASE event_type ");
            for (et, lim) in type_limits {
                cases.push_str(&format!("WHEN '{}' THEN {} ", et.replace('\'', "''"), lim));
            }
            cases.push_str(&format!("ELSE {} END", per_type_limit));
            cases
        };

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM (
               SELECT *, ROW_NUMBER() OVER (PARTITION BY event_type ORDER BY seq DESC) as rn
               FROM system_timeline
               WHERE created_at >= $1 AND created_at <= $2
             ) sub
             WHERE rn <= ({})
             ORDER BY seq DESC",
            limit_expr
        );

        let rows = sqlx::query(&sql)
            .bind(&ts_since)
            .bind(&ts_until)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(row_to_timeline).collect())
    }

    async fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>> {
        let rows = sqlx::query(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE trace_id = $1 ORDER BY seq ASC"
        )
        .bind(trace_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_timeline).collect())
    }

    async fn query_timeline_stats(&self, since: Option<&str>, until: Option<&str>) -> DbResult<TimelineStats> {
        // Build dynamic WHERE clause
        let mut conditions = Vec::new();
        let mut param_idx = 1u32;
        let mut since_val = None;
        let mut until_val = None;

        if let Some(s) = since {
            let ts = parse_since(s);
            conditions.push(format!("created_at >= ${}", param_idx));
            param_idx += 1;
            since_val = Some(ts);
        }
        if let Some(u) = until {
            let ts = parse_until(u);
            conditions.push(format!("created_at <= ${}", param_idx));
            // param_idx += 1; // not needed after last use
            until_val = Some(ts);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Total events
        let total_sql = format!("SELECT COUNT(*) FROM system_timeline {}", where_clause);
        let mut q = sqlx::query_as::<_, (i64,)>(&total_sql);
        if let Some(ref v) = since_val { q = q.bind(v); }
        if let Some(ref v) = until_val { q = q.bind(v); }
        let (total_events,) = q.fetch_one(&self.pool).await?;

        // By type
        let by_type_sql = format!(
            "SELECT event_type, COUNT(*) FROM system_timeline {} GROUP BY event_type ORDER BY COUNT(*) DESC",
            where_clause
        );
        let mut q = sqlx::query_as::<_, (String, i64)>(&by_type_sql);
        if let Some(ref v) = since_val { q = q.bind(v); }
        if let Some(ref v) = until_val { q = q.bind(v); }
        let by_type: Vec<(String, i64)> = q.fetch_all(&self.pool).await?;

        // Traced events
        let traced_where = if where_clause.is_empty() {
            "WHERE trace_id IS NOT NULL AND trace_id != ''".to_string()
        } else {
            format!("{} AND trace_id IS NOT NULL AND trace_id != ''", where_clause)
        };
        let traced_sql = format!(
            "SELECT COUNT(*), COUNT(DISTINCT trace_id) FROM system_timeline {}",
            traced_where
        );
        let mut q = sqlx::query_as::<_, (i64, i64)>(&traced_sql);
        if let Some(ref v) = since_val { q = q.bind(v); }
        if let Some(ref v) = until_val { q = q.bind(v); }
        let (traced_events, unique_traces) = q.fetch_one(&self.pool).await?;

        // Gemini latency from payload JSON
        let gemini_sql = if where_clause.is_empty() {
            "SELECT CAST(payload::json->>'duration_ms' AS BIGINT) FROM system_timeline WHERE event_type = 'gemini_request_completed' ORDER BY seq DESC LIMIT 100".to_string()
        } else {
            format!(
                "SELECT CAST(payload::json->>'duration_ms' AS BIGINT) FROM system_timeline {} AND event_type = 'gemini_request_completed' ORDER BY seq DESC LIMIT 100",
                where_clause
            )
        };
        let mut q = sqlx::query_as::<_, (Option<i64>,)>(&gemini_sql);
        if let Some(ref v) = since_val { q = q.bind(v); }
        if let Some(ref v) = until_val { q = q.bind(v); }
        let duration_rows: Vec<(Option<i64>,)> = q.fetch_all(&self.pool).await?;
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

        Ok(TimelineStats { total_events, by_type, traced_events, unique_traces, gemini_latency })
    }

    async fn query_timeline_search(
        &self,
        keyword: &str,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        // PostgreSQL: use ILIKE for keyword search (no FTS5 equivalent without tsvector setup).
        // This is the simple LIKE fallback approach, matching the SQLite non-FTS path.
        let like_pattern = format!("%{}%", keyword);
        let mut conditions = vec![
            format!("(summary ILIKE ${} OR payload ILIKE ${})", 1, 2),
        ];
        let mut param_idx = 3u32;
        let mut since_val = None;
        let mut until_val = None;

        if let Some(s) = since {
            let ts = parse_since(s);
            conditions.push(format!("created_at >= ${}", param_idx));
            param_idx += 1;
            since_val = Some(ts);
        }
        if let Some(u) = until {
            let ts = parse_until(u);
            conditions.push(format!("created_at <= ${}", param_idx));
            param_idx += 1;
            until_val = Some(ts);
        }

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE {} ORDER BY seq DESC LIMIT ${}",
            conditions.join(" AND "), param_idx
        );

        let mut query = sqlx::query(&sql);
        query = query.bind(&like_pattern).bind(&like_pattern);
        if let Some(ref v) = since_val { query = query.bind(v); }
        if let Some(ref v) = until_val { query = query.bind(v); }
        query = query.bind(limit);

        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.iter().map(row_to_timeline).collect())
    }

    async fn get_briefing_summaries_for_session(&self, session_id: &str) -> DbResult<HashMap<i64, String>> {
        // PostgreSQL JSON extraction: payload::json->>'session_id', payload::json->>'message_id', payload::json->>'preview'
        let rows = sqlx::query(
            "SELECT CAST(payload::json->>'message_id' AS BIGINT), summary
             FROM system_timeline
             WHERE event_type IN ('user_message', 'assistant_message', 'thinking_message')
               AND payload::json->>'session_id' = $1
               AND summary IS NOT NULL
               AND summary != payload::json->>'preview'"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut map = HashMap::new();
        for row in &rows {
            use sqlx::Row;
            let msg_id: Option<i64> = row.get(0);
            let summary: String = row.get(1);
            if let Some(id) = msg_id {
                map.insert(id, summary);
            }
        }
        Ok(map)
    }
}
