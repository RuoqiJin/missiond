//! Backpressure contract for step-3 commit.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! backpressure:
//!
//! > channel   "append channel 有界 (默认 4096)"
//! > overflow  "满则 append() 返回 Err(Backpressure),生产者决定重试/丢弃/panic"
//! > rationale "可见失败 > 静默吞 > 无界内存膨胀"
//!
//! # Implementation anchors
//!
//! The bounded MPSC channel and the `try_send` → `Full` →
//! [`crate::event::log::AppendError::Backpressure`] branch live inside
//! [`super::handle::LogWriterHandle::append`]. The channel is built at
//! writer construction ([`super::log_writer::new_with_backend`]) with
//! capacity [`APPEND_CHANNEL_CAPACITY`] = 4096 (mirror
//! [`crate::event::in_memory::IN_MEMORY_APPEND_CAPACITY`]).
//!
//! Batch-shape constants ([`BATCH_MAX`] and [`BATCH_DEADLINE`]) sit in the
//! same module because batch size is the second pressure-release knob — the
//! faster we drain the channel, the less real backpressure pressure
//! producers feel.
//!
//! The [`PendingAppend`] struct encodes one entry on its way through the
//! bounded channel — producer side writes it, writer task reads it, and
//! the `oneshot::Sender` on `ack` closes the loop back to the caller.

use tokio::sync::oneshot;
use uuid::Uuid;

use crate::event::domain::Domain;
use crate::event::log::{AppendAck, AppendError};

/// Bound on the append channel. Frozen lisp §4.2 step-3 commit
/// backpressure.channel.
pub const APPEND_CHANNEL_CAPACITY: usize = 4096;

/// Maximum rows per INSERT batch. Frozen lisp §4.2 step-3 commit
/// log-writer.batching.
pub const BATCH_MAX: usize = 100;

/// Deadline after first pending row before the writer flushes anyway.
/// Frozen lisp §4.2 step-3 commit log-writer.batching.
pub const BATCH_DEADLINE: std::time::Duration = std::time::Duration::from_millis(10);

/// Single entry moving through the append MPSC — the producer-side future
/// lives on the other end of `ack`.
pub struct PendingAppend {
    pub domain: Domain,
    pub kind: &'static str,
    pub payload_bytes: Vec<u8>,
    pub payload_inline_eligible: bool,
    pub ephemeral: bool,
    pub producer_id: String,
    pub dedupe_key: Option<Uuid>,
    pub causation_depth: i16,
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub ack: oneshot::Sender<Result<AppendAck, AppendError>>,
}
