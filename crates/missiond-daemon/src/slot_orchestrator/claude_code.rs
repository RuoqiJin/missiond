//! ClaudeCodeSlotManager — concurrency governance for Claude Code PTY slots.
//!
//! Dual-track lifecycle:
//! - **Persistent**: Per-slot Mutex for serial access.
//! - **Ephemeral**: Global Semaphore for concurrency control. Spawn → execute → kill.
//!
//! All PTY operations (spawn, send, wait, extract) are delegated to ClaudeCodeController.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::{Mutex, Semaphore};
use tracing::{info, warn};

use missiond_core::db::traits::MissionStore;
use missiond_core::pty::PTYManager;
use missiond_core::types::{CliEngine, Lifecycle};

use super::cc_controller::{ClaudeCodeController, PendingSpawns};
use super::controller::EngineController;
use super::types::{EngineSlotManager, EngineStatus, SlotTaskRequest};

/// Max concurrent ephemeral Claude Code slots.
const EPHEMERAL_CAPACITY: usize = 3;

pub struct ClaudeCodeSlotManager {
    controller: ClaudeCodeController,
    /// Per-slot Mutex: ensures serial access to each persistent slot.
    persistent_locks: DashMap<String, Arc<Mutex<()>>>,
    /// Global concurrency limit for ephemeral slots.
    ephemeral_semaphore: Arc<Semaphore>,
}

impl ClaudeCodeSlotManager {
    pub fn new(pty: Arc<PTYManager>, store: Arc<dyn MissionStore>, pending_spawns: PendingSpawns) -> Self {
        Self {
            controller: ClaudeCodeController::new(pty, store, pending_spawns),
            persistent_locks: DashMap::new(),
            ephemeral_semaphore: Arc::new(Semaphore::new(EPHEMERAL_CAPACITY)),
        }
    }

    /// Execute on a persistent slot: lock → ensure running → ask → return.
    async fn execute_persistent(&self, req: &SlotTaskRequest) -> Result<String> {
        let slot_id = req
            .slot_id
            .as_deref()
            .ok_or_else(|| anyhow!("Persistent task requires slot_id"))?;

        // 1. Per-slot Mutex: serial access
        let lock = self
            .persistent_locks
            .entry(slot_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        // 2. Ensure slot is alive (spawn if needed)
        if !self.controller.is_alive(slot_id).await {
            self.controller
                .spawn_and_register(slot_id, req, false)
                .await?;
        }

        // 3. Send prompt and wait for response (event-driven, race-free)
        info!(
            slot_id,
            prompt_len = req.prompt.len(),
            "ClaudeCodeSlotMgr: sending to persistent slot"
        );
        self.controller
            .ask(slot_id, &req.prompt, req.timeout)
            .await
    }

    /// Execute on an ephemeral slot: semaphore → spawn → ask → kill.
    async fn execute_ephemeral(&self, req: &SlotTaskRequest) -> Result<String> {
        // 1. Concurrency control
        let _permit = self
            .ephemeral_semaphore
            .acquire()
            .await
            .map_err(|_| anyhow!("Ephemeral semaphore closed"))?;

        let temp_id = format!("ephemeral-claude-{}", uuid::Uuid::new_v4().simple());
        info!(
            slot_id = %temp_id,
            task_type = %req.task_type,
            "ClaudeCodeSlotMgr: spawning ephemeral slot"
        );

        // 2. Spawn and register
        self.controller
            .spawn_and_register(&temp_id, req, true)
            .await?;

        // 3. Execute
        let result = self
            .controller
            .ask(&temp_id, &req.prompt, req.timeout)
            .await;

        // 4. Kill (always, regardless of success/failure)
        if let Err(e) = self.controller.destroy(&temp_id).await {
            warn!(slot_id = %temp_id, "ClaudeCodeSlotMgr: kill failed: {}", e);
        }

        result
    }
}

#[async_trait]
impl EngineSlotManager for ClaudeCodeSlotManager {
    fn engine_type(&self) -> CliEngine {
        CliEngine::ClaudeCode
    }

    async fn execute(&self, task: &SlotTaskRequest) -> Result<String> {
        match task.lifecycle {
            Lifecycle::Persistent => self.execute_persistent(task).await,
            Lifecycle::OnDemand => self.execute_ephemeral(task).await,
        }
    }

    async fn status(&self) -> EngineStatus {
        EngineStatus {
            engine: CliEngine::ClaudeCode,
            persistent_slots: self.persistent_locks.len(),
            ephemeral_active: EPHEMERAL_CAPACITY - self.ephemeral_semaphore.available_permits(),
            ephemeral_capacity: EPHEMERAL_CAPACITY,
        }
    }
}
