//! PostgreSQL implementation of [`WriterBackend`].
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! `pg-backend`:
//!
//! > PgWriterBackend impl WriterBackend + map_sqlx + is_unique_violation 等
//! > PG SQL 辅助
//! > feature-gate: `#[cfg(feature = "postgres")]`
//!
//! One INSERT-per-row inside a single transaction. See
//! `intent-event-bus-execution.lisp` DC008 for the rationale (multi-row
//! INSERT with mixed-NULL UUID/JSONB columns is awkward under sqlx; 100-row
//! batches keep amortised cost low).
//!
//! `is_unique_violation` is re-exported from
//! [`super::dedup::is_unique_violation`] so the classifier lives with the
//! dedup contract rather than inside the PG adapter.
//!
//! [`WriterBackend`]: super::backend::WriterBackend

#![cfg(feature = "postgres")]

use async_trait::async_trait;
use uuid::Uuid;

use crate::event::log::Seq;

use super::backend::{BackendError, InsertRow, WriterBackend};
use super::dedup::is_unique_violation;

pub(crate) struct PgWriterBackend {
    pub(crate) pool: sqlx::PgPool,
}

#[async_trait]
impl WriterBackend for PgWriterBackend {
    async fn insert_batch(&self, rows: &[InsertRow<'_>]) -> Result<Vec<Seq>, BackendError> {
        // PG doesn't give us a batched RETURNING that preserves order across
        // multi-value INSERT with mixed nulls for UUIDs without a lot of
        // ceremony — simplest correct approach is one INSERT per row inside
        // a single transaction. Batch sizes are capped at 100 so this keeps
        // amortized cost low.
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let inline_json: Option<serde_json::Value> = match r.payload_inline {
                Some(bytes) => Some(serde_json::from_slice(bytes).map_err(|e| {
                    BackendError::Fatal(format!("payload_inline not JSON: {e}"))
                })?),
                None => None,
            };

            let (seq,): (i64,) = sqlx::query_as(
                r#"
                INSERT INTO event_log
                    (domain, kind, payload_inline, payload_ref, producer_id,
                     dedupe_key, causation_depth, trace_id, span_id,
                     parent_span_id, ephemeral)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                RETURNING seq
                "#,
            )
            .bind(r.domain.as_str())
            .bind(r.kind)
            .bind(inline_json)
            .bind(r.payload_ref.as_deref())
            .bind(r.producer_id)
            .bind(r.dedupe_key)
            .bind(r.causation_depth)
            .bind(r.trace_id)
            .bind(r.span_id)
            .bind(r.parent_span_id)
            .bind(r.ephemeral)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(dbe) if is_unique_violation(dbe.as_ref()) => {
                    BackendError::DedupeCollision
                }
                _ => map_sqlx(e),
            })?;
            out.push(Seq(seq));
        }
        tx.commit().await.map_err(map_sqlx)?;
        Ok(out)
    }

    async fn find_existing_seq(
        &self,
        producer_id: &str,
        dedupe_key: Uuid,
    ) -> Result<Option<Seq>, BackendError> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT seq FROM event_log WHERE producer_id = $1 AND dedupe_key = $2 LIMIT 1",
        )
        .bind(producer_id)
        .bind(dedupe_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(s,)| Seq(s)))
    }
}

/// Crude classifier — io / pool-closed / timeout / tls ⇒ transient.
pub(crate) fn map_sqlx(e: sqlx::Error) -> BackendError {
    let msg = format!("{e}");
    let is_transient = matches!(
        e,
        sqlx::Error::Io(_)
            | sqlx::Error::PoolClosed
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::Tls(_)
    );
    if is_transient {
        BackendError::Transient(msg)
    } else {
        BackendError::Fatal(msg)
    }
}
