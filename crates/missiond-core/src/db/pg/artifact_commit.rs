use async_trait::async_trait;

use crate::db::artifact_commit::{ArtifactCommitOutboxInput, ArtifactCommitOutboxRecord};
use crate::db::error::DbResult;
use crate::db::traits::ArtifactCommitStore;

use super::PgMissionStore;

const ARTIFACT_COMMIT_OUTBOX_COLS: &str = "id, operation_key, surface, request_id, project_id, artifact_kind, artifact_path, artifact_sha256, db_table, db_row_id, event_id, event_seq, status, attempt_count, last_error, payload, created_at, updated_at, completed_at";

#[cfg(feature = "postgres")]
#[async_trait]
impl ArtifactCommitStore for PgMissionStore {
    async fn artifact_commit_outbox_upsert_pending(
        &self,
        input: &ArtifactCommitOutboxInput,
    ) -> DbResult<ArtifactCommitOutboxRecord> {
        let sql = format!(
            "INSERT INTO artifact_commit_outbox (
                operation_key, surface, request_id, project_id, artifact_kind, artifact_path,
                artifact_sha256, db_table, db_row_id, event_id, event_seq, payload
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             ON CONFLICT (operation_key) DO UPDATE SET
                surface = EXCLUDED.surface,
                request_id = COALESCE(EXCLUDED.request_id, artifact_commit_outbox.request_id),
                project_id = COALESCE(EXCLUDED.project_id, artifact_commit_outbox.project_id),
                artifact_kind = EXCLUDED.artifact_kind,
                artifact_path = EXCLUDED.artifact_path,
                artifact_sha256 = COALESCE(EXCLUDED.artifact_sha256, artifact_commit_outbox.artifact_sha256),
                db_table = COALESCE(EXCLUDED.db_table, artifact_commit_outbox.db_table),
                db_row_id = COALESCE(EXCLUDED.db_row_id, artifact_commit_outbox.db_row_id),
                event_id = COALESCE(EXCLUDED.event_id, artifact_commit_outbox.event_id),
                event_seq = COALESCE(EXCLUDED.event_seq, artifact_commit_outbox.event_seq),
                status = CASE
                    WHEN artifact_commit_outbox.status = 'complete' THEN artifact_commit_outbox.status
                    ELSE 'pending'
                END,
                last_error = CASE
                    WHEN artifact_commit_outbox.status = 'complete' THEN artifact_commit_outbox.last_error
                    ELSE NULL
                END,
                payload = CASE
                    WHEN artifact_commit_outbox.status = 'complete' THEN artifact_commit_outbox.payload
                    ELSE artifact_commit_outbox.payload || EXCLUDED.payload
                END,
                updated_at = now()
             RETURNING {ARTIFACT_COMMIT_OUTBOX_COLS}"
        );
        let row = sqlx::query_as::<_, ArtifactCommitOutboxRecord>(&sql)
            .bind(&input.operation_key)
            .bind(&input.surface)
            .bind(&input.request_id)
            .bind(&input.project_id)
            .bind(&input.artifact_kind)
            .bind(&input.artifact_path)
            .bind(&input.artifact_sha256)
            .bind(&input.db_table)
            .bind(&input.db_row_id)
            .bind(&input.event_id)
            .bind(input.event_seq)
            .bind(&input.payload)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn artifact_commit_outbox_mark_complete(
        &self,
        operation_key: &str,
        artifact_sha256: &str,
        payload: &serde_json::Value,
    ) -> DbResult<ArtifactCommitOutboxRecord> {
        let sql = format!(
            "UPDATE artifact_commit_outbox
             SET status = 'complete',
                 artifact_sha256 = $2,
                 payload = artifact_commit_outbox.payload || $3,
                 last_error = NULL,
                 completed_at = COALESCE(completed_at, now()),
                 updated_at = now()
             WHERE operation_key = $1
             RETURNING {ARTIFACT_COMMIT_OUTBOX_COLS}"
        );
        let row = sqlx::query_as::<_, ArtifactCommitOutboxRecord>(&sql)
            .bind(operation_key)
            .bind(artifact_sha256)
            .bind(payload)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn artifact_commit_outbox_mark_failed(
        &self,
        operation_key: &str,
        error: &str,
    ) -> DbResult<ArtifactCommitOutboxRecord> {
        let sql = format!(
            "UPDATE artifact_commit_outbox
             SET status = 'failed',
                 attempt_count = attempt_count + 1,
                 last_error = $2,
                 updated_at = now()
             WHERE operation_key = $1
             RETURNING {ARTIFACT_COMMIT_OUTBOX_COLS}"
        );
        let row = sqlx::query_as::<_, ArtifactCommitOutboxRecord>(&sql)
            .bind(operation_key)
            .bind(error)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn artifact_commit_outbox_get(
        &self,
        operation_key: &str,
    ) -> DbResult<Option<ArtifactCommitOutboxRecord>> {
        let sql = format!(
            "SELECT {ARTIFACT_COMMIT_OUTBOX_COLS}
             FROM artifact_commit_outbox
             WHERE operation_key = $1"
        );
        let row = sqlx::query_as::<_, ArtifactCommitOutboxRecord>(&sql)
            .bind(operation_key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    async fn artifact_commit_outbox_claim_recoverable(
        &self,
        limit: i64,
    ) -> DbResult<Vec<ArtifactCommitOutboxRecord>> {
        let sql = format!(
            "UPDATE artifact_commit_outbox
             SET status = 'pending',
                 attempt_count = attempt_count + 1,
                 updated_at = now()
             WHERE id IN (
                SELECT id
                FROM artifact_commit_outbox
                WHERE status IN ('pending', 'failed')
                ORDER BY updated_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
             )
             RETURNING {ARTIFACT_COMMIT_OUTBOX_COLS}"
        );
        let rows = sqlx::query_as::<_, ArtifactCommitOutboxRecord>(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }
}
