//! PostgreSQL integration tests using testcontainers.
//!
//! Requires Docker running. Skip with: cargo test --test pg_integration -- --ignored
//! Run with: cargo test --test pg_integration --features postgres -- --ignored

#![cfg(feature = "postgres")]

use missiond_core::db::pg::PgMissionStore;
use missiond_core::db::traits::*;
use missiond_core::types::*;

/// Spin up a PostgreSQL container and return PgMissionStore connected to it.
async fn setup_pg() -> (
    PgMissionStore,
    testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
) {
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default()
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let host_port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Failed to get PG port");

    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        host_port
    );

    let store = PgMissionStore::connect(&url)
        .await
        .expect("Failed to connect to PG");

    (store, container)
}

#[tokio::test]
#[ignore] // Requires Docker
async fn test_pg_vision_store() {
    let (store, _container) = setup_pg().await;

    // Test image description cache
    assert!(store
        .get_image_description("abc123")
        .await
        .unwrap()
        .is_none());
    store
        .save_image_description("abc123", "image/png", "A cat sitting on a desk")
        .await
        .unwrap();
    let desc = store.get_image_description("abc123").await.unwrap();
    assert_eq!(desc.unwrap(), "A cat sitting on a desk");

    // Upsert
    store
        .save_image_description("abc123", "image/png", "Updated description")
        .await
        .unwrap();
    let desc = store.get_image_description("abc123").await.unwrap();
    assert_eq!(desc.unwrap(), "Updated description");

    // Count
    assert_eq!(store.image_description_count().await.unwrap(), 1);

    // Translation
    assert!(!store.has_translation(1).await.unwrap());
    store
        .insert_translation(1, "翻译测试", "gemini", 100)
        .await
        .unwrap();
    assert!(store.has_translation(1).await.unwrap());
    let (trans, _) = store.get_translation(1).await.unwrap().unwrap();
    assert_eq!(trans, "翻译测试");
}

#[tokio::test]
#[ignore]
async fn test_pg_board_store() {
    let (store, _container) = setup_pg().await;

    // Create task
    let input = CreateBoardTaskInput {
        title: "Test task".into(),
        description: Some("Integration test task".into()),
        priority: Some("high".into()),
        category: Some("test".into()),
        ..Default::default()
    };
    let task = store.create_board_task(&input).await.unwrap();
    assert_eq!(task.title, "Test task");
    assert_eq!(task.status.as_str(), "open");

    // Get
    let fetched = store
        .get_board_task(task.id.as_str())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id.as_str(), task.id.as_str());

    // List
    let list = store.list_board_tasks(Some("open"), false).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let update = UpdateBoardTaskInput {
        status: Some("done".into()),
        ..Default::default()
    };
    let missing_evidence = store.update_board_task(task.id.as_str(), &update).await;
    assert!(missing_evidence.is_err());
    sqlx::query(
        "INSERT INTO shared_artifacts (hash, kind, media_type, bytes, size_bytes) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind("sha256:test")
    .bind("task-result-artifact")
    .bind("application/json")
    .bind(Vec::<u8>::new())
    .bind(0_i64)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_result_artifacts (id, artifact_hash, task_id, result_status, summary) VALUES ($1,$2,$3,'completed','ok')",
    )
    .bind("artifact-row-test")
    .bind("sha256:test")
    .bind(task.id.as_str())
    .execute(store.pool())
    .await
    .unwrap();
    let update = UpdateBoardTaskInput {
        status: Some("done".into()),
        artifact_hash: Some("sha256:test".into()),
        ..Default::default()
    };
    let updated = store
        .update_board_task(task.id.as_str(), &update)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status.as_str(), "done");

    // Delete
    let deleted = store.delete_board_task(task.id.as_str()).await.unwrap();
    assert_eq!(deleted, 1);
}

#[tokio::test]
#[ignore]
async fn test_pg_knowledge_store() {
    let (store, _container) = setup_pg().await;

    // Remember
    let input = KBRememberInput {
        category: "test".into(),
        key: "test-key".into(),
        summary: "Test knowledge entry".into(),
        detail: None,
        source: Some("test".into()),
        confidence: Some(0.9),
        project_id: None,
    };
    let result = store.kb_remember(&input).await.unwrap();
    assert_eq!(result.action, "created");

    // Get by key
    let entry = store.kb_get("test-key").await.unwrap().unwrap();
    assert_eq!(entry.summary, "Test knowledge entry");

    // Update same key → "updated"
    let input2 = KBRememberInput {
        category: "test".into(),
        key: "test-key".into(),
        summary: "Updated summary".into(),
        detail: None,
        source: Some("test".into()),
        confidence: Some(1.0),
        project_id: None,
    };
    let result2 = store.kb_remember(&input2).await.unwrap();
    assert_eq!(result2.action, "updated");

    // Search
    let results = store.kb_search("test", None).await.unwrap();
    assert!(!results.is_empty());

    // Stats
    let stats = store.kb_stats().await.unwrap();
    assert!(stats["total"].as_i64().unwrap() >= 1);

    // Forget
    assert!(store.kb_forget("test-key").await.unwrap());
    assert!(store.kb_get("test-key").await.unwrap().is_none());
}

#[tokio::test]
#[ignore]
async fn test_pg_slot_store() {
    let (store, _container) = setup_pg().await;

    // Slot session
    store
        .set_slot_session("slot-1", "session-abc")
        .await
        .unwrap();
    let session = store.get_slot_session("slot-1").await.unwrap().unwrap();
    assert_eq!(session, "session-abc");

    // Reverse lookup
    let slot = store
        .get_slot_for_session("session-abc")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(slot, "slot-1");

    // Delete
    store.delete_slot_session("slot-1").await.unwrap();
    assert!(store.get_slot_session("slot-1").await.unwrap().is_none());

    // Daemon state
    store.daemon_state_set("test_key", 42).await.unwrap();
    let val = store.daemon_state_get("test_key").await.unwrap().unwrap();
    assert_eq!(val, 42);
}

#[tokio::test]
#[ignore]
async fn test_pg_timeline_store_projection() {
    // v1.3.0 SSOT cutover: TimelineStore is read-only projection over event_log.
    // Writes go through the bus (`bus.publish_*`); we only verify reads here.
    let (store, _container) = setup_pg().await;

    // Read APIs must all succeed even on an empty event_log.
    let latest = store.timeline_latest_seq().await.unwrap();
    assert!(latest >= 0);

    let stats = store.query_timeline_stats(None, None).await.unwrap();
    assert!(stats.total_events >= 0);

    let _ = store
        .query_timeline_filtered(None, None, None, None, 10, 0)
        .await
        .unwrap();
    let _ = store.query_timeline_since(0, 10).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_pg_mission_store_init() {
    let (store, _container) = setup_pg().await;
    // init() should be a no-op (migrations already ran in connect())
    store.init().await.unwrap();
}
