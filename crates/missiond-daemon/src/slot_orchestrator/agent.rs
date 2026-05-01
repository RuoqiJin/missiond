//! AgentSlotManager — top-level task router.
//!
//! Routes task_type → engine sub-manager based on registry.
//! Business layer calls `execute(task_type, prompt)` — one line.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::RwLock;
use tracing::{info, warn};

use missiond_core::types::CliEngine;

use super::types::{EngineSlotManager, EngineStatus, SlotTaskConfig, SlotTaskRequest};
use crate::control_tree::{ControlManager, CtlProvider};

/// Map CliEngine → CtlProvider for pause checks.
fn engine_to_provider(engine: CliEngine) -> CtlProvider {
    match engine {
        CliEngine::ClaudeCode => CtlProvider::Sonnet, // Historical provider lane for Claude Code slots.
        CliEngine::Gemini => CtlProvider::Gemini,
        CliEngine::Codex => CtlProvider::Codex,
    }
}

/// Top-level slot orchestrator. Routes tasks to engine-specific sub-managers.
pub struct AgentSlotManager {
    /// task_type → config
    registry: RwLock<HashMap<String, SlotTaskConfig>>,
    /// engine → sub-manager
    managers: HashMap<CliEngine, Arc<dyn EngineSlotManager>>,
    /// ControlTree for pause checks.
    control: Arc<ControlManager>,
}

impl AgentSlotManager {
    pub fn new(
        managers: Vec<(CliEngine, Arc<dyn EngineSlotManager>)>,
        control: Arc<ControlManager>,
    ) -> Self {
        Self {
            registry: RwLock::new(HashMap::new()),
            managers: managers.into_iter().collect(),
            control,
        }
    }

    /// Register a task type with its slot configuration.
    pub async fn register(&self, config: SlotTaskConfig) -> Result<()> {
        // Verify engine manager exists
        if !self.managers.contains_key(&config.engine) {
            return Err(anyhow!(
                "No manager for engine {:?} (task: {})",
                config.engine,
                config.task_type
            ));
        }

        info!(
            task_type = %config.task_type,
            engine = ?config.engine,
            lifecycle = ?config.lifecycle,
            slot_id = config.slot_id.as_deref().unwrap_or("-"),
            model = config.model.as_deref().unwrap_or("-"),
            "SlotManager: task registered"
        );

        self.registry
            .write()
            .await
            .insert(config.task_type.clone(), config);
        Ok(())
    }

    /// Execute a task. Routes to the appropriate engine sub-manager.
    ///
    /// ```ignore
    /// let content = slot_manager.execute("arch_maintenance", &prompt).await?;
    /// ```
    pub async fn execute(&self, task_type: &str, prompt: &str) -> Result<String> {
        let config = self
            .registry
            .read()
            .await
            .get(task_type)
            .cloned()
            .ok_or_else(|| anyhow!("Task not registered: {}", task_type))?;

        // ControlTree pause check: respect provider-level and global pause.
        let provider = engine_to_provider(config.engine);
        let tree = self.control.current();
        if tree.is_provider_paused(provider) {
            warn!(
                task_type,
                provider = provider.as_str(),
                "SlotManager: task blocked — provider paused"
            );
            return Err(anyhow!(
                "Provider {} is paused (task: {})",
                provider.as_str(),
                task_type
            ));
        }

        // ControlTree pause check: respect slot_role-level pause.
        if let Some(ref role) = config.role {
            if tree.is_slot_role_paused(role) {
                warn!(
                    task_type,
                    role = role.as_str(),
                    "SlotManager: task blocked — slot_role paused"
                );
                return Err(anyhow!(
                    "Slot role {} is paused (task: {})",
                    role,
                    task_type
                ));
            }
        }

        let manager = self
            .managers
            .get(&config.engine)
            .ok_or_else(|| anyhow!("No manager for engine {:?}", config.engine))?;

        let request = SlotTaskRequest::from_config(&config, prompt);
        manager.execute(&request).await
    }

    /// Check if a task type is registered.
    #[allow(dead_code)]
    pub fn is_registered(&self, task_type: &str) -> bool {
        self.registry
            .try_read()
            .map(|r| r.contains_key(task_type))
            .unwrap_or(false)
    }

    /// Get all engine statuses.
    #[allow(dead_code)]
    pub async fn all_status(&self) -> Vec<EngineStatus> {
        let mut statuses = Vec::new();
        for mgr in self.managers.values() {
            statuses.push(mgr.status().await);
        }
        statuses
    }
}
