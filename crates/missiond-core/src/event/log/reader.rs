//! Pull-based reader for event_log catch-up.
//!
//! Frozen lisp §4.3 phase-1-bootstrap: subscribers hit the DB directly with
//! `SELECT WHERE seq > last_acked ORDER BY seq` to replay missed events.
//! The live path (Phase 3 dispatcher) bypasses this and reads from the
//! in-process broadcast instead.
//!
//! `LoggedEvent` intentionally keeps the payload as `serde_json::Value` so
//! this module stays generic over [`DomainEvent`] implementers — callers
//! deserialize into the concrete enum when they know the [`Domain`].

#![cfg(feature = "postgres")]

use sqlx::PgPool;
use uuid::Uuid;

use super::super::blob_store::BlobStore;
use super::super::blob_store::PayloadRef;
use super::super::domain::Domain;
use super::{LogError, Seq};

/// A replayed row from `event_log`. Rich metadata is included so subscribers
/// can reconstruct [`AppendOpts`] if they need to re-emit.
#[derive(Debug, Clone)]
pub struct LoggedEvent {
    pub seq: Seq,
    pub domain: Domain,
    pub kind: String,
    /// The event payload, resolved through claim-check (inline or fetched
    /// from blob storage).
    pub payload: serde_json::Value,
    pub producer_id: String,
    pub dedupe_key: Option<Uuid>,
    pub causation_depth: i16,
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub ephemeral: bool,
}

/// Handle used by subscribers to pull catch-up batches. Cheap to clone.
#[derive(Clone)]
pub struct LogReader {
    pool: PgPool,
    blob_store: std::sync::Arc<dyn BlobStore>,
}

impl LogReader {
    pub fn new(pool: PgPool, blob_store: std::sync::Arc<dyn BlobStore>) -> Self {
        Self { pool, blob_store }
    }

    /// Returns up to `limit` events in `domain` with `seq > after_seq`.
    pub async fn read_from(
        &self,
        domain: Domain,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, LogError> {
        let rows: Vec<Row> = sqlx::query_as::<_, Row>(
            r#"
            SELECT seq, domain, kind, payload_inline, payload_ref, producer_id,
                   dedupe_key, causation_depth, trace_id, span_id, parent_span_id,
                   ts, ephemeral
            FROM event_log
            WHERE domain = $1 AND seq > $2
            ORDER BY seq ASC
            LIMIT $3
            "#,
        )
        .bind(domain.as_str())
        .bind(after_seq.0)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(resolve_row(row, &*self.blob_store).await?);
        }
        Ok(out)
    }

    /// Current head `seq` across all domains.
    pub async fn head_seq(&self) -> Result<Seq, LogError> {
        let (seq,): (Option<i64>,) = sqlx::query_as("SELECT MAX(seq) FROM event_log")
            .fetch_one(&self.pool)
            .await?;
        Ok(Seq(seq.unwrap_or(0)))
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    seq: i64,
    domain: String,
    kind: String,
    payload_inline: Option<serde_json::Value>,
    payload_ref: Option<String>,
    producer_id: String,
    dedupe_key: Option<Uuid>,
    causation_depth: i16,
    trace_id: Option<Uuid>,
    span_id: Option<Uuid>,
    parent_span_id: Option<Uuid>,
    ts: chrono::DateTime<chrono::Utc>,
    ephemeral: bool,
}

async fn resolve_row(row: Row, blob_store: &dyn BlobStore) -> Result<LoggedEvent, LogError> {
    let domain = domain_from_str(&row.domain).ok_or_else(|| {
        LogError::Other(format!(
            "unknown domain label {:?} at seq {}",
            row.domain, row.seq
        ))
    })?;
    let payload = match (row.payload_inline, row.payload_ref) {
        (Some(inline), _) => inline,
        (None, Some(ref_json)) => {
            let payload_ref = PayloadRef::from_json_str(&ref_json)
                .map_err(|e| LogError::Other(format!("decode payload_ref: {e}")))?;
            let bytes = blob_store
                .get(&payload_ref)
                .await
                .map_err(|e| LogError::Other(format!("fetch blob: {e}")))?;
            serde_json::from_slice(&bytes)?
        }
        (None, None) => serde_json::Value::Null,
    };

    Ok(LoggedEvent {
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
    })
}

pub(crate) fn domain_from_str(s: &str) -> Option<Domain> {
    Some(match s {
        "slot" => Domain::Slot,
        "board" => Domain::Board,
        "task" => Domain::Task,
        "question" => Domain::Question,
        "llm" => Domain::Llm,
        "worker" => Domain::Worker,
        "memory" => Domain::Memory,
        "message" => Domain::Message,
        "session" => Domain::Session,
        "system" => Domain::System,
        "observability" => Domain::Observability,
        "incident" => Domain::Incident,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_from_str_round_trip() {
        for d in Domain::ALL {
            let s = d.as_str();
            let parsed = domain_from_str(s).expect("parse");
            assert_eq!(parsed, d);
        }
    }

    #[test]
    fn domain_from_str_unknown() {
        assert!(domain_from_str("mystery").is_none());
    }
}
