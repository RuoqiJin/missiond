//! Mission Control - Main coordinator
//!
//! Unified management of task queue, slot configuration, agent processes, and inbox.

use super::{
    PermissionConfig, PermissionPolicy, PermissionRule, SlotManager,
};
use crate::types::{Slot, SlotConfig, SlotsConfig};
use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::info;

/// Execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Batch mode: run claude -p
    Batch,
    /// PTY mode: interactive terminal
    Pty,
}

impl ExecutionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionMode::Batch => "batch",
            ExecutionMode::Pty => "pty",
        }
    }
}

/// Options for creating MissionControl
pub struct MissionControlOptions {
    /// Database path. None = PG mode (skip SQLite entirely).
    pub db_path: Option<PathBuf>,
    /// Slots configuration file path
    pub slots_config_path: PathBuf,
    /// Permission configuration file path (optional)
    pub permission_config_path: Option<PathBuf>,
    /// Logs directory (optional)
    pub logs_dir: Option<PathBuf>,
    /// Default execution mode
    pub default_mode: Option<ExecutionMode>,
}

/// Mission Control
///
/// Coordinator for slot configuration and permissions.
/// Task/inbox operations have been migrated to async MissionStore trait.
pub struct MissionControl {
    slot_manager: SlotManager,
    permission_policy: PermissionPolicy,
    started: RwLock<bool>,
    #[allow(dead_code)]
    logs_dir: PathBuf,
    default_mode: RwLock<ExecutionMode>,
    slots_config_path: PathBuf,
}

impl MissionControl {
    /// Create a new MissionControl (PG mode — no SQLite DB).
    pub fn new(options: MissionControlOptions) -> Result<Self> {
        // Logs directory
        let logs_dir = options.logs_dir.unwrap_or_else(|| {
            options.db_path.as_ref()
                .and_then(|p| p.parent())
                .unwrap_or_else(|| Path::new("."))
                .join("logs")
        });

        let default_mode = options.default_mode.unwrap_or(ExecutionMode::Batch);

        let slot_manager = SlotManager::new();

        // Load permission config
        let permission_config_path = options.permission_config_path.unwrap_or_else(|| {
            options.db_path.as_ref()
                .and_then(|p| p.parent())
                .unwrap_or_else(|| Path::new("."))
                .join("config")
                .join("permissions.yaml")
        });
        let permission_policy = PermissionPolicy::new(&permission_config_path);

        let slots_config_path = options.slots_config_path.clone();
        let mc = Self {
            slot_manager,
            permission_policy,
            started: RwLock::new(false),
            logs_dir,
            default_mode: RwLock::new(default_mode),
            slots_config_path: slots_config_path.clone(),
        };

        mc.load_slots_config(&slots_config_path)?;

        info!("MissionControl initialized");
        Ok(mc)
    }

    /// Load slots configuration (initial load)
    fn load_slots_config(&self, config_path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(config_path)?;
        let config: SlotsConfig = serde_yaml::from_str(&content)?;

        // Load into SlotManager
        self.slot_manager.load_slots(config.slots.clone());

        info!(count = config.slots.len(), "Slots loaded");
        Ok(())
    }

    pub fn get_slot_category(&self, slot_id: &str) -> Option<String> {
        self.slot_manager.get_slot_category(slot_id)
    }

    /// Reload slots configuration (hot-reload).
    /// Returns diff of what changed.
    pub fn reload_slots_config(&self) -> Result<super::SlotReloadResult> {
        let content = std::fs::read_to_string(&self.slots_config_path)?;
        let config: SlotsConfig = serde_yaml::from_str(&content)?;

        let result = self.slot_manager.reload_slots(config.slots);

        if result.has_changes() {
            info!(
                added = result.added.len(),
                removed = result.removed.len(),
                updated = result.updated.len(),
                "Slots reloaded"
            );
        } else {
            info!("Slots reload: no changes detected");
        }

        Ok(result)
    }

    /// Start the service
    pub async fn start(&self) -> Result<()> {
        let mut started = self.started.write().await;
        if *started {
            return Ok(());
        }
        *started = true;

        info!("MissionControl started");
        Ok(())
    }

    /// Stop the service
    pub async fn stop(&self) -> Result<()> {
        let mut started = self.started.write().await;
        if !*started {
            return Ok(());
        }
        *started = false;

        info!("MissionControl stopped");
        Ok(())
    }

    // ============ Slot Operations ============

    /// List all slots
    pub fn list_slots(&self) -> Vec<Slot> {
        self.slot_manager.get_all_slots()
    }

    /// Get a slot by ID
    pub fn get_slot(&self, slot_id: &str) -> Option<Slot> {
        self.slot_manager.get_slot(slot_id)
    }

    /// Reset a slot's session
    pub fn reset_slot_session(&self, slot_id: &str) {
        self.slot_manager.reset_session(slot_id);
    }

    /// Register a dynamic slot at runtime (dual-source merge with static slots).
    pub fn register_dynamic_slot(&self, config: SlotConfig) {
        self.slot_manager.register_dynamic_slot(config);
    }

    /// Unregister a dynamic slot (remove from runtime).
    pub fn unregister_dynamic_slot(&self, slot_id: &str) {
        self.slot_manager.unregister_dynamic_slot(slot_id);
    }

    /// Get default execution mode
    pub async fn get_default_mode(&self) -> ExecutionMode {
        *self.default_mode.read().await
    }

    /// Set default execution mode
    pub async fn set_default_mode(&self, mode: ExecutionMode) {
        *self.default_mode.write().await = mode;
        info!(mode = %mode.as_str(), "Default execution mode changed");
    }

    // ============ Permission Management ============

    /// Get permission configuration
    pub fn get_permission_config(&self) -> PermissionConfig {
        self.permission_policy.get_config()
    }

    /// Set role permission rule
    pub fn set_role_permission(&self, role: &str, rule: PermissionRule) {
        self.permission_policy.set_role_rule(role, rule);
        info!(role = %role, "Role permission updated");
    }

    /// Set slot permission rule
    pub fn set_slot_permission(&self, slot_id: &str, rule: PermissionRule) {
        self.permission_policy.set_slot_rule(slot_id, rule);
        info!(slot_id = %slot_id, "Slot permission updated");
    }

    /// Add role auto_allow
    pub fn add_role_auto_allow(&self, role: &str, pattern: &str) {
        self.permission_policy.add_role_auto_allow(role, pattern);
        info!(role = %role, pattern = %pattern, "Added role auto_allow");
    }

    /// Add slot auto_allow
    pub fn add_slot_auto_allow(&self, slot_id: &str, pattern: &str) {
        self.permission_policy.add_slot_auto_allow(slot_id, pattern);
        info!(slot_id = %slot_id, pattern = %pattern, "Added slot auto_allow");
    }

    /// Reload permission configuration
    pub fn reload_permission_config(&self) {
        self.permission_policy.reload();
        info!("Permission config reloaded");
    }

    /// Check tool permission
    pub fn check_permission(
        &self,
        slot_id: &str,
        role: &str,
        tool_name: &str,
    ) -> super::PermissionDecision {
        self.permission_policy
            .check_permission(slot_id, role, tool_name)
    }
}
