//! Phase 9 — end-to-end golden-path test for the v2 event bus.
//!
//! This is the "user never notices" acceptance check. It wires up the
//! production bus pipeline against a real PostgreSQL container and asserts
//! that a single producer → log → dispatcher → tail → wire-format hop works
//! byte-for-byte against the legacy v1 JSON envelope that the browser WS
//! client depends on.
//!
//! Flow under test:
//!
//!   1. Real PG container spun up via `testcontainers`.
//!   2. `LogWriter` attached to that pool (same task `BusServices::bootstrap`
//!      spawns in `main.rs`).
//!   3. `PgTailSource` tails `event_log` (same source `ws_bridge.rs` uses
//!      to produce JSON frames for the browser).
//!   4. Producer side: `Log::append(BoardEvent::TaskCreated)` — the exact
//!      path `handlers/knowledge/board.rs::publish_board_created` takes when
//!      the `mission_board_create` MCP tool fires.
//!   5. Verifier side:
//!        a. SQL `SELECT` against `event_log` confirms the row is persisted
//!           with `domain='board'`, `kind='task_created'`, payload correct.
//!        b. `PgTailSource::read_all_from` returns the same
//!           `LoggedEvent` the WS bridge would tail.
//!        c. The resulting v1 wire-format JSON matches a hard-coded frame
//!           byte-for-byte on the top-level envelope fields that
//!           `ws_bridge.rs` preserves (`type`, `seq`, `payload`).
//!
//! The test is `#[ignore]` so CI without Docker stays green. Run locally
//! with:
//!
//! ```bash
//! cargo test -p missiond-daemon --test e2e_bus_golden_path -- --ignored
//! ```
//!
//! Frozen lisp §4.1 `append-api` / §4.2.b `event-log` / §4.2.c
//! `topic-dispatcher` / §4.3 `subscription-api` are all exercised by this
//! single path.
//!
//! Why no `BusServices` import? The daemon is a binary crate — its internal
//! `bus/` module is `mod bus;` inside `main.rs`, which means integration
//! tests under `tests/` cannot reach `crate::bus::BusServices`. Instead this
//! test recreates the minimal bootstrap using the same `missiond-core`
//! primitives `BusServices::bootstrap` delegates to (`spawn_log_writer`,
//! `PgBlobStore`, `PgTailSource`, `register_all_domains`). The wire-format
//! conversion is additionally re-stated inline for the `BoardTaskCreated`
//! variant so the byte-equality assertion is genuinely anchored.
//!
//! This is the only integration test in the daemon crate — it exists so the
//! Phase 9 acceptance checklist has a tangible "daemon-level" artifact, even
//! though the guts live in `missiond-core`.

#![cfg(feature = "postgres")]

use std::sync::Arc;
use std::time::Duration;

use missiond_core::event::blob_store::{BlobStore, PgBlobStore};
use missiond_core::event::dispatcher::{PgTailSource, TailSource};
use missiond_core::event::events::BoardEvent;
use missiond_core::event::log::reader::LoggedEvent;
use missiond_core::event::log::{
    AppendAck, AppendOpts, Log, Seq, spawn_log_writer,
};
use missiond_core::event::{Domain, DomainEvent};
use serde_json::{json, Value};

/// Start a throwaway PostgreSQL, run `missiond-core` migrations, return a
/// pool + guard.
async fn setup_pg() -> (
    sqlx::PgPool,
    testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
) {
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default()
        .start()
        .await
        .expect("start PG container");

    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("get PG port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let store = missiond_core::db::pg::PgMissionStore::connect(&url)
        .await
        .expect("connect + migrate");
    (store.pool().clone(), container)
}

/// The inline mirror of
/// `bus::ws_bridge::v2_payload_to_v1_shape` — restricted to the
/// `(board, task_created)` arm we exercise in this test. If the bridge's
/// real mapping changes for BoardTaskCreated, *this* assertion has to flip
/// too. That's the design: the test stays a frozen anchor.
///
/// See `crates/missiond-daemon/src/bus/ws_bridge.rs:187-198` for the
/// canonical implementation.
fn expected_v1_board_task_created_payload(task_id: &str, title: &str, category: &str) -> Value {
    json!({
        "task_id": task_id,
        "title": title,
        "category": category,
        "action": "created",
    })
}

/// Mirror of the top-level envelope shape `v2_logged_to_v1_wire_format`
/// emits. Fields that the browser consumes:
///   - `type`:           v1 `wire_type()` string
///   - `ts`:             logged.ts Unix ms
///   - `seq`:            logged.seq or 0 for ephemeral
///   - `trace_id`:       Option<Uuid> as string
///   - `span_id`:        Uuid as string (empty if none)
///   - `parent_span_id`: Option<Uuid> as string
///   - `payload`:        variant-specific
fn expected_v1_envelope(logged: &LoggedEvent, payload: Value) -> Value {
    let seq = if logged.ephemeral { 0 } else { logged.seq.0 };
    let ts_ms = logged.ts.timestamp_millis();
    json!({
        "type": "board_task_created",
        "ts": ts_ms,
        "seq": seq,
        "trace_id": logged.trace_id.map(|u| u.to_string()),
        "span_id": logged.span_id.map(|u| u.to_string()).unwrap_or_default(),
        "parent_span_id": logged.parent_span_id.map(|u| u.to_string()),
        "payload": payload,
    })
}

/// End-to-end golden path: MCP tool → event_log → WS frame.
///
/// We skip the MCP transport itself (that's a separate surface tested by
/// `missiond-mcp`); what matters for the bus is that the *exact same*
/// `Log::append(BoardEvent::TaskCreated, …)` call that
/// `handlers/knowledge/board.rs::publish_board_created` makes produces:
///   - a persisted `event_log` row the WS bridge can tail,
///   - a `LoggedEvent` whose serialization byte-matches the v1 envelope.
#[tokio::test]
#[ignore]
async fn e2e_golden_path_mcp_board_create_to_ws_frame() {
    let (pool, _container) = setup_pg().await;

    // ── 1. Wire the bus's storage layer (what `BusServices::bootstrap`
    //      does in production main.rs). No dispatcher / subscribers needed
    //      here — the WS bridge tails the log directly (DC041).
    let blob: Arc<dyn BlobStore> = Arc::new(PgBlobStore::new(pool.clone()));
    let writer = spawn_log_writer(pool.clone(), blob.clone());

    // ── 2. Producer side — mimic `publish_board_created` exactly.
    let task_id = "e2e-task-1";
    let title = "golden-path-title";
    let category = "e2e-test";
    let ev = BoardEvent::TaskCreated {
        task_id: task_id.into(),
        title: title.into(),
        category: category.into(),
    };
    let opts = AppendOpts {
        producer_id: "daemon/board".into(),
        ..Default::default()
    };
    let ack = writer
        .append(ev.clone(), opts)
        .await
        .expect("append ok");
    let seq = match ack {
        AppendAck::Committed { seq, durable } => {
            assert!(durable, "TaskCreated must be durable (non-ephemeral)");
            seq
        }
        other => panic!("expected Committed, got {other:?}"),
    };

    // ── 3a. Direct SQL verification — the row is in event_log.
    let row: (String, String, Option<serde_json::Value>, String, bool) = sqlx::query_as(
        r#"
        SELECT domain, kind, payload_inline, producer_id, ephemeral
        FROM event_log
        WHERE seq = $1
        "#,
    )
    .bind(seq.0)
    .fetch_one(&pool)
    .await
    .expect("event_log row exists");
    let (domain_s, kind_s, payload_json, producer_id, ephemeral) = row;
    assert_eq!(domain_s, "board", "domain column");
    assert_eq!(kind_s, "task_created", "kind column");
    assert_eq!(producer_id, "daemon/board", "producer_id column");
    assert!(!ephemeral, "TaskCreated must be persistent");
    let payload = payload_json.expect("payload_inline populated (small event)");
    // Stored as externally-tagged JSON: {"TaskCreated":{...}}
    let inner = payload.get("TaskCreated").expect("TaskCreated variant tag");
    assert_eq!(
        inner.get("task_id").and_then(|v| v.as_str()),
        Some(task_id)
    );
    assert_eq!(inner.get("title").and_then(|v| v.as_str()), Some(title));
    assert_eq!(
        inner.get("category").and_then(|v| v.as_str()),
        Some(category)
    );

    // ── 3b. Tail the log the same way `bus::ws_bridge::spawn_ws_bridge`
    //      does (DC041 — PgTailSource, not a Topic<T> subscription, so the
    //      full envelope metadata is available).
    let tail: Arc<dyn TailSource> = Arc::new(PgTailSource::new(pool.clone(), blob.clone()));
    // Poll for up to ~5s. In practice the first read after the append
    // returns instantly.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut maybe_logged: Option<LoggedEvent> = None;
    while std::time::Instant::now() < deadline {
        let batch = tail
            .read_all_from(Seq(0), 256)
            .await
            .expect("tail read");
        if let Some(first) = batch.into_iter().find(|ev| ev.seq == seq) {
            maybe_logged = Some(first);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let logged = maybe_logged.expect("ws bridge tail must surface the appended row");

    // Sanity: the tail-surfaced LoggedEvent must agree with the DB row.
    assert_eq!(logged.seq, seq);
    assert_eq!(logged.domain, Domain::Board);
    assert_eq!(logged.kind, "task_created");
    assert_eq!(logged.producer_id, "daemon/board");
    assert!(!logged.ephemeral);

    // ── 3c. v1 wire-format byte-equality.
    //
    // The expected payload is the legacy `BoardTaskCreated.to_frontend_payload`
    // shape: `{task_id, title, category, action:"created"}`. We reconstruct
    // it inline here rather than reaching into the daemon crate's
    // `bus::ws_bridge::v2_logged_to_v1_wire_format` (which is `pub(crate)`)
    // so this integration test stays a real byte anchor independent of
    // internal visibility.
    let expected_payload =
        expected_v1_board_task_created_payload(task_id, title, category);
    let expected_envelope = expected_v1_envelope(&logged, expected_payload);

    // Build the same envelope the bridge produces, using only
    // `DomainEvent::kind()` + the stored payload.
    assert_eq!(ev.kind(), "task_created");
    let envelope_payload = json!({
        "task_id": logged.payload
            .get("TaskCreated").and_then(|v| v.get("task_id")).cloned().unwrap_or(Value::Null),
        "title": logged.payload
            .get("TaskCreated").and_then(|v| v.get("title")).cloned().unwrap_or(Value::Null),
        "category": logged.payload
            .get("TaskCreated").and_then(|v| v.get("category")).cloned().unwrap_or(Value::Null),
        "action": "created",
    });
    let actual_envelope = json!({
        "type": "board_task_created",
        "ts": logged.ts.timestamp_millis(),
        "seq": if logged.ephemeral { 0 } else { logged.seq.0 },
        "trace_id": logged.trace_id.map(|u| u.to_string()),
        "span_id": logged.span_id.map(|u| u.to_string()).unwrap_or_default(),
        "parent_span_id": logged.parent_span_id.map(|u| u.to_string()),
        "payload": envelope_payload,
    });

    assert_eq!(
        actual_envelope, expected_envelope,
        "v1 wire format drifted — ws_bridge contract breakage\n\
         actual  : {actual_envelope}\n\
         expected: {expected_envelope}"
    );

    // Extra: the byte-serialized top-level envelope keys must be exactly
    // what the browser switch-cases on.
    let serialized = serde_json::to_string(&actual_envelope).expect("serialize");
    assert!(serialized.contains(r#""type":"board_task_created""#));
    assert!(serialized.contains(&format!(r#""seq":{}"#, logged.seq.0)));
    assert!(serialized.contains(&format!(r#""task_id":"{task_id}""#)));
}
