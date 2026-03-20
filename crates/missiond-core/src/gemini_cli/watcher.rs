//! GeminiCliWatcher — monitors Gemini CLI session files for new messages.
//!
//! Parallel to CCTasksWatcher: watches ~/.gemini/tmp/*/chats/*.json,
//! detects incremental messages via message-count cursor (not byte offset),
//! converts to CCMessageLine, and emits WatcherEvent::NewMessages into the
//! shared broadcast channel consumed by ConversationLoggerWorker.
//!
//! Cursor persistence follows the same Ack-based pattern as CCTasksWatcher:
//! cursor only advances in DB AFTER ConversationLoggerWorker confirms PG insert.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::cc_tasks::WatcherEvent;
use crate::db::traits::MissionStore;

use super::parser::{
    discover_chat_dirs, gemini_messages_to_cc, list_session_files, parse_session, ProjectMap,
};

/// Configuration for GeminiCliWatcher.
pub struct GeminiCliWatcherOptions {
    /// Gemini home directory, defaults to ~/.gemini
    pub gemini_home: Option<PathBuf>,
    /// Broadcast sender shared with CCTasksWatcher
    pub event_tx: broadcast::Sender<WatcherEvent>,
    /// Store for cursor persistence
    pub store: Option<Arc<dyn MissionStore>>,
}

/// Watches Gemini CLI session files and emits WatcherEvent::NewMessages.
pub struct GeminiCliWatcher {
    gemini_home: PathBuf,
    /// dir_name → absolute project path
    project_map: Arc<RwLock<ProjectMap>>,
    /// session_file_path → last processed message count
    msg_cursors: Arc<RwLock<HashMap<String, usize>>>,
    /// session_file_path → session_id (for cursor persistence)
    session_ids: Arc<RwLock<HashMap<String, String>>>,
    event_tx: broadcast::Sender<WatcherEvent>,
    store: Option<Arc<dyn MissionStore>>,
    _watcher: Option<RecommendedWatcher>,
    /// Channel for Ack-based cursor persistence (mirrors CC's cursor_persist_tx).
    cursor_persist_tx: mpsc::UnboundedSender<(String, u64)>,
}

impl GeminiCliWatcher {
    pub fn new(options: GeminiCliWatcherOptions) -> Self {
        let gemini_home = options.gemini_home.unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".gemini"))
                .unwrap_or_else(|| PathBuf::from("~/.gemini"))
        });

        let (cursor_persist_tx, cursor_persist_rx) = mpsc::unbounded_channel();

        // Spawn Ack-based cursor persistence loop (micro-batch every 10s)
        if let Some(ref store) = options.store {
            let store = Arc::clone(store);
            tokio::spawn(async move {
                gemini_cursor_persist_loop(store, cursor_persist_rx).await;
            });
        }

        Self {
            gemini_home,
            project_map: Arc::new(RwLock::new(ProjectMap::default())),
            msg_cursors: Arc::new(RwLock::new(HashMap::new())),
            session_ids: Arc::new(RwLock::new(HashMap::new())),
            event_tx: options.event_tx,
            store: options.store,
            _watcher: None,
            cursor_persist_tx,
        }
    }

    /// Ack-based cursor persistence: called by main.rs Ack router after
    /// ConversationLoggerWorker confirms PG insert. This is the ONLY path
    /// that advances the Gemini cursor in the database.
    pub fn persist_cursor_ack(&self, file_path: &str, msg_count: u64) {
        let _ = self.cursor_persist_tx.send((file_path.to_string(), msg_count));
    }

    /// Start the watcher. Loads project map, restores cursors, scans existing files,
    /// then starts fsevents monitoring.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let tmp_dir = self.gemini_home.join("tmp");
        if !tmp_dir.exists() {
            info!("~/.gemini/tmp does not exist, Gemini CLI watcher skipped");
            return Ok(());
        }

        // 1. Load project map
        let map = ProjectMap::load(&self.gemini_home).await;
        *self.project_map.write().await = map;

        // 2. Restore cursors from DB
        if let Some(ref store) = self.store {
            match store.load_gemini_cursors().await {
                Ok(cursors) if !cursors.is_empty() => {
                    let count = cursors.len();
                    let mut mc = self.msg_cursors.write().await;
                    for (path, count_val) in cursors {
                        mc.insert(path, count_val as usize);
                    }
                    info!(count, "Restored Gemini CLI cursors from DB");
                }
                Ok(_) => {}
                Err(e) => warn!(error = %e, "Failed to load Gemini CLI cursors"),
            }
        }

        // 3. Initial scan — anchor cursors on existing files (no events emitted)
        self.initial_scan().await;

        // 4. Start fsevents watcher
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(256);
        let watcher_tx = tx.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = watcher_tx.blocking_send(event);
                }
            },
            Config::default(),
        )?;
        watcher.watch(&tmp_dir, RecursiveMode::Recursive)?;
        self._watcher = Some(watcher);

        // 5. Spawn event handler
        let project_map = self.project_map.clone();
        let msg_cursors = self.msg_cursors.clone();
        let session_ids = self.session_ids.clone();
        let event_tx = self.event_tx.clone();
        let gemini_home = self.gemini_home.clone();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                for path in &event.paths {
                    // Only process session JSON files
                    if !is_session_file(path) {
                        // Check if projects.json changed → hot-reload project map
                        if path.ends_with("projects.json") {
                            let new_map = ProjectMap::load(&gemini_home).await;
                            *project_map.write().await = new_map;
                            debug!("Gemini project map hot-reloaded");
                        }
                        continue;
                    }

                    process_file_change(
                        path,
                        &project_map,
                        &msg_cursors,
                        &session_ids,
                        &event_tx,
                    )
                    .await;
                }
            }
        });

        info!(tmp_dir = %tmp_dir.display(), "GeminiCliWatcher started");
        Ok(())
    }

    /// Scan all existing session files and set cursors to current message counts.
    /// Does NOT emit events — this is an anchor-only pass for first-time setup.
    /// Existing files with no DB cursor get their current count recorded.
    async fn initial_scan(&self) {
        let chat_dirs = discover_chat_dirs(&self.gemini_home).await;
        let mut cursors = self.msg_cursors.write().await;
        let mut session_map = self.session_ids.write().await;
        let mut anchored = 0usize;

        for (_, chats_dir) in &chat_dirs {
            let files = list_session_files(chats_dir).await;
            for file in files {
                let key = file.to_string_lossy().to_string();
                if cursors.contains_key(&key) {
                    continue; // Already have a cursor from DB
                }
                // Anchor: record current message count without emitting events
                if let Some(session) = parse_session(&file).await {
                    session_map.insert(key.clone(), session.session_id.clone());
                    cursors.insert(key, session.messages.len());
                    anchored += 1;
                }
            }
        }

        if anchored > 0 {
            info!(anchored, "Gemini CLI initial scan: anchored new file cursors");
        }
    }

    /// Run startup catchup: emit events for any messages between DB cursor and current file state.
    /// MUST be called AFTER broadcast receivers are created.
    pub async fn run_startup_catchup(&self) {
        let chat_dirs = discover_chat_dirs(&self.gemini_home).await;
        let project_map = self.project_map.read().await;
        let mut cursors = self.msg_cursors.write().await;
        let mut session_map = self.session_ids.write().await;
        let mut caught_up = 0usize;

        for (dir_name, chats_dir) in &chat_dirs {
            let project_path = project_map
                .resolve_dir(dir_name)
                .unwrap_or_else(|| format!("gemini://{}", dir_name));

            let files = list_session_files(chats_dir).await;
            for file in files {
                let key = file.to_string_lossy().to_string();
                let cursor = cursors.get(&key).copied().unwrap_or(0);

                let Some(session) = parse_session(&file).await else {
                    continue;
                };

                if session.messages.len() <= cursor {
                    continue;
                }

                let new_msgs = &session.messages[cursor..];
                let cc_lines = gemini_messages_to_cc(new_msgs, &session, &project_path);

                if !cc_lines.is_empty() {
                    let _ = self.event_tx.send(WatcherEvent::NewMessages {
                        session_id: session.session_id.clone(),
                        project_path: project_path.clone(),
                        jsonl_path: key.clone(),
                        messages: cc_lines,
                        read_end_offset: session.messages.len() as u64,
                    });
                    caught_up += new_msgs.len();
                }

                session_map.insert(key.clone(), session.session_id.clone());
                cursors.insert(key, session.messages.len());
            }
        }

        if caught_up > 0 {
            info!(caught_up, "Gemini CLI startup catchup: emitted pending messages");
        }
    }
}

/// Process a single file change event.
/// NOTE: Does NOT persist cursor to DB — cursor advances only via Ack from
/// ConversationLoggerWorker (see persist_cursor_ack).
async fn process_file_change(
    path: &Path,
    project_map: &RwLock<ProjectMap>,
    msg_cursors: &RwLock<HashMap<String, usize>>,
    session_ids: &RwLock<HashMap<String, String>>,
    event_tx: &broadcast::Sender<WatcherEvent>,
) {
    let key = path.to_string_lossy().to_string();

    let Some(session) = parse_session(path).await else {
        return;
    };

    let cursor = {
        let cursors = msg_cursors.read().await;
        cursors.get(&key).copied().unwrap_or(0)
    };

    let total = session.messages.len();
    if total <= cursor {
        return; // No new messages
    }

    // Resolve project path from directory name
    let dir_name = path
        .parent() // chats/
        .and_then(|p| p.parent()) // project_dir/
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let project_path = {
        let map = project_map.read().await;
        match map.resolve_dir(dir_name) {
            Some(p) => p,
            None => {
                // Cache miss — hot-reload projects.json
                drop(map);
                let gemini_home = path
                    .ancestors()
                    .find(|p| p.ends_with(".gemini"))
                    .map(|p| p.to_path_buf());
                if let Some(home) = gemini_home {
                    let new_map = ProjectMap::load(&home).await;
                    let resolved = new_map.resolve_dir(dir_name);
                    *project_map.write().await = new_map;
                    resolved.unwrap_or_else(|| format!("gemini://{}", dir_name))
                } else {
                    format!("gemini://{}", dir_name)
                }
            }
        }
    };

    let new_msgs = &session.messages[cursor..];
    let cc_lines = gemini_messages_to_cc(new_msgs, &session, &project_path);

    if !cc_lines.is_empty() {
        debug!(
            session_id = %session.session_id,
            new = new_msgs.len(),
            total,
            "GeminiCli: new messages detected"
        );

        let _ = event_tx.send(WatcherEvent::NewMessages {
            session_id: session.session_id.clone(),
            project_path,
            jsonl_path: key.clone(),
            messages: cc_lines,
            read_end_offset: total as u64,
        });
    }

    // Update in-memory cursor (DB persistence deferred to Ack)
    msg_cursors.write().await.insert(key.clone(), total);
    session_ids
        .write()
        .await
        .insert(key, session.session_id.clone());
}

/// Ack-based cursor persistence loop: micro-batches every 10s.
/// Mirrors CCTasksWatcher's cursor_persist_loop pattern.
async fn gemini_cursor_persist_loop(
    store: Arc<dyn MissionStore>,
    mut rx: mpsc::UnboundedReceiver<(String, u64)>,
) {
    let mut pending: HashMap<String, u64> = HashMap::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));

    loop {
        tokio::select! {
            Some((path, count)) = rx.recv() => {
                pending.insert(path, count);
            }
            _ = interval.tick() => {
                if pending.is_empty() {
                    continue;
                }
                let batch = std::mem::take(&mut pending);
                for (path, count) in &batch {
                    // session_id is not available in the ack — use empty string,
                    // the initial insert from startup/reconcile sets the real session_id.
                    if let Err(e) = store.save_gemini_cursor(path, "", *count as i64).await {
                        warn!(error = %e, path, "Failed to persist Gemini cursor");
                    }
                }
                debug!(count = batch.len(), "Gemini cursors persisted");
            }
        }
    }
}

/// Check if a path is a Gemini CLI session file.
fn is_session_file(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    if ext != "json" {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !name.starts_with("session-") {
        return false;
    }
    // Must be under a chats/ directory
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("chats")
}

/// Get file mtime as SystemTime.
#[allow(dead_code)]
pub(crate) fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
