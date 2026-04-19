//! Daemon → core `ControlGate` adapter.
//!
//! `missiond-core` defines `CtlDomain` and `ControlGate` without a back
//! dependency on `missiond-daemon` (per DC010). Here we implement the trait
//! on top of a `watch::Receiver<ControlTree>` so the dispatcher can consult
//! the pause state set via `ControlManager`.
//!
//! Mapping rules:
//!   * Core `CtlDomain::Memory` → v1 `CtlDomain::Memory`
//!   * Core `CtlDomain::Board`  → v1 `CtlDomain::Board`
//!   * Core `CtlDomain::Flow`   → v1 `CtlDomain::Flow`
//!   * Core `CtlDomain::Strategy` → v1 `CtlDomain::Strategy`
//!
//! Global pause is checked inside `ControlTree::is_domain_paused`; we simply
//! forward.

use missiond_core::event::dispatcher::{ControlGate, control_gate::CtlDomain as CoreCtlDomain};
use tokio::sync::watch;

use crate::control_tree::{ControlTree, CtlDomain as DaemonCtlDomain};

/// Adapter that exposes a daemon `watch::Receiver<ControlTree>` as the
/// dispatcher-side [`ControlGate`] trait.
#[derive(Clone)]
pub struct ControlTreeGate {
    rx: watch::Receiver<ControlTree>,
}

impl ControlTreeGate {
    pub fn new(rx: watch::Receiver<ControlTree>) -> Self {
        Self { rx }
    }

    fn map_core_to_daemon(d: CoreCtlDomain) -> DaemonCtlDomain {
        match d {
            CoreCtlDomain::Memory => DaemonCtlDomain::Memory,
            CoreCtlDomain::Flow => DaemonCtlDomain::Flow,
            CoreCtlDomain::Board => DaemonCtlDomain::Board,
            CoreCtlDomain::Strategy => DaemonCtlDomain::Strategy,
        }
    }
}

impl ControlGate for ControlTreeGate {
    fn is_ctl_domain_paused(&self, d: CoreCtlDomain) -> bool {
        let tree = self.rx.borrow();
        tree.is_domain_paused(Self::map_core_to_daemon(d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_tree::ControlTree;

    #[test]
    fn memory_pause_visible_through_adapter() {
        let mut tree = ControlTree::default();
        tree.domains.insert(DaemonCtlDomain::Memory, true);
        let (_tx, rx) = watch::channel(tree);
        let gate = ControlTreeGate::new(rx);
        assert!(gate.is_ctl_domain_paused(CoreCtlDomain::Memory));
        assert!(!gate.is_ctl_domain_paused(CoreCtlDomain::Board));
    }

    #[test]
    fn global_pause_propagates_through_adapter() {
        let mut tree = ControlTree::default();
        tree.global_paused = true;
        let (_tx, rx) = watch::channel(tree);
        let gate = ControlTreeGate::new(rx);
        // global_paused implies every CtlDomain is paused.
        assert!(gate.is_ctl_domain_paused(CoreCtlDomain::Memory));
        assert!(gate.is_ctl_domain_paused(CoreCtlDomain::Board));
        assert!(gate.is_ctl_domain_paused(CoreCtlDomain::Flow));
        assert!(gate.is_ctl_domain_paused(CoreCtlDomain::Strategy));
    }
}
