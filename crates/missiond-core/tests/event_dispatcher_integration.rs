//! Integration tests for the Phase 3 topic dispatcher.
//!
//! Wire-up: real PG via `testcontainers` + `spawn_log_writer` (Phase 2) +
//! `Dispatcher::run` (Phase 3). Append N events, subscribe to each domain
//! topic, assert fan-out counts match.
//!
//! `#[ignore]`d by default; run with:
//!
//! ```bash
//! cargo test -p missiond-core --test event_dispatcher_integration -- --ignored
//! ```

#![cfg(feature = "postgres")]

use std::sync::Arc;
use std::time::Duration;

use missiond_core::event::blob_store::{BlobStore, LocalFileBlobStore};
use missiond_core::event::dispatcher::{
    register_all_domains, DispatcherBuilder, NeverPaused, PgTailSource, TailSource,
};
use missiond_core::event::events::{
    BoardEvent, IncidentEvent, LlmEvent, MemoryEvent, MessageEvent, ObservabilityEvent, Provider,
    QuestionEvent, SessionEndStatus, SessionEvent, SlotEvent, SystemEvent, TaskEvent, WorkerEvent,
};
use missiond_core::event::log::{spawn_log_writer, AppendOpts, Log};
use missiond_core::event::{Domain, DomainEvent};
use missiond_core::types::{IncidentSeverity, IncidentSource, MissionIncident};
use tokio::sync::watch;

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

#[tokio::test]
#[ignore]
async fn dispatcher_fans_out_to_all_12_domain_topics() {
    let (pool, _c) = setup_pg().await;
    let dir = tempfile::tempdir().unwrap();
    let blob: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(dir.path()));
    let writer = spawn_log_writer(pool.clone(), blob.clone());

    // Build dispatcher + register all current domains.
    let dispatcher = register_all_domains(DispatcherBuilder::new()).build();

    // Subscribe BEFORE starting the tail loop so we don't miss anything.
    let mut slot_rx = dispatcher.topic::<SlotEvent>().subscribe();
    let mut board_rx = dispatcher.topic::<BoardEvent>().subscribe();
    let mut task_rx = dispatcher.topic::<TaskEvent>().subscribe();
    let mut question_rx = dispatcher.topic::<QuestionEvent>().subscribe();
    let mut llm_rx = dispatcher.topic::<LlmEvent>().subscribe();
    let mut worker_rx = dispatcher.topic::<WorkerEvent>().subscribe();
    let mut memory_rx = dispatcher.topic::<MemoryEvent>().subscribe();
    let mut message_rx = dispatcher.topic::<MessageEvent>().subscribe();
    let mut session_rx = dispatcher.topic::<SessionEvent>().subscribe();
    let mut system_rx = dispatcher.topic::<SystemEvent>().subscribe();
    let mut obs_rx = dispatcher.topic::<ObservabilityEvent>().subscribe();
    let mut incident_rx = dispatcher.topic::<IncidentEvent>().subscribe();

    // Wire PgTailSource and kick off the dispatcher.
    let source: Arc<dyn TailSource> = Arc::new(PgTailSource::new(pool.clone(), blob.clone()));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let dispatcher_clone = dispatcher.clone();
    let task = tokio::spawn(async move {
        dispatcher_clone
            .run(source, blob, NeverPaused, shutdown_rx)
            .await
    });

    // Append exactly 1 event per domain (12 total).
    let opts = |pid: &str| AppendOpts {
        producer_id: pid.into(),
        ..Default::default()
    };

    writer
        .append(
            SlotEvent::BecameIdle {
                slot_id: "slot-int".into(),
            },
            opts("int-slot"),
        )
        .await
        .unwrap();
    writer
        .append(
            BoardEvent::TaskCreated {
                task_id: "board-1".into(),
                title: "t".into(),
                category: "c".into(),
            },
            opts("int-board"),
        )
        .await
        .unwrap();
    writer
        .append(
            TaskEvent::Created {
                task_id: "task-1".into(),
            },
            opts("int-task"),
        )
        .await
        .unwrap();
    writer
        .append(
            QuestionEvent::Created {
                question_id: "q-1".into(),
            },
            opts("int-q"),
        )
        .await
        .unwrap();
    writer
        .append(
            LlmEvent::RequestStarted {
                request_id: "r-1".into(),
                provider: Provider::Sonnet,
                caller: "int-test".into(),
                session_id: None,
                model: "sonnet-4.7".into(),
                prompt_chars: 10,
                prompt_text: None,
                extra: serde_json::Value::Null,
            },
            opts("int-llm"),
        )
        .await
        .unwrap();
    writer
        .append(
            WorkerEvent::LlmCall {
                caller: "w".into(),
                task_id: None,
                status: "ok".into(),
                prompt_chars: 10,
                response_chars: 10,
                duration_ms: 1,
                queue_wait_ms: 0,
            },
            opts("int-worker"),
        )
        .await
        .unwrap();
    writer
        .append(
            MemoryEvent::PhaseChanged {
                slot_id: "s".into(),
                phase: "Idle".into(),
                active_type: None,
            },
            opts("int-mem"),
        )
        .await
        .unwrap();
    writer
        .append(
            MessageEvent::Logged {
                message_id: 1,
                session_id: "s".into(),
                parent_session_id: None,
                slot_id: None,
                role: "user".into(),
                content_chars: 1,
                preview: "x".into(),
            },
            opts("int-msg"),
        )
        .await
        .unwrap();
    writer
        .append(
            SessionEvent::Completed {
                session_id: "s".into(),
                slot_id: None,
                message_count: 0,
                duration_secs: 0,
                status: SessionEndStatus::Success,
            },
            opts("int-sess"),
        )
        .await
        .unwrap();
    writer
        .append(
            SystemEvent::ConfigChanged {
                path: "x".into(),
                kind: "unknown".into(),
            },
            opts("int-sys"),
        )
        .await
        .unwrap();
    writer
        .append(
            ObservabilityEvent::BusMetric {
                name: "append_rate".into(),
                value: 1.0,
                labels: serde_json::Value::Null,
            },
            opts("int-obs"),
        )
        .await
        .unwrap();
    writer
        .append(
            IncidentEvent::Reported {
                incident: MissionIncident {
                    id: "inc-1".into(),
                    severity: IncidentSeverity::Warning,
                    source: IncidentSource::HealthCheck,
                    title: "t".into(),
                    description: "d".into(),
                    server_id: None,
                    raw_payload: serde_json::Value::Null,
                    created_at: "2026-04-19T00:00:00Z".into(),
                },
            },
            opts("int-inc"),
        )
        .await
        .unwrap();

    // Wait for each domain subscriber to receive exactly one event.
    async fn expect_one<T: Send + Sync + std::fmt::Debug + 'static>(
        rx: &mut tokio::sync::broadcast::Receiver<Arc<T>>,
        label: &str,
    ) {
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timeout waiting for {label}"))
            .unwrap_or_else(|e| panic!("recv error on {label}: {e:?}"));
        println!("{label} received: {:?}", got);
    }

    expect_one(&mut slot_rx, "SlotEvent").await;
    expect_one(&mut board_rx, "BoardEvent").await;
    expect_one(&mut task_rx, "TaskEvent").await;
    expect_one(&mut question_rx, "QuestionEvent").await;
    expect_one(&mut llm_rx, "LlmEvent").await;
    expect_one(&mut worker_rx, "WorkerEvent").await;
    expect_one(&mut memory_rx, "MemoryEvent").await;
    expect_one(&mut message_rx, "MessageEvent").await;
    expect_one(&mut session_rx, "SessionEvent").await;
    expect_one(&mut system_rx, "SystemEvent").await;
    expect_one(&mut obs_rx, "ObservabilityEvent").await;
    expect_one(&mut incident_rx, "IncidentEvent").await;

    // last_dispatched_seq should have advanced past 12.
    assert!(dispatcher.last_dispatched_seq().0 >= 12);

    // Shutdown cleanly.
    shutdown_tx.send(true).unwrap();
    let _ = task.await.unwrap();
}

#[tokio::test]
#[ignore]
async fn dispatcher_preserves_seq_order_across_one_domain() {
    let (pool, _c) = setup_pg().await;
    let dir = tempfile::tempdir().unwrap();
    let blob: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(dir.path()));
    let writer = spawn_log_writer(pool.clone(), blob.clone());

    let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
    let mut board_rx = dispatcher.topic::<BoardEvent>().subscribe();

    let source: Arc<dyn TailSource> = Arc::new(PgTailSource::new(pool.clone(), blob.clone()));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let dispatcher_clone = dispatcher.clone();
    let task = tokio::spawn(async move {
        dispatcher_clone
            .run(source, blob, NeverPaused, shutdown_rx)
            .await
    });

    for i in 0..10usize {
        writer
            .append(
                BoardEvent::TaskCreated {
                    task_id: format!("ordered-{i}"),
                    title: "t".into(),
                    category: "c".into(),
                },
                AppendOpts {
                    producer_id: format!("int-ord-{i}"),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    let mut seen = Vec::with_capacity(10);
    for _ in 0..10 {
        let ev = tokio::time::timeout(Duration::from_secs(5), board_rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        if let BoardEvent::TaskCreated { task_id, .. } = &*ev {
            seen.push(task_id.clone());
        }
    }
    for (i, got) in seen.iter().enumerate() {
        assert_eq!(got, &format!("ordered-{i}"));
    }

    shutdown_tx.send(true).unwrap();
    let _ = task.await.unwrap();
}

/// Domain enum coverage — compile-time sanity that any new domain is
/// visible through `Domain::ALL`. Not strictly a dispatcher test, but
/// guards against drift. Asserts inclusion of `Domain::Execution`
/// (added after the original 12-domain set) and a regression floor of
/// 13 entries; deliberately avoids hardcoding an exact count so that
/// future domain additions remain extensible without churn.
#[test]
fn domain_all_includes_execution() {
    assert!(
        Domain::ALL.contains(&Domain::Execution),
        "Domain::ALL must expose Domain::Execution"
    );
    assert!(
        Domain::ALL.len() >= 13,
        "Domain::ALL regressed below known floor: {}",
        Domain::ALL.len()
    );
    let _ = BoardEvent::domain(); // reference-only
}
