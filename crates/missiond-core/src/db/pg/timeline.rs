//! TimelineStore — PostgreSQL implementation.
//!
//! v1.3.0 SSOT cutover: `event_log` is the timeline SSOT (frozen lisp §4.6
//! `:ssot-declaration`). All reads delegate to `event::projection`; writes
//! are gone (the legacy `system_timeline` table was dropped in
//! `20260420200000_drop_system_timeline.sql` and the timeline-writer v2
//! subscriber was already removed in Phase 8 of the event-bus migration).

use super::PgMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::{TimelineRow, TimelineStats, TimelineStore};
use crate::event::projection;
use async_trait::async_trait;
use std::collections::HashMap;

#[cfg(feature = "postgres")]
#[async_trait]
impl TimelineStore for PgMissionStore {
    async fn query_timeline_since(
        &self,
        since_seq: i64,
        limit: usize,
    ) -> DbResult<Vec<TimelineRow>> {
        projection::query_timeline_since(&self.pool, since_seq, limit).await
    }

    async fn timeline_latest_seq(&self) -> DbResult<i64> {
        projection::timeline_latest_seq(&self.pool).await
    }

    async fn query_timeline_filtered(
        &self,
        event_type: Option<&str>,
        trace_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        projection::query_timeline_filtered(
            &self.pool, event_type, trace_id, since, until, limit, offset,
        )
        .await
    }

    async fn query_timeline_stratified(
        &self,
        since: &str,
        until: &str,
        per_type_limit: i64,
        type_limits: &HashMap<String, i64>,
    ) -> DbResult<Vec<TimelineRow>> {
        projection::query_timeline_stratified(&self.pool, since, until, per_type_limit, type_limits)
            .await
    }

    async fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>> {
        projection::query_timeline_by_trace(&self.pool, trace_id).await
    }

    async fn query_timeline_stats(
        &self,
        since: Option<&str>,
        until: Option<&str>,
    ) -> DbResult<TimelineStats> {
        projection::query_timeline_stats(&self.pool, since, until).await
    }

    async fn query_timeline_search(
        &self,
        keyword: &str,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        projection::query_timeline_search(&self.pool, keyword, since, until, limit).await
    }
}
