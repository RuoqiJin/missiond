//! Backend abstraction for step-3 commit.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! `backend-abstraction`:
//!
//! > WriterBackend trait + InsertRow + BackendError — Writer 和底层 DB
//! > 实现之间的抽象接口
//! > 隔离 PG 专用 SQL,方便 InMemoryLog / 未来 NATS 等替代实现
//!
//! The `LogWriter` main loop consumes `PendingAppend` entries, batches them,
//! and calls into a [`WriterBackend`] to persist the batch and obtain the
//! DB-assigned seqs. Unit tests plug a mock backend; production wires
//! [`super::pg_backend::PgWriterBackend`].
//!
//! [`super::pg_backend::PgWriterBackend`]: crate::event::pipeline::step3_commit::pg_backend::PgWriterBackend

use async_trait::async_trait;
use uuid::Uuid;

use crate::event::domain::Domain;
use crate::event::log::Seq;

/// Abstract over the concrete DB so unit tests can plug in a mock without
/// linking sqlx.
#[async_trait]
pub(crate) trait WriterBackend: Send + Sync {
    async fn insert_batch(&self, rows: &[InsertRow<'_>]) -> Result<Vec<Seq>, BackendError>;

    async fn find_existing_seq(
        &self,
        producer_id: &str,
        dedupe_key: Uuid,
    ) -> Result<Option<Seq>, BackendError>;
}

/// Row to be inserted. Borrowed from the pending append so we don't re-clone
/// the potentially large payload.
pub(crate) struct InsertRow<'a> {
    pub domain: Domain,
    pub kind: &'a str,
    pub payload_inline: Option<&'a [u8]>,
    pub payload_ref: Option<String>,
    pub producer_id: &'a str,
    pub dedupe_key: Option<Uuid>,
    pub causation_depth: i16,
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub ephemeral: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BackendError {
    /// A `(producer_id, dedupe_key)` UNIQUE collision — resolved at the
    /// writer level by [`crate::event::pipeline::step3_commit::dedup`].
    #[error("dedupe collision (producer_id + key already exists)")]
    DedupeCollision,
    /// Transient / retryable; e.g. pool timeout, broken TCP.
    #[error("transient: {0}")]
    Transient(String),
    /// Sticky / schema / logic error; no point retrying within the writer.
    #[error("fatal: {0}")]
    Fatal(String),
}
