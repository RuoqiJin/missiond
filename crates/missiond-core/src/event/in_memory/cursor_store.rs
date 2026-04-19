//! Cursor store re-export for the in-memory bus.
//!
//! The Phase 4 [`super::super::subscription::InMemoryCursorStore`] already
//! provides the exact semantics we want for `InMemoryBus`. This module
//! just re-exports it so the `in_memory::` namespace is self-contained.

pub use super::super::subscription::InMemoryCursorStore;
