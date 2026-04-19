//! Pipeline step 3 · commit — single-writer batching into `event_log`.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit:
//!
//! > 单写入点批处理 + DB INSERT + 分配 seq + dedup 冲突解析
//!
//! Seven sub-concerns from the lisp — each is a dedicated module so the
//! 7-step layout is navigable 1:1 with the frozen design:
//!
//! | Lisp component      | Module                | Role                                   |
//! |---------------------|-----------------------|----------------------------------------|
//! | `log-writer`        | [`log_writer`]        | Main loop + batch + flush orchestration |
//! | `handle`            | [`handle`]            | Producer facade, `impl Log` trait       |
//! | `backend-abstraction` | [`backend`]         | `WriterBackend` trait + `InsertRow` + `BackendError` |
//! | `pg-backend`        | [`pg_backend`]        | PostgreSQL impl + `map_sqlx`            |
//! | `seq-authority`     | [`seq_authority`]     | Doc anchor — DB BIGSERIAL invariant     |
//! | `dedup-semantics`   | [`dedup`]             | `is_unique_violation` + collision docs  |
//! | `backpressure`      | [`backpressure`]      | `PendingAppend` + capacity/batch consts |
//! | `failure-mode`      | [`failure_mode`]      | Retry + `exp_backoff` + failed state    |
//!
//! The actual orchestration is intentionally small: [`log_writer`] wires the
//! seven concerns via a single `tokio::task` that consumes a bounded MPSC
//! channel, batches up to [`backpressure::BATCH_MAX`] appends per flush,
//! delegates claim-check to [`crate::event::blob_store::BlobStore`], and
//! INSERTs via `PgWriterBackend` (or a mock in unit tests). The same task
//! also handles dedup collisions and retry / failed-state transitions.
//!
//! For the parity in-memory writer, see
//! [`crate::event::in_memory::InMemoryLog`] — same contract, different
//! physical backend (Vec<StoredRow> + AtomicI64).

pub mod backend;
pub mod backpressure;
pub mod dedup;
pub mod failure_mode;
pub mod handle;
pub mod log_writer;
pub mod pg_backend;
pub mod seq_authority;

// Re-export the canonical producer-facing surface under the step-3 namespace.
// Internals stay namespaced inside each sub-module.
pub use backpressure::{PendingAppend, APPEND_CHANNEL_CAPACITY, BATCH_DEADLINE, BATCH_MAX};
pub use handle::LogWriterHandle;
pub use log_writer::{new_log_writer, spawn_log_writer, LogWriter};
