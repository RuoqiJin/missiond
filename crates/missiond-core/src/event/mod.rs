//! Event-bus schema + storage + routing layers.
//!
//! Frozen lisp: `.missiond/v2/intent-event-bus.lisp`.
//!
//! Phase 1 added schema (12 domain enums + [`DomainEvent`] trait).
//! Phase 2 added storage — [`log::Log`] trait, [`log::LogWriter`] task, and
//! the [`blob_store::BlobStore`] claim-check layer.
//! Phase 3 adds routing — the [`dispatcher`] module tails the log and
//! fans out events to per-domain [`dispatcher::Topic<T>`] channels.
//! Phase 4 adds egress — the [`subscription`] module implements the
//! two-phase tail-and-pull subscriber with cursor persistence, failure
//! routing, and declarative combinators. Phase 8 deletes the v1
//! `DaemonEvent` god-enum; until then this module coexists with
//! `crates/missiond-daemon/src/event_bus.rs`.

pub mod blob_store;
pub mod dispatcher;
pub mod domain;
pub mod event_trait;
pub mod events;
pub mod log;
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

// Routing layer — Phase 3. Topic/Dispatcher/ControlGate are the common
// surface; registry/tail internals stay under `dispatcher::`.
pub use dispatcher::{ControlGate, Dispatcher, DispatcherBuilder, NeverPaused, Topic};

// Egress layer — Phase 4. The subscription module exposes `subscribe`
// plus the `Ack<T>` handshake + combinators.
pub use subscription::{
    subscribe, Ack, BackoffKind, BatchSize, BatchedSubscription, CoalescingSubscription,
    Cursor, CursorFlush, CursorStore, DebouncedSubscription, FailurePolicy, FilteredSubscription,
    InMemoryCursorStore, InMemoryDlq, MappedSubscription, PauseBehavior, RateLimitedSubscription,
    StartFrom, SubscribeError, Subscription, SubscriptionOpts,
};
