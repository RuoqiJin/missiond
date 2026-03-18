//! MessageStore — PostgreSQL implementation.

use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::MessageStore;
use crate::types::*;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
#[async_trait]
impl MessageStore for PgMissionStore {
    async fn insert_conversation_message(&self, msg: &ConversationMessage) -> DbResult<i64> {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO conversation_messages (session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata, tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
             ON CONFLICT DO NOTHING
             RETURNING id"
        )
        .bind(&msg.session_id)
        .bind(&msg.role)
        .bind(&msg.content)
        .bind(&msg.raw_content)
        .bind(&msg.message_uuid)
        .bind(&msg.parent_uuid)
        .bind(&msg.model)
        .bind(&msg.timestamp)
        .bind(&msg.metadata)
        .bind(&msg.tool_name)
        .bind(&msg.raw_role)
        .bind(&msg.content_types)
        .bind(msg.has_image)
        .bind(msg.has_tool_use)
        .bind(msg.has_tool_result)
        .bind(msg.token_count)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or((0,));
        Ok(row.0)
    }

    async fn insert_conversation_messages_batch(&self, messages: &[ConversationMessage]) -> DbResult<Vec<i64>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        let mut inserted_ids = Vec::new();
        for msg in messages {
            let row: Option<(i64,)> = sqlx::query_as(
                "INSERT INTO conversation_messages (session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata, tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
                 ON CONFLICT DO NOTHING
                 RETURNING id"
            )
            .bind(&msg.session_id)
            .bind(&msg.role)
            .bind(&msg.content)
            .bind(&msg.raw_content)
            .bind(&msg.message_uuid)
            .bind(&msg.parent_uuid)
            .bind(&msg.model)
            .bind(&msg.timestamp)
            .bind(&msg.metadata)
            .bind(&msg.tool_name)
            .bind(&msg.raw_role)
            .bind(&msg.content_types)
            .bind(msg.has_image)
            .bind(msg.has_tool_use)
            .bind(msg.has_tool_result)
            .bind(msg.token_count)
            .fetch_optional(&self.pool)
            .await?;
            if let Some((id,)) = row {
                inserted_ids.push(id);
            }
        }
        Ok(inserted_ids)
    }

    async fn get_conversation_message_by_id(&self, id: i64) -> DbResult<Option<ConversationMessage>> {
        let row = sqlx::query(
            "SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                    tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
             FROM conversation_messages WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(Self::row_to_conversation_message))
    }

    async fn get_conversation_messages(&self, session_id: &str, since_id: Option<i64>, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        let rows = if let Some(since) = since_id {
            sqlx::query(
                "SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                        tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count,
                        ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY id) AS seq
                 FROM conversation_messages WHERE session_id = $1 AND id > $2 ORDER BY id ASC LIMIT $3"
            )
            .bind(session_id)
            .bind(since)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            // Return last N messages (subquery to reverse order, preserving seq)
            sqlx::query(
                "SELECT * FROM (
                    SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                           tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count,
                           ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY id) AS seq
                    FROM conversation_messages WHERE session_id = $1 ORDER BY id DESC LIMIT $2
                ) sub ORDER BY id ASC"
            )
            .bind(session_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.iter().map(Self::row_to_enriched_message).collect())
    }

    async fn get_messages_around(&self, message_id: i64, before: i64, after: i64) -> DbResult<Option<(String, Vec<ConversationMessage>)>> {
        // Resolve session_id from anchor message
        let session_row: Option<(String,)> = sqlx::query_as(
            "SELECT session_id FROM conversation_messages WHERE id = $1"
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        let session_id = match session_row {
            Some((sid,)) => sid,
            None => return Ok(None),
        };

        let rows = sqlx::query(
            "WITH before_msgs AS (
                SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                       tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
                FROM conversation_messages
                WHERE session_id = $1 AND id < $2
                ORDER BY id DESC LIMIT $3
            ),
            after_msgs AS (
                SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                       tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
                FROM conversation_messages
                WHERE session_id = $1 AND id > $2
                ORDER BY id ASC LIMIT $4
            ),
            anchor AS (
                SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                       tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
                FROM conversation_messages WHERE id = $2
            )
            SELECT * FROM (
                SELECT * FROM before_msgs
                UNION ALL
                SELECT * FROM anchor
                UNION ALL
                SELECT * FROM after_msgs
            ) combined ORDER BY id ASC"
        )
        .bind(&session_id)
        .bind(message_id)
        .bind(before)
        .bind(after)
        .fetch_all(&self.pool)
        .await?;

        let msgs: Vec<ConversationMessage> = rows.iter().map(Self::row_to_conversation_message).collect();
        Ok(Some((session_id, msgs)))
    }

    async fn search_conversation_messages(&self, query: &str, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        // Phase 1: PostgreSQL FTS using plainto_tsquery
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
                    m.model, m.timestamp, m.metadata, m.tool_name, m.raw_role, m.content_types,
                    m.has_image, m.has_tool_use, m.has_tool_result, m.token_count
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.fts_content @@ plainto_tsquery('simple', $1)
               AND c.conversation_type NOT IN ('meta', 'compaction')
             ORDER BY ts_rank(m.fts_content, plainto_tsquery('simple', $1)) DESC
             LIMIT $2"
        )
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        if !rows.is_empty() {
            return Ok(rows.iter().map(Self::row_to_conversation_message).collect());
        }

        // Phase 2: LIKE fallback for Chinese substrings / non-FTS matches
        let pattern = format!("%{}%", query);
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
                    m.model, m.timestamp, m.metadata, m.tool_name, m.raw_role, m.content_types,
                    m.has_image, m.has_tool_use, m.has_tool_result, m.token_count
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.content LIKE $1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             ORDER BY m.timestamp DESC LIMIT $2"
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_conversation_message).collect())
    }

    async fn search_messages_filtered(&self, query: &str, session_id: Option<&str>, role: Option<&str>, tool_name: Option<&str>, time_after: Option<&str>, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        // Build dynamic WHERE conditions
        let mut conditions = vec![
            "c.conversation_type NOT IN ('meta', 'compaction')".to_string(),
        ];
        let mut param_idx = 2u32; // $1 = query, $2 = limit

        let mut bind_values: Vec<String> = Vec::new();

        if let Some(sid) = session_id {
            param_idx += 1;
            conditions.push(format!("m.session_id = ${}", param_idx));
            bind_values.push(sid.to_string());
        }
        if let Some(r) = role {
            param_idx += 1;
            conditions.push(format!("m.role = ${}", param_idx));
            bind_values.push(r.to_string());
        }
        if let Some(tn) = tool_name {
            param_idx += 1;
            // Match tool name in comma-separated list
            conditions.push(format!("(',' || m.tool_name || ',') LIKE ${}", param_idx));
            bind_values.push(format!("%,{},%", tn));
        }
        if let Some(ta) = time_after {
            param_idx += 1;
            conditions.push(format!("m.timestamp >= ${}", param_idx));
            bind_values.push(ta.to_string());
        }

        let where_clause = conditions.join(" AND ");

        // Phase 1: FTS search
        let fts_sql = format!(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
                    m.model, m.timestamp, m.metadata, m.tool_name, m.raw_role, m.content_types,
                    m.has_image, m.has_tool_use, m.has_tool_result, m.token_count
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.fts_content @@ plainto_tsquery('simple', $1)
               AND {}
             ORDER BY ts_rank(m.fts_content, plainto_tsquery('simple', $1)) DESC
             LIMIT $2",
            where_clause
        );

        // Build and execute the query with dynamic bindings
        let mut q = sqlx::query(&fts_sql).bind(query).bind(limit);
        for val in &bind_values {
            q = q.bind(val);
        }
        let rows = q.fetch_all(&self.pool).await?;
        if !rows.is_empty() {
            return Ok(rows.iter().map(Self::row_to_conversation_message).collect());
        }

        // Phase 2: LIKE fallback
        let pattern = format!("%{}%", query);
        let like_sql = format!(
            "SELECT m.id, m.session_id, m.role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
                    m.model, m.timestamp, m.metadata, m.tool_name, m.raw_role, m.content_types,
                    m.has_image, m.has_tool_use, m.has_tool_result, m.token_count
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.content LIKE $1
               AND {}
             ORDER BY m.timestamp DESC LIMIT $2",
            where_clause
        );

        let mut q = sqlx::query(&like_sql).bind(&pattern).bind(limit);
        for val in &bind_values {
            q = q.bind(val);
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.iter().map(Self::row_to_conversation_message).collect())
    }

    async fn search_conversation_sessions_fts(&self, query: &str, limit: i64) -> DbResult<Vec<(String, f64)>> {
        // Phase 1: PostgreSQL FTS grouped by session
        let rows: Vec<(String, f64)> = sqlx::query_as(
            "SELECT m.session_id, MAX(ts_rank(m.fts_content, plainto_tsquery('simple', $1)))::float8 as best_score
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.fts_content @@ plainto_tsquery('simple', $1)
               AND c.conversation_type NOT IN ('meta', 'compaction')
             GROUP BY m.session_id
             ORDER BY best_score DESC
             LIMIT $2"
        )
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        if !rows.is_empty() {
            return Ok(rows);
        }

        // Phase 2: LIKE fallback (count-based scoring)
        let pattern = format!("%{}%", query);
        let rows: Vec<(String, f64)> = sqlx::query_as(
            "SELECT m.session_id, COUNT(*)::float8 as hits
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.content LIKE $1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             GROUP BY m.session_id
             ORDER BY hits DESC
             LIMIT $2"
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_session_fts_snippets(&self, session_id: &str, query: &str, limit: usize) -> DbResult<Vec<(String, String)>> {
        // PostgreSQL: use ts_headline for snippet extraction
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT m.role,
                    ts_headline('simple', m.content, plainto_tsquery('simple', $1),
                        'StartSel=**, StopSel=**, MaxFragments=1, MaxWords=48, MinWords=12') as snip
             FROM conversation_messages m
             WHERE m.fts_content @@ plainto_tsquery('simple', $1) AND m.session_id = $2
             ORDER BY ts_rank(m.fts_content, plainto_tsquery('simple', $1)) DESC
             LIMIT $3"
        )
        .bind(query)
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        if !rows.is_empty() {
            return Ok(rows);
        }

        // LIKE fallback — return truncated content as snippet
        let pattern = format!("%{}%", query);
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT role, LEFT(content, 200) as snip
             FROM conversation_messages
             WHERE content LIKE $1 AND session_id = $2
             ORDER BY timestamp DESC
             LIMIT $3"
        )
        .bind(&pattern)
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn search_conversation_sessions_fts_filtered(&self, query: &str, limit: i64, time_after: Option<&str>, project: Option<&str>) -> DbResult<Vec<(String, f64)>> {
        let mut conditions = vec![
            "m.fts_content @@ plainto_tsquery('simple', $1)".to_string(),
            "c.conversation_type NOT IN ('meta', 'compaction')".to_string(),
        ];
        let mut param_idx = 2u32; // $1 = query, $2 = limit

        let mut extra_vals: Vec<String> = Vec::new();

        if let Some(ta) = time_after {
            param_idx += 1;
            conditions.push(format!("c.started_at >= ${}", param_idx));
            extra_vals.push(ta.to_string());
        }
        if let Some(p) = project {
            param_idx += 1;
            conditions.push(format!("c.project = ${}", param_idx));
            extra_vals.push(p.to_string());
        }

        let where_clause = conditions.join(" AND ");
        let fts_sql = format!(
            "SELECT m.session_id, MAX(ts_rank(m.fts_content, plainto_tsquery('simple', $1)))::float8 as best_score
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE {}
             GROUP BY m.session_id
             ORDER BY best_score DESC
             LIMIT $2",
            where_clause
        );

        let mut q = sqlx::query_as::<_, (String, f64)>(&fts_sql)
            .bind(query)
            .bind(limit);
        for val in &extra_vals {
            q = q.bind(val);
        }
        let rows = q.fetch_all(&self.pool).await?;
        if !rows.is_empty() {
            return Ok(rows);
        }

        // LIKE fallback
        let pattern = format!("%{}%", query);
        let mut like_conditions = vec![
            "m.content LIKE $1".to_string(),
            "c.conversation_type NOT IN ('meta', 'compaction')".to_string(),
        ];
        let mut like_idx = 2u32;
        let mut like_vals: Vec<String> = Vec::new();

        if let Some(ta) = time_after {
            like_idx += 1;
            like_conditions.push(format!("c.started_at >= ${}", like_idx));
            like_vals.push(ta.to_string());
        }
        if let Some(p) = project {
            like_idx += 1;
            like_conditions.push(format!("c.project = ${}", like_idx));
            like_vals.push(p.to_string());
        }

        let like_sql = format!(
            "SELECT m.session_id, COUNT(*)::float8 as hits
             FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE {}
             GROUP BY m.session_id
             ORDER BY hits DESC LIMIT $2",
            like_conditions.join(" AND ")
        );

        let mut q = sqlx::query_as::<_, (String, f64)>(&like_sql)
            .bind(&pattern)
            .bind(limit);
        for val in &like_vals {
            q = q.bind(val);
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    async fn get_first_user_message(&self, session_id: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT content FROM conversation_messages
             WHERE session_id = $1 AND role = 'user'
             ORDER BY id ASC LIMIT 1"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }
}
