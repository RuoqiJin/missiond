//! v1.3.0 SSOT cutover: SQLite TimelineStore is a stub.
//!
//! The SQLite backend is retained only for the one-shot migration tool
//! (`sqlite → postgres`). Timeline projection is PostgreSQL-only (reads from
//! the `event_log` table which only exists in PG). Any caller hitting the
//! SQLite store for timeline queries is a bug — we fail fast rather than
//! silently returning empty results.

use std::collections::HashMap;
use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::{DbError, DbResult};
use crate::db::traits::TimelineStore;
use crate::db::shared::{TimelineRow, TimelineStats};

fn unsupported<T>() -> DbResult<T> {
    Err(DbError::Other(
        "TimelineStore is projection over PG event_log; SQLite backend unsupported".into(),
    ))
}

#[async_trait]
impl TimelineStore for SqliteMissionStore {
    async fn query_timeline_since(&self, _since_seq: i64, _limit: usize) -> DbResult<Vec<TimelineRow>> {
        unsupported()
    }

    async fn timeline_latest_seq(&self) -> DbResult<i64> {
        unsupported()
    }

    async fn query_timeline_filtered(&self, _event_type: Option<&str>, _trace_id: Option<&str>, _since: Option<&str>, _until: Option<&str>, _limit: i64, _offset: i64) -> DbResult<Vec<TimelineRow>> {
        unsupported()
    }

    async fn query_timeline_stratified(&self, _since: &str, _until: &str, _per_type_limit: i64, _type_limits: &HashMap<String, i64>) -> DbResult<Vec<TimelineRow>> {
        unsupported()
    }

    async fn query_timeline_by_trace(&self, _trace_id: &str) -> DbResult<Vec<TimelineRow>> {
        unsupported()
    }

    async fn query_timeline_stats(&self, _since: Option<&str>, _until: Option<&str>) -> DbResult<TimelineStats> {
        unsupported()
    }

    async fn query_timeline_search(&self, _keyword: &str, _since: Option<&str>, _until: Option<&str>, _limit: i64) -> DbResult<Vec<TimelineRow>> {
        unsupported()
    }
}
