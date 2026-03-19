//! PTY-based transport for Gemini CLI.
//!
//! Replaces the broken NDJSON subprocess mode (`gemini -o stream-json`)
//! with interactive PTY session (`gemini --yolo -m gemini-3.1-pro-preview`).
//!
//! Strategy: persistent session + `/clear` per call.
//! A single Gemini CLI PTY session is kept alive across calls. Before each prompt,
//! `/clear` resets Gemini's internal context (router_chat manages its own history).
//! The Mutex guarantees `/clear` + `send` is atomic — no other request can interleave.
//!
//! Concurrency safety (two layers):
//! 1. `GeminiClient` semaphore (capacity=1, with acquisition timeout) — queue management
//! 2. `GeminiPtyTransport` Mutex — defense-in-depth, atomic `/clear` + `send`
//!
//! Large prompts (>32KB) are written to a temp file and sent via `@filepath` syntax
//! to avoid PTY pipe buffer overflow (OS limit ~4-64KB per write).
//!
//! If the session dies or gets stuck, it is automatically respawned on the next call.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::sync::Mutex;
use tracing::{info, warn};

use missiond_core::pty::{PTYSession, PTYSessionOptions, SessionState};
use missiond_core::types::CliEngine;

const GEMINI_MODEL: &str = "gemini-3.1-pro-preview";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
/// Time to wait for `/clear` to complete and return to Idle.
const CLEAR_TIMEOUT: Duration = Duration::from_secs(15);
/// Time to wait for interrupt to bring session back to Idle.
const INTERRUPT_TIMEOUT: Duration = Duration::from_secs(5);
/// Prompt size threshold for switching to @file mode.
/// Below this: direct bracketed paste. Above: write to temp file + `@filepath`.
/// Conservative: well below the 64KB OS PTY buffer limit.
const FILE_MODE_THRESHOLD: usize = 32_000;
/// Temp directory for prompt files.
const PROMPT_TEMP_DIR: &str = "missiond-gemini-pty";

/// Auth-related keywords detected in PTY screen text during spawn failure.
/// If any match, return a specific auth error instead of generic timeout.
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

/// PTY-based Gemini CLI transport.
///
/// Holds a persistent `gemini --yolo -m gemini-3.1-pro-preview` PTY session.
/// Access is serialized via Mutex — `/clear` and `send` are always atomic.
pub(crate) struct GeminiPtyTransport {
    cwd: PathBuf,
    log_file: PathBuf,
    /// Mutex-protected session. Only one caller can interact at a time.
    /// This guarantees no `/clear` can hit a working session.
    session: Mutex<Option<PTYSession>>,
}

impl GeminiPtyTransport {
    pub fn new(cwd: PathBuf) -> Self {
        let log_file = missiond_core::default_mission_home()
            .join("logs")
            .join("pty-gemini-router.log");
        Self {
            cwd,
            log_file,
            session: Mutex::new(None),
        }
    }

    /// Spawn a fresh Gemini CLI PTY session and wait for Idle.
    async fn spawn_session(&self) -> Result<PTYSession> {
        info!("GeminiPty: spawning session");
        let mut env = std::collections::HashMap::new();
        env.insert("GEMINI_MODEL".to_string(), GEMINI_MODEL.to_string());

        let mut session = PTYSession::new(PTYSessionOptions {
            slot_id: "gemini-router".to_string(),
            cwd: self.cwd.clone(),
            env: Some(env),
            log_file: Some(self.log_file.clone()),
            cols: 200,
            rows: 50,
            engine: CliEngine::Gemini,
            mcp_config: None,
            dangerously_skip_permissions: false,
            model: Some(GEMINI_MODEL.to_string()),
        })?;

        session.start().await?;

        // Wait for Idle. If timeout, close() to prevent zombie PTY process leak.
        // Without explicit close(), the PTY process and background tasks (read_loop,
        // feed_loop, state_check_loop) survive session drop — leaking on every failed spawn.
        if let Err(e) = session
            .wait_for_state(SessionState::Idle, STARTUP_TIMEOUT)
            .await
        {
            // Check screen text for auth-related keywords before closing.
            // OAuth expiry causes Gemini CLI to show auth prompts instead of reaching Idle,
            // leading to infinite spawn-fail loops if not detected early.
            let screen_text = session.get_screen_text().await.to_lowercase();
            let _ = session.close().await;

            let is_auth = AUTH_ERROR_KEYWORDS.iter().any(|kw| screen_text.contains(kw));
            if is_auth {
                return Err(anyhow!(
                    "Gemini CLI: OAuth authentication required. Run `gemini` manually to re-authenticate. Screen: {}",
                    screen_text.chars().take(500).collect::<String>()
                ));
            }

            return Err(anyhow!(
                "Gemini CLI startup failed ({}s): {}",
                STARTUP_TIMEOUT.as_secs(),
                e
            ));
        }

        info!("GeminiPty: session ready");
        Ok(session)
    }

    /// Write prompt to a temp file for @file mode.
    /// Returns the file path. Caller is responsible for cleanup.
    fn write_prompt_file(prompt: &str) -> Result<PathBuf> {
        let tmp_dir = std::env::temp_dir().join(PROMPT_TEMP_DIR);
        std::fs::create_dir_all(&tmp_dir)
            .map_err(|e| anyhow!("Failed to create temp dir: {}", e))?;
        let file_path = tmp_dir.join(format!("{}.md", uuid::Uuid::new_v4()));
        std::fs::write(&file_path, prompt)
            .map_err(|e| anyhow!("Failed to write prompt file: {}", e))?;
        Ok(file_path)
    }

    /// Send a prompt via PTY and wait for Gemini's response.
    ///
    /// Flow (all under Mutex — atomic, no interleaving):
    ///   1. Ensure session alive + Idle (spawn/interrupt/respawn as needed)
    ///   2. `/clear` to reset Gemini's internal context (sync send, consumes Complete)
    ///   3. Send prompt: direct paste (<32KB) or @file (≥32KB)
    ///   4. Wait `TextOutputEvent::Complete`
    ///   5. Return response text
    pub async fn call(&self, prompt: &str, idle_timeout: Duration) -> Result<String> {
        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt"));
        }

        let use_file = prompt.len() >= FILE_MODE_THRESHOLD;
        info!(
            prompt_len = prompt.len(),
            timeout_secs = idle_timeout.as_secs(),
            file_mode = use_file,
            "GeminiPty: call"
        );

        let mut guard = self.session.lock().await;

        // ── Phase 1: Ensure session is alive and Idle ──

        let mut needs_spawn = !guard.as_ref().map_or(false, |s| s.is_running());

        if !needs_spawn {
            let session = guard.as_ref().unwrap();
            let state = session.state().await;
            if state != SessionState::Idle {
                // Session alive but busy (previous call timed out?) — try interrupt
                warn!(?state, "GeminiPty: session not idle, interrupting");
                let _ = session.interrupt().await;
                if session
                    .wait_for_state(SessionState::Idle, INTERRUPT_TIMEOUT)
                    .await
                    .is_err()
                {
                    warn!("GeminiPty: interrupt failed, will respawn");
                    needs_spawn = true;
                }
            }
        }

        if needs_spawn {
            // Close old session if any
            if let Some(mut old) = guard.take() {
                info!("GeminiPty: closing dead/stuck session");
                let _ = old.close().await;
            }
            *guard = Some(self.spawn_session().await?);
        }

        // ── Phase 2: /clear + send (atomic under Mutex) ──

        let session = guard.as_ref().unwrap();

        // /clear resets Gemini CLI's internal conversation context.
        // MUST use send() (synchronous), NOT send_fire_and_forget().
        // send() waits for TextOutputEvent::Complete and consumes it, ensuring
        // the event queue is clean before the real prompt. With send_fire_and_forget,
        // the /clear's Complete event would linger and be misattributed to the next send().
        let _ = session.send("/clear", CLEAR_TIMEOUT.as_millis() as u64).await;

        // ── Phase 3: Send prompt ──

        let timeout_ms = idle_timeout.as_millis() as u64;

        let response = if use_file {
            // Large prompt: write to temp file, send via @filepath.
            // Avoids PTY pipe buffer overflow (OS limit ~4-64KB per write).
            // Gemini CLI's @file syntax reads the file and includes content as user input.
            let prompt_file = Self::write_prompt_file(prompt)?;
            let file_msg = format!("@{}", prompt_file.display());
            info!(path = %prompt_file.display(), "GeminiPty: sending via @file ({} chars)", prompt.len());

            let result = session.send(&file_msg, timeout_ms).await;

            // Clean up temp file regardless of result
            if let Err(e) = std::fs::remove_file(&prompt_file) {
                warn!(path = %prompt_file.display(), "GeminiPty: failed to clean temp file: {}", e);
            }

            result?
        } else {
            // Small prompt: direct bracketed paste
            info!("GeminiPty: sending via paste ({} chars)", prompt.len());
            session.send(prompt, timeout_ms).await?
        };

        if response.is_empty() {
            return Err(anyhow!("GeminiPty: empty response from Gemini CLI"));
        }

        info!(response_len = response.len(), "GeminiPty: done");
        Ok(response)
    }
}

impl std::fmt::Debug for GeminiPtyTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiPtyTransport")
            .field("cwd", &self.cwd)
            .finish()
    }
}
