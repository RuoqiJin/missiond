//! Subscription combinators — declarative wrappers that preserve [`Ack`]
//! semantics.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3 subscription-
//! combinators. Each combinator lives in its own file; this module hosts
//! the trait-level entry methods, the shared `wrap_ack` helper, and the
//! internal `Ack::__new_for_combinator` / `silent_ack` primitives the
//! combinators rely on.
//!
//! Each combinator consumes a [`Subscription<T>`] and exposes a
//! `next().await` that returns an `Ack<U>` (same ack plumbing, wrapped
//! event). The implementation sticks to primitive `tokio` building blocks
//! — no `tokio_stream` dependency was added.
//!
//! Key design points:
//!
//! * **ack fidelity** — `debounce` / `rate_limit` / `coalesce` / `batch`
//!   squash multiple source acks into a single downstream ack. The last
//!   source event wins (its seq becomes the downstream ack's seq) and the
//!   earlier events are auto-acked so the cursor can advance through them.
//! * **filter** — dropped events are auto-acked so the subscription makes
//!   progress. Use `FailurePolicy` if you want failure routing instead.
//! * **map** — a pure transform; seq/ack routing unchanged.
//!
//! These combinators are zero-copy over [`Arc<T>`] wherever possible; the
//! `batch` combinator clones the `Arc` into the batch vector.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

use super::super::event_trait::DomainEvent;
use super::super::log::Seq;
use super::{Ack, FlushSignal, Subscription, SubscriptionState};

pub mod batch;
pub mod coalesce;
pub mod debounce;
pub mod filter;
pub mod map;
pub mod rate_limit;

pub use batch::{BatchedSubscription, EventBatch};
pub use coalesce::CoalescingSubscription;
pub use debounce::DebouncedSubscription;
pub use filter::FilteredSubscription;
pub use map::MappedSubscription;
pub use rate_limit::RateLimitedSubscription;

/// Produce an [`Ack<U>`] that shares the underlying subscription's flush
/// channel + shared state. Used by combinators that emit a new wrapper
/// event type.
pub(crate) fn wrap_ack<U: DomainEvent>(
    event: Arc<U>,
    seq: Seq,
    attempt: u8,
    shared: Arc<Mutex<SubscriptionState>>,
    flush_signal: tokio::sync::mpsc::Sender<FlushSignal>,
) -> Ack<U> {
    // SAFETY: Ack fields are private; construct via internal constructor.
    Ack::__new_for_combinator(event, seq, attempt, shared, flush_signal)
}

impl<T: DomainEvent> Ack<T> {
    /// Internal constructor used by combinators to fabricate a new Ack that
    /// still talks back to the source subscription's flusher. Not public.
    pub(crate) fn __new_for_combinator(
        event: Arc<T>,
        seq: Seq,
        attempt: u8,
        shared: Arc<Mutex<SubscriptionState>>,
        flush_signal: tokio::sync::mpsc::Sender<FlushSignal>,
    ) -> Self {
        Ack {
            event,
            seq,
            attempt,
            shared,
            flush_signal,
            consumed: false,
        }
    }

    /// Silently ack — used by combinators to auto-ack squashed events
    /// without surfacing them to the caller. Takes ownership so the
    /// combinator cannot accidentally use the value twice.
    pub(crate) async fn silent_ack(mut self) {
        self.consumed = true;
        {
            let mut st = self.shared.lock().await;
            if self.seq > st.cursor {
                st.cursor = self.seq;
            }
            st.dirty_since_flush = st.dirty_since_flush.saturating_add(1);
        }
        let _ = self.flush_signal.send(FlushSignal::Dirty { count: 1 }).await;
    }
}

impl<T: DomainEvent> Subscription<T> {
    /// Fixed-window debounce: collect events for `window`; emit the last
    /// one received. Earlier events are silently acked.
    pub fn debounce(self, window: Duration) -> DebouncedSubscription<T> {
        DebouncedSubscription { inner: self, window }
    }

    /// Cap the delivery rate to `max_per_sec` events. Excess events wait,
    /// they are not dropped.
    pub fn rate_limit(self, max_per_sec: u32) -> RateLimitedSubscription<T> {
        let interval = if max_per_sec == 0 {
            Duration::from_secs(1)
        } else {
            Duration::from_millis((1000 / max_per_sec).max(1) as u64)
        };
        RateLimitedSubscription {
            inner: self,
            interval,
            last_emit: None,
        }
    }

    /// Coalesce adjacent events via a user-supplied fold. Earlier events
    /// are silently acked.
    pub fn coalesce<F>(self, f: F) -> CoalescingSubscription<T, F>
    where
        F: Fn(&T, &T) -> T + Send + Sync + 'static,
    {
        CoalescingSubscription { inner: self, fold: f }
    }

    /// Pass-through filter. Dropped events are silently acked.
    pub fn filter<F>(self, pred: F) -> FilteredSubscription<T, F>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        FilteredSubscription { inner: self, pred }
    }

    /// Map to a new type. The new `Ack<U>` preserves the source seq and
    /// participates in the same flush plumbing.
    pub fn map<U, F>(self, f: F) -> MappedSubscription<T, U, F>
    where
        U: DomainEvent,
        F: Fn(&T) -> U + Send + Sync + 'static,
    {
        MappedSubscription {
            inner: self,
            map_fn: f,
            _p: std::marker::PhantomData,
        }
    }

    /// Collect up to `max` events or until `window` elapses, whichever
    /// comes first. Emits the batch as one `Ack<Vec<Arc<T>>>`.
    pub fn batch(self, max: usize, window: Duration) -> BatchedSubscription<T> {
        BatchedSubscription {
            inner: self,
            max,
            window,
        }
    }
}

// Suppress unused-import warnings for re-exported items that aren't
// referenced in this file's impl blocks directly (Instant is used via
// tests only; keep for documentation-level consistency).
#[allow(dead_code)]
fn _marker_types_referenced() -> Option<Instant> {
    None
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::dispatcher::{register_all_domains, DispatcherBuilder};
    use crate::event::events::BoardEvent;
    use crate::event::log::reader::LoggedEvent;
    use crate::event::log::{AppendAck, AppendError, AppendOpts, Log, LogError, LogReadable};
    use crate::event::subscription::failure::InMemoryDlq;
    use crate::event::subscription::{subscribe, InMemoryCursorStore, StartFrom, SubscriptionOpts};
    use crate::event::{Domain, DomainEvent};
    use async_trait::async_trait;
    use std::time::Duration;

    fn board(i: usize) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: format!("t-{i}"),
            title: "t".into(),
            category: "c".into(),
        }
    }

    fn logged(seq: i64) -> LoggedEvent {
        let ev = board(seq as usize);
        LoggedEvent {
            seq: Seq(seq),
            domain: Domain::Board,
            kind: ev.kind().into(),
            payload: serde_json::to_value(&ev).unwrap(),
            producer_id: "t".into(),
            dedupe_key: None,
            causation_depth: 0,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            ts: chrono::Utc::now(),
            ephemeral: false,
        }
    }

    struct StaticLog(Vec<LoggedEvent>);

    #[async_trait]
    impl Log for StaticLog {
        async fn append<E>(&self, _: E, _: AppendOpts) -> Result<AppendAck, AppendError>
        where
            E: DomainEvent,
        {
            unimplemented!()
        }
        async fn read_from(
            &self,
            domain: Domain,
            after: Seq,
            limit: usize,
        ) -> Result<Vec<LoggedEvent>, LogError> {
            Ok(self
                .0
                .iter()
                .filter(|e| e.domain == domain && e.seq > after)
                .take(limit)
                .cloned()
                .collect())
        }
        async fn head_seq(&self) -> Result<Seq, LogError> {
            Ok(Seq(self.0.iter().map(|e| e.seq.0).max().unwrap_or(0)))
        }
    }

    async fn make_sub(
        events: Vec<LoggedEvent>,
        name: &str,
    ) -> Subscription<BoardEvent> {
        let log: Arc<dyn LogReadable> = Arc::new(StaticLog(events));
        let store = Arc::new(InMemoryCursorStore::new());
        let dlq = Arc::new(InMemoryDlq::new());
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let topic = dispatcher.topic::<BoardEvent>();
        let mut opts = SubscriptionOpts::named("c");
        opts.start_from = StartFrom::Earliest;
        subscribe::<BoardEvent>(name, opts, log, topic, store, dlq)
            .await
            .expect("subscribe ok")
    }

    #[tokio::test]
    async fn filter_drops_non_matching_and_silent_acks() {
        let events = vec![logged(1), logged(2), logged(3)];
        let sub = make_sub(events, "filter-t").await;

        // Filter keeps only events with task_id == "t-2".
        let mut filtered = sub.filter(|ev: &BoardEvent| match ev {
            BoardEvent::TaskCreated { task_id, .. } => task_id == "t-2",
            _ => false,
        });

        let ack = tokio::time::timeout(Duration::from_secs(1), filtered.next())
            .await
            .expect("timeout")
            .expect("ack");
        match ack.event() {
            BoardEvent::TaskCreated { task_id, .. } => assert_eq!(task_id, "t-2"),
            _ => panic!(),
        }
        assert_eq!(ack.seq(), Seq(2));
        ack.ack().await;
        // By the time we reach seq 2 through the filter, seq 1 must already
        // be silently acked. Cursor >= 2.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(filtered.cursor().await.0 >= 2);
    }

    #[tokio::test]
    async fn map_translates_event_preserving_seq() {
        let events = vec![logged(1)];
        let sub = make_sub(events, "map-t").await;
        let mut mapped = sub.map(|ev: &BoardEvent| ev.clone());

        let ack = mapped.next().await.expect("ack");
        assert_eq!(ack.seq(), Seq(1));
        ack.ack().await;
    }

    #[tokio::test]
    async fn debounce_emits_only_the_latest_within_window() {
        let events: Vec<_> = (1..=5).map(logged).collect();
        let sub = make_sub(events, "debounce-t").await;
        let mut deb = sub.debounce(Duration::from_millis(100));

        let ack = tokio::time::timeout(Duration::from_secs(2), deb.next())
            .await
            .expect("timeout")
            .expect("ack");
        // The last event should win — seq 5.
        assert_eq!(ack.seq(), Seq(5));
        ack.ack().await;
    }

    #[tokio::test]
    async fn coalesce_merges_adjacent_events() {
        let events = vec![logged(1), logged(2), logged(3)];
        let sub = make_sub(events, "coalesce-t").await;

        // Fold that replaces previous with new — effectively "latest wins".
        let mut coal = sub.coalesce(|_prev: &BoardEvent, new: &BoardEvent| new.clone());

        let ack = tokio::time::timeout(Duration::from_secs(2), coal.next())
            .await
            .expect("timeout")
            .expect("ack");
        // Last event in the merge.
        assert_eq!(ack.seq(), Seq(3));
    }

    #[tokio::test]
    async fn rate_limit_enforces_min_spacing() {
        let events: Vec<_> = (1..=4).map(logged).collect();
        let sub = make_sub(events, "rl-t").await;
        let mut rl = sub.rate_limit(10); // 100ms per event

        let start = Instant::now();
        for _ in 0..3 {
            let ack = rl.next().await.expect("ack");
            ack.ack().await;
        }
        // First event is immediate; next two enforce >= 200ms combined.
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(150),
            "rate limit too fast: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn batch_aggregates_up_to_max_within_window() {
        let events: Vec<_> = (1..=5).map(logged).collect();
        let sub = make_sub(events, "batch-t").await;
        let mut b = sub.batch(10, Duration::from_millis(100));

        let (items, tail_ack) = tokio::time::timeout(Duration::from_secs(2), b.next())
            .await
            .expect("timeout")
            .expect("batch");
        assert_eq!(items.len(), 5);
        assert_eq!(tail_ack.seq(), Seq(5));
        tail_ack.ack().await;
    }

    #[tokio::test]
    async fn batch_respects_max_cap() {
        let events: Vec<_> = (1..=10).map(logged).collect();
        let sub = make_sub(events, "batch-cap-t").await;
        let mut b = sub.batch(3, Duration::from_secs(5));

        let (items, tail_ack) = tokio::time::timeout(Duration::from_secs(3), b.next())
            .await
            .expect("timeout")
            .expect("batch");
        assert_eq!(items.len(), 3);
        assert_eq!(tail_ack.seq(), Seq(3));
    }
}
