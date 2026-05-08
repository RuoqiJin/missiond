//! Failure handling — the three [`FailurePolicy`] variants wired against the
//! dead-letter queue table.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3:
//!
//! * `Retry { max, backoff }` — handler failed once; the outer runtime
//!   sleeps `backoff.delay(attempt)`, hands the event back to the consumer,
//!   and after `max` attempts falls through to [`FailurePolicy::SkipToDLQ`]
//!   so the subscription is never permanently blocked on a single event.
//! * `SkipToDLQ` — move the event to `dead_letter_queue`, advance the
//!   cursor, carry on.
//! * `Halt` — stop the subscription and emit an `IncidentEvent::Reported`
//!   for ops. The subscription's `next()` returns `None` from here on.
//!
//! The [`FailureRouter`] type is the plumbing the subscription lifecycle
//! uses to route a failed event through one of the three policies. It is
//! intentionally decoupled from `Subscription<T>` so unit tests can drive
//! it without spinning up a tail loop.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::super::log::Seq;
use super::options::FailurePolicy;

/// Structured failure reason from a consumer's `nack()`.
#[derive(Debug, Clone)]
pub struct FailureInfo {
    pub subscription_name: String,
    pub seq: Seq,
    pub reason: String,
    pub payload_snapshot: Option<serde_json::Value>,
    pub attempt: u8,
}

/// Errors returned while routing a failure.
#[derive(Debug, thiserror::Error)]
pub enum FailureError {
    #[error("dlq backend error: {0}")]
    Dlq(String),

    #[error("halt triggered for subscription {subscription_name:?}: {reason}")]
    Halted {
        subscription_name: String,
        reason: String,
    },

    #[error("encode dlq payload: {0}")]
    Encode(#[from] serde_json::Error),
}

#[cfg(feature = "postgres")]
impl From<sqlx::Error> for FailureError {
    fn from(e: sqlx::Error) -> Self {
        FailureError::Dlq(e.to_string())
    }
}

/// Outcome of routing a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureOutcome {
    /// Consumer should retry after `delay`. Caller sleeps then re-invokes
    /// the handler with `attempt + 1`.
    Retry {
        delay: std::time::Duration,
        next_attempt: u8,
    },
    /// Event was written to DLQ; cursor must advance past `seq`.
    Dlqd,
    /// Halt the subscription. Caller marks the Subscription as terminated.
    Halt,
}

/// Abstract DLQ writer so unit tests can plug an in-memory buffer.
#[async_trait]
pub trait DlqSink: Send + Sync {
    async fn record(&self, info: &FailureInfo) -> Result<(), FailureError>;
}

/// In-memory DLQ — used by unit tests and fallback boot path.
#[derive(Clone, Default)]
pub struct InMemoryDlq {
    inner: Arc<Mutex<Vec<FailureInfo>>>,
}

impl InMemoryDlq {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshot(&self) -> Vec<FailureInfo> {
        self.inner.lock().await.clone()
    }
}

#[async_trait]
impl DlqSink for InMemoryDlq {
    async fn record(&self, info: &FailureInfo) -> Result<(), FailureError> {
        self.inner.lock().await.push(info.clone());
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────
// PG DLQ
// ─────────────────────────────────────────────────────────────────────────

#[cfg(feature = "postgres")]
pub use pg::PgDlqSink;

#[cfg(feature = "postgres")]
mod pg {
    use super::*;
    use sqlx::PgPool;

    /// PostgreSQL-backed [`DlqSink`] — writes into `dead_letter_queue`.
    #[derive(Clone)]
    pub struct PgDlqSink {
        pool: PgPool,
    }

    impl PgDlqSink {
        pub fn new(pool: PgPool) -> Self {
            Self { pool }
        }
    }

    #[async_trait]
    impl DlqSink for PgDlqSink {
        async fn record(&self, info: &FailureInfo) -> Result<(), FailureError> {
            sqlx::query(
                r#"
                INSERT INTO dead_letter_queue
                    (subscription_name, event_seq, failure_reason, payload_snapshot)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(&info.subscription_name)
            .bind(info.seq.0)
            .bind(&info.reason)
            .bind(info.payload_snapshot.clone())
            .execute(&self.pool)
            .await?;
            Ok(())
        }
    }
}

/// Routes a failure through its policy.
///
/// The router is called exactly once per `nack()`; it knows nothing about the
/// subscription's topic — its only input is the `FailureInfo` + policy.
pub struct FailureRouter {
    policy: FailurePolicy,
    dlq: Arc<dyn DlqSink>,
}

impl FailureRouter {
    pub fn new(policy: FailurePolicy, dlq: Arc<dyn DlqSink>) -> Self {
        Self { policy, dlq }
    }

    /// Apply the policy to `info` and return the next step.
    pub async fn route(&self, info: &FailureInfo) -> Result<FailureOutcome, FailureError> {
        match self.policy {
            FailurePolicy::Retry { max, backoff } => {
                if info.attempt < max {
                    let next_attempt = info.attempt + 1;
                    Ok(FailureOutcome::Retry {
                        delay: backoff.delay(next_attempt),
                        next_attempt,
                    })
                } else {
                    // Retry budget exhausted — fall through to SkipToDLQ so
                    // the consumer never permanently stalls.
                    self.dlq.record(info).await?;
                    Ok(FailureOutcome::Dlqd)
                }
            }
            FailurePolicy::SkipToDLQ => {
                self.dlq.record(info).await?;
                Ok(FailureOutcome::Dlqd)
            }
            FailurePolicy::Halt => {
                // Record into DLQ so ops can inspect the offending event —
                // but the subscription still stops.
                let _ = self.dlq.record(info).await;
                Ok(FailureOutcome::Halt)
            }
        }
    }

    pub fn policy(&self) -> FailurePolicy {
        self.policy
    }
}

/// Helper to craft a `FailureInfo` from the live nack path.
pub fn build_failure_info(
    subscription_name: &str,
    seq: Seq,
    reason: &str,
    payload: Option<serde_json::Value>,
    attempt: u8,
) -> FailureInfo {
    FailureInfo {
        subscription_name: subscription_name.to_string(),
        seq,
        reason: reason.to_string(),
        payload_snapshot: payload,
        attempt,
    }
}

#[cfg(test)]
mod tests {
    use super::super::options::BackoffKind;
    use super::*;
    use std::time::Duration;

    fn sample_info(attempt: u8) -> FailureInfo {
        build_failure_info(
            "sub-test",
            Seq(42),
            "boom",
            Some(serde_json::json!({ "x": 1 })),
            attempt,
        )
    }

    #[tokio::test]
    async fn retry_under_max_returns_retry_outcome() {
        let dlq = Arc::new(InMemoryDlq::new());
        let router = FailureRouter::new(
            FailurePolicy::Retry {
                max: 3,
                backoff: BackoffKind::Fixed(Duration::from_millis(10)),
            },
            dlq.clone(),
        );

        let out = router.route(&sample_info(0)).await.unwrap();
        match out {
            FailureOutcome::Retry {
                delay,
                next_attempt,
            } => {
                assert_eq!(delay, Duration::from_millis(10));
                assert_eq!(next_attempt, 1);
            }
            other => panic!("expected retry, got {other:?}"),
        }
        // No DLQ entry yet.
        assert!(dlq.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn retry_exceeds_max_falls_through_to_dlq() {
        let dlq = Arc::new(InMemoryDlq::new());
        let router = FailureRouter::new(
            FailurePolicy::Retry {
                max: 3,
                backoff: BackoffKind::Fixed(Duration::from_millis(10)),
            },
            dlq.clone(),
        );

        // attempt == max means budget exhausted (the policy limit is the
        // total attempt count; after `max` failures the next call stops).
        let out = router.route(&sample_info(3)).await.unwrap();
        assert_eq!(out, FailureOutcome::Dlqd);
        assert_eq!(dlq.snapshot().await.len(), 1);
    }

    #[tokio::test]
    async fn skip_to_dlq_always_records() {
        let dlq = Arc::new(InMemoryDlq::new());
        let router = FailureRouter::new(FailurePolicy::SkipToDLQ, dlq.clone());

        let out = router.route(&sample_info(0)).await.unwrap();
        assert_eq!(out, FailureOutcome::Dlqd);
        let snap = dlq.snapshot().await;
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].reason, "boom");
        assert_eq!(snap[0].seq, Seq(42));
    }

    #[tokio::test]
    async fn halt_returns_halt_outcome_and_records_dlq() {
        let dlq = Arc::new(InMemoryDlq::new());
        let router = FailureRouter::new(FailurePolicy::Halt, dlq.clone());

        let out = router.route(&sample_info(0)).await.unwrap();
        assert_eq!(out, FailureOutcome::Halt);
        // DLQ still has the record for ops visibility.
        assert_eq!(dlq.snapshot().await.len(), 1);
    }

    #[tokio::test]
    async fn retry_exp_backoff_grows_with_attempt() {
        let dlq = Arc::new(InMemoryDlq::new());
        let router = FailureRouter::new(
            FailurePolicy::Retry {
                max: 5,
                backoff: BackoffKind::Exponential {
                    base: Duration::from_millis(100),
                    cap: Duration::from_secs(10),
                },
            },
            dlq,
        );

        // attempt 0 → next_attempt 1 → 100ms
        let delay1 = match router.route(&sample_info(0)).await.unwrap() {
            FailureOutcome::Retry { delay, .. } => delay,
            _ => panic!(),
        };
        // attempt 2 → next_attempt 3 → 400ms
        let delay3 = match router.route(&sample_info(2)).await.unwrap() {
            FailureOutcome::Retry { delay, .. } => delay,
            _ => panic!(),
        };
        assert!(delay3 > delay1, "exp backoff should grow");
    }
}
