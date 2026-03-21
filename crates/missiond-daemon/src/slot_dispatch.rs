//! Per-slot dispatch guard — prevents concurrent dispatch to the same PTY slot.
//!
//! All code paths that send work to a PTY slot (mission_submit, dispatch_queued_submit_tasks,
//! autopilot board dispatch) must acquire the guard before checking idle state + sending.
//! This eliminates the race where two callers both find a slot idle and both send.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Non-blocking per-slot dispatch guard.
///
/// Usage:
/// ```ignore
/// if !state.slot_dispatch.try_acquire(slot_id) {
///     // Another caller is dispatching to this slot, skip
///     continue;
/// }
/// // ... check idle + send ...
/// state.slot_dispatch.release(slot_id);
/// ```
pub(crate) struct SlotDispatchGuard {
    flags: RwLock<HashMap<String, Arc<AtomicBool>>>,
}

impl SlotDispatchGuard {
    pub fn new() -> Self {
        Self {
            flags: RwLock::new(HashMap::new()),
        }
    }

    fn get_flag(&self, slot_id: &str) -> Arc<AtomicBool> {
        // Fast path: read lock
        {
            let flags = self.flags.read().unwrap();
            if let Some(flag) = flags.get(slot_id) {
                return flag.clone();
            }
        }
        // Slow path: create flag
        let mut flags = self.flags.write().unwrap();
        flags
            .entry(slot_id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    /// Try to acquire dispatch lock for a slot. Returns true if acquired.
    /// Non-blocking: returns immediately.
    pub fn try_acquire(&self, slot_id: &str) -> bool {
        self.get_flag(slot_id)
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Release dispatch lock for a slot.
    pub fn release(&self, slot_id: &str) {
        self.get_flag(slot_id).store(false, Ordering::SeqCst);
    }

    /// Try to acquire and return an RAII guard that auto-releases on drop.
    /// Returns None if the slot is already locked.
    pub fn try_acquire_guard(&self, slot_id: &str) -> Option<SlotAcquireGuard<'_>> {
        if self.try_acquire(slot_id) {
            Some(SlotAcquireGuard {
                dispatch: self,
                slot_id: slot_id.to_string(),
            })
        } else {
            None
        }
    }
}

/// RAII guard — automatically releases slot dispatch lock on Drop.
/// Eliminates manual release() calls and prevents lock leaks on early return or panic.
pub(crate) struct SlotAcquireGuard<'a> {
    dispatch: &'a SlotDispatchGuard,
    slot_id: String,
}

impl<'a> SlotAcquireGuard<'a> {
    pub fn slot_id(&self) -> &str {
        &self.slot_id
    }
}

impl Drop for SlotAcquireGuard<'_> {
    fn drop(&mut self) {
        self.dispatch.release(&self.slot_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_acquire_release() {
        let guard = SlotDispatchGuard::new();
        assert!(guard.try_acquire("slot-1"));
        assert!(!guard.try_acquire("slot-1")); // second acquire fails
        guard.release("slot-1");
        assert!(guard.try_acquire("slot-1")); // can acquire after release
    }

    #[test]
    fn test_independent_slots() {
        let guard = SlotDispatchGuard::new();
        assert!(guard.try_acquire("slot-1"));
        assert!(guard.try_acquire("slot-2")); // different slot, independent
        guard.release("slot-1");
        guard.release("slot-2");
    }
}
