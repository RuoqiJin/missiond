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
//! The retry loop and failed-state atomic flag live in
//! [`super::log_writer::LogWriter::flush`]:
//!
//! * [`RETRY_BASE_DELAY`], [`RETRY_MAX_DELAY`], [`FAILED_STATE_RETRY_CAP`]
//!   tune the exponential backoff.
//! * [`exp_backoff`] computes the per-try delay (saturating shift,
//!   capped at `RETRY_MAX_DELAY`).
//! * `LogWriter::flush` loops over `backend.insert_batch()` with backoff
//!   until success or `attempt >= FAILED_STATE_RETRY_CAP`, at which point
//!   `self.failed = true` and subsequent batches short-circuit to
//!   [`crate::event::log::AppendError::LogUnavailable`].
//! * Self-emission of `IncidentEvent::Reported { severity = critical }`
//!   happens at the **daemon** layer (the writer itself can't publish
//!   while in failed state without looping). See
//!   `crates/missiond-daemon/src/bus/v2_subscribers.rs`
//!   `spawn_incident_reactor` for the wiring.

use std::time::Duration;

/// Retry knobs for transient DB failures.
///
/// Frozen lisp §4.2 step-3 commit failure-mode.retry — exp backoff 6 次.
pub const RETRY_BASE_DELAY: Duration = Duration::from_millis(50);
pub const RETRY_MAX_DELAY: Duration = Duration::from_secs(2);

/// How many consecutive transient errors the writer tolerates before
/// locking into failed state. Frozen lisp §4.2 step-3 commit failure-mode.fatal.
pub const FAILED_STATE_RETRY_CAP: u32 = 6;

/// Exponential backoff schedule — attempt 1 returns `RETRY_BASE_DELAY`,
/// each subsequent attempt doubles, capped at `RETRY_MAX_DELAY`.
///
/// Saturating shift keeps large `attempt` values from panicking.
pub(crate) fn exp_backoff(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(10);
    let raw = RETRY_BASE_DELAY.saturating_mul(1 << shift);
    raw.min(RETRY_MAX_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_backoff_capped() {
        // Attempt 1 is base; it doubles until RETRY_MAX_DELAY.
        assert_eq!(exp_backoff(1), RETRY_BASE_DELAY);
        assert!(exp_backoff(20) <= RETRY_MAX_DELAY);
    }
}
