//! Subscription options — the knob set for `bus.subscribe::<T>(name, opts)`.
//!
//! Frozen lisp §4.3 subscription-api:
//!
//! * [`StartFrom`] — where a brand-new cursor starts when it doesn't exist
//!   yet (`Latest` / `Earliest` / `Seq(n)`).
//! * [`BatchSize`] — how many events to pull per phase-1 bootstrap round or
//!   before a cursor flush.
//! * [`FailurePolicy`] — what happens when the consumer calls `ack.nack(...)`
//!   (`Retry` / `SkipToDLQ` / `Halt`).
//! * [`PauseBehavior`] — how the subscription reacts to a paused domain
//!   (`DropAndLiveResume` default; `FreezeAndCatchUp` is opt-in and
//!   Phase-4 MVP marks it deferred — see `D001`).
//! * [`CursorFlush`] — how often the persisted cursor catches up to the
//!   in-memory ack position (`PerEvent` / `BatchOr1s` / `Periodic`).
//!
//! Everything here serde-round-trips so the cursor row in `event_subscriptions`
//! can keep a structural JSONB copy of `failure_policy`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::super::log::Seq;

/// Where the subscription starts when no cursor row is persisted yet.
///
/// If a cursor row already exists for `subscription_name`, the subscription
/// resumes from `last_acked_seq` regardless of this setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartFrom {
    /// Skip history; start from the current log head. Safe default for new
    /// consumers that don't want the replay cost.
    Latest,
    /// Replay from seq 0. Only meaningful for analytical consumers that
    /// expect to see every event ever appended.
    Earliest,
    /// Start after the given seq (exclusive). Useful for point-in-time replay
    /// during ops recovery.
    Seq(Seq),
}

impl Default for StartFrom {
    fn default() -> Self {
        StartFrom::Latest
    }
}

/// How many events to pull per phase-1 round and per double-threshold flush.
///
/// Frozen lisp §4.3 subscription-api `batch-size`: default 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchSize(pub usize);

impl Default for BatchSize {
    fn default() -> Self {
        BatchSize(100)
    }
}

impl From<usize> for BatchSize {
    fn from(v: usize) -> Self {
        BatchSize(v.max(1))
    }
}

impl BatchSize {
    pub fn get(self) -> usize {
        self.0.max(1)
    }
}

/// Backoff shape for [`FailurePolicy::Retry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackoffKind {
    Fixed(Duration),
    Exponential { base: Duration, cap: Duration },
}

impl BackoffKind {
    /// Compute the delay for the Nth retry (1-based).
    pub fn delay(&self, attempt: u8) -> Duration {
        match *self {
            BackoffKind::Fixed(d) => d,
            BackoffKind::Exponential { base, cap } => {
                let shift = (attempt as u32).saturating_sub(1).min(30);
                let scaled = base.saturating_mul(1u32 << shift);
                std::cmp::min(scaled, cap)
            }
        }
    }
}

impl Default for BackoffKind {
    fn default() -> Self {
        BackoffKind::Exponential {
            base: Duration::from_millis(100),
            cap: Duration::from_secs(5),
        }
    }
}

/// What to do when a consumer's handler fails on an event.
///
/// Frozen lisp §4.3 subscription-api `FailurePolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailurePolicy {
    /// Retry in-place up to `max` attempts. After the cap, the implementation
    /// falls back to [`FailurePolicy::SkipToDLQ`] so the consumer is never
    /// permanently stuck on a single event.
    Retry { max: u8, backoff: BackoffKind },
    /// Push the failing event + reason onto `dead_letter_queue`, advance
    /// the cursor, carry on.
    SkipToDLQ,
    /// Stop the subscription after the first failure. Also emits an
    /// `IncidentEvent::Reported` so ops know the consumer is halted.
    Halt,
}

impl Default for FailurePolicy {
    fn default() -> Self {
        FailurePolicy::Retry {
            max: 3,
            backoff: BackoffKind::default(),
        }
    }
}

/// Pause-aware behavior for the subscription.
///
/// Frozen lisp §4.3 subscription-api `PauseBehavior`.
///
/// # Phase-4 MVP deviation (D001)
/// Only [`PauseBehavior::DropAndLiveResume`] is fully wired; the
/// `FreezeAndCatchUp` variant exists for API compatibility but maps to the
/// same behavior. A TODO is tracked under issue I009 to enable the
/// catch-up path once subscribers need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PauseBehavior {
    /// Dispatcher gates paused domains at the source; the subscriber simply
    /// observes no deliveries during the pause. On resume the first live
    /// event that reaches the subscriber dictates the next cursor.
    DropAndLiveResume,
    /// Opt-in: keep the cursor frozen during pause and pull accumulated
    /// history once resumed. See D001 — not fully implemented in the Phase-4
    /// MVP; currently aliases [`PauseBehavior::DropAndLiveResume`].
    FreezeAndCatchUp,
}

impl Default for PauseBehavior {
    fn default() -> Self {
        PauseBehavior::DropAndLiveResume
    }
}

/// Cursor flush policy.
///
/// Frozen lisp §4.3 cursor-store `flush-policy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorFlush {
    /// Flush on every ack. Highest durability, lowest throughput.
    PerEvent,
    /// Default: flush after a batch fills OR after 1s has passed —
    /// whichever comes first.
    BatchOr1s,
    /// Flush on a fixed interval.
    Periodic(Duration),
}

impl Default for CursorFlush {
    fn default() -> Self {
        CursorFlush::BatchOr1s
    }
}

impl CursorFlush {
    /// The time window after which a dirty cursor MUST be flushed.
    pub fn time_window(&self) -> Duration {
        match *self {
            CursorFlush::PerEvent => Duration::from_millis(1),
            CursorFlush::BatchOr1s => Duration::from_secs(1),
            CursorFlush::Periodic(d) => d,
        }
    }
}

/// Top-level subscription configuration passed to `bus.subscribe`.
///
/// Frozen lisp §4.3 subscription-api `SubscriptionOpts`.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionOpts {
    pub start_from: StartFrom,
    pub batch_size: BatchSize,
    pub failure_policy: FailurePolicy,
    pub pause_behavior: PauseBehavior,
    pub cursor_flush: CursorFlush,
    /// Free-form consumer name for ops. Kept alongside the cursor row.
    pub consumer_name: String,
}

impl SubscriptionOpts {
    /// Shorthand for building an opts struct with a consumer name only.
    pub fn named(consumer_name: impl Into<String>) -> Self {
        Self {
            consumer_name: consumer_name.into(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_from_round_trip() {
        let all = [
            StartFrom::Latest,
            StartFrom::Earliest,
            StartFrom::Seq(Seq(42)),
        ];
        for sf in all {
            let json = serde_json::to_string(&sf).unwrap();
            let back: StartFrom = serde_json::from_str(&json).unwrap();
            assert_eq!(sf, back);
        }
    }

    #[test]
    fn failure_policy_round_trip() {
        let all = [
            FailurePolicy::Retry {
                max: 5,
                backoff: BackoffKind::Fixed(Duration::from_millis(200)),
            },
            FailurePolicy::Retry {
                max: 3,
                backoff: BackoffKind::Exponential {
                    base: Duration::from_millis(50),
                    cap: Duration::from_secs(10),
                },
            },
            FailurePolicy::SkipToDLQ,
            FailurePolicy::Halt,
        ];
        for p in all {
            let json = serde_json::to_string(&p).unwrap();
            let back: FailurePolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn pause_behavior_round_trip() {
        let all = [
            PauseBehavior::DropAndLiveResume,
            PauseBehavior::FreezeAndCatchUp,
        ];
        for p in all {
            let json = serde_json::to_string(&p).unwrap();
            let back: PauseBehavior = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn cursor_flush_round_trip() {
        let all = [
            CursorFlush::PerEvent,
            CursorFlush::BatchOr1s,
            CursorFlush::Periodic(Duration::from_secs(5)),
        ];
        for f in all {
            let json = serde_json::to_string(&f).unwrap();
            let back: CursorFlush = serde_json::from_str(&json).unwrap();
            assert_eq!(f, back);
        }
    }

    #[test]
    fn backoff_exponential_saturates_at_cap() {
        let b = BackoffKind::Exponential {
            base: Duration::from_millis(100),
            cap: Duration::from_secs(2),
        };
        assert_eq!(b.delay(1), Duration::from_millis(100));
        assert_eq!(b.delay(2), Duration::from_millis(200));
        assert_eq!(b.delay(3), Duration::from_millis(400));
        // Large attempt values stay at cap.
        assert_eq!(b.delay(20), Duration::from_secs(2));
    }

    #[test]
    fn backoff_fixed_is_constant() {
        let b = BackoffKind::Fixed(Duration::from_millis(250));
        for attempt in 1..=10 {
            assert_eq!(b.delay(attempt), Duration::from_millis(250));
        }
    }

    #[test]
    fn batch_size_clamps_zero_to_one() {
        let b = BatchSize::from(0);
        assert_eq!(b.get(), 1);
    }

    #[test]
    fn cursor_flush_time_windows() {
        assert_eq!(CursorFlush::BatchOr1s.time_window(), Duration::from_secs(1));
        assert_eq!(
            CursorFlush::Periodic(Duration::from_millis(500)).time_window(),
            Duration::from_millis(500)
        );
    }

    #[test]
    fn defaults_match_frozen_lisp() {
        let opts = SubscriptionOpts::default();
        assert!(matches!(opts.start_from, StartFrom::Latest));
        assert_eq!(opts.batch_size.get(), 100);
        assert!(matches!(
            opts.failure_policy,
            FailurePolicy::Retry { max: 3, .. }
        ));
        assert!(matches!(
            opts.pause_behavior,
            PauseBehavior::DropAndLiveResume
        ));
        assert!(matches!(opts.cursor_flush, CursorFlush::BatchOr1s));
    }
}
