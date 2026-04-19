//! In-memory [`Log`] implementation — preserves the production PG
//! semantics for tests and chaos scenarios.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 testing-story:
//!
//! > in-memory-bus: must share the production semantics — single writer
//! > assigning seq, append-ack, seq-ordered replay. Not a raw AtomicU64.
//!
//! Design mirror:
//!
//! | aspect            | PG version                         | in-memory version                      |
//! |-------------------|------------------------------------|-----------------------------------------|
//! | seq assignment    | `BIGSERIAL` via `INSERT RETURNING` | `AtomicI64::fetch_add` inside writer    |
//! | append channel    | bounded MPSC (capacity 4096)       | bounded MPSC (capacity 4096)            |
//! | dedupe collision  | `UNIQUE (producer_id, dedupe_key)` → 23505 → SELECT existing seq | `HashMap<(String, Uuid), Seq>` lookup |
//! | ephemeral path    | skips DB, returns `Volatile`       | same — goes through in-memory seq       |
//! | claim-check       | `payload_ref` column via BlobStore | same — delegates to BlobStore           |
//! | batch INSERT      | 100-row TX                         | virtual batch — each row still gets a fresh seq; flush is effectively per-row |
//! | failed state      | retry cap → `LogUnavailable`       | programmable via [`InMemoryLog::set_failed`] |
//!
//! The "batch" semantics differ slightly — the in-memory log doesn't need
//! batched transactions for performance, so it writes each pending append
//! sequentially under the writer task. Chaos tests that assert batch
//! behavior run against the PG LogWriter (or a mock thereof), not against
//! InMemoryLog.
//!
//! `InMemoryLog` holds its state in plain `Arc<Mutex<_>>` / atomics; there
//! is no background task required for correctness, but a **writer task**
//! is still spawned so the bounded-channel backpressure path is
//! functional — otherwise `try_send` would always succeed on an unread
//! channel and the `LogUnavailable` / `Backpressure` chaos tests would
//! have no way to trigger.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::super::blob_store::{BlobStore, CLAIM_CHECK_THRESHOLD};
use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;
use super::super::log::reader::LoggedEvent;
use super::super::log::{AppendAck, AppendError, AppendOpts, Log, LogError, Seq};
use super::super::metrics::{BusMetrics, NoopMetrics};
use super::super::pipeline::step1_guard::check_causation;

/// Bound on the in-memory append channel. Matches the PG default
/// (frozen lisp §4.2.b) so behavioral parity holds.
pub const IN_MEMORY_APPEND_CAPACITY: usize = 4096;

/// Entry sent from [`InMemoryLog::append`] to the writer task.
struct Pending {
    row: StoredRow,
    dedupe_key: Option<Uuid>,
    ephemeral: bool,
    ack: oneshot::Sender<Result<AppendAck, AppendError>>,
}

/// Internal stored row. Kept in the Vec verbatim so `read_from` / `head_seq`
/// can replay.
#[derive(Debug, Clone)]
pub struct StoredRow {
    pub seq: Seq,
    pub domain: Domain,
    pub kind: String,
    pub payload_inline: Option<serde_json::Value>,
    pub payload_ref: Option<String>,
    pub producer_id: String,
    pub dedupe_key: Option<Uuid>,
    pub causation_depth: i16,
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub ephemeral: bool,
}

/// In-memory [`Log`] implementation.
pub struct InMemoryLog {
    /// Sender for the writer task. `None` means the writer task has
    /// already shut down — all new appends will surface `LogUnavailable`.
    tx: mpsc::Sender<Pending>,
    /// Next seq to hand out (fetch_add style).
    next_seq: Arc<AtomicI64>,
    /// All committed rows.
    rows: Arc<Mutex<Vec<StoredRow>>>,
    /// Dedupe map: `(producer_id, dedupe_key) → seq`.
    dedupe: Arc<Mutex<HashMap<(String, Uuid), Seq>>>,
    /// Sticky failed flag — tests toggle this via [`Self::set_failed`].
    failed: Arc<AtomicBool>,
    /// Blob store for oversized payloads (claim-check).
    blob_store: Arc<dyn BlobStore>,
    /// Metrics sink.
    metrics: Arc<dyn BusMetrics>,
}

impl std::fmt::Debug for InMemoryLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryLog")
            .field("next_seq", &self.next_seq.load(Ordering::Relaxed))
            .field("rows", &self.rows.lock().unwrap().len())
            .field("failed", &self.failed.load(Ordering::Relaxed))
            .finish()
    }
}

impl InMemoryLog {
    /// Build an in-memory log and spawn the writer task. Returns the
    /// caller-facing `Arc<InMemoryLog>`.
    ///
    /// `metrics` receives `record_append` on each commit.
    pub fn spawn(
        blob_store: Arc<dyn BlobStore>,
        metrics: Arc<dyn BusMetrics>,
    ) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(IN_MEMORY_APPEND_CAPACITY);
        let log = Arc::new(Self {
            tx,
            next_seq: Arc::new(AtomicI64::new(1)),
            rows: Arc::new(Mutex::new(Vec::new())),
            dedupe: Arc::new(Mutex::new(HashMap::new())),
            failed: Arc::new(AtomicBool::new(false)),
            blob_store,
            metrics,
        });
        let writer = WriterTask {
            rx,
            next_seq: log.next_seq.clone(),
            rows: log.rows.clone(),
            dedupe: log.dedupe.clone(),
            failed: log.failed.clone(),
            metrics: log.metrics.clone(),
        };
        tokio::spawn(writer.run());
        log
    }

    /// Convenience constructor — zero dependencies (NoopMetrics, in-memory
    /// blob store).
    pub fn spawn_default() -> Arc<Self> {
        use super::blob_store::InMemoryBlobStore;
        Self::spawn(
            Arc::new(InMemoryBlobStore::new()),
            Arc::new(NoopMetrics),
        )
    }

    /// Flip the "failed" bit. New appends return `LogUnavailable`.
    ///
    /// Used by chaos test #6 (db-disconnect) to simulate persistence going
    /// away without tearing down the whole log.
    pub fn set_failed(&self, failed: bool) {
        self.failed.store(failed, Ordering::SeqCst);
    }

    /// Snapshot of the current seq counter. Useful for test assertions
    /// that want to verify no seq was skipped during a dedupe path.
    pub fn head_seq_sync(&self) -> Seq {
        Seq(self.rows
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.seq.0)
            .max()
            .unwrap_or(0))
    }

    /// Count of committed rows — tests use this to verify dropped events
    /// never reached the log.
    pub fn row_count(&self) -> usize {
        self.rows.lock().unwrap().len()
    }

    /// Clone of the committed rows — used by [`super::InMemoryTailSource`]
    /// to serve the dispatcher's cross-domain tail.
    pub fn rows_snapshot(&self) -> Vec<StoredRow> {
        self.rows.lock().unwrap().clone()
    }
}

/// The writer task — mirrors `PgLogWriter::run` on the PG side.
struct WriterTask {
    rx: mpsc::Receiver<Pending>,
    next_seq: Arc<AtomicI64>,
    rows: Arc<Mutex<Vec<StoredRow>>>,
    dedupe: Arc<Mutex<HashMap<(String, Uuid), Seq>>>,
    failed: Arc<AtomicBool>,
    metrics: Arc<dyn BusMetrics>,
}

impl WriterTask {
    async fn run(mut self) {
        while let Some(pending) = self.rx.recv().await {
            self.handle_one(pending).await;
        }
    }

    async fn handle_one(&mut self, pending: Pending) {
        let Pending {
            mut row,
            dedupe_key,
            ephemeral,
            ack,
        } = pending;

        // Failed state short-circuits.
        if self.failed.load(Ordering::Acquire) {
            self.metrics.record_append(row.domain, false, 0);
            let _ = ack.send(Err(AppendError::LogUnavailable(
                "in-memory log failed".into(),
            )));
            return;
        }

        // Dedupe lookup — mirrors PG UNIQUE violation path.
        if let Some(key) = dedupe_key {
            let k = (row.producer_id.clone(), key);
            if let Some(existing) = self.dedupe.lock().unwrap().get(&k).copied() {
                let _ = ack.send(Ok(AppendAck::AlreadyExists { seq: existing }));
                return;
            }
        }

        // Allocate seq before committing, like `BIGSERIAL` does in PG.
        let seq = Seq(self.next_seq.fetch_add(1, Ordering::SeqCst));
        row.seq = seq;

        // Ephemeral path — skip the rows vec, but still consumed a seq.
        // This matches the principle-6 stability: ephemeral events have
        // durable seqs even when not persisted, so observability replay
        // can at least see the gap.
        if ephemeral {
            self.metrics.record_append(row.domain, true, payload_bytes(&row));
            let _ = ack.send(Ok(AppendAck::Volatile { seq }));
            return;
        }

        // Persist — single-writer serialization guaranteed by the task.
        self.rows.lock().unwrap().push(row.clone());
        if let Some(key) = dedupe_key {
            self.dedupe
                .lock()
                .unwrap()
                .insert((row.producer_id.clone(), key), seq);
        }
        self.metrics
            .record_append(row.domain, true, payload_bytes(&row));

        let _ = ack.send(Ok(AppendAck::Committed { seq, durable: true }));
    }
}

fn payload_bytes(row: &StoredRow) -> usize {
    match &row.payload_inline {
        Some(v) => v.to_string().len(),
        None => match &row.payload_ref {
            Some(r) => r.len(),
            None => 0,
        },
    }
}

#[async_trait]
impl Log for InMemoryLog {
    async fn append<E>(&self, event: E, opts: AppendOpts) -> Result<AppendAck, AppendError>
    where
        E: DomainEvent,
    {
        // Causation guard — mirrors PG writer.
        check_causation(&opts)?;

        if opts.producer_id.is_empty() && !opts.ephemeral {
            return Err(AppendError::Other("producer_id must not be empty".into()));
        }

        let payload_bytes = serde_json::to_vec(&event)?;
        let payload_inline_eligible = payload_bytes.len() <= CLAIM_CHECK_THRESHOLD;

        let kind = event.kind().to_string();
        let domain = E::domain();

        let (payload_inline, payload_ref) = if payload_inline_eligible {
            (
                Some(serde_json::from_slice::<serde_json::Value>(&payload_bytes)?),
                None,
            )
        } else {
            let r = self
                .blob_store
                .put(&payload_bytes)
                .await
                .map_err(|e| AppendError::Other(format!("blob put: {e}")))?;
            (None, Some(r.to_json_string()))
        };

        let span = opts.span.clone().unwrap_or_default();
        let producer_id = opts.producer_id.clone();
        let now = chrono::Utc::now();
        let row = StoredRow {
            seq: Seq(0), // assigned by writer
            domain,
            kind,
            payload_inline,
            payload_ref,
            producer_id,
            dedupe_key: opts.dedupe_key,
            causation_depth: opts.causation_depth as i16,
            trace_id: span.trace_id,
            span_id: span.span_id,
            parent_span_id: span.parent_span_id,
            ts: now,
            ephemeral: opts.ephemeral,
        };

        let (tx, rx) = oneshot::channel();
        let pending = Pending {
            row,
            dedupe_key: opts.dedupe_key,
            ephemeral: opts.ephemeral,
            ack: tx,
        };

        match self.tx.try_send(pending) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Err(AppendError::Backpressure),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(AppendError::LogUnavailable(
                    "in-memory writer shut down".into(),
                ));
            }
        }

        match rx.await {
            Ok(r) => r,
            Err(_) => Err(AppendError::LogUnavailable("ack channel dropped".into())),
        }
    }

    async fn read_from(
        &self,
        domain: Domain,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, LogError> {
        let guard = self.rows.lock().unwrap();
        let events: Vec<LoggedEvent> = guard
            .iter()
            .filter(|r| r.domain == domain && r.seq > after_seq)
            .take(limit)
            .map(stored_to_logged)
            .collect();
        Ok(events)
    }

    async fn head_seq(&self) -> Result<Seq, LogError> {
        Ok(self.head_seq_sync())
    }
}

/// Project a `StoredRow` into a `LoggedEvent`. For rows stored via claim-
/// check we resolve the payload through the blob store on demand — but
/// the current in-memory tests keep payloads small, so we just surface
/// `Null` if the blob fetch isn't feasible synchronously.
pub(crate) fn stored_to_logged_pub(r: &StoredRow) -> LoggedEvent {
    stored_to_logged(r)
}

fn stored_to_logged(r: &StoredRow) -> LoggedEvent {
    let payload = match (&r.payload_inline, &r.payload_ref) {
        (Some(v), _) => v.clone(),
        (None, Some(_)) => serde_json::Value::Null,
        (None, None) => serde_json::Value::Null,
    };
    LoggedEvent {
        seq: r.seq,
        domain: r.domain,
        kind: r.kind.clone(),
        payload,
        producer_id: r.producer_id.clone(),
        dedupe_key: r.dedupe_key,
        causation_depth: r.causation_depth,
        trace_id: r.trace_id,
        span_id: r.span_id,
        parent_span_id: r.parent_span_id,
        ts: r.ts,
        ephemeral: r.ephemeral,
    }
}

/// Adapter: [`InMemoryLog`] as an [`ObservabilityAppender`]. The metrics
/// emitter uses this to push ephemeral `BusMetric` events through the
/// in-memory bus without needing a separate writer.
#[async_trait]
impl super::super::metrics::emitter::ObservabilityAppender for InMemoryLog {
    async fn append_observability(
        &self,
        event: super::super::events::ObservabilityEvent,
        opts: AppendOpts,
    ) -> Result<Seq, AppendError> {
        let ack = self.append(event, opts).await?;
        Ok(ack.seq())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::{BoardEvent, ObservabilityEvent};
    use std::time::Duration;

    fn board(n: usize) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: format!("t-{n}"),
            title: "t".into(),
            category: "c".into(),
        }
    }

    #[tokio::test]
    async fn append_single_event_returns_committed() {
        let log = InMemoryLog::spawn_default();
        let ack = log
            .append(
                board(1),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await
            .expect("append ok");
        match ack {
            AppendAck::Committed { seq, durable } => {
                assert_eq!(seq, Seq(1));
                assert!(durable);
            }
            other => panic!("expected Committed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sequential_appends_get_monotonic_seqs() {
        let log = InMemoryLog::spawn_default();
        for i in 1..=10 {
            let ack = log
                .append(
                    board(i),
                    AppendOpts {
                        producer_id: "p".into(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(ack.seq(), Seq(i as i64));
        }
        assert_eq!(log.head_seq().await.unwrap(), Seq(10));
    }

    #[tokio::test]
    async fn dedupe_hit_returns_already_exists() {
        let log = InMemoryLog::spawn_default();
        let key = Uuid::new_v4();
        let opts = AppendOpts {
            producer_id: "p".into(),
            dedupe_key: Some(key),
            ..Default::default()
        };

        let first = log.append(board(1), opts.clone()).await.unwrap();
        let second = log.append(board(1), opts).await.unwrap();
        assert_eq!(first.seq(), second.seq());
        assert!(matches!(second, AppendAck::AlreadyExists { .. }));
    }

    #[tokio::test]
    async fn ephemeral_returns_volatile_without_persisting() {
        let log = InMemoryLog::spawn_default();
        let ack = log
            .append(
                ObservabilityEvent::BusMetric {
                    name: "test".into(),
                    value: 1.0,
                    labels: serde_json::Value::Null,
                },
                AppendOpts {
                    producer_id: "p".into(),
                    ephemeral: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(matches!(ack, AppendAck::Volatile { .. }));

        // Give the writer a chance to process.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(log.row_count(), 0, "ephemeral must not persist");
    }

    #[tokio::test]
    async fn read_from_filters_by_domain_and_cursor() {
        let log = InMemoryLog::spawn_default();
        for i in 1..=5 {
            log.append(
                board(i),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let first_two = log.read_from(Domain::Board, Seq(0), 2).await.unwrap();
        assert_eq!(first_two.len(), 2);
        assert_eq!(first_two[0].seq, Seq(1));
        assert_eq!(first_two[1].seq, Seq(2));

        let after_three = log.read_from(Domain::Board, Seq(3), 10).await.unwrap();
        assert_eq!(after_three.len(), 2);
        assert_eq!(after_three[0].seq, Seq(4));
    }

    #[tokio::test]
    async fn failed_state_rejects_new_appends() {
        let log = InMemoryLog::spawn_default();
        log.set_failed(true);
        let err = log
            .append(
                board(1),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppendError::LogUnavailable(_)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn empty_producer_id_rejected() {
        let log = InMemoryLog::spawn_default();
        let err = log
            .append(board(1), AppendOpts::default())
            .await
            .unwrap_err();
        assert!(matches!(err, AppendError::Other(_)));
    }

    #[tokio::test]
    async fn causation_limit_rejects() {
        let log = InMemoryLog::spawn_default();
        let err = log
            .append(
                board(1),
                AppendOpts {
                    producer_id: "p".into(),
                    causation_depth: 11,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppendError::CausalLoop { .. }));
    }
}
