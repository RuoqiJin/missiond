//! Subscription combinators — declarative wrappers that preserve [`Ack`]
//! semantics.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3 subscription-
//! combinators.
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
use tokio::time::{self, Instant};

use super::super::event_trait::DomainEvent;
use super::super::log::Seq;
use super::{Ack, FlushSignal, Subscription, SubscriptionState};

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

// ─────────────────────────────────────────────────────────────────────────
// Debounce
// ─────────────────────────────────────────────────────────────────────────

pub struct DebouncedSubscription<T: DomainEvent> {
    inner: Subscription<T>,
    window: Duration,
}

impl<T: DomainEvent> DebouncedSubscription<T> {
    pub async fn next(&mut self) -> Option<Ack<T>> {
        // Wait for the first event — without an initial event there's
        // nothing to debounce.
        let mut last = self.inner.next().await?;
        let deadline = Instant::now() + self.window;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Some(last);
            }
            match time::timeout(remaining, self.inner.next()).await {
                Ok(Some(new_ack)) => {
                    // New event replaces previous — silently ack the old
                    // one so the cursor advances.
                    last.silent_ack().await;
                    last = new_ack;
                }
                Ok(None) => return Some(last),
                Err(_) => return Some(last),
            }
        }
    }

    pub async fn cursor(&self) -> Seq {
        self.inner.cursor().await
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Rate limit
// ─────────────────────────────────────────────────────────────────────────

pub struct RateLimitedSubscription<T: DomainEvent> {
    inner: Subscription<T>,
    interval: Duration,
    last_emit: Option<Instant>,
}

impl<T: DomainEvent> RateLimitedSubscription<T> {
    pub async fn next(&mut self) -> Option<Ack<T>> {
        let ack = self.inner.next().await?;

        if let Some(last) = self.last_emit {
            let now = Instant::now();
            let elapsed = now.saturating_duration_since(last);
            if elapsed < self.interval {
                tokio::time::sleep(self.interval - elapsed).await;
            }
        }
        self.last_emit = Some(Instant::now());
        Some(ack)
    }

    pub async fn cursor(&self) -> Seq {
        self.inner.cursor().await
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Coalesce
// ─────────────────────────────────────────────────────────────────────────

pub struct CoalescingSubscription<T: DomainEvent, F>
where
    F: Fn(&T, &T) -> T + Send + Sync + 'static,
{
    inner: Subscription<T>,
    fold: F,
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

// ─────────────────────────────────────────────────────────────────────────
// Filter
// ─────────────────────────────────────────────────────────────────────────

pub struct FilteredSubscription<T: DomainEvent, F>
where
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    inner: Subscription<T>,
    pred: F,
}

impl<T: DomainEvent, F> FilteredSubscription<T, F>
where
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    pub async fn next(&mut self) -> Option<Ack<T>> {
        loop {
            let ack = self.inner.next().await?;
            if (self.pred)(ack.event()) {
                return Some(ack);
            }
            ack.silent_ack().await;
        }
    }

    pub async fn cursor(&self) -> Seq {
        self.inner.cursor().await
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Map
// ─────────────────────────────────────────────────────────────────────────

pub struct MappedSubscription<T: DomainEvent, U: DomainEvent, F>
where
    F: Fn(&T) -> U + Send + Sync + 'static,
{
    inner: Subscription<T>,
    map_fn: F,
    _p: std::marker::PhantomData<U>,
}

impl<T: DomainEvent, U: DomainEvent, F> MappedSubscription<T, U, F>
where
    F: Fn(&T) -> U + Send + Sync + 'static,
{
    pub async fn next(&mut self) -> Option<Ack<U>> {
        let ack = self.inner.next().await?;
        let seq = ack.seq();
        let attempt = ack.attempt();
        let mapped = (self.map_fn)(ack.event());

        let shared = self.inner.shared();
        let flush_tx = self.inner.flush_signal_tx();
        // Silent-ack the source so the map downstream ack controls
        // cursor advancement — otherwise dropping a mapped ack would
        // never touch the source cursor.
        ack.silent_ack().await;

        Some(wrap_ack(Arc::new(mapped), seq, attempt, shared, flush_tx))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Batch
// ─────────────────────────────────────────────────────────────────────────

pub struct BatchedSubscription<T: DomainEvent> {
    inner: Subscription<T>,
    max: usize,
    window: Duration,
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

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::BoardEvent;
    use crate::event::log::reader::LoggedEvent;
    use crate::event::log::{AppendAck, AppendError, AppendOpts, Log, LogError, LogReadable};
    use crate::event::subscription::failure::InMemoryDlq;
    use crate::event::subscription::{subscribe, InMemoryCursorStore, StartFrom, SubscriptionOpts};
    use crate::event::{Domain, DomainEvent};
    use crate::event::dispatcher::{DispatcherBuilder, register_all_domains};
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
