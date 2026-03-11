//! System Timeline database operations.

use super::MissionDB;
use super::error::DbResult;

/// A row from the system_timeline table.
#[derive(Debug, Clone)]
pub struct TimelineRow {
    pub seq: i64,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub event_type: String,
    pub summary: Option<String>,
    pub payload: String,
    pub created_at: String,
}

/// Timeline statistics returned by query_timeline_stats.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TimelineStats {
    pub total_events: i64,
    pub by_type: Vec<(String, i64)>,
    pub traced_events: i64,
    pub unique_traces: i64,
    pub gemini_latency: Option<LatencyStats>,
}

/// Latency percentile statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatencyStats {
    pub count: i64,
    pub avg_ms: i64,
    pub p50_ms: i64,
    pub p90_ms: i64,
    pub p99_ms: i64,
}

impl MissionDB {
    // ── System Timeline (Phase 6) ──

    /// Batch-insert timeline entries in a single transaction.
    /// Returns the seq (AUTOINCREMENT rowid) for each entry.
    pub fn insert_timeline_batch(
        &self,
        entries: &[(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)],
    ) -> DbResult<Vec<i64>> {
        let conn = self.conn();
        conn.execute_batch("BEGIN")?;
        let mut seqs = Vec::with_capacity(entries.len());
        for (trace_id, span_id, parent_span_id, event_type, summary, payload) in entries {
            conn.execute(
                "INSERT INTO system_timeline (trace_id, span_id, parent_span_id, event_type, summary, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![trace_id, span_id, parent_span_id, event_type, summary, payload],
            )?;
            seqs.push(conn.last_insert_rowid());
        }
        conn.execute_batch("COMMIT")?;
        Ok(seqs)
    }

    /// Query timeline events after a given seq (for WS catch-up).
    pub fn query_timeline_since(&self, since_seq: i64, limit: usize) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![since_seq, limit as i64], |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Get the latest seq in the timeline (for connected message).
    pub fn timeline_latest_seq(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        let seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM system_timeline",
            [],
            |row| row.get(0),
        )?;
        Ok(seq)
    }

    /// Delete timeline events older than N days. Returns count deleted.
    pub fn cleanup_timeline_ttl(&self, days: i64) -> DbResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM system_timeline WHERE created_at < datetime('now', ?1)",
            rusqlite::params![format!("-{} days", days)],
        )?;
        Ok(deleted)
    }

    /// Find timeline entries that need semantic summary generation.
    /// Returns (seq, event_type, payload, summary) for entries where summary is
    /// the mechanical preview (from message_handler) and content is long enough
    /// to benefit from semantic summarization.
    pub fn find_timeline_needing_briefing(&self, min_content_chars: usize, limit: usize) -> DbResult<Vec<(i64, String, String, Option<String>)>> {
        let conn = self.read_conn();
        // Target: conversation messages (user/assistant/thinking) with long content
        // that still have the mechanical preview (or NULL summary).
        // Use payload.content_chars to filter long messages.
        let mut stmt = conn.prepare(
            "SELECT seq, event_type, payload, summary FROM system_timeline
             WHERE event_type IN ('user_message', 'assistant_message', 'thinking_message')
               AND json_extract(payload, '$.content_chars') > ?1
               AND (summary IS NULL OR summary = json_extract(payload, '$.preview'))
             ORDER BY seq DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![min_content_chars as i64, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Update the summary column for a timeline entry (used by briefing worker).
    pub fn update_timeline_summary(&self, seq: i64, summary: &str) -> DbResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE system_timeline SET summary = ?1 WHERE seq = ?2",
            rusqlite::params![summary, seq],
        )?;
        Ok(updated > 0)
    }

    // ── L3: Timeline Query Methods ──

    /// Filtered timeline query with optional event_type, trace_id, time range, and pagination.
    pub fn query_timeline_filtered(
        &self,
        event_type: Option<&str>,
        trace_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(et) = event_type {
            conditions.push(format!("event_type = ?{}", params.len() + 1));
            params.push(Box::new(et.to_string()));
        }
        if let Some(tid) = trace_id {
            conditions.push(format!("trace_id = ?{}", params.len() + 1));
            params.push(Box::new(tid.to_string()));
        }
        if let Some(s) = since {
            let ts = Self::parse_relative_time(s);
            conditions.push(format!("created_at >= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        if let Some(u) = until {
            let ts = Self::parse_relative_time(u);
            conditions.push(format!("created_at <= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline {} ORDER BY seq DESC LIMIT ?{} OFFSET ?{}",
            where_clause, params.len() + 1, params.len() + 2
        );
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Stratified timeline query: returns up to `per_type_limit` events per event_type
    /// within the time range, using a single SQL query with window functions.
    /// Custom per-type limits can override the default (e.g., high-volume types get fewer).
    pub fn query_timeline_stratified(
        &self,
        since: &str,
        until: &str,
        per_type_limit: i64,
        type_limits: &std::collections::HashMap<String, i64>,
    ) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let ts_since = Self::parse_relative_time(since);
        let ts_until = Self::parse_relative_time(until);

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
               WHERE created_at >= ?1 AND created_at <= ?2
             )
             WHERE rn <= ({})
             ORDER BY seq DESC",
            limit_expr
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![ts_since, ts_until], |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Query all events belonging to a single trace (causal chain).
    pub fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE trace_id = ?1 ORDER BY seq ASC"
        )?;
        let rows = stmt.query_map(rusqlite::params![trace_id], |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Timeline statistics: event count by type, trace stats, Gemini latency distribution.
    pub fn query_timeline_stats(&self, since: Option<&str>, until: Option<&str>) -> DbResult<TimelineStats> {
        let conn = self.read_conn();

        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(s) = since {
            let ts = Self::parse_relative_time(s);
            conditions.push(format!("created_at >= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        if let Some(u) = until {
            let ts = Self::parse_relative_time(u);
            conditions.push(format!("created_at <= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        // Total events
        let total_sql = format!("SELECT COUNT(*) FROM system_timeline {}", where_clause);
        let total_events: i64 = conn.query_row(&total_sql, param_refs.as_slice(), |r| r.get(0))?;

        // By type
        let by_type_sql = format!(
            "SELECT event_type, COUNT(*) FROM system_timeline {} GROUP BY event_type ORDER BY COUNT(*) DESC",
            where_clause
        );
        let mut stmt = conn.prepare(&by_type_sql)?;
        let by_type: Vec<(String, i64)> = stmt.query_map(param_refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?.collect::<Result<Vec<_>, _>>()?;

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
        let (traced_events, unique_traces): (i64, i64) = conn.query_row(
            &traced_sql, param_refs.as_slice(), |r| Ok((r.get(0)?, r.get(1)?))
        )?;

        // Gemini latency from payload JSON
        let gemini_sql = if where_clause.is_empty() {
            "SELECT json_extract(payload, '$.duration_ms') FROM system_timeline WHERE event_type = 'gemini_request_completed' ORDER BY seq DESC LIMIT 100".to_string()
        } else {
            format!(
                "SELECT json_extract(payload, '$.duration_ms') FROM system_timeline {} AND event_type = 'gemini_request_completed' ORDER BY seq DESC LIMIT 100",
                where_clause
            )
        };
        let mut stmt = conn.prepare(&gemini_sql)?;
        let durations: Vec<i64> = stmt.query_map(param_refs.as_slice(), |row| {
            row.get::<_, Option<i64>>(0)
        })?.filter_map(|r| r.ok().flatten()).collect();

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

    /// Search timeline by keyword in summary and payload.
    pub fn query_timeline_search(
        &self,
        keyword: &str,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let like_pattern = format!("%{}%", keyword);
        let mut conditions = vec![
            format!("(summary LIKE ?{} OR payload LIKE ?{})", 1, 2),
        ];
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(like_pattern.clone()),
            Box::new(like_pattern),
        ];

        if let Some(s) = since {
            let ts = Self::parse_relative_time(s);
            conditions.push(format!("created_at >= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        if let Some(u) = until {
            let ts = Self::parse_relative_time(u);
            conditions.push(format!("created_at <= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE {} ORDER BY seq DESC LIMIT ?{}",
            conditions.join(" AND "), params.len() + 1
        );
        params.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Parse relative time strings like "10min", "1h", "24h", "7d" into SQLite datetime expressions.
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
        // Assume ISO datetime, pass through
        s.to_string()
    }

    /// Fetch briefing summaries for a session's conversation messages.
    /// Returns a map of message_id → summary from system_timeline where the
    /// summary differs from the raw preview (i.e., a semantic summary exists).
    pub fn get_briefing_summaries_for_session(&self, session_id: &str) -> DbResult<std::collections::HashMap<i64, String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT json_extract(payload, '$.message_id'), summary
             FROM system_timeline
             WHERE event_type IN ('user_message', 'assistant_message', 'thinking_message')
               AND json_extract(payload, '$.session_id') = ?1
               AND summary IS NOT NULL
               AND summary != json_extract(payload, '$.preview')"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map(rusqlite::params![session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            if let Ok((msg_id, summary)) = row {
                map.insert(msg_id, summary);
            }
        }
        Ok(map)
    }
}
