//! Control gate — the Domain → CtlDomain mapping that lets the dispatcher
//! consult `ControlTree::is_domain_paused` before fan-out.
//!
//! Frozen lisp §4.2.c `control-gate`:
//!
//! > Dispatcher 在派发前检查 ControlManager,暂停域不进 topic
//!
//! Phase 3 resolution of **I002** ("Domain→CtlDomain 映射未规定"):
//!
//! * The v1 `ControlTree::CtlDomain` is a **coarse** 4-bucket set
//!   (Memory / Flow / Board / Strategy) — it represents "functional area to
//!   pause", not event topic type.
//! * The v2 [`Domain`] is a fine-grained 12-value set — one per
//!   `DomainEvent` enum.
//! * Mapping is **many-to-one** and **partial**: some v2 domains have no
//!   v1 counterpart and are therefore **never** subject to domain gating.
//!   They remain gated by the coarser `global_paused` / `provider_paused`
//!   dimensions that the dispatcher does NOT (and must not) consult —
//!   frozen lisp §4.2.c keeps the dispatcher stateless w.r.t. pause
//!   policy; slot-level or worker-level pause is the subscriber's concern.
//!
//! Keep the mapping aligned with the 12 domain enum definitions in
//! `events/` — whenever a domain is added, update `domain_to_ctl_domain`.
//!
//! # Why the mapping is partial
//!
//! `Observability` and `Incident` must never be paused by a domain gate:
//! they're the bus's self-reporting channels. Silencing them silences the
//! very signals used to notice a stuck pause. See frozen lisp §4.4
//! `observability.self-emission`.
//!
//! For `Slot / Task / Question / Llm / Worker / Message / Session / System`,
//! v1 `CtlDomain` has no matching bucket — pausing Memory must not
//! accidentally silence unrelated telemetry. If future ops needs finer
//! control, add a new `CtlDomain` variant in v1 first, then update this
//! mapping.
//!
//! The daemon side adapts `watch::Receiver<ControlTree>` into a
//! [`ControlGate`] implementation. See Phase 8 wiring.

use crate::event::domain::Domain;

/// Functional pause bucket mirrored from
/// `missiond_daemon::control_tree::CtlDomain`.
///
/// Duplicated here so the dispatcher can stay in `missiond-core` without a
/// cycle back into `missiond-daemon`. The daemon-side adapter converts
/// `missiond_daemon::control_tree::CtlDomain` ↔ [`CtlDomain`] one-for-one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CtlDomain {
    Memory,
    Flow,
    Board,
    Strategy,
}

/// Map a v2 event [`Domain`] to its v1 [`CtlDomain`] pause bucket.
///
/// Returning `None` means "this domain is not controlled by the dispatcher
/// gate at all" — the event always fans out (subject to any other
/// subscriber-side policy).
///
/// Mapping table (stable contract — DO NOT tweak without updating the
/// frozen lisp or recording a decision):
///
/// | v2 [`Domain`]      | v1 `CtlDomain` |
/// |--------------------|----------------|
/// | Memory             | Memory         |
/// | Board              | Board          |
/// | others (11)        | None           |
///
/// * `Slot / Task / Question / Llm / Worker / Message / Session / System /
///   Execution` are not bound to a v1 pause bucket — pausing them would
///   require a new v1 bucket first. For now they are always delivered.
///   `Execution` carries `mission_execution` claim/audit projection; it is
///   operational state, not memory mutation, so it stays unbucketed.
/// * `Observability / Incident` are bus-self-report channels and must
///   never be pause-gated (frozen lisp §4.4).
/// * `Flow` and `Strategy` in v1 `CtlDomain` have no direct v2 `Domain`
///   counterpart today — they remain reachable through worker-level pause
///   (e.g. by pausing specific workers), not via dispatcher gate.
pub fn domain_to_ctl_domain(d: Domain) -> Option<CtlDomain> {
    match d {
        Domain::Memory => Some(CtlDomain::Memory),
        Domain::Board => Some(CtlDomain::Board),
        // See module doc — remaining 11 domains are intentionally not
        // gate-bound. When adding a new CtlDomain bucket, update this
        // match arm and the doc table above.
        Domain::Slot
        | Domain::Task
        | Domain::Question
        | Domain::Llm
        | Domain::Worker
        | Domain::Message
        | Domain::Session
        | Domain::System
        | Domain::Observability
        | Domain::Incident
        | Domain::Execution => None,
    }
}

/// Abstraction the dispatcher uses to consult the control tree without a
/// back-dependency on `missiond-daemon`.
///
/// Daemon-side implementations wrap `watch::Receiver<ControlTree>` and
/// hot-read the current snapshot on each event.
pub trait ControlGate: Send + Sync {
    /// Return `true` if a v1 [`CtlDomain`] bucket (or `global_paused`) is
    /// currently paused. The dispatcher consults this only after
    /// [`domain_to_ctl_domain`] returns `Some(_)`.
    fn is_ctl_domain_paused(&self, d: CtlDomain) -> bool;

    /// Convenience: project a v2 [`Domain`] into the effective pause state
    /// via [`domain_to_ctl_domain`]. Default implementation is correct —
    /// implementors rarely need to override.
    fn is_domain_paused(&self, d: Domain) -> bool {
        match domain_to_ctl_domain(d) {
            Some(ctl) => self.is_ctl_domain_paused(ctl),
            None => false,
        }
    }
}

// Blanket impl for Arc so callers can pass Arc<G: ControlGate>.
impl<T: ControlGate + ?Sized> ControlGate for std::sync::Arc<T> {
    fn is_ctl_domain_paused(&self, d: CtlDomain) -> bool {
        (**self).is_ctl_domain_paused(d)
    }
}

/// Trivial `ControlGate` impl used by tests + the "no control tree" boot
/// path. Nothing is ever paused.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverPaused;

impl ControlGate for NeverPaused {
    fn is_ctl_domain_paused(&self, _d: CtlDomain) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_maps_to_ctl_memory() {
        assert_eq!(
            domain_to_ctl_domain(Domain::Memory),
            Some(CtlDomain::Memory)
        );
    }

    #[test]
    fn board_maps_to_ctl_board() {
        assert_eq!(domain_to_ctl_domain(Domain::Board), Some(CtlDomain::Board));
    }

    #[test]
    fn observability_and_incident_never_gated() {
        assert_eq!(domain_to_ctl_domain(Domain::Observability), None);
        assert_eq!(domain_to_ctl_domain(Domain::Incident), None);
    }

    #[test]
    fn other_domains_return_none() {
        let expected_none = [
            Domain::Slot,
            Domain::Task,
            Domain::Question,
            Domain::Llm,
            Domain::Worker,
            Domain::Message,
            Domain::Session,
            Domain::System,
            Domain::Observability,
            Domain::Incident,
            Domain::Execution,
        ];
        for d in expected_none {
            assert_eq!(
                domain_to_ctl_domain(d),
                None,
                "{:?} should not have a CtlDomain mapping",
                d
            );
        }
    }

    #[test]
    fn mapping_is_total_over_domain_all() {
        // Every Domain value must have a decision — either Some or None.
        // A missing match arm would fail to compile, but a surprise
        // mapping would be caught here since the expected counts are
        // exact.
        let mapped: Vec<_> = Domain::ALL
            .iter()
            .copied()
            .filter(|d| domain_to_ctl_domain(*d).is_some())
            .collect();
        assert_eq!(mapped.len(), 2, "expected exactly 2 mapped domains");
        assert!(mapped.contains(&Domain::Memory));
        assert!(mapped.contains(&Domain::Board));
    }

    #[test]
    fn never_paused_returns_false_for_every_domain() {
        let gate = NeverPaused;
        for d in Domain::ALL {
            assert!(!gate.is_domain_paused(d), "{:?} should not be paused", d);
        }
    }

    /// Test-only gate that forces a specific CtlDomain to paused.
    struct PausedOnly(CtlDomain);
    impl ControlGate for PausedOnly {
        fn is_ctl_domain_paused(&self, d: CtlDomain) -> bool {
            d == self.0
        }
    }

    #[test]
    fn paused_memory_only_affects_memory_domain() {
        let gate = PausedOnly(CtlDomain::Memory);
        // Memory → paused
        assert!(gate.is_domain_paused(Domain::Memory));
        // Board → not paused (different CtlDomain bucket)
        assert!(!gate.is_domain_paused(Domain::Board));
        // Observability → not paused (no CtlDomain mapping)
        assert!(!gate.is_domain_paused(Domain::Observability));
        // Incident → not paused (no CtlDomain mapping)
        assert!(!gate.is_domain_paused(Domain::Incident));
        // Slot → not paused (no mapping)
        assert!(!gate.is_domain_paused(Domain::Slot));
    }
}
