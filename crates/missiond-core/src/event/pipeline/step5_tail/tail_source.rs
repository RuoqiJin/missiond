//! Tail-source abstraction + dispatch error types.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-5 tail
//! `tail-source`:
//!
//! > TailSource trait(抽象)+ DispatchError + TailError
//! > 抽象出 tail 数据来源,方便 InMemory / 未来 NATS 替代 PG
//!
//! The dispatcher reads **all domains** at once — it scans the whole log
//! forward. Per-domain filtering happens inside the consumer (Phase 4
//! subscription). This keeps the tail cursor O(1).

use async_trait::async_trait;

use crate::event::domain::Domain;
use crate::event::log::reader::LoggedEvent;
use crate::event::log::{LogError, Seq};

/// Abstraction over the log tail source. Production wires this to the
/// Phase 2 `LogReader`; unit tests plug a `Vec<LoggedEvent>` mock.
#[async_trait]
pub trait TailSource: Send + Sync {
    /// Return up to `limit` events with `seq > after_seq`, ordered by seq
    /// ascending. An empty Vec is normal (no new rows).
    async fn read_all_from(
        &self,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, TailError>;
}

/// Structured errors returned by [`super::dispatcher::run_tail`].
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("tail source error: {0}")]
    Tail(#[from] TailError),

    #[error(
        "unknown domain {domain:?} encountered in event_log tail — registry did not include it"
    )]
    UnknownDomain { domain: Domain },

    #[error("payload deserialize failed for domain {domain:?}: {err}")]
    PayloadDeserialize { domain: Domain, err: String },

    #[error("shutdown signal received")]
    Shutdown,
}

#[derive(Debug, thiserror::Error)]
pub enum TailError {
    #[error("log read error: {0}")]
    LogRead(String),
}

impl From<LogError> for TailError {
    fn from(e: LogError) -> Self {
        TailError::LogRead(e.to_string())
    }
}
