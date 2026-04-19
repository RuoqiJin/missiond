//! Programmable control gate for in-memory bus tests.
//!
//! Production control gates read their pause state from a
//! `watch::Receiver<ControlTree>` in `missiond-daemon`; unit tests need a
//! far simpler surface so they can toggle a given `CtlDomain` paused at
//! will.
//!
//! [`InMemoryControlGate`] is the simplest such impl: it wraps a
//! `Mutex<HashSet<CtlDomain>>` and lets tests `.pause(CtlDomain::Memory)`
//! or `.resume_all()` at any point.

use std::collections::HashSet;
use std::sync::Mutex;

use super::super::dispatcher::control_gate::{ControlGate, CtlDomain};

/// Clone-cheap, programmable [`ControlGate`] used by chaos / integration
/// tests.
#[derive(Debug, Default)]
pub struct InMemoryControlGate {
    paused: Mutex<HashSet<CtlDomain>>,
}

impl InMemoryControlGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a domain bucket paused. Events with that bucket are dropped by
    /// the dispatcher tail loop.
    pub fn pause(&self, d: CtlDomain) {
        self.paused.lock().unwrap().insert(d);
    }

    /// Release a single bucket.
    pub fn resume(&self, d: CtlDomain) {
        self.paused.lock().unwrap().remove(&d);
    }

    /// Release all buckets.
    pub fn resume_all(&self) {
        self.paused.lock().unwrap().clear();
    }

    /// Snapshot — used by tests to assert the gate state.
    pub fn snapshot(&self) -> Vec<CtlDomain> {
        self.paused.lock().unwrap().iter().copied().collect()
    }
}

impl ControlGate for InMemoryControlGate {
    fn is_ctl_domain_paused(&self, d: CtlDomain) -> bool {
        self.paused.lock().unwrap().contains(&d)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::domain::Domain;
    use super::*;

    #[test]
    fn empty_gate_pauses_nothing() {
        let gate = InMemoryControlGate::new();
        for d in Domain::ALL {
            assert!(!gate.is_domain_paused(d));
        }
    }

    #[test]
    fn pause_memory_then_resume() {
        let gate = InMemoryControlGate::new();
        gate.pause(CtlDomain::Memory);
        assert!(gate.is_domain_paused(Domain::Memory));
        assert!(!gate.is_domain_paused(Domain::Board));

        gate.resume(CtlDomain::Memory);
        assert!(!gate.is_domain_paused(Domain::Memory));
    }

    #[test]
    fn resume_all_clears_every_bucket() {
        let gate = InMemoryControlGate::new();
        gate.pause(CtlDomain::Memory);
        gate.pause(CtlDomain::Board);
        gate.resume_all();
        assert!(gate.snapshot().is_empty());
    }

    #[test]
    fn gate_is_clone_via_arc_and_retains_state() {
        use std::sync::Arc;
        let gate = Arc::new(InMemoryControlGate::new());
        gate.pause(CtlDomain::Memory);
        let clone = gate.clone();
        assert!(clone.is_domain_paused(Domain::Memory));
    }
}
