//! GeminiCliController — PTY operations for Gemini CLI engine.
//!
//! Handles the specifics of Gemini CLI interaction:
//! - `/clear` context isolation (synchronous send, consumes Complete event)
//! - `@filepath` for large prompts (>32KB → temp file)
//! - OAuth by default, API Key injection for ephemeral slots
//! - Result from TextComplete.content (Gemini has no JSONL pipeline)
//! - Session ID: synthetic `pty-{slot_id}` (no JSONL UUID)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::{info, warn};

use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{ManagerEvent, PTYManager, PTYSpawnOptions, SessionState};
use missiond_core::types::CliEngine;

use super::controller::EngineController;
use super::register_slot_session;
use super::types::SlotTaskRequest;

/// Gemini model to use.
const GEMINI_MODEL: &str = "gemini-3.1-pro-preview";
/// Prompt size threshold for @file mode (bytes).
const FILE_MODE_THRESHOLD: usize = 32_000;
/// Timeout for /clear command (seconds).
const CLEAR_TIMEOUT_MS: u64 = 15_000;
/// Max time to wait for slot to reach Idle before sending prompt.
const PRE_SEND_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct GeminiCliController {
    pty: Arc<PTYManager>,
    store: Arc<dyn MissionStore>,
}

impl GeminiCliController {
    pub fn new(pty: Arc<PTYManager>, store: Arc<dyn MissionStore>) -> Self {
        Self { pty, store }
    }

    /// Wait for a slot to reach Idle state (event-driven with polling fallback).
    async fn wait_until_idle(&self, slot_id: &str, timeout: Duration) -> Result<()> {
        if self.pty.is_available(slot_id).await {
            return Ok(());
        }

        let deadline = tokio::time::Instant::now() + timeout;
        let mut rx = self.pty.subscribe();

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "Timeout ({}s) waiting for Gemini slot {} to reach Idle",
                    timeout.as_secs(),
                    slot_id
                ));
            }

            if self.pty.is_available(slot_id).await {
                return Ok(());
            }

            match tokio::time::timeout(remaining.min(Duration::from_millis(500)), rx.recv()).await {
                Ok(Ok(ManagerEvent::StateChange {
                    slot_id: ref id,
                    new_state: SessionState::Idle,
                    ..
                })) if id == slot_id => return Ok(()),
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => continue,
                Err(_) => continue,
            }
        }
    }

    /// Write prompt to temp file for @file mode. Returns (message_to_send, temp_path).
    fn prepare_file_prompt(prompt: &str) -> Result<(String, std::path::PathBuf)> {
        let dir = std::env::temp_dir().join("missiond-gemini-prompts");
        std::fs::create_dir_all(&dir)
            .map_err(|e| anyhow!("Failed to create prompt dir: {}", e))?;
        let path = dir.join(format!("{}.md", uuid::Uuid::new_v4().simple()));
        std::fs::write(&path, prompt)
            .map_err(|e| anyhow!("Failed to write prompt file: {}", e))?;
        Ok((format!("@{}", path.display()), path))
    }
}

#[async_trait]
impl EngineController for GeminiCliController {
    async fn is_alive(&self, slot_id: &str) -> bool {
        self.pty.is_running(slot_id).await
    }

    async fn spawn_and_register(
        &self,
        slot_id: &str,
        req: &SlotTaskRequest,
        is_ephemeral: bool,
    ) -> Result<String> {
        info!(slot_id, is_ephemeral, "GeminiCtrl: spawning");

        let pty_slot = missiond_core::pty::Slot {
            id: slot_id.to_string(),
            role: req.task_type.clone(),
            cwd: Some(req.cwd.clone()),
            engine: CliEngine::Gemini,
        };

        self.pty.init_slot(&pty_slot).await;

        // Build env: OAuth by default, API Key only for ephemeral
        let extra_env = HashMap::new();
        // Model is set via CLI flag in build_cli_command, not env
        // For ephemeral: API Key would be injected here (Phase 4 ApiKeyPool)

        self.pty
            .spawn(
                &pty_slot,
                PTYSpawnOptions {
                    auto_restart: !is_ephemeral,
                    wait_for_idle: true,
                    timeout_secs: Some(120),
                    mcp_config: None,
                    dangerously_skip_permissions: true, // Gemini CLI has no permission system
                    model: Some(GEMINI_MODEL.to_string()),
                    extra_env,
                },
            )
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
        // NOTE: No wait_until_idle here. For Gemini persistent slots, clear_context()
        // (called by GeminiCliSlotManager before ask) already guarantees the PTY is idle
        // via PTYManager::send("/clear"). For ephemeral slots, spawn_and_register uses
        // wait_for_idle:true. Adding wait_until_idle here would cause a deadlock:
        // send("/clear") returns → Idle event already broadcasted → subscribe too late → timeout.

        // 1. Handle large prompts: write to temp file, send @filepath
        let (message, _temp_file) = if prompt.len() >= FILE_MODE_THRESHOLD {
            let (msg, path) = Self::prepare_file_prompt(prompt)?;
            info!(slot_id, file = %path.display(), "GeminiCtrl: using @file mode");
            (msg, Some(path))
        } else {
            (prompt.to_string(), None)
        };

        // 2. Subscribe BEFORE send
        let mut rx = self.pty.subscribe();

        // 3. Send prompt
        self.pty
            .send_fire_and_forget(slot_id, &message)
            .await?;

        info!(
            slot_id,
            prompt_len = prompt.len(),
            "GeminiCtrl: prompt sent, waiting for TextComplete"
        );

        // 4. Event-driven state machine: wait for Thinking → TextComplete
        //    For Gemini, TextComplete.content IS the authoritative source
        //    (no separate JSONL pipeline like Claude Code).
        let deadline = tokio::time::Instant::now() + timeout;
        let mut saw_thinking = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                // Clean up temp file on timeout
                if let Some(ref path) = _temp_file {
                    let _ = std::fs::remove_file(path);
                }
                return Err(anyhow!(
                    "Timeout ({}s) waiting for Gemini response from slot {}",
                    timeout.as_secs(),
                    slot_id
                ));
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(ManagerEvent::StateChange {
                    slot_id: ref id,
                    new_state: SessionState::Thinking,
                    ..
                })) if id == slot_id => {
                    saw_thinking = true;
                }

                Ok(Ok(ManagerEvent::TextComplete {
                    slot_id: ref id,
                    content,
                    ..
                })) if id == slot_id && saw_thinking => {
                    info!(
                        slot_id,
                        content_len = content.len(),
                        "GeminiCtrl: TextComplete received"
                    );
                    // Clean up temp file
                    if let Some(ref path) = _temp_file {
                        let _ = std::fs::remove_file(path);
                    }
                    return Ok(content);
                }

                Ok(Ok(ManagerEvent::Exited {
                    slot_id: ref id,
                    exit_code,
                })) if id == slot_id => {
                    if let Some(ref path) = _temp_file {
                        let _ = std::fs::remove_file(path);
                    }
                    return Err(anyhow!(
                        "Gemini slot {} exited (code {}) during task execution",
                        slot_id,
                        exit_code
                    ));
                }

                Ok(Ok(_)) => continue,

                // Broadcast lag — check if slot already returned to Idle
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                    warn!(slot_id, skipped = n, "GeminiCtrl: broadcast lagged, checking state");
                    if self.pty.is_available(slot_id).await {
                        // Missed TextComplete. Try DB extraction as fallback.
                        let session_id = format!("pty-{}", slot_id);
                        if let Ok(Some(content)) =
                            self.store.get_last_assistant_content(&session_id).await
                        {
                            if !content.is_empty() {
                                if let Some(ref path) = _temp_file {
                                    let _ = std::fs::remove_file(path);
                                }
                                return Ok(content);
                            }
                        }
                    }
                    saw_thinking = true;
                    continue;
                }

                Ok(Err(_)) => {
                    if let Some(ref path) = _temp_file {
                        let _ = std::fs::remove_file(path);
                    }
                    return Err(anyhow!(
                        "Event channel closed while waiting for Gemini slot {}",
                        slot_id
                    ));
                }

                Err(_) => continue,
            }
        }
    }

    async fn clear_context(&self, slot_id: &str) -> Result<()> {
        // Use PTYManager::send() (synchronous) for /clear.
        // send() internally subscribes and waits for TextOutputEvent::Complete,
        // consuming the /clear response so it doesn't leak into the next prompt's
        // event stream.
        self.wait_until_idle(slot_id, PRE_SEND_IDLE_TIMEOUT).await?;
        let _ = self.pty.send(slot_id, "/clear", CLEAR_TIMEOUT_MS).await?;
        Ok(())
    }

    async fn destroy(&self, slot_id: &str) -> Result<()> {
        self.pty.kill(slot_id).await
    }
}
