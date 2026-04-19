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
//! The collision-handling code lives inline inside [`super::log_writer`]:
//!
//! * `PgWriterBackend::insert_batch` classifies PostgreSQL SQLSTATE `23505`
//!   (unique_violation) as [`BackendError::DedupeCollision`].
//! * `LogWriter::flush` catches that error and walks each pending entry
//!   through `backend.find_existing_seq(producer_id, dedupe_key)` to
//!   resolve the `AppendAck::AlreadyExists(seq)` reply without any
//!   side-effect rollback.
//! * The in-memory parity bus
//!   ([`crate::event::in_memory::InMemoryLog`]) uses an explicit
//!   `HashMap<(String, Uuid), Seq>` to achieve the same contract.
//!
//! This module intentionally hosts no executable code — it is a single
//! documentation anchor so readers browsing the 7-step layout can locate
//! the dedup contract without spelunking through `log_writer.rs`. The
//! helper below is re-exported for code that wants a named constant for
//! the unique-index column pair.
//!
//! [`BackendError::DedupeCollision`]: crate::event::pipeline::step3_commit::log_writer

/// The UNIQUE-index column pair that drives the dedup contract. Exposed
/// mostly for integration tests and migrations to cross-check that the SQL
/// schema and the writer agree on the key shape.
pub const DEDUP_UNIQUE_COLUMNS: (&str, &str) = ("producer_id", "dedupe_key");
