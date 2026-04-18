//! Integration tests for the Phase 4 subscription runtime.
//!
//! Each test stands up real PostgreSQL (via `testcontainers`), real
//! [`LogWriter`] + [`Dispatcher`] + [`subscribe`], and validates the
//! two-phase tail-and-pull contract end-to-end.
//!
//! All tests are `#[ignore]`d by default — run with Docker available:
//!
//! ```bash
//! cargo test -p missiond-core --test event_subscription_integration -- --ignored
//! ```

#![cfg(feature = "postgres")]

use std::sync::Arc;
use std::time::Duration;

use missiond_core::event::blob_store::{BlobStore, LocalFileBlobStore};
use missiond_core::event::dispatcher::{DispatcherBuilder, NeverPaused, register_all_domains};
use missiond_core::event::events::board::BoardEvent;
use missiond_core::event::log::{spawn_log_writer, AppendOpts, Log};
use missiond_core::event::subscription::{
    subscribe, CursorStore, FailurePolicy, InMemoryDlq, PgCursorStore, PgDlqSink, StartFrom,
    SubscriptionOpts,
};

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

fn dummy_blob() -> Arc<dyn BlobStore> {
    let d = tempfile::tempdir().unwrap();
    Arc::new(LocalFileBlobStore::new(d.keep()))
}

/// Append 100 board events, subscribe from Earliest, and verify the
/// consumer sees the full series in order.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn subscribe_from_earliest_sees_all_persisted_events() {
    let (pool, _container) = setup_pg().await;
    let blob = dummy_blob();
    let log_handle = spawn_log_writer(pool.clone(), blob.clone());

    // 100 events.
    for i in 1..=100u32 {
        log_handle
            .append(
                BoardEvent::TaskCreated {
                    task_id: format!("t-{i}"),
                    title: "title".into(),
                    category: "cat".into(),
                },
                AppendOpts {
                    producer_id: "integration".into(),
                    ..AppendOpts::default()
                },
            )
            .await
            .expect("append ok");
    }

    let cursor_store = Arc::new(PgCursorStore::new(pool.clone()));
    let dlq = Arc::new(PgDlqSink::new(pool.clone()));
    let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
    let topic = dispatcher.topic::<BoardEvent>();

    // Spawn dispatcher tail so live source is populated if we lose race
    // with bootstrap.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let tail_source: Arc<dyn missiond_core::event::dispatcher::TailSource> =
        Arc::new(missiond_core::event::dispatcher::PgTailSource::new(
            pool.clone(),
            blob.clone(),
        ));
    let dispatcher_ref = dispatcher.clone();
    let tail_task = tokio::spawn(async move {
        dispatcher_ref
            .run(tail_source, blob.clone(), NeverPaused, shutdown_rx)
            .await
    });

    let mut opts = SubscriptionOpts::named("integ-cons");
    opts.start_from = StartFrom::Earliest;

    let mut sub = subscribe::<BoardEvent>(
        "sub-full-series",
        opts,
        Arc::new(log_handle.clone()),
        topic,
        cursor_store.clone(),
        dlq,
    )
    .await
    .expect("subscribe");

    let mut seqs = Vec::new();
    for _ in 0..100 {
        let ack = tokio::time::timeout(Duration::from_secs(10), sub.next())
            .await
            .expect("ack timeout")
            .expect("ack");
        seqs.push(ack.seq().0);
        ack.ack().await;
    }
    // Must have 100 unique, ascending seqs.
    assert_eq!(seqs.len(), 100);
    for w in seqs.windows(2) {
        assert!(w[0] < w[1], "seq not monotonic: {:?}", w);
    }

    // Persisted cursor should reflect the last ack.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let persisted = cursor_store.get("sub-full-series").await.unwrap().unwrap();
    assert!(persisted.last_acked_seq.0 > 0);

    shutdown_tx.send(true).ok();
    let _ = tail_task.await;
}

/// Crash-simulation: subscribe, consume half, drop subscription, re-subscribe
/// with same name; must resume after the persisted cursor.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn crash_recovery_resumes_from_persisted_cursor() {
    let (pool, _container) = setup_pg().await;
    let blob = dummy_blob();
    let log_handle = spawn_log_writer(pool.clone(), blob.clone());

    for i in 1..=50u32 {
        log_handle
            .append(
                BoardEvent::TaskCreated {
                    task_id: format!("t-{i}"),
                    title: "t".into(),
                    category: "c".into(),
                },
                AppendOpts {
                    producer_id: "integ".into(),
                    ..AppendOpts::default()
                },
            )
            .await
            .expect("append ok");
    }

    let cursor_store = Arc::new(PgCursorStore::new(pool.clone()));
    let dlq = Arc::new(PgDlqSink::new(pool.clone()));
    let dispatcher = register_all_domains(DispatcherBuilder::new()).build();

    let mut opts = SubscriptionOpts::named("integ");
    opts.start_from = StartFrom::Earliest;

    // First sub — drain 20 events, then drop.
    {
        let topic = dispatcher.topic::<BoardEvent>();
        let mut sub = subscribe::<BoardEvent>(
            "sub-crash",
            opts.clone(),
            Arc::new(log_handle.clone()),
            topic,
            cursor_store.clone(),
            dlq.clone(),
        )
        .await
        .expect("subscribe");
        for _ in 0..20 {
            let ack = tokio::time::timeout(Duration::from_secs(5), sub.next())
                .await
                .expect("timeout")
                .expect("ack");
            ack.ack().await;
        }
        tokio::time::sleep(Duration::from_millis(1500)).await; // flush
        drop(sub);
    }

    // Second sub — must see seq > 20.
    {
        let topic = dispatcher.topic::<BoardEvent>();
        let mut sub = subscribe::<BoardEvent>(
            "sub-crash",
            opts,
            Arc::new(log_handle.clone()),
            topic,
            cursor_store.clone(),
            dlq,
        )
        .await
        .expect("re-subscribe");
        let ack = tokio::time::timeout(Duration::from_secs(5), sub.next())
            .await
            .expect("timeout")
            .expect("ack");
        assert!(
            ack.seq().0 > 20,
            "expected resume past seq 20, got {:?}",
            ack.seq()
        );
        ack.ack().await;
    }
}

/// SkipToDLQ: nack an event and verify it lands in the DLQ table.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn nack_routes_to_dlq() {
    let (pool, _container) = setup_pg().await;
    let blob = dummy_blob();
    let log_handle = spawn_log_writer(pool.clone(), blob.clone());

    log_handle
        .append(
            BoardEvent::TaskCreated {
                task_id: "will-fail".into(),
                title: "t".into(),
                category: "c".into(),
            },
            AppendOpts {
                producer_id: "integ".into(),
                ..AppendOpts::default()
            },
        )
        .await
        .expect("append");

    let cursor_store = Arc::new(PgCursorStore::new(pool.clone()));
    let dlq = Arc::new(PgDlqSink::new(pool.clone()));
    let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
    let topic = dispatcher.topic::<BoardEvent>();

    let mut opts = SubscriptionOpts::named("integ-dlq");
    opts.start_from = StartFrom::Earliest;
    opts.failure_policy = FailurePolicy::SkipToDLQ;

    let mut sub = subscribe::<BoardEvent>(
        "sub-dlq",
        opts,
        Arc::new(log_handle.clone()),
        topic,
        cursor_store.clone(),
        dlq,
    )
    .await
    .expect("subscribe");

    let ack = tokio::time::timeout(Duration::from_secs(3), sub.next())
        .await
        .expect("timeout")
        .expect("ack");
    let failing_seq = ack.seq().0;
    ack.nack("boom").await;
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Check DLQ table.
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM dead_letter_queue WHERE subscription_name = $1")
            .bind("sub-dlq")
            .fetch_one(&pool)
            .await
            .expect("dlq count");
    assert_eq!(count, 1);

    let _ = failing_seq; // silence unused if we later drop the assertion
    let _ = InMemoryDlq::new(); // keep symbol imported
}
