//! Dedup semantics for step-3 commit.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! dedup-semantics:
//!
//! > purpose            "Producer 重试保护,非业务去重"
//! > key                "(producer_id, dedupe_key) UNIQUE INDEX WHERE dedupe_key IS NOT NULL"
//! > collision-behavior "二次 append 相同 key → SELECT existing → 返回
//!                       Ok(AlreadyExists(seq)),无副作用"
//! > producer-contract  "生产者超时/崩溃重试必须带同一 dedupe_key"
//!
//! # Implementation anchors
//!
//! The collision path is split across three sites:
//!
//! * [`is_unique_violation`] — classifies a PostgreSQL `DatabaseError` as a
//!   UNIQUE-constraint violation (SQLSTATE `23505`) so the PG backend can
//!   translate it into [`BackendError::DedupeCollision`]. Lives here rather
//!   than in the PG adapter because the classifier expresses a contract
//!   (dedup), not a driver implementation detail.
//! * [`BackendError::DedupeCollision`] — surfaced by
//!   [`super::backend::WriterBackend::insert_batch`] when the UNIQUE index
//!   rejects a row.
//! * `LogWriter::flush` (in [`super::log_writer`]) catches
//!   `DedupeCollision`, walks each pending entry through
//!   `backend.find_existing_seq(producer_id, dedupe_key)` and returns
//!   `AppendAck::AlreadyExists(seq)` without any side-effect rollback.
//!
//! The in-memory parity bus
//! ([`crate::event::in_memory::InMemoryLog`]) uses an explicit
//! `HashMap<(String, Uuid), Seq>` to achieve the same contract — see
//! `intent-event-bus-execution.lisp` DC023.
//!
//! [`BackendError::DedupeCollision`]: super::backend::BackendError::DedupeCollision

/// The UNIQUE-index column pair that drives the dedup contract. Exposed
/// mostly for integration tests and migrations to cross-check that the SQL
/// schema and the writer agree on the key shape.
pub const DEDUP_UNIQUE_COLUMNS: (&str, &str) = ("producer_id", "dedupe_key");

/// PostgreSQL SQLSTATE `23505` = `unique_violation`.
///
/// Classify a `sqlx::error::DatabaseError` as the dedup-unique collision so
/// [`super::pg_backend::PgWriterBackend`] can map it to
/// [`BackendError::DedupeCollision`]. The helper lives in `dedup.rs` because
/// it encodes the dedup *contract* (which SQLSTATE = a dedup hit), not a
/// driver concern.
///
/// [`BackendError::DedupeCollision`]: super::backend::BackendError::DedupeCollision
#[cfg(feature = "postgres")]
pub(crate) fn is_unique_violation(e: &dyn sqlx::error::DatabaseError) -> bool {
    e.code().as_deref() == Some("23505")
}
