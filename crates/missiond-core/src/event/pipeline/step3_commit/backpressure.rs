//! Backpressure contract for step-3 commit.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! backpressure:
//!
//! > channel   "append channel 有界 (默认 4096)"
//! > overflow  "满则 append() 返回 Err(Backpressure),生产者决定重试/丢弃/panic"
//! > rationale "可见失败 > 静默吞 > 无界内存膨胀"
//!
//! # Implementation anchor
//!
//! The bounded MPSC channel and the `try_send` → `Full` →
//! [`crate::event::log::AppendError::Backpressure`] branch live inside
//! [`super::log_writer::LogWriterHandle::append`]. The capacity constant is
//! [`super::log_writer::APPEND_CHANNEL_CAPACITY`] = 4096 (mirror
//! [`crate::event::in_memory::IN_MEMORY_APPEND_CAPACITY`]).
//!
//! This module is a doc-only anchor: calling it "Backpressure" at the
//! module-path level makes the 7-step layout readable without forcing
//! a second codebase of helpers. If backpressure policy grows (e.g.
//! adaptive capacity, priority queues, per-domain channels), the
//! implementation moves here.

/// Re-export of [`super::log_writer::APPEND_CHANNEL_CAPACITY`] — the single
/// tunable for the step-3 commit channel.
pub use super::log_writer::APPEND_CHANNEL_CAPACITY;
