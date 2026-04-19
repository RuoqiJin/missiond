//! Failure mode for step-3 commit.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! failure-mode:
//!
//! > retry  "batch INSERT 临时错误 → exp backoff 6 次"
//! > fatal  "超限 → LogWriter 进 failed state,拒新 append →
//!           AppendError::LogUnavailable"
//! > self-report "进 failed 时发 IncidentEvent::Reported(severity=critical)"
//!
//! # Implementation anchors
//!
//! All three bullets are implemented inline inside [`super::log_writer`]:
//!
//! * `RETRY_BASE_DELAY / RETRY_MAX_DELAY / FAILED_STATE_RETRY_CAP`
//!   (private consts, file-top) tune the exponential backoff.
//! * `exp_backoff(attempt)` (private fn) computes the per-try delay.
//! * `LogWriter::flush` loops over `insert_batch()` with backoff until
//!   success or `attempt >= FAILED_STATE_RETRY_CAP`, at which point
//!   `self.failed = true` and subsequent batches short-circuit to
//!   [`crate::event::log::AppendError::LogUnavailable`].
//! * Self-emission of `IncidentEvent::Reported { severity = critical }`
//!   happens at the **daemon** layer (the writer itself can't publish
//!   while in failed state without looping). See
//!   `crates/missiond-daemon/src/bus/v2_subscribers.rs` `spawn_incident_reactor`
//!   for the wiring.
//!
//! This module is a doc-only anchor. If retry / failed-state policy grows
//! (e.g. half-open state, per-error classifier), its state machine moves
//! here.

/// How many consecutive transient errors the writer tolerates before
/// locking into failed state. Mirrors the private constant in
/// [`super::log_writer`].
pub const FAILED_STATE_RETRY_CAP: u32 = 6;
