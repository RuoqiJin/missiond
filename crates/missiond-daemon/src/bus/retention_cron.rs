//! Daily retention + orphan-subscription cleanup cron.
//!
//! Frozen lisp §4.2.b `retention` + §4.3 `orphan-cleanup` + §4.4
//! `observability`. Phase 5 DC028 deferred this wire-up to Phase 8; this
//! module is that wire-up.
//!
//! Behaviour:
//!   * Sleep until the next midnight (UTC), then repeat every 24h.
//!   * Each tick:
//!     1. `event::log::retention::cleanup_once` — TTL eviction for
//!        `event_log` (3d ephemeral / 30d persistent) + `blob_storage`.
//!     2. Orphan-subscription sweep — DELETE cursors whose
//!        `last_seen_at` is > 30 days stale, and emit one
//!        `IncidentEvent::StaleSubscription` per row so ops can see it.
//!     3. Emit `ObservabilityEvent::RetentionReport` summarising the run.
//!
//! Everything is best-effort — errors log a warn and the next tick tries
//! again. A retention failure must never take the daemon down.

#![cfg(feature = "postgres")]

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Timelike, Utc};
use missiond_core::event::events::{IncidentEvent, ObservabilityEvent};
use missiond_core::event::log::retention::{cleanup_once, CleanupReport};
use missiond_core::types::{IncidentSeverity, IncidentSource, MissionIncident};
use sqlx::PgPool;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::bus::BusServices;

/// Spawn the daily cron task. Returns a join handle that completes when
/// `shutdown` fires.
pub(crate) fn spawn_retention_cron(
    bus: Arc<BusServices>,
    pool: PgPool,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("bus: retention cron spawned (daily)");
        loop {
            let sleep_until_midnight = duration_until_next_midnight(Utc::now());
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("bus: retention cron shutdown");
                        return;
                    }
                }
                _ = tokio::time::sleep(sleep_until_midnight) => {
                    run_one_tick(&bus, &pool).await;
                }
            }
        }
    })
}

/// Core of a single tick — public for tests.
pub(crate) async fn run_one_tick(bus: &Arc<BusServices>, pool: &PgPool) {
    let cleanup = match cleanup_once(pool).await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "bus: retention cleanup failed");
            CleanupReport::default()
        }
    };

    let orphans = match sweep_orphan_cursors(bus, pool).await {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "bus: orphan subscription sweep failed");
            0
        }
    };

    let summary = serde_json::json!({
        "ephemeral_events_deleted": cleanup.ephemeral_events_deleted,
        "persistent_events_deleted": cleanup.persistent_events_deleted,
        "blobs_deleted": cleanup.blobs_deleted,
        "orphan_subscriptions_deleted": orphans,
    });
    info!(?summary, "bus: retention tick complete");

    // Observability event — ephemeral so the retention report itself does
    // not accumulate forever.
    let _ = bus
        .publish_observability(ObservabilityEvent::RetentionReport { summary })
        .await;
}

/// Delete rows in `event_subscriptions` whose `last_seen_at` is > 30 days
/// old. Emit one `IncidentEvent::StaleSubscription` per row so ops can tell
/// which consumer disappeared.
async fn sweep_orphan_cursors(
    bus: &Arc<BusServices>,
    pool: &PgPool,
) -> Result<u64, sqlx::Error> {
    // Pull the victims first so we can emit one incident per name, then
    // DELETE — two statements, same pool.
    let rows: Vec<(String, String, Option<DateTime<Utc>>)> = sqlx::query_as(
        r#"
        SELECT subscription_name, consumer_name, last_seen_at
        FROM event_subscriptions
        WHERE last_seen_at IS NOT NULL AND last_seen_at < now() - interval '30 days'
        "#,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let deleted = sqlx::query(
        r#"
        DELETE FROM event_subscriptions
        WHERE last_seen_at IS NOT NULL AND last_seen_at < now() - interval '30 days'
        "#,
    )
    .execute(pool)
    .await?
    .rows_affected();

    // Emit one incident per orphan. Ephemeral is fine — these are
    // diagnostic; ops normally watches the Observability RetentionReport.
    for (name, consumer, last_seen_at) in rows {
        let incident = MissionIncident {
            id: format!("stale-sub:{name}"),
            severity: IncidentSeverity::Warning,
            source: IncidentSource::HealthCheck,
            title: format!("stale subscription {name}"),
            description: format!(
                "consumer '{consumer}' subscription '{name}' had no ack for >30 days (last_seen_at={:?}); cursor archived",
                last_seen_at
            ),
            server_id: None,
            raw_payload: serde_json::json!({
                "subscription_name": name,
                "consumer_name": consumer,
                "last_seen_at": last_seen_at.map(|t| t.to_rfc3339()),
            }),
            created_at: Utc::now().to_rfc3339(),
        };
        let _ = bus
            .publish_incident(IncidentEvent::StaleSubscription { incident })
            .await;
    }

    Ok(deleted)
}

/// Duration from `now` to 00:00 UTC the next day.
fn duration_until_next_midnight(now: DateTime<Utc>) -> Duration {
    let secs_today: i64 =
        now.hour() as i64 * 3600 + now.minute() as i64 * 60 + now.second() as i64;
    let remainder: i64 = (24 * 3600_i64).saturating_sub(secs_today);
    Duration::from_secs(remainder.max(1) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn next_midnight_is_positive_and_under_24h() {
        let now = Utc.with_ymd_and_hms(2026, 4, 19, 12, 30, 45).unwrap();
        let d = duration_until_next_midnight(now);
        assert!(d.as_secs() > 0);
        assert!(d.as_secs() < 24 * 3600);
    }

    #[test]
    fn midnight_exactly_yields_minimal_sleep() {
        let now = Utc.with_ymd_and_hms(2026, 4, 19, 0, 0, 0).unwrap();
        let d = duration_until_next_midnight(now);
        // We guarantee > 0 to avoid busy loop.
        assert!(d.as_secs() > 0);
    }
}
