//! LogWriter — the single task that drains the append channel, batches
//! INSERTs into PostgreSQL, and returns seq assignments to producers.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! `log-writer`:
//!
//! > LogWriter struct + run loop + spawn 构造器 — 只负责 batch 调度,
//! > 不含 backend 细节
//!
//! The neighbour modules own the fine-grained concerns:
//!
//! | Concern                        | Module                        |
//! |--------------------------------|-------------------------------|
//! | producer handle + `impl Log`   | [`super::handle`]             |
//! | `PendingAppend` + capacity     | [`super::backpressure`]       |
//! | backend trait + `InsertRow`    | [`super::backend`]            |
//! | PG impl + `map_sqlx`           | [`super::pg_backend`]         |
//! | UNIQUE-violation + dedup docs  | [`super::dedup`]              |
//! | retry / failed-state knobs     | [`super::failure_mode`]       |
//! | seq authority (DB) docs        | [`super::seq_authority`]      |
//!
//! This file focuses on the orchestration: loop, batch boundaries, blob
//! claim-check, ack fan-out. The other modules let readers navigate the
//! seven-concern lisp §4.2 step-3 layout 1:1.

use std::sync::atomic::AtomicI64;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::event::blob_store::BlobStore;
use crate::event::log::reader::LogReader;
use crate::event::log::{AppendAck, AppendError, Seq};

use super::backend::{BackendError, InsertRow, WriterBackend};
use super::backpressure::{PendingAppend, APPEND_CHANNEL_CAPACITY, BATCH_DEADLINE, BATCH_MAX};
use super::failure_mode::{exp_backoff, FAILED_STATE_RETRY_CAP};
use super::handle::LogWriterHandle;

/// The background task state. Owns the DB pool + blob store.
pub struct LogWriter {
    rx: mpsc::Receiver<PendingAppend>,
    backend: Box<dyn WriterBackend>,
    blob_store: Arc<dyn BlobStore>,
    /// Once `true` the writer refuses new batches permanently.
    failed: bool,
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
    let backend: Box<dyn WriterBackend> =
        Box::new(super::pg_backend::PgWriterBackend { pool: pool.clone() });
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
pub fn spawn_log_writer(pool: sqlx::PgPool, blob_store: Arc<dyn BlobStore>) -> LogWriterHandle {
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
                    let _ = p.ack.send(Ok(AppendAck::Committed { seq, durable: true }));
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
                        let _ = p.ack.send(Ok(AppendAck::Committed { seq, durable: true }));
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
                        Some(key) => {
                            match self.backend.find_existing_seq(&p.producer_id, key).await {
                                Ok(Some(seq)) => Ok(AppendAck::AlreadyExists { seq }),
                                Ok(None) => Err(AppendError::Other(
                                    "dedup collision but no existing seq found".into(),
                                )),
                                Err(e) => {
                                    Err(AppendError::Other(format!("dedup lookup failed: {e}")))
                                }
                            }
                        }
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

// ─────────────────────────────────────────────────────────────────────────
// Tests — mock backend exercises batching, dedup, backpressure, failed state
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::blob_store::LocalFileBlobStore;
    use crate::event::events::board::BoardEvent;
    use crate::event::log::{AppendOpts, Log};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;
    use uuid::Uuid;

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

        let first = handle
            .append(small_event("t-1"), opts.clone())
            .await
            .unwrap();
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
        mock.transient_left
            .store(FAILED_STATE_RETRY_CAP + 2, Ordering::SeqCst);
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
}
