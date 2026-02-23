//! Mission Control - Main coordinator
//!
//! Unified management of task queue, slot configuration, agent processes, and inbox.

use super::{
    Inbox, PermissionConfig, PermissionPolicy, PermissionRule, ProcessManager, SlotManager,
    SpawnOptions,
};
use crate::db::MissionDB;
use crate::types::{
    CreateTaskInput, EventType, InboxMessage, Slot, SlotsConfig, Task, TaskStatus, TaskUpdate,
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
/// Main coordinator for task queue, slot configuration, agent processes, and inbox.
pub struct MissionControl {
    db: Arc<MissionDB>,
    slot_manager: SlotManager,
    process_manager: ProcessManager,
    permission_policy: PermissionPolicy,
    inbox: Inbox,
    started: RwLock<bool>,
    #[allow(dead_code)]
    logs_dir: PathBuf,
    default_mode: RwLock<ExecutionMode>,
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
        let process_manager = ProcessManager::new(Arc::clone(&db), logs_dir.clone());
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
        let mc = Self {
            db,
            slot_manager,
            process_manager,
            permission_policy,
            inbox,
            started: RwLock::new(false),
            logs_dir,
            default_mode: RwLock::new(default_mode),
        };

        mc.load_slots_config(&options.slots_config_path)?;

        info!("MissionControl initialized");
        Ok(mc)
    }

    /// Load slots configuration
    fn load_slots_config(&self, config_path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(config_path)?;
        let config: SlotsConfig = serde_yaml::from_str(&content)?;

        // Load into SlotManager
        self.slot_manager.load_slots(config.slots.clone());

        // Initialize process state
        for slot_config in &config.slots {
            if let Some(slot) = self.slot_manager.get_slot(&slot_config.id) {
                self.process_manager.init_slot(&slot);
            }
        }

        info!(count = config.slots.len(), "Slots loaded");
        Ok(())
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

        // Shutdown all processes
        self.process_manager.shutdown().await;

        info!("MissionControl stopped");
        Ok(())
    }

    /// Get a reference to the database
    pub fn db(&self) -> &MissionDB {
        &self.db
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

    /// Synchronous ask (submit + wait)
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

        // Synchronous execution
        self.process_task(&task).await
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

        let _ = self.db.insert_task(&task);
        let data = serde_json::json!({ "role": input.role });
        let _ = self.db.insert_event(&task.id, EventType::TaskCreated, Some(&data), now);

        info!(task_id = %task.id, role = %input.role, "Task created");
        Ok(task)
    }

    /// Process a task
    async fn process_task(&self, task: &Task) -> Result<String> {
        // Find slots for the role
        let slots = self.slot_manager.get_slots_by_role(&task.role);
        if slots.is_empty() {
            return Err(anyhow!("No slot found for role: {}", task.role));
        }

        // Find an available slot (idle)
        let mut target_slot: Option<Slot> = None;
        for slot in &slots {
            if self.process_manager.is_available(&slot.config.id) {
                target_slot = Some(slot.clone());
                break;
            }
        }

        // If no available slot, try to spawn one
        if target_slot.is_none() {
            for slot in &slots {
                if let Some(status) = self.process_manager.get_status(&slot.config.id) {
                    if status.status == super::AgentStatus::Stopped {
                        self.process_manager
                            .spawn(slot, SpawnOptions::default())
                            .await?;
                        target_slot = Some(slot.clone());
                        break;
                    }
                }
            }
        }

        let target_slot =
            target_slot.ok_or_else(|| anyhow!("No available slot for role: {}", task.role))?;

        let now = chrono::Utc::now().timestamp_millis();

        // Update task status
        let _ = self.db.update_task(
            &task.id,
            &TaskUpdate {
                status: Some(TaskStatus::Running),
                slot_id: Some(target_slot.config.id.clone()),
                started_at: Some(now),
                ..Default::default()
            },
        );

        let data = serde_json::json!({ "slotId": target_slot.config.id });
        let _ = self.db.insert_event(&task.id, EventType::TaskStarted, Some(&data), now);

        info!(task_id = %task.id, slot_id = %target_slot.config.id, "Task started");

        // Execute task
        match self.process_manager.execute_task(&target_slot, task).await {
            Ok(result) => {
                let now = chrono::Utc::now().timestamp_millis();

                // Update task status
                let _ = self.db.update_task(
                    &task.id,
                    &TaskUpdate {
                        status: Some(TaskStatus::Done),
                        session_id: Some(result.session_id.clone()),
                        result: Some(result.result.clone()),
                        finished_at: Some(now),
                        ..Default::default()
                    },
                );

                let data = serde_json::json!({ "resultLength": result.result.len() });
                let _ = self.db.insert_event(&task.id, EventType::TaskDone, Some(&data), now);

                // Add to inbox
                self.inbox.add_message(&task.id, &task.role, &result.result);

                info!(task_id = %task.id, "Task completed");
                Ok(result.result)
            }
            Err(e) => {
                let error_msg = e.to_string();
                let now = chrono::Utc::now().timestamp_millis();

                let _ = self.db.update_task(
                    &task.id,
                    &TaskUpdate {
                        status: Some(TaskStatus::Failed),
                        error: Some(error_msg.clone()),
                        finished_at: Some(now),
                        ..Default::default()
                    },
                );

                let data = serde_json::json!({ "error": error_msg });
                let _ = self.db.insert_event(&task.id, EventType::TaskFailed, Some(&data), now);

                error!(task_id = %task.id, error = %error_msg, "Task failed");
                Err(e)
            }
        }
    }

    /// Get task status
    pub fn get_status(&self, task_id: &str) -> Option<Task> {
        self.db.get_task(task_id).ok().flatten()
    }

    /// Cancel a task
    pub async fn cancel(&self, task_id: &str) -> Result<bool> {
        let task = match self.db.get_task(task_id).ok().flatten() {
            Some(t) => t,
            None => return Ok(false),
        };

        let now = chrono::Utc::now().timestamp_millis();

        if task.status == TaskStatus::Queued {
            let _ = self.db.update_task(
                task_id,
                &TaskUpdate {
                    status: Some(TaskStatus::Cancelled),
                    finished_at: Some(now),
                    ..Default::default()
                },
            );
            return Ok(true);
        }

        if task.status == TaskStatus::Running {
            if let Some(slot_id) = &task.slot_id {
                self.process_manager.kill(slot_id).await?;
                let _ = self.db.update_task(
                    task_id,
                    &TaskUpdate {
                        status: Some(TaskStatus::Cancelled),
                        finished_at: Some(now),
                        ..Default::default()
                    },
                );
                return Ok(true);
            }
        }

        Ok(false)
    }

    // ============ Process Control ============

    /// Spawn an agent process
    pub async fn spawn_agent(
        &self,
        slot_id: &str,
        options: Option<SpawnOptions>,
    ) -> Result<super::AgentProcess> {
        let slot = self
            .slot_manager
            .get_slot(slot_id)
            .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;
        self.process_manager
            .spawn(&slot, options.unwrap_or_default())
            .await
    }

    /// Kill an agent process
    pub async fn kill_agent(&self, slot_id: &str) -> Result<()> {
        self.process_manager.kill(slot_id).await
    }

    /// Restart an agent process
    pub async fn restart_agent(
        &self,
        slot_id: &str,
        options: Option<SpawnOptions>,
    ) -> Result<super::AgentProcess> {
        let slot = self
            .slot_manager
            .get_slot(slot_id)
            .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;
        self.process_manager
            .restart(&slot, options.unwrap_or_default())
            .await
    }

    /// Get all agent statuses
    pub fn get_agents(&self) -> Vec<super::AgentProcess> {
        self.process_manager.get_all_status()
    }

    /// Get a specific agent's status
    pub fn get_agent(&self, slot_id: &str) -> Option<super::AgentProcess> {
        self.process_manager.get_status(slot_id)
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

    /// Reset a slot's session
    pub fn reset_slot_session(&self, slot_id: &str) {
        self.slot_manager.reset_session(slot_id);
    }

    // ============ Statistics ============

    /// Get statistics
    pub fn get_stats(&self) -> MissionStats {
        let process_stats = self.process_manager.get_stats();
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
            agents: AgentStats {
                total: process_stats.total,
                stopped: process_stats.stopped,
                idle: process_stats.idle,
                busy: process_stats.busy,
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

/// Agent statistics
#[derive(Debug, Clone)]
pub struct AgentStats {
    pub total: usize,
    pub stopped: usize,
    pub idle: usize,
    pub busy: usize,
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
    pub agents: AgentStats,
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

    #[tokio::test]
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
        assert_eq!(stats.agents.total, 2);
        assert_eq!(stats.agents.stopped, 2);
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
