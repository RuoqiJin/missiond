//! GenericCliSlotManager — PTY-backed lanes for agent CLIs without a bespoke
//! controller yet (Codex worker lanes and Agy read-only lanes).
//!
//! This is intentionally small: it only owns lifecycle/concurrency and delegates
//! all terminal semantics to PTYManager plus provider-specific recognition.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tracing::{info, warn};

use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{ManagerEvent, PTYManager, PTYSpawnOptions, SessionState};
use missiond_core::types::{CliEngine, Lifecycle, SharedProjectRegistry};
use missiond_core::LearnedPermissions;

use super::controller::EngineController;
use super::register_slot_session;
use super::types::{EngineSlotManager, EngineStatus, SlotTaskRequest};

const EPHEMERAL_CAPACITY: usize = 2;

pub struct GenericCliSlotManager {
    controller: GenericCliController,
    persistent_locks: DashMap<String, Arc<Mutex<()>>>,
    ephemeral_semaphore: Arc<Semaphore>,
}

impl GenericCliSlotManager {
    pub fn new(
        engine: CliEngine,
        pty: Arc<PTYManager>,
        store: Arc<dyn MissionStore>,
        pty_session_uuids: Arc<RwLock<HashSet<String>>>,
        project_registry: SharedProjectRegistry,
        learned: Option<Arc<LearnedPermissions>>,
    ) -> Self {
        Self {
            controller: GenericCliController {
                engine,
                pty,
                store,
                pty_session_uuids,
                project_registry,
                learned,
            },
            persistent_locks: DashMap::new(),
            ephemeral_semaphore: Arc::new(Semaphore::new(EPHEMERAL_CAPACITY)),
        }
    }

    async fn execute_persistent(&self, req: &SlotTaskRequest) -> Result<String> {
        let slot_id = req
            .slot_id
            .as_deref()
            .ok_or_else(|| anyhow!("Persistent generic CLI task requires slot_id"))?;
        let lock = self
            .persistent_locks
            .entry(slot_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        if !self.controller.is_alive(slot_id).await {
            self.controller
                .spawn_and_register(slot_id, req, false)
                .await?;
        }

        info!(
            slot_id,
            engine = ?self.controller.engine,
            prompt_len = req.prompt.len(),
            "GenericCliSlotMgr: sending to persistent slot"
        );
        self.controller.ask(slot_id, &req.prompt, req.timeout).await
    }

    async fn execute_ephemeral(&self, req: &SlotTaskRequest) -> Result<String> {
        let _permit = self
            .ephemeral_semaphore
            .acquire()
            .await
            .map_err(|_| anyhow!("Generic CLI ephemeral semaphore closed"))?;
        let temp_id = format!(
            "ephemeral-{}-{}",
            self.controller.engine,
            uuid::Uuid::new_v4().simple()
        );
        self.controller
            .spawn_and_register(&temp_id, req, true)
            .await?;
        let result = self
            .controller
            .ask(&temp_id, &req.prompt, req.timeout)
            .await;
        if let Err(e) = self.controller.destroy(&temp_id).await {
            warn!(slot_id = %temp_id, "GenericCliSlotMgr: kill failed: {}", e);
        }
        result
    }
}

#[async_trait]
impl EngineSlotManager for GenericCliSlotManager {
    fn engine_type(&self) -> CliEngine {
        self.controller.engine
    }

    async fn execute(&self, task: &SlotTaskRequest) -> Result<String> {
        match task.lifecycle {
            Lifecycle::Persistent => self.execute_persistent(task).await,
            Lifecycle::OnDemand => self.execute_ephemeral(task).await,
        }
    }

    async fn status(&self) -> EngineStatus {
        EngineStatus {
            engine: self.controller.engine,
            persistent_slots: self.persistent_locks.len(),
            ephemeral_active: EPHEMERAL_CAPACITY - self.ephemeral_semaphore.available_permits(),
            ephemeral_capacity: EPHEMERAL_CAPACITY,
        }
    }
}

pub struct GenericCliController {
    engine: CliEngine,
    pty: Arc<PTYManager>,
    store: Arc<dyn MissionStore>,
    pty_session_uuids: Arc<RwLock<HashSet<String>>>,
    project_registry: SharedProjectRegistry,
    learned: Option<Arc<LearnedPermissions>>,
}

#[async_trait]
impl EngineController for GenericCliController {
    async fn is_alive(&self, slot_id: &str) -> bool {
        self.pty.is_running(slot_id).await
    }

    async fn spawn_and_register(
        &self,
        slot_id: &str,
        req: &SlotTaskRequest,
        is_ephemeral: bool,
    ) -> Result<String> {
        let pty_slot = missiond_core::pty::Slot {
            id: slot_id.to_string(),
            role: format!("{}_worker", self.engine),
            cwd: Some(req.cwd.clone()),
            engine: self.engine,
        };
        self.pty.init_slot(&pty_slot).await;
        let mut extra_env = HashMap::new();
        if self.engine == CliEngine::ClaudeCode && req.skip_permissions {
            extra_env.insert(
                "MISSIOND_ALLOW_BROAD_SKIP_PERMISSIONS".to_string(),
                "true".to_string(),
            );
        }
        super::spawner::spawn_tracked_slot(
            &self.pty,
            &self.store,
            &self.pty_session_uuids,
            &self.project_registry,
            self.learned.as_ref(),
            &pty_slot,
            PTYSpawnOptions {
                auto_restart: !is_ephemeral,
                wait_for_idle: true,
                timeout_secs: Some(45),
                mcp_config: None,
                dangerously_skip_permissions: req.skip_permissions,
                model: req.model.clone(),
                reasoning_effort: req.reasoning_effort.clone(),
                search_enabled: req.search_enabled,
                sandbox: req.sandbox.clone(),
                approval_policy: req.approval_policy.clone(),
                tool_policy_path: req.tool_policy_path.clone(),
                extra_env: req.env.clone(),
                initial_prompt: None,
                command_override: None,
                ..Default::default()
            },
            Some(&req.env),
        )
        .await?;
        let session_id = format!("pty-{}", slot_id);
        register_slot_session(
            &self.store,
            slot_id,
            &session_id,
            super::canonical_source_for_engine(self.engine),
            is_ephemeral,
        )
        .await;
        Ok(session_id)
    }

    async fn ask(
        &self,
        slot_id: &str,
        prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<String> {
        let mut rx = self.pty.subscribe();
        self.pty.send_fire_and_forget(slot_id, prompt).await?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "Timeout ({}s) waiting for {:?} response from slot {}",
                    timeout.as_secs(),
                    self.engine,
                    slot_id
                ));
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(ManagerEvent::TextComplete {
                    slot_id: ref id,
                    content,
                    ..
                })) if id == slot_id => {
                    if content.trim().is_empty() {
                        return Ok(self.pty.get_screen_text(slot_id).await.unwrap_or_default());
                    }
                    return Ok(content);
                }
                Ok(Ok(ManagerEvent::StateChange {
                    slot_id: ref id,
                    new_state: SessionState::Idle,
                    ..
                })) if id == slot_id => {
                    return Ok(self.pty.get_screen_text(slot_id).await.unwrap_or_default());
                }
                Ok(Ok(ManagerEvent::Exited {
                    slot_id: ref id,
                    exit_code,
                })) if id == slot_id => {
                    return Err(anyhow!(
                        "{:?} slot {} exited (code {}) during task",
                        self.engine,
                        slot_id,
                        exit_code
                    ));
                }
                Ok(Ok(_)) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) => return Err(anyhow!("Event channel closed for slot {}", slot_id)),
                Err(_) => continue,
            }
        }
    }

    async fn clear_context(&self, _slot_id: &str) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self, slot_id: &str) -> Result<()> {
        self.pty.kill(slot_id).await
    }
}
