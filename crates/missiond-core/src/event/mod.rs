//! Event-bus schema layer — 12 domain enums + `DomainEvent` trait.
//!
//! Frozen lisp: `.missiond/v2/intent-event-bus.lisp` §4.2.a.
//!
//! Phase 1 scope: type definitions only. No log / dispatcher / subscription
//! code — that lives in later phases (§4.2.b storage, §4.2.c routing,
//! §4.3 egress). Phase 8 deletes the v1 `DaemonEvent` god-enum; until then
//! this module coexists with `crates/missiond-daemon/src/event_bus.rs`.

pub mod domain;
pub mod event_trait;
pub mod events;

pub use domain::Domain;
pub use event_trait::DomainEvent;

// Flatten event re-exports so downstream crates can write
// `use missiond_core::event::{BoardEvent, LlmEvent}` etc.
pub use events::{
    BoardEvent, IncidentEvent, LlmEvent, MemoryEvent, MessageEvent, ObservabilityEvent, Provider,
    QuestionEvent, SessionEndStatus, SessionEvent, SlotEvent, SystemEvent, TaskEvent, WorkerEvent,
};
