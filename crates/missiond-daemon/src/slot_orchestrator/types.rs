//! Core types for the SlotManager orchestration layer.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use missiond_core::types::{CliEngine, Lifecycle};

/// Task configuration registered in AgentSlotManager.
#[derive(Debug, Clone)]
pub struct SlotTaskConfig {
    pub task_type: String,
    pub engine: CliEngine,
    pub lifecycle: Lifecycle,
    /// Bound slot_id for Persistent lifecycle.
    pub slot_id: Option<String>,
    /// Slot role for ControlTree pause checks (e.g., "memory", "coder").
    pub role: Option<String>,
    /// Model override (e.g., "sonnet", "opus", "gemini-3.1-pro-preview").
    pub model: Option<String>,
    /// Max time to wait for LLM response.
    pub timeout: Duration,
    /// Working directory for the slot.
    pub cwd: PathBuf,
    /// Skip permission confirmations (for unattended slots).
    pub skip_permissions: bool,
}

/// Runtime request passed to EngineSlotManager::execute().
#[derive(Debug)]
pub struct SlotTaskRequest {
    pub task_type: String,
    pub prompt: String,
    pub timeout: Duration,
    pub slot_id: Option<String>,
    pub model: Option<String>,
    pub lifecycle: Lifecycle,
    pub cwd: PathBuf,
    pub skip_permissions: bool,
}

impl SlotTaskRequest {
    pub fn from_config(config: &SlotTaskConfig, prompt: &str) -> Self {
        Self {
            task_type: config.task_type.clone(),
            prompt: prompt.to_string(),
            timeout: config.timeout,
            slot_id: config.slot_id.clone(),
            model: config.model.clone(),
            lifecycle: config.lifecycle,
            cwd: config.cwd.clone(),
            skip_permissions: config.skip_permissions,
        }
    }
}

/// Health/queue status for an engine sub-manager.
#[derive(Debug, Clone)]
pub struct EngineStatus {
    pub engine: CliEngine,
    pub persistent_slots: usize,
    pub ephemeral_active: usize,
    pub ephemeral_capacity: usize,
}

/// Engine sub-manager trait. Implemented by ClaudeCodeSlotManager and GeminiCliSlotManager.
#[async_trait]
pub trait EngineSlotManager: Send + Sync {
    fn engine_type(&self) -> CliEngine;

    /// Execute a task: provision slot → send prompt → wait idle → DB extract → cleanup.
    async fn execute(&self, task: &SlotTaskRequest) -> Result<String>;

    /// Engine health & queue info.
    async fn status(&self) -> EngineStatus;
}
