//! Event log — append-only persistence layer for the event bus.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 pre-req
//! event-log-schema + §4.3 phase-1-bootstrap.
//!
//! This thin module provides:
//!
//! * [`Log`] / [`LogReadable`] — the traits every producer & subscriber uses.
//! * [`AppendOpts`] / [`AppendAck`] / [`AppendError`] / [`Seq`] — the public
//!   surface for the producer append path (frozen lisp §4.1 append-api).
//! * [`reader`] — pull-based catch-up reader for subscribers
//!   (frozen lisp §4.3 phase-1-bootstrap).
//!
//! The actual single-writer task (`LogWriter`) now lives under
//! [`crate::event::pipeline::step3_commit::log_writer`]; retention lives
//! under [`crate::event::lifecycle::retention`]. Both are re-exported here
//! for backwards-compat with callers that still import the `log::` path.

pub mod reader;

pub use reader::{LogReader, LoggedEvent};

// Re-exports pointing at the pipeline / lifecycle modules so existing
// call sites (`event::log::spawn_log_writer` / `event::log::cleanup_once`)
// keep compiling. The canonical homes are documented in each target module.
pub use crate::event::lifecycle::retention::{cleanup_once, CleanupReport};
pub use crate::event::pipeline::step3_commit::log_writer::{
    new_log_writer, spawn_log_writer, LogWriter, LogWriterHandle, PendingAppend,
};

/// Back-compat module alias so `event::log::writer::…` paths still resolve.
///
/// Prefer [`crate::event::pipeline::step3_commit::log_writer`] in new code.
pub mod writer {
    pub use crate::event::pipeline::step3_commit::log_writer::*;
}

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::domain::Domain;
use super::event_trait::DomainEvent;

/// Monotonic event sequence. Assigned by the PG `BIGSERIAL` on the
/// `event_log.seq` column; wrapping in a newtype keeps call sites from
/// mixing it up with arbitrary `i64` ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Seq(pub i64);

impl From<i64> for Seq {
    fn from(v: i64) -> Self {
        Seq(v)
    }
}

impl From<Seq> for i64 {
    fn from(v: Seq) -> Self {
        v.0
    }
}

/// Producer-supplied options for a single append.
///
/// Frozen lisp §4.1 `AppendOpts`. All fields are optional so that the common
/// path (`log.append(event, AppendOpts::default())`) stays terse. `Clone` is
/// derived so the same opts can be reused across retries — `dedupe_key`
/// ensures the retry is idempotent.
#[derive(Debug, Clone, Default)]
pub struct AppendOpts {
    /// `true` ⇒ skip DB persistence; in Phase 2 this simply returns a
    /// [`AppendAck::Volatile`] carrying a process-local monotonic seq hint of
    /// 0 (dispatcher owns real seq assignment in Phase 3). Keep the knob
    /// available for producers that already want to opt into ephemeral
    /// semantics ahead of Phase 3.
    pub ephemeral: bool,

    /// Producer retry protection. Frozen lisp §4.2.b `dedup-semantics`:
    /// re-appending with the same `dedupe_key` returns
    /// [`AppendAck::AlreadyExists`] without side effects.
    pub dedupe_key: Option<Uuid>,

    /// Identifies the producing component. Stamped into `event_log.producer_id`
    /// so that ops can trace who emitted an event.
    pub producer_id: String,

    /// Depth of the causation chain. Incremented by handlers that create
    /// follow-up events; exceeding `MAX_CAUSATION_DEPTH` (10) returns
    /// [`AppendError::CausalLoop`].
    pub causation_depth: u8,

    /// Distributed tracing — carried over from v1 `TraceContext`.
    pub span: Option<SpanContext>,
}

/// OpenTelemetry-lite span carrier.
#[derive(Debug, Clone, Default)]
pub struct SpanContext {
    pub trace_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
}

/// Successful append outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendAck {
    /// Normal path — event is durable in `event_log`.
    Committed { seq: Seq, durable: bool },
    /// Ephemeral path — not persisted.
    Volatile { seq: Seq },
    /// Dedupe hit — returned the pre-existing seq, no side effects.
    AlreadyExists { seq: Seq },
}

impl AppendAck {
    pub fn seq(&self) -> Seq {
        match self {
            Self::Committed { seq, .. } | Self::Volatile { seq } | Self::AlreadyExists { seq } => {
                *seq
            }
        }
    }
}

/// Failure modes surfaced to producers. Frozen lisp §4.1 `AppendError`.
#[derive(Debug, thiserror::Error)]
pub enum AppendError {
    /// Channel full — producer decides whether to retry or drop.
    #[error("append channel saturated")]
    Backpressure,

    /// Causation depth exceeded `MAX_CAUSATION_DEPTH`.
    #[error("causal loop detected at depth {depth}")]
    CausalLoop { depth: u8 },

    /// Writer is in its failed state (DB unreachable past retry cap) or the
    /// writer task has shut down.
    #[error("log writer unavailable: {0}")]
    LogUnavailable(String),

    /// Unexpected serialization error. Should be unreachable for
    /// well-formed `DomainEvent` impls.
    #[error("serialize event: {0}")]
    Serialize(#[from] serde_json::Error),

    /// Catch-all for everything else (blob store down, etc.).
    #[error("{0}")]
    Other(String),
}

/// Frozen lisp §4.4 causation-loop-guard.
pub const MAX_CAUSATION_DEPTH: u8 = 10;

/// Errors returned by read-side APIs.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("db: {0}")]
    #[cfg(feature = "postgres")]
    Db(#[from] sqlx::Error),

    #[error("decode event payload: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

/// The single surface producers and subscribers use.
///
/// Frozen lisp §4.1 append-api + §4.2.b reader surface.
#[async_trait]
pub trait Log: Send + Sync {
    /// Submit a domain event for persistence. Returns once the event is
    /// durable (or the path — dedupe / volatile — is otherwise resolved).
    async fn append<E>(&self, event: E, opts: AppendOpts) -> Result<AppendAck, AppendError>
    where
        E: DomainEvent;

    /// Pull-based catch-up. Returns up to `limit` events with `seq > after_seq`
    /// in a given domain, ordered by seq ascending.
    async fn read_from(
        &self,
        domain: Domain,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, LogError>;

    /// Current head `seq` in the log (max across all domains). Used by
    /// `StartFrom::Latest` subscribers.
    async fn head_seq(&self) -> Result<Seq, LogError>;
}

/// Read-only slice of [`Log`] used by the subscription layer.
///
/// Split out because [`Log::append`] is generic and therefore prevents
/// dyn-compatibility. The subscription only ever needs `read_from` and
/// `head_seq`; making this a separate trait keeps the producer-facing
/// `Log::append<E>` zero-cost while allowing `Arc<dyn LogReadable>` to
/// flow through the subscription runtime.
#[async_trait]
pub trait LogReadable: Send + Sync {
    async fn read_from(
        &self,
        domain: Domain,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, LogError>;

    async fn head_seq(&self) -> Result<Seq, LogError>;
}

/// Blanket: every [`Log`] is automatically a [`LogReadable`].
#[async_trait]
impl<T> LogReadable for T
where
    T: Log + ?Sized,
{
    async fn read_from(
        &self,
        domain: Domain,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, LogError> {
        Log::read_from(self, domain, after_seq, limit).await
    }

    async fn head_seq(&self) -> Result<Seq, LogError> {
        Log::head_seq(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_conversions() {
        let s = Seq(42);
        let raw: i64 = s.into();
        assert_eq!(raw, 42);
        let back: Seq = raw.into();
        assert_eq!(s, back);
    }

    #[test]
    fn append_ack_exposes_seq() {
        assert_eq!(
            AppendAck::Committed {
                seq: Seq(3),
                durable: true
            }
            .seq(),
            Seq(3)
        );
        assert_eq!(AppendAck::Volatile { seq: Seq(4) }.seq(), Seq(4));
        assert_eq!(AppendAck::AlreadyExists { seq: Seq(5) }.seq(), Seq(5));
    }

    #[test]
    fn max_depth_matches_frozen_lisp() {
        assert_eq!(MAX_CAUSATION_DEPTH, 10);
    }
}
