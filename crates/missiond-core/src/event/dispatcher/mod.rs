//! Dispatcher — orchestrator that composes pipeline steps 5-7 into a
//! runnable tail loop.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-5..7 live fan-out.
//!
//! This module owns only the orchestration surface (`Dispatcher`,
//! `DispatcherBuilder`, `register_all_domains`). The underlying implementations
//! were moved to their 7-step homes:
//!
//! | Concern                              | Canonical home                                    |
//! |--------------------------------------|---------------------------------------------------|
//! | tail loop + `TailSource`             | [`crate::event::pipeline::step5_tail`]            |
//! | control gate / `CtlDomain` mapping   | [`crate::event::pipeline::step6_gate`]            |
//! | per-topic broadcast + registry       | [`crate::event::pipeline::step7_fanout`]          |
//!
//! The re-exports below keep legacy `event::dispatcher::…` paths resolving
//! so callers (tests + daemon) don't churn; new code is encouraged to
//! import directly from the step module.
//!
//! Scope (unchanged from Phase 3 but re-stated):
//!
//!   * **live fan-out only** — the dispatcher tails `event_log`, routes rows
//!     to the matching [`Topic<T>`] and broadcasts `Arc<T>` to every live
//!     subscriber.
//!   * **O(1) state** — only `last_dispatched_seq` is tracked. Offline
//!     consumers are NOT backfilled by the dispatcher; they pull from the
//!     log themselves (Phase 4 subscription API).
//!   * **control-gate** — per-event [`ControlGate::is_domain_paused`] check
//!     before fan-out. Paused-domain events are silently skipped (still
//!     persisted in the log); resume is live-resume by default.
//!   * **fault isolation** — each [`Topic<T>`] owns its own
//!     `tokio::sync::broadcast` channel, so a slow subscriber on one topic
//!     can never stall another.

// Re-export step-5/6/7 symbols under the legacy `dispatcher::` namespace.
pub use crate::event::pipeline::step5_tail::{
    DispatchError, DispatchMetrics, TailError, TailSource, TAIL_BATCH_LIMIT, TAIL_POLL_INTERVAL,
};
#[cfg(feature = "postgres")]
pub use crate::event::pipeline::step5_tail::PgTailSource;
pub use crate::event::pipeline::step6_gate::{domain_to_ctl_domain, ControlGate, CtlDomain, NeverPaused};
pub use crate::event::pipeline::step7_fanout::{
    AnyTopic, Topic, TopicRegistry, TopicRegistryBuilder, TypedTopic, TOPIC_BUFFER_SIZE,
};

// Back-compat sub-module aliases so `event::dispatcher::control_gate::CtlDomain`
// and friends continue to resolve. Each alias points at its step-module home.
pub mod control_gate {
    pub use crate::event::pipeline::step6_gate::*;
}
pub mod registry {
    pub use crate::event::pipeline::step7_fanout::registry::*;
}
pub mod tail {
    pub use crate::event::pipeline::step5_tail::*;
}
pub mod topic {
    pub use crate::event::pipeline::step7_fanout::topic::*;
}

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::watch;

use super::blob_store::BlobStore;
use super::event_trait::DomainEvent;
use super::log::Seq;

/// The runtime dispatcher handle.
///
/// Clone-cheap: all fields are already behind `Arc`.
#[derive(Clone)]
pub struct Dispatcher {
    registry: Arc<TopicRegistry>,
    /// Last seq we have dispatched successfully; shared across the tail task
    /// and any admin probe.
    last_dispatched_seq: Arc<AtomicI64>,
}

impl Dispatcher {
    /// Access the `Topic<T>` for the domain that `T` represents. Callers
    /// typically pipe this through `Topic::subscribe()` for live fan-out.
    pub fn topic<T: DomainEvent>(&self) -> Topic<T> {
        self.registry
            .topic::<T>()
            .expect("domain topic registered; DispatcherBuilder must cover every Domain")
    }

    /// Snapshot of the last-dispatched seq. Tests and metrics use this.
    pub fn last_dispatched_seq(&self) -> Seq {
        Seq(self.last_dispatched_seq.load(Ordering::Acquire))
    }

    /// The topic registry, useful for admin / introspection.
    pub fn registry(&self) -> Arc<TopicRegistry> {
        self.registry.clone()
    }

    /// Run the tail-and-fan-out loop until `shutdown` fires or the log
    /// fails permanently. See [`crate::event::pipeline::step5_tail`] for the
    /// full contract.
    pub async fn run<G>(
        self,
        tail_source: Arc<dyn TailSource>,
        blob_store: Arc<dyn BlobStore>,
        control: G,
        shutdown: watch::Receiver<bool>,
    ) -> Result<DispatchMetrics, DispatchError>
    where
        G: ControlGate + Clone + 'static,
    {
        crate::event::pipeline::step5_tail::run_tail(
            tail_source,
            blob_store,
            self.registry,
            self.last_dispatched_seq,
            control,
            shutdown,
        )
        .await
    }
}

/// Builder that registers every domain topic up front.
///
/// Usage: chain a `register::<T>()` call per domain event, then `.build()`.
/// `build()` must be called exactly once.
pub struct DispatcherBuilder {
    registry: TopicRegistryBuilder,
}

impl DispatcherBuilder {
    pub fn new() -> Self {
        Self {
            registry: TopicRegistryBuilder::new(),
        }
    }

    /// Register a topic for the given domain event type `T`. Registration
    /// order doesn't matter.
    pub fn register<T: DomainEvent>(mut self) -> Self {
        self.registry = self.registry.register::<T>();
        self
    }

    /// Finalize the registry and return a runnable [`Dispatcher`].
    pub fn build(self) -> Dispatcher {
        let registry = self.registry.build();
        Dispatcher {
            registry: Arc::new(registry),
            last_dispatched_seq: Arc::new(AtomicI64::new(0)),
        }
    }
}

impl Default for DispatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience shortcut: register all built-in domains. Daemon code
/// typically uses this to avoid listing every `register::<T>()` call.
pub fn register_all_domains(builder: DispatcherBuilder) -> DispatcherBuilder {
    use super::events::*;
    builder
        .register::<SlotEvent>()
        .register::<BoardEvent>()
        .register::<TaskEvent>()
        .register::<QuestionEvent>()
        .register::<LlmEvent>()
        .register::<WorkerEvent>()
        .register::<MemoryEvent>()
        .register::<MessageEvent>()
        .register::<SessionEvent>()
        .register::<SystemEvent>()
        .register::<ObservabilityEvent>()
        .register::<IncidentEvent>()
        .register::<ExecutionEvent>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::domain::Domain;

    /// Smoke-check: `DispatcherBuilder` + `register_all_domains` covers every
    /// value of [`Domain`] and every `.topic::<T>()` resolves.
    #[test]
    fn register_all_domains_covers_every_domain() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        for d in Domain::ALL {
            assert!(
                dispatcher.registry.any_topic(d).is_some(),
                "domain {:?} not registered",
                d
            );
        }
    }

    #[test]
    fn dispatcher_topic_resolves_for_registered_domain() {
        use super::super::events::BoardEvent;

        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        let t = dispatcher.topic::<BoardEvent>();
        assert_eq!(t.domain(), Domain::Board);
    }

    #[test]
    fn initial_last_dispatched_seq_is_zero() {
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
        assert_eq!(dispatcher.last_dispatched_seq(), Seq(0));
    }
}
