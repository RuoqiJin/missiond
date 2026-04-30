use missiond_core::event::events::ExecutionEvent;
use missiond_core::event::log::{
    EventLogQuery, EventLogQueryable, LogReadable, EVENT_LOG_QUERY_LIMIT_CAP,
};
use missiond_core::event::Domain;
use std::collections::HashMap;
use std::sync::Mutex;

use super::{EventRef, EventRefStatus};

// ═════════════════════════════════════════════════════════════════════════
// wave-16 / task 07 — EventRefResolver
//
// Conservative passive-cache lookup so call sites that no longer carry the
// live `Seq` from the original publish path can still attach a real event
// id when a subscriber observed the same event.
//
// Strategy chosen (lowest risk):
//   * In-memory bounded HashMap keyed by deterministic correlation tuple
//     (currently only `plan-node` lifecycle transitions; future kinds add
//     their own correlation key constructor).
//   * Populated by a single passive `ExecutionEvent` subscriber spawned in
//     `bus::v2_subscribers::spawn_event_ref_cache_sub` — the subscriber
//     never mutates DB / fires further events; it just acks and inserts.
//   * Bounded retention: a soft cap evicts the oldest insertion order
//     entries when the cap is hit. `EVENT_REF_CACHE_CAP` is intentionally
//     small (1024) — the cache exists for the "evidence write happens
//     immediately after the publish" case, not for cold history replay.
//   * Lookup miss → `EventRef::unavailable("not in resolver cache")`.
//     We deliberately do NOT block / poll the cache — primary dispatch
//     and evidence write must never wait on a resolver lookup.
//
// Log-query path (the `EventRefStatus::Log` constructor) is reserved for
// a future caller that wants to query `LogReadable::read_from(...)` for
// recent execution events. Today only the in-memory cache is wired so
// every cache hit surfaces as `EventRefStatus::Log` (resolved post-hoc,
// not from the publish call site that ran the dispatch).
// ═════════════════════════════════════════════════════════════════════════

/// Soft cap on the resolver's in-memory cache. When the cache reaches this
/// size, the oldest insertion-order entry is evicted on every new insert.
/// Sized for the "evidence write happens within seconds of the publish"
/// pattern — not a long-term audit store.
pub(crate) const EVENT_REF_CACHE_CAP: usize = 1024;

/// Reason string stamped on `EventRef::unavailable` returned from a cache
/// miss. Kept as a constant so call sites + tests can pin the wire form.
pub(crate) const EVENT_REF_RESOLVER_MISS_REASON: &str = "event ref not in resolver cache";

/// Reason stamped when neither the cache nor the persistent event log can
/// produce a matching `PlanNodeStateChanged` ref. Surfaces on the
/// `unavailable_reason` field so audit consumers can distinguish a clean
/// "no event ever recorded" miss from a transient query error
/// ([`EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX`] below).
pub(crate) const EVENT_REF_LOG_QUERY_MISS_REASON: &str =
    "event ref not in resolver cache or event log";

/// Prefix stamped when the log-query path itself errored (DB unavailable,
/// pg feature off, etc.). The full reason format is
/// `<prefix>: <underlying error>` so consumers can pivot on the prefix
/// without parsing the inner detail.
pub(crate) const EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX: &str = "event ref log-query error";

/// Bound on the number of execution-domain rows the resolver will scan when
/// looking for a `PlanNodeStateChanged` match. The scan is read-only and
/// happens off the dispatch hot path (only when the cache misses) — sized
/// so a few hundred recent transitions are always reachable while a long
/// history replay can never balloon a single lookup. Adjust upward only if
/// the dispatch fan-out grows past this within the cache miss window.
pub(crate) const EVENT_REF_LOG_QUERY_SCAN_LIMIT: usize = 512;

/// Cached event entry — what the subscriber stored alongside the
/// correlation key. Currently just the (source, kind, event_id) triple
/// surfaced as `EventRef`. Kept as a separate struct so the cache value
/// can grow (e.g. add `recorded_at`) without churning the `EventRef`
/// constructor surface.
#[derive(Debug, Clone)]
struct CachedEvent {
    source: String,
    kind: String,
    event_id: String,
}

/// Lightweight in-memory cache + lookup surface for `EventRef` recovery.
///
/// Construct one per daemon and share via `Arc`. The subscriber inserts
/// via [`record_plan_node_state_change`]; consumer call sites query via
/// [`lookup_plan_node_state_change`].
///
/// The resolver NEVER fails the caller: a cache miss returns
/// `EventRef::unavailable(EVENT_REF_RESOLVER_MISS_REASON)` so the
/// downstream evidence write proceeds without a hard error. This matches
/// the wave-16 / task 07 brief: "event lookup failure must not fail
/// primary dispatch / evidence write".
#[derive(Debug)]
pub(crate) struct EventRefResolver {
    inner: Mutex<EventRefResolverInner>,
}

#[derive(Debug)]
struct EventRefResolverInner {
    /// Insertion-order keys for FIFO eviction when cap is hit. We use a
    /// `Vec<String>` rather than `VecDeque<String>` because the eviction
    /// rate is far lower than the lookup rate; the constant-time `pop`
    /// from the back is preferable but eviction happens at the front so
    /// we accept the O(n) shift — `EVENT_REF_CACHE_CAP=1024` keeps the
    /// shift cost negligible against the publish rate.
    order: Vec<String>,
    map: HashMap<String, CachedEvent>,
    cap: usize,
}

impl EventRefResolver {
    /// Construct a resolver with the default capacity ([`EVENT_REF_CACHE_CAP`]).
    pub(crate) fn new() -> Self {
        Self::with_capacity(EVENT_REF_CACHE_CAP)
    }

    /// Construct a resolver with a caller-chosen capacity. Exposed so
    /// tests can pin small caps and exercise the eviction path.
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            inner: Mutex::new(EventRefResolverInner {
                order: Vec::with_capacity(cap.min(64)),
                map: HashMap::with_capacity(cap.min(64)),
                cap,
            }),
        }
    }

    /// Build the deterministic correlation key for a plan-node lifecycle
    /// transition. Mirrors the format `plan_dag::deterministic_plan_node_event_id`
    /// uses for the deterministic-fallback id so cache keys and event ids
    /// are derivable from the same correlation tuple.
    pub(crate) fn plan_node_state_change_key(
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
    ) -> String {
        format!(
            "plan-node:{}:{}:{}:{}-{}",
            plan_id, node_id, attempt, from, to
        )
    }

    /// Insert a plan-node lifecycle event into the cache. Idempotent on
    /// repeat inserts of the same key (later inserts overwrite the prior
    /// value but do NOT bump the eviction order — the first publish wins
    /// for FIFO eviction purposes).
    ///
    /// `event_id` should be the live `Seq` (or deterministic fallback id)
    /// the publish path used. `source` / `kind` mirror what the producer
    /// would have stamped on `EventRef::new`.
    pub(crate) fn record_plan_node_state_change(
        &self,
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
        source: impl Into<String>,
        kind: impl Into<String>,
        event_id: impl Into<String>,
    ) {
        let key = Self::plan_node_state_change_key(plan_id, node_id, attempt, from, to);
        let entry = CachedEvent {
            source: source.into(),
            kind: kind.into(),
            event_id: event_id.into(),
        };
        let mut g = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !g.map.contains_key(&key) {
            // FIFO eviction when cap reached.
            if g.order.len() >= g.cap {
                if let Some(oldest) = g.order.first().cloned() {
                    g.order.remove(0);
                    g.map.remove(&oldest);
                }
            }
            g.order.push(key.clone());
        }
        g.map.insert(key, entry);
    }

    /// Look up a plan-node lifecycle event by deterministic correlation
    /// key. On hit returns an `EventRef` with `status=Log` (the id was
    /// recovered post-hoc rather than carried directly from the publish
    /// path). On miss returns `EventRef::unavailable` with the canonical
    /// miss reason — never errors / blocks.
    pub(crate) fn lookup_plan_node_state_change(
        &self,
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
    ) -> EventRef {
        let key = Self::plan_node_state_change_key(plan_id, node_id, attempt, from, to);
        let g = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match g.map.get(&key) {
            Some(c) => EventRef::from_log(c.source.clone(), c.kind.clone(), c.event_id.clone()),
            None => EventRef::unavailable(EVENT_REF_RESOLVER_MISS_REASON),
        }
    }

    /// Three-tier lookup: live in-memory cache, then a bounded read-only
    /// scan of the persistent event log (`Domain::Execution` rows newer
    /// than `Seq(0)`, capped at [`EVENT_REF_LOG_QUERY_SCAN_LIMIT`]),
    /// then `EventRef::unavailable(...)` with a descriptive reason.
    ///
    /// Wave-17 / task 06: the persistent path lets evidence refs survive
    /// daemon restarts — once a `PlanNodeStateChanged` was committed to
    /// the event log, any later evidence write can recover it through
    /// this method even though the in-memory cache (`EventRefResolver`)
    /// was dropped on restart.
    ///
    /// Contract:
    ///   * Cache hit returns immediately with `EventRef::from_log` (no
    ///     log scan triggered).
    ///   * Cache miss triggers `LogReadable::read_from(Domain::Execution,
    ///     Seq(0), EVENT_REF_LOG_QUERY_SCAN_LIMIT)`. We scan the result
    ///     for `PlanNodeStateChanged` payloads whose deterministic
    ///     correlation tuple matches the requested key. First match wins
    ///     (rows are ordered by `seq` ASC; matching is exact on
    ///     plan_id/node_id/from/to and `attempt` if the row carried one).
    ///   * Log query failure returns `EventRef::unavailable` with reason
    ///     `<EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX>: <error>` — the
    ///     primary dispatch must NEVER fail because of this.
    ///   * No match returns `EventRef::unavailable(EVENT_REF_LOG_QUERY_MISS_REASON)`.
    ///
    /// `attempt` is matched when the persisted row carries a non-empty
    /// `attempt` field; rows without an `attempt` are accepted as
    /// matches for any caller `attempt` (mirrors the `attempt: Option<u32>`
    /// shape on `ExecutionEvent::PlanNodeStateChanged` — early producers
    /// may not populate it).
    pub(crate) async fn lookup_or_query_plan_node_state_change(
        &self,
        log: &dyn LogReadable,
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
    ) -> EventRef {
        // Tier 1 — live in-memory cache.
        let cached = self.lookup_plan_node_state_change(plan_id, node_id, attempt, from, to);
        if cached.status == EventRefStatus::Log {
            return cached;
        }
        // Tier 2 — bounded read-only typed query of the persistent event
        // log. Wave-18 / task 01 replaces the prior raw
        // `LogReadable::read_from(Domain::Execution, Seq(0), 512)` scan
        // with the [`EventLogQueryable`] surface so the `kind=
        // plan_node_state_changed` predicate is expressed declaratively
        // and the limit is clamped through [`EVENT_LOG_QUERY_LIMIT_CAP`].
        //
        // We deliberately do NOT push the plan_id / node_id / from / to
        // correlation into the query builder for *this* event — `serde`
        // serializes [`ExecutionEvent`] in the externally-tagged form
        // (`{"PlanNodeStateChanged": { plan_id, ... }}`) so the matchable
        // fields live one level deeper than the top-level
        // [`CorrelationPredicate::key`] surface supports. The bounded
        // kind+limit query already cuts the scan budget; the deserialise
        // loop below applies the per-field correlation. Future events that
        // serialize with top-level scalar fields can lift the predicates
        // straight into the query.
        let query = EventLogQuery::new(Domain::Execution)
            .kind("plan_node_state_changed")
            .limit(EVENT_REF_LOG_QUERY_SCAN_LIMIT.min(EVENT_LOG_QUERY_LIMIT_CAP));
        match EventLogQueryable::query(log, query).await {
            Ok(rows) => {
                for row in rows.into_iter().rev() {
                    // Iterate newest-first so the latest matching row wins
                    // when the same correlation tuple was published more
                    // than once (e.g. attempt re-emit after a partial
                    // failure).
                    let ev: ExecutionEvent = match serde_json::from_value(row.payload.clone()) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if let ExecutionEvent::PlanNodeStateChanged {
                        plan_id: rp,
                        node_id: rn,
                        from: rf,
                        to: rt,
                        attempt: ra,
                        ..
                    } = ev
                    {
                        if rp != plan_id || rn != node_id || rf != from || rt != to {
                            continue;
                        }
                        if let Some(ra) = ra {
                            if ra != attempt {
                                continue;
                            }
                        }
                        // Match. Populate the cache so subsequent lookups
                        // skip the query — the cached value reuses
                        // [`EventRef::from_log`] which stamps
                        // `EventRefProvenance::PassiveCache`, mirroring
                        // the wave-16 single-tier resolver semantics.
                        let seq_id = row.seq.0.to_string();
                        self.record_plan_node_state_change(
                            plan_id,
                            node_id,
                            attempt,
                            from,
                            to,
                            "execution",
                            "plan_node_state_changed",
                            seq_id.clone(),
                        );
                        return EventRef::from_event_log_query(
                            "execution",
                            "plan_node_state_changed",
                            seq_id,
                        );
                    }
                }
                // Tier 3 — no match in the log either.
                EventRef::unavailable(EVENT_REF_LOG_QUERY_MISS_REASON)
            }
            Err(err) => EventRef::unavailable(format!(
                "{}: {}",
                EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX, err
            )),
        }
    }

    /// Current entry count — exposed for tests / metrics.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(g) => g.map.len(),
            Err(poisoned) => poisoned.into_inner().map.len(),
        }
    }
}

impl Default for EventRefResolver {
    fn default() -> Self {
        Self::new()
    }
}
