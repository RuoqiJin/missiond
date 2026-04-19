//! Lifecycle maintenance — background tasks that are NOT on the append /
//! dispatch hot path.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 lifecycle-maintenance:
//!
//! > 不在 append/dispatch 主路径上的后台任务
//!
//! Currently one concern:
//!
//! * [`retention`] — TTL cleanup for `event_log` rows and expired blobs.
//!   Frozen lisp §4.2 retention: persistent rows 30 days, ephemeral rows
//!   3 days, blobs obey `ttl_expires`.
//!
//! The cron driver that invokes `retention::cleanup_once` lives in the
//! daemon — see `crates/missiond-daemon/src/bus/retention_cron.rs`.
//!
//! The sibling concern (orphan-cleanup for stale subscription cursors)
//! also lives in the daemon: the predicate query is built against
//! `event_subscriptions` in `retention_cron.rs`. If that query migrates
//! into `missiond-core` in the future, it belongs in this module.

pub mod retention;

pub use retention::{cleanup_once, CleanupReport};
