//! TTL retention job.
//!
//! Frozen lisp §4.2.b retention:
//!   * persistent events: 30 day window
//!   * ephemeral events : 3 day window
//!   * blob_storage     : rows whose `ttl_expires` has passed
//!
//! Called once per day from the Phase 8 daemon main loop. Phase 2 ships the
//! implementation only; wiring happens later.

#![cfg(feature = "postgres")]

use sqlx::PgPool;

/// Counts of rows removed by a single [`cleanup_once`] run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub ephemeral_events_deleted: u64,
    pub persistent_events_deleted: u64,
    pub blobs_deleted: u64,
}

/// Run one pass of retention cleanup against the provided pool.
///
/// Three independent DELETE statements — each is idempotent so a stalled /
/// crashed previous run does not corrupt state. Rows that failed to delete
/// will simply be retried the next day.
pub async fn cleanup_once(pool: &PgPool) -> Result<CleanupReport, sqlx::Error> {
    let ephemeral = sqlx::query(
        "DELETE FROM event_log WHERE ephemeral = true AND ts < now() - interval '3 days'",
    )
    .execute(pool)
    .await?
    .rows_affected();

    let persistent = sqlx::query(
        "DELETE FROM event_log WHERE ephemeral = false AND ts < now() - interval '30 days'",
    )
    .execute(pool)
    .await?
    .rows_affected();

    let blobs = sqlx::query(
        "DELETE FROM blob_storage WHERE ttl_expires IS NOT NULL AND ttl_expires < now()",
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(CleanupReport {
        ephemeral_events_deleted: ephemeral,
        persistent_events_deleted: persistent,
        blobs_deleted: blobs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_report_default_is_zero() {
        let r = CleanupReport::default();
        assert_eq!(r.ephemeral_events_deleted, 0);
        assert_eq!(r.persistent_events_deleted, 0);
        assert_eq!(r.blobs_deleted, 0);
    }
}
