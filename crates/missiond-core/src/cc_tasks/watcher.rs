//! Claude Code Tasks Watcher
//!
//! Monitors Claude Code sessions and their tasks in real-time

use super::parser::*;
use super::types::*;
use chrono::{Duration, Utc};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

/// Watcher options
pub struct CCTasksWatcherOptions {
    /// Claude home directory, defaults to ~/.claude
    pub claude_home: Option<PathBuf>,
    /// How often to check for inactive sessions (ms), default 60000
    pub inactive_check_interval_ms: u64,
    /// Max recent changes to keep, default 50
    pub max_recent_changes: usize,
}

impl Default for CCTasksWatcherOptions {
    fn default() -> Self {
        Self {
            claude_home: None,
            inactive_check_interval_ms: 60000,
            max_recent_changes: 50,
        }
    }
}

/// Watcher events
#[derive(Debug, Clone)]
pub enum WatcherEvent {
    TasksChanged(CCTaskChangeEvent),
    TaskStarted { session: CCSession, task: CCTask },
    TaskCompleted { session: CCSession, task: CCTask },
    SessionActive(CCSession),
    SessionInactive(CCSession),
    /// New messages detected in a JSONL file (for conversation logging)
    NewMessages {
        session_id: String,
        project_path: String,
        jsonl_path: String,
        messages: Vec<CCMessageLine>,
    },
    /// New system events detected in a JSONL file (progress, system, queue-operation, etc.)
    NewEvents {
        session_id: String,
        events: Vec<serde_json::Value>,
    },
}

/// Claude Code Tasks Watcher
pub struct CCTasksWatcher {
    projects_dir: PathBuf,
    sessions: Arc<RwLock<HashMap<String, CCSession>>>,
    file_positions: Arc<RwLock<HashMap<String, u64>>>,
    recent_changes: Arc<RwLock<Vec<CCTaskChangeEvent>>>,
    max_recent_changes: usize,
    inactive_check_interval_ms: u64,
    event_tx: broadcast::Sender<WatcherEvent>,
    watcher: Option<RecommendedWatcher>,
    started: Arc<RwLock<bool>>,
}

impl CCTasksWatcher {
    /// Create a new watcher
    pub fn new(options: CCTasksWatcherOptions) -> Self {
        let claude_home = options.claude_home.unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".claude"))
                .unwrap_or_else(|| PathBuf::from("~/.claude"))
        });
        let projects_dir = claude_home.join("projects");

        let (event_tx, _) = broadcast::channel(100);

        Self {
            projects_dir,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            file_positions: Arc::new(RwLock::new(HashMap::new())),
            recent_changes: Arc::new(RwLock::new(Vec::new())),
            max_recent_changes: options.max_recent_changes,
            inactive_check_interval_ms: options.inactive_check_interval_ms,
            event_tx,
            watcher: None,
            started: Arc::new(RwLock::new(false)),
        }
    }

    /// Subscribe to watcher events
    pub fn subscribe(&self) -> broadcast::Receiver<WatcherEvent> {
        self.event_tx.subscribe()
    }

    /// Start watching Claude Code sessions
    pub async fn start(&mut self) -> anyhow::Result<()> {
        {
            let mut started = self.started.write().await;
            if *started {
                return Ok(());
            }
            *started = true;
        }

        info!(projects_dir = ?self.projects_dir, "Starting CCTasksWatcher");

        // Initial scan
        self.scan_all_projects().await;

        // Setup file watcher
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        let watcher_tx = tx.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = watcher_tx.blocking_send(event);
                }
            },
            Config::default(),
        )?;

        watcher.watch(&self.projects_dir, RecursiveMode::Recursive)?;
        self.watcher = Some(watcher);

        // Spawn event handler
        let sessions = self.sessions.clone();
        let file_positions = self.file_positions.clone();
        let recent_changes = self.recent_changes.clone();
        let max_recent_changes = self.max_recent_changes;
        let event_tx = self.event_tx.clone();
        let projects_dir = self.projects_dir.clone();
        let started = self.started.clone();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if !*started.read().await {
                    break;
                }

                for path in event.paths {
                    let path_str = path.to_string_lossy();

                    // Skip non-relevant files
                    if path_str.contains("tool-results") || path_str.contains("session-memory") {
                        continue;
                    }

                    if path.ends_with("sessions-index.json") {
                        Self::load_project_sessions_static(
                            &path,
                            &sessions,
                            &file_positions,
                            &event_tx,
                        )
                        .await;
                    } else if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                        Self::check_session_for_changes_static(
                            &path,
                            &sessions,
                            &recent_changes,
                            max_recent_changes,
                            &event_tx,
                            &projects_dir,
                            &file_positions,
                        )
                        .await;
                    }
                }
            }
        });

        // Spawn inactive check + rescan timer
        let sessions = self.sessions.clone();
        let file_positions = self.file_positions.clone();
        let recent_changes = self.recent_changes.clone();
        let max_recent_changes = self.max_recent_changes;
        let event_tx = self.event_tx.clone();
        let inactive_check_interval = self.inactive_check_interval_ms;
        let started = self.started.clone();
        let projects_dir = self.projects_dir.clone();

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(inactive_check_interval));

            loop {
                interval.tick().await;

                if !*started.read().await {
                    break;
                }

                Self::check_inactive_sessions_static(&sessions, &event_tx).await;

                // Rescan active sessions as safety net for missed FSEvents (e.g., after sleep)
                Self::rescan_active_sessions(
                    &sessions,
                    &file_positions,
                    &recent_changes,
                    max_recent_changes,
                    &event_tx,
                    &projects_dir,
                ).await;
            }
        });

        info!("CCTasksWatcher started");
        Ok(())
    }

    /// Stop watching
    pub async fn stop(&mut self) {
        {
            let mut started = self.started.write().await;
            if !*started {
                return;
            }
            *started = false;
        }

        self.watcher = None;
        self.sessions.write().await.clear();
        self.file_positions.write().await.clear();
        self.recent_changes.write().await.clear();

        info!("CCTasksWatcher stopped");
    }

    /// Scan all projects directories
    async fn scan_all_projects(&self) {
        if !self.projects_dir.exists() {
            warn!(projects_dir = ?self.projects_dir, "Projects directory not found");
            return;
        }

        let mut entries = match tokio::fs::read_dir(&self.projects_dir).await {
            Ok(e) => e,
            Err(e) => {
                error!(?e, "Failed to read projects directory");
                return;
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let index_path = path.join("sessions-index.json");
            Self::load_project_sessions_static(
                &index_path,
                &self.sessions,
                &self.file_positions,
                &self.event_tx,
            )
            .await;
        }

        let session_count = self.sessions.read().await.len();
        info!(session_count, "Initial scan complete");
    }

    /// Load sessions from a project's sessions-index.json (static version)
    async fn load_project_sessions_static(
        index_path: &Path,
        sessions: &Arc<RwLock<HashMap<String, CCSession>>>,
        file_positions: &Arc<RwLock<HashMap<String, u64>>>,
        _event_tx: &broadcast::Sender<WatcherEvent>,
    ) {
        let index = match parse_sessions_index(index_path).await {
            Some(i) => i,
            None => return,
        };

        for entry in index.entries {
            let session = entry_to_session(&entry).await;
            let mut sessions = sessions.write().await;

            if !sessions.contains_key(&session.session_id) {
                let file_size = get_file_size(Path::new(&session.full_path)).await;
                file_positions
                    .write()
                    .await
                    .insert(session.full_path.clone(), file_size);
            }

            sessions.insert(session.session_id.clone(), session);
        }
    }

    /// Check a session file for task changes (static version)
    async fn check_session_for_changes_static(
        file_path: &Path,
        sessions: &Arc<RwLock<HashMap<String, CCSession>>>,
        recent_changes: &Arc<RwLock<Vec<CCTaskChangeEvent>>>,
        max_recent_changes: usize,
        event_tx: &broadcast::Sender<WatcherEvent>,
        _projects_dir: &Path,
        file_positions: &Arc<RwLock<HashMap<String, u64>>>,
    ) {
        let file_path_str = file_path.to_string_lossy().to_string();

        // Find session by file path
        let session_opt = {
            let sessions = sessions.read().await;
            sessions
                .values()
                .find(|s| s.full_path == file_path_str)
                .cloned()
        };

        let mut session = match session_opt {
            Some(s) => s,
            None => {
                // New session file, try to load from index
                if let Some(parent) = file_path.parent() {
                    let index_path = parent.join("sessions-index.json");
                    Self::load_project_sessions_static(
                        &index_path,
                        sessions,
                        file_positions,
                        event_tx,
                    )
                    .await;
                }

                // Re-check after index reload
                let session_retry = {
                    let sessions = sessions.read().await;
                    sessions
                        .values()
                        .find(|s| s.full_path == file_path_str)
                        .cloned()
                };

                match session_retry {
                    Some(s) => s,
                    None => {
                        // Session not in index — synthesize from file path for conversation logging
                        // Extract session_id from filename (e.g. "1bbf1a09-...jsonl")
                        let session_id = file_path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if session_id.is_empty() {
                            return;
                        }

                        let project_path = file_path
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();

                        // Still emit NewMessages/NewEvents even without a registered session
                        let from_pos = {
                            let positions = file_positions.read().await;
                            positions.get(&file_path_str).copied().unwrap_or(0)
                        };

                        if let Ok((raw_lines, new_pos)) =
                            read_new_lines_raw(file_path, from_pos).await
                        {
                            if new_pos > from_pos {
                                file_positions
                                    .write()
                                    .await
                                    .insert(file_path_str.clone(), new_pos);

                                let mut messages: Vec<CCMessageLine> = Vec::new();
                                let mut events: Vec<serde_json::Value> = Vec::new();

                                for val in raw_lines {
                                    let msg_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    match msg_type {
                                        "user" | "assistant" | "tool_result" | "tool_use" => {
                                            let raw_backup = val.clone();
                                            match serde_json::from_value::<CCMessageLine>(val) {
                                                Ok(msg) => messages.push(msg),
                                                Err(e) => {
                                                    warn!(error = %e, "JSONL message parse failed, demoting to event");
                                                    events.push(raw_backup);
                                                }
                                            }
                                        }
                                        "progress" | "system" | "queue-operation" | "file-history-snapshot" => {
                                            events.push(val);
                                        }
                                        _ => {
                                            events.push(val);
                                        }
                                    }
                                }

                                if !messages.is_empty() {
                                    let _ = event_tx.send(WatcherEvent::NewMessages {
                                        session_id: session_id.clone(),
                                        project_path,
                                        jsonl_path: file_path_str.clone(),
                                        messages,
                                    });
                                }
                                if !events.is_empty() {
                                    let _ = event_tx.send(WatcherEvent::NewEvents {
                                        session_id,
                                        events,
                                    });
                                }
                            }
                        }

                        return;
                    }
                }
            }
        };

        // --- Incremental message reading for conversation logging ---
        let from_pos = {
            let positions = file_positions.read().await;
            positions.get(&file_path_str).copied().unwrap_or(0)
        };

        if let Ok((raw_lines, new_pos)) = read_new_lines_raw(file_path, from_pos).await {
            if new_pos > from_pos {
                file_positions
                    .write()
                    .await
                    .insert(file_path_str.clone(), new_pos);

                // Route by message type: conversation messages vs system events
                let mut messages: Vec<CCMessageLine> = Vec::new();
                let mut events: Vec<serde_json::Value> = Vec::new();

                for val in raw_lines {
                    let msg_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match msg_type {
                        "user" | "assistant" | "tool_result" | "tool_use" => {
                            // Clone before from_value (which consumes) so we can demote on failure
                            let raw_backup = val.clone();
                            match serde_json::from_value::<CCMessageLine>(val) {
                                Ok(msg) => messages.push(msg),
                                Err(e) => {
                                    warn!(error = %e, "JSONL message parse failed, demoting to event");
                                    events.push(raw_backup);
                                }
                            }
                        }
                        "progress" | "system" | "queue-operation" | "file-history-snapshot" => {
                            events.push(val);
                        }
                        _ => {
                            // Unknown type — preserve as event for future compatibility
                            events.push(val);
                        }
                    }
                }

                if !messages.is_empty() {
                    let _ = event_tx.send(WatcherEvent::NewMessages {
                        session_id: session.session_id.clone(),
                        project_path: session.project_path.clone(),
                        jsonl_path: file_path_str.clone(),
                        messages,
                    });
                }
                if !events.is_empty() {
                    let _ = event_tx.send(WatcherEvent::NewEvents {
                        session_id: session.session_id.clone(),
                        events,
                    });
                }
            }
        }

        // --- Existing: todos change detection ---
        // Get previous tasks
        let previous_tasks = session.tasks.clone();

        // Parse new tasks
        let current_tasks = parse_last_todos(file_path, 100).await;

        // Check for changes
        let diff = diff_tasks(&previous_tasks, &current_tasks);

        if !diff.is_empty() {
            let was_active = session.is_active;
            session.tasks = current_tasks.clone();
            session.modified = Utc::now();
            session.is_active = true;

            // Create change event
            let change_event = CCTaskChangeEvent {
                session_id: session.session_id.clone(),
                project_path: session.project_path.clone(),
                project_name: session.project_name.clone(),
                previous_tasks,
                current_tasks,
                added: diff.added.clone(),
                removed: diff.removed,
                status_changed: diff.status_changed.clone(),
                timestamp: Utc::now(),
            };

            // Add to recent changes
            {
                let mut changes = recent_changes.write().await;
                changes.insert(0, change_event.clone());
                if changes.len() > max_recent_changes {
                    changes.pop();
                }
            }

            // Update session
            sessions
                .write()
                .await
                .insert(session.session_id.clone(), session.clone());

            // Emit events
            let _ = event_tx.send(WatcherEvent::TasksChanged(change_event));

            // Emit specific events for started/completed tasks
            for change in &diff.status_changed {
                if change.task.status == CCTaskStatus::InProgress
                    && change.previous_status == CCTaskStatus::Pending
                {
                    let _ = event_tx.send(WatcherEvent::TaskStarted {
                        session: session.clone(),
                        task: change.task.clone(),
                    });
                } else if change.task.status == CCTaskStatus::Completed {
                    let _ = event_tx.send(WatcherEvent::TaskCompleted {
                        session: session.clone(),
                        task: change.task.clone(),
                    });
                }
            }

            // Emit session active if it wasn't before
            if !was_active {
                let _ = event_tx.send(WatcherEvent::SessionActive(session));
            }

            debug!(
                added = diff.added.len(),
                status_changed = diff.status_changed.len(),
                "Tasks changed"
            );
        }
    }

    /// Check for inactive sessions (static version)
    async fn check_inactive_sessions_static(
        sessions: &Arc<RwLock<HashMap<String, CCSession>>>,
        event_tx: &broadcast::Sender<WatcherEvent>,
    ) {
        let now = Utc::now();
        let five_minutes_ago = now - Duration::minutes(5);

        let mut sessions = sessions.write().await;
        for session in sessions.values_mut() {
            if session.is_active && session.modified < five_minutes_ago {
                session.is_active = false;
                let _ = event_tx.send(WatcherEvent::SessionInactive(session.clone()));
            }
        }
    }

    /// Rescan active sessions to catch missed file events (e.g., after laptop sleep).
    /// Only checks file metadata (size), not content. If file grew since last tracked
    /// position, delegates to check_session_for_changes_static for actual parsing.
    async fn rescan_active_sessions(
        sessions: &Arc<RwLock<HashMap<String, CCSession>>>,
        file_positions: &Arc<RwLock<HashMap<String, u64>>>,
        recent_changes: &Arc<RwLock<Vec<CCTaskChangeEvent>>>,
        max_recent_changes: usize,
        event_tx: &broadcast::Sender<WatcherEvent>,
        projects_dir: &Path,
    ) {
        // Collect active session file paths (release read lock before processing)
        let paths: Vec<String> = {
            let sessions = sessions.read().await;
            sessions.values()
                .filter(|s| s.is_active)
                .map(|s| s.full_path.clone())
                .collect()
        };

        for full_path in &paths {
            let file_path = Path::new(full_path);
            let current_size = match tokio::fs::metadata(file_path).await {
                Ok(m) => m.len(),
                Err(_) => continue,
            };
            let tracked_size = file_positions.read().await.get(full_path).copied().unwrap_or(0);
            if current_size > tracked_size {
                debug!(
                    path = %full_path,
                    tracked = tracked_size,
                    current = current_size,
                    "Rescan: detected missed file growth"
                );
                Self::check_session_for_changes_static(
                    file_path,
                    sessions,
                    recent_changes,
                    max_recent_changes,
                    event_tx,
                    projects_dir,
                    file_positions,
                ).await;
            }
        }
    }

    // ============ Public API ============

    /// Get all sessions
    pub async fn get_all_sessions(&self) -> Vec<CCSession> {
        self.sessions.read().await.values().cloned().collect()
    }

    /// Get active sessions (updated within last 5 minutes)
    pub async fn get_active_sessions(&self) -> Vec<CCSession> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.is_active)
            .cloned()
            .collect()
    }

    /// Get sessions by project path
    pub async fn get_sessions_by_project(&self, project_path: &str) -> Vec<CCSession> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.project_path.contains(project_path) || s.project_name.contains(project_path))
            .cloned()
            .collect()
    }

    /// Get a specific session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<CCSession> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Get tasks for a specific session
    pub async fn get_session_tasks(&self, session_id: &str) -> Option<Vec<CCTask>> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|s| s.tasks.clone())
    }

    /// Get all in-progress tasks across all sessions
    pub async fn get_in_progress_tasks(&self) -> Vec<CCInProgressTask> {
        let sessions = self.sessions.read().await;
        let mut result = Vec::new();

        for session in sessions.values() {
            for task in &session.tasks {
                if task.status == CCTaskStatus::InProgress {
                    result.push(CCInProgressTask {
                        session_id: session.session_id.clone(),
                        project_path: session.project_path.clone(),
                        project_name: session.project_name.clone(),
                        summary: session.summary.clone(),
                        task: task.clone(),
                        modified: session.modified,
                    });
                }
            }
        }

        // Sort by most recently modified
        result.sort_by(|a, b| b.modified.cmp(&a.modified));
        result
    }

    /// Get tasks overview statistics
    pub async fn get_overview(&self) -> CCTasksOverview {
        let sessions: Vec<_> = self.sessions.read().await.values().cloned().collect();
        let mut pending = 0;
        let mut in_progress = 0;
        let mut completed = 0;
        let mut sessions_with_tasks = 0;

        for session in &sessions {
            if !session.tasks.is_empty() {
                sessions_with_tasks += 1;
            }
            for task in &session.tasks {
                match task.status {
                    CCTaskStatus::Pending => pending += 1,
                    CCTaskStatus::InProgress => in_progress += 1,
                    CCTaskStatus::Completed => completed += 1,
                }
            }
        }

        let recent_changes: Vec<_> = self
            .recent_changes
            .read()
            .await
            .iter()
            .take(10)
            .cloned()
            .collect();

        CCTasksOverview {
            total_sessions: sessions.len(),
            active_sessions: sessions.iter().filter(|s| s.is_active).count(),
            tasks_by_status: TasksByStatus {
                pending,
                in_progress,
                completed,
            },
            sessions_with_tasks,
            recent_changes,
        }
    }

    /// Get recent task changes
    pub async fn get_recent_changes(&self, limit: usize) -> Vec<CCTaskChangeEvent> {
        self.recent_changes
            .read()
            .await
            .iter()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Force refresh a specific session
    pub async fn refresh_session(&self, session_id: &str) -> Option<CCSession> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)?;

        let tasks = parse_last_todos(Path::new(&session.full_path), 100).await;
        session.tasks = tasks;
        session.modified = Utc::now();

        Some(session.clone())
    }

    /// Force refresh all sessions
    pub async fn refresh_all(&self) {
        // Re-scan all projects
        if !self.projects_dir.exists() {
            return;
        }

        let mut entries = match tokio::fs::read_dir(&self.projects_dir).await {
            Ok(e) => e,
            Err(_) => return,
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let index_path = path.join("sessions-index.json");
            Self::load_project_sessions_static(
                &index_path,
                &self.sessions,
                &self.file_positions,
                &self.event_tx,
            )
            .await;
        }
    }
}
