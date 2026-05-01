//! Slot Manager - Workstation configuration management
//!
//! Manages slot configurations. Process lifecycle is handled by PTYManager.
//! Post-v0.4.23 Stage 2E: session persistence is owned entirely by the async
//! `SlotStore` trait (PG backend); this manager is DB-free.

use crate::types::{Slot, SlotConfig};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

/// Slot Manager
///
/// Manages workstation configurations (process lifecycle handled by PTYManager)
pub struct SlotManager {
    slots: Arc<RwLock<HashMap<String, Slot>>>,
}

impl SlotManager {
    /// Create a new SlotManager (PG mode — no SQLite DB).
    pub fn new() -> Self {
        Self {
            slots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load slot configurations
    pub fn load_slots(&self, configs: Vec<SlotConfig>) {
        let mut slots = self.slots.write().unwrap();

        for mut config in configs {
            // Apply default traits based on role if none explicitly configured
            config.apply_default_traits();

            // Session restoration is handled by async SlotStore in PG mode.
            let saved_session_id: Option<String> = None;

            let traits_desc = if config.traits.is_empty() {
                String::new()
            } else {
                format!(" traits={:?}", config.traits)
            };
            info!(slot_id = %config.id, role = %config.role, "{}", traits_desc);

            let slot = Slot {
                config: config.clone(),
                session_id: saved_session_id,
            };
            slots.insert(config.id.clone(), slot);
        }
    }

    /// Get all slots
    pub fn get_all_slots(&self) -> Vec<Slot> {
        let slots = self.slots.read().unwrap();
        slots.values().cloned().collect()
    }

    /// Get a slot by ID
    pub fn get_slot(&self, slot_id: &str) -> Option<Slot> {
        let slots = self.slots.read().unwrap();
        slots.get(slot_id).cloned()
    }

    /// Retrieve the declarative pipeline category for a given slot.
    pub fn get_slot_category(&self, slot_id: &str) -> Option<String> {
        self.get_slot(slot_id)
            .and_then(|s| s.config.category.clone())
    }

    /// Discover slots by declarative trait (e.g. IsMetaAgent).
    pub fn find_slots_by_trait(&self, trait_to_find: crate::types::SlotTrait) -> Vec<Slot> {
        let slots = self.slots.read().unwrap();
        slots
            .values()
            .filter(|s| s.config.traits.contains(&trait_to_find))
            .cloned()
            .collect()
    }

    /// Find a slot by role
    pub fn find_slot_by_role(&self, role: &str) -> Option<Slot> {
        let slots = self.slots.read().unwrap();
        slots.values().find(|s| s.config.role == role).cloned()
    }

    /// Find the first slot with a given trait
    pub fn find_slot_by_trait(&self, t: crate::types::SlotTrait) -> Option<Slot> {
        let slots = self.slots.read().unwrap();
        slots
            .values()
            .find(|s| s.config.traits.contains(&t))
            .cloned()
    }

    /// Get all slots with a specific role
    pub fn get_slots_by_role(&self, role: &str) -> Vec<Slot> {
        let slots = self.slots.read().unwrap();
        slots
            .values()
            .filter(|s| s.config.role == role)
            .cloned()
            .collect()
    }

    /// Update a slot's session
    pub fn update_session(&self, slot_id: &str, session_id: &str) {
        let mut slots = self.slots.write().unwrap();

        if let Some(slot) = slots.get_mut(slot_id) {
            slot.session_id = Some(session_id.to_string());
            debug!(slot_id = %slot_id, session_id = %session_id, "Session updated");
        }
    }

    /// Reset a slot's session
    pub fn reset_session(&self, slot_id: &str) {
        let mut slots = self.slots.write().unwrap();

        if let Some(slot) = slots.get_mut(slot_id) {
            slot.session_id = None;
            info!(slot_id = %slot_id, "Session reset");
        }
    }

    /// Reload slots with diff-based update.
    /// Preserves session_id for existing slots. Returns what changed.
    pub fn reload_slots(&self, configs: Vec<SlotConfig>) -> SlotReloadResult {
        let mut slots = self.slots.write().unwrap();

        let new_ids: HashSet<String> = configs.iter().map(|c| c.id.clone()).collect();
        let old_ids: HashSet<String> = slots.keys().cloned().collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut updated = Vec::new();

        // Remove slots no longer in config
        for id in old_ids.difference(&new_ids) {
            slots.remove(id);
            removed.push(id.clone());
            info!(slot_id = %id, "Slot removed (hot-reload)");
        }

        // Add or update slots
        for mut config in configs {
            config.apply_default_traits();

            if let Some(existing) = slots.get_mut(&config.id) {
                // Update config, preserve session_id
                if existing.config != config {
                    existing.config = config.clone();
                    updated.push(config.id.clone());
                    info!(slot_id = %config.id, "Slot config updated (hot-reload)");
                }
            } else {
                // New slot
                let saved_session_id: Option<String> = None;
                let slot = Slot {
                    config: config.clone(),
                    session_id: saved_session_id,
                };
                slots.insert(config.id.clone(), slot);
                added.push(config.id.clone());
                info!(slot_id = %config.id, role = %config.role, "Slot added (hot-reload)");
            }
        }

        SlotReloadResult {
            added,
            removed,
            updated,
        }
    }

    /// Register a dynamic slot at runtime (dual-source merge).
    pub fn register_dynamic_slot(&self, mut config: SlotConfig) {
        config.apply_default_traits();
        let mut slots = self.slots.write().unwrap();
        info!(slot_id = %config.id, role = %config.role, "Dynamic slot registered");
        slots.insert(
            config.id.clone(),
            Slot {
                config,
                session_id: None,
            },
        );
    }

    /// Register a runtime-projected slot without persisting it to slots.yaml.
    pub fn register_runtime_slot(&self, mut config: SlotConfig) {
        config.apply_default_traits();
        let mut slots = self.slots.write().unwrap();
        info!(slot_id = %config.id, role = %config.role, "Runtime slot registered");
        slots.insert(
            config.id.clone(),
            Slot {
                config,
                session_id: None,
            },
        );
    }

    /// Unregister a dynamic slot (remove from runtime).
    pub fn unregister_dynamic_slot(&self, slot_id: &str) {
        let mut slots = self.slots.write().unwrap();
        if slots.remove(slot_id).is_some() {
            info!(slot_id = %slot_id, "Dynamic slot unregistered");
        }
    }

    /// Get statistics (config stats only, no process state)
    pub fn get_stats(&self) -> SlotStats {
        let slots = self.slots.read().unwrap();
        let mut by_role: HashMap<String, usize> = HashMap::new();

        for slot in slots.values() {
            *by_role.entry(slot.config.role.clone()).or_insert(0) += 1;
        }

        SlotStats {
            total: slots.len(),
            by_role,
        }
    }
}

/// Slot statistics
#[derive(Debug, Clone)]
pub struct SlotStats {
    pub total: usize,
    pub by_role: HashMap<String, usize>,
}

/// Result of a slot reload operation
#[derive(Debug, Clone)]
pub struct SlotReloadResult {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub updated: Vec<String>,
}

impl SlotReloadResult {
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || !self.updated.is_empty()
    }
}
