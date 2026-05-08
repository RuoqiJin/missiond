//! Bounded, typed read-only query API over `event_log`.
//!
//! Wave-18 / task 01 — turns the wave-17 `LogReadable::read_from(...)` ad hoc
//! scan into a reusable surface so future features can express
//! domain/kind/correlation/time-window/limit lookups without re-implementing
//! the projection / payload-match plumbing each time.
//!
//! # Design
//!
//! * Builder ([`EventLogQuery`]) gathers the optional filters; the only
//!   required field is `domain` (the `event_log.domain` index is the
//!   primary scan path so we always pin it).
//! * The trait ([`EventLogQueryable`]) carries a single `query(...)` entry
//!   point. Existing [`LogReadable`] implementers gain it for free via the
//!   blanket impl below — the default executes `read_from(domain, Seq(0),
//!   capped_limit)` and applies kind/time/correlation predicates in memory.
//! * A defensive cap ([`EVENT_LOG_QUERY_LIMIT_CAP`]) clamps the request so a
//!   typo'd `usize::MAX` cannot turn into a multi-million row scan. Today
//!   the cap matches the prior wave-17 ad hoc scan limit; bump only with a
//!   matching producer-rate audit.
//! * No new migration: `event_log (domain, seq)` already covers the primary
//!   scan, kind/correlation are matched in-memory after the bounded fetch.
//!   PG impl can grow a SQL-side filter later without changing the trait.
//!
//! # Failure
//!
//! `query(...)` returns the same [`LogError`] surface as `read_from`. The
//! caller decides whether a transient query failure degrades to an
//! `unavailable` ref or hard-errors — the resolver
//! ([`crate::handlers::knowledge::evidence_collector::EventRefResolver`])
//! always degrades so primary dispatch + evidence write never block on the
//! lookup. The trait does not own that policy.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::super::domain::Domain;
use super::reader::LoggedEvent;
use super::{LogError, LogReadable, Seq};

/// Defensive cap on the number of rows the query API will ever return in a
/// single call. Sized to match the prior wave-17 ad hoc scan
/// (`EVENT_REF_LOG_QUERY_SCAN_LIMIT = 512`) so behaviour stays identical for
/// callers migrating from the raw `read_from(...)` path.
///
/// Bump only after auditing the publish rate over the lookup window —
/// callers that want more rows should run multiple bounded queries instead
/// of asking the trait for an unbounded scan.
pub const EVENT_LOG_QUERY_LIMIT_CAP: usize = 512;

/// Default limit when the caller does not specify one. Matches the cap so
/// the no-limit common case behaves like the prior raw scan.
pub const EVENT_LOG_QUERY_DEFAULT_LIMIT: usize = EVENT_LOG_QUERY_LIMIT_CAP;

/// One JSON correlation predicate — `payload[key] == value` (exact match
/// after JSON deserialization). Multiple predicates compose with AND.
///
/// The match runs against the resolved [`LoggedEvent::payload`] so it works
/// regardless of whether the row was stored inline or via claim-check.
#[derive(Debug, Clone)]
pub struct CorrelationPredicate {
    pub key: String,
    pub value: Value,
}

/// Read-only query over `event_log`. Build via [`EventLogQuery::new`] and
/// pass to [`EventLogQueryable::query`].
#[derive(Debug, Clone)]
pub struct EventLogQuery {
    /// Required — domain to scan. We always pin this so the
    /// `idx_event_log_domain_seq` index drives the read.
    pub domain: Domain,
    /// Optional `event_log.kind` filter. When set, only rows whose stored
    /// `kind` string equals this value are returned.
    pub kind: Option<String>,
    /// Zero or more JSON-payload correlation predicates (AND-composed).
    pub correlation: Vec<CorrelationPredicate>,
    /// Optional `ts >= since` lower bound (inclusive).
    pub since: Option<DateTime<Utc>>,
    /// Optional `ts < until` upper bound (exclusive).
    pub until: Option<DateTime<Utc>>,
    /// Optional `seq > after_seq` lower bound. Defaults to `Seq(0)` which
    /// scans from the start of the domain partition.
    pub after_seq: Option<Seq>,
    /// Caller-requested limit. Always clamped to [`EVENT_LOG_QUERY_LIMIT_CAP`]
    /// in [`Self::effective_limit`]; producers cannot bypass the cap.
    pub limit: Option<usize>,
}

impl EventLogQuery {
    /// Start a new query against `domain`. Add filters with the chained
    /// setters below.
    pub fn new(domain: Domain) -> Self {
        Self {
            domain,
            kind: None,
            correlation: Vec::new(),
            since: None,
            until: None,
            after_seq: None,
            limit: None,
        }
    }

    /// Filter by the canonical `event_log.kind` wire tag (e.g.
    /// `"plan_node_state_changed"`).
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Add an exact-match correlation predicate against the resolved JSON
    /// payload. Multiple calls AND together.
    pub fn correlation(mut self, key: impl Into<String>, value: Value) -> Self {
        self.correlation.push(CorrelationPredicate {
            key: key.into(),
            value,
        });
        self
    }

    /// Inclusive `ts >= since` lower bound.
    pub fn since(mut self, since: DateTime<Utc>) -> Self {
        self.since = Some(since);
        self
    }

    /// Exclusive `ts < until` upper bound.
    pub fn until(mut self, until: DateTime<Utc>) -> Self {
        self.until = Some(until);
        self
    }

    /// Strict `seq > after_seq` lower bound. Defaults to `Seq(0)`.
    pub fn after_seq(mut self, after: Seq) -> Self {
        self.after_seq = Some(after);
        self
    }

    /// Caller-requested limit. Always clamped to [`EVENT_LOG_QUERY_LIMIT_CAP`].
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Effective limit applied after defensive capping. Exposed so callers
    /// (and tests) can assert the cap is honoured even when they request
    /// more rows.
    pub fn effective_limit(&self) -> usize {
        self.limit
            .unwrap_or(EVENT_LOG_QUERY_DEFAULT_LIMIT)
            .min(EVENT_LOG_QUERY_LIMIT_CAP)
    }

    /// Effective `seq > after_seq` lower bound. Defaults to `Seq(0)` so the
    /// query scans from the start of the domain partition.
    pub fn effective_after_seq(&self) -> Seq {
        self.after_seq.unwrap_or(Seq(0))
    }

    /// Apply the in-memory predicates (kind / correlation / time window) to
    /// a row already fetched by the underlying log scan.
    ///
    /// `pub` so a custom [`EventLogQueryable`] impl that pushes some
    /// predicates into SQL can still re-use the canonical match logic for
    /// the predicates that did not get pushed down.
    pub fn matches(&self, row: &LoggedEvent) -> bool {
        if let Some(ref k) = self.kind {
            if row.kind != *k {
                return false;
            }
        }
        if let Some(since) = self.since {
            if row.ts < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if row.ts >= until {
                return false;
            }
        }
        for pred in &self.correlation {
            match row.payload.get(&pred.key) {
                Some(v) if *v == pred.value => continue,
                _ => return false,
            }
        }
        true
    }
}

/// Read-only typed query surface over the event log.
///
/// Every [`LogReadable`] gains this trait via the blanket impl below. PG /
/// custom backends can override `query` to push the kind / correlation /
/// time predicates into SQL when the index supports it; callers MUST NOT
/// rely on that — the trait contract is "results are equivalent to running
/// `read_from` then filtering through [`EventLogQuery::matches`]".
#[async_trait]
pub trait EventLogQueryable: Send + Sync {
    /// Execute the query. Returns at most [`EventLogQuery::effective_limit`]
    /// rows ordered by `seq` ASC.
    async fn query(&self, query: EventLogQuery) -> Result<Vec<LoggedEvent>, LogError>;
}

#[async_trait]
impl<T> EventLogQueryable for T
where
    T: LogReadable + ?Sized,
{
    async fn query(&self, query: EventLogQuery) -> Result<Vec<LoggedEvent>, LogError> {
        let after = query.effective_after_seq();
        let cap = query.effective_limit();
        // Fetch up to the cap from the underlying domain scan; in-memory
        // predicates may then reduce the result further. The over-fetch
        // budget intentionally stays at the cap — bumping it here would
        // defeat the defensive limit.
        let rows = self.read_from(query.domain, after, cap).await?;
        let filtered: Vec<LoggedEvent> = rows.into_iter().filter(|r| query.matches(r)).collect();
        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(seq: i64, kind: &str, payload: Value) -> LoggedEvent {
        LoggedEvent {
            seq: Seq(seq),
            domain: Domain::Execution,
            kind: kind.to_string(),
            payload,
            producer_id: "test".to_string(),
            dedupe_key: None,
            causation_depth: 0,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            ts: Utc::now(),
            ephemeral: false,
        }
    }

    #[test]
    fn builder_caps_requested_limit() {
        let q = EventLogQuery::new(Domain::Execution).limit(usize::MAX);
        assert_eq!(q.effective_limit(), EVENT_LOG_QUERY_LIMIT_CAP);
    }

    #[test]
    fn builder_default_limit_matches_cap() {
        let q = EventLogQuery::new(Domain::Execution);
        assert_eq!(q.effective_limit(), EVENT_LOG_QUERY_LIMIT_CAP);
    }

    #[test]
    fn builder_honours_smaller_limit() {
        let q = EventLogQuery::new(Domain::Execution).limit(7);
        assert_eq!(q.effective_limit(), 7);
    }

    #[test]
    fn matches_kind_exact() {
        let q = EventLogQuery::new(Domain::Execution).kind("plan_node_state_changed");
        assert!(q.matches(&fixture(1, "plan_node_state_changed", json!({}))));
        assert!(!q.matches(&fixture(1, "opened", json!({}))));
    }

    #[test]
    fn matches_correlation_exact_value() {
        let q = EventLogQuery::new(Domain::Execution)
            .correlation("plan_id", json!("abc"))
            .correlation("node_id", json!("n1"));
        let hit = fixture(1, "x", json!({"plan_id": "abc", "node_id": "n1"}));
        let miss = fixture(1, "x", json!({"plan_id": "abc", "node_id": "n2"}));
        let absent = fixture(1, "x", json!({"plan_id": "abc"}));
        assert!(q.matches(&hit));
        assert!(!q.matches(&miss));
        assert!(!q.matches(&absent), "missing key fails the predicate");
    }

    #[test]
    fn matches_time_window_inclusive_lower_exclusive_upper() {
        let now = Utc::now();
        let earlier = now - chrono::Duration::seconds(60);
        let later = now + chrono::Duration::seconds(60);

        let q = EventLogQuery::new(Domain::Execution)
            .since(earlier)
            .until(later);

        let mut row_now = fixture(1, "x", json!({}));
        row_now.ts = now;
        assert!(q.matches(&row_now));

        let mut row_too_early = fixture(1, "x", json!({}));
        row_too_early.ts = earlier - chrono::Duration::seconds(1);
        assert!(!q.matches(&row_too_early));

        let mut row_at_until = fixture(1, "x", json!({}));
        row_at_until.ts = later;
        assert!(!q.matches(&row_at_until), "until is exclusive");

        let mut row_at_since = fixture(1, "x", json!({}));
        row_at_since.ts = earlier;
        assert!(q.matches(&row_at_since), "since is inclusive");
    }

    #[test]
    fn after_seq_defaults_to_zero() {
        let q = EventLogQuery::new(Domain::Execution);
        assert_eq!(q.effective_after_seq(), Seq(0));
        let q = EventLogQuery::new(Domain::Execution).after_seq(Seq(123));
        assert_eq!(q.effective_after_seq(), Seq(123));
    }

    // Stub LogReadable used by the blanket-impl test below.
    struct StubLog {
        rows: Vec<LoggedEvent>,
        last_limit: std::sync::Mutex<usize>,
    }
    #[async_trait]
    impl LogReadable for StubLog {
        async fn read_from(
            &self,
            _domain: Domain,
            _after: Seq,
            limit: usize,
        ) -> Result<Vec<LoggedEvent>, LogError> {
            *self.last_limit.lock().unwrap() = limit;
            Ok(self.rows.clone())
        }
        async fn head_seq(&self) -> Result<Seq, LogError> {
            Ok(Seq(self.rows.iter().map(|r| r.seq.0).max().unwrap_or(0)))
        }
    }

    #[tokio::test]
    async fn blanket_impl_filters_by_kind_and_correlation() {
        let stub = StubLog {
            rows: vec![
                fixture(
                    1,
                    "plan_node_state_changed",
                    json!({"plan_id": "p1", "node_id": "n1"}),
                ),
                fixture(2, "opened", json!({})),
                fixture(
                    3,
                    "plan_node_state_changed",
                    json!({"plan_id": "p2", "node_id": "n1"}),
                ),
            ],
            last_limit: std::sync::Mutex::new(0),
        };
        let q = EventLogQuery::new(Domain::Execution)
            .kind("plan_node_state_changed")
            .correlation("plan_id", json!("p1"));
        let out = stub.query(q).await.expect("query ok");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].seq, Seq(1));
    }

    #[tokio::test]
    async fn blanket_impl_passes_capped_limit_to_underlying_read() {
        let stub = StubLog {
            rows: Vec::new(),
            last_limit: std::sync::Mutex::new(0),
        };
        let q = EventLogQuery::new(Domain::Execution).limit(usize::MAX);
        let _ = stub.query(q).await;
        assert_eq!(
            *stub.last_limit.lock().unwrap(),
            EVENT_LOG_QUERY_LIMIT_CAP,
            "blanket impl must clamp at the defensive cap"
        );
    }

    #[tokio::test]
    async fn blanket_impl_propagates_log_error() {
        struct ErrLog;
        #[async_trait]
        impl LogReadable for ErrLog {
            async fn read_from(
                &self,
                _domain: Domain,
                _after: Seq,
                _limit: usize,
            ) -> Result<Vec<LoggedEvent>, LogError> {
                Err(LogError::Other("simulated db down".into()))
            }
            async fn head_seq(&self) -> Result<Seq, LogError> {
                Err(LogError::Other("simulated db down".into()))
            }
        }
        let q = EventLogQuery::new(Domain::Execution);
        let err = ErrLog.query(q).await.unwrap_err();
        assert!(format!("{err}").contains("simulated db down"));
    }
}
