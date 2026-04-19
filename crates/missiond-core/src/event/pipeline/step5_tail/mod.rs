//! Tail loop — long-poll the event log and fan out each row to its topic.
//!
//! Frozen lisp §4.2.c `tail-mechanism`:
//!
//! > `SELECT WHERE seq > last_dispatched ORDER BY seq LIMIT 256` every 100ms
//!
//! Contract (see the module-level doc for the full list):
//!   1. Read up to [`TAIL_BATCH_LIMIT`] events with `seq >
//!      last_dispatched_seq` across **all** domains.
//!   2. For each event, in seq order:
//!      - Look up the `ControlGate` → skip (drop) if the domain is paused.
//!        The seq cursor still advances so paused events are not retried.
//!      - Deserialize via the registered `AnyTopic` and fan out.
//!   3. Wait [`TAIL_POLL_INTERVAL`] before the next read. Exit early on
//!      `shutdown`.
//!
//! Failure modes:
//!   * Unknown domain label (corrupt row, schema mismatch) → log + drop
//!     the row + advance cursor. We do NOT halt the dispatcher — a single
//!     bad row must not stall the bus.
//!   * Payload deserialize failure → same: log + drop + advance. The
//!     subscriber path (Phase 4) has its own retries; the dispatcher is
//!     at-most-once for live fan-out.
//!   * Tail source error → return [`DispatchError::Tail`] up to the
//!     supervisor. Restart strategy is the daemon's job.
//!
//! I003 note (Phase 3 resolution):
//!
//! Frozen lisp §4.2.c mandates "paused=true 时跳过该 domain 的所有投递". The
//! implementation below honors that contract by default — the skip is the
//! entire action (event stays persisted, subscriber cursor jumps forward
//! on resume). `Observability` and `Incident` always bypass this gate via
//! `control_gate::domain_to_ctl_domain` → `None`, which is exactly what we
//! need for bus self-telemetry.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::watch;
use tokio::time::timeout;

use crate::event::blob_store::{BlobStore, PayloadRef};
use crate::event::domain::Domain;
use crate::event::log::reader::LoggedEvent;
use crate::event::log::{LogError, Seq};
use crate::event::pipeline::step6_gate::ControlGate;
use crate::event::pipeline::step7_fanout::registry::TopicRegistry;

/// Per-poll row cap. Frozen lisp §4.2.c: `LIMIT 256`.
pub const TAIL_BATCH_LIMIT: usize = 256;

/// Sleep between polls when the previous batch was empty. Frozen lisp
/// §4.2.c: long-polling interval ≈ 100ms. When the previous batch was
/// full, we skip the sleep and loop again to drain quickly.
pub const TAIL_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Structured errors returned by [`run_tail`].
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("tail source error: {0}")]
    Tail(#[from] TailError),

    #[error(
        "unknown domain {domain:?} encountered in event_log tail — registry did not include it"
    )]
    UnknownDomain { domain: Domain },

    #[error("payload deserialize failed for domain {domain:?}: {err}")]
    PayloadDeserialize { domain: Domain, err: String },

    #[error("shutdown signal received")]
    Shutdown,
}

#[derive(Debug, thiserror::Error)]
pub enum TailError {
    #[error("log read error: {0}")]
    LogRead(String),
}

impl From<LogError> for TailError {
    fn from(e: LogError) -> Self {
        TailError::LogRead(e.to_string())
    }
}

/// Metrics summary returned from a terminated tail loop. Lightweight, used
/// by tests + shutdown diagnostics.
#[derive(Debug, Default, Clone)]
pub struct DispatchMetrics {
    pub rows_read: u64,
    pub rows_fanned_out: u64,
    pub rows_dropped_paused: u64,
    pub rows_dropped_unknown_domain: u64,
    pub rows_dropped_deserialize: u64,
    pub last_seq: i64,
}

/// Abstraction over the log tail source. Production wires this to the
/// Phase 2 `LogReader`; unit tests plug a `Vec<LoggedEvent>` mock.
///
/// The dispatcher reads **all domains** at once — it scans the whole log
/// forward. Per-domain filtering happens inside the consumer (Phase 4
/// subscription). This keeps the tail cursor O(1).
#[async_trait]
pub trait TailSource: Send + Sync {
    /// Return up to `limit` events with `seq > after_seq`, ordered by seq
    /// ascending. An empty Vec is normal (no new rows).
    async fn read_all_from(
        &self,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, TailError>;
}

/// Adapter: a PG-backed tail source reusing the Phase 2 [`LogReader`]. The
/// reader already knows how to resolve `payload_ref` via the blob store,
/// but its native API is per-domain. Here we loop over all domains in one
/// batch — simple and fast for the 12-domain cardinality.
#[cfg(feature = "postgres")]
pub struct PgTailSource {
    pool: sqlx::PgPool,
    blob_store: Arc<dyn BlobStore>,
}

#[cfg(feature = "postgres")]
impl PgTailSource {
    pub fn new(pool: sqlx::PgPool, blob_store: Arc<dyn BlobStore>) -> Self {
        Self { pool, blob_store }
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl TailSource for PgTailSource {
    async fn read_all_from(
        &self,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, TailError> {
        use super::super::log::reader::domain_from_str;

        #[derive(sqlx::FromRow)]
        struct Row {
            seq: i64,
            domain: String,
            kind: String,
            payload_inline: Option<serde_json::Value>,
            payload_ref: Option<String>,
            producer_id: String,
            dedupe_key: Option<uuid::Uuid>,
            causation_depth: i16,
            trace_id: Option<uuid::Uuid>,
            span_id: Option<uuid::Uuid>,
            parent_span_id: Option<uuid::Uuid>,
            ts: chrono::DateTime<chrono::Utc>,
            ephemeral: bool,
        }

        let rows: Vec<Row> = sqlx::query_as::<_, Row>(
            r#"
            SELECT seq, domain, kind, payload_inline, payload_ref, producer_id,
                   dedupe_key, causation_depth, trace_id, span_id, parent_span_id,
                   ts, ephemeral
            FROM event_log
            WHERE seq > $1
            ORDER BY seq ASC
            LIMIT $2
            "#,
        )
        .bind(after_seq.0)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TailError::LogRead(e.to_string()))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(domain) = domain_from_str(&row.domain) else {
                // Log + skip this row; the tail loop decides what to do
                // with an unresolved domain (typically: drop).
                tracing::warn!(
                    seq = row.seq,
                    domain = %row.domain,
                    "tail: unknown domain label; skipping row"
                );
                continue;
            };
            let payload = match (row.payload_inline, row.payload_ref) {
                (Some(inline), _) => inline,
                (None, Some(ref_json)) => {
                    let payload_ref = PayloadRef::from_json_str(&ref_json)
                        .map_err(|e| TailError::LogRead(format!("decode payload_ref: {e}")))?;
                    let bytes = self
                        .blob_store
                        .get(&payload_ref)
                        .await
                        .map_err(|e| TailError::LogRead(format!("fetch blob: {e}")))?;
                    serde_json::from_slice(&bytes)
                        .map_err(|e| TailError::LogRead(format!("decode blob payload: {e}")))?
                }
                (None, None) => serde_json::Value::Null,
            };

            out.push(LoggedEvent {
                seq: Seq(row.seq),
                domain,
                kind: row.kind,
                payload,
                producer_id: row.producer_id,
                dedupe_key: row.dedupe_key,
                causation_depth: row.causation_depth,
                trace_id: row.trace_id,
                span_id: row.span_id,
                parent_span_id: row.parent_span_id,
                ts: row.ts,
                ephemeral: row.ephemeral,
            });
        }
        Ok(out)
    }
}

/// Main dispatcher loop. Invoked from [`super::Dispatcher::run`].
///
/// The loop is bounded by `shutdown`: when `*shutdown.borrow()` becomes
/// `true` the loop returns `Err(DispatchError::Shutdown)` with current
/// metrics preserved via the last_dispatched_seq counter.
pub async fn run_tail<G>(
    tail_source: Arc<dyn TailSource>,
    _blob_store: Arc<dyn BlobStore>,
    registry: Arc<TopicRegistry>,
    last_dispatched_seq: Arc<AtomicI64>,
    control: G,
    mut shutdown: watch::Receiver<bool>,
) -> Result<DispatchMetrics, DispatchError>
where
    G: ControlGate + Clone + 'static,
{
    let mut metrics = DispatchMetrics::default();

    loop {
        // Fast shutdown check before each poll.
        if *shutdown.borrow() {
            return Err(DispatchError::Shutdown);
        }

        let cursor = Seq(last_dispatched_seq.load(Ordering::Acquire));
        let batch = tail_source
            .read_all_from(cursor, TAIL_BATCH_LIMIT)
            .await
            .map_err(DispatchError::Tail)?;

        metrics.rows_read += batch.len() as u64;
        let was_full = batch.len() == TAIL_BATCH_LIMIT;

        for logged in batch {
            dispatch_one(&registry, &control, &logged, &mut metrics);
            last_dispatched_seq.store(logged.seq.0, Ordering::Release);
            metrics.last_seq = logged.seq.0;
        }

        // If we emptied the batch (or none was available), wait the
        // poll interval; `timeout` on the shutdown receiver lets us wake
        // promptly on shutdown.
        if !was_full {
            match timeout(TAIL_POLL_INTERVAL, shutdown.changed()).await {
                Ok(Ok(())) => {
                    // shutdown flipped
                    if *shutdown.borrow() {
                        return Err(DispatchError::Shutdown);
                    }
                }
                Ok(Err(_)) => {
                    // shutdown tx dropped — treat as shutdown.
                    return Err(DispatchError::Shutdown);
                }
                Err(_) => {
                    // poll interval elapsed; loop again.
                }
            }
        }
    }
}

/// Route a single LoggedEvent to its topic, respecting the control gate.
///
/// Metrics are updated in place. Errors are swallowed (logged at WARN) on
/// purpose — see module docs. This keeps one bad row from stalling the
/// whole tail loop.
fn dispatch_one(
    registry: &TopicRegistry,
    control: &impl ControlGate,
    logged: &LoggedEvent,
    metrics: &mut DispatchMetrics,
) {
    // Control-gate check first. Paused ⇒ drop + advance cursor.
    if control.is_domain_paused(logged.domain) {
        metrics.rows_dropped_paused += 1;
        tracing::debug!(
            seq = logged.seq.0,
            domain = logged.domain.as_str(),
            "tail: domain paused; dropping"
        );
        return;
    }

    let Some(topic) = registry.any_topic(logged.domain) else {
        metrics.rows_dropped_unknown_domain += 1;
        tracing::warn!(
            seq = logged.seq.0,
            domain = logged.domain.as_str(),
            "tail: unregistered domain topic; dropping"
        );
        return;
    };

    match topic.fanout_json(&logged.payload) {
        Ok(()) => {
            metrics.rows_fanned_out += 1;
        }
        Err(e) => {
            metrics.rows_dropped_deserialize += 1;
            tracing::warn!(
                seq = logged.seq.0,
                domain = logged.domain.as_str(),
                error = %e,
                "tail: payload deserialize failed; dropping"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Tests — mock TailSource exercises the dispatch loop without a DB
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::blob_store::{BlobBackend, BlobResult, BlobStoreError, LocalFileBlobStore};
    use crate::event::dispatcher::control_gate::CtlDomain;
    use crate::event::dispatcher::registry::TopicRegistryBuilder;
    use crate::event::dispatcher::{DispatcherBuilder, NeverPaused, register_all_domains};
    use crate::event::events::{BoardEvent, MemoryEvent, ObservabilityEvent, SlotEvent};
    use chrono::Utc;
    use std::sync::Mutex;
    use std::time::Instant;
    use tokio::time::Duration as TokioDuration;

    /// Record of what the mock tail source has served so tests can assert
    /// the cursor was advanced.
    #[derive(Default)]
    struct MockTailInner {
        /// Events to serve, keyed by seq. Sorted.
        events: Vec<LoggedEvent>,
        /// Record of (cursor, limit) requests the dispatcher made.
        requests: Vec<(i64, usize)>,
    }

    #[derive(Clone, Default)]
    struct MockTailSource {
        inner: Arc<Mutex<MockTailInner>>,
    }

    impl MockTailSource {
        fn push(&self, e: LoggedEvent) {
            let mut g = self.inner.lock().unwrap();
            g.events.push(e);
            g.events.sort_by_key(|e| e.seq.0);
        }

        fn requests(&self) -> Vec<(i64, usize)> {
            self.inner.lock().unwrap().requests.clone()
        }
    }

    #[async_trait]
    impl TailSource for MockTailSource {
        async fn read_all_from(
            &self,
            after_seq: Seq,
            limit: usize,
        ) -> Result<Vec<LoggedEvent>, TailError> {
            let mut g = self.inner.lock().unwrap();
            g.requests.push((after_seq.0, limit));
            Ok(g.events
                .iter()
                .filter(|e| e.seq.0 > after_seq.0)
                .take(limit)
                .cloned()
                .collect())
        }
    }

    fn make_logged<T: crate::event::DomainEvent + serde::Serialize>(
        seq: i64,
        event: &T,
    ) -> LoggedEvent {
        LoggedEvent {
            seq: Seq(seq),
            domain: T::domain(),
            kind: event.kind().to_string(),
            payload: serde_json::to_value(event).unwrap(),
            producer_id: "test".into(),
            dedupe_key: None,
            causation_depth: 0,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            ts: Utc::now(),
            ephemeral: false,
        }
    }

    fn make_board_event(idx: usize) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: format!("t-{idx}"),
            title: "title".into(),
            category: "cat".into(),
        }
    }

    /// Lightweight blob store — the mock tail source doesn't call into
    /// it, but the run_tail signature demands one.
    fn dummy_blob_store() -> Arc<dyn BlobStore> {
        let d = tempfile::tempdir().unwrap();
        let p = d.keep();
        Arc::new(LocalFileBlobStore::new(p))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_fanout_delivers_events_in_seq_order() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let board = dispatcher.topic::<BoardEvent>();
        let mut rx = board.subscribe();

        let mock = MockTailSource::default();
        // 100 events, seq 1..=100
        for i in 1..=100i64 {
            mock.push(make_logged(i, &make_board_event(i as usize)));
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock.clone());
        let blob = dummy_blob_store();

        let dispatcher_handle = dispatcher.clone();
        let task = tokio::spawn(async move {
            dispatcher_handle
                .run(source, blob, NeverPaused, shutdown_rx)
                .await
        });

        // Receive all 100 events; verify seq-equivalent order by task_id.
        let mut received = Vec::new();
        for _ in 0..100 {
            let ev = tokio::time::timeout(TokioDuration::from_secs(2), rx.recv())
                .await
                .expect("recv timed out")
                .expect("recv ok");
            match &*ev {
                BoardEvent::TaskCreated { task_id, .. } => received.push(task_id.clone()),
                _ => unreachable!(),
            }
        }
        assert_eq!(received.len(), 100);
        for (i, tid) in received.iter().enumerate() {
            assert_eq!(tid, &format!("t-{}", i + 1));
        }

        // Verify cursor advanced monotonically to 100.
        assert_eq!(dispatcher.last_dispatched_seq(), Seq(100));

        // Shutdown the loop.
        shutdown_tx.send(true).unwrap();
        let result = task.await.unwrap();
        let metrics = match result {
            Err(DispatchError::Shutdown) => {
                // expected — get metrics via cursor (DispatchMetrics not
                // returned via this path, so assert via last_seq on
                // cursor instead).
                Default::default()
            }
            Ok(m) => m,
            other => panic!("unexpected result {other:?}"),
        };
        assert!(metrics.last_seq <= 100);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn last_dispatched_seq_is_monotonic() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let _rx = dispatcher.topic::<BoardEvent>().subscribe();

        let mock = MockTailSource::default();
        for i in 1..=50i64 {
            mock.push(make_logged(i, &make_board_event(i as usize)));
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock.clone());
        let blob = dummy_blob_store();

        let dispatcher_handle = dispatcher.clone();
        let task = tokio::spawn(async move {
            dispatcher_handle
                .run(source, blob, NeverPaused, shutdown_rx)
                .await
        });

        // Wait for cursor to reach 50.
        for _ in 0..50 {
            if dispatcher.last_dispatched_seq() == Seq(50) {
                break;
            }
            tokio::time::sleep(TokioDuration::from_millis(20)).await;
        }
        assert_eq!(dispatcher.last_dispatched_seq(), Seq(50));

        shutdown_tx.send(true).unwrap();
        let _ = task.await.unwrap();

        // All requests' after_seq must be monotonically nondecreasing.
        let reqs = mock.requests();
        let mut last = -1i64;
        for (cursor, _) in reqs {
            assert!(
                cursor >= last,
                "cursor regressed: last={last} now={cursor}"
            );
            last = cursor;
        }
    }

    /// Paused Memory domain ⇒ drop, but other domains still flow.
    #[tokio::test(flavor = "multi_thread")]
    async fn paused_memory_drops_memory_events_but_passes_others() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let mut memory_rx = dispatcher.topic::<MemoryEvent>().subscribe();
        let mut board_rx = dispatcher.topic::<BoardEvent>().subscribe();

        let mock = MockTailSource::default();
        mock.push(make_logged(
            1,
            &MemoryEvent::PhaseChanged {
                slot_id: "s".into(),
                phase: "Idle".into(),
                active_type: None,
            },
        ));
        mock.push(make_logged(2, &make_board_event(2)));
        mock.push(make_logged(
            3,
            &MemoryEvent::PhaseChanged {
                slot_id: "s".into(),
                phase: "Busy".into(),
                active_type: None,
            },
        ));

        #[derive(Clone)]
        struct MemoryPaused;
        impl ControlGate for MemoryPaused {
            fn is_ctl_domain_paused(&self, d: CtlDomain) -> bool {
                matches!(d, CtlDomain::Memory)
            }
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock);
        let blob = dummy_blob_store();

        let dispatcher_handle = dispatcher.clone();
        let task = tokio::spawn(async move {
            dispatcher_handle
                .run(source, blob, MemoryPaused, shutdown_rx)
                .await
        });

        // Board event should arrive.
        let board_ev = tokio::time::timeout(TokioDuration::from_secs(1), board_rx.recv())
            .await
            .expect("board recv timeout")
            .expect("board recv");
        assert!(matches!(&*board_ev, BoardEvent::TaskCreated { .. }));

        // Memory receiver must not get anything within the poll window.
        let got = tokio::time::timeout(TokioDuration::from_millis(300), memory_rx.recv()).await;
        assert!(
            got.is_err(),
            "paused memory events must not fan out, got {:?}",
            got
        );

        // Cursor still advanced past all 3 events.
        for _ in 0..50 {
            if dispatcher.last_dispatched_seq() == Seq(3) {
                break;
            }
            tokio::time::sleep(TokioDuration::from_millis(20)).await;
        }
        assert_eq!(dispatcher.last_dispatched_seq(), Seq(3));

        shutdown_tx.send(true).unwrap();
        let _ = task.await.unwrap();
    }

    /// Observability is never subject to the control gate — pausing any
    /// CtlDomain must not silence it.
    #[tokio::test(flavor = "multi_thread")]
    async fn observability_ignores_control_gate() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let mut obs_rx = dispatcher.topic::<ObservabilityEvent>().subscribe();

        let mock = MockTailSource::default();
        mock.push(make_logged(
            1,
            &ObservabilityEvent::BusMetric {
                name: "append_rate".into(),
                value: 1.23,
                labels: serde_json::Value::Null,
            },
        ));

        #[derive(Clone)]
        struct AllPaused;
        impl ControlGate for AllPaused {
            fn is_ctl_domain_paused(&self, _d: CtlDomain) -> bool {
                true
            }
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock);
        let blob = dummy_blob_store();

        let dispatcher_handle = dispatcher.clone();
        let task = tokio::spawn(async move {
            dispatcher_handle
                .run(source, blob, AllPaused, shutdown_rx)
                .await
        });

        let ev = tokio::time::timeout(TokioDuration::from_secs(1), obs_rx.recv())
            .await
            .expect("obs recv timeout")
            .expect("obs recv");
        assert!(matches!(&*ev, ObservabilityEvent::BusMetric { .. }));

        shutdown_tx.send(true).unwrap();
        let _ = task.await.unwrap();
    }

    /// A subscriber that never drains (simulated slow consumer) must NOT
    /// stall the tail loop. Other subscribers still keep receiving.
    #[tokio::test(flavor = "multi_thread")]
    async fn slow_subscriber_does_not_block_tail_loop() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        // Slow subscriber — never drains, sits until broadcast overflow.
        let _slow_rx = dispatcher.topic::<BoardEvent>().subscribe();
        // Fast subscriber — we read from it.
        let mut fast_rx = dispatcher.topic::<BoardEvent>().subscribe();

        let mock = MockTailSource::default();
        // Push more than TOPIC_BUFFER_SIZE events to force Lagged on slow
        // subscriber but not stall the sender.
        for i in 1..=2000i64 {
            mock.push(make_logged(i, &make_board_event(i as usize)));
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock);
        let blob = dummy_blob_store();

        let dispatcher_handle = dispatcher.clone();
        let task = tokio::spawn(async move {
            dispatcher_handle
                .run(source, blob, NeverPaused, shutdown_rx)
                .await
        });

        // Fast consumer eventually sees events. It may see Lagged too as
        // we don't drain in real time, but should get *some* before shutdown.
        let mut ok = 0usize;
        let mut lagged = 0usize;
        let start = Instant::now();
        while ok + lagged < 100 && start.elapsed() < TokioDuration::from_secs(3) {
            match fast_rx.try_recv() {
                Ok(_) => ok += 1,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => lagged += 1,
                Err(_) => tokio::time::sleep(TokioDuration::from_millis(10)).await,
            }
        }
        assert!(
            ok > 0 || lagged > 0,
            "expected some fast-side activity: ok={ok} lagged={lagged}"
        );

        // Cursor eventually reaches 2000.
        let deadline = Instant::now() + TokioDuration::from_secs(3);
        while dispatcher.last_dispatched_seq() != Seq(2000) && Instant::now() < deadline {
            tokio::time::sleep(TokioDuration::from_millis(20)).await;
        }
        assert_eq!(
            dispatcher.last_dispatched_seq(),
            Seq(2000),
            "tail loop must process all events despite slow subscriber"
        );

        shutdown_tx.send(true).unwrap();
        let _ = task.await.unwrap();
    }

    /// A LoggedEvent whose domain is not in the registry is dropped (not
    /// panicked). This can only happen if the daemon forgot to register a
    /// domain — we log a warning but keep the bus running.
    #[tokio::test(flavor = "multi_thread")]
    async fn missing_registry_entry_is_dropped_not_crashed() {
        // Build a partial registry missing BoardEvent on purpose.
        let partial = TopicRegistryBuilder::new()
            .register::<SlotEvent>()
            .build();
        let registry = Arc::new(partial);
        let cursor = Arc::new(AtomicI64::new(0));

        let mock = MockTailSource::default();
        mock.push(make_logged(1, &make_board_event(1)));
        mock.push(make_logged(
            2,
            &SlotEvent::BecameIdle {
                slot_id: "s-1".into(),
            },
        ));

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock);
        let blob = dummy_blob_store();

        let reg2 = registry.clone();
        let cur2 = cursor.clone();
        let task = tokio::spawn(async move {
            run_tail(source, blob, reg2, cur2, NeverPaused, shutdown_rx).await
        });

        // Wait for cursor to reach 2.
        for _ in 0..100 {
            if cursor.load(Ordering::Acquire) == 2 {
                break;
            }
            tokio::time::sleep(TokioDuration::from_millis(20)).await;
        }
        assert_eq!(cursor.load(Ordering::Acquire), 2);

        shutdown_tx.send(true).unwrap();
        let _ = task.await.unwrap();
    }

    /// A LoggedEvent with a payload that doesn't parse into the registered
    /// type: drop + advance, don't stall the loop.
    #[tokio::test(flavor = "multi_thread")]
    async fn bad_payload_is_dropped_not_crashed() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let cursor_ref = dispatcher.clone();
        let _rx = dispatcher.topic::<BoardEvent>().subscribe();

        let mock = MockTailSource::default();
        // Intentionally bad payload shape — not a valid BoardEvent variant.
        let bad = LoggedEvent {
            seq: Seq(1),
            domain: Domain::Board,
            kind: "task_created".into(),
            payload: serde_json::json!({ "NotAVariant": {} }),
            producer_id: "test".into(),
            dedupe_key: None,
            causation_depth: 0,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            ts: Utc::now(),
            ephemeral: false,
        };
        mock.push(bad);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock);
        let blob = dummy_blob_store();
        let task = tokio::spawn(async move {
            dispatcher.run(source, blob, NeverPaused, shutdown_rx).await
        });

        for _ in 0..50 {
            if cursor_ref.last_dispatched_seq() == Seq(1) {
                break;
            }
            tokio::time::sleep(TokioDuration::from_millis(20)).await;
        }
        assert_eq!(cursor_ref.last_dispatched_seq(), Seq(1));

        shutdown_tx.send(true).unwrap();
        let _ = task.await.unwrap();
    }

    /// Ensure we don't panic on tail-source error during dispatch — the
    /// error is returned up to the supervisor.
    #[tokio::test(flavor = "multi_thread")]
    async fn tail_source_error_is_returned() {
        struct Broken;
        #[async_trait]
        impl TailSource for Broken {
            async fn read_all_from(
                &self,
                _after: Seq,
                _limit: usize,
            ) -> Result<Vec<LoggedEvent>, TailError> {
                Err(TailError::LogRead("db is on fire".into()))
            }
        }

        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let blob = dummy_blob_store();
        let source: Arc<dyn TailSource> = Arc::new(Broken);

        let result = dispatcher.run(source, blob, NeverPaused, shutdown_rx).await;
        match result {
            Err(DispatchError::Tail(_)) => {}
            other => panic!("expected Tail error, got {other:?}"),
        }
    }

    /// Demonstrate the dummy_blob_store doesn't get used when all payloads
    /// are inline (Phase 3 tests).
    #[tokio::test]
    async fn blob_store_not_required_when_all_payloads_inline() {
        struct RejectBlob;
        #[async_trait]
        impl BlobStore for RejectBlob {
            async fn put(&self, _bytes: &[u8]) -> BlobResult<PayloadRef> {
                Err(BlobStoreError::Other("should not be called".into()))
            }
            async fn get(&self, _r: &PayloadRef) -> BlobResult<Vec<u8>> {
                Err(BlobStoreError::Other("should not be called".into()))
            }
            async fn delete(&self, _r: &PayloadRef) -> BlobResult<()> {
                Err(BlobStoreError::Other("should not be called".into()))
            }
            fn backend(&self) -> BlobBackend {
                BlobBackend::PgTable
            }
        }

        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let _rx = dispatcher.topic::<BoardEvent>().subscribe();

        let mock = MockTailSource::default();
        mock.push(make_logged(1, &make_board_event(1)));

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let source: Arc<dyn TailSource> = Arc::new(mock);
        let blob: Arc<dyn BlobStore> = Arc::new(RejectBlob);

        let dispatcher_handle = dispatcher.clone();
        let task = tokio::spawn(async move {
            dispatcher_handle
                .run(source, blob, NeverPaused, shutdown_rx)
                .await
        });

        for _ in 0..50 {
            if dispatcher.last_dispatched_seq() == Seq(1) {
                break;
            }
            tokio::time::sleep(TokioDuration::from_millis(20)).await;
        }
        assert_eq!(dispatcher.last_dispatched_seq(), Seq(1));

        shutdown_tx.send(true).unwrap();
        let _ = task.await.unwrap();
    }
}
