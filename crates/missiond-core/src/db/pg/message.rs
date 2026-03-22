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
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::timestamptz, $9, $10, $11, $12, $13, $14, $15, $16)
             ON CONFLICT (message_uuid) DO NOTHING
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
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8::timestamptz, $9, $10, $11, $12, $13, $14, $15, $16)
                 ON CONFLICT (message_uuid) DO NOTHING
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

    async fn check_message_uuids_exist(&self, uuids: &[&str]) -> DbResult<std::collections::HashSet<String>> {
        if uuids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        // Build parameterized IN clause
        let placeholders: Vec<String> = (1..=uuids.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "SELECT message_uuid FROM conversation_messages WHERE message_uuid IN ({})",
            placeholders.join(", ")
        );
        let mut query = sqlx::query_scalar::<_, String>(&sql);
        for uuid in uuids {
            query = query.bind(uuid);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().collect())
    }

    async fn get_conversation_message_by_id(&self, id: i64) -> DbResult<Option<ConversationMessage>> {
        let row = sqlx::query(
            "SELECT id, session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = id AND l.label = 'semantic_role'), role) as role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
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
                "SELECT id, session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = id AND l.label = 'semantic_role'), role) as role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                        tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count,
                        ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY timestamp, id) AS seq
                 FROM conversation_messages WHERE session_id = $1 AND id > $2 ORDER BY timestamp ASC, id ASC LIMIT $3"
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
                    SELECT id, session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = id AND l.label = 'semantic_role'), role) as role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                           tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count,
                           ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY timestamp, id) AS seq
                    FROM conversation_messages WHERE session_id = $1 ORDER BY timestamp DESC, id DESC LIMIT $2
                ) sub ORDER BY timestamp ASC, id ASC"
            )
            .bind(session_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.iter().map(Self::row_to_enriched_message).collect())
    }

    async fn get_messages_around(&self, message_id: i64, before: i64, after: i64) -> DbResult<Option<(String, Vec<ConversationMessage>)>> {
        // Resolve session_id + timestamp from anchor message
        let anchor_row: Option<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT session_id, timestamp FROM conversation_messages WHERE id = $1"
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        let (session_id, anchor_ts) = match anchor_row {
            Some(r) => r,
            None => return Ok(None),
        };

        let rows = sqlx::query(
            "WITH before_msgs AS (
                SELECT id, session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = id AND l.label = 'semantic_role'), role) as role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                       tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
                FROM conversation_messages
                WHERE session_id = $1 AND (timestamp < $5 OR (timestamp = $5 AND id < $2))
                ORDER BY timestamp DESC, id DESC LIMIT $3
            ),
            after_msgs AS (
                SELECT id, session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = id AND l.label = 'semantic_role'), role) as role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                       tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
                FROM conversation_messages
                WHERE session_id = $1 AND (timestamp > $5 OR (timestamp = $5 AND id > $2))
                ORDER BY timestamp ASC, id ASC LIMIT $4
            ),
            anchor AS (
                SELECT id, session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = id AND l.label = 'semantic_role'), role) as role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                       tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
                FROM conversation_messages WHERE id = $2
            )
            SELECT * FROM (
                SELECT * FROM before_msgs
                UNION ALL
                SELECT * FROM anchor
                UNION ALL
                SELECT * FROM after_msgs
            ) combined ORDER BY timestamp ASC, id ASC"
        )
        .bind(&session_id)
        .bind(message_id)
        .bind(before)
        .bind(after)
        .bind(anchor_ts)
        .fetch_all(&self.pool)
        .await?;

        let msgs: Vec<ConversationMessage> = rows.iter().map(Self::row_to_conversation_message).collect();
        Ok(Some((session_id, msgs)))
    }

    async fn search_conversation_messages(&self, query: &str, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        // Phase 1: PostgreSQL FTS using plainto_tsquery
        let rows = sqlx::query(
            "SELECT m.id, m.session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = m.id AND l.label = 'semantic_role'), m.role) as role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
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
            "SELECT m.id, m.session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = m.id AND l.label = 'semantic_role'), m.role) as role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
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
            conditions.push(format!("m.timestamp >= ${}::timestamptz", param_idx));
            bind_values.push(ta.to_string());
        }

        let where_clause = conditions.join(" AND ");

        // Phase 1: FTS search
        let fts_sql = format!(
            "SELECT m.id, m.session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = m.id AND l.label = 'semantic_role'), m.role) as role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
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
            "SELECT m.id, m.session_id, COALESCE((SELECT value FROM message_labels l WHERE l.message_id = m.id AND l.label = 'semantic_role'), m.role) as role, m.content, m.raw_content, m.message_uuid, m.parent_uuid,
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

    async fn search_conversation_sessions_fts_filtered(&self, query: &str, limit: i64, time_after: Option<&str>, project: Option<&str>, conversation_type: Option<&str>) -> DbResult<Vec<(String, f64)>> {
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
        // conversation_type: supports comma-separated for IN clause (e.g. "gemini_chat,router_chat")
        if let Some(ct) = conversation_type {
            let types: Vec<&str> = ct.split(',').map(|s| s.trim()).collect();
            if types.len() == 1 {
                param_idx += 1;
                conditions.push(format!("c.conversation_type = ${}", param_idx));
                extra_vals.push(types[0].to_string());
            } else {
                let placeholders: Vec<String> = types.iter().map(|t| {
                    param_idx += 1;
                    extra_vals.push(t.to_string());
                    format!("${}", param_idx)
                }).collect();
                conditions.push(format!("c.conversation_type IN ({})", placeholders.join(",")));
            }
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

        // ILIKE fallback (leverages gin_trgm_ops index for Chinese text)
        let pattern = format!("%{}%", query);
        let mut like_conditions = vec![
            "m.content ILIKE $1".to_string(),
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
        if let Some(ct) = conversation_type {
            let types: Vec<&str> = ct.split(',').map(|s| s.trim()).collect();
            if types.len() == 1 {
                like_idx += 1;
                like_conditions.push(format!("c.conversation_type = ${}", like_idx));
                like_vals.push(types[0].to_string());
            } else {
                let placeholders: Vec<String> = types.iter().map(|t| {
                    like_idx += 1;
                    like_vals.push(t.to_string());
                    format!("${}", like_idx)
                }).collect();
                like_conditions.push(format!("c.conversation_type IN ({})", placeholders.join(",")));
            }
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

    async fn search_conversations_by_metadata(&self, query: &str, limit: i64, conversation_type: Option<&str>) -> DbResult<Vec<(String, f64)>> {
        let pattern = format!("%{}%", query);
        let mut extra_vals: Vec<String> = Vec::new();
        let mut param_idx = 2u32; // $1 = pattern, $2 = limit
        let ct_clause = if let Some(ct) = conversation_type {
            let types: Vec<&str> = ct.split(',').map(|s| s.trim()).collect();
            if types.len() == 1 {
                param_idx += 1;
                extra_vals.push(types[0].to_string());
                format!("AND conversation_type = ${}", param_idx)
            } else {
                let placeholders: Vec<String> = types.iter().map(|t| {
                    param_idx += 1;
                    extra_vals.push(t.to_string());
                    format!("${}", param_idx)
                }).collect();
                format!("AND conversation_type IN ({})", placeholders.join(","))
            }
        } else {
            String::new()
        };
        let sql = format!(
            "SELECT id, 1.0::float8 as score FROM conversations
             WHERE conversation_type NOT IN ('meta', 'compaction')
               AND (id::text ILIKE $1 OR task_id ILIKE $1 OR llm_summary ILIKE $1)
               {}
             ORDER BY started_at DESC LIMIT $2",
            ct_clause
        );
        let mut q = sqlx::query_as::<_, (String, f64)>(&sql)
            .bind(&pattern)
            .bind(limit);
        for val in &extra_vals {
            q = q.bind(val);
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    async fn get_user_message_index(&self, session_id: &str) -> DbResult<Vec<(i64, String, String)>> {
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT id, to_char(timestamp, 'HH24:MI:SS') as ts, LEFT(content, 50) as preview
             FROM conversation_messages
             WHERE session_id = $1 AND role = 'user'
             ORDER BY id ASC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
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

    // -- pgvector hybrid search (P3: message-level embedding) --

    async fn hybrid_message_search(&self, query_vec: &[f32], query_text: &str, session_id: Option<&str>, limit: i64) -> DbResult<Vec<(i64, String, String, String, String, f64)>> {
        let vec_lit = vec_to_pg_literal(query_vec);
        let pool_size = limit * 3; // recall pool

        // Session-scoped: B-Tree exact distance (NOT HNSW — anti-pattern with WHERE filter)
        if let Some(sid) = session_id {
            let sql = format!(
                "WITH vec_top AS (
                    SELECT me.message_id,
                           (me.embedding_vec <=> '{vec}'::halfvec) AS distance
                    FROM message_embeddings me
                    WHERE me.session_id = $3
                    ORDER BY distance
                    LIMIT {pool}
                ),
                vec_ranked AS (
                    SELECT message_id, ROW_NUMBER() OVER (ORDER BY distance) AS vec_rank
                    FROM vec_top
                ),
                fts_top AS (
                    SELECT id AS message_id,
                           ts_rank(fts_content, plainto_tsquery('simple', $1)) AS score
                    FROM conversation_messages
                    WHERE fts_content @@ plainto_tsquery('simple', $1)
                      AND session_id = $3
                    ORDER BY score DESC
                    LIMIT {pool}
                ),
                fts_ranked AS (
                    SELECT message_id, ROW_NUMBER() OVER (ORDER BY score DESC) AS fts_rank
                    FROM fts_top
                )
                SELECT cm.id, cm.session_id, cm.role, cm.content, cm.timestamp,
                       (COALESCE(0.6 / (60.0 + v.vec_rank), 0) + COALESCE(0.4 / (60.0 + f.fts_rank), 0))::float8 AS rrf_score
                FROM vec_ranked v
                FULL OUTER JOIN fts_ranked f ON v.message_id = f.message_id
                JOIN conversation_messages cm ON cm.id = COALESCE(v.message_id, f.message_id)
                ORDER BY rrf_score DESC
                LIMIT $2",
                vec = vec_lit, pool = pool_size
            );
            let rows = sqlx::query_as::<_, (i64, String, String, String, String, f64)>(&sql)
                .bind(query_text)
                .bind(limit)
                .bind(sid)
                .fetch_all(&self.pool)
                .await?;
            return Ok(rows);
        }

        // Global: HNSW recall → RRF fusion with FTS (two-step CTE)
        let sql = format!(
            "WITH vec_top AS (
                SELECT me.message_id, me.embedding_vec <=> '{vec}'::halfvec AS distance
                FROM message_embeddings me
                ORDER BY me.embedding_vec <=> '{vec}'::halfvec
                LIMIT {pool}
            ),
            vec_ranked AS (
                SELECT message_id, ROW_NUMBER() OVER (ORDER BY distance) AS vec_rank
                FROM vec_top
            ),
            fts_top AS (
                SELECT id AS message_id,
                       ts_rank(fts_content, plainto_tsquery('simple', $1)) AS score
                FROM conversation_messages
                WHERE fts_content @@ plainto_tsquery('simple', $1)
                ORDER BY score DESC
                LIMIT {pool}
            ),
            fts_ranked AS (
                SELECT message_id, ROW_NUMBER() OVER (ORDER BY score DESC) AS fts_rank
                FROM fts_top
            )
            SELECT cm.id, cm.session_id, cm.role, cm.content, cm.timestamp,
                   (COALESCE(0.6 / (60.0 + v.vec_rank), 0) + COALESCE(0.4 / (60.0 + f.fts_rank), 0))::float8 AS rrf_score
            FROM vec_ranked v
            FULL OUTER JOIN fts_ranked f ON v.message_id = f.message_id
            JOIN conversation_messages cm ON cm.id = COALESCE(v.message_id, f.message_id)
            ORDER BY rrf_score DESC
            LIMIT $2",
            vec = vec_lit, pool = pool_size
        );
        let rows = sqlx::query_as::<_, (i64, String, String, String, String, f64)>(&sql)
            .bind(query_text)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn session_semantic_search(&self, query_vec: &[f32], session_id: &str, limit: i64) -> DbResult<Vec<(i64, String, String, String, f64)>> {
        let vec_lit = vec_to_pg_literal(query_vec);
        // B-Tree filter on session_id → exact cosine distance (no HNSW needed)
        let sql = format!(
            "SELECT cm.id, cm.role, cm.content, cm.timestamp,
                    (1.0 - (me.embedding_vec <=> '{vec}'::halfvec))::float8 AS similarity
             FROM message_embeddings me
             JOIN conversation_messages cm ON cm.id = me.message_id
             WHERE me.session_id = $1
             ORDER BY me.embedding_vec <=> '{vec}'::halfvec
             LIMIT $2",
            vec = vec_lit
        );
        let rows = sqlx::query_as::<_, (i64, String, String, String, f64)>(&sql)
            .bind(session_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn semantic_conversation_search(&self, query_vec: &[f32], limit: i64) -> DbResult<Vec<(String, f64)>> {
        let vec_lit = vec_to_pg_literal(query_vec);
        // HNSW recall Top-N topics → group by session_id (small set, safe to GROUP BY)
        let pool_size = limit * 5;
        let sql = format!(
            "WITH top_topics AS (
                SELECT session_id,
                       (1.0 - (embedding_vec <=> '{vec}'::vector))::float8 AS similarity
                FROM conversation_topic_vectors
                WHERE embedding_vec IS NOT NULL
                ORDER BY embedding_vec <=> '{vec}'::vector
                LIMIT {pool}
            )
            SELECT session_id, MAX(similarity)::float8 AS max_sim
            FROM top_topics
            GROUP BY session_id
            ORDER BY max_sim DESC
            LIMIT $1",
            vec = vec_lit, pool = pool_size
        );
        let rows = sqlx::query_as::<_, (String, f64)>(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }
}

/// Zero-allocation pgvector literal serializer (shared with conversation.rs).
fn vec_to_pg_literal(v: &[f32]) -> String {
    use std::fmt::Write;
    let mut buf = String::with_capacity(v.len() * 14 + 2);
    buf.push('[');
    for (i, f) in v.iter().enumerate() {
        if i > 0 { buf.push(','); }
        let _ = write!(buf, "{}", f);
    }
    buf.push(']');
    buf
}
