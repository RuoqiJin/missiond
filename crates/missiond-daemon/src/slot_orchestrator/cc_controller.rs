//! ClaudeCodeController — PTY operations for Claude Code engine.
//!
//! Handles the specifics of Claude Code interaction:
//! - JSONL session detection for session_id binding
//! - Event-driven completion: subscribe → send → wait(Thinking) → wait(TextComplete)
//! - Wait-for-idle before send (handles auto-start race)
//! - No /clear (relies on Claude Code's own compaction)
//! - Result from DB (authoritative JSONL), TextComplete as trigger only

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::{info, warn};

use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{ManagerEvent, PTYManager, PTYSpawnOptions, SessionState};
use missiond_core::types::CliEngine;
use missiond_core::types::SharedProjectRegistry;
use missiond_core::LearnedPermissions;

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;

use super::controller::EngineController;
use super::register_slot_session;
use super::types::SlotTaskRequest;

/// Max time to wait for slot to reach Idle before sending prompt.
/// Covers auto-start race: daemon boots → slot spawning → task arrives.
const PRE_SEND_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// DB poll interval after TextComplete (async JSONL write latency).
const DB_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Max DB poll attempts (200ms × 25 = 5s).
const DB_POLL_MAX: usize = 25;

use std::collections::HashSet;
use tokio::sync::RwLock;

/// Expectation tickets for pending slot spawns.
/// Shared with IngestionRouter for late-binding session discovery.
pub struct ClaudeCodeController {
    pty: Arc<PTYManager>,
    store: Arc<dyn MissionStore>,
    pty_session_uuids: Arc<RwLock<HashSet<String>>>,
    project_registry: SharedProjectRegistry,
    learned: Option<Arc<LearnedPermissions>>,
}

impl ClaudeCodeController {
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

    /// Wait for a slot to reach Idle state (event-driven).
    /// Used before sending to handle auto-start and post-spawn warmup.
    async fn wait_until_idle(&self, slot_id: &str, timeout: Duration) -> Result<()> {
        // Fast path: already idle
        if self.pty.is_available(slot_id).await {
            return Ok(());
        }

        let deadline = tokio::time::Instant::now() + timeout;
        let mut rx = self.pty.subscribe();

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "Timeout ({}s) waiting for slot {} to reach Idle",
                    timeout.as_secs(),
                    slot_id
                ));
            }

            // Re-check after subscribe (covers race between subscribe and state change)
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
                Err(_) => continue, // poll interval, loop re-checks is_available
            }
        }
    }

    /// Extract result from DB after TextComplete signal.
    /// TextComplete is used as trigger only; DB (JSONL pipeline) is the authoritative source.
    /// Falls back to TextComplete.content if DB is unavailable within poll window.
    async fn extract_from_db_or_fallback(
        &self,
        slot_id: &str,
        text_complete_content: &str,
    ) -> String {
        let session_id = match self.store.get_slot_session(slot_id).await {
            Ok(Some(id)) => id,
            _ => return text_complete_content.to_string(),
        };

        for attempt in 0..DB_POLL_MAX {
            if let Ok(Some(content)) = self.store.get_last_assistant_content(&session_id).await {
                if !content.is_empty() {
                    // DB has content — use it (authoritative JSONL source)
                    return content;
                }
            }
            if attempt < DB_POLL_MAX - 1 {
                tokio::time::sleep(DB_POLL_INTERVAL).await;
            }
        }

        // DB unavailable within 5s — fallback to TextComplete.content
        warn!(
            slot_id,
            "ClaudeCodeCtrl: DB content unavailable after {}ms, using TextComplete fallback",
            DB_POLL_INTERVAL.as_millis() as u64 * DB_POLL_MAX as u64
        );
        text_complete_content.to_string()
    }
}

#[async_trait]
impl EngineController for ClaudeCodeController {
    async fn is_alive(&self, slot_id: &str) -> bool {
        self.pty.is_running(slot_id).await
    }

    async fn spawn_and_register(
        &self,
        slot_id: &str,
        req: &SlotTaskRequest,
        is_ephemeral: bool,
    ) -> Result<String> {
        info!(slot_id, "ClaudeCodeCtrl: spawning");
        let runtime_config = WorkstationRuntimeConfig::load_for_project_root(Some(
            req.cwd.to_string_lossy().as_ref(),
        ))
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))?;
        let spawn_timeout_secs = runtime_config.dynamic_slot_spawn_timeout_secs();

        let pty_slot = missiond_core::pty::Slot {
            id: slot_id.to_string(),
            role: req.task_type.clone(),
            cwd: Some(req.cwd.clone()),
            engine: CliEngine::ClaudeCode,
        };

        self.pty.init_slot(&pty_slot).await;

        crate::slot_orchestrator::spawner::spawn_tracked_slot(
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
                dangerously_skip_permissions: req.skip_permissions,
                model: req.model.clone(),
                reasoning_effort: None,
                search_enabled: false,
                sandbox: None,
                approval_policy: None,
                extra_env: HashMap::new(),
                initial_prompt: None,
            },
            None, // Pass the custom environment from the request if it had one
        )
        .await?;

        // Also try immediate registration if session is already known (fast-spawn case)
        if let Ok(Some(session_id)) = self.store.get_slot_session(slot_id).await {
            register_slot_session(
                &self.store,
                slot_id,
                &session_id,
                "claude_code",
                is_ephemeral,
            )
            .await;
            info!(slot_id, session_id = %session_id, "ClaudeCodeCtrl: spawned + immediately registered");
            return Ok(session_id);
        }

        let placeholder = format!("pending-{}", slot_id);
        info!(slot_id, project = %req.cwd.display(), "ClaudeCodeCtrl: spawned slot process");
        Ok(placeholder)
    }

    async fn ask(&self, slot_id: &str, prompt: &str, timeout: Duration) -> Result<String> {
        // 0. Wait for slot to be Idle before sending.
        //    Handles auto-start race: daemon boots → slot in Starting → task arrives.
        //    Also handles back-to-back tasks where previous turn is still wrapping up.
        self.wait_until_idle(slot_id, PRE_SEND_IDLE_TIMEOUT).await?;

        // 1. Subscribe BEFORE send — eliminates the fatal race condition.
        let mut rx = self.pty.subscribe();

        // 2. Send prompt
        self.pty.send_fire_and_forget(slot_id, prompt).await?;

        info!(
            slot_id,
            prompt_len = prompt.len(),
            "ClaudeCodeCtrl: prompt sent, waiting for TextComplete"
        );

        // 3. Event-driven state machine: wait for Thinking → TextComplete
        let deadline = tokio::time::Instant::now() + timeout;
        let mut saw_thinking = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "Timeout ({}s) waiting for response from slot {}",
                    timeout.as_secs(),
                    slot_id
                ));
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                // Thinking confirmed — PTY has started processing
                Ok(Ok(ManagerEvent::StateChange {
                    slot_id: ref id,
                    new_state: SessionState::Thinking,
                    ..
                })) if id == slot_id => {
                    saw_thinking = true;
                }

                // TextComplete — turn finished. Use as trigger, extract from DB.
                Ok(Ok(ManagerEvent::TextComplete {
                    slot_id: ref id,
                    ref content,
                    ..
                })) if id == slot_id && saw_thinking => {
                    info!(
                        slot_id,
                        content_len = content.len(),
                        "ClaudeCodeCtrl: TextComplete received, extracting from DB"
                    );
                    // TextComplete = trigger only. DB (JSONL) = authoritative source.
                    return Ok(self.extract_from_db_or_fallback(slot_id, content).await);
                }

                // Slot exited unexpectedly
                Ok(Ok(ManagerEvent::Exited {
                    slot_id: ref id,
                    exit_code,
                })) if id == slot_id => {
                    return Err(anyhow!(
                        "Slot {} exited (code {}) during task execution",
                        slot_id,
                        exit_code
                    ));
                }

                // Other events — keep waiting
                Ok(Ok(_)) => continue,

                // Broadcast lag — events were dropped.
                // Fallback: check if slot already returned to Idle (we missed TextComplete).
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                    warn!(
                        slot_id,
                        skipped = n,
                        "ClaudeCodeCtrl: broadcast lagged, checking state"
                    );
                    // If slot is back to Idle, we missed the completion. Try DB extraction.
                    if self.pty.is_available(slot_id).await {
                        info!(
                            slot_id,
                            "ClaudeCodeCtrl: slot Idle after lag, extracting from DB"
                        );
                        return Ok(self.extract_from_db_or_fallback(slot_id, "").await);
                    }
                    // Still processing — re-subscribe implicitly (broadcast recv continues)
                    saw_thinking = true; // Assume we missed Thinking too
                    continue;
                }

                // Channel closed
                Ok(Err(_)) => {
                    return Err(anyhow!(
                        "Event channel closed while waiting for slot {}",
                        slot_id
                    ));
                }
                // Timeout on recv — loop will check deadline
                Err(_) => continue,
            }
        }
    }

    async fn clear_context(&self, _slot_id: &str) -> Result<()> {
        // Claude Code relies on its own compaction mechanism.
        Ok(())
    }

    async fn destroy(&self, slot_id: &str) -> Result<()> {
        self.pty.kill(slot_id).await
    }
}
