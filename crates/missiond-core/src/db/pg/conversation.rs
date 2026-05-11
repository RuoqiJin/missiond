//! ConversationStore — PostgreSQL implementation (unified per memory pillar v0.4.23).
//!
//! Covers the full ConversationStore surface for PG backend:
//!   - session + message lifecycle, embeddings, extraction
//!   - tool calls (merged from former ToolCallStore)
//!   - conversation events (merged from former EventStore)
//!   - retrospective results + narration (merged from former RetrospectiveStore)
//!
//! Note: Rust coherence requires a single `impl ConversationStore for PgMissionStore`
//! block per crate. All methods live here.

use super::PgMissionStore;
use crate::db::error::DbResult;
use crate::db::shared::MessageRoleBackfillCandidate;
use crate::db::traits::ConversationStore;
use crate::types::*;
use async_trait::async_trait;
use sqlx::Row;
use std::collections::HashSet;

const COMPACTION_FRAGMENTS_QUERY: &str =
    "SELECT id, COALESCE(started_at, '') AS started_at, COALESCE(message_count, 0) AS message_count FROM conversations
             WHERE parent_session_id = $1 AND conversation_type = 'compaction'
             ORDER BY started_at ASC NULLS LAST, id ASC";

fn task_scoped_type_clause(conv_type: Option<&str>, type_clause: &str) -> String {
    match conv_type {
        None | Some("all") => String::new(),
        _ => type_clause.to_string(),
    }
}

fn task_scoped_order_clause() -> &'static str {
    "ORDER BY CASE WHEN task_id = $1 THEN 0 ELSE 1 END, started_at DESC"
}

/// Zero-allocation pgvector literal serializer.
/// Pre-allocates a single String for 512-dim halfvec (~7KB) instead of 513 heap allocs.
fn vec_to_pg_literal(v: &[f32]) -> String {
    use std::fmt::Write;
    let mut buf = String::with_capacity(v.len() * 14 + 2);
    buf.push('[');
    for (i, f) in v.iter().enumerate() {
        if i > 0 {
            buf.push(',');
        }
        let _ = write!(buf, "{}", f);
    }
    buf.push(']');
    buf
}

#[cfg(feature = "postgres")]
impl PgMissionStore {
    /// Extract a Conversation from a sqlx::PgRow.
    fn row_to_conversation(row: &sqlx::postgres::PgRow) -> Conversation {
        use sqlx::Row;
        Conversation {
            id: row.get("id"),
            project: row.get("project"),
            project_id: row.try_get("project_id").unwrap_or(None),
            slot_id: row.get("slot_id"),
            source: row.get("source"),
            model: row.get("model"),
            git_branch: row.get("git_branch"),
            jsonl_path: row.get("jsonl_path"),
            parent_session_id: row.get("parent_session_id"),
            task_id: row.get("task_id"),
            message_count: row.get("message_count"),
            started_at: row.get("started_at"),
            ended_at: row.get("ended_at"),
            status: row.get("status"),
            analyzed_at: row.get("analyzed_at"),
            analysis_version: row.try_get("analysis_version").unwrap_or(0),
            analysis_retries: row.try_get("analysis_retries").unwrap_or(0),
            deep_analyzed_message_id: row.try_get("deep_analyzed_message_id").unwrap_or(0),
            chat_type: row.get("chat_type"),
            conversation_type: row
                .try_get("conversation_type")
                .unwrap_or_else(|_| "user".to_string()),
            updated_at: row.get("updated_at"),
            llm_summary: row.get("llm_summary"),
            embedding_provider: row.get("embedding_provider"),
            session_timeline: row.get("session_timeline"),
            timeline_built_at: row.get("timeline_built_at"),
        }
    }

    /// Extract a ConversationMessage from a sqlx::PgRow.
    pub(super) fn row_to_conversation_message(row: &sqlx::postgres::PgRow) -> ConversationMessage {
        use sqlx::Row;
        ConversationMessage {
            id: row.get("id"),
            session_id: row.get("session_id"),
            role: row.get("role"),
            content: row.get("content"),
            raw_content: row.get("raw_content"),
            message_uuid: row.get("message_uuid"),
            parent_uuid: row.get("parent_uuid"),
            model: row.get("model"),
            timestamp: row
                .get::<chrono::DateTime<chrono::Utc>, _>("timestamp")
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            metadata: row.get("metadata"),
            tool_name: row.get("tool_name"),
            raw_role: row.get("raw_role"),
            content_types: row.get("content_types"),
            has_image: row.get::<bool, _>("has_image"),
            has_tool_use: row.get::<bool, _>("has_tool_use"),
            has_tool_result: row.get::<bool, _>("has_tool_result"),
            token_count: row.get("token_count"),
            seq: None,
            role_display: None,
        }
    }

    /// Extract a ConversationMessage with seq (ROW_NUMBER) from a PgRow.
    pub(super) fn row_to_enriched_message(row: &sqlx::postgres::PgRow) -> ConversationMessage {
        use sqlx::Row;
        let mut msg = Self::row_to_conversation_message(row);
        msg.seq = row.try_get::<i64, _>("seq").ok();
        let has_tool_use = msg.has_tool_use;
        msg.role_display = Some(crate::types::role_display(&msg.role, has_tool_use).to_string());
        msg
    }

    /// Parse relative time strings (e.g. "30min", "2h", "7d") to absolute timestamps.
    fn parse_relative_time(s: &str) -> String {
        let s = s.trim();
        if let Some(mins) = s.strip_suffix("min").and_then(|v| v.parse::<i64>().ok()) {
            return format!(
                "{}",
                (chrono::Utc::now() - chrono::Duration::minutes(mins)).format("%Y-%m-%dT%H:%M:%S")
            );
        }
        if let Some(hours) = s.strip_suffix('h').and_then(|v| v.parse::<i64>().ok()) {
            return format!(
                "{}",
                (chrono::Utc::now() - chrono::Duration::hours(hours)).format("%Y-%m-%dT%H:%M:%S")
            );
        }
        if let Some(days) = s.strip_suffix('d').and_then(|v| v.parse::<i64>().ok()) {
            return format!(
                "{}",
                (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%dT%H:%M:%S")
            );
        }
        if s.contains('T') {
            return s.replace('T', " ").chars().take(19).collect();
        }
        s.to_string()
    }

    fn parse_since(s: &str) -> String {
        let ts = Self::parse_relative_time(s);
        if ts.len() == 10 {
            format!("{}T00:00:00", ts)
        } else {
            ts.replace(' ', "T")
        }
    }

    fn parse_until(s: &str) -> String {
        let ts = Self::parse_relative_time(s);
        if ts.len() == 10 {
            format!("{}T23:59:59", ts)
        } else {
            ts.replace(' ', "T")
        }
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl ConversationStore for PgMissionStore {
    // -- session CRUD --

    async fn upsert_conversation(&self, conv: &Conversation) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO conversations (id, project, slot_id, source, model, git_branch, jsonl_path, parent_session_id, task_id, message_count, started_at, ended_at, status, analyzed_at, conversation_type, project_id, chat_type)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
             ON CONFLICT (id) DO UPDATE SET
                slot_id = COALESCE($3, conversations.slot_id),
                source = $4,
                model = COALESCE($5, conversations.model),
                git_branch = COALESCE($6, conversations.git_branch),
                message_count = $10,
                started_at = CASE WHEN $11 != '' AND $11 != 'unknown' THEN $11 ELSE conversations.started_at END,
                ended_at = $12,
                status = $13,
                parent_session_id = COALESCE(conversations.parent_session_id, $8),
                conversation_type = COALESCE($15, conversations.conversation_type),
                project_id = COALESCE($16, conversations.project_id),
                chat_type = COALESCE($17, conversations.chat_type)"
        )
        .bind(&conv.id)
        .bind(&conv.project)
        .bind(&conv.slot_id)
        .bind(&conv.source)
        .bind(&conv.model)
        .bind(&conv.git_branch)
        .bind(&conv.jsonl_path)
        .bind(&conv.parent_session_id)
        .bind(&conv.task_id)
        .bind(conv.message_count)
        .bind(&conv.started_at)
        .bind(&conv.ended_at)
        .bind(&conv.status)
        .bind(&conv.analyzed_at)
        .bind(&conv.conversation_type)
        .bind(&conv.project_id)
        .bind(&conv.chat_type)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_conversation_exists(
        &self,
        session_id: &str,
        project_path: &str,
        jsonl_path: &str,
        status: &str,
        conversation_type: &str,
        parent_session_id: Option<&str>,
        started_at: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO conversations (id, project, source, jsonl_path, message_count, started_at, status, conversation_type, parent_session_id)
             VALUES ($1, $2, 'claude_code', $3, 0, COALESCE($7::timestamptz, NOW()), $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET
                source = 'claude_code',
                parent_session_id = COALESCE(conversations.parent_session_id, EXCLUDED.parent_session_id)"
        )
            .bind(session_id)
            .bind(project_path)
            .bind(jsonl_path)
            .bind(status)
            .bind(conversation_type)
            .bind(parent_session_id)
            .bind(started_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn refresh_conversation_message_count(&self, session_id: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1) WHERE id = $1"
        )
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn upsert_conversation_source_state(
        &self,
        state: &ConversationSourceStateInput,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO conversation_source_state (
                conversation_id, source, raw_path, raw_state, raw_first_seen_at,
                raw_last_seen_at, raw_line_count, raw_message_line_count,
                raw_hash, reason, updated_at
             )
             VALUES ($1, $2, $3, $4, $5::timestamptz, $6::timestamptz, $7, $8, $9, $10, NOW())
             ON CONFLICT (conversation_id) DO UPDATE SET
                source = EXCLUDED.source,
                raw_path = EXCLUDED.raw_path,
                raw_state = EXCLUDED.raw_state,
                raw_first_seen_at = COALESCE(EXCLUDED.raw_first_seen_at, conversation_source_state.raw_first_seen_at),
                raw_last_seen_at = COALESCE(EXCLUDED.raw_last_seen_at, conversation_source_state.raw_last_seen_at),
                raw_line_count = COALESCE(EXCLUDED.raw_line_count, conversation_source_state.raw_line_count),
                raw_message_line_count = COALESCE(EXCLUDED.raw_message_line_count, conversation_source_state.raw_message_line_count),
                raw_hash = COALESCE(EXCLUDED.raw_hash, conversation_source_state.raw_hash),
                reason = EXCLUDED.reason,
                updated_at = NOW()",
        )
        .bind(&state.conversation_id)
        .bind(&state.source)
        .bind(&state.raw_path)
        .bind(&state.raw_state)
        .bind(&state.raw_first_seen_at)
        .bind(&state.raw_last_seen_at)
        .bind(state.raw_line_count)
        .bind(state.raw_message_line_count)
        .bind(&state.raw_hash)
        .bind(&state.reason)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_conversation(&self, id: &str) -> DbResult<Option<Conversation>> {
        let row = sqlx::query("SELECT * FROM conversations WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(Self::row_to_conversation))
    }

    async fn list_conversations(
        &self,
        status: Option<&str>,
        limit: i64,
        conv_type: Option<&str>,
        task_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        source: Option<&str>,
    ) -> DbResult<Vec<Conversation>> {
        let source_clause = match source {
            Some(s) if s.starts_with('!') => {
                // Exclusion: "!gemini_cli,!router_chat" → NOT IN ('gemini_cli','router_chat')
                let excluded: Vec<String> = s
                    .split(',')
                    .map(|v| v.trim_start_matches('!').replace('\'', "''"))
                    .filter(|v| !v.is_empty())
                    .map(|v| format!("'{}'", v))
                    .collect();
                if excluded.is_empty() {
                    String::new()
                } else {
                    format!(" AND source NOT IN ({})", excluded.join(","))
                }
            }
            Some(s) if s.contains(',') => {
                // Multi-inclusion: "claude_cli,pty_jsonl" → IN ('claude_cli','pty_jsonl')
                let included: Vec<String> = s
                    .split(',')
                    .map(|v| format!("'{}'", v.replace('\'', "''")))
                    .collect();
                format!(" AND source IN ({})", included.join(","))
            }
            Some(s) => format!(" AND source = '{}'", s.replace('\'', "''")),
            None => String::new(),
        };
        let type_clause = match conv_type {
            Some("all") => String::new(),
            Some("user") => " AND conversation_type = 'user'".to_string(),
            Some("meta") => " AND conversation_type = 'meta'".to_string(),
            Some("worker") => " AND conversation_type = 'worker'".to_string(),
            Some("jarvis") => " AND conversation_type = 'jarvis'".to_string(),
            Some("subagent") => " AND conversation_type = 'subagent'".to_string(),
            Some("compaction") => " AND conversation_type = 'compaction'".to_string(),
            Some("system") => " AND conversation_type IN ('meta', 'worker')".to_string(),
            Some("gemini") => " AND conversation_type = 'gemini_chat'".to_string(),
            _ => " AND conversation_type IN ('user', 'worker')".to_string(),
        };

        let time_clause = {
            let mut parts = Vec::new();
            if let Some(s) = since {
                let ts = Self::parse_since(s);
                parts.push(format!("started_at >= '{}'", ts.replace('\'', "''")));
            }
            if let Some(u) = until {
                let ts = Self::parse_until(u);
                parts.push(format!("started_at <= '{}'", ts.replace('\'', "''")));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!(" AND {}", parts.join(" AND "))
            }
        };

        // Fast path: filter by task_id
        if let Some(tid) = task_id {
            let task_type_clause = task_scoped_type_clause(conv_type, &type_clause);
            let sql = format!(
                "SELECT * FROM conversations WHERE (task_id = $1 OR id IN (
                    SELECT DISTINCT m.session_id
                    FROM conversation_messages m
                    WHERE m.content ILIKE ('%' || $1 || '%')
                    LIMIT 50
                )){}{}{} {} LIMIT $2",
                task_type_clause,
                source_clause,
                time_clause,
                task_scoped_order_clause()
            );
            let rows = sqlx::query(&sql)
                .bind(tid)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
            return Ok(rows.iter().map(Self::row_to_conversation).collect());
        }

        // Time range path
        if since.is_some() || until.is_some() {
            let status_clause = if let Some(s) = status {
                format!("status = '{}'", s.replace('\'', "''"))
            } else {
                "1=1".to_string()
            };
            let sql = format!(
                "SELECT * FROM conversations WHERE {}{}{}{} ORDER BY started_at DESC LIMIT $1",
                status_clause, type_clause, source_clause, time_clause
            );
            let rows = sqlx::query(&sql).bind(limit).fetch_all(&self.pool).await?;
            return Ok(rows.iter().map(Self::row_to_conversation).collect());
        }

        if let Some(s) = status {
            let sql = format!(
                "SELECT * FROM conversations WHERE status = $1{}{} ORDER BY started_at DESC LIMIT $2",
                type_clause, source_clause
            );
            let rows = sqlx::query(&sql)
                .bind(s)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
            Ok(rows.iter().map(Self::row_to_conversation).collect())
        } else {
            // Active first, then others
            let sql_active = format!(
                "SELECT * FROM conversations WHERE status = 'active'{}{} ORDER BY started_at DESC",
                type_clause, source_clause
            );
            let active_rows = sqlx::query(&sql_active).fetch_all(&self.pool).await?;
            let mut convs: Vec<Conversation> =
                active_rows.iter().map(Self::row_to_conversation).collect();

            let remaining = limit - convs.len() as i64;
            if remaining > 0 {
                let sql_rest = format!(
                    "SELECT * FROM conversations WHERE status != 'active'{}{} ORDER BY started_at DESC LIMIT $1",
                    type_clause, source_clause
                );
                let rest_rows = sqlx::query(&sql_rest)
                    .bind(remaining)
                    .fetch_all(&self.pool)
                    .await?;
                convs.extend(rest_rows.iter().map(Self::row_to_conversation));
            }

            convs.sort_by(|a, b| {
                let a_active = a.status == "active";
                let b_active = b.status == "active";
                match (a_active, b_active) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.started_at.cmp(&a.started_at),
                }
            });
            Ok(convs)
        }
    }

    async fn get_child_conversations(
        &self,
        parent_session_id: &str,
    ) -> DbResult<Vec<Conversation>> {
        let rows = sqlx::query(
            "SELECT * FROM conversations WHERE parent_session_id = $1 ORDER BY started_at ASC",
        )
        .bind(parent_session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    async fn fix_orphan_parent_links(&self, session_ids: &[String]) -> DbResult<usize> {
        if session_ids.is_empty() {
            return Ok(0);
        }
        // Batch SQL: extract parent_session_id from jsonl_path using regexp, single query
        let result = sqlx::query(
            "UPDATE conversations
             SET parent_session_id = (regexp_match(jsonl_path, '/([^/]+)/subagents/[^/]+\\.jsonl$'))[1]
             WHERE id = ANY($1)
               AND parent_session_id IS NULL
               AND jsonl_path IS NOT NULL
               AND jsonl_path ~ '/[^/]+/subagents/[^/]+\\.jsonl$'"
        )
            .bind(session_ids)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn link_compaction_fragment(
        &self,
        fragment_id: &str,
        original_id: &str,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE conversations SET parent_session_id = $1
             WHERE id = $2 AND parent_session_id IS NULL AND conversation_type = 'compaction'",
        )
        .bind(original_id)
        .bind(fragment_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    // -- deep analysis tracking --

    async fn get_pending_deep_analysis(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<Vec<Conversation>> {
        let rows = sqlx::query(
            "SELECT * FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND (slot_id IS NULL OR (
                   slot_id NOT LIKE 'slot-memory%'
                   AND slot_id NOT LIKE 'slot-diagnosis%'
                   AND slot_id NOT LIKE 'agent-%'
               ))
               AND ended_at < (NOW() - INTERVAL '5 minutes')::text
               AND analysis_retries < $1
               AND (analyzed_at IS NULL OR analysis_version < $2)

             UNION ALL

             SELECT * FROM conversations
             WHERE status = 'compacted'
               AND conversation_type = 'user'
               AND (slot_id IS NULL OR (
                   slot_id NOT LIKE 'slot-memory%'
                   AND slot_id NOT LIKE 'slot-diagnosis%'
                   AND slot_id NOT LIKE 'agent-%'
               ))
               AND analysis_retries < $1
               AND (analyzed_at IS NULL OR analysis_version < $2)

             UNION ALL

             SELECT * FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND (slot_id IS NULL OR (
                   slot_id NOT LIKE 'slot-memory%'
                   AND slot_id NOT LIKE 'slot-diagnosis%'
                   AND slot_id NOT LIKE 'agent-%'
               ))
               AND analysis_retries < $1
               AND EXISTS (
                   SELECT 1 FROM conversation_messages m
                   WHERE m.session_id = conversations.id
                     AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                     AND m.role IN ('user', 'assistant')
                   ORDER BY m.id ASC
                   OFFSET 99
                   LIMIT 1
               )

             ORDER BY started_at ASC",
        )
        .bind(max_retries)
        .bind(current_version)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    async fn has_pending_deep_analysis(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<bool> {
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(
                SELECT 1 FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND (slot_id IS NULL OR (
                      slot_id NOT LIKE 'slot-memory%'
                      AND slot_id NOT LIKE 'slot-diagnosis%'
                      AND slot_id NOT LIKE 'agent-%'
                  ))
                  AND ended_at < (NOW() - INTERVAL '5 minutes')::text
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT 1 FROM conversations
                WHERE status = 'compacted'
                  AND conversation_type = 'user'
                  AND (slot_id IS NULL OR (
                      slot_id NOT LIKE 'slot-memory%'
                      AND slot_id NOT LIKE 'slot-diagnosis%'
                      AND slot_id NOT LIKE 'agent-%'
                  ))
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT 1 FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND (slot_id IS NULL OR (
                      slot_id NOT LIKE 'slot-memory%'
                      AND slot_id NOT LIKE 'slot-diagnosis%'
                      AND slot_id NOT LIKE 'agent-%'
                  ))
                  AND analysis_retries < $1
                  AND EXISTS (
                      SELECT 1 FROM conversation_messages m
                      WHERE m.session_id = conversations.id
                        AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                        AND m.role IN ('user', 'assistant')
                      ORDER BY m.id ASC
                      OFFSET 99
                      LIMIT 1
                  )
            )",
        )
        .bind(max_retries)
        .bind(current_version)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    async fn count_pending_deep_analysis(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM (
                SELECT id FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND (slot_id IS NULL OR (
                      slot_id NOT LIKE 'slot-memory%'
                      AND slot_id NOT LIKE 'slot-diagnosis%'
                      AND slot_id NOT LIKE 'agent-%'
                  ))
                  AND ended_at < (NOW() - INTERVAL '5 minutes')::text
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT id FROM conversations
                WHERE status = 'compacted'
                  AND conversation_type = 'user'
                  AND (slot_id IS NULL OR (
                      slot_id NOT LIKE 'slot-memory%'
                      AND slot_id NOT LIKE 'slot-diagnosis%'
                      AND slot_id NOT LIKE 'agent-%'
                  ))
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT id FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND (slot_id IS NULL OR (
                      slot_id NOT LIKE 'slot-memory%'
                      AND slot_id NOT LIKE 'slot-diagnosis%'
                      AND slot_id NOT LIKE 'agent-%'
                  ))
                  AND analysis_retries < $1
                  AND EXISTS (
                      SELECT 1 FROM conversation_messages m
                      WHERE m.session_id = conversations.id
                        AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                        AND m.role IN ('user', 'assistant')
                      ORDER BY m.id ASC
                      OFFSET 99
                      LIMIT 1
                  )
            ) sub",
        )
        .bind(max_retries)
        .bind(current_version)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn count_pending_realtime(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations c
             WHERE c.conversation_type = 'user'
               AND (c.slot_id IS NULL OR (
                   c.slot_id NOT LIKE 'slot-memory%'
                   AND c.slot_id NOT LIKE 'slot-diagnosis%'
                   AND c.slot_id NOT LIKE 'agent-%'
               ))
               AND EXISTS (
                   SELECT 1
                   FROM conversation_messages m
                   WHERE m.session_id = c.id
                     AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)::timestamptz
                     AND m.role IN ('user', 'assistant')
                   LIMIT 1
               )",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn pending_realtime_detail(&self) -> DbResult<Vec<(String, i64, String)>> {
        let rows: Vec<(String, i64, String)> = sqlx::query_as(
            "WITH candidate AS MATERIALIZED (
                 SELECT m.session_id, m.timestamp
                 FROM conversation_messages m
                 JOIN conversations c ON c.id = m.session_id
                 WHERE c.conversation_type = 'user'
                   AND (c.slot_id IS NULL OR (
                       c.slot_id NOT LIKE 'slot-memory%'
                       AND c.slot_id NOT LIKE 'slot-diagnosis%'
                       AND c.slot_id NOT LIKE 'agent-%'
                   ))
                   AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)::timestamptz
                   AND m.role IN ('user', 'assistant')
                 ORDER BY m.timestamp ASC
                 LIMIT 2000
             )
             SELECT session_id, COUNT(*)::bigint as cnt, MIN(timestamp)::text as oldest
             FROM candidate
             GROUP BY session_id
             ORDER BY cnt DESC
             LIMIT 20",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn pending_deep_detail(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<Vec<(String, String, i32)>> {
        let rows: Vec<(String, String, i32)> = sqlx::query_as(
            "SELECT id, COALESCE(ended_at, '[active]'), analysis_retries FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND (slot_id IS NULL OR (
                   slot_id NOT LIKE 'slot-memory%'
                   AND slot_id NOT LIKE 'slot-diagnosis%'
                   AND slot_id NOT LIKE 'agent-%'
               ))
               AND ended_at < (NOW() - INTERVAL '5 minutes')::text
               AND analysis_retries < $1
               AND (analyzed_at IS NULL OR analysis_version < $2)

             UNION ALL

             SELECT id, '[checkpoint]', analysis_retries FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND (slot_id IS NULL OR (
                   slot_id NOT LIKE 'slot-memory%'
                   AND slot_id NOT LIKE 'slot-diagnosis%'
                   AND slot_id NOT LIKE 'agent-%'
               ))
               AND analysis_retries < $1
               AND EXISTS (
                   SELECT 1 FROM conversation_messages m
                   WHERE m.session_id = conversations.id
                     AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                     AND m.role IN ('user', 'assistant')
                   ORDER BY m.id ASC
                   OFFSET 99
                   LIMIT 1
               )

             ORDER BY 2 ASC
             LIMIT 20",
        )
        .bind(max_retries)
        .bind(current_version)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn mark_analysis_complete(&self, id: &str, version: i32) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE conversations SET analyzed_at = $1, analysis_version = $2, analysis_retries = 0, deep_analyzed_message_id = 0 WHERE id = $3"
        )
        .bind(&now)
        .bind(version)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_deep_checkpoint(&self, id: &str, message_id: i64) -> DbResult<()> {
        sqlx::query(
            "UPDATE conversations SET deep_analyzed_message_id = $1, analysis_retries = 0 WHERE id = $2"
        )
        .bind(message_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_analysis_failed(&self, id: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE conversations SET analysis_retries = analysis_retries + 1 WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // -- habit scanning --

    async fn get_unscanned_conversations(&self, limit: usize) -> DbResult<Vec<Conversation>> {
        let rows = sqlx::query(
            "SELECT * FROM conversations
             WHERE habit_scanned_at IS NULL
               AND conversation_type = 'user'
               AND message_count >= 4
               AND status IN ('completed', 'compacted', 'active')
             ORDER BY started_at ASC
             LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    async fn mark_habit_scanned(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        sqlx::query("UPDATE conversations SET habit_scanned_at = $1 WHERE id = $2")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn count_unscanned_conversations(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations
             WHERE habit_scanned_at IS NULL
               AND conversation_type = 'user'
               AND message_count >= 4",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn count_scannable_conversations(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations
             WHERE conversation_type = 'user'
               AND message_count >= 4",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    // -- summary & embeddings --

    async fn set_conversation_summary(&self, id: &str, summary: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET llm_summary = $1 WHERE id = $2")
            .bind(summary)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear_conversation_summary(&self, id: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET llm_summary = NULL, embedding = NULL, embedding_provider = NULL WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_conversation_embedding(
        &self,
        id: &str,
        embedding: &[f32],
        provider: &str,
    ) -> DbResult<()> {
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        // Note: conversations table doesn't have embedding_vec column (no ANN search needed),
        // only BYTEA blob for in-memory cosine similarity.
        sqlx::query(
            "UPDATE conversations SET embedding = $1, embedding_provider = $2 WHERE id = $3",
        )
        .bind(&bytes)
        .bind(provider)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_conversation_embeddings(
        &self,
        provider: &str,
    ) -> DbResult<Vec<(String, Vec<f32>)>> {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM conversations
             WHERE embedding IS NOT NULL AND embedding_provider = $1
               AND conversation_type NOT IN ('meta', 'compaction')",
        )
        .bind(provider)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, blob)| (id, crate::embedding::bytes_to_f32_vec(&blob)))
            .collect())
    }

    async fn conversations_missing_summary(&self, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM conversations
             WHERE llm_summary IS NULL AND status = 'completed' AND message_count >= 6
               AND conversation_type IN ('user', 'worker')
             ORDER BY started_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn conversations_stale_embedding(
        &self,
        current_provider: &str,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM conversations
             WHERE llm_summary IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != $1
               AND conversation_type IN ('user', 'worker')
             ORDER BY started_at DESC LIMIT $2",
        )
        .bind(current_provider)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    // -- topic vectors --

    async fn set_conversation_topic_vectors(
        &self,
        session_id: &str,
        topics: &[(String, Vec<f32>)],
        provider: &str,
    ) -> DbResult<()> {
        sqlx::query("DELETE FROM conversation_topic_vectors WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        for (idx, (topic, vec)) in topics.iter().enumerate() {
            let bytes = crate::embedding::f32_vec_to_bytes(vec);
            sqlx::query(
                "INSERT INTO conversation_topic_vectors (session_id, chunk_idx, topic, embedding, embedding_provider)
                 VALUES ($1, $2, $3, $4, $5)"
            )
            .bind(session_id)
            .bind(idx as i64)
            .bind(topic)
            .bind(&bytes)
            .bind(provider)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn load_conversation_topic_vectors(
        &self,
        provider: &str,
    ) -> DbResult<Vec<(String, Vec<Vec<f32>>)>> {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT tv.session_id, tv.embedding
             FROM conversation_topic_vectors tv
             JOIN conversations c ON c.id = tv.session_id
             WHERE tv.embedding_provider = $1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             ORDER BY tv.session_id, tv.chunk_idx",
        )
        .bind(provider)
        .fetch_all(&self.pool)
        .await?;

        let mut result: Vec<(String, Vec<Vec<f32>>)> = Vec::new();
        for (sid, blob) in rows {
            let vec = crate::embedding::bytes_to_f32_vec(&blob);
            if let Some(last) = result.last_mut() {
                if last.0 == sid {
                    last.1.push(vec);
                    continue;
                }
            }
            result.push((sid, vec![vec]));
        }
        Ok(result)
    }

    async fn get_conversation_topics(&self, session_id: &str) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT topic FROM conversation_topic_vectors
             WHERE session_id = $1 ORDER BY chunk_idx",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn conversations_needing_topic_vectors(
        &self,
        provider: &str,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT c.id FROM conversations c
             WHERE c.llm_summary IS NOT NULL
               AND c.conversation_type IN ('user', 'worker')
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = c.id AND tv.embedding_provider = $1
               )
             ORDER BY c.started_at DESC LIMIT $2",
        )
        .bind(provider)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    // -- timeline reconstruction --

    async fn conversations_needing_timeline(&self, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT c.parent_session_id
             FROM conversations c
             JOIN conversations p ON p.id = c.parent_session_id
             WHERE c.conversation_type = 'compaction'
               AND p.status = 'completed'
               AND p.timeline_built_at IS NULL
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_compaction_fragments(
        &self,
        parent_id: &str,
    ) -> DbResult<Vec<(String, String, i64)>> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(COMPACTION_FRAGMENTS_QUERY)
            .bind(parent_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn get_last_assistant_content(&self, session_id: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT content FROM conversation_messages
             WHERE session_id = $1 AND role = 'assistant'
             ORDER BY id DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    async fn set_session_timeline(&self, parent_id: &str, timeline_json: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE conversations SET session_timeline = $1, timeline_built_at = to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
             WHERE id = $2 AND timeline_built_at IS NULL"
        )
        .bind(timeline_json)
        .bind(parent_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    // -- conversation lifecycle (audit.rs) --

    async fn mark_conversation_analyzed(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE conversations SET analyzed_at = $1 WHERE id = $2")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_unanalyzed_conversations(&self) -> DbResult<Vec<Conversation>> {
        let rows = sqlx::query(
            "SELECT * FROM conversations WHERE status = 'completed' AND analyzed_at IS NULL ORDER BY started_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    async fn complete_conversation(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE conversations SET status = 'completed', ended_at = $1 WHERE id = $2")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn save_conversation_exit_code(&self, id: &str, exit_code: i32) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET exit_code = $1 WHERE id = $2")
            .bind(exit_code)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn complete_stale_conversations(&self, cutoff: &str) -> DbResult<usize> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT c.id FROM conversations c
             WHERE c.status = 'active'
               AND (SELECT MAX(m.timestamp) FROM conversation_messages m WHERE m.session_id = c.id) < $1::timestamptz"
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        let now = chrono::Utc::now().to_rfc3339();
        for (id,) in &rows {
            sqlx::query(
                "UPDATE conversations SET status = 'completed', ended_at = $1 WHERE id = $2",
            )
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(rows.len())
    }

    async fn mark_conversation_compacted(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE conversations SET status = 'compacted', ended_at = $1 WHERE id = $2")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_conversation_task_id(&self, id: &str, task_id: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET task_id = $1 WHERE id = $2")
            .bind(task_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_conversation_type(&self, id: &str, conversation_type: &str) -> DbResult<usize> {
        let result = sqlx::query(
            "UPDATE conversations
             SET conversation_type = $1
             WHERE id = $2 AND conversation_type <> $1",
        )
        .bind(conversation_type)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn backfill_missing_raw_roles_for_session(&self, id: &str) -> DbResult<usize> {
        let result = sqlx::query(
            "UPDATE conversation_messages
             SET raw_role = role
             WHERE session_id = $1
               AND raw_role IS NULL
               AND role IS NOT NULL
               AND role <> ''",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn claude_worker_user_role_backfill_candidates(
        &self,
        session_id: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<MessageRoleBackfillCandidate>> {
        let rows = sqlx::query(
            r#"
            SELECT
              m.id,
              m.session_id,
              c.conversation_type,
              c.slot_id,
              c.task_id,
              m.role,
              m.raw_role,
              COALESCE(m.timestamp::text, '') AS timestamp,
              CASE
                WHEN m.content LIKE '<local-command-%' THEN 'local-command'
                WHEN m.content LIKE '<command-name>%' THEN 'local-command'
                WHEN m.content LIKE '<command-message>%' THEN 'local-command'
                WHEN m.content LIKE '<command-args>%' THEN 'local-command'
                WHEN m.content LIKE 'Execute MissionD task %' THEN 'worker-prompt'
                WHEN m.content LIKE 'Fix MissionD-side swarm %' THEN 'worker-prompt'
                WHEN m.content LIKE 'Implement accepted swarm shard%' THEN 'worker-prompt'
                WHEN m.content LIKE 'Survey exact shards for swarm objective%' THEN 'worker-prompt'
                WHEN m.content ILIKE '%BoardTask ID%' THEN 'worker-prompt'
                WHEN m.content ILIKE '%Task contract SSOT%' THEN 'worker-prompt'
                WHEN m.content ILIKE '%completion protocol%' THEN 'worker-prompt'
                WHEN m.content ILIKE '%write_scope%' THEN 'worker-prompt'
                WHEN m.content ILIKE '%must_not_touch%' THEN 'worker-prompt'
                ELSE 'worker-user-raw-role'
              END AS reason,
              LEFT(regexp_replace(COALESCE(m.content, ''), '\s+', ' ', 'g'), 240) AS content_preview
            FROM conversation_messages m
            JOIN conversations c ON c.id = m.session_id
            WHERE c.source = 'claude_code'
              AND c.conversation_type = 'worker'
              AND m.role = 'user'
              AND m.raw_role = 'user'
              AND ($1::text IS NULL OR m.session_id = $1)
              AND (
                   m.content LIKE '<local-command-%'
                OR m.content LIKE '<command-name>%'
                OR m.content LIKE '<command-message>%'
                OR m.content LIKE '<command-args>%'
                OR m.content LIKE 'Execute MissionD task %'
                OR m.content LIKE 'Fix MissionD-side swarm %'
                OR m.content LIKE 'Implement accepted swarm shard%'
                OR m.content LIKE 'Survey exact shards for swarm objective%'
                OR m.content ILIKE '%BoardTask ID%'
                OR m.content ILIKE '%Task contract SSOT%'
                OR m.content ILIKE '%completion protocol%'
                OR m.content ILIKE '%write_scope%'
                OR m.content ILIKE '%must_not_touch%'
              )
            ORDER BY m.timestamp DESC NULLS LAST, m.id DESC
            LIMIT $2
            "#,
        )
        .bind(session_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| MessageRoleBackfillCandidate {
                message_id: row.get("id"),
                session_id: row.get("session_id"),
                conversation_type: row.get("conversation_type"),
                slot_id: row.get("slot_id"),
                task_id: row.get("task_id"),
                role: row.get("role"),
                raw_role: row.get("raw_role"),
                timestamp: row.get("timestamp"),
                reason: row.get("reason"),
                content_preview: row.get("content_preview"),
            })
            .collect())
    }

    async fn backfill_claude_worker_user_message_roles(
        &self,
        message_ids: &[i64],
    ) -> DbResult<usize> {
        if message_ids.is_empty() {
            return Ok(0);
        }
        let result = sqlx::query(
            r#"
            UPDATE conversation_messages m
            SET role = 'worker_user'
            FROM conversations c
            WHERE c.id = m.session_id
              AND c.source = 'claude_code'
              AND c.conversation_type = 'worker'
              AND m.role = 'user'
              AND m.raw_role = 'user'
              AND m.id = ANY($1)
            "#,
        )
        .bind(message_ids)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn get_conversations_by_task_id(&self, task_id: &str) -> DbResult<Vec<Conversation>> {
        let rows =
            sqlx::query("SELECT * FROM conversations WHERE task_id = $1 ORDER BY started_at ASC")
                .bind(task_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    async fn reactivate_conversation(&self, id: &str) -> DbResult<usize> {
        let result = sqlx::query(
            "UPDATE conversations SET status = 'active', ended_at = NULL WHERE id = $1 AND status = 'completed'"
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    // -- message embeddings (independent table) --

    async fn insert_message_embedding(
        &self,
        message_id: i64,
        session_id: &str,
        embedding_vec: &[f32],
        model_version: &str,
    ) -> DbResult<()> {
        let vec_str = vec_to_pg_literal(embedding_vec);
        sqlx::query(
            "INSERT INTO message_embeddings (message_id, session_id, embedding_vec, model_version)
             VALUES ($1, $2, $3::halfvec(512), $4)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(message_id)
        .bind(session_id)
        .bind(&vec_str)
        .bind(model_version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_message_embedding_skip(
        &self,
        message_id: i64,
        skip_reason: &str,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO message_embedding_skips (message_id, skip_reason)
             VALUES ($1, $2)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(message_id)
        .bind(skip_reason)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_message_embeddings_batch(
        &self,
        entries: &[(i64, &str, Vec<f32>, &str)],
    ) -> DbResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        // Use sqlx::QueryBuilder for safe dynamic VALUES batch insert
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "INSERT INTO message_embeddings (message_id, session_id, embedding_vec, model_version) "
        );
        qb.push_values(entries, |mut b, (msg_id, sid, vec_data, model_ver)| {
            b.push_bind(*msg_id);
            b.push_bind(*sid);
            // halfvec literal — cannot use native bind, push as raw SQL cast
            let lit = vec_to_pg_literal(vec_data);
            b.push(format!("'{}'::halfvec(512)", lit));
            b.push_bind(*model_ver);
        });
        qb.push(" ON CONFLICT (message_id) DO NOTHING");
        let result = qb.build().execute(&self.pool).await?;
        Ok(result.rows_affected() as usize)
    }

    async fn insert_message_embedding_skips_batch(
        &self,
        entries: &[(i64, &str)],
    ) -> DbResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "INSERT INTO message_embedding_skips (message_id, skip_reason) ",
        );
        qb.push_values(entries, |mut b, (msg_id, reason)| {
            b.push_bind(*msg_id);
            b.push_bind(*reason);
        });
        qb.push(" ON CONFLICT (message_id) DO NOTHING");
        let result = qb.build().execute(&self.pool).await?;
        Ok(result.rows_affected() as usize)
    }

    async fn messages_pending_embedding(
        &self,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<(i64, String, String, String)>> {
        // v2: pure cursor scan — no LEFT JOIN, O(batch_size) not O(N)
        let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
            "SELECT cm.id, cm.session_id, cm.role, cm.content
             FROM conversation_messages cm
             WHERE cm.id > $1
             ORDER BY cm.id ASC LIMIT $2",
        )
        .bind(cursor)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn message_embedding_stats(&self) -> DbResult<serde_json::Value> {
        let (total_msgs,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM conversation_messages")
            .fetch_one(&self.pool)
            .await?;
        let (embedded,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM message_embeddings")
            .fetch_one(&self.pool)
            .await?;
        let (skipped,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM message_embedding_skips")
            .fetch_one(&self.pool)
            .await?;
        let pending = total_msgs - embedded - skipped;

        let skip_dist: Vec<(String, i64)> = sqlx::query_as(
            "SELECT skip_reason, COUNT(*) FROM message_embedding_skips GROUP BY skip_reason ORDER BY 2 DESC"
        ).fetch_all(&self.pool).await?;

        let model_dist: Vec<(String, i64)> = sqlx::query_as(
            "SELECT model_version, COUNT(*) FROM message_embeddings GROUP BY model_version ORDER BY 2 DESC"
        ).fetch_all(&self.pool).await?;

        let coverage = if total_msgs > 0 {
            format!(
                "{:.1}%",
                (embedded + skipped) as f64 / total_msgs as f64 * 100.0
            )
        } else {
            "N/A".to_string()
        };

        Ok(serde_json::json!({
            "totalMessages": total_msgs,
            "embedded": embedded,
            "skipped": skipped,
            "pending": pending,
            "coverage": coverage,
            "skipReasons": skip_dist.into_iter().map(|(r, c)| serde_json::json!({"reason": r, "count": c})).collect::<Vec<_>>(),
            "modelVersions": model_dist.into_iter().map(|(m, c)| serde_json::json!({"model": m, "count": c})).collect::<Vec<_>>(),
        }))
    }

    // -- extraction watermarks --

    async fn get_pending_memory_messages(
        &self,
        today: &str,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid,
                    m.parent_uuid, m.model, m.timestamp, m.metadata, m.tool_name,
                    m.raw_role, m.content_types, m.has_image, m.has_tool_use, m.has_tool_result, m.token_count,
                    COALESCE(c.project, '') as c_project
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp >= $1::timestamptz
               AND m.timestamp > COALESCE(c.memory_forwarded_at, $1)::timestamptz
               AND m.role IN ('user', 'assistant')
             ORDER BY c.started_at DESC, m.timestamp ASC, m.id ASC"
        )
        .bind(today)
        .fetch_all(&self.pool)
        .await?;

        let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();
        for row in &rows {
            use sqlx::Row;
            let msg = Self::row_to_conversation_message(row);
            let project: String = row.get("c_project");
            let session_id = msg.session_id.clone();
            if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                entry.2.push(msg);
            } else {
                results.push((session_id, project, vec![msg]));
            }
        }
        Ok(results)
    }

    async fn update_memory_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET memory_forwarded_at = $1 WHERE id = $2")
            .bind(timestamp)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_pending_user_voice_messages(
        &self,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid,
                    m.parent_uuid, m.model, m.timestamp, m.metadata, m.tool_name,
                    m.raw_role, m.content_types, m.has_image, m.has_tool_use, m.has_tool_result, m.token_count,
                    COALESCE(c.project, '') as c_project
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.user_voice_forwarded_at, c.started_at)::timestamptz
               AND m.role = 'user'
             ORDER BY m.timestamp ASC"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();
        for row in &rows {
            use sqlx::Row;
            let msg = Self::row_to_conversation_message(row);
            let project: String = row.get("c_project");
            let session_id = msg.session_id.clone();
            if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                entry.2.push(msg);
            } else {
                results.push((session_id, project, vec![msg]));
            }
        }
        Ok(results)
    }

    async fn update_user_voice_forwarded_at(
        &self,
        session_id: &str,
        timestamp: &str,
    ) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET user_voice_forwarded_at = $1 WHERE id = $2")
            .bind(timestamp)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_pending_realtime_messages(
        &self,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        self.get_pending_realtime_messages_with_limit(50).await
    }

    async fn get_pending_realtime_messages_with_limit(
        &self,
        limit: usize,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid,
                    m.parent_uuid, m.model, m.timestamp, m.metadata, m.tool_name,
                    m.raw_role, m.content_types, m.has_image, m.has_tool_use, m.has_tool_result, m.token_count,
                    COALESCE(c.project, '') as c_project
             FROM conversations c
             CROSS JOIN LATERAL (
                 SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid,
                        m.parent_uuid, m.model, m.timestamp, m.metadata, m.tool_name,
                        m.raw_role, m.content_types, m.has_image, m.has_tool_use, m.has_tool_result, m.token_count
                 FROM conversation_messages m
                 WHERE m.session_id = c.id
                   AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)::timestamptz
                   AND m.role IN ('user', 'assistant', 'tool_result')
                 ORDER BY m.timestamp ASC, m.id ASC
                 LIMIT 15
             ) m
             WHERE c.conversation_type = 'user'
             ORDER BY m.timestamp ASC
             LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();
        for row in &rows {
            use sqlx::Row;
            let msg = Self::row_to_conversation_message(row);
            let project: String = row.get("c_project");
            let session_id = msg.session_id.clone();
            if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                entry.2.push(msg);
            } else {
                results.push((session_id, project, vec![msg]));
            }
        }
        Ok(results)
    }

    async fn update_realtime_forwarded_at(
        &self,
        session_id: &str,
        timestamp: &str,
    ) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET realtime_forwarded_at = $1 WHERE id = $2")
            .bind(timestamp)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- conversation_turns (S3 Tagger & Chunker) --

    async fn get_last_turn_end_message_id(&self, session_id: &str) -> DbResult<Option<i64>> {
        let row = sqlx::query_scalar::<_, i64>(
            "SELECT end_message_id FROM conversation_turns
             WHERE session_id = $1
             ORDER BY turn_idx DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn get_max_turn_idx(&self, session_id: &str) -> DbResult<Option<i32>> {
        // MAX() always returns exactly one row; empty table → [NULL].
        // Must decode as Option<i32> to avoid UnexpectedNullError.
        let row = sqlx::query_scalar::<_, Option<i32>>(
            "SELECT MAX(turn_idx) FROM conversation_turns WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn insert_conversation_turns_batch(
        &self,
        session_id: &str,
        base_idx: i32,
        turns: &[RawTurn],
    ) -> DbResult<usize> {
        if turns.is_empty() {
            return Ok(0);
        }
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO conversation_turns \
             (session_id, turn_idx, start_message_id, end_message_id, \
              user_content, tool_names, tool_call_count, message_count, \
              has_code_change, has_mcp_call, started_at, ended_at, \
              files_read, files_changed, outcome, skeleton) ",
        );
        qb.push_values(turns.iter().enumerate(), |mut b, (i, turn)| {
            b.push_bind(session_id)
                .push_bind(base_idx + i as i32)
                .push_bind(turn.start_message_id)
                .push_bind(turn.end_message_id)
                .push_bind(&turn.user_content)
                .push_bind(&turn.tool_names)
                .push_bind(turn.tool_call_count)
                .push_bind(turn.message_count)
                .push_bind(turn.has_code_change)
                .push_bind(turn.has_mcp_call)
                .push_bind(&turn.started_at)
                .push_bind(&turn.ended_at)
                .push_bind(&turn.files_read)
                .push_bind(&turn.files_changed)
                .push_bind(&turn.outcome)
                .push_bind(&turn.skeleton);
        });
        qb.push(" ON CONFLICT (session_id, turn_idx) DO NOTHING");
        let result = qb.build().execute(&self.pool).await?;
        Ok(result.rows_affected() as usize)
    }

    async fn clear_conversation_turns(&self, session_id: &str) -> DbResult<usize> {
        let result = sqlx::query("DELETE FROM conversation_turns WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn insert_message_labels_batch(
        &self,
        labels: &[(i64, &str, &str, &str)],
    ) -> DbResult<usize> {
        if labels.is_empty() {
            return Ok(0);
        }
        let mut inserted = 0usize;
        for &(message_id, label, value, source) in labels {
            let result = sqlx::query(
                "INSERT INTO message_labels (message_id, label, value, source)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (message_id, label) DO NOTHING",
            )
            .bind(message_id)
            .bind(label)
            .bind(value)
            .bind(source)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() > 0 {
                inserted += 1;
            }
        }
        Ok(inserted)
    }

    async fn sessions_pending_turn_extraction(&self, limit: i64) -> DbResult<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT c.id FROM conversations c
             WHERE c.message_count > 0
               AND c.status IN ('completed', 'compacted')
               AND NOT EXISTS (SELECT 1 FROM conversation_turns ct WHERE ct.session_id = c.id)
             ORDER BY c.started_at DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn sessions_recently_active_without_turns(
        &self,
        since_minutes: i64,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT c.id FROM conversations c
             WHERE c.message_count > 0
               AND c.updated_at IS NOT NULL
               AND c.updated_at > to_char((NOW() - ($1 || ' minutes')::interval) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS')
               AND NOT EXISTS (SELECT 1 FROM conversation_turns ct WHERE ct.session_id = c.id)
             ORDER BY c.updated_at DESC
             LIMIT $2"
        )
            .bind(since_minutes)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    // -- S4 per-turn embedding --

    async fn turns_pending_embedding(
        &self,
        session_id: &str,
        provider: &str,
    ) -> DbResult<Vec<ConversationTurn>> {
        let rows = sqlx::query_as::<_, ConversationTurn>(
            "SELECT t.id, t.session_id, t.turn_idx, t.start_message_id, t.end_message_id,
                    t.user_content, t.tool_names, t.tool_call_count, t.message_count,
                    t.has_code_change, t.has_mcp_call, t.started_at, t.ended_at,
                    t.topic, t.intent_group_id,
                    t.files_read, t.files_changed, t.outcome, t.skeleton
             FROM conversation_turns t
             WHERE t.session_id = $1
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = t.session_id
                     AND tv.chunk_idx = t.turn_idx
                     AND tv.embedding_provider = $2
               )
             ORDER BY t.turn_idx",
        )
        .bind(session_id)
        .bind(provider)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn update_turn_topics_batch(&self, updates: &[(i64, &str)]) -> DbResult<usize> {
        let mut count = 0usize;
        for &(id, topic) in updates {
            let result = sqlx::query("UPDATE conversation_turns SET topic = $2 WHERE id = $1")
                .bind(id)
                .bind(topic)
                .execute(&self.pool)
                .await?;
            count += result.rows_affected() as usize;
        }
        Ok(count)
    }

    async fn set_conversation_turn_vectors(
        &self,
        session_id: &str,
        vectors: &[(String, i32, Vec<f32>)],
        provider: &str,
    ) -> DbResult<usize> {
        if vectors.is_empty() {
            return Ok(0);
        }
        let mut count = 0usize;
        for (topic, turn_idx, vec) in vectors {
            let bytes = crate::embedding::f32_vec_to_bytes(vec);
            let result = sqlx::query(
                "INSERT INTO conversation_topic_vectors (session_id, chunk_idx, topic, embedding, embedding_provider)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (session_id, chunk_idx)
                 DO UPDATE SET topic = EXCLUDED.topic,
                               embedding = EXCLUDED.embedding,
                               embedding_provider = EXCLUDED.embedding_provider"
            )
                .bind(session_id)
                .bind(*turn_idx as i64)
                .bind(topic)
                .bind(&bytes)
                .bind(provider)
                .execute(&self.pool)
                .await?;
            count += result.rows_affected() as usize;
        }
        Ok(count)
    }

    async fn sessions_with_turns_but_no_vectors(
        &self,
        provider: &str,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT ct.session_id
             FROM conversation_turns ct
             JOIN conversations c ON c.id = ct.session_id
             WHERE c.status IN ('completed', 'compacted')
               AND c.conversation_type IN ('user', 'worker')
               AND c.id > $3
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = ct.session_id
                     AND tv.embedding_provider = $1
               )
             ORDER BY ct.session_id
             LIMIT $2",
        )
        .bind(provider)
        .bind(limit)
        .bind(cursor.to_string())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -- Phase 6: user_intents --

    async fn insert_user_intent(
        &self,
        session_id: &str,
        turn_range_start: i32,
        turn_range_end: i32,
        intent_type: &str,
        confidence: f32,
        summary: Option<&str>,
        context_json: Option<&str>,
        related_goal_id: Option<&str>,
    ) -> DbResult<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            "INSERT INTO user_intents (session_id, turn_range_start, turn_range_end, intent_type, confidence, summary, context_json, related_goal_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (session_id, turn_range_start) DO UPDATE
             SET turn_range_end = EXCLUDED.turn_range_end,
                 intent_type = EXCLUDED.intent_type,
                 confidence = EXCLUDED.confidence,
                 summary = EXCLUDED.summary,
                 context_json = EXCLUDED.context_json
             RETURNING id"
        )
            .bind(session_id)
            .bind(turn_range_start)
            .bind(turn_range_end)
            .bind(intent_type)
            .bind(confidence)
            .bind(summary)
            .bind(context_json)
            .bind(related_goal_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn get_intent_coverage(&self, session_id: &str) -> DbResult<Option<i32>> {
        let row = sqlx::query_scalar::<_, Option<i32>>(
            "SELECT MAX(turn_range_end) FROM user_intents WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn get_turns_after(
        &self,
        session_id: &str,
        after_idx: i32,
    ) -> DbResult<Vec<ConversationTurn>> {
        let rows = sqlx::query_as::<_, ConversationTurn>(
            "SELECT id, session_id, turn_idx, start_message_id, end_message_id,
                    user_content, tool_names, tool_call_count, message_count,
                    has_code_change, has_mcp_call, started_at, ended_at, topic, intent_group_id,
                    files_read, files_changed, outcome, skeleton
             FROM conversation_turns
             WHERE session_id = $1 AND turn_idx > $2
             ORDER BY turn_idx",
        )
        .bind(session_id)
        .bind(after_idx)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn update_turns_intent_group(
        &self,
        session_id: &str,
        turn_range_start: i32,
        turn_range_end: i32,
        intent_id: i64,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE conversation_turns SET intent_group_id = $4
             WHERE session_id = $1 AND turn_idx >= $2 AND turn_idx <= $3",
        )
        .bind(session_id)
        .bind(turn_range_start)
        .bind(turn_range_end)
        .bind(intent_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_recent_intents(&self, since_secs: i64) -> DbResult<Vec<UserIntent>> {
        let rows = sqlx::query_as::<_, UserIntent>(
            "SELECT id, session_id, turn_range_start, turn_range_end, intent_type,
                    confidence, summary, context_json, related_goal_id,
                    created_at::text as created_at
             FROM user_intents
             WHERE created_at > NOW() - make_interval(secs => $1::double precision)
             ORDER BY created_at DESC",
        )
        .bind(since_secs as f64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn sessions_pending_intent_analysis(&self, limit: i64) -> DbResult<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT ct.session_id
             FROM conversation_turns ct
             JOIN conversations c ON c.id = ct.session_id
             WHERE c.status IN ('completed', 'compacted')
               AND c.conversation_type IN ('user', 'worker')
               AND NOT EXISTS (SELECT 1 FROM user_intents ui WHERE ui.session_id = ct.session_id)
             ORDER BY ct.session_id
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ══════════════════════════════════════════════════════════════════════
    // -- from ToolCallStore v0.4.x: tool call CRUD, stats, retrospective --
    // ══════════════════════════════════════════════════════════════════════

    async fn insert_tool_call(&self, tc: &ToolCallRecord) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO NOTHING"
        )
        .bind(&tc.id)
        .bind(&tc.session_id)
        .bind(tc.message_id)
        .bind(&tc.tool_name)
        .bind(&tc.input_summary)
        .bind(&tc.raw_input)
        .bind(&tc.output_summary)
        .bind(&tc.raw_output)
        .bind(&tc.status)
        .bind(tc.duration_ms)
        .bind(&tc.timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_tool_calls_batch(&self, calls: &[ToolCallRecord]) -> DbResult<usize> {
        if calls.is_empty() {
            return Ok(0);
        }
        // Insert new calls; for already-existing calls, allow ONLY a
        // pending → terminal transition. This handles ingestion sources (e.g.
        // Codex JSONL) that write the function_call line first and the matching
        // function_call_output later: the first ingest creates a pending row,
        // the next ingest (after the output is appended) flips it to success/error
        // with the actual output. Existing terminal rows are never overwritten.
        let mut count = 0usize;
        for tc in calls {
            let result = sqlx::query(
                "INSERT INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT (id) DO UPDATE SET
                    output_summary = EXCLUDED.output_summary,
                    raw_output     = EXCLUDED.raw_output,
                    status         = EXCLUDED.status,
                    duration_ms    = EXCLUDED.duration_ms
                 WHERE conversation_tool_calls.status = 'pending'
                   AND EXCLUDED.status <> 'pending'"
            )
            .bind(&tc.id)
            .bind(&tc.session_id)
            .bind(tc.message_id)
            .bind(&tc.tool_name)
            .bind(&tc.input_summary)
            .bind(&tc.raw_input)
            .bind(&tc.output_summary)
            .bind(&tc.raw_output)
            .bind(&tc.status)
            .bind(tc.duration_ms)
            .bind(&tc.timestamp)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() > 0 {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn update_tool_call_output(
        &self,
        tool_use_id: &str,
        output_summary: &str,
        raw_output: &str,
        status: &str,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE conversation_tool_calls SET output_summary = $1, raw_output = $2, status = $3 WHERE id = $4"
        )
        .bind(output_summary)
        .bind(raw_output)
        .bind(status)
        .bind(tool_use_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn get_tool_calls_by_session(
        &self,
        session_id: &str,
        tool_filter: Option<&[String]>,
        limit: i64,
    ) -> DbResult<Vec<ToolCallRecord>> {
        if let Some(filter) = tool_filter {
            if filter.is_empty() {
                return Ok(Vec::new());
            }
            // Build dynamic IN clause
            let placeholders: Vec<String> =
                (0..filter.len()).map(|i| format!("${}", i + 2)).collect();
            let sql = format!(
                "SELECT id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp
                 FROM conversation_tool_calls WHERE session_id = $1 AND tool_name IN ({}) ORDER BY id ASC LIMIT ${}",
                placeholders.join(","),
                filter.len() + 2
            );
            // sqlx doesn't support dynamic bind count easily, so use query_as with raw
            let mut query = sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    Option<i64>,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    String,
                    Option<i64>,
                    String,
                ),
            >(&sql);
            query = query.bind(session_id);
            for f in filter {
                query = query.bind(f);
            }
            query = query.bind(limit);
            let rows = query.fetch_all(&self.pool).await?;
            Ok(rows
                .into_iter()
                .map(|r| ToolCallRecord {
                    id: r.0,
                    session_id: r.1,
                    message_id: r.2,
                    tool_name: r.3,
                    input_summary: r.4,
                    raw_input: r.5,
                    output_summary: r.6,
                    raw_output: r.7,
                    status: r.8,
                    duration_ms: r.9,
                    timestamp: r.10,
                })
                .collect())
        } else {
            let rows: Vec<(String, String, Option<i64>, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<i64>, String)> = sqlx::query_as(
                "SELECT id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp
                 FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC LIMIT $2"
            )
            .bind(session_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows
                .into_iter()
                .map(|r| ToolCallRecord {
                    id: r.0,
                    session_id: r.1,
                    message_id: r.2,
                    tool_name: r.3,
                    input_summary: r.4,
                    raw_input: r.5,
                    output_summary: r.6,
                    raw_output: r.7,
                    status: r.8,
                    duration_ms: r.9,
                    timestamp: r.10,
                })
                .collect())
        }
    }

    async fn get_tool_call_by_id(&self, tool_use_id: &str) -> DbResult<Option<ToolCallRecord>> {
        let row: Option<(String, String, Option<i64>, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<i64>, String)> = sqlx::query_as(
            "SELECT id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp
             FROM conversation_tool_calls WHERE id = $1"
        )
        .bind(tool_use_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| ToolCallRecord {
            id: r.0,
            session_id: r.1,
            message_id: r.2,
            tool_name: r.3,
            input_summary: r.4,
            raw_input: r.5,
            output_summary: r.6,
            raw_output: r.7,
            status: r.8,
            duration_ms: r.9,
            timestamp: r.10,
        }))
    }

    async fn get_tool_call_stats(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(String, i64, i64, i64)>> {
        let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
            "SELECT tool_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count
             FROM conversation_tool_calls WHERE session_id = $1
             GROUP BY tool_name ORDER BY total DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn count_pending_tool_calls(&self) -> DbResult<i64> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM conversation_tool_calls WHERE status = 'pending'")
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }

    async fn get_sessions_with_pending_tool_calls(&self) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT session_id FROM conversation_tool_calls WHERE status = 'pending'",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_sessions_with_tool_calls(&self) -> DbResult<HashSet<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT DISTINCT session_id FROM conversation_tool_calls")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_tool_call_global_stats(
        &self,
        since_iso: Option<&str>,
    ) -> DbResult<Vec<(String, i64, Option<String>, i64, i64)>> {
        // capability-usage-read-model :: tool-calls truth source.
        // `timestamp` is ISO text; lexicographic compare is correct here because
        // ISO-8601 with the same offset/zone sorts the same as chronological.
        let rows: Vec<(String, i64, Option<String>, i64, i64)> = match since_iso {
            Some(since) => {
                sqlx::query_as(
                    "SELECT tool_name,
                        COUNT(*)::bigint AS total,
                        MAX(timestamp) AS last_at,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END)::bigint AS ok,
                        SUM(CASE WHEN status='error'   THEN 1 ELSE 0 END)::bigint AS err
                 FROM conversation_tool_calls
                 WHERE timestamp >= $1
                 GROUP BY tool_name
                 ORDER BY total DESC",
                )
                .bind(since)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as(
                    "SELECT tool_name,
                        COUNT(*)::bigint AS total,
                        MAX(timestamp) AS last_at,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END)::bigint AS ok,
                        SUM(CASE WHEN status='error'   THEN 1 ELSE 0 END)::bigint AS err
                 FROM conversation_tool_calls
                 GROUP BY tool_name
                 ORDER BY total DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows)
    }

    async fn get_messages_for_tool_call_backfill(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT role, raw_content, timestamp FROM conversation_messages
             WHERE session_id = $1 AND raw_content IS NOT NULL AND raw_content != ''
             AND role IN ('assistant', 'user', 'worker_user', 'thinking', 'system', 'tool_result')
             ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_conversations_with_jsonl(&self) -> DbResult<Vec<(String, String)>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, jsonl_path FROM conversations WHERE jsonl_path IS NOT NULL AND jsonl_path != ''"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -- Retrospective tool analysis --

    async fn get_retrospective_tool_stats(
        &self,
        session_id: &str,
        limit: i64,
    ) -> DbResult<Vec<(String, i64, i64, i64, f64)>> {
        let rows: Vec<(String, i64, i64, i64, f64)> = sqlx::query_as(
            "SELECT tool_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count,
                    COALESCE(AVG(duration_ms), 0) as avg_duration
             FROM conversation_tool_calls WHERE session_id = $1
             GROUP BY tool_name ORDER BY total DESC LIMIT $2",
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_retrospective_meta(&self, session_id: &str) -> DbResult<(i64, i64, i64, i64)> {
        let (total_calls, total_duration, unique_tools): (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(duration_ms), 0), COUNT(DISTINCT tool_name)
             FROM conversation_tool_calls WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        let (compact_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_events
             WHERE session_id = $1 AND event_type = 'compact_boundary'",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((total_calls, total_duration, unique_tools, compact_count))
    }

    async fn get_retrospective_repeat_patterns(
        &self,
        session_id: &str,
        min_streak: i64,
    ) -> DbResult<Vec<(String, i64, String, String)>> {
        let rows: Vec<(String, i64, String, String)> = sqlx::query_as(
            "WITH numbered AS (
                SELECT tool_name, timestamp,
                       ROW_NUMBER() OVER (ORDER BY id) as rn,
                       ROW_NUMBER() OVER (PARTITION BY tool_name ORDER BY id) as grn
                FROM conversation_tool_calls WHERE session_id = $1
            )
            SELECT tool_name, COUNT(*) as streak, MIN(timestamp) as start_t, MAX(timestamp) as end_t
            FROM numbered GROUP BY tool_name, (rn - grn)
            HAVING COUNT(*) >= $2 ORDER BY streak DESC",
        )
        .bind(session_id)
        .bind(min_streak)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_tool_name_sequence(&self, session_id: &str) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT tool_name FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_retrospective_high_error_tools(
        &self,
        session_id: &str,
        min_error_rate: f64,
    ) -> DbResult<Vec<(String, f64, i64)>> {
        let rows: Vec<(String, f64, i64)> = sqlx::query_as(
            "SELECT tool_name,
                    ROUND(100.0 * SUM(CASE WHEN status='error' THEN 1 ELSE 0 END)::numeric / COUNT(*), 1)::float8 as error_rate,
                    COUNT(*) as total
             FROM conversation_tool_calls WHERE session_id = $1
             GROUP BY tool_name HAVING (100.0 * SUM(CASE WHEN status='error' THEN 1 ELSE 0 END)::numeric / COUNT(*)) > $2
             ORDER BY error_rate DESC"
        )
        .bind(session_id)
        .bind(min_error_rate)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_tool_error_samples(
        &self,
        session_id: &str,
        tool_name: &str,
    ) -> DbResult<Vec<(String, String, String)>> {
        let mut samples = Vec::new();
        // First error
        let first: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT COALESCE(input_summary, ''), COALESCE(output_summary, ''), timestamp
             FROM conversation_tool_calls
             WHERE session_id = $1 AND tool_name = $2 AND status = 'error'
             ORDER BY id ASC LIMIT 1",
        )
        .bind(session_id)
        .bind(tool_name)
        .fetch_all(&self.pool)
        .await?;
        samples.extend(first);
        // Last error (if different from first)
        let last: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT COALESCE(input_summary, ''), COALESCE(output_summary, ''), timestamp
             FROM conversation_tool_calls
             WHERE session_id = $1 AND tool_name = $2 AND status = 'error'
             ORDER BY id DESC LIMIT 1",
        )
        .bind(session_id)
        .bind(tool_name)
        .fetch_all(&self.pool)
        .await?;
        for sample in last {
            if samples.is_empty() || samples[0].2 != sample.2 {
                samples.push(sample);
            }
        }
        Ok(samples)
    }

    async fn get_tool_calls_for_detailed_analysis(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(String, String, String, String)>> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT tool_name, COALESCE(input_summary, ''), COALESCE(output_summary, ''), status
             FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_tool_calls_with_status_timeline(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(String, String, String, String)>> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT tool_name, status, COALESCE(input_summary, ''), timestamp
             FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ══════════════════════════════════════════════════════════════════════
    // -- from EventStore v0.4.x: conversation events (JSONL audit) --
    // ══════════════════════════════════════════════════════════════════════

    async fn insert_conversation_events_batch(
        &self,
        events: &[ConversationEvent],
    ) -> DbResult<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let mut count = 0usize;
        for event in events {
            let result = sqlx::query(
                "INSERT INTO conversation_events (session_id, event_uuid, event_type, content, raw_data, timestamp)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (session_id, event_uuid) DO NOTHING"
            )
            .bind(&event.session_id)
            .bind(&event.event_uuid)
            .bind(&event.event_type)
            .bind(&event.content)
            .bind(&event.raw_data)
            .bind(&event.timestamp)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() > 0 {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn get_conversation_events(
        &self,
        session_id: &str,
        event_type: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<ConversationEvent>> {
        let rows = if let Some(et) = event_type {
            sqlx::query(
                "SELECT id, session_id, event_uuid, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = $1 AND event_type = $2
                 ORDER BY id ASC LIMIT $3",
            )
            .bind(session_id)
            .bind(et)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, session_id, event_uuid, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = $1
                 ORDER BY id ASC LIMIT $2",
            )
            .bind(session_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        let results: Vec<ConversationEvent> = rows
            .iter()
            .map(|row| ConversationEvent {
                id: row.get("id"),
                session_id: row.get("session_id"),
                event_uuid: row.get("event_uuid"),
                event_type: row.get("event_type"),
                content: row.get("content"),
                raw_data: row.get("raw_data"),
                timestamp: row.get("timestamp"),
            })
            .collect();
        Ok(results)
    }

    async fn is_compact_boundary_event(
        &self,
        session_id: &str,
        event_uuid: &str,
    ) -> DbResult<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_events
             WHERE session_id = $1 AND event_uuid = $2 AND event_type = 'compact_boundary'",
        )
        .bind(session_id)
        .bind(event_uuid)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    async fn get_agent_trajectory(
        &self,
        tool_use_id: &str,
        limit: i64,
    ) -> DbResult<Vec<ConversationMessage>> {
        let rows = sqlx::query(
            "SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                    tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
             FROM conversation_messages WHERE parent_uuid = $1 AND role LIKE 'agent_%'
             ORDER BY id ASC LIMIT $2"
        )
        .bind(tool_use_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let results: Vec<ConversationMessage> = rows
            .iter()
            .map(|row| ConversationMessage {
                id: row.get("id"),
                session_id: row.get("session_id"),
                role: row.get("role"),
                content: row.get("content"),
                raw_content: row.get("raw_content"),
                message_uuid: row.get("message_uuid"),
                parent_uuid: row.get("parent_uuid"),
                model: row.get("model"),
                timestamp: row.get("timestamp"),
                metadata: row.get("metadata"),
                tool_name: row.get("tool_name"),
                raw_role: row.get("raw_role"),
                content_types: row.get("content_types"),
                has_image: row.get::<bool, _>("has_image"),
                has_tool_use: row.get::<bool, _>("has_tool_use"),
                has_tool_result: row.get::<bool, _>("has_tool_result"),
                token_count: row.get("token_count"),
                seq: None,
                role_display: None,
            })
            .collect();
        Ok(results)
    }

    async fn get_event_type_summary(
        &self,
        session_id: Option<&str>,
    ) -> DbResult<Vec<(String, i64)>> {
        let rows = if let Some(sid) = session_id {
            sqlx::query(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 WHERE session_id = $1 GROUP BY event_type ORDER BY cnt DESC",
            )
            .bind(sid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 GROUP BY event_type ORDER BY cnt DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };

        let results: Vec<(String, i64)> = rows
            .iter()
            .map(|row| (row.get::<String, _>("event_type"), row.get::<i64, _>("cnt")))
            .collect();
        Ok(results)
    }

    async fn cleanup_old_events(&self, cutoff: &str) -> DbResult<usize> {
        let result = sqlx::query(
            "DELETE FROM conversation_events
             WHERE event_type IN ('progress:bash_progress', 'progress:mcp_progress', 'hook_progress', 'progress:waiting_for_task')
             AND timestamp < $1"
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn get_sessions_with_events(&self) -> DbResult<HashSet<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT DISTINCT session_id FROM conversation_events")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    // ══════════════════════════════════════════════════════════════════════
    // -- from RetrospectiveStore v0.4.x: retrospective results + narration --
    // ══════════════════════════════════════════════════════════════════════

    // -- audit.rs: retrospective --

    async fn save_retrospective_result(
        &self,
        session_id: &str,
        trigger_reason: &str,
        quick_stats: &str,
        full_analysis: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO retrospective_results (session_id, trigger_reason, quick_stats, full_analysis, created_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (session_id) DO UPDATE SET
                trigger_reason = EXCLUDED.trigger_reason,
                quick_stats = EXCLUDED.quick_stats,
                full_analysis = EXCLUDED.full_analysis,
                created_at = NOW()"
        )
        .bind(session_id)
        .bind(trigger_reason)
        .bind(quick_stats)
        .bind(full_analysis)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn has_retrospective_result(&self, session_id: &str) -> DbResult<bool> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM retrospective_results WHERE session_id = $1")
                .bind(session_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count > 0)
    }

    async fn get_sessions_needing_retrospective(&self) -> DbResult<Vec<(String, i64, i64, f64)>> {
        let rows = sqlx::query(
            "SELECT c.id, c.message_count,
                    COALESCE((SELECT COUNT(*) FROM conversation_tool_calls tc WHERE tc.session_id = c.id), 0) as tool_count,
                    COALESCE(
                        (SELECT (100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0))::float8
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    )::float8 as error_rate
             FROM conversations c
             WHERE c.status = 'completed'
               AND c.conversation_type = 'user'
               AND (c.slot_id IS NULL OR (
                   c.slot_id NOT LIKE 'slot-memory%'
                   AND c.slot_id NOT LIKE 'slot-diagnosis%'
                   AND c.slot_id NOT LIKE 'agent-%'
               ))
               AND c.id NOT IN (SELECT session_id FROM retrospective_results)
               AND (
                   c.message_count > 100
                   OR (SELECT COUNT(*) FROM conversation_tool_calls tc3 WHERE tc3.session_id = c.id) > 50
                   OR EXTRACT(EPOCH FROM (c.ended_at::timestamp - c.started_at::timestamp)) / 60 > 60
               )
             ORDER BY c.ended_at DESC
             LIMIT 5"
        )
        .fetch_all(&self.pool)
        .await?;

        let results = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("id"),
                    row.get::<i64, _>("message_count"),
                    row.get::<i64, _>("tool_count"),
                    row.get::<f64, _>("error_rate"),
                )
            })
            .collect();
        Ok(results)
    }

    async fn get_sessions_for_retro_backfill(
        &self,
        since: &str,
        force: bool,
    ) -> DbResult<Vec<(String, i64, i64, f64)>> {
        let sql = if force {
            "SELECT c.id, c.message_count,
                    COALESCE((SELECT COUNT(*) FROM conversation_tool_calls tc WHERE tc.session_id = c.id), 0) as tool_count,
                    COALESCE(
                        (SELECT (100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0))::float8
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    )::float8 as error_rate
             FROM conversations c
             WHERE c.conversation_type = 'user'
               AND (c.slot_id IS NULL OR (
                   c.slot_id NOT LIKE 'slot-memory%'
                   AND c.slot_id NOT LIKE 'slot-diagnosis%'
                   AND c.slot_id NOT LIKE 'agent-%'
               ))
               AND c.message_count >= 6
               AND c.started_at >= $1
             ORDER BY c.started_at ASC"
        } else {
            "SELECT c.id, c.message_count,
                    COALESCE((SELECT COUNT(*) FROM conversation_tool_calls tc WHERE tc.session_id = c.id), 0) as tool_count,
                    COALESCE(
                        (SELECT (100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0))::float8
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    )::float8 as error_rate
             FROM conversations c
             WHERE c.conversation_type = 'user'
               AND (c.slot_id IS NULL OR (
                   c.slot_id NOT LIKE 'slot-memory%'
                   AND c.slot_id NOT LIKE 'slot-diagnosis%'
                   AND c.slot_id NOT LIKE 'agent-%'
               ))
               AND c.message_count >= 6
               AND c.started_at >= $1
               AND c.id NOT IN (SELECT session_id FROM retrospective_results)
             ORDER BY c.started_at ASC"
        };

        let rows = sqlx::query(sql).bind(since).fetch_all(&self.pool).await?;

        let results = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("id"),
                    row.get::<i64, _>("message_count"),
                    row.get::<i64, _>("tool_count"),
                    row.get::<f64, _>("error_rate"),
                )
            })
            .collect();
        Ok(results)
    }

    async fn list_retrospective_results(
        &self,
        limit: i64,
    ) -> DbResult<Vec<(String, String, String, Option<String>, String)>> {
        let rows = sqlx::query(
            "SELECT session_id, trigger_reason, quick_stats, full_analysis, created_at
             FROM retrospective_results ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let results = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("session_id"),
                    row.get::<String, _>("trigger_reason"),
                    row.get::<String, _>("quick_stats"),
                    row.get::<Option<String>, _>("full_analysis"),
                    row.get::<String, _>("created_at"),
                )
            })
            .collect();
        Ok(results)
    }

    async fn get_retrospective_result(
        &self,
        session_id: &str,
    ) -> DbResult<Option<(String, String, Option<String>, String)>> {
        let row = sqlx::query(
            "SELECT trigger_reason, quick_stats, full_analysis, created_at
             FROM retrospective_results WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            (
                r.get::<String, _>("trigger_reason"),
                r.get::<String, _>("quick_stats"),
                r.get::<Option<String>, _>("full_analysis"),
                r.get::<String, _>("created_at"),
            )
        }))
    }

    // -- narration.rs removed in v0.4.23 Phase 6 (tables dropped, worker deleted) --
}

#[cfg(test)]
mod tests {
    use super::{task_scoped_order_clause, task_scoped_type_clause, COMPACTION_FRAGMENTS_QUERY};

    #[test]
    fn task_scoped_query_without_type_includes_provider_conversations() {
        assert_eq!(
            task_scoped_type_clause(None, " AND conversation_type IN ('user', 'worker')"),
            ""
        );
        assert_eq!(
            task_scoped_type_clause(Some("all"), " AND conversation_type = 'user'"),
            ""
        );
    }

    #[test]
    fn task_scoped_query_keeps_explicit_type_filters() {
        let clause = " AND conversation_type = 'gemini_chat'";
        assert_eq!(task_scoped_type_clause(Some("gemini"), clause), clause);
    }

    #[test]
    fn task_scoped_query_prefers_direct_binding_then_latest() {
        let clause = task_scoped_order_clause();
        assert!(clause.contains("CASE WHEN task_id = $1 THEN 0 ELSE 1 END"));
        assert!(clause.contains("started_at DESC"));
    }

    #[test]
    fn compaction_fragment_query_coalesces_nullable_legacy_fields() {
        assert!(COMPACTION_FRAGMENTS_QUERY.contains("COALESCE(started_at, '') AS started_at"));
        assert!(COMPACTION_FRAGMENTS_QUERY.contains("COALESCE(message_count, 0) AS message_count"));
        assert!(COMPACTION_FRAGMENTS_QUERY.contains("NULLS LAST"));
    }
}
