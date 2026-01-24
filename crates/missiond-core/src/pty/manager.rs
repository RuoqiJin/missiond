//! PTY Manager - Manages multiple PTY sessions
//!
//! Unlike simple process management, PTYManager maintains persistent
//! interactive sessions for Claude Code agents.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};

use super::session::{
    ConfirmInfo, ConfirmResponse, PTYSession, PTYSessionOptions, PermissionDecision, SessionEvent,
    SessionState,
};

// ========== Types ==========

/// Information about a PTY agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PTYAgentInfo {
    pub slot_id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub state: SessionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task_id: Option<String>,
    pub log_file: PathBuf,
}

/// Options for spawning a PTY session
#[derive(Debug, Clone, Default)]
pub struct PTYSpawnOptions {
    /// Automatically restart on crash
    pub auto_restart: bool,
}

/// Result of executing a message
#[derive(Debug, Clone, Serialize)]
pub struct PTYExecuteResult {
    pub response: String,
    pub duration_ms: u64,
}

/// Slot configuration
#[derive(Debug, Clone)]
pub struct Slot {
    pub id: String,
    pub role: String,
    pub cwd: Option<PathBuf>,
}

/// Permission policy trait
pub trait PermissionPolicy: Send + Sync {
    fn check_permission(&self, slot_id: &str, role: &str, tool_name: &str) -> PermissionDecision;
}

// ========== PTYManager ==========

/// Manager for multiple PTY sessions
///
/// Handles lifecycle, message routing, and event aggregation
/// for multiple Claude Code PTY sessions.
pub struct PTYManager {
    /// Active sessions by slot ID
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<PTYSession>>>>>,
    /// Agent info by slot ID
    agent_info: Arc<RwLock<HashMap<String, PTYAgentInfo>>>,
    /// Directory for log files
    logs_dir: PathBuf,
    /// Slots configured for auto-restart
    auto_restart_slots: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Permission policy
    permission_policy: Arc<RwLock<Option<Arc<dyn PermissionPolicy>>>>,
    /// Aggregated event channel
    event_tx: broadcast::Sender<ManagerEvent>,
}

/// Events from the manager
#[derive(Debug, Clone)]
pub enum ManagerEvent {
    /// Session spawned
    Spawned { slot_id: String },
    /// State changed
    StateChange {
        slot_id: String,
        new_state: SessionState,
        prev_state: SessionState,
    },
    /// Confirmation required
    ConfirmRequired {
        slot_id: String,
        prompt: String,
        tool_info: Option<ConfirmInfo>,
    },
    /// Session exited
    Exited { slot_id: String, exit_code: i32 },
}

impl PTYManager {
    /// Create a new PTY manager
    pub fn new(logs_dir: PathBuf) -> Self {
        // Ensure logs directory exists
        if !logs_dir.exists() {
            std::fs::create_dir_all(&logs_dir).ok();
        }

        let (event_tx, _) = broadcast::channel(1000);

        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            agent_info: Arc::new(RwLock::new(HashMap::new())),
            logs_dir,
            auto_restart_slots: Arc::new(RwLock::new(std::collections::HashSet::new())),
            permission_policy: Arc::new(RwLock::new(None)),
            event_tx,
        }
    }

    /// Set permission policy
    pub async fn set_permission_policy(&self, policy: Arc<dyn PermissionPolicy>) {
        *self.permission_policy.write().await = Some(policy);
        info!("Permission policy set");
    }

    /// Initialize a slot
    pub async fn init_slot(&self, slot: &Slot) {
        let log_file = self.logs_dir.join(format!("pty-{}.log", slot.id));

        let info = PTYAgentInfo {
            slot_id: slot.id.clone(),
            role: slot.role.clone(),
            pid: None,
            state: SessionState::Exited,
            started_at: None,
            current_task_id: None,
            log_file,
        };

        self.agent_info.write().await.insert(slot.id.clone(), info);
        debug!(slot_id = %slot.id, role = %slot.role, "PTY slot initialized");
    }

    /// Spawn a PTY session for a slot
    pub async fn spawn(&self, slot: &Slot, options: PTYSpawnOptions) -> Result<PTYAgentInfo> {
        let info = {
            let agent_info = self.agent_info.read().await;
            agent_info
                .get(&slot.id)
                .cloned()
                .ok_or_else(|| anyhow!("Slot not initialized: {}", slot.id))?
        };

        // Check for existing running session
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&slot.id) {
                let session = session.read().await;
                if session.is_running() {
                    return Err(anyhow!("PTY session already running: {}", slot.id));
                }
            }
        }

        // Track auto-restart
        if options.auto_restart {
            self.auto_restart_slots.write().await.insert(slot.id.clone());
        }

        // Create session
        let cwd = slot.cwd.clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        });

        let mut session = PTYSession::new(PTYSessionOptions {
            slot_id: slot.id.clone(),
            cwd,
            env: None,
            log_file: Some(info.log_file.clone()),
            cols: 120,
            rows: 30,
        })?;

        // Set up permission check
        let policy = self.permission_policy.read().await.clone();
        let slot_id = slot.id.clone();
        let role = slot.role.clone();
        if let Some(policy) = policy {
            session
                .set_permission_check(move |confirm_info: &ConfirmInfo| {
                    let tool_name = confirm_info
                        .tool
                        .as_ref()
                        .map(|t| t.name.as_str())
                        .unwrap_or("");
                    policy.check_permission(&slot_id, &role, tool_name)
                })
                .await;
        }

        // Set up event forwarding
        let event_tx = self.event_tx.clone();
        let slot_id_for_events = slot.id.clone();
        let mut session_rx = session.subscribe();

        tokio::spawn(async move {
            while let Ok(event) = session_rx.recv().await {
                match event {
                    SessionEvent::StateChange {
                        new_state,
                        prev_state,
                    } => {
                        let _ = event_tx.send(ManagerEvent::StateChange {
                            slot_id: slot_id_for_events.clone(),
                            new_state,
                            prev_state,
                        });
                    }
                    SessionEvent::ConfirmRequired { prompt, info } => {
                        let _ = event_tx.send(ManagerEvent::ConfirmRequired {
                            slot_id: slot_id_for_events.clone(),
                            prompt,
                            tool_info: info,
                        });
                    }
                    SessionEvent::Exit(code) => {
                        let _ = event_tx.send(ManagerEvent::Exited {
                            slot_id: slot_id_for_events.clone(),
                            exit_code: code,
                        });
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Start session
        session.start().await?;

        let pid = session.pid().await;
        let state = session.state().await;

        // Update agent info
        {
            let mut agent_info = self.agent_info.write().await;
            if let Some(info) = agent_info.get_mut(&slot.id) {
                info.pid = pid;
                info.state = state;
                info.started_at = Some(Utc::now().timestamp_millis());
            }
        }

        // Store session
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(slot.id.clone(), Arc::new(RwLock::new(session)));
        }

        info!(slot_id = %slot.id, pid = ?pid, "PTY session started");
        let _ = self.event_tx.send(ManagerEvent::Spawned {
            slot_id: slot.id.clone(),
        });

        // Set up auto-restart handler
        let manager_sessions = Arc::clone(&self.sessions);
        let manager_info = Arc::clone(&self.agent_info);
        let manager_auto_restart = Arc::clone(&self.auto_restart_slots);
        let manager_policy = Arc::clone(&self.permission_policy);
        let manager_event_tx = self.event_tx.clone();
        let slot_for_restart = slot.clone();

        tokio::spawn(async move {
            // Wait for session to exit
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;

                let should_restart = {
                    let sessions = manager_sessions.read().await;
                    if let Some(session) = sessions.get(&slot_for_restart.id) {
                        let session = session.read().await;
                        !session.is_running()
                    } else {
                        false
                    }
                };

                if should_restart {
                    // Check if auto-restart is enabled
                    let auto_restart = manager_auto_restart
                        .read()
                        .await
                        .contains(&slot_for_restart.id);

                    if auto_restart {
                        info!(slot_id = %slot_for_restart.id, "Auto-restarting PTY session");

                        // Create new session
                        let cwd = slot_for_restart.cwd.clone().unwrap_or_else(|| {
                            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
                        });

                        let log_file = {
                            let info = manager_info.read().await;
                            info.get(&slot_for_restart.id)
                                .map(|i| i.log_file.clone())
                        };

                        if let Ok(mut new_session) = PTYSession::new(PTYSessionOptions {
                            slot_id: slot_for_restart.id.clone(),
                            cwd,
                            env: None,
                            log_file,
                            cols: 120,
                            rows: 30,
                        }) {
                            // Set up permission check
                            let policy = manager_policy.read().await.clone();
                            let slot_id = slot_for_restart.id.clone();
                            let role = slot_for_restart.role.clone();
                            if let Some(policy) = policy {
                                new_session
                                    .set_permission_check(move |confirm_info: &ConfirmInfo| {
                                        let tool_name = confirm_info
                                            .tool
                                            .as_ref()
                                            .map(|t| t.name.as_str())
                                            .unwrap_or("");
                                        policy.check_permission(&slot_id, &role, tool_name)
                                    })
                                    .await;
                            }

                            if new_session.start().await.is_ok() {
                                let pid = new_session.pid().await;
                                let state = new_session.state().await;

                                // Update info
                                {
                                    let mut info = manager_info.write().await;
                                    if let Some(agent_info) = info.get_mut(&slot_for_restart.id) {
                                        agent_info.pid = pid;
                                        agent_info.state = state;
                                        agent_info.started_at =
                                            Some(Utc::now().timestamp_millis());
                                    }
                                }

                                // Store session
                                {
                                    let mut sessions = manager_sessions.write().await;
                                    sessions.insert(
                                        slot_for_restart.id.clone(),
                                        Arc::new(RwLock::new(new_session)),
                                    );
                                }

                                let _ = manager_event_tx.send(ManagerEvent::Spawned {
                                    slot_id: slot_for_restart.id.clone(),
                                });

                                info!(slot_id = %slot_for_restart.id, "Auto-restart successful");
                            } else {
                                error!(slot_id = %slot_for_restart.id, "Auto-restart failed");
                            }
                        }
                    }

                    break;
                }
            }
        });

        // Return current info
        let agent_info = self.agent_info.read().await;
        Ok(agent_info.get(&slot.id).cloned().unwrap())
    }

    /// Send message and wait for response
    pub async fn send(
        &self,
        slot_id: &str,
        message: &str,
        timeout_ms: u64,
    ) -> Result<PTYExecuteResult> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(slot_id)
                .cloned()
                .ok_or_else(|| anyhow!("No PTY session for slot: {}", slot_id))?
        };

        // Update state to thinking
        {
            let mut agent_info = self.agent_info.write().await;
            if let Some(info) = agent_info.get_mut(slot_id) {
                info.state = SessionState::Thinking;
            }
        }

        let start = std::time::Instant::now();

        let response = {
            let session = session.read().await;
            session.send(message, timeout_ms).await?
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        info!(
            slot_id = slot_id,
            message_len = message.len(),
            response_len = response.len(),
            duration_ms = duration_ms,
            "Message sent and response received"
        );

        Ok(PTYExecuteResult {
            response,
            duration_ms,
        })
    }

    /// Subscribe to session events (raw data/state/exit/etc.)
    pub async fn subscribe_session(
        &self,
        slot_id: &str,
    ) -> Result<broadcast::Receiver<SessionEvent>> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(slot_id)
                .cloned()
                .ok_or_else(|| anyhow!("No PTY session for slot: {}", slot_id))?
        };

        let session = session.read().await;
        Ok(session.subscribe())
    }

    /// Execute a task
    pub async fn execute_task(
        &self,
        slot: &Slot,
        task_id: &str,
        prompt: &str,
    ) -> Result<PTYExecuteResult> {
        // Set current task
        {
            let mut agent_info = self.agent_info.write().await;
            if let Some(info) = agent_info.get_mut(&slot.id) {
                info.current_task_id = Some(task_id.to_string());
            }
        }

        let result = self.send(&slot.id, prompt, 300_000).await;

        // Clear current task
        {
            let mut agent_info = self.agent_info.write().await;
            if let Some(info) = agent_info.get_mut(&slot.id) {
                info.current_task_id = None;
            }
        }

        result
    }

    /// Send confirmation response
    pub async fn confirm(&self, slot_id: &str, response: ConfirmResponse) -> Result<()> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(slot_id)
                .cloned()
                .ok_or_else(|| anyhow!("No PTY session for slot: {}", slot_id))?
        };

        let session = session.read().await;
        session.confirm(response).await
    }

    /// Write raw input to the PTY (no state checks)
    pub async fn write(&self, slot_id: &str, data: &str) -> Result<()> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(slot_id)
                .cloned()
                .ok_or_else(|| anyhow!("No PTY session for slot: {}", slot_id))?
        };

        let session = session.read().await;
        session.write(data).await
    }

    /// Send interrupt signal
    pub async fn interrupt(&self, slot_id: &str) -> Result<()> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(slot_id)
                .cloned()
                .ok_or_else(|| anyhow!("No PTY session for slot: {}", slot_id))?
        };

        let session = session.read().await;
        session.interrupt().await
    }

    /// Get screen content
    pub async fn get_screen(&self, slot_id: &str) -> Result<String> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(slot_id)
                .cloned()
                .ok_or_else(|| anyhow!("No PTY session for slot: {}", slot_id))?
        };

        let session = session.read().await;
        Ok(session.get_screen_text().await)
    }

    /// Get last N lines
    pub async fn get_last_lines(&self, slot_id: &str, n: usize) -> Result<Vec<String>> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(slot_id)
                .cloned()
                .ok_or_else(|| anyhow!("No PTY session for slot: {}", slot_id))?
        };

        let session = session.read().await;
        Ok(session.get_last_lines(n).await)
    }

    /// Get chat history
    pub async fn get_history(&self, slot_id: &str) -> Vec<super::session::Message> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(slot_id) {
            let session = session.read().await;
            session.history().await
        } else {
            Vec::new()
        }
    }

    /// Kill a session
    pub async fn kill(&self, slot_id: &str) -> Result<()> {
        // Remove from auto-restart
        self.auto_restart_slots.write().await.remove(slot_id);

        // Get and close session
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(slot_id)
        };

        if let Some(session) = session {
            let mut session = session.write().await;
            session.close().await?;
        }

        // Update agent info
        {
            let mut agent_info = self.agent_info.write().await;
            if let Some(info) = agent_info.get_mut(slot_id) {
                info.state = SessionState::Exited;
                info.pid = None;
            }
        }

        info!(slot_id = slot_id, "PTY session killed");
        Ok(())
    }

    /// Restart a session
    pub async fn restart(&self, slot: &Slot, options: PTYSpawnOptions) -> Result<PTYAgentInfo> {
        self.kill(&slot.id).await?;
        self.spawn(slot, options).await
    }

    /// Get session status
    pub async fn get_status(&self, slot_id: &str) -> Option<PTYAgentInfo> {
        self.agent_info.read().await.get(slot_id).cloned()
    }

    /// Get all session statuses
    pub async fn get_all_status(&self) -> Vec<PTYAgentInfo> {
        self.agent_info.read().await.values().cloned().collect()
    }

    /// Check if session is available (idle)
    pub async fn is_available(&self, slot_id: &str) -> bool {
        if let Some(info) = self.agent_info.read().await.get(slot_id) {
            info.state == SessionState::Idle
        } else {
            false
        }
    }

    /// Check if session is running
    pub async fn is_running(&self, slot_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(slot_id) {
            let session = session.read().await;
            session.is_running()
        } else {
            false
        }
    }

    /// Get statistics
    pub async fn get_stats(&self) -> ManagerStats {
        let mut stats = ManagerStats::default();

        let agent_info = self.agent_info.read().await;
        stats.total = agent_info.len();

        for info in agent_info.values() {
            match info.state {
                SessionState::Idle => {
                    stats.idle += 1;
                    stats.running += 1;
                }
                SessionState::Thinking
                | SessionState::Responding
                | SessionState::ToolRunning
                | SessionState::Confirming => {
                    stats.busy += 1;
                    stats.running += 1;
                }
                SessionState::Starting => {
                    stats.running += 1;
                }
                SessionState::Exited | SessionState::Error => {
                    stats.stopped += 1;
                }
            }
        }

        stats
    }

    /// Subscribe to manager events
    pub fn subscribe(&self) -> broadcast::Receiver<ManagerEvent> {
        self.event_tx.subscribe()
    }

    /// Shutdown all sessions
    pub async fn shutdown(&self) {
        info!("Shutting down all PTY sessions...");

        // Clear auto-restart
        self.auto_restart_slots.write().await.clear();

        // Collect slot IDs
        let slot_ids: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions.keys().cloned().collect()
        };

        // Kill all sessions
        for slot_id in slot_ids {
            if let Err(e) = self.kill(&slot_id).await {
                error!(slot_id = %slot_id, error = %e, "Error killing PTY session");
            }
        }

        info!("All PTY sessions shut down");
    }
}

/// Manager statistics
#[derive(Debug, Clone, Default, Serialize)]
pub struct ManagerStats {
    pub total: usize,
    pub running: usize,
    pub idle: usize,
    pub busy: usize,
    pub stopped: usize,
}
