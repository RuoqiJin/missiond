//! PostgreSQL integration tests using testcontainers.
//!
//! Requires Docker running. Skip with: cargo test --test pg_integration -- --ignored
//! Run with: cargo test --test pg_integration --features postgres -- --ignored

#![cfg(feature = "postgres")]

use missiond_core::db::error::DbError;
use missiond_core::db::pg::PgMissionStore;
use missiond_core::db::traits::*;
use missiond_core::types::*;
use serde_json::json;
use sqlx::Row;

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
async fn test_pg_board_claim_uses_work_leases_authority() {
    let (store, _container) = setup_pg().await;
    let input = CreateBoardTaskInput {
        title: "Lease-backed Board claim".into(),
        description: Some("Board projection over work_leases".into()),
        priority: Some("medium".into()),
        category: Some("test".into()),
        ..Default::default()
    };
    let task = store.create_board_task(&input).await.unwrap();

    let claimed = store
        .claim_board_task(task.id.as_str(), "slot-worker-1", "pty_slot")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.status.as_str(), "running");
    assert_eq!(claimed.claim_executor_id.as_deref(), Some("slot-worker-1"));

    let lease_row = sqlx::query(
        r#"
        SELECT holder_id, holder_kind, status
        FROM work_leases
        WHERE task_id = $1 AND scope_kind = 'board_task' AND scope_key = $1
        "#,
    )
    .bind(task.id.as_str())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        lease_row.try_get::<String, _>("holder_id").unwrap(),
        "slot-worker-1"
    );
    assert_eq!(
        lease_row.try_get::<String, _>("holder_kind").unwrap(),
        "pty_slot"
    );
    assert_eq!(lease_row.try_get::<String, _>("status").unwrap(), "active");

    let conflict = store
        .claim_board_task(task.id.as_str(), "slot-worker-2", "pty_slot")
        .await
        .expect_err("second active Board claim must fail at work_leases");
    match conflict {
        DbError::ClaimConflict {
            scope_kind,
            scope_key,
            holder,
            lease_expires_at,
        } => {
            assert_eq!(scope_kind, "board_task");
            assert_eq!(scope_key, task.id.as_str());
            assert_eq!(holder.as_deref(), Some("slot-worker-1"));
            assert!(lease_expires_at.is_some());
        }
        other => panic!("expected CLAIM_CONFLICT, got {other:?}"),
    }

    let released = store
        .release_board_claims_by_executor("slot-worker-1")
        .await
        .unwrap();
    assert_eq!(released, 1);

    let released_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM work_leases WHERE task_id = $1 AND holder_id = 'slot-worker-1'",
    )
    .bind(task.id.as_str())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(released_status, "released");

    let reclaimed = store
        .claim_board_task(task.id.as_str(), "slot-worker-2", "pty_slot")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reclaimed.claim_executor_id.as_deref(),
        Some("slot-worker-2")
    );
    let active_holder = sqlx::query_scalar::<_, String>(
        r#"
        SELECT holder_id
        FROM work_leases
        WHERE task_id = $1 AND scope_kind = 'board_task' AND status = 'active'
        "#,
    )
    .bind(task.id.as_str())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(active_holder, "slot-worker-2");
}

#[tokio::test]
#[ignore]
async fn test_pg_control_plane_kernel_schema_contracts() {
    let (store, _container) = setup_pg().await;
    let pool = store.pool();
    let task_id = "task-cpk-schema-test";

    for (id, subject_kind, subject_id, operation) in [
        ("grant-worker-spawn", "worker", "slot-cpk", "spawn"),
        ("grant-system-settle", "system", "missiond-system", "settle"),
        (
            "grant-operator-delegate",
            "operator",
            "operator-local",
            "delegate",
        ),
        ("grant-daemon-claim", "daemon", "missiond-daemon", "claim"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO capability_grants
              (id, subject_kind, subject_id, operation, scope_kind, scope_key,
               task_id, issuer, details)
            VALUES ($1, $2, $3, $4, 'task', $5, $5, 'integration-test', $6)
            "#,
        )
        .bind(id)
        .bind(subject_kind)
        .bind(subject_id)
        .bind(operation)
        .bind(task_id)
        .bind(json!({"test": "control-plane-kernel-schema"}))
        .execute(pool)
        .await
        .unwrap();
    }

    let exact_spawn_grant = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM capability_grants
        WHERE id = 'grant-worker-spawn'
          AND subject_kind = 'worker'
          AND subject_id = 'slot-cpk'
          AND operation = 'spawn'
          AND scope_kind = 'task'
          AND scope_key = $1
          AND task_id = $1
          AND status = 'active'
          AND consumed_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .unwrap();
    assert_eq!(exact_spawn_grant.as_deref(), Some("grant-worker-spawn"));

    let wrong_subject_grant = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM capability_grants
        WHERE id = 'grant-worker-spawn'
          AND subject_kind = 'worker'
          AND subject_id = 'other-slot'
          AND operation = 'spawn'
          AND scope_kind = 'task'
          AND scope_key = $1
          AND task_id = $1
          AND status = 'active'
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .unwrap();
    assert!(wrong_subject_grant.is_none());

    sqlx::query(
        r#"
        INSERT INTO model_route_outcomes
          (id, request_id, task_id, project_id, task_class, provider, model,
           route, decision, outcome, latency_ms, prompt_tokens,
           completion_tokens, total_tokens, cost_usd, artifact_hash, job_state,
           status)
        VALUES
          ('route-outcome-cpk', 'req-cpk', $1, 'missiond', 'code',
           'codex', 'gpt-5.5', 'compiled-policy', $2, $3, 1200, 10, 5,
           15, 0.00012345, 'sha256:cpk', 'completed', 'succeeded')
        "#,
    )
    .bind(task_id)
    .bind(json!({"source": "compiled_policy"}))
    .bind(json!({"result": "completed", "verified": true}))
    .execute(pool)
    .await
    .unwrap();

    let route_row = sqlx::query(
        r#"
        SELECT prompt_tokens, completion_tokens, cost_usd::float8 AS cost_usd,
               status, decision, outcome
        FROM model_route_outcomes
        WHERE id = 'route-outcome-cpk'
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(route_row.try_get::<i64, _>("prompt_tokens").unwrap(), 10);
    assert_eq!(route_row.try_get::<i64, _>("completion_tokens").unwrap(), 5);
    assert_eq!(
        route_row.try_get::<String, _>("status").unwrap(),
        "succeeded"
    );
    assert!(route_row
        .try_get::<f64, _>("cost_usd")
        .unwrap()
        .is_sign_positive());
    assert_eq!(
        route_row
            .try_get::<serde_json::Value, _>("outcome")
            .unwrap()["result"],
        "completed"
    );

    sqlx::query(
        r#"
        INSERT INTO task_contracts
          (id, task_id, task_contract_id, read_scope, write_scope,
           must_not_touch, sandbox_profile)
        VALUES
          ('contract-cpk', $1, 'task-contract-cpk', $2, $3, $4, 'workspace-write')
        "#,
    )
    .bind(task_id)
    .bind(json!(["."]))
    .bind(json!(["src/**"]))
    .bind(json!(["secrets/**"]))
    .execute(pool)
    .await
    .unwrap();
    let write_scope = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT write_scope FROM task_contracts WHERE task_id = $1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(write_scope, json!(["src/**"]));

    sqlx::query(
        r#"
        INSERT INTO work_leases
          (id, task_id, holder_id, holder_kind, scope_kind, scope_key,
           lease_expires_at, metadata)
        VALUES
          ('lease-cpk-1', $1, 'holder-1', 'worker', 'path', 'src/lib.rs',
           now() + interval '10 minutes', $2)
        "#,
    )
    .bind(task_id)
    .bind(json!({"mirror": "shared_claims_projection"}))
    .execute(pool)
    .await
    .unwrap();

    let duplicate_active = sqlx::query(
        r#"
        INSERT INTO work_leases
          (id, task_id, holder_id, holder_kind, scope_kind, scope_key,
           lease_expires_at)
        VALUES
          ('lease-cpk-duplicate', $1, 'holder-2', 'worker', 'path', 'src/lib.rs',
           now() + interval '10 minutes')
        "#,
    )
    .bind(task_id)
    .execute(pool)
    .await;
    assert!(duplicate_active.is_err());

    sqlx::query(
        "UPDATE work_leases SET status = 'released', released_at = now() WHERE id = 'lease-cpk-1'",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO work_leases
          (id, task_id, holder_id, holder_kind, scope_kind, scope_key,
           lease_expires_at)
        VALUES
          ('lease-cpk-2', $1, 'holder-2', 'worker', 'path', 'src/lib.rs',
           now() + interval '10 minutes')
        "#,
    )
    .bind(task_id)
    .execute(pool)
    .await
    .unwrap();

    let active_holder = sqlx::query_scalar::<_, String>(
        r#"
        SELECT holder_id
        FROM work_leases
        WHERE scope_kind = 'path' AND scope_key = 'src/lib.rs' AND status = 'active'
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(active_holder, "holder-2");
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
