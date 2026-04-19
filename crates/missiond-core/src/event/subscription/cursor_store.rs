//! Cursor store — persistence layer for subscription progress.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.3 cursor-store:
//!
//! > `subscription_name` PK (one consumer may hold multiple logical
//! > subscriptions);  `last_acked_seq` advanced on ack;  double-threshold
//! > flush (per batch OR 1s).
//!
//! This module defines:
//!
//!   * [`Cursor`]        — the in-memory representation of one row in
//!     `event_subscriptions`.
//!   * [`CursorStore`]   — the pluggable trait (PG in prod, in-memory mock
//!     in unit tests).
//!   * [`PgCursorStore`] — feature-gated PostgreSQL implementation.
//!   * [`InMemoryCursorStore`] — unit-test helper; also used by the
//!     no-persistence boot mode.
//!
//! The subscription runtime owns the flush scheduling (see `lifecycle.rs`);
//! the store stays mechanical.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::super::domain::Domain;
use super::super::log::Seq;
use super::options::FailurePolicy;

/// In-memory cursor state.
///
/// Frozen lisp §4.3 cursor-store.schema mirror. `failure_policy` is round-
/// tripped via serde so ops can reconfigure a subscription's recovery
/// behavior by editing JSONB in-place.
#[derive(Debug, Clone)]
pub struct Cursor {
    pub subscription_name: String,
    pub consumer_name: String,
    pub domain: Domain,
    pub last_acked_seq: Seq,
    pub last_seen_at: DateTime<Utc>,
    pub failure_policy: FailurePolicy,
}

/// Errors surfaced by [`CursorStore`].
#[derive(Debug, thiserror::Error)]
pub enum CursorError {
    #[error("db error: {0}")]
    Db(String),

    #[error("encode failure_policy: {0}")]
    Encode(#[from] serde_json::Error),

    #[error("cursor row missing domain mapping: {0}")]
    UnknownDomain(String),

    #[error("{0}")]
    Other(String),
}

#[cfg(feature = "postgres")]
impl From<sqlx::Error> for CursorError {
    fn from(e: sqlx::Error) -> Self {
        CursorError::Db(e.to_string())
    }
}

/// Abstraction over persistence so unit tests and in-process experiments
/// can use [`InMemoryCursorStore`] while production wires in [`PgCursorStore`].
#[async_trait]
pub trait CursorStore: Send + Sync {
    /// Load the cursor for `subscription_name`, or `None` if this is a brand-
    /// new subscription.
    async fn get(&self, subscription_name: &str) -> Result<Option<Cursor>, CursorError>;

    /// Insert or update the cursor row. The implementation MUST atomically
    /// replace the previous state — partial updates are not allowed.
    async fn upsert(&self, cursor: &Cursor) -> Result<(), CursorError>;

    /// Remove the cursor row. Primarily used by the orphan cleanup job and
    /// by tests.
    async fn delete(&self, subscription_name: &str) -> Result<(), CursorError>;
}

/// In-memory cursor store — fast, used for unit tests and for the
/// "no-persistence" boot mode when PG is unavailable.
#[derive(Clone, Default)]
pub struct InMemoryCursorStore {
    inner: Arc<Mutex<HashMap<String, Cursor>>>,
}

impl InMemoryCursorStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-only helper: snapshot the map for assertions.
    pub async fn snapshot(&self) -> HashMap<String, Cursor> {
        self.inner.lock().await.clone()
    }
}

#[async_trait]
impl CursorStore for InMemoryCursorStore {
    async fn get(&self, subscription_name: &str) -> Result<Option<Cursor>, CursorError> {
        Ok(self.inner.lock().await.get(subscription_name).cloned())
    }

    async fn upsert(&self, cursor: &Cursor) -> Result<(), CursorError> {
        self.inner
            .lock()
            .await
            .insert(cursor.subscription_name.clone(), cursor.clone());
        Ok(())
    }

    async fn delete(&self, subscription_name: &str) -> Result<(), CursorError> {
        self.inner.lock().await.remove(subscription_name);
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────
// PG backend
// ─────────────────────────────────────────────────────────────────────────

#[cfg(feature = "postgres")]
pub use pg::PgCursorStore;

#[cfg(feature = "postgres")]
mod pg {
    use super::super::super::log::reader::domain_from_str;
    use super::*;
    use sqlx::PgPool;

    /// PostgreSQL-backed [`CursorStore`] — reads/writes `event_subscriptions`.
    #[derive(Clone)]
    pub struct PgCursorStore {
        pool: PgPool,
    }

    impl PgCursorStore {
        pub fn new(pool: PgPool) -> Self {
            Self { pool }
        }
    }

    #[derive(sqlx::FromRow)]
    struct Row {
        subscription_name: String,
        consumer_name: String,
        domain: String,
        last_acked_seq: i64,
        last_seen_at: Option<DateTime<Utc>>,
        failure_policy: Option<serde_json::Value>,
    }

    #[async_trait]
    impl CursorStore for PgCursorStore {
        async fn get(
            &self,
            subscription_name: &str,
        ) -> Result<Option<Cursor>, CursorError> {
            let row: Option<Row> = sqlx::query_as::<_, Row>(
                r#"
                SELECT subscription_name, consumer_name, domain, last_acked_seq,
                       last_seen_at, failure_policy
                FROM event_subscriptions
                WHERE subscription_name = $1
                "#,
            )
            .bind(subscription_name)
            .fetch_optional(&self.pool)
            .await?;

            let Some(row) = row else { return Ok(None) };
            let domain = domain_from_str(&row.domain)
                .ok_or_else(|| CursorError::UnknownDomain(row.domain.clone()))?;
            let failure_policy = match row.failure_policy {
                Some(v) => serde_json::from_value(v)?,
                None => FailurePolicy::default(),
            };
            Ok(Some(Cursor {
                subscription_name: row.subscription_name,
                consumer_name: row.consumer_name,
                domain,
                last_acked_seq: Seq(row.last_acked_seq),
                last_seen_at: row.last_seen_at.unwrap_or_else(Utc::now),
                failure_policy,
            }))
        }

        async fn upsert(&self, cursor: &Cursor) -> Result<(), CursorError> {
            let policy_json = serde_json::to_value(cursor.failure_policy)?;
            sqlx::query(
                r#"
                INSERT INTO event_subscriptions
                    (subscription_name, consumer_name, domain,
                     last_acked_seq, last_seen_at, failure_policy)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (subscription_name) DO UPDATE SET
                    consumer_name  = EXCLUDED.consumer_name,
                    domain         = EXCLUDED.domain,
                    last_acked_seq = EXCLUDED.last_acked_seq,
                    last_seen_at   = EXCLUDED.last_seen_at,
                    failure_policy = EXCLUDED.failure_policy
                "#,
            )
            .bind(&cursor.subscription_name)
            .bind(&cursor.consumer_name)
            .bind(cursor.domain.as_str())
            .bind(cursor.last_acked_seq.0)
            .bind(cursor.last_seen_at)
            .bind(policy_json)
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        async fn delete(&self, subscription_name: &str) -> Result<(), CursorError> {
            sqlx::query("DELETE FROM event_subscriptions WHERE subscription_name = $1")
                .bind(subscription_name)
                .execute(&self.pool)
                .await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cursor(name: &str) -> Cursor {
        Cursor {
            subscription_name: name.into(),
            consumer_name: "tester".into(),
            domain: Domain::Board,
            last_acked_seq: Seq(7),
            last_seen_at: Utc::now(),
            failure_policy: FailurePolicy::default(),
        }
    }

    #[tokio::test]
    async fn in_memory_get_returns_none_for_missing() {
        let store = InMemoryCursorStore::new();
        let c = store.get("nope").await.unwrap();
        assert!(c.is_none());
    }

    #[tokio::test]
    async fn in_memory_upsert_then_get_returns_same_row() {
        let store = InMemoryCursorStore::new();
        let c = sample_cursor("sub-a");
        store.upsert(&c).await.unwrap();
        let loaded = store.get("sub-a").await.unwrap().unwrap();
        assert_eq!(loaded.subscription_name, "sub-a");
        assert_eq!(loaded.last_acked_seq, Seq(7));
    }

    #[tokio::test]
    async fn in_memory_upsert_replaces_state() {
        let store = InMemoryCursorStore::new();
        let mut c = sample_cursor("sub-b");
        store.upsert(&c).await.unwrap();
        c.last_acked_seq = Seq(100);
        store.upsert(&c).await.unwrap();
        let loaded = store.get("sub-b").await.unwrap().unwrap();
        assert_eq!(loaded.last_acked_seq, Seq(100));
    }

    #[tokio::test]
    async fn in_memory_delete_removes_row() {
        let store = InMemoryCursorStore::new();
        let c = sample_cursor("sub-c");
        store.upsert(&c).await.unwrap();
        store.delete("sub-c").await.unwrap();
        assert!(store.get("sub-c").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_multiple_subscriptions_coexist() {
        let store = InMemoryCursorStore::new();
        store.upsert(&sample_cursor("sub-1")).await.unwrap();
        store.upsert(&sample_cursor("sub-2")).await.unwrap();
        let snap = store.snapshot().await;
        assert_eq!(snap.len(), 2);
        assert!(snap.contains_key("sub-1"));
        assert!(snap.contains_key("sub-2"));
    }
}
