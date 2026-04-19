//! Type-resolve step — convert `impl DomainEvent` into the static domain,
//! `kind`, and payload-size hint consumed by the rest of the pipeline.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-1 guard:
//!
//! > type-resolve: 调用 DomainEvent::domain() / kind() / payload_size_hint()
//! > 准备下一步
//!
//! The trait methods themselves live on [`crate::event::DomainEvent`]. This
//! module is a thin wrapper that documents the semantic step and provides a
//! small `EventMetadata` helper used by the writer when it hands a pending
//! append to the batch loop. The writer currently computes those fields
//! inline (see `step3_commit::log_writer::Log::append`); keeping this module
//! separate gives future refactors a well-named anchor if type-resolve grows
//! (e.g. schema registry lookup, producer-token validation).

use crate::event::domain::Domain;
use crate::event::event_trait::DomainEvent;

/// Resolved metadata for a single append. Pulled out of the [`DomainEvent`]
/// trait on the hot path — zero-cost because every method is already
/// compile-time constant-folded by the trait implementations in `events/`.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedEventMeta {
    pub domain: Domain,
    pub kind: &'static str,
}

impl ResolvedEventMeta {
    /// Call the `DomainEvent` trait methods and capture their outputs.
    #[inline]
    pub fn from_event<E: DomainEvent>(event: &E) -> Self {
        Self {
            domain: E::domain(),
            kind: event.kind(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::BoardEvent;

    #[test]
    fn resolves_board_event_domain_and_kind() {
        let ev = BoardEvent::TaskCreated {
            task_id: "t-1".into(),
            title: "title".into(),
            category: "cat".into(),
        };
        let meta = ResolvedEventMeta::from_event(&ev);
        assert_eq!(meta.domain, Domain::Board);
        assert_eq!(meta.kind, "task_created");
    }
}
