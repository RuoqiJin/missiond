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
use missiond_core::types::SharedProjectRegistry;
use missiond_core::LearnedPermissions;

use crate::context::v3_blueprint_runtime::{RouterRuntimeConfig, WorkstationRuntimeConfig};

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
use missiond_core::db::traits::MissionStore;
use std::collections::HashSet;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct GeminiPtyDriver {
    pty: Arc<PTYManager>,
    store: Arc<dyn MissionStore>,
    pty_session_uuids: Arc<RwLock<HashSet<String>>>,
    project_registry: SharedProjectRegistry,
    learned: Option<Arc<LearnedPermissions>>,
}

impl GeminiPtyDriver {
    pub fn new(
        pty: Arc<PTYManager>,
        store: Arc<dyn MissionStore>,
        pty_session_uuids: Arc<RwLock<HashSet<String>>>,
        project_registry: SharedProjectRegistry,
        learned: Option<Arc<LearnedPermissions>>,
    ) -> Self {
        Self {
            pty,
            store,
            pty_session_uuids,
            project_registry,
            learned,
        }
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
        let project_root = cwd.to_string_lossy();
        let runtime_config =
            WorkstationRuntimeConfig::load_for_project_root(Some(project_root.as_ref()))
                .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))?;
        let spawn_timeout_secs = runtime_config.dynamic_slot_spawn_timeout_secs();
        let router_config = RouterRuntimeConfig::load_for_project_root(Some(project_root.as_ref()))
            .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))?;
        let default_model = router_config.flow_gemini_model.as_str();

        let pty_slot = missiond_core::pty::Slot {
            id: slot_id.to_string(),
            role: "gemini_worker".to_string(),
            cwd: Some(cwd.to_path_buf()),
            engine: CliEngine::Gemini,
        };

        self.pty.init_slot(&pty_slot).await;

        // Force dumb terminal mode: disables React Ink TUI, making output
        // append-only (like Claude Code). This fixes two critical issues:
        // 1. IncrementalExtractor can't handle Ink's in-place redraws → ▀▀▀ garbage
        // 2. DA1 response leak (1;2c) from terminal capability queries
        let mut extra_env = std::collections::HashMap::new();
        extra_env.insert("TERM".to_string(), "dumb".to_string());
        extra_env.insert("FORCE_COLOR".to_string(), "0".to_string());
        extra_env.insert("NO_COLOR".to_string(), "1".to_string());

        match crate::slot_orchestrator::spawner::spawn_tracked_slot(
            &self.pty,
            &self.store,
            &self.pty_session_uuids,
            &self.project_registry,
            self.learned.as_ref(),
            &pty_slot,
            PTYSpawnOptions {
                auto_restart: !is_ephemeral,
                wait_for_idle: true,
                timeout_secs: Some(spawn_timeout_secs),
                mcp_config: None,
                dangerously_skip_permissions: true,
                model: Some(model.unwrap_or(default_model).to_string()),
                extra_env,
                initial_prompt: None,
            },
            None, // No slot_env provided here, could be passed if needed
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
    pub async fn ask(&self, slot_id: &str, prompt: &str, timeout: Duration) -> Result<String> {
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

        info!(
            slot_id,
            prompt_len = prompt.len(),
            "GeminiDriver: prompt sent"
        );

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
                    slot_id: ref id, ..
                })) if id == slot_id && saw_thinking => {
                    // TextComplete is just a signal that the turn is done.
                    // Don't use its content — IncrementalExtractor mangles React Ink
                    // in-place redraws (captures only ▀▀▀ borders, drops actual text).
                    // Instead, capture the final screen frame and extract text from it.
                    let content = self.extract_from_screen(slot_id).await;
                    info!(
                        slot_id,
                        content_len = content.len(),
                        "GeminiDriver: TextComplete → screen extraction"
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

    /// Capture the final screen frame and extract Gemini's response text.
    /// Used instead of TextComplete.content because IncrementalExtractor
    /// mangles React Ink's in-place redraws (captures only ▀▀▀ borders).
    async fn extract_from_screen(&self, slot_id: &str) -> String {
        let screen = self.pty.get_screen_text(slot_id).await.unwrap_or_default();

        Self::sanitize_tui_output(&screen)
    }

    /// Strip Gemini CLI TUI artifacts from screen text.
    /// Removes box-drawing characters, block elements, sparkles, prompt lines,
    /// YOLO banner, workspace footer, and other Ink TUI decorations.
    fn sanitize_tui_output(raw: &str) -> String {
        raw.lines()
            .filter(|line| {
                let trimmed = line.trim();
                // Skip empty lines
                if trimmed.is_empty() {
                    return false;
                }
                // Skip lines that are purely box-drawing / block elements
                let stripped: String = trimmed
                    .chars()
                    .filter(|c| {
                        !matches!(
                            c,
                            '▀' | '▄'
                                | '█'
                                | '╭'
                                | '╮'
                                | '╰'
                                | '╯'
                                | '│'
                                | '─'
                                | '├'
                                | '┤'
                                | '┬'
                                | '┴'
                                | '┼'
                                | '┌'
                                | '┐'
                                | '└'
                                | '┘'
                                | '╌'
                                | '╍'
                        )
                    })
                    .collect();
                if stripped.trim().is_empty() {
                    return false;
                }
                // Skip Gemini CLI chrome lines
                if trimmed.starts_with("YOLO")
                    || trimmed.starts_with("workspace (")
                    || trimmed.starts_with("~/")
                    || trimmed.starts_with("Gemini CLI")
                    || trimmed.starts_with("Signed in")
                    || trimmed.starts_with("Plan:")
                    || trimmed.starts_with("We're making changes")
                    || trimmed.starts_with("What's Changing")
                    || trimmed.starts_with("How it affects")
                    || trimmed.starts_with("Read more:")
                    || trimmed.contains("GEMINI.md file")
                    || trimmed.contains("skills")
                    || trimmed.starts_with("/model")
                    || trimmed.starts_with("branch")
                    || trimmed.starts_with("sandbox")
                    || trimmed.starts_with("no sandbox")
                    || trimmed.starts_with("main")
                    || trimmed.starts_with("Auto (Gemini")
                {
                    return false;
                }
                // Skip prompt lines (> ... or * ...)
                if trimmed.starts_with("> ") || trimmed.starts_with("* ") {
                    // But keep if it looks like content (markdown list items)
                    if trimmed.starts_with("* ") && !trimmed.contains(";2c") {
                        return true;
                    }
                    return false;
                }
                // Skip DA response residue (1;2c...)
                if trimmed.contains(";2c") && !trimmed.contains(' ') {
                    return false;
                }
                true
            })
            .map(|line| {
                // Strip leading sparkle (✦) from content lines
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("✦") {
                    rest.trim_start().to_string()
                } else {
                    trimmed.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }

    /// Write prompt to temp file for @file mode. Returns (message_to_send, temp_path).
    fn prepare_file_prompt(prompt: &str) -> Result<(String, PathBuf)> {
        let dir = std::env::temp_dir().join("missiond-gemini-prompts");
        std::fs::create_dir_all(&dir).map_err(|e| anyhow!("Failed to create prompt dir: {}", e))?;
        let path = dir.join(format!("{}.md", uuid::Uuid::new_v4().simple()));
        std::fs::write(&path, prompt).map_err(|e| anyhow!("Failed to write prompt file: {}", e))?;
        Ok((format!("@{}", path.display()), path))
    }
}
