//! Coalesce combinator — fold adjacent events via a user-supplied
//! `(prev, new) -> merged` closure.
//!
//! Earlier events are silently acked; a fresh `Ack` is fabricated via
//! [`super::wrap_ack`] carrying the merged event and the tail seq.

use std::sync::Arc;
use std::time::Duration;

use tokio::time;

use super::super::super::event_trait::DomainEvent;
use super::super::{Ack, Subscription};
use super::wrap_ack;

pub struct CoalescingSubscription<T: DomainEvent, F>
where
    F: Fn(&T, &T) -> T + Send + Sync + 'static,
{
    pub(super) inner: Subscription<T>,
    pub(super) fold: F,
}

impl<T: DomainEvent, F> CoalescingSubscription<T, F>
where
    F: Fn(&T, &T) -> T + Send + Sync + 'static,
{
    /// Emit one event after draining a burst. If no further event arrives
    /// within `200ms` the current value is released.
    pub async fn next(&mut self) -> Option<Ack<T>> {
        let first = self.inner.next().await?;
        let mut combined_event = (*first.event()).clone();
        let mut combined_seq = first.seq();
        let mut combined_attempt = first.attempt();
        // Ack the initial event silently — we'll issue a new combined ack.
        let shared = self.inner.shared();
        let flush_tx = self.inner.flush_signal_tx();
        first.silent_ack().await;

        loop {
            match time::timeout(Duration::from_millis(200), self.inner.next()).await {
                Ok(Some(next)) => {
                    combined_event = (self.fold)(&combined_event, next.event());
                    combined_seq = next.seq();
                    combined_attempt = next.attempt();
                    next.silent_ack().await;
                }
                Ok(None) | Err(_) => {
                    return Some(wrap_ack(
                        Arc::new(combined_event),
                        combined_seq,
                        combined_attempt,
                        shared,
                        flush_tx,
                    ));
                }
            }
        }
    }
}
