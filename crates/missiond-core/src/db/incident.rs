use rusqlite::{params, Result as SqliteResult};
use crate::types::*;
use super::MissionDB;

impl MissionDB {
    // ============ AIOps Incidents ============

    /// Insert an incident record.
    pub fn insert_incident(
        &self,
        id: &str,
        severity: &str,
        source: &str,
        title: &str,
        description: &str,
        server_id: Option<&str>,
        raw_payload: Option<&str>,
        board_task_id: Option<&str>,
        dedupe_key: &str,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO incidents (id, severity, source, title, description, server_id, raw_payload, board_task_id, dedupe_key, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![id, severity, source, title, description, server_id, raw_payload, board_task_id, dedupe_key, now],
        )?;
        Ok(())
    }

    /// Check if an incident with the same dedupe_key was recorded within the window.
    pub fn has_recent_incident(&self, dedupe_key: &str, window_secs: i64) -> SqliteResult<bool> {
        let conn = self.read_conn();
        let now = chrono::Utc::now().to_rfc3339();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM incidents
             WHERE dedupe_key = ?1
               AND julianday(?2) - julianday(created_at) < ?3 / 86400.0",
            params![dedupe_key, now, window_secs as f64],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// List recent incidents.
    pub fn list_incidents(&self, limit: i64) -> SqliteResult<Vec<IncidentRow>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, severity, source, title, description, server_id, board_task_id, dedupe_key, created_at
             FROM incidents ORDER BY created_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(IncidentRow {
                id: row.get("id")?,
                severity: row.get("severity")?,
                source: row.get("source")?,
                title: row.get("title")?,
                description: row.get("description")?,
                server_id: row.get("server_id")?,
                board_task_id: row.get("board_task_id")?,
                dedupe_key: row.get("dedupe_key")?,
                created_at: row.get("created_at")?,
            })
        })?;
        rows.collect()
    }

    // ── Token Usage Ledger ──────────────────────────────────────────

    /// Insert a token usage record into the ledger (append-only).
    pub fn insert_token_usage(
        &self,
        conversation_id: &str,
        slot_id: Option<&str>,
        slot_task_id: Option<&str>,
        model: Option<&str>,
        input_tokens: i64,
        cache_creation_tokens: i64,
        cache_read_tokens: i64,
        output_tokens: i64,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO token_usage_ledger
                (conversation_id, slot_id, slot_task_id, model,
                 input_tokens, cache_creation_tokens, cache_read_tokens, output_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                conversation_id,
                slot_id,
                slot_task_id,
                model,
                input_tokens,
                cache_creation_tokens,
                cache_read_tokens,
                output_tokens,
            ],
        )?;
        Ok(())
    }

    /// Query aggregated token stats from the ledger.
    /// Supports filtering by conversation_id, slot_id, and time range.
    /// group_by: "session" | "slot" | "model" | "day" | None (total).
    pub fn token_stats(
        &self,
        conversation_id: Option<&str>,
        slot_id: Option<&str>,
        since: Option<&str>,
        group_by: Option<&str>,
    ) -> SqliteResult<Vec<std::collections::HashMap<String, serde_json::Value>>> {
        let conn = self.read_conn();

        let group_col = match group_by {
            Some("session") => Some("conversation_id"),
            Some("slot") => Some("slot_id"),
            Some("model") => Some("model"),
            Some("day") => Some("date(created_at)"),
            _ => None,
        };

        let mut sql = String::from("SELECT ");
        if let Some(col) = group_col {
            sql.push_str(&format!("{col} AS group_key, "));
        }
        sql.push_str(
            "SUM(input_tokens) AS total_input,
             SUM(cache_creation_tokens) AS total_cache_creation,
             SUM(cache_read_tokens) AS total_cache_read,
             SUM(output_tokens) AS total_output,
             COUNT(*) AS record_count
             FROM token_usage_ledger WHERE 1=1",
        );

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(cid) = conversation_id {
            sql.push_str(&format!(" AND conversation_id = ?{idx}"));
            param_values.push(Box::new(cid.to_string()));
            idx += 1;
        }
        if let Some(sid) = slot_id {
            sql.push_str(&format!(" AND slot_id = ?{idx}"));
            param_values.push(Box::new(sid.to_string()));
            idx += 1;
        }
        if let Some(s) = since {
            sql.push_str(&format!(" AND created_at >= ?{idx}"));
            param_values.push(Box::new(s.to_string()));
            let _ = idx; // suppress unused warning
        }

        if let Some(col) = group_col {
            sql.push_str(&format!(" GROUP BY {col} ORDER BY total_output DESC"));
        }

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            let mut map = std::collections::HashMap::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json_val = match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::json!(n),
                    rusqlite::types::Value::Real(f) => serde_json::json!(f),
                    rusqlite::types::Value::Text(s) => serde_json::json!(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                };
                map.insert(name.clone(), json_val);
            }
            Ok(map)
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

}
