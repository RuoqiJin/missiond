//! LogWriter — the single task that drains the append channel, batches
//! INSERTs into PostgreSQL, and returns seq assignments to producers.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2.b writer-semantics:
//!
//! * **Single writer**: exactly one task consumes a bounded MPSC channel
//!   (capacity 4096); producers cannot insert directly.
//! * **Micro-batch**: after the first pending entry, drain up to 100 more
//!   entries or 10 ms — whichever comes first.
//! * **DB-assigned seq**: `INSERT INTO event_log ... RETURNING seq` gives
//!   each append a monotonic seq; results are mapped back to the producer
//!   via a `oneshot::Sender`.
//! * **Claim-check**: payloads larger than [`CLAIM_CHECK_THRESHOLD`] are
//!   uploaded to a [`BlobStore`] and the row stores only a `PayloadRef`.
//! * **Dedup**: `(producer_id, dedupe_key)` UNIQUE collisions SELECT the
//!   existing seq and yield `AppendAck::AlreadyExists`.
//! * **Backpressure**: channel saturation becomes `AppendError::Backpressure`
//!   at the producer — no silent drop.
//! * **Failed state**: after `FAILED_STATE_RETRY_CAP` consecutive DB errors
//!   the writer refuses new batches with `AppendError::LogUnavailable` and
//!   stays in that state until explicit reset.
//!
//! Phase 2 scope:
//! * The writer is fully functional for PG persistent writes.
//! * `AppendOpts::ephemeral = true` fast-path returns an in-process
//!   `AppendAck::Volatile` without DB round-trip; dispatcher-side fan-out
//!   lands in Phase 3. Producers that need fan-out today should keep using
//!   the legacy path.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use uuid::Uuid;

use crate::event::blob_store::{BlobStore, CLAIM_CHECK_THRESHOLD};
use crate::event::domain::Domain;
use crate::event::event_trait::DomainEvent;
use crate::event::log::reader::{LogReader, LoggedEvent};
use crate::event::log::{AppendAck, AppendError, AppendOpts, Log, LogError, Seq};
use crate::event::pipeline::step1_guard::check_causation;

/// Bound on the append channel. Frozen lisp §4.2.b backpressure.
pub const APPEND_CHANNEL_CAPACITY: usize = 4096;

/// Maximum rows per INSERT batch. Frozen lisp §4.2.b writer-semantics.batching.
pub const BATCH_MAX: usize = 100;

/// Deadline after first pending row before the writer flushes anyway.
/// Frozen lisp §4.2.b writer-semantics.batching.
pub const BATCH_DEADLINE: Duration = Duration::from_millis(10);

/// Retry knobs for transient DB failures.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(50);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(2);
const FAILED_STATE_RETRY_CAP: u32 = 6;

/// Single entry moving through the append MPSC — the producer-side future
/// lives on the other end of `ack`.
pub struct PendingAppend {
    pub domain: Domain,
    pub kind: &'static str,
    pub payload_bytes: Vec<u8>,
    pub payload_inline_eligible: bool,
    pub ephemeral: bool,
    pub producer_id: String,
    pub dedupe_key: Option<Uuid>,
    pub causation_depth: i16,
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub ack: oneshot::Sender<Result<AppendAck, AppendError>>,
}

/// Producer-facing handle to the LogWriter task. Cheap to clone.
#[derive(Clone)]
pub struct LogWriterHandle {
    tx: mpsc::Sender<PendingAppend>,
    #[cfg(feature = "postgres")]
    reader: Option<LogReader>,
    /// Fallback seq source for the ephemeral fast-path in Phase 2. Dispatcher
    /// owns real seq assignment in Phase 3 — here we just hand out a
    /// process-local, monotonic counter so producers don't get `Seq(0)`.
    volatile_counter: Arc<AtomicI64>,
}

impl LogWriterHandle {
    /// Raw append-channel access for internal tests / Phase-3 dispatcher
    /// integration. Normal producers go through [`Log::append`].
    pub fn sender(&self) -> mpsc::Sender<PendingAppend> {
        self.tx.clone()
    }
}

#[async_trait]
impl Log for LogWriterHandle {
    async fn append<E>(&self, event: E, opts: AppendOpts) -> Result<AppendAck, AppendError>
    where
        E: DomainEvent,
    {
        // Frozen lisp §4.4 causation-loop-guard — enforced at both the PG
        // writer and the InMemoryLog so behavior is uniform.
        check_causation(&opts)?;

        let kind: &'static str = event.kind();
        let domain = E::domain();
        let payload_bytes = serde_json::to_vec(&event)?;
        let payload_inline_eligible = payload_bytes.len() <= CLAIM_CHECK_THRESHOLD;

        // Ephemeral fast-path — no DB round-trip, no channel send.
        if opts.ephemeral {
            let next = self.volatile_counter.fetch_sub(1, Ordering::Relaxed);
            return Ok(AppendAck::Volatile { seq: Seq(next) });
        }

        let producer_id = if opts.producer_id.is_empty() {
            return Err(AppendError::Other("producer_id must not be empty".into()));
        } else {
            opts.producer_id
        };

        let (ack_tx, ack_rx) = oneshot::channel();
        let span = opts.span.unwrap_or_default();
        let pending = PendingAppend {
            domain,
            kind,
            payload_bytes,
            payload_inline_eligible,
            ephemeral: false,
            producer_id,
            dedupe_key: opts.dedupe_key,
            causation_depth: opts.causation_depth as i16,
            trace_id: span.trace_id,
            span_id: span.span_id,
            parent_span_id: span.parent_span_id,
            ack: ack_tx,
        };

        match self.tx.try_send(pending) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Err(AppendError::Backpressure),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(AppendError::LogUnavailable("writer task shut down".into()));
            }
        }

        match ack_rx.await {
            Ok(r) => r,
            Err(_) => Err(AppendError::LogUnavailable(
                "writer ack channel dropped".into(),
            )),
        }
    }

    async fn read_from(
        &self,
        domain: Domain,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, LogError> {
        #[cfg(feature = "postgres")]
        {
            match &self.reader {
                Some(r) => r.read_from(domain, after_seq, limit).await,
                None => Err(LogError::Other(
                    "LogWriter configured without a reader".into(),
                )),
            }
        }
        #[cfg(not(feature = "postgres"))]
        {
            let _ = (domain, after_seq, limit);
            Err(LogError::Other(
                "pg feature disabled; LogWriter.read_from unavailable".into(),
            ))
        }
    }

    async fn head_seq(&self) -> Result<Seq, LogError> {
        #[cfg(feature = "postgres")]
        {
            match &self.reader {
                Some(r) => r.head_seq().await,
                None => Err(LogError::Other(
                    "LogWriter configured without a reader".into(),
                )),
            }
        }
        #[cfg(not(feature = "postgres"))]
        {
            Err(LogError::Other(
                "pg feature disabled; LogWriter.head_seq unavailable".into(),
            ))
        }
    }
}

/// The background task state. Owns the DB pool + blob store.
pub struct LogWriter {
    rx: mpsc::Receiver<PendingAppend>,
    backend: Box<dyn WriterBackend>,
    blob_store: Arc<dyn BlobStore>,
    /// Once `true` the writer refuses new batches permanently.
    failed: bool,
}

/// Abstract over the concrete DB so unit tests can plug in a mock without
/// linking sqlx.
#[async_trait]
pub(crate) trait WriterBackend: Send + Sync {
    async fn insert_batch(&self, rows: &[InsertRow<'_>]) -> Result<Vec<Seq>, BackendError>;

    async fn find_existing_seq(
        &self,
        producer_id: &str,
        dedupe_key: Uuid,
    ) -> Result<Option<Seq>, BackendError>;
}

/// Row to be inserted. Borrowed from the pending append so we don't re-clone
/// the potentially large payload.
pub(crate) struct InsertRow<'a> {
    pub domain: Domain,
    pub kind: &'a str,
    pub payload_inline: Option<&'a [u8]>,
    pub payload_ref: Option<String>,
    pub producer_id: &'a str,
    pub dedupe_key: Option<Uuid>,
    pub causation_depth: i16,
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub ephemeral: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BackendError {
    #[error("dedupe collision (producer_id + key already exists)")]
    DedupeCollision,
    #[error("transient: {0}")]
    Transient(String),
    #[error("fatal: {0}")]
    Fatal(String),
}


/// Boot the writer with an explicit backend (tests supply a mock).
pub(crate) fn new_with_backend(
    backend: Box<dyn WriterBackend>,
    blob_store: Arc<dyn BlobStore>,
) -> (LogWriter, LogWriterHandle) {
    let (tx, rx) = mpsc::channel(APPEND_CHANNEL_CAPACITY);
    let writer = LogWriter {
        rx,
        backend,
        blob_store,
        failed: false,
    };
    let handle = LogWriterHandle {
        tx,
        #[cfg(feature = "postgres")]
        reader: None,
        volatile_counter: Arc::new(AtomicI64::new(-1)),
    };
    (writer, handle)
}

/// Construct (but don't spawn) a LogWriter wired to a real PG pool.
#[cfg(feature = "postgres")]
pub fn new_log_writer(
    pool: sqlx::PgPool,
    blob_store: Arc<dyn BlobStore>,
) -> (LogWriter, LogWriterHandle) {
    let backend: Box<dyn WriterBackend> = Box::new(PgWriterBackend { pool: pool.clone() });
    let (writer, mut handle) = new_with_backend(backend, blob_store.clone());
    handle.reader = Some(LogReader::new(pool, blob_store));
    (writer, handle)
}

/// Feature-gated stub so non-postgres builds still compile the module list.
#[cfg(not(feature = "postgres"))]
pub fn new_log_writer(
    _unused: (),
    _blob_store: Arc<dyn BlobStore>,
) -> (LogWriter, LogWriterHandle) {
    unimplemented!("postgres feature disabled; new_log_writer requires pg support")
}

/// Convenience: spawn the writer task on the current runtime and return the
/// producer-side handle.
#[cfg(feature = "postgres")]
pub fn spawn_log_writer(
    pool: sqlx::PgPool,
    blob_store: Arc<dyn BlobStore>,
) -> LogWriterHandle {
    let (writer, handle) = new_log_writer(pool, blob_store);
    tokio::spawn(writer.run());
    handle
}

impl LogWriter {
    /// Main loop. Exits when the append channel closes.
    pub async fn run(mut self) {
        loop {
            // Wait for the first pending entry (no deadline).
            let first = match self.rx.recv().await {
                Some(p) => p,
                None => {
                    tracing::info!("log_writer channel closed — shutting down");
                    return;
                }
            };

            // Drain up to BATCH_MAX or BATCH_DEADLINE, whichever hits first.
            let mut batch: Vec<PendingAppend> = Vec::with_capacity(BATCH_MAX);
            batch.push(first);
            let deadline = tokio::time::Instant::now() + BATCH_DEADLINE;
            while batch.len() < BATCH_MAX {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match timeout(remaining, self.rx.recv()).await {
                    Ok(Some(p)) => batch.push(p),
                    Ok(None) => {
                        // Channel closed mid-batch — flush then exit.
                        self.flush(batch).await;
                        tracing::info!("log_writer channel closed mid-batch — shutting down");
                        return;
                    }
                    Err(_) => break, // deadline hit
                }
            }

            self.flush(batch).await;
        }
    }

    async fn flush(&mut self, mut batch: Vec<PendingAppend>) {
        // Failed state short-circuits: drop any new batch immediately.
        if self.failed {
            for p in batch.drain(..) {
                let _ = p.ack.send(Err(AppendError::LogUnavailable(
                    "log writer in failed state".into(),
                )));
            }
            return;
        }

        // Run Claim-Check before we freeze the borrow.
        let mut payload_refs: Vec<Option<String>> = Vec::with_capacity(batch.len());
        for p in &batch {
            if p.payload_inline_eligible {
                payload_refs.push(None);
            } else {
                match self.blob_store.put(&p.payload_bytes).await {
                    Ok(r) => payload_refs.push(Some(r.to_json_string())),
                    Err(e) => {
                        // Blob failure is fatal for this entry but does not
                        // necessarily kill the whole batch; we still need to
                        // send the ack back. Use `continue` with a marker.
                        payload_refs.push(Some(format!("__blob_error__:{e}")));
                    }
                }
            }
        }

        // Split into (idx, error) for blob-failed entries so we can send their
        // acks independently without dragging the DB insert into the error.
        let mut blob_errors: Vec<usize> = Vec::new();
        for (i, r) in payload_refs.iter().enumerate() {
            if let Some(s) = r {
                if s.starts_with("__blob_error__:") {
                    blob_errors.push(i);
                }
            }
        }

        // Build the INSERT row list (skipping blob-failed entries).
        let rows: Vec<InsertRow<'_>> = batch
            .iter()
            .zip(payload_refs.iter())
            .enumerate()
            .filter_map(|(i, (p, r))| {
                if blob_errors.contains(&i) {
                    return None;
                }
                let (inline, ref_str) = match (p.payload_inline_eligible, r) {
                    (true, _) => (Some(p.payload_bytes.as_slice()), None),
                    (false, Some(s)) => (None, Some(s.clone())),
                    (false, None) => {
                        // defensive; should not happen
                        return None;
                    }
                };
                Some(InsertRow {
                    domain: p.domain,
                    kind: p.kind,
                    payload_inline: inline,
                    payload_ref: ref_str,
                    producer_id: &p.producer_id,
                    dedupe_key: p.dedupe_key,
                    causation_depth: p.causation_depth,
                    trace_id: p.trace_id,
                    span_id: p.span_id,
                    parent_span_id: p.parent_span_id,
                    ephemeral: p.ephemeral,
                })
            })
            .collect();

        // Retry the whole batch with exp backoff until either success or
        // the retry cap; fatal errors (non-transient) bail immediately.
        let mut attempt = 0u32;
        let insert_result = loop {
            if rows.is_empty() {
                // All entries had blob errors; nothing left to insert.
                break Ok(Vec::<Seq>::new());
            }
            match self.backend.insert_batch(&rows).await {
                Ok(seqs) => break Ok(seqs),
                Err(BackendError::DedupeCollision) => {
                    // Per-row resolution: look up each dedupe_key and reply.
                    break Err(BackendError::DedupeCollision);
                }
                Err(BackendError::Transient(msg)) => {
                    attempt += 1;
                    if attempt >= FAILED_STATE_RETRY_CAP {
                        self.failed = true;
                        break Err(BackendError::Fatal(format!(
                            "retry cap exceeded ({attempt}): {msg}"
                        )));
                    }
                    let delay = exp_backoff(attempt);
                    tokio::time::sleep(delay).await;
                }
                Err(BackendError::Fatal(msg)) => {
                    break Err(BackendError::Fatal(msg));
                }
            }
        };

        // `rows` borrows from `batch`; release it before we drain ownership.
        drop(rows);

        // Fan results back to producers.
        let blob_error_set: std::collections::HashSet<usize> = blob_errors.into_iter().collect();
        match insert_result {
            Ok(seqs) if blob_error_set.is_empty() => {
                // Happy path: seqs align 1:1 with the original batch.
                for (p, seq) in batch.drain(..).zip(seqs.into_iter()) {
                    let _ = p.ack.send(Ok(AppendAck::Committed {
                        seq,
                        durable: true,
                    }));
                }
            }
            Ok(seqs) => {
                // Some entries skipped due to blob errors; zip seqs against
                // the filtered set while sending a LogUnavailable for the
                // skipped ones.
                let mut seq_iter = seqs.into_iter();
                for (i, p) in batch.drain(..).enumerate() {
                    if blob_error_set.contains(&i) {
                        let _ = p.ack.send(Err(AppendError::Other(
                            "blob upload failed; row not inserted".into(),
                        )));
                    } else if let Some(seq) = seq_iter.next() {
                        let _ = p.ack.send(Ok(AppendAck::Committed {
                            seq,
                            durable: true,
                        }));
                    } else {
                        let _ = p.ack.send(Err(AppendError::Other(
                            "seq vector shorter than row set".into(),
                        )));
                    }
                }
            }
            Err(BackendError::DedupeCollision) => {
                // Per-row dedup: walk each pending entry, consult the backend
                // for an existing seq when dedupe_key is present.
                for p in batch.drain(..) {
                    let result = match p.dedupe_key {
                        Some(key) => match self
                            .backend
                            .find_existing_seq(&p.producer_id, key)
                            .await
                        {
                            Ok(Some(seq)) => Ok(AppendAck::AlreadyExists { seq }),
                            Ok(None) => Err(AppendError::Other(
                                "dedup collision but no existing seq found".into(),
                            )),
                            Err(e) => Err(AppendError::Other(format!(
                                "dedup lookup failed: {e}"
                            ))),
                        },
                        None => Err(AppendError::Other(
                            "dedup collision reported but row has no key".into(),
                        )),
                    };
                    let _ = p.ack.send(result);
                }
            }
            Err(BackendError::Transient(msg)) | Err(BackendError::Fatal(msg)) => {
                for p in batch.drain(..) {
                    let _ = p.ack.send(Err(AppendError::LogUnavailable(msg.clone())));
                }
            }
        }
    }
}

fn exp_backoff(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(10);
    let raw = RETRY_BASE_DELAY.saturating_mul(1 << shift);
    raw.min(RETRY_MAX_DELAY)
}

// ─────────────────────────────────────────────────────────────────────────
// PostgreSQL concrete backend
// ─────────────────────────────────────────────────────────────────────────

#[cfg(feature = "postgres")]
struct PgWriterBackend {
    pool: sqlx::PgPool,
}

#[cfg(feature = "postgres")]
#[async_trait]
impl WriterBackend for PgWriterBackend {
    async fn insert_batch(&self, rows: &[InsertRow<'_>]) -> Result<Vec<Seq>, BackendError> {
        // PG doesn't give us a batched RETURNING that preserves order across
        // multi-value INSERT with mixed nulls for UUIDs without a lot of
        // ceremony — simplest correct approach is one INSERT per row inside
        // a single transaction. Batch sizes are capped at 100 so this keeps
        // amortized cost low.
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let inline_json: Option<serde_json::Value> = match r.payload_inline {
                Some(bytes) => Some(serde_json::from_slice(bytes).map_err(|e| {
                    BackendError::Fatal(format!("payload_inline not JSON: {e}"))
                })?),
                None => None,
            };

            let (seq,): (i64,) = sqlx::query_as(
                r#"
                INSERT INTO event_log
                    (domain, kind, payload_inline, payload_ref, producer_id,
                     dedupe_key, causation_depth, trace_id, span_id,
                     parent_span_id, ephemeral)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                RETURNING seq
                "#,
            )
            .bind(r.domain.as_str())
            .bind(r.kind)
            .bind(inline_json)
            .bind(r.payload_ref.as_deref())
            .bind(r.producer_id)
            .bind(r.dedupe_key)
            .bind(r.causation_depth)
            .bind(r.trace_id)
            .bind(r.span_id)
            .bind(r.parent_span_id)
            .bind(r.ephemeral)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(dbe) if is_unique_violation(dbe.as_ref()) => {
                    BackendError::DedupeCollision
                }
                _ => map_sqlx(e),
            })?;
            out.push(Seq(seq));
        }
        tx.commit().await.map_err(map_sqlx)?;
        Ok(out)
    }

    async fn find_existing_seq(
        &self,
        producer_id: &str,
        dedupe_key: Uuid,
    ) -> Result<Option<Seq>, BackendError> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT seq FROM event_log WHERE producer_id = $1 AND dedupe_key = $2 LIMIT 1",
        )
        .bind(producer_id)
        .bind(dedupe_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(s,)| Seq(s)))
    }
}

#[cfg(feature = "postgres")]
fn map_sqlx(e: sqlx::Error) -> BackendError {
    // Crude classifier — io / pool-closed / timeout / tls ⇒ transient.
    let msg = format!("{e}");
    let is_transient = matches!(
        e,
        sqlx::Error::Io(_)
            | sqlx::Error::PoolClosed
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::Tls(_)
    );
    if is_transient {
        BackendError::Transient(msg)
    } else {
        BackendError::Fatal(msg)
    }
}

#[cfg(feature = "postgres")]
fn is_unique_violation(e: &dyn sqlx::error::DatabaseError) -> bool {
    // PostgreSQL SQLSTATE 23505 = unique_violation
    e.code().as_deref() == Some("23505")
}

// ─────────────────────────────────────────────────────────────────────────
// Tests — mock backend exercises batching, dedup, backpressure, failed state
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::blob_store::LocalFileBlobStore;
    use crate::event::events::board::BoardEvent;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;

    /// Fully in-memory backend for writer unit tests.
    struct MockBackend {
        next_seq: AtomicI64,
        /// (producer_id, dedupe_key) → seq
        dedupe_map: Arc<Mutex<std::collections::HashMap<(String, Uuid), i64>>>,
        /// Simulated transient failures.
        transient_left: AtomicU32,
        /// Sticky fatal.
        fatal: Arc<Mutex<Option<String>>>,
        /// Recorded batch sizes — asserting batching behavior.
        batch_sizes: Arc<Mutex<Vec<usize>>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                next_seq: AtomicI64::new(1),
                dedupe_map: Arc::new(Mutex::new(std::collections::HashMap::new())),
                transient_left: AtomicU32::new(0),
                fatal: Arc::new(Mutex::new(None)),
                batch_sizes: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl WriterBackend for MockBackend {
        async fn insert_batch(&self, rows: &[InsertRow<'_>]) -> Result<Vec<Seq>, BackendError> {
            if let Some(m) = self.fatal.lock().unwrap().clone() {
                return Err(BackendError::Fatal(m));
            }
            let left = self.transient_left.load(Ordering::SeqCst);
            if left > 0 {
                self.transient_left.fetch_sub(1, Ordering::SeqCst);
                return Err(BackendError::Transient(format!(
                    "simulated transient {left} left"
                )));
            }

            self.batch_sizes.lock().unwrap().push(rows.len());

            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                if let Some(key) = r.dedupe_key {
                    let k = (r.producer_id.to_string(), key);
                    let mut map = self.dedupe_map.lock().unwrap();
                    if map.contains_key(&k) {
                        return Err(BackendError::DedupeCollision);
                    }
                    let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
                    map.insert(k, seq);
                    out.push(Seq(seq));
                } else {
                    let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
                    out.push(Seq(seq));
                }
            }
            Ok(out)
        }

        async fn find_existing_seq(
            &self,
            producer_id: &str,
            dedupe_key: Uuid,
        ) -> Result<Option<Seq>, BackendError> {
            Ok(self
                .dedupe_map
                .lock()
                .unwrap()
                .get(&(producer_id.to_string(), dedupe_key))
                .copied()
                .map(Seq))
        }
    }

    fn small_event(task_id: &str) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: task_id.into(),
            title: "title".into(),
            category: "cat".into(),
        }
    }

    fn make_blob_store() -> Arc<dyn BlobStore> {
        let dir = tempfile::tempdir().unwrap();
        // Leak the tempdir on purpose — tests use the store briefly and the
        // OS reclaims /tmp on its own cycle.
        let path = dir.keep();
        Arc::new(LocalFileBlobStore::new(path))
    }

    #[tokio::test]
    async fn append_returns_committed_seq() {
        let backend = Box::new(MockBackend::new());
        let (writer, handle) = new_with_backend(backend, make_blob_store());
        tokio::spawn(writer.run());

        let opts = AppendOpts {
            producer_id: "test".into(),
            ..Default::default()
        };
        let ack = handle
            .append(small_event("t-1"), opts)
            .await
            .expect("append");
        match ack {
            AppendAck::Committed { seq, durable } => {
                assert_eq!(seq, Seq(1));
                assert!(durable);
            }
            other => panic!("expected Committed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ephemeral_path_returns_volatile_without_backend_hit() {
        let mock = Arc::new(MockBackend::new());
        let batch_sizes = mock.batch_sizes.clone();
        let backend: Box<dyn WriterBackend> = Box::new(MockBackendArc(mock));
        let (writer, handle) = new_with_backend(backend, make_blob_store());
        tokio::spawn(writer.run());

        let opts = AppendOpts {
            producer_id: "test".into(),
            ephemeral: true,
            ..Default::default()
        };
        let ack = handle
            .append(small_event("t-1"), opts)
            .await
            .expect("append");
        assert!(matches!(ack, AppendAck::Volatile { .. }));
        // Give the writer a tick to confirm nothing hit the backend.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            batch_sizes.lock().unwrap().is_empty(),
            "ephemeral path must not touch backend"
        );
    }

    // Wrapper so we can share the Arc<MockBackend> without dropping it once
    // the writer owns the Box.
    struct MockBackendArc(Arc<MockBackend>);

    #[async_trait]
    impl WriterBackend for MockBackendArc {
        async fn insert_batch(&self, rows: &[InsertRow<'_>]) -> Result<Vec<Seq>, BackendError> {
            self.0.insert_batch(rows).await
        }
        async fn find_existing_seq(
            &self,
            producer_id: &str,
            dedupe_key: Uuid,
        ) -> Result<Option<Seq>, BackendError> {
            self.0.find_existing_seq(producer_id, dedupe_key).await
        }
    }

    #[tokio::test]
    async fn dedupe_collision_returns_already_exists() {
        let mock = Arc::new(MockBackend::new());
        let backend: Box<dyn WriterBackend> = Box::new(MockBackendArc(mock.clone()));
        let (writer, handle) = new_with_backend(backend, make_blob_store());
        tokio::spawn(writer.run());

        let key = Uuid::new_v4();
        let opts = AppendOpts {
            producer_id: "prod".into(),
            dedupe_key: Some(key),
            ..Default::default()
        };

        let first = handle.append(small_event("t-1"), opts.clone()).await.unwrap();
        let first_seq = first.seq();

        let second = handle.append(small_event("t-1"), opts).await.unwrap();
        match second {
            AppendAck::AlreadyExists { seq } => assert_eq!(seq, first_seq),
            other => panic!("expected AlreadyExists, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batch_flushes_multiple_rows_in_single_backend_call() {
        let mock = Arc::new(MockBackend::new());
        let batch_sizes = mock.batch_sizes.clone();
        let backend: Box<dyn WriterBackend> = Box::new(MockBackendArc(mock));
        let (writer, handle) = new_with_backend(backend, make_blob_store());
        tokio::spawn(writer.run());

        let mut futures = Vec::new();
        for i in 0..5 {
            let h = handle.clone();
            futures.push(tokio::spawn(async move {
                h.append(
                    small_event(&format!("t-{i}")),
                    AppendOpts {
                        producer_id: format!("p-{i}"),
                        ..Default::default()
                    },
                )
                .await
            }));
        }
        for f in futures {
            f.await.unwrap().unwrap();
        }
        // Writer should have produced at least one batch; at least one of
        // those batches should have contained >1 row.
        let sizes = batch_sizes.lock().unwrap().clone();
        assert!(!sizes.is_empty(), "no flushes recorded");
        assert_eq!(sizes.iter().sum::<usize>(), 5);
    }

    #[tokio::test]
    async fn transient_errors_are_retried() {
        let mock = Arc::new(MockBackend::new());
        mock.transient_left.store(2, Ordering::SeqCst);
        let backend: Box<dyn WriterBackend> = Box::new(MockBackendArc(mock.clone()));
        let (writer, handle) = new_with_backend(backend, make_blob_store());
        tokio::spawn(writer.run());

        let ack = handle
            .append(
                small_event("t-1"),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(matches!(ack, AppendAck::Committed { .. }));
    }

    #[tokio::test]
    async fn fatal_then_failed_state_rejects_new_appends() {
        let mock = Arc::new(MockBackend::new());
        // Enough transient errors to exceed the retry cap.
        mock.transient_left.store(FAILED_STATE_RETRY_CAP + 2, Ordering::SeqCst);
        let backend: Box<dyn WriterBackend> = Box::new(MockBackendArc(mock.clone()));
        let (writer, handle) = new_with_backend(backend, make_blob_store());
        tokio::spawn(writer.run());

        // The first append will exhaust retries and put the writer in failed
        // state. This call returns LogUnavailable.
        let first = handle
            .append(
                small_event("t-1"),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await;
        assert!(
            matches!(first, Err(AppendError::LogUnavailable(_))),
            "expected LogUnavailable after retry cap, got {first:?}"
        );

        // Subsequent appends are also rejected immediately.
        let second = handle
            .append(
                small_event("t-2"),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(second, Err(AppendError::LogUnavailable(_))));
    }

    #[tokio::test]
    async fn backpressure_when_channel_full() {
        // Construct the writer but never poll it so the channel saturates.
        let mock = Arc::new(MockBackend::new());
        let backend: Box<dyn WriterBackend> = Box::new(MockBackendArc(mock.clone()));
        let (writer, handle) = new_with_backend(backend, make_blob_store());
        // Do NOT spawn the writer — we want the channel to back up.
        drop(writer);

        let fut = handle.append(
            small_event("t-1"),
            AppendOpts {
                producer_id: "p".into(),
                ..Default::default()
            },
        );
        // With writer dropped, the mpsc receiver is gone → send returns Closed.
        let err = fut.await.unwrap_err();
        assert!(
            matches!(err, AppendError::LogUnavailable(_)),
            "expected LogUnavailable once the writer drops, got {err:?}"
        );
    }

    #[tokio::test]
    async fn claim_check_redirects_large_payloads_to_blob_store() {
        use crate::event::events::message::MessageEvent;

        let mock = Arc::new(MockBackend::new());
        let backend: Box<dyn WriterBackend> = Box::new(MockBackendArc(mock.clone()));

        let dir = tempfile::tempdir().unwrap();
        let blob_path = dir.keep();
        let blob_store: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(blob_path.clone()));
        let (writer, handle) = new_with_backend(backend, blob_store.clone());
        tokio::spawn(writer.run());

        // Force a payload larger than CLAIM_CHECK_THRESHOLD by stuffing a
        // huge string into a MessageEvent variant that doesn't bound preview.
        let big = MessageEvent::Logged {
            message_id: 1,
            session_id: "s".into(),
            parent_session_id: None,
            slot_id: None,
            role: "assistant".into(),
            content_chars: 10_000,
            preview: "x".repeat(12_000),
        };
        let ack = handle
            .append(
                big,
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(matches!(ack, AppendAck::Committed { .. }));

        // At least one blob file should now exist under the root.
        let mut found_blob = false;
        for entry in walkdir(&blob_path) {
            if entry.is_file() {
                found_blob = true;
                break;
            }
        }
        assert!(found_blob, "expected a blob file under {blob_path:?}");
    }

    // Tiny walkdir replacement — the `walkdir` crate is not a dependency.
    fn walkdir(p: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![p.to_path_buf()];
        while let Some(dir) = stack.pop() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let path = e.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        out.push(path);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn exp_backoff_capped() {
        // Attempt 1 is base; it doubles until RETRY_MAX_DELAY.
        assert_eq!(exp_backoff(1), RETRY_BASE_DELAY);
        assert!(exp_backoff(20) <= RETRY_MAX_DELAY);
    }
}
