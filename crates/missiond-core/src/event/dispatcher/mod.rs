//! Topic dispatcher — live fan-out of appended events to in-process subscribers.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2.c topic-dispatcher.
//!
//! Scope (frozen):
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
//!
//! Phase 3 non-goals:
//!   * No subscription API (Phase 4). Today's tests call
//!     [`Topic::subscribe`] directly to exercise fan-out.
//!   * No `InMemoryBus` (Phase 5).
//!   * No retry / Lagged handling beyond the broadcast channel default; the
//!     subscription layer wraps those semantics in Phase 4.
//!   * No PG `LISTEN/NOTIFY` — the tail loop runs a long-polling SELECT
//!     every 100ms. See DC011 for rationale.
//!
//! # Wiring (Phase 8)
//!
//! The daemon will construct the dispatcher like:
//!
//! ```ignore
//! let dispatcher = register_all_domains(DispatcherBuilder::new()).build();
//! let ws_sub = dispatcher.topic::<BoardEvent>().subscribe();
//! tokio::spawn(dispatcher.run(tail_source, blob_store, gate, shutdown_rx));
//! ```

pub mod control_gate;
pub mod registry;
pub mod tail;
pub mod topic;

pub use control_gate::{ControlGate, NeverPaused, domain_to_ctl_domain};
pub use registry::{AnyTopic, TopicRegistry, TopicRegistryBuilder};
pub use tail::{DispatchError, DispatchMetrics, TailError, TailSource, TAIL_BATCH_LIMIT, TAIL_POLL_INTERVAL};
#[cfg(feature = "postgres")]
pub use tail::PgTailSource;
pub use topic::Topic;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::watch;

use super::blob_store::BlobStore;
use super::event_trait::DomainEvent;
use super::log::Seq;

/// Default broadcast buffer per topic — matches frozen lisp
/// §4.2.c `topic-registry.buffer-size`.
pub const TOPIC_BUFFER_SIZE: usize = 1024;

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
            .expect("domain topic registered; DispatcherBuilder must cover 12 domains")
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
    /// fails permanently. See [`tail`] for the full contract.
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
        tail::run_tail(
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

/// Builder that registers the 12 domain topics up front.
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

/// Convenience shortcut: register all 12 built-in domains. Daemon code
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::domain::Domain;

    /// Smoke-check: `DispatcherBuilder` + `register_all_domains` covers all 12
    /// values of [`Domain`] and every `.topic::<T>()` resolves.
    #[test]
    fn register_all_domains_covers_12_domains() {
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
