//! RetrospectiveStore — PostgreSQL implementation.

use async_trait::async_trait;
use sqlx::Row;
use crate::db::error::DbResult;
use crate::db::traits::RetrospectiveStore;
use crate::types::*;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
#[async_trait]
impl RetrospectiveStore for PgMissionStore {
    // -- audit.rs: retrospective --

    async fn save_retrospective_result(&self, session_id: &str, trigger_reason: &str, quick_stats: &str, full_analysis: Option<&str>) -> DbResult<()> {
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
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM retrospective_results WHERE session_id = $1"
        )
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
                        (SELECT 100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0)
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    ) as error_rate
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

        let results = rows.iter().map(|row| {
            (
                row.get::<String, _>("id"),
                row.get::<i64, _>("message_count"),
                row.get::<i64, _>("tool_count"),
                row.get::<f64, _>("error_rate"),
            )
        }).collect();
        Ok(results)
    }

    async fn get_sessions_for_retro_backfill(&self, since: &str, force: bool) -> DbResult<Vec<(String, i64, i64, f64)>> {
        let sql = if force {
            "SELECT c.id, c.message_count,
                    COALESCE((SELECT COUNT(*) FROM conversation_tool_calls tc WHERE tc.session_id = c.id), 0) as tool_count,
                    COALESCE(
                        (SELECT 100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0)
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    ) as error_rate
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
                        (SELECT 100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0)
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    ) as error_rate
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

        let rows = sqlx::query(sql)
            .bind(since)
            .fetch_all(&self.pool)
            .await?;

        let results = rows.iter().map(|row| {
            (
                row.get::<String, _>("id"),
                row.get::<i64, _>("message_count"),
                row.get::<i64, _>("tool_count"),
                row.get::<f64, _>("error_rate"),
            )
        }).collect();
        Ok(results)
    }

    async fn list_retrospective_results(&self, limit: i64) -> DbResult<Vec<(String, String, String, Option<String>, String)>> {
        let rows = sqlx::query(
            "SELECT session_id, trigger_reason, quick_stats, full_analysis, created_at
             FROM retrospective_results ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let results = rows.iter().map(|row| {
            (
                row.get::<String, _>("session_id"),
                row.get::<String, _>("trigger_reason"),
                row.get::<String, _>("quick_stats"),
                row.get::<Option<String>, _>("full_analysis"),
                row.get::<String, _>("created_at"),
            )
        }).collect();
        Ok(results)
    }

    async fn get_retrospective_result(&self, session_id: &str) -> DbResult<Option<(String, String, Option<String>, String)>> {
        let row = sqlx::query(
            "SELECT trigger_reason, quick_stats, full_analysis, created_at
             FROM retrospective_results WHERE session_id = $1"
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

    // -- narration.rs --

    async fn insert_narrations(&self, narrations: &[(i64, &str, &str, &str, &str)]) -> DbResult<usize> {
        let mut count = 0usize;
        for (message_id, session_id, step_title, step_intent, step_result) in narrations {
            sqlx::query(
                "INSERT INTO message_narrations (message_id, session_id, step_title, step_intent, step_result)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (message_id) DO UPDATE SET
                    session_id = EXCLUDED.session_id,
                    step_title = EXCLUDED.step_title,
                    step_intent = EXCLUDED.step_intent,
                    step_result = EXCLUDED.step_result"
            )
            .bind(message_id)
            .bind(session_id)
            .bind(step_title)
            .bind(step_intent)
            .bind(step_result)
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    async fn get_narrations_for_session(&self, session_id: &str) -> DbResult<Vec<(i64, String, String, String)>> {
        let rows = sqlx::query(
            "SELECT message_id, step_title, step_intent, step_result FROM message_narrations WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let results = rows.iter().map(|row| {
            (
                row.get::<i64, _>("message_id"),
                row.get::<String, _>("step_title"),
                row.get::<String, _>("step_intent"),
                row.get::<String, _>("step_result"),
            )
        }).collect();
        Ok(results)
    }

    async fn get_sessions_needing_narration(&self, min_unnarrated: i64) -> DbResult<Vec<(String, i64)>> {
        let rows = sqlx::query(
            "SELECT cm.session_id, COUNT(*) as unnarrated
             FROM conversation_messages cm
             JOIN conversations c ON c.id = cm.session_id
             LEFT JOIN message_narrations mn ON mn.message_id = cm.id
             WHERE mn.message_id IS NULL
               AND c.conversation_type = 'user'
               AND cm.role IN ('user', 'assistant')
             GROUP BY cm.session_id
             HAVING COUNT(*) >= $1
             ORDER BY MAX(cm.timestamp) DESC
             LIMIT 5"
        )
        .bind(min_unnarrated)
        .fetch_all(&self.pool)
        .await?;

        let results = rows.iter().map(|row| {
            (
                row.get::<String, _>("session_id"),
                row.get::<i64, _>("unnarrated"),
            )
        }).collect();
        Ok(results)
    }

    async fn get_or_create_narration_cursor(&self, session_id: &str) -> DbResult<(i64, i64, String, i64, i64)> {
        // Count total user/assistant messages for this session
        let (total,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1 AND role IN ('user', 'assistant')"
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        // Upsert cursor
        sqlx::query(
            "INSERT INTO narration_cursors (session_id, total_messages) VALUES ($1, $2)
             ON CONFLICT (session_id) DO UPDATE SET total_messages = $2"
        )
        .bind(session_id)
        .bind(total)
        .execute(&self.pool)
        .await?;

        // Fetch cursor state
        let row = sqlx::query(
            "SELECT last_processed_id, batch_index, status, retry_count, total_messages FROM narration_cursors WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok((
            row.get::<i64, _>("last_processed_id"),
            row.get::<i64, _>("batch_index"),
            row.get::<String, _>("status"),
            row.get::<i64, _>("retry_count"),
            row.get::<i64, _>("total_messages"),
        ))
    }

    async fn fetch_narration_batch(&self, session_id: &str, after_id: i64, batch_size: i64) -> DbResult<Vec<ConversationMessage>> {
        let rows = sqlx::query(
            "SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                    tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
             FROM conversation_messages
             WHERE session_id = $1 AND id > $2 AND role IN ('user', 'assistant')
             ORDER BY id ASC LIMIT $3"
        )
        .bind(session_id)
        .bind(after_id)
        .bind(batch_size)
        .fetch_all(&self.pool)
        .await?;

        let results = rows.iter().map(|row| {
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
                seq: None,
                role_display: None,
            }
        }).collect();
        Ok(results)
    }

    async fn get_last_narration(&self, session_id: &str) -> DbResult<Option<(i64, String, String, String)>> {
        let row = sqlx::query(
            "SELECT message_id, step_title, step_intent, step_result FROM message_narrations
             WHERE session_id = $1 ORDER BY message_id DESC LIMIT 1"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            (
                r.get::<i64, _>("message_id"),
                r.get::<String, _>("step_title"),
                r.get::<String, _>("step_intent"),
                r.get::<String, _>("step_result"),
            )
        }))
    }

    async fn commit_narration_batch(&self, session_id: &str, last_msg_id: i64, narrations: &[(i64, &str, &str, &str, &str)]) -> DbResult<usize> {
        // Insert narrations
        let mut count = 0usize;
        for (message_id, sid, step_title, step_intent, step_result) in narrations {
            sqlx::query(
                "INSERT INTO message_narrations (message_id, session_id, step_title, step_intent, step_result)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (message_id) DO UPDATE SET
                    session_id = EXCLUDED.session_id,
                    step_title = EXCLUDED.step_title,
                    step_intent = EXCLUDED.step_intent,
                    step_result = EXCLUDED.step_result"
            )
            .bind(message_id)
            .bind(sid)
            .bind(step_title)
            .bind(step_intent)
            .bind(step_result)
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        // Advance cursor
        sqlx::query(
            "UPDATE narration_cursors SET last_processed_id = $2, batch_index = batch_index + 1,
             status = 'idle', retry_count = 0, updated_at = NOW()
             WHERE session_id = $1"
        )
        .bind(session_id)
        .bind(last_msg_id)
        .execute(&self.pool)
        .await?;
        Ok(count)
    }

    async fn mark_narration_cursor_processing(&self, session_id: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE narration_cursors SET status = 'processing', updated_at = NOW() WHERE session_id = $1"
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_narration_cursor_failed(&self, session_id: &str, max_retries: i64) -> DbResult<bool> {
        // Increment retry count
        sqlx::query(
            "UPDATE narration_cursors SET retry_count = retry_count + 1, updated_at = NOW() WHERE session_id = $1"
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        // Check if max retries exceeded
        let (retry_count,): (i64,) = sqlx::query_as(
            "SELECT retry_count FROM narration_cursors WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        if retry_count >= max_retries {
            sqlx::query(
                "UPDATE narration_cursors SET status = 'error' WHERE session_id = $1"
            )
            .bind(session_id)
            .execute(&self.pool)
            .await?;
            Ok(true) // permanently failed
        } else {
            sqlx::query(
                "UPDATE narration_cursors SET status = 'idle' WHERE session_id = $1"
            )
            .bind(session_id)
            .execute(&self.pool)
            .await?;
            Ok(false) // will retry
        }
    }
}
