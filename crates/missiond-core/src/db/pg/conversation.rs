//! ConversationStore — PostgreSQL implementation.

use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::ConversationStore;
use crate::types::*;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
impl PgMissionStore {
    /// Extract a Conversation from a sqlx::PgRow.
    fn row_to_conversation(row: &sqlx::postgres::PgRow) -> Conversation {
        use sqlx::Row;
        Conversation {
            id: row.get("id"),
            project: row.get("project"),
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
            conversation_type: row.try_get("conversation_type").unwrap_or_else(|_| "user".to_string()),
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
            timestamp: row.get("timestamp"),
            metadata: row.get("metadata"),
            tool_name: row.get("tool_name"),
            raw_role: row.get("raw_role"),
            content_types: row.get("content_types"),
            has_image: row.get::<bool, _>("has_image"),
            has_tool_use: row.get::<bool, _>("has_tool_use"),
            has_tool_result: row.get::<bool, _>("has_tool_result"),
            token_count: row.get("token_count"),
        }
    }

    /// Parse relative time strings (e.g. "30min", "2h", "7d") to absolute timestamps.
    fn parse_relative_time(s: &str) -> String {
        let s = s.trim();
        if let Some(mins) = s.strip_suffix("min").and_then(|v| v.parse::<i64>().ok()) {
            return format!("{}", (chrono::Utc::now() - chrono::Duration::minutes(mins)).format("%Y-%m-%dT%H:%M:%S"));
        }
        if let Some(hours) = s.strip_suffix('h').and_then(|v| v.parse::<i64>().ok()) {
            return format!("{}", (chrono::Utc::now() - chrono::Duration::hours(hours)).format("%Y-%m-%dT%H:%M:%S"));
        }
        if let Some(days) = s.strip_suffix('d').and_then(|v| v.parse::<i64>().ok()) {
            return format!("{}", (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%dT%H:%M:%S"));
        }
        if s.contains('T') {
            return s.replace('T', " ").chars().take(19).collect();
        }
        s.to_string()
    }

    fn parse_since(s: &str) -> String {
        let ts = Self::parse_relative_time(s);
        if ts.len() == 10 { format!("{}T00:00:00", ts) } else { ts.replace(' ', "T") }
    }

    fn parse_until(s: &str) -> String {
        let ts = Self::parse_relative_time(s);
        if ts.len() == 10 { format!("{}T23:59:59", ts) } else { ts.replace(' ', "T") }
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl ConversationStore for PgMissionStore {
    // -- session CRUD --

    async fn upsert_conversation(&self, conv: &Conversation) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO conversations (id, project, slot_id, source, model, git_branch, jsonl_path, parent_session_id, task_id, message_count, started_at, ended_at, status, analyzed_at, conversation_type)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (id) DO UPDATE SET
                slot_id = COALESCE($3, conversations.slot_id),
                model = COALESCE($5, conversations.model),
                git_branch = COALESCE($6, conversations.git_branch),
                message_count = $10,
                ended_at = $12,
                status = $13,
                conversation_type = COALESCE($15, conversations.conversation_type)"
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

    async fn list_conversations(&self, status: Option<&str>, limit: i64, conv_type: Option<&str>, task_id: Option<&str>, since: Option<&str>, until: Option<&str>, source: Option<&str>) -> DbResult<Vec<Conversation>> {
        let source_clause = match source {
            Some(s) => format!(" AND source = '{}'", s.replace('\'', "''")),
            None => String::new(),
        };
        let type_clause = match conv_type {
            Some("all") => String::new(),
            Some("meta") => " AND conversation_type = 'meta'".to_string(),
            Some("worker") => " AND conversation_type = 'worker'".to_string(),
            Some("system") => " AND conversation_type IN ('meta', 'worker')".to_string(),
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
            if parts.is_empty() { String::new() } else { format!(" AND {}", parts.join(" AND ")) }
        };

        // Fast path: filter by task_id
        if let Some(tid) = task_id {
            let sql = format!(
                "SELECT * FROM conversations WHERE task_id = $1{}{}{} ORDER BY started_at ASC LIMIT $2",
                if matches!(conv_type, Some("all")) { String::new() } else { type_clause.clone() },
                source_clause, time_clause
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
            let rows = sqlx::query(&sql)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
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
            let active_rows = sqlx::query(&sql_active)
                .fetch_all(&self.pool)
                .await?;
            let mut convs: Vec<Conversation> = active_rows.iter().map(Self::row_to_conversation).collect();

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

    async fn get_child_conversations(&self, parent_session_id: &str) -> DbResult<Vec<Conversation>> {
        let rows = sqlx::query("SELECT * FROM conversations WHERE parent_session_id = $1 ORDER BY started_at ASC")
            .bind(parent_session_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    // -- deep analysis tracking --

    async fn get_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<Vec<Conversation>> {
        let rows = sqlx::query(
            "SELECT * FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND ended_at < (NOW() - INTERVAL '5 minutes')::text
               AND analysis_retries < $1
               AND (analyzed_at IS NULL OR analysis_version < $2)

             UNION ALL

             SELECT * FROM conversations
             WHERE status = 'compacted'
               AND conversation_type = 'user'
               AND analysis_retries < $1
               AND (analyzed_at IS NULL OR analysis_version < $2)

             UNION ALL

             SELECT * FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND analysis_retries < $1
               AND (SELECT COUNT(*) FROM conversation_messages m
                    WHERE m.session_id = conversations.id
                      AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                      AND m.role IN ('user', 'assistant')) >= 100

             ORDER BY started_at ASC"
        )
        .bind(max_retries)
        .bind(current_version)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    async fn has_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<bool> {
        let (exists,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(
                SELECT 1 FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND ended_at < (NOW() - INTERVAL '5 minutes')::text
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT 1 FROM conversations
                WHERE status = 'compacted'
                  AND conversation_type = 'user'
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT 1 FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND analysis_retries < $1
                  AND (SELECT COUNT(*) FROM conversation_messages m
                       WHERE m.session_id = conversations.id
                         AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                         AND m.role IN ('user', 'assistant')) >= 100
            )"
        )
        .bind(max_retries)
        .bind(current_version)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    async fn count_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM (
                SELECT id FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND ended_at < (NOW() - INTERVAL '5 minutes')::text
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT id FROM conversations
                WHERE status = 'compacted'
                  AND conversation_type = 'user'
                  AND analysis_retries < $1
                  AND (analyzed_at IS NULL OR analysis_version < $2)

                UNION ALL

                SELECT id FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND analysis_retries < $1
                  AND (SELECT COUNT(*) FROM conversation_messages m
                       WHERE m.session_id = conversations.id
                         AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                         AND m.role IN ('user', 'assistant')) >= 100
            ) sub"
        )
        .bind(max_retries)
        .bind(current_version)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn count_pending_realtime(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(DISTINCT c.id) FROM conversations c
             JOIN conversation_messages m ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
               AND m.role IN ('user', 'assistant')"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn pending_realtime_detail(&self) -> DbResult<Vec<(String, i64, String)>> {
        let rows: Vec<(String, i64, String)> = sqlx::query_as(
            "SELECT c.id, COUNT(*) as cnt, MIN(m.timestamp) as oldest
             FROM conversations c
             JOIN conversation_messages m ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
               AND m.role IN ('user', 'assistant')
             GROUP BY c.id
             ORDER BY cnt DESC
             LIMIT 20"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn pending_deep_detail(&self, current_version: i32, max_retries: i32) -> DbResult<Vec<(String, String, i32)>> {
        let rows: Vec<(String, String, i32)> = sqlx::query_as(
            "SELECT id, COALESCE(ended_at, '[active]'), analysis_retries FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND ended_at < (NOW() - INTERVAL '5 minutes')::text
               AND analysis_retries < $1
               AND (analyzed_at IS NULL OR analysis_version < $2)

             UNION ALL

             SELECT id, '[checkpoint]', analysis_retries FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND analysis_retries < $1
               AND (SELECT COUNT(*) FROM conversation_messages m
                    WHERE m.session_id = conversations.id
                      AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                      AND m.role IN ('user', 'assistant')) >= 100

             ORDER BY 2 ASC
             LIMIT 20"
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
            "UPDATE conversations SET analysis_retries = analysis_retries + 1 WHERE id = $1"
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
             LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_conversation).collect())
    }

    async fn mark_habit_scanned(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
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
               AND message_count >= 4"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn count_scannable_conversations(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations
             WHERE conversation_type = 'user'
               AND message_count >= 4"
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

    async fn set_conversation_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        sqlx::query("UPDATE conversations SET embedding = $1, embedding_provider = $2 WHERE id = $3")
            .bind(&bytes)
            .bind(provider)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn load_conversation_embeddings(&self, provider: &str) -> DbResult<Vec<(String, Vec<f32>)>> {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM conversations
             WHERE embedding IS NOT NULL AND embedding_provider = $1
               AND conversation_type NOT IN ('meta', 'compaction')"
        )
        .bind(provider)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id, blob)| {
            (id, crate::embedding::bytes_to_f32_vec(&blob))
        }).collect())
    }

    async fn conversations_missing_summary(&self, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM conversations
             WHERE llm_summary IS NULL AND status = 'completed' AND message_count >= 6
               AND conversation_type IN ('user', 'worker')
             ORDER BY started_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn conversations_stale_embedding(&self, current_provider: &str, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM conversations
             WHERE llm_summary IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != $1
               AND conversation_type IN ('user', 'worker')
             ORDER BY started_at DESC LIMIT $2"
        )
        .bind(current_provider)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    // -- topic vectors --

    async fn set_conversation_topic_vectors(&self, session_id: &str, topics: &[(String, Vec<f32>)], provider: &str) -> DbResult<()> {
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

    async fn load_conversation_topic_vectors(&self, provider: &str) -> DbResult<Vec<(String, Vec<Vec<f32>>)>> {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT tv.session_id, tv.embedding
             FROM conversation_topic_vectors tv
             JOIN conversations c ON c.id = tv.session_id
             WHERE tv.embedding_provider = $1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             ORDER BY tv.session_id, tv.chunk_idx"
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
             WHERE session_id = $1 ORDER BY chunk_idx"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn conversations_needing_topic_vectors(&self, provider: &str, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT c.id FROM conversations c
             WHERE c.llm_summary IS NOT NULL
               AND c.conversation_type IN ('user', 'worker')
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = c.id AND tv.embedding_provider = $1
               )
             ORDER BY c.started_at DESC LIMIT $2"
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
             LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_compaction_fragments(&self, parent_id: &str) -> DbResult<Vec<(String, String, i64)>> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT id, started_at, message_count FROM conversations
             WHERE parent_session_id = $1 AND conversation_type = 'compaction'
             ORDER BY started_at ASC, id ASC"
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_last_assistant_content(&self, session_id: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT content FROM conversation_messages
             WHERE session_id = $1 AND role = 'assistant'
             ORDER BY id DESC LIMIT 1"
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
               AND (SELECT MAX(m.timestamp) FROM conversation_messages m WHERE m.session_id = c.id) < $1"
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        let now = chrono::Utc::now().to_rfc3339();
        for (id,) in &rows {
            sqlx::query("UPDATE conversations SET status = 'completed', ended_at = $1 WHERE id = $2")
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

    async fn get_conversations_by_task_id(&self, task_id: &str) -> DbResult<Vec<Conversation>> {
        let rows = sqlx::query("SELECT * FROM conversations WHERE task_id = $1 ORDER BY started_at ASC")
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

    // -- extraction watermarks --

    async fn get_pending_memory_messages(&self, today: &str) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid,
                    m.parent_uuid, m.model, m.timestamp, m.metadata, m.tool_name,
                    m.raw_role, m.content_types, m.has_image, m.has_tool_use, m.has_tool_result, m.token_count,
                    COALESCE(c.project, '') as c_project
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp >= $1
               AND m.timestamp > COALESCE(c.memory_forwarded_at, $1)
               AND m.role IN ('user', 'assistant')
             ORDER BY c.started_at DESC, m.id ASC"
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

    async fn get_pending_user_voice_messages(&self) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid,
                    m.parent_uuid, m.model, m.timestamp, m.metadata, m.tool_name,
                    m.raw_role, m.content_types, m.has_image, m.has_tool_use, m.has_tool_result, m.token_count,
                    COALESCE(c.project, '') as c_project
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.user_voice_forwarded_at, c.started_at)
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

    async fn update_user_voice_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET user_voice_forwarded_at = $1 WHERE id = $2")
            .bind(timestamp)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_pending_realtime_messages(&self) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        self.get_pending_realtime_messages_with_limit(50).await
    }

    async fn get_pending_realtime_messages_with_limit(&self, limit: usize) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let rows = sqlx::query(
            "WITH ranked AS (
                SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid,
                       m.parent_uuid, m.model, m.timestamp, m.metadata, m.tool_name,
                       m.raw_role, m.content_types, m.has_image, m.has_tool_use, m.has_tool_result, m.token_count,
                       COALESCE(c.project, '') as c_project,
                       ROW_NUMBER() OVER(PARTITION BY m.session_id ORDER BY m.timestamp ASC) as rn
                FROM conversation_messages m
                JOIN conversations c ON c.id = m.session_id
                WHERE c.conversation_type = 'user'
                  AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
                  AND m.role IN ('user', 'assistant', 'tool_result')
            )
            SELECT id, session_id, role, content, raw_content, message_uuid,
                   parent_uuid, model, timestamp, metadata, tool_name,
                   raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count,
                   c_project
            FROM ranked
            WHERE rn <= 15
            ORDER BY timestamp ASC
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

    async fn update_realtime_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET realtime_forwarded_at = $1 WHERE id = $2")
            .bind(timestamp)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
