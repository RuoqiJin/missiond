//! GeminiPtyDriver — unified Gemini CLI PTY driver.
//!
//! Encapsulates all Gemini-specific PTY "dirty work" that was previously
//! duplicated between GeminiCliController (slot_orchestrator) and
//! GeminiPtyTransport (llm/gemini_pty.rs):
//!
//! - @file large prompt handling (>32KB → temp file → @filepath)
//! - /clear context isolation (synchronous send, consumes Complete event)
//! - Strict event-driven state machine (subscribe → send → Thinking → TextComplete)
//! - Auth error detection on spawn failure (screen text keyword matching)
//! - wait_until_idle (event-driven + 500ms poll fallback)
//!
//! Both consumers delegate all PTY operations to this driver, which in turn
//! operates exclusively through PTYManager (no private PTYSession instances).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tracing::{info, warn};

use missiond_core::pty::{ManagerEvent, PTYManager, PTYSpawnOptions, SessionState};
use missiond_core::types::CliEngine;

/// Default Gemini model.
const GEMINI_MODEL: &str = "gemini-3.1-pro-preview";
/// Prompt size threshold for @file mode (bytes).
const FILE_MODE_THRESHOLD: usize = 32_000;
/// Timeout for /clear command (ms).
const CLEAR_TIMEOUT_MS: u64 = 15_000;
/// Max time to wait for slot to reach Idle before sending prompt.
const PRE_SEND_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Auth-related keywords detected in PTY screen text during spawn failure.
/// PTYManager is engine-agnostic — auth detection is our responsibility.
const AUTH_ERROR_KEYWORDS: &[&str] = &[
    "authenticate",
    "authorization",
    "token expired",
    "sign in",
    "login",
    "oauth",
    "credentials",
    "opening authentication page",
];

/// RAII guard for temp file cleanup.
/// Automatically deletes the temp file when dropped, regardless of how
/// the function exits (normal return, `?`, panic). Prevents /tmp leaks.
struct TempFileGuard(Option<PathBuf>);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(ref path) = self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Unified Gemini CLI PTY driver.
///
/// Stateless — all session state lives in PTYManager.
/// Both GeminiCliController (slot orchestrator) and GeminiPtyTransport (router)
/// delegate PTY operations to this driver.
#[derive(Clone)]
pub struct GeminiPtyDriver {
    pty: Arc<PTYManager>,
}

impl GeminiPtyDriver {
    pub fn new(pty: Arc<PTYManager>) -> Self {
        Self { pty }
    }

    /// Expose PTYManager for is_running/is_available checks.
    pub fn pty(&self) -> &Arc<PTYManager> {
        &self.pty
    }

    /// Ensure Gemini CLI slot is alive. Spawns if not running.
    /// On spawn failure, checks PTY screen text for Auth error keywords.
    pub async fn ensure_spawned(
        &self,
        slot_id: &str,
        cwd: &Path,
        is_ephemeral: bool,
        model: Option<&str>,
    ) -> Result<()> {
        if self.pty.is_running(slot_id).await {
            return Ok(());
        }

        let pty_slot = missiond_core::pty::Slot {
            id: slot_id.to_string(),
            role: "gemini_worker".to_string(),
            cwd: Some(cwd.to_path_buf()),
            engine: CliEngine::Gemini,
        };

        self.pty.init_slot(&pty_slot).await;

        match self
            .pty
            .spawn(
                &pty_slot,
                PTYSpawnOptions {
                    auto_restart: !is_ephemeral,
                    wait_for_idle: true,
                    timeout_secs: Some(120),
                    mcp_config: None,
                    dangerously_skip_permissions: true,
                    model: Some(model.unwrap_or(GEMINI_MODEL).to_string()),
                    extra_env: std::collections::HashMap::new(),
                },
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                // PTYManager.spawn failed (likely timeout waiting for Idle).
                // Check screen text for auth-related keywords.
                let screen_text = self
                    .pty
                    .get_screen_text(slot_id)
                    .await
                    .unwrap_or_default()
                    .to_lowercase();

                for kw in AUTH_ERROR_KEYWORDS {
                    if screen_text.contains(kw) {
                        return Err(anyhow!(
                            "Gemini CLI OAuth authentication required. \
                             Run `gemini` manually to re-authenticate. \
                             Screen: {}",
                            screen_text.chars().take(500).collect::<String>()
                        ));
                    }
                }
                Err(e)
            }
        }
    }

    /// Wait for slot to reach Idle state (event-driven with 500ms poll fallback).
    pub async fn wait_until_idle(&self, slot_id: &str, timeout: Duration) -> Result<()> {
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
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => continue,
            }
        }
    }

    /// Clear Gemini CLI internal context via /clear command.
    /// Synchronous send — consumes the Complete event to keep event queue clean.
    pub async fn clear_context(&self, slot_id: &str) -> Result<()> {
        self.wait_until_idle(slot_id, PRE_SEND_IDLE_TIMEOUT).await?;
        let _ = self.pty.send(slot_id, "/clear", CLEAR_TIMEOUT_MS).await?;
        Ok(())
    }

    /// Send prompt and wait for Gemini's response (event-driven).
    ///
    /// Handles:
    /// - Large prompts (>32KB) via @file mode
    /// - Subscribe-before-send (race-free)
    /// - Strict Thinking → TextComplete state machine
    /// - RAII temp file cleanup (TempFileGuard)
    /// - Broadcast lag recovery
    pub async fn ask(
        &self,
        slot_id: &str,
        prompt: &str,
        timeout: Duration,
    ) -> Result<String> {
        // 1. Handle large prompts: write to temp file
        let (message, temp_file) = if prompt.len() >= FILE_MODE_THRESHOLD {
            let (msg, path) = Self::prepare_file_prompt(prompt)?;
            info!(slot_id, file = %path.display(), "GeminiDriver: @file mode");
            (msg, Some(path))
        } else {
            (prompt.to_string(), None)
        };

        // RAII guard: auto-delete temp file on ANY exit path
        let _temp_guard = TempFileGuard(temp_file);

        // 2. Subscribe BEFORE send (race-free)
        let mut rx = self.pty.subscribe();

        // 3. Send prompt
        self.pty.send_fire_and_forget(slot_id, &message).await?;

        info!(slot_id, prompt_len = prompt.len(), "GeminiDriver: prompt sent");

        // 4. Event-driven state machine
        let deadline = tokio::time::Instant::now() + timeout;
        let mut saw_thinking = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
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
                        "GeminiDriver: TextComplete received"
                    );
                    return Ok(content);
                }

                Ok(Ok(ManagerEvent::Exited {
                    slot_id: ref id,
                    exit_code,
                })) if id == slot_id => {
                    return Err(anyhow!(
                        "Gemini slot {} exited (code {}) during task",
                        slot_id,
                        exit_code
                    ));
                }

                Ok(Ok(_)) => continue,

                // Broadcast lag recovery
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                    warn!(
                        slot_id,
                        skipped = n,
                        "GeminiDriver: broadcast lagged, checking state"
                    );
                    if self.pty.is_available(slot_id).await {
                        return Err(anyhow!(
                            "Gemini slot {} returned to Idle but TextComplete was missed (lagged {})",
                            slot_id,
                            n
                        ));
                    }
                    saw_thinking = true;
                    continue;
                }

                Ok(Err(_)) => {
                    return Err(anyhow!(
                        "Event channel closed while waiting for Gemini slot {}",
                        slot_id
                    ));
                }

                Err(_) => continue,
            }
        }
    }

    // ── Private helpers ──

    /// Write prompt to temp file for @file mode. Returns (message_to_send, temp_path).
    fn prepare_file_prompt(prompt: &str) -> Result<(String, PathBuf)> {
        let dir = std::env::temp_dir().join("missiond-gemini-prompts");
        std::fs::create_dir_all(&dir)
            .map_err(|e| anyhow!("Failed to create prompt dir: {}", e))?;
        let path = dir.join(format!("{}.md", uuid::Uuid::new_v4().simple()));
        std::fs::write(&path, prompt)
            .map_err(|e| anyhow!("Failed to write prompt file: {}", e))?;
        Ok((format!("@{}", path.display()), path))
    }
}
