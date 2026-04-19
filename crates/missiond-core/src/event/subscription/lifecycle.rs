//! Lifecycle — the tail-and-pull state machine that drives a
//! [`super::Subscription<T>`].
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3 subscription-lifecycle:
//!
//! Phase 1 — bootstrap:
//! ```text
//! loop {
//!   batch = log.read_from(domain, after=last_acked, limit=batch_size)
//!   if batch is empty { break; }  // caught up
//!   yield each event
//! }
//! ```
//!
//! Phase 2 — live:
//! ```text
//! rx = dispatcher.topic::<T>().subscribe()
//! while let Ok(event) = rx.recv().await {
//!   if event.seq <= last_acked { continue; }  // overlap de-dup
//!   yield event
//! }
//! ```
//!
//! On `broadcast::error::Lagged` we flip back to phase-1 and replay the
//! missed seq range from the log before resuming live. The pluggable live
//! transport lives in [`super::live_source`] so lifecycle orchestration
//! stays independent of the dispatcher — unit tests can feed events from
//! an mpsc adapter, production uses the broadcast adapter.

use std::sync::Arc;

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;
use super::super::log::reader::LoggedEvent;
use super::super::log::{LogReadable, Seq};
use super::live_source::LiveSource;
use super::options::BatchSize;

/// Lifecycle phase for a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Pull from the log to catch up from `last_acked` to head.
    Bootstrap,
    /// Attached to the dispatcher's broadcast channel; events arrive live.
    Live,
    /// Terminal state — `next()` returns `None` forever. Reached via Halt
    /// or an explicit stop.
    Terminated,
}

/// Errors returned from a single lifecycle fetch.
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("log read: {0}")]
    LogRead(String),

    #[error("deserialize seq {seq:?} payload into {type_name}: {err}")]
    DeserializePayload {
        seq: Seq,
        type_name: &'static str,
        err: String,
    },

    /// The live receiver noticed `Lagged` — the lifecycle swallows this and
    /// transitions back to bootstrap; exposed for diagnostics.
    #[error("broadcast lagged by {skipped} events at last_acked {cursor:?}")]
    LiveLagged { skipped: u64, cursor: Seq },
}

/// Resolved event handed back to the subscription handler.
pub struct Fetched<T>
where
    T: DomainEvent,
{
    pub seq: Seq,
    pub event: Arc<T>,
}

/// State held by the lifecycle driver.
///
/// The lifecycle tracks two distinct notions of cursor:
///
///   * `watermark` — the highest seq the lifecycle has already handed out
///     via `next_from_*`. The next log pull uses `seq > watermark` so we
///     don't re-deliver the same batch. This advances on every delivery
///     regardless of ack.
///   * `ack_cursor` — the highest seq the consumer has ack'd. This lives
///     in the shared `SubscriptionState`; the lifecycle just uses it for
///     the live-overlap check.
///
/// Separating the two is essential — otherwise the bootstrap buffer would
/// never drain past the first batch because the unacked watermark would
/// stay at 0.
pub struct Lifecycle<T>
where
    T: DomainEvent,
{
    pub phase: Phase,
    /// Highest seq already delivered. Next bootstrap pull asks for
    /// `seq > watermark`.
    pub watermark: Seq,
    /// Pending events from the current bootstrap batch (seq-ordered).
    pub bootstrap_buffer: Vec<Fetched<T>>,
    /// Per-subscription batch size for bootstrap pulls.
    pub batch_size: BatchSize,
    /// Subscription-facing domain — needed to call `log.read_from` correctly.
    pub domain: Domain,
    /// Consumer-visible ack cursor — only used for the live-overlap check.
    pub ack_cursor: Seq,
}

impl<T> Lifecycle<T>
where
    T: DomainEvent,
{
    pub fn new(cursor: Seq, batch_size: BatchSize, domain: Domain) -> Self {
        Self {
            phase: Phase::Bootstrap,
            watermark: cursor,
            bootstrap_buffer: Vec::new(),
            batch_size,
            domain,
            ack_cursor: cursor,
        }
    }

    /// Drain the in-memory bootstrap buffer if any, otherwise pull another
    /// batch from the log. Returns `None` when the log says "you're caught
    /// up" — caller flips to live.
    pub async fn next_from_bootstrap(
        &mut self,
        log: &dyn LogReadable,
    ) -> Result<Option<Fetched<T>>, LifecycleError> {
        if let Some(fetched) = self.bootstrap_buffer.pop() {
            if fetched.seq > self.watermark {
                self.watermark = fetched.seq;
            }
            return Ok(Some(fetched));
        }

        let batch = log
            .read_from(self.domain, self.watermark, self.batch_size.get())
            .await
            .map_err(|e| LifecycleError::LogRead(e.to_string()))?;

        if batch.is_empty() {
            return Ok(None);
        }

        // Decode each row and stash them (reverse-ordered so Vec::pop gives
        // us the lowest-seq entry next).
        let mut decoded = Vec::with_capacity(batch.len());
        for row in batch {
            let LoggedEvent { seq, payload, .. } = row;
            if seq <= self.watermark {
                // Safety net: the log should never return seq <= watermark.
                continue;
            }
            let event: T = serde_json::from_value(payload).map_err(|e| {
                LifecycleError::DeserializePayload {
                    seq,
                    type_name: std::any::type_name::<T>(),
                    err: e.to_string(),
                }
            })?;
            decoded.push(Fetched {
                seq,
                event: Arc::new(event),
            });
        }

        // Reverse so the Vec::pop() in the next call returns the lowest seq
        // first.
        decoded.reverse();
        self.bootstrap_buffer = decoded;
        if let Some(fetched) = self.bootstrap_buffer.pop() {
            if fetched.seq > self.watermark {
                self.watermark = fetched.seq;
            }
            Ok(Some(fetched))
        } else {
            Ok(None)
        }
    }

    /// Pull one live event, filtering out overlap with bootstrap.
    ///
    /// On `LiveLagged` this returns `Err(...)`; the caller flips `phase` to
    /// `Bootstrap` and replays.
    pub async fn next_from_live(
        &mut self,
        live: &mut dyn LiveSource<T>,
        extract_seq: impl Fn(&T) -> Option<Seq>,
    ) -> Result<Option<Fetched<T>>, LifecycleError> {
        loop {
            match live.recv().await {
                Ok(Some(ev)) => {
                    let seq = extract_seq(&ev).unwrap_or(Seq(0));
                    if seq.0 != 0 && seq <= self.ack_cursor {
                        // Bootstrap/live overlap — skip.
                        continue;
                    }
                    if seq > self.watermark {
                        self.watermark = seq;
                    }
                    return Ok(Some(Fetched { seq, event: ev }));
                }
                Ok(None) => return Ok(None),
                Err(LifecycleError::LiveLagged { skipped, .. }) => {
                    return Err(LifecycleError::LiveLagged {
                        skipped,
                        cursor: self.ack_cursor,
                    });
                }
                Err(other) => return Err(other),
            }
        }
    }

    /// Advance the ack cursor. Idempotent: accepts `seq <= cursor` without
    /// change so a consumer that re-acks an earlier event doesn't rewind.
    pub fn record_ack(&mut self, seq: Seq) {
        if seq > self.ack_cursor {
            self.ack_cursor = seq;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::BoardEvent;
    use crate::event::log::reader::LoggedEvent;
    use crate::event::log::Log;
    use crate::event::log::LogError;
    use crate::event::subscription::live_source::MpscLiveSource;
    use crate::event::{AppendAck, AppendError, AppendOpts};
    use async_trait::async_trait;
    use tokio::sync::Mutex;

    fn board(i: usize) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: format!("t-{i}"),
            title: "t".into(),
            category: "c".into(),
        }
    }

    fn logged(seq: i64, ev: &BoardEvent) -> LoggedEvent {
        LoggedEvent {
            seq: Seq(seq),
            domain: Domain::Board,
            kind: ev.kind().into(),
            payload: serde_json::to_value(ev).unwrap(),
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

    struct CannedLog {
        events: Vec<LoggedEvent>,
        head: Mutex<Seq>,
    }

    impl CannedLog {
        fn new(events: Vec<LoggedEvent>) -> Self {
            let head = events.iter().map(|e| e.seq).max().unwrap_or(Seq(0));
            Self {
                events,
                head: Mutex::new(head),
            }
        }
    }

    #[async_trait]
    impl Log for CannedLog {
        async fn append<E>(&self, _: E, _: AppendOpts) -> Result<AppendAck, AppendError>
        where
            E: DomainEvent,
        {
            unimplemented!("tests don't append")
        }
        async fn read_from(
            &self,
            domain: Domain,
            after: Seq,
            limit: usize,
        ) -> Result<Vec<LoggedEvent>, LogError> {
            Ok(self
                .events
                .iter()
                .filter(|e| e.domain == domain && e.seq > after)
                .take(limit)
                .cloned()
                .collect())
        }
        async fn head_seq(&self) -> Result<Seq, LogError> {
            Ok(*self.head.lock().await)
        }
    }

    #[tokio::test]
    async fn bootstrap_drains_in_seq_order() {
        let events: Vec<_> = (1..=5).map(|i| logged(i, &board(i as usize))).collect();
        let log = CannedLog::new(events);
        let mut lc = Lifecycle::<BoardEvent>::new(Seq(0), BatchSize(100), Domain::Board);

        let mut seqs = Vec::new();
        while let Some(f) = lc.next_from_bootstrap(&log).await.unwrap() {
            seqs.push(f.seq.0);
            lc.record_ack(f.seq);
        }
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn bootstrap_returns_none_when_caught_up() {
        let log = CannedLog::new(vec![]);
        let mut lc = Lifecycle::<BoardEvent>::new(Seq(10), BatchSize(100), Domain::Board);

        let out = lc.next_from_bootstrap(&log).await.unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn bootstrap_respects_batch_size() {
        let events: Vec<_> = (1..=10).map(|i| logged(i, &board(i as usize))).collect();
        let log = CannedLog::new(events);
        let mut lc = Lifecycle::<BoardEvent>::new(Seq(0), BatchSize(3), Domain::Board);

        let mut seqs = Vec::new();
        loop {
            match lc.next_from_bootstrap(&log).await.unwrap() {
                Some(f) => {
                    seqs.push(f.seq.0);
                    lc.record_ack(f.seq);
                }
                None => break,
            }
        }
        assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[tokio::test]
    async fn bootstrap_skips_seq_at_or_below_cursor() {
        // log has events 5..=7 but cursor already at 6.
        let events = vec![
            logged(5, &board(5)),
            logged(6, &board(6)),
            logged(7, &board(7)),
        ];
        // Use a Log that returns everything > 0 — we want to see lifecycle
        // guard against dupes itself.
        struct NaiveLog(Vec<LoggedEvent>);
        #[async_trait]
        impl Log for NaiveLog {
            async fn append<E>(&self, _: E, _: AppendOpts) -> Result<AppendAck, AppendError>
            where
                E: DomainEvent,
            {
                unimplemented!()
            }
            async fn read_from(
                &self,
                domain: Domain,
                _after: Seq,
                _limit: usize,
            ) -> Result<Vec<LoggedEvent>, LogError> {
                Ok(self
                    .0
                    .iter()
                    .filter(|e| e.domain == domain)
                    .cloned()
                    .collect())
            }
            async fn head_seq(&self) -> Result<Seq, LogError> {
                Ok(Seq(7))
            }
        }

        let log = NaiveLog(events);
        let mut lc = Lifecycle::<BoardEvent>::new(Seq(6), BatchSize(10), Domain::Board);

        let mut seqs = Vec::new();
        while let Some(f) = lc.next_from_bootstrap(&log).await.unwrap() {
            seqs.push(f.seq.0);
            lc.record_ack(f.seq);
        }
        // Only seq 7 should be surfaced — 5 and 6 are at/below cursor.
        assert_eq!(seqs, vec![7]);
    }

    #[tokio::test]
    async fn live_overlap_is_deduped() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Arc<BoardEvent>>(8);
        let mut live = MpscLiveSource::new(rx);
        let mut lc = Lifecycle::<BoardEvent>::new(Seq(10), BatchSize(100), Domain::Board);

        // Send a seq-5 event (<= cursor) followed by seq-11 (> cursor).
        tx.send(Arc::new(board(5))).await.unwrap();
        tx.send(Arc::new(board(11))).await.unwrap();

        // seq-5 arrives but extract_seq says 5; cursor is 10; skip.
        let extract = |ev: &BoardEvent| -> Option<Seq> {
            match ev {
                BoardEvent::TaskCreated { task_id, .. } => {
                    let n: i64 = task_id.trim_start_matches("t-").parse().ok()?;
                    Some(Seq(n))
                }
                _ => None,
            }
        };

        let got = lc.next_from_live(&mut live, extract).await.unwrap().unwrap();
        assert_eq!(got.seq, Seq(11));
    }

    #[tokio::test]
    async fn record_ack_monotonic() {
        let mut lc = Lifecycle::<BoardEvent>::new(Seq(5), BatchSize(100), Domain::Board);
        lc.record_ack(Seq(8));
        assert_eq!(lc.ack_cursor, Seq(8));
        // Earlier ack is ignored (idempotent).
        lc.record_ack(Seq(3));
        assert_eq!(lc.ack_cursor, Seq(8));
    }
}
