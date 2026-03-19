//! GeminiCliSlotManager — concurrency governance for Gemini CLI PTY slots.
//!
//! Dual-track lifecycle:
//! - **Persistent**: Per-slot Mutex for serial access + /clear context isolation.
//! - **Ephemeral**: Global Semaphore for concurrency control. Spawn → execute → kill.
//!
//! All PTY operations delegated to GeminiCliController.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::{Mutex, Semaphore};
use tracing::{info, warn};

use missiond_core::db::traits::MissionStore;
use missiond_core::types::{CliEngine, Lifecycle};

use crate::llm::gemini_driver::GeminiPtyDriver;

use super::controller::EngineController;
use super::gemini_controller::GeminiCliController;
use super::types::{EngineSlotManager, EngineStatus, SlotTaskRequest};

/// Max concurrent ephemeral Gemini CLI slots.
const EPHEMERAL_CAPACITY: usize = 2;

pub struct GeminiCliSlotManager {
    controller: GeminiCliController,
    /// Per-slot Mutex: ensures serial access to each persistent slot.
    persistent_locks: DashMap<String, Arc<Mutex<()>>>,
    /// Global concurrency limit for ephemeral slots.
    ephemeral_semaphore: Arc<Semaphore>,
}

impl GeminiCliSlotManager {
    pub fn new(driver: GeminiPtyDriver, store: Arc<dyn MissionStore>) -> Self {
        Self {
            controller: GeminiCliController::new(driver, store),
            persistent_locks: DashMap::new(),
            ephemeral_semaphore: Arc::new(Semaphore::new(EPHEMERAL_CAPACITY)),
        }
    }

    /// Execute on a persistent slot: lock → ensure running → /clear → ask → return.
    async fn execute_persistent(&self, req: &SlotTaskRequest) -> Result<String> {
        let slot_id = req
            .slot_id
            .as_deref()
            .ok_or_else(|| anyhow!("Persistent Gemini task requires slot_id"))?;

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

        // 3. /clear context isolation (Gemini-specific, persistent only)
        self.controller.clear_context(slot_id).await?;

        // 4. Send prompt and wait for response
        info!(
            slot_id,
            prompt_len = req.prompt.len(),
            "GeminiSlotMgr: sending to persistent slot"
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
            .map_err(|_| anyhow!("Gemini ephemeral semaphore closed"))?;

        let temp_id = format!("ephemeral-gemini-{}", uuid::Uuid::new_v4().simple());
        info!(
            slot_id = %temp_id,
            task_type = %req.task_type,
            "GeminiSlotMgr: spawning ephemeral slot"
        );

        // 2. Spawn and register
        self.controller
            .spawn_and_register(&temp_id, req, true)
            .await?;

        // 3. Execute (no /clear needed — fresh session is clean)
        let result = self
            .controller
            .ask(&temp_id, &req.prompt, req.timeout)
            .await;

        // 4. Kill (always)
        if let Err(e) = self.controller.destroy(&temp_id).await {
            warn!(slot_id = %temp_id, "GeminiSlotMgr: kill failed: {}", e);
        }

        result
    }
}

#[async_trait]
impl EngineSlotManager for GeminiCliSlotManager {
    fn engine_type(&self) -> CliEngine {
        CliEngine::Gemini
    }

    async fn execute(&self, task: &SlotTaskRequest) -> Result<String> {
        match task.lifecycle {
            Lifecycle::Persistent => self.execute_persistent(task).await,
            Lifecycle::OnDemand => self.execute_ephemeral(task).await,
        }
    }

    async fn status(&self) -> EngineStatus {
        EngineStatus {
            engine: CliEngine::Gemini,
            persistent_slots: self.persistent_locks.len(),
            ephemeral_active: EPHEMERAL_CAPACITY - self.ephemeral_semaphore.available_permits(),
            ephemeral_capacity: EPHEMERAL_CAPACITY,
        }
    }
}
