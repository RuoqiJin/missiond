//! Writer task for [`super::InMemoryLog`].
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 testing-story
//! in-memory-breakdown: `writer_task.rs` encapsulates the single-writer
//! loop that assigns seq, honors the failed flag, performs dedupe
//! lookups, and surfaces ack outcomes to the caller's `oneshot` sender.
//!
//! This mirrors the PG writer on the production side, keeping the
//! in-memory log behaviorally identical to the real bus (single writer
//! assigns seq, append-ack, seq-ordered replay — frozen lisp §4.4).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::super::log::{AppendAck, AppendError, Seq};
use super::super::metrics::BusMetrics;
use super::storage::StoredRow;

/// Entry sent from [`super::InMemoryLog::append`] to the writer task.
pub(super) struct Pending {
    pub(super) row: StoredRow,
    pub(super) dedupe_key: Option<Uuid>,
    pub(super) ephemeral: bool,
    pub(super) ack: oneshot::Sender<Result<AppendAck, AppendError>>,
}

/// The writer task — mirrors `PgLogWriter::run` on the PG side.
pub(super) struct WriterTask {
    pub(super) rx: mpsc::Receiver<Pending>,
    pub(super) next_seq: Arc<AtomicI64>,
    pub(super) rows: Arc<Mutex<Vec<StoredRow>>>,
    pub(super) dedupe: Arc<Mutex<HashMap<(String, Uuid), Seq>>>,
    pub(super) failed: Arc<AtomicBool>,
    pub(super) metrics: Arc<dyn BusMetrics>,
}

impl WriterTask {
    pub(super) async fn run(mut self) {
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
            self.metrics
                .record_append(row.domain, true, payload_bytes(&row));
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

pub(super) fn payload_bytes(row: &StoredRow) -> usize {
    match &row.payload_inline {
        Some(v) => v.to_string().len(),
        None => match &row.payload_ref {
            Some(r) => r.len(),
            None => 0,
        },
    }
}
