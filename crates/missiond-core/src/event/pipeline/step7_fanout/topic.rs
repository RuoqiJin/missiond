//! Per-domain topic — the live fan-out channel for one `DomainEvent` type.
//!
//! Frozen lisp §4.2.c `topic-registry`:
//!
//! > `fanout-transport`: `tokio::broadcast::channel<Arc<Event>> per Topic`
//!
//! Each registered domain event type `T` gets exactly one
//! `broadcast::Sender<Arc<T>>` with capacity [`super::TOPIC_BUFFER_SIZE`].
//! Subscribers call [`Topic::subscribe`] for a cloned `Receiver<Arc<T>>`.
//!
//! The broadcast channel is chosen (not MPSC) so:
//!   * Fault isolation — a slow subscriber's `Lagged` is local to its
//!     receiver and cannot stall the sender or other receivers.
//!   * Zero-copy fan-out — payload is moved once into `Arc<T>`; every
//!     receiver clones a cheap `Arc`.
//!
//! Slow-consumer handling is intentionally minimal at this layer: Phase 4's
//! subscription API wraps `RecvError::Lagged` in a cursor rewind + an
//! `ObservabilityEvent::SlowConsumer` emission.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::event::domain::Domain;
use crate::event::event_trait::DomainEvent;

/// Typed handle to a domain topic. Clone-cheap (wraps an `Arc` internally).
///
/// Use [`Topic::subscribe`] to register a live subscriber.
pub struct Topic<T: DomainEvent> {
    inner: Arc<TopicInner<T>>,
}

impl<T: DomainEvent> Clone for Topic<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

struct TopicInner<T> {
    tx: broadcast::Sender<Arc<T>>,
    domain: Domain,
}

impl<T: DomainEvent> Topic<T> {
    pub(crate) fn new(buffer_size: usize) -> Self {
        let (tx, _rx) = broadcast::channel(buffer_size);
        Self {
            inner: Arc::new(TopicInner {
                tx,
                domain: T::domain(),
            }),
        }
    }

    /// Register a live subscriber. Receives every event fanned out after
    /// the call returns, up to the broadcast capacity lag.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<T>> {
        self.inner.tx.subscribe()
    }

    /// Send an event to every live subscriber. Returns the number of
    /// receivers notified, or 0 if no one is listening (which is not an
    /// error — fan-out is fire-and-forget at this layer).
    pub fn publish(&self, event: Arc<T>) -> usize {
        match self.inner.tx.send(event) {
            Ok(n) => n,
            Err(broadcast::error::SendError(_)) => 0,
        }
    }

    /// Static domain this topic routes. Same as `T::domain()`.
    pub fn domain(&self) -> Domain {
        self.inner.domain
    }

    /// How many live receivers this topic currently has.
    pub fn receiver_count(&self) -> usize {
        self.inner.tx.receiver_count()
    }
}

impl<T: DomainEvent> std::fmt::Debug for Topic<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Topic")
            .field("domain", &self.inner.domain)
            .field("receivers", &self.inner.tx.receiver_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::BoardEvent;
    use std::time::Duration;

    fn sample_event(task_id: &str) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: task_id.into(),
            title: "t".into(),
            category: "c".into(),
        }
    }

    #[test]
    fn topic_domain_matches_event_type() {
        let t: Topic<BoardEvent> = Topic::new(16);
        assert_eq!(t.domain(), Domain::Board);
    }

    #[tokio::test]
    async fn subscribe_then_publish_delivers_to_receiver() {
        let t: Topic<BoardEvent> = Topic::new(16);
        let mut rx = t.subscribe();

        let sent = Arc::new(sample_event("task-1"));
        let delivered = t.publish(sent.clone());
        assert_eq!(delivered, 1, "one live subscriber expected");

        let got = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("recv timeout")
            .expect("recv ok");
        assert_eq!(&*got, &*sent);
    }

    #[tokio::test]
    async fn publish_without_subscribers_returns_zero() {
        let t: Topic<BoardEvent> = Topic::new(16);
        let delivered = t.publish(Arc::new(sample_event("t")));
        assert_eq!(delivered, 0);
    }

    #[tokio::test]
    async fn multiple_subscribers_each_receive_event() {
        let t: Topic<BoardEvent> = Topic::new(16);
        let mut rx1 = t.subscribe();
        let mut rx2 = t.subscribe();
        let mut rx3 = t.subscribe();

        let sent = Arc::new(sample_event("multi"));
        let delivered = t.publish(sent.clone());
        assert_eq!(delivered, 3);

        for rx in [&mut rx1, &mut rx2, &mut rx3] {
            let got = tokio::time::timeout(Duration::from_millis(200), rx.recv())
                .await
                .expect("timeout")
                .expect("recv");
            assert_eq!(&*got, &*sent);
        }
    }

    #[tokio::test]
    async fn slow_subscriber_lags_without_blocking_sender() {
        // Capacity = 4 — push more than 4 messages before rx reads; the
        // slow consumer should see Lagged while publish still completes.
        let t: Topic<BoardEvent> = Topic::new(4);
        let mut slow_rx = t.subscribe();

        for i in 0..10 {
            let n = t.publish(Arc::new(sample_event(&format!("e{i}"))));
            // Sender never blocks even though receiver is slow.
            assert_eq!(n, 1);
        }

        // Slow receiver sees Lagged eventually — but the sender did NOT
        // block, which is the contract we care about.
        let mut saw_lagged = false;
        let mut saw_any_event = false;
        for _ in 0..20 {
            match slow_rx.try_recv() {
                Ok(_) => saw_any_event = true,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    saw_lagged = true;
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
        assert!(saw_lagged, "expected Lagged on overflowed slow subscriber");
        assert!(saw_any_event, "expected to still see some events");
    }

    #[tokio::test]
    async fn receiver_count_reflects_live_subscribers() {
        let t: Topic<BoardEvent> = Topic::new(8);
        assert_eq!(t.receiver_count(), 0);
        let rx1 = t.subscribe();
        assert_eq!(t.receiver_count(), 1);
        let rx2 = t.subscribe();
        assert_eq!(t.receiver_count(), 2);
        drop(rx1);
        assert_eq!(t.receiver_count(), 1);
        drop(rx2);
        assert_eq!(t.receiver_count(), 0);
    }

    #[tokio::test]
    async fn clone_shares_underlying_channel() {
        let t1: Topic<BoardEvent> = Topic::new(4);
        let t2 = t1.clone();
        let mut rx = t2.subscribe();

        // Publish via t1, receive via a subscription from the clone t2 —
        // they must share the same broadcast channel.
        t1.publish(Arc::new(sample_event("shared")));

        let got = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert!(matches!(&*got, BoardEvent::TaskCreated { .. }));
    }
}
