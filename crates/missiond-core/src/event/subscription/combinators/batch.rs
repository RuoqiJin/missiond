//! Batch combinator — aggregate up to `max` events or until `window`
//! elapses, whichever comes first. The tail ack controls cursor
//! advancement across the whole batch.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{self, Instant};

use super::super::super::event_trait::DomainEvent;
use super::super::super::log::Seq;
use super::super::{Ack, Subscription};
use super::wrap_ack;

pub struct BatchedSubscription<T: DomainEvent> {
    pub(super) inner: Subscription<T>,
    pub(super) max: usize,
    pub(super) window: Duration,
}

/// Wrapper type so `BatchedSubscription::next` can return a real
/// `DomainEvent` (same domain as `T`). We piggyback on the source domain;
/// `batch` is a consumer-local view that never hits the event log directly.
#[derive(Debug, Clone)]
pub struct EventBatch<T: DomainEvent> {
    pub items: Vec<Arc<T>>,
    /// The seq of the last event in the batch.
    pub tail_seq: Seq,
}

impl<T: DomainEvent> BatchedSubscription<T> {
    /// Returns `(Vec<Arc<T>>, Ack<T>)` — the batch events are the first
    /// tuple element; the second is the ack handle for the last seq
    /// in the batch. Acking this handle advances the cursor past every
    /// event in the batch.
    pub async fn next(&mut self) -> Option<(Vec<Arc<T>>, Ack<T>)> {
        let first = self.inner.next().await?;
        let mut items = vec![first.event().clone().into()];
        let mut last_seq = first.seq();
        let mut last_attempt = first.attempt();
        let shared = self.inner.shared();
        let flush_tx = self.inner.flush_signal_tx();

        // The first event's ack is silently auto-acked once the final one
        // arrives — we'll issue a combined ack at the tail.
        first.silent_ack().await;

        let deadline = Instant::now() + self.window;
        while items.len() < self.max {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match time::timeout(remaining, self.inner.next()).await {
                Ok(Some(ack)) => {
                    items.push(ack.event().clone().into());
                    last_seq = ack.seq();
                    last_attempt = ack.attempt();
                    ack.silent_ack().await;
                }
                Ok(None) | Err(_) => break,
            }
        }

        // A tail ack we DON'T auto-silent — surfaced to the caller so
        // they can commit only when their batch processing succeeds.
        let tail_event = items.last().cloned().expect("batch non-empty");
        let tail_ack = wrap_ack(tail_event, last_seq, last_attempt, shared, flush_tx);
        Some((items, tail_ack))
    }
}
