//! Event-bus v2 wiring — daemon-side bootstrap for the Phase 1-5 bus
//! infrastructure defined in `crates/missiond-core/src/event/`.
//!
//! Frozen lisp: `.missiond/v2/intent-event-bus.lisp`.
//!
//! Phase 6 scope (producer migration):
//!   * Construct a [`BusServices`] at daemon start that aggregates every
//!     subsystem (`Log` / `BlobStore` / `CursorStore` / `Dispatcher` /
//!     `ControlGate` adapter / metrics).
//!   * Start the dispatcher + metrics emitter tasks.
//!   * Provide convenience `publish_*` methods so producer sites don't have
//!     to juggle [`AppendOpts`] boilerplate.
//!   * Provide a [`compat::publish_v1_shim`] bridge that lets the existing 83
//!     v1 publish sites dual-emit to v2 without rewriting every field.
//!
//! Phase 7 will start migrating subscribers — until then the v1
//! `event_bus::publish()` stays alongside and every publish site "dual-emits"
//! so no consumer gets cut off mid-refactor.

pub mod bootstrap;
pub mod compat;
pub mod control_gate_adapter;

pub use bootstrap::{BusServices, BusStartHandle};
pub use compat::{incident_reported, publish_v1_shim, spawn_v1_shim};
pub use control_gate_adapter::ControlTreeGate;
