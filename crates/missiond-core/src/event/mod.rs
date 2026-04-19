//! Event-bus schema + storage + routing layers.
//!
//! Frozen lisp: `.missiond/v2/intent-event-bus.lisp`.
//!
//! # Layout
//!
//! The `event/` tree is organized to mirror the frozen lisp sections 1:1:
//!
//! | Directory            | Frozen lisp section                              |
//! |----------------------|--------------------------------------------------|
//! | `domain.rs` + `event_trait.rs` + `events/` | §4.2 prerequisite event-types |
//! | `pipeline/step{1..7}_*/` | §4.2 core · 7-step flow                   |
//! | `blob_store/`        | services used by step-2 claim-check              |
//! | `log/`               | pull-read surface (§4.3 phase-1-bootstrap) + append API types |
//! | `dispatcher/`        | thin orchestrator composing step-5/6/7           |
//! | `subscription/`      | §4.3 egress                                      |
//! | `in_memory/`         | §4.4 testing-story — PG-free parity bus          |
//! | `metrics/`           | §4.4 observability                                |
//! | `lifecycle/`         | §4.2 lifecycle-maintenance — retention cron      |
//!
//! # Phase history
//!
//! Phase 1 added schema (12 domain enums + [`DomainEvent`] trait).
//! Phase 2 added storage — the single-writer `LogWriter` + the
//! [`blob_store::BlobStore`] claim-check layer.
//! Phase 3 added routing — tail loop + per-domain topic broadcast.
//! Phase 4 added egress — tail-and-pull subscriber + combinators.
//! Phase 5 added cross-cutting — causation guard + BusMetrics + InMemoryBus.
//! Phase 6-8 migrated all producers and subscribers off the v1 bus.
//! Phase 9 E2E-validated the migration.
//! Phase 10 reorganized the pipeline files into `pipeline/step{1..7}_*/`
//! with each step mapped 1:1 to the frozen lisp §4.2 execution-flow.

pub mod blob_store;
pub mod dispatcher;
pub mod domain;
pub mod event_trait;
pub mod events;
pub mod in_memory;
pub mod lifecycle;
pub mod log;
pub mod metrics;
pub mod pipeline;
pub mod subscription;

pub use domain::Domain;
pub use event_trait::DomainEvent;

// Flatten event re-exports so downstream crates can write
// `use missiond_core::event::{BoardEvent, LlmEvent}` etc.
pub use events::{
    BoardEvent, IncidentEvent, LlmEvent, MemoryEvent, MessageEvent, ObservabilityEvent, Provider,
    QuestionEvent, SessionEndStatus, SessionEvent, SlotEvent, SystemEvent, TaskEvent, WorkerEvent,
};

// Convenience re-exports for the storage layer. Specific types stay under
// `log::` / `blob_store::` to keep the surface focused.
pub use blob_store::{BlobBackend, BlobStore, BlobStoreError, PayloadRef};
pub use log::{AppendAck, AppendError, AppendOpts, Log, Seq, SpanContext};

// Routing layer. Topic/Dispatcher/ControlGate are the common surface;
// registry/tail internals stay under `dispatcher::` (which re-exports from
// `pipeline::step{5,6,7}_*`).
pub use dispatcher::{ControlGate, Dispatcher, DispatcherBuilder, NeverPaused, Topic};

// Egress layer. The subscription module exposes `subscribe` plus the
// `Ack<T>` handshake + combinators.
pub use subscription::{
    subscribe, Ack, BackoffKind, BatchSize, BatchedSubscription, CoalescingSubscription,
    Cursor, CursorFlush, CursorStore, DebouncedSubscription, FailurePolicy, FilteredSubscription,
    InMemoryCursorStore, InMemoryDlq, MappedSubscription, PauseBehavior, RateLimitedSubscription,
    StartFrom, SubscribeError, Subscription, SubscriptionOpts,
};

// Cross-cutting layer. Causation guard is re-exported from
// `pipeline::step1_guard` for legacy callers; `metrics` + `in_memory` unchanged.
pub use pipeline::step1_guard::{check_causation, MAX_CAUSATION_DEPTH};
pub use in_memory::{InMemoryBlobStore, InMemoryBus, InMemoryBusHandle, InMemoryControlGate, InMemoryLog};
pub use metrics::{
    spawn_bus_metrics_emitter, AtomicBusMetrics, BusMetrics, BusMetricsEmitterHandle,
    MetricsSnapshot, NoopMetrics,
};

// Back-compat: `event::guards::{check_causation, MAX_CAUSATION_DEPTH}` still
// resolves. Canonical home is `pipeline::step1_guard`.
pub mod guards {
    pub use crate::event::pipeline::step1_guard::*;
}
