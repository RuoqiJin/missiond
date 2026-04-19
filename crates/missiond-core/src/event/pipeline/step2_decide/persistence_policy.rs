//! Persistence policy — the `ephemeral` flag decides TTL / UI routing, not
//! "write vs skip DB".
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-2 decide
//! persistence-policy:
//!
//! > default 持久化 — 所有 append 默认写 event_log (ephemeral=false, 30 天 TTL)
//! > ephemeral AppendOpts.ephemeral=true 仍写 event_log 但标 ephemeral=true,
//! > 3 天 TTL 自动清理
//! > use-case ObservabilityEvent / HealthSnapshot / 高频心跳 默认 ephemeral=true
//!
//! # History note
//!
//! In Phase 2 the writer short-circuited `ephemeral=true` to a purely
//! in-process [`crate::event::log::AppendAck::Volatile`] (no DB round-trip).
//! That fast-path still exists today in
//! [`crate::event::pipeline::step3_commit::log_writer::LogWriterHandle::append`]
//! — see the `volatile_counter`. Phase 8 resolved `I007` by routing ephemeral
//! appends through the daemon `BusServices::publish_*` helpers, which choose
//! `ephemeral=true` per-variant (frozen lisp §4.4 observability.self-emission
//! contract: any event produced by observability must be ephemeral).
//!
//! The policy is therefore:
//!   * `AppendOpts::ephemeral=false` → DB row inserted with `ephemeral=false`,
//!     30-day retention (30d TTL in `event_log`).
//!   * `AppendOpts::ephemeral=true`  → DB row inserted with `ephemeral=true`,
//!     3-day retention (see `event::lifecycle::retention`).
//!
//! There is **no** third "don't persist at all" variant outside the
//! Phase-2-legacy volatile fast-path which remains for single-producer
//! ephemeral writes that predate the dispatcher.
//!
//! This module exposes a single helper so downstream step-3 code (and tests
//! inspecting the policy) have a named call site; the underlying flag is a
//! plain `bool` field on [`crate::event::log::AppendOpts`].

use crate::event::log::AppendOpts;

/// Inspect the policy that step-3 should honor for the given `AppendOpts`.
///
/// `PersistencePolicy::Ephemeral` ⇒ the row is written with
/// `event_log.ephemeral = true` (3-day TTL).
/// `PersistencePolicy::Durable`   ⇒ the row is written with
/// `event_log.ephemeral = false` (30-day TTL, the bus recovery baseline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistencePolicy {
    Ephemeral,
    Durable,
}

impl PersistencePolicy {
    #[inline]
    pub fn from_opts(opts: &AppendOpts) -> Self {
        if opts.ephemeral {
            Self::Ephemeral
        } else {
            Self::Durable
        }
    }

    #[inline]
    pub fn is_ephemeral(self) -> bool {
        matches!(self, Self::Ephemeral)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_opts_are_durable() {
        assert_eq!(
            PersistencePolicy::from_opts(&AppendOpts::default()),
            PersistencePolicy::Durable
        );
    }

    #[test]
    fn ephemeral_flag_selects_ephemeral_policy() {
        let opts = AppendOpts {
            ephemeral: true,
            ..Default::default()
        };
        assert!(PersistencePolicy::from_opts(&opts).is_ephemeral());
    }
}
