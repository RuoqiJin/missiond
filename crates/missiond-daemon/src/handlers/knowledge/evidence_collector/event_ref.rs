use super::*;

/// Provenance tag for an [`EventRef`]. Surfaces on the JSON envelope so
/// downstream consumers can tell at a glance whether the id came from the
/// live publish path, was recovered out-of-band from the in-memory cache /
/// log query (wave-16 / task 07 resolver), or could not be correlated at all.
///
/// Wire form is the all-lowercase variant tag (`"live"` / `"log"` /
/// `"unavailable"`) — kept stable so audit dashboards can pivot on a single
/// string without parsing structured discriminants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventRefStatus {
    /// Producer obtained the id directly from a successful publish (e.g.
    /// `BusServices::publish_execution_with_seq` returned the `Seq`) or
    /// from a deterministic correlation key the producer itself owns.
    Live,
    /// Recovered from a passive subscriber cache or a log-query lookup
    /// after the original publish path no longer carried the id. The id is
    /// still real — the recovery just happened post-hoc.
    Log,
    /// Caller wanted to attach an id but neither the publish path nor the
    /// resolver could surface one. The entry records the attempt + reason
    /// so consumers can tell "no events" apart from "we tried but failed
    /// to correlate".
    Unavailable,
}

impl EventRefStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            EventRefStatus::Live => "live",
            EventRefStatus::Log => "log",
            EventRefStatus::Unavailable => "unavailable",
        }
    }
}

/// Wave-18 / task 01 — provenance tag describing **how** the resolver
/// obtained the event ref. Surfaces on the JSON envelope as
/// `event_ref_source` so audit consumers can pivot on the lookup path
/// (passive cache vs persistent event-log query) without re-deriving it
/// from the surrounding warning string.
///
/// Distinct from [`EventRefStatus`]: the status describes whether the ref
/// is live / log-recovered / missing; the provenance describes *which*
/// resolver tier produced it. A live ref skips the resolver entirely so
/// its provenance is also `Live`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventRefProvenance {
    /// Producer obtained the ref directly from a successful publish — no
    /// resolver lookup was needed.
    Live,
    /// Recovered from the in-memory passive subscriber cache (the wave-16
    /// `EventRefResolver` HashMap populated by
    /// `bus::v2_subscribers::spawn_event_ref_cache_sub`).
    PassiveCache,
    /// Recovered from a bounded read-only event-log query (the wave-18
    /// typed [`EventLogQueryable`] surface). Used after a passive-cache
    /// miss so refs survive daemon restarts.
    EventLogQuery,
    /// Neither the cache nor the log query produced a match (or the query
    /// itself errored). The ref is `unavailable` and the warning carries
    /// the reason.
    Unavailable,
}

impl EventRefProvenance {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            EventRefProvenance::Live => "live",
            EventRefProvenance::PassiveCache => "passive_cache",
            EventRefProvenance::EventLogQuery => "event_log_query",
            EventRefProvenance::Unavailable => "unavailable",
        }
    }
}

/// Reference to an `ExecutionEvent` (or any other domain event) the caller
/// observed while producing this evidence entry.
///
/// `event_id` is whatever string identifier the producer has — for
/// `mission_execution` events that is the helper-protocol prefix-encoded id
/// (`C001`, `D003`, `COMP005`, …). `source` is the originating tool /
/// surface (e.g. `"mission_execution"`). `kind` is the event variant tag
/// (e.g. `"opened"`, `"completed"`).
///
/// When the caller knows an event SHOULD be referenced but cannot produce
/// one (the bus subscription is offline, the event id was lost, etc.) use
/// [`EventRef::unavailable`] — the entry then records `"unavailable": true`
/// plus a reason string so consumers can tell "no events" apart from "we
/// tried but failed to correlate".
///
/// Wave-16 / task 07 added the [`EventRef::status`] tag. Existing
/// [`EventRef::new`] callers stay byte-compatible — `new(...)` is now an
/// alias for [`EventRef::live`] (the publish-path producers always know
/// they have a live id when they call it). The `status` field surfaces on
/// the JSON envelope as `"status": "live" | "log" | "unavailable"` so
/// downstream consumers can pivot without re-deriving provenance.
#[derive(Debug, Clone)]
pub(crate) struct EventRef {
    pub event_id: Option<String>,
    pub source: Option<String>,
    pub kind: Option<String>,
    pub unavailable_reason: Option<String>,
    pub status: EventRefStatus,
    /// Wave-18 / task 01 — which resolver tier produced this ref. Surfaces
    /// on the top-level evidence envelope as `event_ref_source` so audit
    /// consumers know whether the lookup hit the in-memory passive cache,
    /// the persistent event-log query, or surrendered to `unavailable`.
    pub provenance: EventRefProvenance,
}

impl EventRef {
    /// Typed constructor for "I have a real event id" case. Wave-14 / Task
    /// 02 wired the PLAN DAG runtime v2 (`plan_dag.rs`) to call this with
    /// either the live `Seq` returned from
    /// `BusServices::publish_execution_with_seq(...)` or the deterministic
    /// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` fallback id
    /// when the bus publish fails. Plan-runner v0 (`plan.rs`) keeps using
    /// `EventRef::unavailable(...)` because the single-node runner does not
    /// yet propagate the inner `mission_execution` seq back to the
    /// evidence-write call site — that wiring is a separate follow-up.
    ///
    /// Wave-16 / task 07: alias for [`EventRef::live`]. Kept on the public
    /// surface for byte-compat with the wave-13/14 call sites and tests
    /// that already pin `EventRef::new(...)`.
    pub(crate) fn new(
        source: impl Into<String>,
        kind: impl Into<String>,
        event_id: impl Into<String>,
    ) -> Self {
        Self::live(source, kind, event_id)
    }

    /// Record a live event id (publish path returned it directly).
    pub(crate) fn live(
        source: impl Into<String>,
        kind: impl Into<String>,
        event_id: impl Into<String>,
    ) -> Self {
        Self {
            event_id: Some(event_id.into()),
            source: Some(source.into()),
            kind: Some(kind.into()),
            unavailable_reason: None,
            status: EventRefStatus::Live,
            provenance: EventRefProvenance::Live,
        }
    }

    /// Record an event id recovered from a post-hoc lookup (subscriber
    /// cache or log query). Same wire shape as [`EventRef::live`] except
    /// the `status` tag flips to `"log"` so consumers can tell the id was
    /// resolved out-of-band.
    ///
    /// `#[allow(dead_code)]`: wave-16 / task 07 introduced the resolver
    /// surface for downstream call sites that want post-hoc correlation.
    /// No production caller writes `from_log` directly today — the only
    /// producer is the resolver itself (see `EventRefResolver::lookup_*`).
    /// Kept on the public surface so future call sites that want to stamp
    /// a log-recovered id without going through the resolver have a typed
    /// entry point. Exercised by `event_ref_log_status_round_trips`.
    #[allow(dead_code)]
    pub(crate) fn from_log(
        source: impl Into<String>,
        kind: impl Into<String>,
        event_id: impl Into<String>,
    ) -> Self {
        // Default provenance for `from_log`: passive in-memory cache
        // (the wave-16 single-tier resolver). The wave-18 query path uses
        // `from_event_log_query` to stamp `EventRefProvenance::EventLogQuery`
        // so audit consumers can tell which resolver tier produced the ref.
        Self {
            event_id: Some(event_id.into()),
            source: Some(source.into()),
            kind: Some(kind.into()),
            unavailable_reason: None,
            status: EventRefStatus::Log,
            provenance: EventRefProvenance::PassiveCache,
        }
    }

    /// Record an event id recovered from a bounded read-only event-log
    /// query (the wave-18 typed query path). Same wire shape as
    /// [`EventRef::from_log`] except provenance is stamped as
    /// [`EventRefProvenance::EventLogQuery`].
    pub(crate) fn from_event_log_query(
        source: impl Into<String>,
        kind: impl Into<String>,
        event_id: impl Into<String>,
    ) -> Self {
        Self {
            event_id: Some(event_id.into()),
            source: Some(source.into()),
            kind: Some(kind.into()),
            unavailable_reason: None,
            status: EventRefStatus::Log,
            provenance: EventRefProvenance::EventLogQuery,
        }
    }

    /// Record a placeholder: caller wanted to attach an event reference but
    /// the live source isn't available. `reason` is mandatory so the
    /// resulting entry never silently shows up as "no events at all".
    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            event_id: None,
            source: None,
            kind: None,
            unavailable_reason: Some(reason.into()),
            status: EventRefStatus::Unavailable,
            provenance: EventRefProvenance::Unavailable,
        }
    }

    pub(super) fn into_json(self) -> Value {
        let mut m = Map::new();
        m.insert(
            "status".to_string(),
            Value::String(self.status.as_wire().to_string()),
        );
        if let Some(id) = self.event_id {
            m.insert("event_id".to_string(), Value::String(id));
        }
        if let Some(src) = self.source {
            m.insert("source".to_string(), Value::String(src));
        }
        if let Some(k) = self.kind {
            m.insert("kind".to_string(), Value::String(k));
        }
        if let Some(reason) = self.unavailable_reason {
            m.insert("unavailable".to_string(), Value::Bool(true));
            m.insert("unavailable_reason".to_string(), Value::String(reason));
        }
        Value::Object(m)
    }
}
