//! Cross-cutting guards — append-time safety checks that span all layers.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 cross-cutting.
//!
//! Currently implements:
//!
//! * [`causation::check_causation`] — rejects appends whose `causation_depth`
//!   exceeds [`causation::MAX_CAUSATION_DEPTH`]. Frozen lisp says:
//!
//!   > `causation-loop-guard` — prevent a consumer from processing an event
//!   > that triggers a further event it itself subscribes to.
//!
//! The guards are plain functions (not traits) — the Log / InMemoryBus
//! call them directly on the append hot path, so the overhead is one
//! `u8` comparison per append.

pub mod causation;

pub use causation::{check_causation, MAX_CAUSATION_DEPTH};
