//! GeminiCliController — PTY operations for Gemini CLI engine.
//!
//! Thin wrapper around GeminiPtyDriver that implements EngineController trait.
//! Only adds DB session binding — all PTY operations delegated to Driver.
//!
//! - Result from TextComplete.content (Gemini has no JSONL pipeline)
//! - Session ID: synthetic `pty-{slot_id}` (no JSONL UUID)

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use tracing::info;

use missiond_core::db::traits::MissionStore;

use crate::llm::gemini_driver::GeminiPtyDriver;

use super::controller::EngineController;
use super::register_slot_session;
use super::types::SlotTaskRequest;

pub struct GeminiCliController {
    driver: GeminiPtyDriver,
    store: Arc<dyn MissionStore>,
}

impl GeminiCliController {
    pub fn new(driver: GeminiPtyDriver, store: Arc<dyn MissionStore>) -> Self {
        Self { driver, store }
    }
}

#[async_trait]
impl EngineController for GeminiCliController {
    async fn is_alive(&self, slot_id: &str) -> bool {
        self.driver.pty().is_running(slot_id).await
    }

    async fn spawn_and_register(
        &self,
        slot_id: &str,
        req: &SlotTaskRequest,
        is_ephemeral: bool,
    ) -> Result<String> {
        info!(slot_id, is_ephemeral, "GeminiCtrl: spawning via driver");

        self.driver
            .ensure_spawned(slot_id, Path::new(&req.cwd), is_ephemeral, req.model.as_deref())
            .await?;

        // Gemini has no JSONL — use synthetic session_id
        let session_id = format!("pty-{}", slot_id);
        register_slot_session(&self.store, slot_id, &session_id, is_ephemeral).await;

        info!(slot_id, session_id = %session_id, "GeminiCtrl: spawned and registered");
        Ok(session_id)
    }

    async fn ask(
        &self,
        slot_id: &str,
        prompt: &str,
        timeout: Duration,
    ) -> Result<String> {
        self.driver.ask(slot_id, prompt, timeout).await
    }

    async fn clear_context(&self, slot_id: &str) -> Result<()> {
        self.driver.clear_context(slot_id).await
    }

    async fn destroy(&self, slot_id: &str) -> Result<()> {
        self.driver.pty().kill(slot_id).await
    }
}
