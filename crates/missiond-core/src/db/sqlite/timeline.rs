use std::collections::HashMap;
use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::TimelineStore;
use crate::db::timeline::{TimelineRow, TimelineStats};

#[async_trait]
impl TimelineStore for SqliteMissionStore {
    async fn insert_timeline_batch(&self, entries: &[(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)]) -> DbResult<Vec<i64>> {
        let entries: Vec<(Option<String>, String, Option<String>, String, Option<String>, String)> = entries
            .iter()
            .map(|(a, b, c, d, e, f)| (
                a.map(|s| s.to_string()),
                b.to_string(),
                c.map(|s| s.to_string()),
                d.to_string(),
                e.map(|s| s.to_string()),
                f.to_string(),
            ))
            .collect();
        self.executor.run(move |db| {
            let refs: Vec<(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)> = entries
                .iter()
                .map(|(a, b, c, d, e, f)| (
                    a.as_deref(),
                    b.as_str(),
                    c.as_deref(),
                    d.as_str(),
                    e.as_deref(),
                    f.as_str(),
                ))
                .collect();
            db.insert_timeline_batch(&refs)
        }).await
    }

    async fn query_timeline_since(&self, since_seq: i64, limit: usize) -> DbResult<Vec<TimelineRow>> {
        self.executor.run(move |db| db.query_timeline_since(since_seq, limit)).await
    }

    async fn timeline_latest_seq(&self) -> DbResult<i64> {
        self.executor.run(|db| db.timeline_latest_seq()).await
    }

    async fn cleanup_timeline_ttl(&self, days: i64) -> DbResult<usize> {
        self.executor.run(move |db| db.cleanup_timeline_ttl(days)).await
    }

    async fn find_timeline_needing_briefing(&self, min_content_chars: usize, limit: usize) -> DbResult<Vec<(i64, String, String, Option<String>)>> {
        self.executor.run(move |db| db.find_timeline_needing_briefing(min_content_chars, limit)).await
    }

    async fn update_timeline_summary(&self, seq: i64, summary: &str) -> DbResult<bool> {
        let summary = summary.to_owned();
        self.executor.run(move |db| db.update_timeline_summary(seq, &summary)).await
    }

    async fn query_timeline_filtered(&self, event_type: Option<&str>, trace_id: Option<&str>, since: Option<&str>, until: Option<&str>, limit: i64, offset: i64) -> DbResult<Vec<TimelineRow>> {
        let event_type = event_type.map(|s| s.to_owned());
        let trace_id = trace_id.map(|s| s.to_owned());
        let since = since.map(|s| s.to_owned());
        let until = until.map(|s| s.to_owned());
        self.executor.run(move |db| db.query_timeline_filtered(event_type.as_deref(), trace_id.as_deref(), since.as_deref(), until.as_deref(), limit, offset)).await
    }

    async fn query_timeline_stratified(&self, since: &str, until: &str, per_type_limit: i64, type_limits: &HashMap<String, i64>) -> DbResult<Vec<TimelineRow>> {
        let since = since.to_owned();
        let until = until.to_owned();
        let type_limits = type_limits.clone();
        self.executor.run(move |db| db.query_timeline_stratified(&since, &until, per_type_limit, &type_limits)).await
    }

    async fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>> {
        let trace_id = trace_id.to_owned();
        self.executor.run(move |db| db.query_timeline_by_trace(&trace_id)).await
    }

    async fn query_timeline_stats(&self, since: Option<&str>, until: Option<&str>) -> DbResult<TimelineStats> {
        let since = since.map(|s| s.to_owned());
        let until = until.map(|s| s.to_owned());
        self.executor.run(move |db| db.query_timeline_stats(since.as_deref(), until.as_deref())).await
    }

    async fn query_timeline_search(&self, keyword: &str, since: Option<&str>, until: Option<&str>, limit: i64) -> DbResult<Vec<TimelineRow>> {
        let keyword = keyword.to_owned();
        let since = since.map(|s| s.to_owned());
        let until = until.map(|s| s.to_owned());
        self.executor.run(move |db| db.query_timeline_search(&keyword, since.as_deref(), until.as_deref(), limit)).await
    }

    async fn get_briefing_summaries_for_session(&self, session_id: &str) -> DbResult<HashMap<i64, String>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_briefing_summaries_for_session(&session_id)).await
    }
}
