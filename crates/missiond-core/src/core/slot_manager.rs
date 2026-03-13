//! Slot Manager - Workstation configuration management
//!
//! Manages slot configurations. Process lifecycle is handled by PTYManager.

use crate::db::MissionDB;
use crate::types::{Slot, SlotConfig};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

/// Slot Manager
///
/// Manages workstation configurations (process lifecycle handled by PTYManager)
pub struct SlotManager {
    slots: Arc<RwLock<HashMap<String, Slot>>>,
    db: Arc<MissionDB>,
}

impl SlotManager {
    /// Create a new SlotManager
    pub fn new(db: Arc<MissionDB>) -> Self {
        Self {
            slots: Arc::new(RwLock::new(HashMap::new())),
            db,
        }
    }

    /// Load slot configurations
    pub fn load_slots(&self, configs: Vec<SlotConfig>) {
        let mut slots = self.slots.write().unwrap();

        for mut config in configs {
            // Apply default traits based on role if none explicitly configured
            config.apply_default_traits();

            // Restore session_id from database
            let saved_session_id = self.db.get_slot_session(&config.id).ok().flatten();

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

    /// Find a slot by role
    pub fn find_slot_by_role(&self, role: &str) -> Option<Slot> {
        let slots = self.slots.read().unwrap();
        slots.values().find(|s| s.config.role == role).cloned()
    }

    /// Find the first slot with a given trait
    pub fn find_slot_by_trait(&self, t: crate::types::SlotTrait) -> Option<Slot> {
        let slots = self.slots.read().unwrap();
        slots.values().find(|s| s.config.traits.contains(&t)).cloned()
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
            let _ = self.db.set_slot_session(slot_id, session_id);
            debug!(slot_id = %slot_id, session_id = %session_id, "Session updated");
        }
    }

    /// Reset a slot's session
    pub fn reset_session(&self, slot_id: &str) {
        let mut slots = self.slots.write().unwrap();

        if let Some(slot) = slots.get_mut(slot_id) {
            slot.session_id = None;
            self.db.clear_slot_session(slot_id);
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
                let saved_session_id = self.db.get_slot_session(&config.id).ok().flatten();
                let slot = Slot {
                    config: config.clone(),
                    session_id: saved_session_id,
                };
                slots.insert(config.id.clone(), slot);
                added.push(config.id.clone());
                info!(slot_id = %config.id, role = %config.role, "Slot added (hot-reload)");
            }
        }

        SlotReloadResult { added, removed, updated }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_db() -> Arc<MissionDB> {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        Arc::new(MissionDB::open(db_path).unwrap())
    }

    #[test]
    fn test_load_and_get_slots() {
        let db = create_test_db();
        let manager = SlotManager::new(db);

        let configs = vec![
            SlotConfig {
                id: "slot-1".to_string(),
                role: "worker".to_string(),
                description: "Worker slot".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
            SlotConfig {
                id: "slot-2".to_string(),
                role: "worker".to_string(),
                description: "Another worker".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
            SlotConfig {
                id: "slot-3".to_string(),
                role: "specialist".to_string(),
                description: "Specialist slot".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
        ];

        manager.load_slots(configs);

        // Test get_all_slots
        let all = manager.get_all_slots();
        assert_eq!(all.len(), 3);

        // Test get_slot
        let slot = manager.get_slot("slot-1").unwrap();
        assert_eq!(slot.config.role, "worker");

        // Test get_slots_by_role
        let workers = manager.get_slots_by_role("worker");
        assert_eq!(workers.len(), 2);

        // Test find_slot_by_role
        let specialist = manager.find_slot_by_role("specialist").unwrap();
        assert_eq!(specialist.config.id, "slot-3");
    }

    #[test]
    fn test_session_management() {
        let db = create_test_db();
        let manager = SlotManager::new(db);

        let configs = vec![SlotConfig {
            id: "slot-1".to_string(),
            role: "worker".to_string(),
            description: "Worker slot".to_string(),
            engine: Default::default(),
            cwd: None,
            mcp_config: None,
            lifecycle: None,
            auto_start: None,
            dangerously_skip_permissions: None,
            model: None,
            traits: vec![],
            env: None,
        }];

        manager.load_slots(configs);

        // Initially no session
        let slot = manager.get_slot("slot-1").unwrap();
        assert!(slot.session_id.is_none());

        // Update session
        manager.update_session("slot-1", "session-abc");
        let slot = manager.get_slot("slot-1").unwrap();
        assert_eq!(slot.session_id, Some("session-abc".to_string()));

        // Reset session
        manager.reset_session("slot-1");
        let slot = manager.get_slot("slot-1").unwrap();
        assert!(slot.session_id.is_none());
    }

    #[test]
    fn test_reload_slots() {
        let db = create_test_db();
        let manager = SlotManager::new(db);

        // Initial load
        manager.load_slots(vec![
            SlotConfig {
                id: "slot-1".to_string(),
                role: "worker".to_string(),
                description: "Worker 1".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
            SlotConfig {
                id: "slot-2".to_string(),
                role: "worker".to_string(),
                description: "Worker 2".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
        ]);

        // Set a session on slot-1 to verify it's preserved
        manager.update_session("slot-1", "session-xyz");

        // Reload: remove slot-2, add slot-3, update slot-1's role
        let result = manager.reload_slots(vec![
            SlotConfig {
                id: "slot-1".to_string(),
                role: "specialist".to_string(), // changed
                description: "Worker 1".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
            SlotConfig {
                id: "slot-3".to_string(),
                role: "coder".to_string(),
                description: "New coder".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
        ]);

        assert_eq!(result.added, vec!["slot-3"]);
        assert_eq!(result.removed, vec!["slot-2"]);
        assert_eq!(result.updated, vec!["slot-1"]);
        assert!(result.has_changes());

        // Verify slot-1 session preserved
        let slot1 = manager.get_slot("slot-1").unwrap();
        assert_eq!(slot1.session_id, Some("session-xyz".to_string()));
        assert_eq!(slot1.config.role, "specialist");

        // Verify slot-2 removed
        assert!(manager.get_slot("slot-2").is_none());

        // Verify slot-3 added
        let slot3 = manager.get_slot("slot-3").unwrap();
        assert_eq!(slot3.config.role, "coder");
    }

    #[test]
    fn test_stats() {
        let db = create_test_db();
        let manager = SlotManager::new(db);

        let configs = vec![
            SlotConfig {
                id: "slot-1".to_string(),
                role: "worker".to_string(),
                description: "Worker 1".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
            SlotConfig {
                id: "slot-2".to_string(),
                role: "worker".to_string(),
                description: "Worker 2".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
            SlotConfig {
                id: "slot-3".to_string(),
                role: "specialist".to_string(),
                description: "Specialist".to_string(),
                engine: Default::default(),
                cwd: None,
                mcp_config: None,
                lifecycle: None,
                auto_start: None,
                dangerously_skip_permissions: None,
                model: None,
                traits: vec![],
                env: None,
            },
        ];

        manager.load_slots(configs);

        let stats = manager.get_stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.by_role.get("worker"), Some(&2));
        assert_eq!(stats.by_role.get("specialist"), Some(&1));
    }
}
