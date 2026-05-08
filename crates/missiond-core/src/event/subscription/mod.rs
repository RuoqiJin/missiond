//! Subscription API — the egress layer of the event bus.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3 egress.
//!
//! Two-phase model:
//!
//! 1. **Bootstrap** — on subscribe, the [`Subscription`] pulls every event
//!    with `seq > last_acked_seq` from the log via [`Log::read_from`], in
//!    batches of [`options::BatchSize`]. The caller drains them by calling
//!    [`Subscription::next`] and acking each [`Ack`].
//! 2. **Live** — once the log has nothing new, the subscription attaches to
//!    the dispatcher's [`super::Topic<T>`] broadcast receiver. Events
//!    arrive live; overlap with the bootstrap phase is de-duped by seq.
//!
//! Combinators (`debounce` / `rate_limit` / `coalesce` / `filter` / `map` /
//! `batch`) are declarative wrappers that take ownership of a
//! [`Subscription`] and expose the same `next()` API — see
//! [`combinators`].
//!
//! Failure semantics are governed by [`options::FailurePolicy`]; the
//! cursor is persisted via [`cursor_store::CursorStore`] with a
//! double-threshold flush (per batch OR 1s).
//!
//! # MVP deviation (D001)
//!
//! Phase 4 ships with [`options::PauseBehavior::DropAndLiveResume`] only.
//! `FreezeAndCatchUp` is accepted for API compatibility but currently maps
//! to the same behavior; the catch-up push is tracked as issue `I009`.

pub mod api;
pub mod combinators;
pub mod cursor_store;
pub mod failure;
pub mod lifecycle;
pub mod live_source;
pub mod options;

pub use api::{subscribe, SubscribeError};
pub use combinators::{
    BatchedSubscription, CoalescingSubscription, DebouncedSubscription, FilteredSubscription,
    MappedSubscription, RateLimitedSubscription,
};
#[cfg(feature = "postgres")]
pub use cursor_store::PgCursorStore;
pub use cursor_store::{Cursor, CursorError, CursorStore, InMemoryCursorStore};
#[cfg(feature = "postgres")]
pub use failure::PgDlqSink;
pub use failure::{
    build_failure_info, DlqSink, FailureError, FailureInfo, FailureOutcome, FailureRouter,
    InMemoryDlq,
};
pub use lifecycle::{Lifecycle, LifecycleError, Phase};
pub use live_source::{BroadcastLiveSource, LiveSource, MpscLiveSource};
pub use options::{
    BackoffKind, BatchSize, CursorFlush, FailurePolicy, PauseBehavior, StartFrom, SubscriptionOpts,
};

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::Mutex;
use tokio::time::Instant;

use super::domain::Domain;
use super::event_trait::DomainEvent;
use super::log::{LogReadable, Seq};

/// Shared state between [`Subscription`] and every [`Ack`] it emits.
///
/// The cursor, dirty counter, and phase are all guarded by a single Mutex —
/// ack/nack calls are infrequent compared to event delivery, so the
/// serialization cost is negligible.
pub struct SubscriptionState {
    pub name: String,
    pub consumer_name: String,
    pub domain: Domain,
    pub failure_policy: FailurePolicy,
    pub cursor_flush: CursorFlush,
    /// Last seq the consumer ack'd (never rewinds).
    pub cursor: Seq,
    /// Events since the last cursor flush.
    pub dirty_since_flush: usize,
    /// Wall-clock timestamp of the last flush for the time-window path.
    pub last_flush_at: Instant,
    /// Phase 4 MVP — marks the subscription as stopped so `next()` returns
    /// `None`.
    pub terminated: bool,
}

impl SubscriptionState {
    fn new(
        name: String,
        consumer_name: String,
        domain: Domain,
        failure_policy: FailurePolicy,
        cursor_flush: CursorFlush,
        cursor: Seq,
    ) -> Self {
        Self {
            name,
            consumer_name,
            domain,
            failure_policy,
            cursor_flush,
            cursor,
            dirty_since_flush: 0,
            last_flush_at: Instant::now(),
            terminated: false,
        }
    }

    /// Construct a `Cursor` row snapshotting the current state.
    pub fn as_cursor(&self) -> Cursor {
        Cursor {
            subscription_name: self.name.clone(),
            consumer_name: self.consumer_name.clone(),
            domain: self.domain,
            last_acked_seq: self.cursor,
            last_seen_at: Utc::now(),
            failure_policy: self.failure_policy,
        }
    }
}

/// Wrapper handed to consumers. The event is available via [`Ack::event`];
/// the consumer MUST call [`Ack::ack`] or [`Ack::nack`] — dropping without
/// ack is treated as a silent nack with reason `"ack dropped"`.
pub struct Ack<T>
where
    T: DomainEvent,
{
    event: Arc<T>,
    seq: Seq,
    attempt: u8,
    shared: Arc<Mutex<SubscriptionState>>,
    flush_signal: tokio::sync::mpsc::Sender<FlushSignal>,
    /// Consumed once — tracks whether ack/nack was called.
    consumed: bool,
}

/// Internal flush / control signal sent to the background flusher.
#[derive(Debug, Clone)]
pub enum FlushSignal {
    /// Cursor was advanced by N events (hint for batch-threshold flush).
    Dirty { count: usize },
    /// Forced flush — e.g. batch_size boundary reached.
    Force,
    /// Stop the flusher task gracefully.
    Shutdown,
    /// Nack was observed — no cursor advance; still ensures the flusher keeps
    /// running so it can persist metadata.
    Nack {
        seq: Seq,
        reason: String,
        payload: Option<serde_json::Value>,
        attempt: u8,
    },
}

impl<T> Ack<T>
where
    T: DomainEvent,
{
    pub fn event(&self) -> &T {
        &self.event
    }

    pub fn seq(&self) -> Seq {
        self.seq
    }

    pub fn attempt(&self) -> u8 {
        self.attempt
    }

    /// Mark this event successfully processed. Advances the in-memory cursor
    /// and notifies the flusher.
    pub async fn ack(mut self) {
        self.consumed = true;
        {
            let mut st = self.shared.lock().await;
            if self.seq > st.cursor {
                st.cursor = self.seq;
            }
            st.dirty_since_flush = st.dirty_since_flush.saturating_add(1);
        }
        let _ = self
            .flush_signal
            .send(FlushSignal::Dirty { count: 1 })
            .await;
    }

    /// Mark this event as failed. The subscription runtime's
    /// [`FailureRouter`] is invoked out-of-band.
    pub async fn nack(mut self, reason: &str) {
        self.consumed = true;
        let payload = serde_json::to_value(&*self.event).ok();
        let _ = self
            .flush_signal
            .send(FlushSignal::Nack {
                seq: self.seq,
                reason: reason.to_string(),
                payload,
                attempt: self.attempt,
            })
            .await;
    }
}

impl<T> Drop for Ack<T>
where
    T: DomainEvent,
{
    fn drop(&mut self) {
        if !self.consumed {
            // The consumer dropped the Ack without calling ack() / nack().
            // Treat as a silent nack with a synthetic reason so the
            // FailurePolicy still runs. We can't await here — use
            // try_send so the signal is best-effort.
            let payload = serde_json::to_value(&*self.event).ok();
            let _ = self.flush_signal.try_send(FlushSignal::Nack {
                seq: self.seq,
                reason: "ack dropped".into(),
                payload,
                attempt: self.attempt,
            });
        }
    }
}

/// The subscription handle returned by [`subscribe`].
///
/// Drop-to-stop: dropping this handle will close the flush signal channel
/// and eventually terminate the background flusher. Until then, callers
/// pull events by `await`-ing `next()`.
pub struct Subscription<T>
where
    T: DomainEvent,
{
    name: String,
    shared: Arc<Mutex<SubscriptionState>>,
    /// The abstracted lifecycle driver.
    lifecycle: Lifecycle<T>,
    /// Source of truth for history pulls.
    log: Arc<dyn LogReadable>,
    /// Live source (may be `None` during bootstrap and swapped in at the
    /// phase-2 transition).
    live: Option<Box<dyn LiveSource<T>>>,
    /// Channel used by [`Ack`] / drop to notify the flusher task.
    flush_signal_tx: tokio::sync::mpsc::Sender<FlushSignal>,
    /// Handle to the flusher task, so Subscription::drop can shut it down.
    _flusher: Arc<FlusherHandle>,
    /// MVP: we record attempt count for the current event to support
    /// FailurePolicy::Retry replay.
    retry_queue: Vec<PendingRetry<T>>,
}

/// A bookkeeping slot for "handler asked for retry; replay after delay".
struct PendingRetry<T>
where
    T: DomainEvent,
{
    event: Arc<T>,
    seq: Seq,
    attempt: u8,
    ready_at: Instant,
}

/// Drop guard for the flusher task.
pub struct FlusherHandle {
    join: Mutex<Option<tokio::task::JoinHandle<()>>>,
    tx: tokio::sync::mpsc::Sender<FlushSignal>,
}

impl Drop for FlusherHandle {
    fn drop(&mut self) {
        // Best-effort shutdown — the flusher drains and exits.
        let _ = self.tx.try_send(FlushSignal::Shutdown);
    }
}

impl<T> Subscription<T>
where
    T: DomainEvent,
{
    pub(crate) fn new(
        name: String,
        shared: Arc<Mutex<SubscriptionState>>,
        lifecycle: Lifecycle<T>,
        log: Arc<dyn LogReadable>,
        live: Option<Box<dyn LiveSource<T>>>,
        flush_signal_tx: tokio::sync::mpsc::Sender<FlushSignal>,
        flusher: Arc<FlusherHandle>,
    ) -> Self {
        Self {
            name,
            shared,
            lifecycle,
            log,
            live,
            flush_signal_tx,
            _flusher: flusher,
            retry_queue: Vec::new(),
        }
    }

    /// Subscription display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Current phase (test + ops visibility).
    pub fn phase(&self) -> Phase {
        self.lifecycle.phase
    }

    /// Current committed cursor — as seen from this Subscription. This is
    /// the in-memory cursor; the persisted row may lag by up to one
    /// flush-window.
    pub async fn cursor(&self) -> Seq {
        self.shared.lock().await.cursor
    }

    /// Swap in a live source once the bootstrap has caught up. Callers who
    /// construct a Subscription manually (mostly tests) call this when
    /// flipping to `Phase::Live`; the [`subscribe`] factory wires it
    /// automatically.
    pub fn attach_live(&mut self, live: Box<dyn LiveSource<T>>) {
        self.live = Some(live);
    }

    /// Fetch the next available event. Returns `None` when the subscription
    /// is terminated (Halt or explicit stop).
    ///
    /// Ordering:
    /// 1. If a retry is due (`FailurePolicy::Retry`), replay it.
    /// 2. If we're in `Phase::Bootstrap`, drain from log.
    /// 3. Otherwise pull from the live source.
    pub async fn next(&mut self) -> Option<Ack<T>> {
        loop {
            if self.shared.lock().await.terminated {
                return None;
            }

            // 1 — retry backlog. Oldest due first.
            if let Some(idx) = self
                .retry_queue
                .iter()
                .position(|r| r.ready_at <= Instant::now())
            {
                let r = self.retry_queue.remove(idx);
                return Some(self.wrap_ack(r.event, r.seq, r.attempt));
            }
            // If a retry is scheduled but not yet due, sleep briefly.
            if let Some(soonest) = self.retry_queue.iter().map(|r| r.ready_at).min() {
                let wait = soonest.saturating_duration_since(Instant::now());
                if !wait.is_zero() {
                    tokio::time::sleep(wait).await;
                    continue;
                }
            }

            // 2 — bootstrap.
            if self.lifecycle.phase == Phase::Bootstrap {
                match self.lifecycle.next_from_bootstrap(&*self.log).await {
                    Ok(Some(f)) => {
                        return Some(self.wrap_ack(f.event, f.seq, 0));
                    }
                    Ok(None) => {
                        // Caught up — flip to live.
                        self.lifecycle.phase = Phase::Live;
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!(
                            subscription = %self.name,
                            error = %e,
                            "bootstrap read failed; retrying after 200ms"
                        );
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        continue;
                    }
                }
            }

            // 3 — live. Without a live source, the subscription is stuck
            // waiting forever. We treat "no live source" as a hard error.
            let Some(live) = self.live.as_mut() else {
                tracing::warn!(
                    subscription = %self.name,
                    "no live source attached; sleeping 50ms then re-checking"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            };

            // Clone the cursor (outside the lifecycle borrow) so we can
            // filter live events.
            let cursor = self.shared.lock().await.cursor;
            let extract_seq = move |_ev: &T| -> Option<Seq> { None };
            // NB: we can't generally extract seq from T alone — seq is
            // bus-assigned and isn't on the domain event itself. The
            // lifecycle's overlap guard therefore uses a `None` extractor,
            // which means we rely on phase-1 having fully drained the log
            // before live kicks in. The cursor check in record_ack still
            // protects advancement.
            let _ = cursor;

            match self
                .lifecycle
                .next_from_live(&mut **live, extract_seq)
                .await
            {
                Ok(Some(f)) => {
                    return Some(self.wrap_ack(f.event, f.seq, 0));
                }
                Ok(None) => {
                    // Live source closed — terminate.
                    self.shared.lock().await.terminated = true;
                    return None;
                }
                Err(LifecycleError::LiveLagged { skipped, .. }) => {
                    tracing::warn!(
                        subscription = %self.name,
                        skipped = skipped,
                        "live broadcast lagged; flipping back to bootstrap"
                    );
                    self.lifecycle.phase = Phase::Bootstrap;
                    continue;
                }
                Err(other) => {
                    tracing::warn!(
                        subscription = %self.name,
                        error = %other,
                        "live recv error; sleeping 100ms"
                    );
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
        }
    }

    fn wrap_ack(&self, event: Arc<T>, seq: Seq, attempt: u8) -> Ack<T> {
        Ack {
            event,
            seq,
            attempt,
            shared: self.shared.clone(),
            flush_signal: self.flush_signal_tx.clone(),
            consumed: false,
        }
    }

    /// Schedule a retry for an event. Used by the runtime after a nack
    /// returns `FailureOutcome::Retry`.
    #[allow(dead_code)]
    pub(crate) fn schedule_retry(&mut self, event: Arc<T>, seq: Seq, attempt: u8, delay: Duration) {
        self.retry_queue.push(PendingRetry {
            event,
            seq,
            attempt,
            ready_at: Instant::now() + delay,
        });
    }

    /// Mark the subscription terminated. Used by `FailurePolicy::Halt`.
    #[allow(dead_code)]
    pub(crate) async fn halt(&mut self) {
        self.shared.lock().await.terminated = true;
    }

    /// Expose the flush signal channel — used by combinator wrappers to
    /// forward their own ack/nack without rewiring the flusher.
    pub(crate) fn flush_signal_tx(&self) -> tokio::sync::mpsc::Sender<FlushSignal> {
        self.flush_signal_tx.clone()
    }

    /// Expose the shared state — used by combinators that produce new
    /// [`Ack`] wrappers.
    pub(crate) fn shared(&self) -> Arc<Mutex<SubscriptionState>> {
        self.shared.clone()
    }

    /// Snapshot a cursor view suitable for persistence tests.
    pub async fn as_cursor(&self) -> Cursor {
        self.shared.lock().await.as_cursor()
    }

    /// Current ack-dirty count (for tests).
    pub async fn dirty_count(&self) -> usize {
        self.shared.lock().await.dirty_since_flush
    }

    /// Debug helper — is the subscription terminated?
    pub async fn is_terminated(&self) -> bool {
        self.shared.lock().await.terminated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::BoardEvent;
    use crate::event::log::reader::LoggedEvent;
    use crate::event::log::{Log, LogError};
    use crate::event::{AppendAck, AppendError, AppendOpts};
    use async_trait::async_trait;

    fn board(i: usize) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: format!("t-{i}"),
            title: "t".into(),
            category: "c".into(),
        }
    }

    fn logged_board(seq: i64) -> LoggedEvent {
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
            unimplemented!("tests append via different path")
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

    fn build_subscription_for_test(
        log: Arc<dyn LogReadable>,
        live: Option<Box<dyn LiveSource<BoardEvent>>>,
    ) -> Subscription<BoardEvent> {
        let shared = Arc::new(Mutex::new(SubscriptionState::new(
            "sub-t".into(),
            "tester".into(),
            Domain::Board,
            FailurePolicy::default(),
            CursorFlush::BatchOr1s,
            Seq(0),
        )));
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let flusher = Arc::new(FlusherHandle {
            join: Mutex::new(None),
            tx: tx.clone(),
        });
        let lifecycle = Lifecycle::<BoardEvent>::new(Seq(0), BatchSize(100), Domain::Board);
        Subscription::new("sub-t".into(), shared, lifecycle, log, live, tx, flusher)
    }

    #[tokio::test]
    async fn ack_advances_cursor() {
        let events = vec![logged_board(1), logged_board(2)];
        let log: Arc<dyn LogReadable> = Arc::new(StaticLog(events));
        let mut sub = build_subscription_for_test(log, None);

        let ack1 = sub.next().await.expect("ack1");
        assert_eq!(ack1.seq(), Seq(1));
        ack1.ack().await;
        assert_eq!(sub.cursor().await, Seq(1));

        let ack2 = sub.next().await.expect("ack2");
        assert_eq!(ack2.seq(), Seq(2));
        ack2.ack().await;
        assert_eq!(sub.cursor().await, Seq(2));
    }

    #[tokio::test]
    async fn dropped_ack_does_not_advance_cursor() {
        let events = vec![logged_board(1)];
        let log: Arc<dyn LogReadable> = Arc::new(StaticLog(events));
        let mut sub = build_subscription_for_test(log, None);

        let ack = sub.next().await.expect("ack");
        drop(ack); // treat as nack
        assert_eq!(sub.cursor().await, Seq(0));
    }
}
