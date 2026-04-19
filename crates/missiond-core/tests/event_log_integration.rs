//! Integration tests for the Phase 2 event-log storage layer.
//!
//! These tests spin up a real PostgreSQL instance via `testcontainers` and
//! exercise the append → read path end-to-end. They are `#[ignore]`d by
//! default so CI environments without Docker don't fail; run them with
//!
//! ```bash
//! cargo test -p missiond-core --test event_log_integration -- --ignored
//! ```
//!
//! The whole file is behind `#[cfg(feature = "postgres")]`, so if that
//! feature isn't on these tests are simply absent.

#![cfg(feature = "postgres")]

use std::sync::Arc;

use missiond_core::event::blob_store::{BlobStore, LocalFileBlobStore};
use missiond_core::event::events::board::BoardEvent;
use missiond_core::event::events::message::MessageEvent;
use missiond_core::event::log::{
    AppendAck, AppendError, AppendOpts, Log, Seq, spawn_log_writer,
};
use missiond_core::event::{Domain, DomainEvent};
use uuid::Uuid;

/// Bring up a throwaway PostgreSQL, apply migrations, and hand back a pool.
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

    // Use the library helper to apply migrations. PgMissionStore::connect
    // runs `sqlx::migrate!()` internally, which is exactly what we need.
    let store = missiond_core::db::pg::PgMissionStore::connect(&url)
        .await
        .expect("connect + migrate");
    (store.pool().clone(), container)
}

#[tokio::test]
#[ignore]
async fn append_then_read_from_round_trip() {
    let (pool, _c) = setup_pg().await;
    let dir = tempfile::tempdir().unwrap();
    let blob: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(dir.path()));
    let writer = spawn_log_writer(pool.clone(), blob);

    let opts = AppendOpts {
        producer_id: "integration-test".into(),
        ..Default::default()
    };
    let ev = BoardEvent::TaskCreated {
        task_id: "t-1".into(),
        title: "hello".into(),
        category: "test".into(),
    };
    let ack = writer.append(ev, opts).await.expect("append ok");
    let seq = match ack {
        AppendAck::Committed { seq, durable } => {
            assert!(durable);
            seq
        }
        other => panic!("expected Committed, got {other:?}"),
    };

    let events = writer
        .read_from(Domain::Board, Seq(0), 10)
        .await
        .expect("read_from");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, seq);
    assert_eq!(events[0].kind, "task_created");
    assert_eq!(events[0].domain, Domain::Board);
    assert_eq!(events[0].producer_id, "integration-test");

    // head_seq reflects the committed row.
    let head = writer.head_seq().await.unwrap();
    assert!(head >= seq);
}

#[tokio::test]
#[ignore]
async fn dedupe_key_returns_existing_seq() {
    let (pool, _c) = setup_pg().await;
    let dir = tempfile::tempdir().unwrap();
    let blob: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(dir.path()));
    let writer = spawn_log_writer(pool, blob);

    let key = Uuid::new_v4();
    let opts = AppendOpts {
        producer_id: "dedupe-producer".into(),
        dedupe_key: Some(key),
        ..Default::default()
    };
    let ev = BoardEvent::TaskCreated {
        task_id: "t-dup".into(),
        title: "x".into(),
        category: "x".into(),
    };

    let first = writer.append(ev.clone(), opts.clone()).await.unwrap();
    let first_seq = first.seq();
    let second = writer.append(ev, opts).await.unwrap();
    match second {
        AppendAck::AlreadyExists { seq } => assert_eq!(seq, first_seq),
        other => panic!("expected AlreadyExists, got {other:?}"),
    }
}

#[tokio::test]
#[ignore]
async fn claim_check_stores_large_payload_in_blob_and_fetches_back() {
    let (pool, _c) = setup_pg().await;
    let dir = tempfile::tempdir().unwrap();
    let blob: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(dir.path()));
    let writer = spawn_log_writer(pool.clone(), blob);

    let big = MessageEvent::Logged {
        message_id: 777,
        session_id: "sess".into(),
        parent_session_id: None,
        slot_id: None,
        role: "assistant".into(),
        content_chars: 20_000,
        preview: "y".repeat(20_000),
    };

    let ack = writer
        .append(
            big.clone(),
            AppendOpts {
                producer_id: "claim-check-test".into(),
                ..Default::default()
            },
        )
        .await
        .expect("append");
    let seq = match ack {
        AppendAck::Committed { seq, .. } => seq,
        other => panic!("expected Committed, got {other:?}"),
    };

    // The row should come back with the same kind and a payload that
    // decodes to the original MessageEvent::Logged.
    let events = writer.read_from(Domain::Message, Seq(0), 10).await.unwrap();
    assert_eq!(events.len(), 1);
    let ev = &events[0];
    assert_eq!(ev.seq, seq);
    assert_eq!(ev.kind, big.kind());
    let back: MessageEvent = serde_json::from_value(ev.payload.clone()).unwrap();
    // Only check the lengths, not the full 40 KB string — both matching by
    // length implies equality since our test payload is a single char.
    if let (
        MessageEvent::Logged {
            preview: orig_preview,
            ..
        },
        MessageEvent::Logged {
            preview: back_preview,
            ..
        },
    ) = (big, back)
    {
        assert_eq!(orig_preview.len(), back_preview.len());
        assert_eq!(orig_preview, back_preview);
    } else {
        panic!("unexpected variant");
    }
}

#[tokio::test]
#[ignore]
async fn ephemeral_append_does_not_persist() {
    let (pool, _c) = setup_pg().await;
    let dir = tempfile::tempdir().unwrap();
    let blob: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(dir.path()));
    let writer = spawn_log_writer(pool, blob);

    let ack = writer
        .append(
            BoardEvent::TaskCreated {
                task_id: "t-volatile".into(),
                title: "x".into(),
                category: "x".into(),
            },
            AppendOpts {
                producer_id: "ephemeral-producer".into(),
                ephemeral: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(matches!(ack, AppendAck::Volatile { .. }));

    let rows = writer.read_from(Domain::Board, Seq(0), 10).await.unwrap();
    assert!(
        rows.is_empty(),
        "ephemeral append must not write to event_log"
    );
}

#[tokio::test]
#[ignore]
async fn missing_producer_id_is_rejected() {
    let (pool, _c) = setup_pg().await;
    let dir = tempfile::tempdir().unwrap();
    let blob: Arc<dyn BlobStore> = Arc::new(LocalFileBlobStore::new(dir.path()));
    let writer = spawn_log_writer(pool, blob);

    let err = writer
        .append(
            BoardEvent::TaskCreated {
                task_id: "t".into(),
                title: "x".into(),
                category: "x".into(),
            },
            AppendOpts::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppendError::Other(_)),
        "empty producer_id must fail fast, got {err:?}"
    );
}

#[tokio::test]
#[ignore]
async fn retention_cleanup_removes_old_ephemeral_rows() {
    use missiond_core::event::log::cleanup_once;

    let (pool, _c) = setup_pg().await;

    // Insert a stale ephemeral row directly so we can control the age.
    sqlx::query(
        r#"
        INSERT INTO event_log (seq, domain, kind, payload_inline, producer_id, ts, ephemeral)
        VALUES (DEFAULT, 'observability', 'old_metric', '{}'::jsonb, 'retention-test',
                now() - interval '10 days', true)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert stale ephemeral row");

    // Fresh persistent row that must survive.
    sqlx::query(
        r#"
        INSERT INTO event_log (seq, domain, kind, payload_inline, producer_id, ts, ephemeral)
        VALUES (DEFAULT, 'board', 'task_created', '{}'::jsonb, 'retention-test',
                now(), false)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert fresh persistent row");

    let report = cleanup_once(&pool).await.expect("cleanup");
    assert!(report.ephemeral_events_deleted >= 1);
    // The fresh persistent row was well inside 30 days so it must survive.
    let remaining: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM event_log WHERE producer_id = 'retention-test'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining.0, 1);
}
