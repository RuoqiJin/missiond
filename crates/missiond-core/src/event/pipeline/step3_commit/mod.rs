//! Pipeline step 3 · commit — single-writer batching into `event_log`.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit:
//!
//! > 单写入点批处理 + DB INSERT + 分配 seq + dedup 冲突解析
//!
//! Five sub-concerns from the lisp — implemented inline inside
//! [`log_writer`] but **named** here so readers can navigate the 7-step
//! layout 1:1 with the frozen design:
//!
//! | Lisp component    | Anchor module           | Kind          |
//! |-------------------|-------------------------|---------------|
//! | `log-writer`      | [`log_writer`]          | Implementation |
//! | `seq-authority`   | [`seq_authority`]       | Doc-only       |
//! | `dedup-semantics` | [`dedup`]               | Doc-only       |
//! | `backpressure`    | [`backpressure`]        | Doc-only       |
//! | `failure-mode`    | [`failure_mode`]        | Doc-only       |
//!
//! The implementation in [`log_writer`] wires the five concerns together
//! via a single `tokio::task` that consumes a bounded MPSC channel,
//! batches up to 100 appends per flush, delegates claim-check to
//! [`crate::event::blob_store::BlobStore`], and INSERTs via
//! `PgWriterBackend` (or a mock in unit tests). The same task also handles
//! dedup collisions and retry / failed-state transitions.
//!
//! For the parity in-memory writer, see
//! [`crate::event::in_memory::InMemoryLog`] — same contract, different
//! physical backend (Vec<StoredRow> + AtomicI64).

pub mod backpressure;
pub mod dedup;
pub mod failure_mode;
pub mod log_writer;
pub mod seq_authority;

// Re-export the canonical producer-facing surface. Internals stay namespaced.
pub use log_writer::{
    new_log_writer, spawn_log_writer, LogWriter, LogWriterHandle, PendingAppend,
    APPEND_CHANNEL_CAPACITY, BATCH_DEADLINE, BATCH_MAX,
};
