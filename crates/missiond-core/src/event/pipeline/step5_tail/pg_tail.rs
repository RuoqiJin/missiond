//! PostgreSQL [`TailSource`] — long-poll `event_log` via `SELECT`.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-5 tail
//! `pg-tail`:
//!
//! > PgTailSource impl TailSource — PG 长轮询 SELECT 实现
//! > feature-gate: `#[cfg(feature = "postgres")]`
//!
//! The dispatcher reads across every domain in `Domain::ALL` in a single query, ordered by
//! `seq` ascending; see [`super::dispatcher::run_tail`] for the polling
//! loop. Payload refs (claim-check) are resolved here via the attached
//! [`BlobStore`] so `LoggedEvent::payload` is always dense.
//!
//! [`TailSource`]: super::tail_source::TailSource

#![cfg(feature = "postgres")]

use std::sync::Arc;

use async_trait::async_trait;

use crate::event::blob_store::{BlobStore, PayloadRef};
use crate::event::log::reader::LoggedEvent;
use crate::event::log::Seq;

use super::tail_source::{TailError, TailSource};

/// PG-backed tail source.
pub struct PgTailSource {
    pool: sqlx::PgPool,
    blob_store: Arc<dyn BlobStore>,
}

impl PgTailSource {
    pub fn new(pool: sqlx::PgPool, blob_store: Arc<dyn BlobStore>) -> Self {
        Self { pool, blob_store }
    }
}

#[async_trait]
impl TailSource for PgTailSource {
    async fn read_all_from(
        &self,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, TailError> {
        use crate::event::log::reader::domain_from_str;

        #[derive(sqlx::FromRow)]
        struct Row {
            seq: i64,
            domain: String,
            kind: String,
            payload_inline: Option<serde_json::Value>,
            payload_ref: Option<String>,
            producer_id: String,
            dedupe_key: Option<uuid::Uuid>,
            causation_depth: i16,
            trace_id: Option<uuid::Uuid>,
            span_id: Option<uuid::Uuid>,
            parent_span_id: Option<uuid::Uuid>,
            ts: chrono::DateTime<chrono::Utc>,
            ephemeral: bool,
        }

        let rows: Vec<Row> = sqlx::query_as::<_, Row>(
            r#"
            SELECT seq, domain, kind, payload_inline, payload_ref, producer_id,
                   dedupe_key, causation_depth, trace_id, span_id, parent_span_id,
                   ts, ephemeral
            FROM event_log
            WHERE seq > $1
            ORDER BY seq ASC
            LIMIT $2
            "#,
        )
        .bind(after_seq.0)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TailError::LogRead(e.to_string()))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(domain) = domain_from_str(&row.domain) else {
                // Log + skip this row; the tail loop decides what to do
                // with an unresolved domain (typically: drop).
                tracing::warn!(
                    seq = row.seq,
                    domain = %row.domain,
                    "tail: unknown domain label; skipping row"
                );
                continue;
            };
            let payload = match (row.payload_inline, row.payload_ref) {
                (Some(inline), _) => inline,
                (None, Some(ref_json)) => {
                    let payload_ref = PayloadRef::from_json_str(&ref_json)
                        .map_err(|e| TailError::LogRead(format!("decode payload_ref: {e}")))?;
                    let bytes = self
                        .blob_store
                        .get(&payload_ref)
                        .await
                        .map_err(|e| TailError::LogRead(format!("fetch blob: {e}")))?;
                    serde_json::from_slice(&bytes)
                        .map_err(|e| TailError::LogRead(format!("decode blob payload: {e}")))?
                }
                (None, None) => serde_json::Value::Null,
            };

            out.push(LoggedEvent {
                seq: Seq(row.seq),
                domain,
                kind: row.kind,
                payload,
                producer_id: row.producer_id,
                dedupe_key: row.dedupe_key,
                causation_depth: row.causation_depth,
                trace_id: row.trace_id,
                span_id: row.span_id,
                parent_span_id: row.parent_span_id,
                ts: row.ts,
                ephemeral: row.ephemeral,
            });
        }
        Ok(out)
    }
}
