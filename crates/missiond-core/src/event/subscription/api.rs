//! Subscription API entry point — `bus.subscribe::<T>(name, opts, ...)`.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3 subscription-api.
//!
//! The factory here wires together:
//!
//!   * The [`CursorStore`] — read existing cursor or initialize from
//!     [`StartFrom`].
//!   * The [`Log`] — source for the bootstrap phase.
//!   * A [`LiveSource`] backed by the dispatcher's [`Topic<T>`] — source
//!     for the live phase.
//!   * A background flusher task implementing the double-threshold cursor
//!     flush policy (`batch_size` OR `1s`) and routing `Nack` signals
//!     through the [`FailureRouter`].
//!
//! The wiring is kept separate from [`Subscription`] itself so the factory
//! can grow new parameters without churning the data-carrying struct.

use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;
use tokio::time::{self, Instant};

use super::super::dispatcher::Topic;
use super::super::event_trait::DomainEvent;
use super::super::log::{LogReadable, Seq};
use super::cursor_store::{Cursor, CursorStore};
use super::failure::{build_failure_info, DlqSink, FailureRouter};
use super::lifecycle::Lifecycle;
use super::live_source::{BroadcastLiveSource, LiveSource};
use super::options::{CursorFlush, FailurePolicy, StartFrom, SubscriptionOpts};
use super::{
    FailureOutcome, FlushSignal, FlusherHandle, Subscription, SubscriptionState,
};

/// Errors returned by [`subscribe`].
#[derive(Debug, thiserror::Error)]
pub enum SubscribeError {
    #[error("cursor store error: {0}")]
    Cursor(#[from] super::cursor_store::CursorError),

    #[error("log read error while initialising: {0}")]
    LogInit(String),

    #[error("subscription name must not be empty")]
    EmptyName,
}

/// Subscribe to a typed domain topic.
///
/// `name` must be unique per subscription across the process lifetime — the
/// cursor row in `event_subscriptions` is keyed by this value. Two
/// subscriptions with the same name will race on the cursor and should be
/// avoided.
///
/// The returned [`Subscription`] is already attached to the dispatcher's
/// live topic receiver and preloaded to bootstrap. Callers iterate by
/// `await`-ing [`Subscription::next`].
pub async fn subscribe<T: DomainEvent>(
    name: &str,
    opts: SubscriptionOpts,
    log: Arc<dyn LogReadable>,
    topic: Topic<T>,
    cursor_store: Arc<dyn CursorStore>,
    dlq: Arc<dyn DlqSink>,
) -> Result<Subscription<T>, SubscribeError> {
    if name.is_empty() {
        return Err(SubscribeError::EmptyName);
    }

    let domain = T::domain();

    // 1) Resolve the starting cursor — existing row wins; otherwise we pick
    //    from StartFrom.
    let initial_cursor_seq = match cursor_store.get(name).await? {
        Some(row) => row.last_acked_seq,
        None => resolve_start_from(&opts.start_from, log.as_ref()).await?,
    };

    // 2) Persist an initial row so orphan-cleanup can see the sub.
    let initial_cursor = Cursor {
        subscription_name: name.to_string(),
        consumer_name: opts.consumer_name.clone(),
        domain,
        last_acked_seq: initial_cursor_seq,
        last_seen_at: Utc::now(),
        failure_policy: opts.failure_policy,
    };
    cursor_store.upsert(&initial_cursor).await?;

    // 3) Construct the shared state.
    let shared = Arc::new(Mutex::new(SubscriptionState::new(
        name.to_string(),
        opts.consumer_name.clone(),
        domain,
        opts.failure_policy,
        opts.cursor_flush,
        initial_cursor_seq,
    )));

    // 4) Spawn the background flusher task.
    let (flush_tx, flush_rx) = tokio::sync::mpsc::channel::<FlushSignal>(256);
    let flusher_handle = spawn_flusher(
        name.to_string(),
        opts.cursor_flush,
        opts.batch_size.get(),
        FailureRouter::new(opts.failure_policy, dlq.clone()),
        cursor_store.clone(),
        shared.clone(),
        flush_rx,
        flush_tx.clone(),
    );

    // 5) Attach the live receiver.
    let live: Box<dyn LiveSource<T>> =
        Box::new(BroadcastLiveSource::new(topic.subscribe()));

    // 6) Build the subscription.
    let lifecycle = Lifecycle::<T>::new(initial_cursor_seq, opts.batch_size, domain);
    Ok(Subscription::new(
        name.to_string(),
        shared,
        lifecycle,
        log,
        Some(live),
        flush_tx,
        flusher_handle,
    ))
}

async fn resolve_start_from(
    start_from: &StartFrom,
    log: &dyn LogReadable,
) -> Result<Seq, SubscribeError> {
    match *start_from {
        StartFrom::Latest => log
            .head_seq()
            .await
            .map_err(|e| SubscribeError::LogInit(e.to_string())),
        StartFrom::Earliest => Ok(Seq(0)),
        StartFrom::Seq(s) => Ok(s),
    }
}

/// Spawn the background flusher task — receives [`FlushSignal`]s, enforces
/// the cursor-flush policy, and runs failure routing.
#[allow(clippy::too_many_arguments)]
fn spawn_flusher(
    subscription_name: String,
    flush_policy: CursorFlush,
    batch_threshold: usize,
    router: FailureRouter,
    store: Arc<dyn CursorStore>,
    shared: Arc<Mutex<SubscriptionState>>,
    mut rx: tokio::sync::mpsc::Receiver<FlushSignal>,
    tx: tokio::sync::mpsc::Sender<FlushSignal>,
) -> Arc<FlusherHandle> {
    let handle = Arc::new(FlusherHandle {
        join: Mutex::new(None),
        tx: tx.clone(),
    });
    let handle_ref = handle.clone();

    let join = tokio::spawn(async move {
        let time_window = flush_policy.time_window();
        let mut tick = time::interval(time_window);
        tick.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        tick.tick().await; // consume the immediate first tick

        let mut dirty_since_flush: usize = 0;

        loop {
            tokio::select! {
                maybe_msg = rx.recv() => {
                    match maybe_msg {
                        Some(FlushSignal::Dirty { count }) => {
                            dirty_since_flush = dirty_since_flush.saturating_add(count);
                            if matches!(flush_policy, CursorFlush::PerEvent)
                                || dirty_since_flush >= batch_threshold
                            {
                                do_flush(
                                    &subscription_name,
                                    &store,
                                    &shared,
                                    &mut dirty_since_flush,
                                )
                                .await;
                            }
                        }
                        Some(FlushSignal::Force) => {
                            do_flush(
                                &subscription_name,
                                &store,
                                &shared,
                                &mut dirty_since_flush,
                            )
                            .await;
                        }
                        Some(FlushSignal::Nack { seq, reason, payload, attempt }) => {
                            handle_nack(
                                &subscription_name,
                                &router,
                                &shared,
                                seq,
                                &reason,
                                payload,
                                attempt,
                            )
                            .await;
                        }
                        Some(FlushSignal::Shutdown) | None => {
                            // Final flush before exiting so we don't lose
                            // the last ack cluster.
                            if dirty_since_flush > 0 {
                                do_flush(
                                    &subscription_name,
                                    &store,
                                    &shared,
                                    &mut dirty_since_flush,
                                )
                                .await;
                            }
                            break;
                        }
                    }
                }
                _ = tick.tick() => {
                    if dirty_since_flush > 0 {
                        do_flush(
                            &subscription_name,
                            &store,
                            &shared,
                            &mut dirty_since_flush,
                        )
                        .await;
                    }
                }
            }
        }
    });

    // Stash the join handle on the FlusherHandle so Subscription::drop can
    // await it — best-effort, non-blocking.
    if let Ok(mut guard) = handle_ref.join.try_lock() {
        *guard = Some(join);
    }
    handle_ref
}

async fn do_flush(
    subscription_name: &str,
    store: &Arc<dyn CursorStore>,
    shared: &Arc<Mutex<SubscriptionState>>,
    dirty_since_flush: &mut usize,
) {
    // Take a cursor snapshot, then release the lock before hitting the DB.
    let cursor = {
        let mut st = shared.lock().await;
        let snapshot = st.as_cursor();
        st.dirty_since_flush = 0;
        st.last_flush_at = Instant::now();
        snapshot
    };

    match store.upsert(&cursor).await {
        Ok(()) => {
            *dirty_since_flush = 0;
            tracing::trace!(
                subscription = %subscription_name,
                seq = cursor.last_acked_seq.0,
                "flushed cursor"
            );
        }
        Err(e) => {
            tracing::warn!(
                subscription = %subscription_name,
                error = %e,
                "cursor flush failed; will retry next window"
            );
            // Keep dirty_since_flush non-zero so we retry the flush.
            *dirty_since_flush = dirty_since_flush.saturating_add(1);
        }
    }
}

async fn handle_nack(
    subscription_name: &str,
    router: &FailureRouter,
    shared: &Arc<Mutex<SubscriptionState>>,
    seq: Seq,
    reason: &str,
    payload: Option<serde_json::Value>,
    attempt: u8,
) {
    let info = build_failure_info(subscription_name, seq, reason, payload, attempt);
    match router.route(&info).await {
        Ok(FailureOutcome::Retry { .. }) => {
            tracing::debug!(
                subscription = %subscription_name,
                seq = seq.0,
                attempt = attempt,
                "retry scheduled (runtime holds the event)"
            );
            // The actual re-delivery is owned by Subscription::schedule_retry;
            // this branch exists so router metrics register correctly.
        }
        Ok(FailureOutcome::Dlqd) => {
            // Advance cursor past the DLQ'd seq — otherwise the
            // subscription would replay it on restart.
            let mut st = shared.lock().await;
            if seq > st.cursor {
                st.cursor = seq;
                st.dirty_since_flush = st.dirty_since_flush.saturating_add(1);
            }
        }
        Ok(FailureOutcome::Halt) => {
            let mut st = shared.lock().await;
            st.terminated = true;
            tracing::error!(
                subscription = %subscription_name,
                seq = seq.0,
                reason = %reason,
                "FailurePolicy::Halt triggered; subscription terminated"
            );
        }
        Err(e) => {
            tracing::error!(
                subscription = %subscription_name,
                seq = seq.0,
                error = %e,
                "failure router error"
            );
            // Fail-fast: halt the subscription to surface the problem
            // rather than silently drop.
            shared.lock().await.terminated = true;
        }
    }

    // Ensure we're never holding an unused ref to the signaling FailurePolicy
    // — reference it once to avoid dead_code warnings in production builds.
    let _policy: FailurePolicy = router.policy();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::dispatcher::{DispatcherBuilder, register_all_domains};
    use crate::event::domain::Domain;
    use crate::event::events::BoardEvent;
    use crate::event::log::reader::LoggedEvent;
    use crate::event::log::{AppendAck, AppendError, AppendOpts, Log, LogError, Seq};
    use crate::event::subscription::failure::InMemoryDlq;
    use crate::event::subscription::{CursorFlush, InMemoryCursorStore};
    use async_trait::async_trait;

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

    #[tokio::test]
    async fn subscribe_bootstraps_and_flushes_cursor() {
        let events = vec![logged(1), logged(2), logged(3)];
        let log: Arc<dyn LogReadable> = Arc::new(StaticLog(events));
        let store = Arc::new(InMemoryCursorStore::new());
        let dlq = Arc::new(InMemoryDlq::new());
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let topic = dispatcher.topic::<BoardEvent>();

        let mut opts = SubscriptionOpts::named("consumer-a");
        opts.start_from = StartFrom::Earliest;
        opts.cursor_flush = CursorFlush::PerEvent;

        let mut sub = subscribe::<BoardEvent>(
            "sub-a",
            opts,
            log,
            topic,
            store.clone(),
            dlq.clone(),
        )
        .await
        .expect("subscribe ok");

        for i in 1..=3 {
            let ack = sub.next().await.expect("ack");
            assert_eq!(ack.seq(), Seq(i));
            ack.ack().await;
        }
        // Give the flusher a tick to persist.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let persisted = store.get("sub-a").await.unwrap().unwrap();
        assert_eq!(persisted.last_acked_seq, Seq(3));
        assert_eq!(persisted.consumer_name, "consumer-a");
    }

    #[tokio::test]
    async fn subscribe_rejects_empty_name() {
        let log: Arc<dyn LogReadable> = Arc::new(StaticLog(vec![]));
        let store = Arc::new(InMemoryCursorStore::new());
        let dlq = Arc::new(InMemoryDlq::new());
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let topic = dispatcher.topic::<BoardEvent>();

        let result = subscribe::<BoardEvent>(
            "",
            SubscriptionOpts::default(),
            log,
            topic,
            store,
            dlq,
        )
        .await;
        assert!(matches!(result, Err(SubscribeError::EmptyName)));
    }

    #[tokio::test]
    async fn subscribe_resumes_from_existing_cursor() {
        let events: Vec<_> = (1..=10).map(logged).collect();
        let log: Arc<dyn LogReadable> = Arc::new(StaticLog(events));
        let store = Arc::new(InMemoryCursorStore::new());
        let dlq = Arc::new(InMemoryDlq::new());
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let topic = dispatcher.topic::<BoardEvent>();

        // Pre-seed cursor at seq 5.
        store
            .upsert(&Cursor {
                subscription_name: "sub-r".into(),
                consumer_name: "c".into(),
                domain: Domain::Board,
                last_acked_seq: Seq(5),
                last_seen_at: Utc::now(),
                failure_policy: FailurePolicy::default(),
            })
            .await
            .unwrap();

        let mut opts = SubscriptionOpts::named("c");
        opts.start_from = StartFrom::Earliest; // should be ignored due to pre-existing cursor
        opts.cursor_flush = CursorFlush::PerEvent;

        let mut sub = subscribe::<BoardEvent>(
            "sub-r",
            opts,
            log,
            topic,
            store.clone(),
            dlq,
        )
        .await
        .unwrap();

        // The first event surfaced should be seq 6.
        let ack = sub.next().await.expect("ack");
        assert_eq!(ack.seq(), Seq(6));
        ack.ack().await;
    }

    #[tokio::test]
    async fn start_from_latest_resolves_to_head_seq() {
        let events: Vec<_> = (1..=5).map(logged).collect();
        let log = StaticLog(events);
        let seq = resolve_start_from(&StartFrom::Latest, &log).await.unwrap();
        assert_eq!(seq, Seq(5));
    }

    #[tokio::test]
    async fn start_from_earliest_is_zero() {
        let log = StaticLog(vec![]);
        let seq = resolve_start_from(&StartFrom::Earliest, &log).await.unwrap();
        assert_eq!(seq, Seq(0));
    }

    #[tokio::test]
    async fn start_from_seq_echoes_value() {
        let log = StaticLog(vec![]);
        let seq = resolve_start_from(&StartFrom::Seq(Seq(123)), &log).await.unwrap();
        assert_eq!(seq, Seq(123));
    }
}
