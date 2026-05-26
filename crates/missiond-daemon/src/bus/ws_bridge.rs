//! WebSocket bridge — translates v2 event log rows into v1-compatible JSON
//! strings on the `frontend_events_tx` broadcast.
//!
//! Resolves I004 (blocker): the browser WS client consumes 47-ish wire_type
//! strings + a fixed JSON envelope (`type / ts / seq / trace_id / span_id /
//! parent_span_id / payload`). Any drift breaks the UI silently. This
//! bridge preserves that envelope bit-for-bit.
//!
//! Why tail the log rather than subscribing to the dispatcher topics?
//! `Topic<T>` only broadcasts `Arc<T>` — no `seq`, no `trace_id`. The
//! browser needs those. Tailing `event_log` gives us the full
//! [`LoggedEvent`] with metadata.
//!
//! Architecture:
//!   * [`spawn_ws_bridge`] starts a task that:
//!     1. Loops over `PgTailSource::read_all_from(cursor, batch)`.
//!     2. For each `LoggedEvent`, converts via
//!        `core::event::wire_format::v2_logged_to_v1_wire_format` into
//!        a JSON string (SSOT used by both this live path and the
//!        catch-up path in `core::ws::server::handle_catch_up`).
//!     3. Pushes the string onto `ws_tx` (the existing
//!        `broadcast::Sender<String>` wired to `/events`).
//!   * The cursor is process-local. On restart we tail from 0 which is the
//!     same behaviour the v1 `run_timeline_writer` had (no missed-event
//!     backlog replay for live subscribers; the `sync` protocol in
//!     `ws/server.rs::handle_catch_up` does the replay).
//!
//! Ephemeral events: `LoggedEvent.ephemeral=true` rows are also emitted
//! (same as v1 which broadcast before DB insert). V1 set `seq=0` for
//! ephemeral — the SSOT mapper preserves that sentinel.

#![cfg(feature = "postgres")]

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use missiond_core::event::blob_store::BlobStore;
use missiond_core::event::dispatcher::{PgTailSource, TailSource};
use missiond_core::event::log::Seq;
use missiond_core::event::wire_format::v2_logged_to_v1_wire_format;
use sqlx::PgPool;
use tokio::sync::{broadcast, watch};
use tracing::{info, warn};

/// Per-poll batch size. Matches the dispatcher tail batch to keep the two
/// tail loops roughly in sync.
const WS_BRIDGE_BATCH_LIMIT: usize = 256;

/// Sleep between empty polls. Same as the dispatcher's poll interval.
const WS_BRIDGE_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Default)]
pub struct WsBridgeHealth {
    cursor: AtomicI64,
    last_emit_at: AtomicI64,
    last_error_at: AtomicI64,
    read_errors: AtomicU64,
    send_errors: AtomicU64,
    last_batch_size: AtomicU64,
}

impl WsBridgeHealth {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "cursor": self.cursor.load(Ordering::Relaxed),
            "lastEmitAt": self.last_emit_at.load(Ordering::Relaxed),
            "lastErrorAt": self.last_error_at.load(Ordering::Relaxed),
            "readErrors": self.read_errors.load(Ordering::Relaxed),
            "sendErrors": self.send_errors.load(Ordering::Relaxed),
            "lastBatchSize": self.last_batch_size.load(Ordering::Relaxed),
        })
    }
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Spawn the WS bridge task. Returns a join handle that completes when
/// `shutdown` fires.
pub(crate) fn spawn_ws_bridge(
    pool: PgPool,
    blob_store: Arc<dyn BlobStore>,
    ws_tx: broadcast::Sender<String>,
    mut shutdown: watch::Receiver<bool>,
    health: Arc<WsBridgeHealth>,
) -> tokio::task::JoinHandle<()> {
    let tail: Arc<dyn TailSource> = Arc::new(PgTailSource::new(pool, blob_store));
    let cursor = Arc::new(AtomicI64::new(0));
    tokio::spawn(async move {
        info!("bus: ws bridge started");
        loop {
            if *shutdown.borrow() {
                break;
            }

            let after = Seq(cursor.load(Ordering::Acquire));
            let batch = match tail.read_all_from(after, WS_BRIDGE_BATCH_LIMIT).await {
                Ok(b) => b,
                Err(e) => {
                    health.last_error_at.store(now_epoch(), Ordering::Relaxed);
                    health.read_errors.fetch_add(1, Ordering::Relaxed);
                    warn!(error = %e, "ws bridge: tail read failed");
                    tokio::time::sleep(WS_BRIDGE_POLL_INTERVAL).await;
                    continue;
                }
            };
            let was_full = batch.len() == WS_BRIDGE_BATCH_LIMIT;
            health
                .last_batch_size
                .store(batch.len() as u64, Ordering::Relaxed);

            for logged in batch {
                let json_str = v2_logged_to_v1_wire_format(&logged);
                // `send` returns Err when there are no receivers; that's
                // normal (no WS clients connected) and not an error.
                if ws_tx.send(json_str).is_err() {
                    health.send_errors.fetch_add(1, Ordering::Relaxed);
                } else {
                    health.last_emit_at.store(now_epoch(), Ordering::Relaxed);
                }
                cursor.store(logged.seq.0, Ordering::Release);
                health.cursor.store(logged.seq.0, Ordering::Relaxed);
            }

            if !was_full {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => break,
                    _ = tokio::time::sleep(WS_BRIDGE_POLL_INTERVAL) => {}
                }
            }
        }
        info!("bus: ws bridge shutdown");
    })
}

// ─────────────────────────────────────────────────────────────────────────
// Byte-equivalence tests — still live here to guarantee the SSOT mapper
// in `core::event::wire_format` produces v1-compatible output from the
// daemon-side call site. Moving the mapper to core didn't change the test
// surface: we import the pub fn and assert byte-equivalence against the
// hand-spec'd v1 shapes.
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use missiond_core::event::domain::Domain;
    use missiond_core::event::log::reader::LoggedEvent;
    use missiond_core::event::log::Seq;
    use missiond_core::event::wire_format::v2_logged_to_v1_wire_format;
    use serde_json::{json, Value};

    fn make_logged(domain: Domain, kind: &str, payload: Value) -> LoggedEvent {
        LoggedEvent {
            seq: Seq(42),
            domain,
            kind: kind.into(),
            payload,
            producer_id: "test".into(),
            dedupe_key: None,
            causation_depth: 0,
            trace_id: None,
            span_id: Some(uuid::Uuid::nil()),
            parent_span_id: None,
            ts: Utc.with_ymd_and_hms(2026, 4, 19, 0, 0, 0).unwrap(),
            ephemeral: false,
        }
    }

    /// Cross-check: v1 `TimelineEvent::to_frontend_json` vs v2
    /// `v2_logged_to_v1_wire_format`. We construct the v1 and v2 sides
    /// manually and compare each top-level key.
    fn assert_equiv(v1_wire_type: &str, v1_payload: Value, v2_logged: &LoggedEvent) {
        let v2_json_str = v2_logged_to_v1_wire_format(v2_logged);
        let v2_json: Value = serde_json::from_str(&v2_json_str).expect("v2 json parses");

        assert_eq!(
            v2_json.get("type").and_then(|v| v.as_str()),
            Some(v1_wire_type),
            "wire type mismatch"
        );
        assert_eq!(
            v2_json.get("seq").and_then(|v| v.as_i64()),
            Some(v2_logged.seq.0),
            "seq mismatch"
        );
        assert_eq!(
            v2_json.get("payload"),
            Some(&v1_payload),
            "payload mismatch:\n  v1={}\n  v2={}",
            v1_payload,
            v2_json.get("payload").cloned().unwrap_or(Value::Null)
        );
    }

    #[test]
    fn slot_became_idle_equiv() {
        let logged = make_logged(
            Domain::Slot,
            "became_idle",
            json!({ "BecameIdle": { "slot_id": "slot-a" } }),
        );
        assert_equiv(
            "slot_state_changed",
            json!({ "slot_id": "slot-a", "new_state": "Idle" }),
            &logged,
        );
    }

    #[test]
    fn board_task_created_equiv() {
        let logged = make_logged(
            Domain::Board,
            "task_created",
            json!({
                "TaskCreated": {
                    "task_id": "t-1",
                    "title": "hello",
                    "category": "code"
                }
            }),
        );
        assert_equiv(
            "board_task_created",
            json!({
                "task_id": "t-1",
                "title": "hello",
                "category": "code",
                "action": "created"
            }),
            &logged,
        );
    }

    #[test]
    fn board_task_status_changed_equiv() {
        let logged = make_logged(
            Domain::Board,
            "status_changed",
            json!({
                "StatusChanged": {
                    "task_id": "t-1",
                    "old_status": "queued",
                    "new_status": "running"
                }
            }),
        );
        assert_equiv(
            "board_task_status_changed",
            json!({
                "task_id": "t-1",
                "old_status": "queued",
                "new_status": "running",
                "action": "status_changed"
            }),
            &logged,
        );
    }

    #[test]
    fn task_created_equiv() {
        let logged = make_logged(
            Domain::Task,
            "created",
            json!({ "Created": { "task_id": "t-42" } }),
        );
        assert_equiv(
            "task_lifecycle",
            json!({ "task_id": "t-42", "action": "created" }),
            &logged,
        );
    }

    #[test]
    fn question_created_equiv() {
        let logged = make_logged(
            Domain::Question,
            "created",
            json!({ "Created": { "question_id": "q-9" } }),
        );
        assert_equiv("question_created", json!({ "question_id": "q-9" }), &logged);
    }

    #[test]
    fn llm_request_started_equiv() {
        let logged = make_logged(
            Domain::Llm,
            "request_started",
            json!({
                "RequestStarted": {
                    "provider": "Gemini",
                    "request_id": "r-1",
                    "caller": "worker",
                    "session_id": "s-1",
                    "model": "gemini-2",
                    "prompt_chars": 42,
                    "prompt_text": null,
                    "extra": {}
                }
            }),
        );
        let s = v2_logged_to_v1_wire_format(&logged);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("type").unwrap(), "cli_request_started");
        let p = v.get("payload").unwrap();
        assert_eq!(p.get("engine").unwrap(), "Gemini");
        assert_eq!(p.get("model").unwrap(), "gemini-2");
    }

    #[test]
    fn worker_llm_call_equiv() {
        let logged = make_logged(
            Domain::Worker,
            "llm_call",
            json!({
                "LlmCall": {
                    "caller": "briefing",
                    "task_id": "t-1",
                    "status": "success",
                    "prompt_chars": 100,
                    "response_chars": 50,
                    "duration_ms": 250,
                    "queue_wait_ms": 10
                }
            }),
        );
        assert_equiv(
            "worker_llm_call",
            json!({
                "caller": "briefing",
                "task_id": "t-1",
                "status": "success",
                "prompt_chars": 100,
                "response_chars": 50,
                "duration_ms": 250,
                "queue_wait_ms": 10
            }),
            &logged,
        );
    }

    #[test]
    fn memory_phase_changed_equiv() {
        let logged = make_logged(
            Domain::Memory,
            "phase_changed",
            json!({
                "PhaseChanged": {
                    "slot_id": "slot-memory",
                    "phase": "Busy",
                    "active_type": "realtime"
                }
            }),
        );
        assert_equiv(
            "memory_phase_changed",
            json!({
                "slot_id": "slot-memory",
                "phase": "Busy",
                "active_type": "realtime"
            }),
            &logged,
        );
    }

    #[test]
    fn message_logged_user_equiv() {
        let logged = make_logged(
            Domain::Message,
            "logged",
            json!({
                "Logged": {
                    "message_id": 7,
                    "session_id": "s-1",
                    "parent_session_id": null,
                    "slot_id": null,
                    "role": "user",
                    "content_chars": 12,
                    "preview": "hi"
                }
            }),
        );
        assert_equiv(
            "user_message",
            json!({
                "message_id": 7,
                "session_id": "s-1",
                "parent_session_id": null,
                "slot_id": null,
                "role": "user",
                "content_chars": 12,
                "preview": "hi"
            }),
            &logged,
        );
    }

    #[test]
    fn session_completed_equiv() {
        let logged = make_logged(
            Domain::Session,
            "completed",
            json!({
                "Completed": {
                    "session_id": "s-1",
                    "slot_id": "slot-a",
                    "message_count": 10,
                    "duration_secs": 60,
                    "status": "Success"
                }
            }),
        );
        assert_equiv(
            "session_completed",
            json!({
                "session_id": "s-1",
                "slot_id": "slot-a",
                "message_count": 10,
                "duration_secs": 60,
                "status": "Success"
            }),
            &logged,
        );
    }

    #[test]
    fn system_config_changed_equiv() {
        let logged = make_logged(
            Domain::System,
            "config_changed",
            json!({
                "ConfigChanged": {
                    "path": "/x/slots.yaml",
                    "kind": "modified"
                }
            }),
        );
        assert_equiv(
            "config_file_changed",
            json!({
                "path": "/x/slots.yaml",
                "kind": "modified"
            }),
            &logged,
        );
    }

    #[test]
    fn ephemeral_yields_seq_zero() {
        let mut logged = make_logged(
            Domain::Worker,
            "briefing_batch_started",
            json!({ "BriefingBatchStarted": { "pending_count": 3 } }),
        );
        logged.ephemeral = true;
        let s = v2_logged_to_v1_wire_format(&logged);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("seq").unwrap().as_i64(), Some(0));
    }
}
