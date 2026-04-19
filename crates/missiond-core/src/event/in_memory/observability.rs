//! [`ObservabilityAppender`] adapter for [`super::InMemoryLog`].
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 testing-story
//! in-memory-breakdown: `observability.rs` carries the narrowed
//! append path used by the metrics emitter to push ephemeral
//! `BusMetric` events through the in-memory bus without needing a
//! separate writer.
//!
//! [`ObservabilityAppender`]: super::super::metrics::emitter::ObservabilityAppender

use async_trait::async_trait;

use super::super::events::ObservabilityEvent;
use super::super::log::{AppendError, AppendOpts, Log, Seq};
use super::super::metrics::emitter::ObservabilityAppender;
use super::log::InMemoryLog;

#[async_trait]
impl ObservabilityAppender for InMemoryLog {
    async fn append_observability(
        &self,
        event: ObservabilityEvent,
        opts: AppendOpts,
    ) -> Result<Seq, AppendError> {
        let ack = self.append(event, opts).await?;
        Ok(ack.seq())
    }
}
