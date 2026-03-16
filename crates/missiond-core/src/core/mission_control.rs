//! Mission Control - Main coordinator
//!
//! Unified management of task queue, slot configuration, agent processes, and inbox.

use super::{
    Inbox, PermissionConfig, PermissionPolicy, PermissionRule, SlotManager,
};
use crate::db::MissionDB;
use crate::types::{
    CreateTaskInput, EventType, InboxMessage, Slot, SlotConfig, SlotsConfig, Task, TaskStatus, TaskUpdate,
};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};
use uuid::Uuid;

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
    /// Database path
    pub db_path: PathBuf,
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
/// Main coordinator for task queue, slot configuration, and inbox.
/// Agent process lifecycle is managed by PTYManager (single source of truth).
pub struct MissionControl {
    db: Arc<MissionDB>,
    slot_manager: SlotManager,
    permission_policy: PermissionPolicy,
    inbox: Inbox,
    started: RwLock<bool>,
    #[allow(dead_code)]
    logs_dir: PathBuf,
    default_mode: RwLock<ExecutionMode>,
    slots_config_path: PathBuf,
}

impl MissionControl {
    /// Create a new MissionControl
    pub fn new(options: MissionControlOptions) -> Result<Self> {
        // Initialize database
        let db = Arc::new(MissionDB::open(&options.db_path)?);

        // Logs directory
        let logs_dir = options
            .logs_dir
            .unwrap_or_else(|| options.db_path.parent().unwrap().join("logs"));

        let default_mode = options.default_mode.unwrap_or(ExecutionMode::Batch);

        // Initialize components
        let slot_manager = SlotManager::new(Arc::clone(&db));
        let inbox = Inbox::new(Arc::clone(&db));

        // Load permission config
        let permission_config_path = options.permission_config_path.unwrap_or_else(|| {
            options
                .db_path
                .parent()
                .unwrap()
                .join("config")
                .join("permissions.yaml")
        });
        let permission_policy = PermissionPolicy::new(&permission_config_path);

        // Load slots config
        let slots_config_path = options.slots_config_path.clone();
        let mc = Self {
            db,
            slot_manager,
            permission_policy,
            inbox,
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

    /// Get a reference to the database
    pub fn db(&self) -> &MissionDB {
        &self.db
    }

    /// Get a shared Arc to the database (for DbExecutor construction).
    pub fn db_arc(&self) -> Arc<MissionDB> {
        Arc::clone(&self.db)
    }

    // ============ Task Operations ============

    /// Submit a task (async, returns immediately)
    pub fn submit(&self, role: &str, prompt: &str) -> Result<String> {
        let input = CreateTaskInput {
            role: role.to_string(),
            prompt: prompt.to_string(),
        };
        let task = self.create_task(input)?;
        Ok(task.id)
    }

    /// Synchronous ask — creates a task and returns its ID.
    /// Actual execution is dispatched via PTY by the caller (mission_ask handler).
    pub async fn ask_expert(
        &self,
        role: &str,
        question: &str,
        _timeout_ms: u64,
    ) -> Result<String> {
        let input = CreateTaskInput {
            role: role.to_string(),
            prompt: question.to_string(),
        };
        let task = self.create_task(input)?;
        Ok(task.id)
    }

    /// Create a task
    fn create_task(&self, input: CreateTaskInput) -> Result<Task> {
        let now = chrono::Utc::now().timestamp_millis();
        let task = Task {
            id: Uuid::new_v4().to_string(),
            role: input.role.clone(),
            prompt: input.prompt.clone(),
            status: TaskStatus::Queued,
            slot_id: None,
            session_id: None,
            result: None,
            error: None,
            created_at: now,
            started_at: None,
            finished_at: None,
        };

        if let Err(e) = self.db.insert_task(&task) {
            error!(task_id = %task.id, error = %e, "Failed to persist task to DB");
            return Err(anyhow!("Failed to create task: {}", e));
        }
        let data = serde_json::json!({ "role": input.role });
        if let Err(e) = self.db.insert_event(&task.id, EventType::TaskCreated, Some(&data), now) {
            error!(task_id = %task.id, error = %e, "Failed to persist task event");
            // Non-fatal: task row exists, event is supplementary
        }

        info!(task_id = %task.id, role = %input.role, "Task created");
        Ok(task)
    }

    /// Get task status
    pub fn get_status(&self, task_id: &str) -> Option<Task> {
        self.db.get_task(task_id).ok().flatten()
    }

    /// Cancel a task (DB status update only; PTY kill is caller's responsibility)
    pub async fn cancel(&self, task_id: &str) -> Result<bool> {
        let task = match self.db.get_task(task_id).ok().flatten() {
            Some(t) => t,
            None => return Ok(false),
        };

        let now = chrono::Utc::now().timestamp_millis();

        if task.status == TaskStatus::Queued || task.status == TaskStatus::Running {
            if let Err(e) = self.db.update_task(
                task_id,
                &TaskUpdate {
                    status: Some(TaskStatus::Cancelled),
                    finished_at: Some(now),
                    ..Default::default()
                },
            ) {
                error!(task_id = %task_id, error = %e, "Failed to cancel task");
            }
            return Ok(true);
        }

        Ok(false)
    }

    // ============ Inbox Operations ============

    /// Get inbox messages
    pub fn get_inbox(&self, unread_only: bool, limit: usize) -> Vec<InboxMessage> {
        self.inbox.get_messages(unread_only, limit)
    }

    /// Mark a message as read
    pub fn mark_inbox_read(&self, message_id: &str) {
        self.inbox.mark_read(message_id);
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

    // ============ Statistics ============

    /// Get statistics
    pub fn get_stats(&self) -> MissionStats {
        let slot_stats = self.slot_manager.get_stats();

        MissionStats {
            tasks: TaskStats {
                queued: self
                    .db
                    .get_tasks_by_status(TaskStatus::Queued)
                    .map(|v| v.len())
                    .unwrap_or(0),
                running: self
                    .db
                    .get_tasks_by_status(TaskStatus::Running)
                    .map(|v| v.len())
                    .unwrap_or(0),
                done: self
                    .db
                    .get_tasks_by_status(TaskStatus::Done)
                    .map(|v| v.len())
                    .unwrap_or(0),
                failed: self
                    .db
                    .get_tasks_by_status(TaskStatus::Failed)
                    .map(|v| v.len())
                    .unwrap_or(0),
            },
            slots: SlotStats {
                total: slot_stats.total,
                by_role: slot_stats.by_role,
            },
            inbox: InboxStats {
                unread: self.inbox.get_unread_count(),
            },
        }
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

/// Task statistics
#[derive(Debug, Clone)]
pub struct TaskStats {
    pub queued: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
}

/// Slot statistics
#[derive(Debug, Clone)]
pub struct SlotStats {
    pub total: usize,
    pub by_role: std::collections::HashMap<String, usize>,
}

/// Inbox statistics
#[derive(Debug, Clone)]
pub struct InboxStats {
    pub unread: usize,
}

/// Mission statistics
#[derive(Debug, Clone)]
pub struct MissionStats {
    pub tasks: TaskStats,
    pub slots: SlotStats,
    pub inbox: InboxStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_config(dir: &Path) -> (PathBuf, PathBuf) {
        let db_path = dir.join("mission.db");
        let slots_config_path = dir.join("slots.yaml");

        // Create slots config
        let slots_config = r#"
slots:
  - id: slot-1
    role: worker
    description: Test worker slot
  - id: slot-2
    role: specialist
    description: Test specialist slot
"#;
        std::fs::write(&slots_config_path, slots_config).unwrap();

        (db_path, slots_config_path)
    }

    #[tokio::test]
    async fn test_create_mission_control() {
        let dir = tempdir().unwrap();
        let (db_path, slots_config_path) = create_test_config(dir.path());

        let mc = MissionControl::new(MissionControlOptions {
            db_path,
            slots_config_path,
            permission_config_path: None,
            logs_dir: None,
            default_mode: None,
        })
        .unwrap();

        let slots = mc.list_slots();
        assert_eq!(slots.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_stats() {
        let dir = tempdir().unwrap();
        let (db_path, slots_config_path) = create_test_config(dir.path());

        let mc = MissionControl::new(MissionControlOptions {
            db_path,
            slots_config_path,
            permission_config_path: None,
            logs_dir: None,
            default_mode: None,
        })
        .unwrap();

        let stats = mc.get_stats();
        assert_eq!(stats.slots.total, 2);
    }

    #[tokio::test]
    async fn test_default_mode() {
        let dir = tempdir().unwrap();
        let (db_path, slots_config_path) = create_test_config(dir.path());

        let mc = MissionControl::new(MissionControlOptions {
            db_path,
            slots_config_path,
            permission_config_path: None,
            logs_dir: None,
            default_mode: Some(ExecutionMode::Pty),
        })
        .unwrap();

        assert_eq!(mc.get_default_mode().await, ExecutionMode::Pty);

        mc.set_default_mode(ExecutionMode::Batch).await;
        assert_eq!(mc.get_default_mode().await, ExecutionMode::Batch);
    }
}
