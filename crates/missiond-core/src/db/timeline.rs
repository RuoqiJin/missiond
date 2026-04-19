//! v1.3.0 SSOT cutover: legacy `system_timeline` operations removed.
//!
//! Reads now flow through `missiond_core::event::projection` which queries
//! `event_log`. Writes no longer exist — the table was dropped in migration
//! `20260420200000_drop_system_timeline.sql`.
//!
//! This file is retained (empty) because the SQLite gated `pub(crate) mod
//! timeline;` in `db/mod.rs` expects it. It carries the public re-exports
//! that were documented against it for back-compat.

pub use super::shared::{LatencyStats, TimelineRow, TimelineStats};
