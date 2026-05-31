//! AgyCliWatcher — monitors Antigravity (`agy`) CLI session trajectories for
//! new messages.
//!
//! Parallel to GeminiCliWatcher: watches
//! `~/.gemini/antigravity-cli/brain/<conversationId>/.system_generated/logs/transcript_full.jsonl`,
//! detects incremental steps via a step-count cursor (not byte offset),
//! converts to CCMessageLine, and emits WatcherEvent::NewMessages into the
//! shared broadcast channel consumed by ConversationLoggerWorker.
//!
//! Cursor persistence reuses the generic `consumer_watermarks` table
//! (consumer = "agy_cli") via the InfraStore watermark API, so no new migration
//! is required. Like the other watchers, the cursor only advances in the DB
//! AFTER ConversationLoggerWorker confirms the PG insert (Ack-based).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::cc_tasks::WatcherEvent;
use crate::db::traits::MissionStore;

use super::parser::{
    agy_steps_to_cc, discover_sessions, parse_session, session_id_from_transcript, HistoryIndex,
};

/// Consumer key for the generic watermark cursor store.
const AGY_CURSOR_CONSUMER: &str = "agy_cli";

/// Configuration for AgyCliWatcher.
pub struct AgyCliWatcherOptions {
    /// Antigravity CLI home, defaults to ~/.gemini/antigravity-cli
    /// (override via MISSIOND_AGY_CLI_HOME).
    pub agy_home: Option<PathBuf>,
    /// Broadcast sender shared with CCTasksWatcher / GeminiCliWatcher.
    pub event_tx: broadcast::Sender<WatcherEvent>,
    /// Store for cursor persistence.
    pub store: Option<Arc<dyn MissionStore>>,
}

/// Watches Antigravity CLI trajectories and emits WatcherEvent::NewMessages.
pub struct AgyCliWatcher {
    brain_root: PathBuf,
    agy_home: PathBuf,
    /// conversationId → workspace cwd (from history.jsonl).
    history: Arc<RwLock<HistoryIndex>>,
    /// transcript_path → last processed step count.
    step_cursors: Arc<RwLock<HashMap<String, usize>>>,
    event_tx: broadcast::Sender<WatcherEvent>,
    store: Option<Arc<dyn MissionStore>>,
    _watcher: Option<RecommendedWatcher>,
    /// Channel for Ack-based cursor persistence.
    cursor_persist_tx: mpsc::UnboundedSender<(String, u64)>,
}

impl AgyCliWatcher {
    pub fn new(options: AgyCliWatcherOptions) -> Self {
        let agy_home = options.agy_home.unwrap_or_else(default_agy_home);
        let brain_root = agy_home.join("brain");

        let (cursor_persist_tx, cursor_persist_rx) = mpsc::unbounded_channel();
        if let Some(ref store) = options.store {
            let store = Arc::clone(store);
            tokio::spawn(async move {
                agy_cursor_persist_loop(store, cursor_persist_rx).await;
            });
        }

        Self {
            brain_root,
            agy_home,
            history: Arc::new(RwLock::new(HistoryIndex::default())),
            step_cursors: Arc::new(RwLock::new(HashMap::new())),
            event_tx: options.event_tx,
            store: options.store,
            _watcher: None,
            cursor_persist_tx,
        }
    }

    /// Ack-based cursor persistence: called by the main.rs Ack router after
    /// ConversationLoggerWorker confirms the PG insert. This is the ONLY path
    /// that advances the agy cursor in the database.
    pub fn persist_cursor_ack(&self, file_path: &str, step_count: u64) {
        let path = file_path.to_string();
        let step_cursors = Arc::clone(&self.step_cursors);
        tokio::spawn(async move {
            let mut cursors = step_cursors.write().await;
            let entry = cursors.entry(path).or_insert(0);
            *entry = (*entry).max(step_count as usize);
        });
        let _ = self
            .cursor_persist_tx
            .send((file_path.to_string(), step_count));
    }

    /// Start the watcher. Loads the history index, restores persisted cursors,
    /// records startup visibility, then starts fsevents monitoring.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if !self.brain_root.exists() {
            info!(
                brain = %self.brain_root.display(),
                "Antigravity brain dir does not exist, agy CLI watcher skipped"
            );
            return Ok(());
        }

        // 1. Load history index (conversationId → workspace cwd)
        *self.history.write().await = HistoryIndex::load(&self.agy_home).await;

        // 2. Restore cursors from the generic watermark store
        if let Some(ref store) = self.store {
            match store.watermark_list(AGY_CURSOR_CONSUMER).await {
                Ok(entries) if !entries.is_empty() => {
                    let mut cursors = self.step_cursors.write().await;
                    for (path, count, _) in entries {
                        if let Some(count) = count {
                            cursors.insert(path, count as usize);
                        }
                    }
                    info!(count = cursors.len(), "Restored agy CLI cursors from DB");
                }
                Ok(_) => {}
                Err(e) => warn!(error = %e, "Failed to load agy CLI cursors"),
            }
        }

        // 3. Initial scan — keep uncursored files open for startup catchup.
        self.initial_scan().await;

        // 4. Start fsevents watcher over the brain root (recursive: transcripts
        //    live three levels down under .system_generated/logs/).
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
        watcher.watch(&self.brain_root, RecursiveMode::Recursive)?;
        self._watcher = Some(watcher);

        // 5. Spawn event handler
        let history = self.history.clone();
        let step_cursors = self.step_cursors.clone();
        let event_tx = self.event_tx.clone();
        let agy_home = self.agy_home.clone();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                for path in &event.paths {
                    if !is_agy_transcript(path) {
                        continue;
                    }
                    process_file_change(path, &agy_home, &history, &step_cursors, &event_tx).await;
                }
            }
        });

        info!(brain = %self.brain_root.display(), "AgyCliWatcher started");
        Ok(())
    }

    /// Record startup visibility without marking existing transcript steps as
    /// consumed. On a first boot of the AGY watcher there is no DB watermark
    /// yet, so anchoring to the current step count here would cause
    /// run_startup_catchup() to skip the existing conversation entirely.
    async fn initial_scan(&self) {
        let sessions = discover_sessions(&self.brain_root).await;
        let cursors = self.step_cursors.read().await;
        let mut known = 0usize;
        let mut uncursored = 0usize;
        for (_, transcript) in &sessions {
            let key = transcript.to_string_lossy().to_string();
            if cursors.contains_key(&key) {
                known += 1;
                continue;
            }
            uncursored += 1;
        }
        if !sessions.is_empty() {
            info!(
                known,
                uncursored,
                "agy CLI initial scan: uncursored files will be replayed by startup catchup"
            );
        }
    }

    /// Emit events for any steps between the DB cursor and current file state.
    /// MUST be called AFTER broadcast receivers are created.
    pub async fn run_startup_catchup(&self) {
        let sessions = discover_sessions(&self.brain_root).await;
        let history = self.history.read().await;
        let cursors = self.step_cursors.read().await;
        let mut caught_up = 0usize;

        for (session_id, transcript) in sessions {
            let key = transcript.to_string_lossy().to_string();
            let cursor = cursors.get(&key).copied().unwrap_or(0);

            let Some(session) = parse_session(&transcript).await else {
                continue;
            };
            if session.steps.len() <= cursor {
                continue;
            }

            let cwd = history
                .resolve_cwd(&session_id)
                .unwrap_or_else(|| format!("agy://{session_id}"));
            let new_steps = &session.steps[cursor..];
            let cc_lines = agy_steps_to_cc(new_steps, &session_id, &cwd);

            if !cc_lines.is_empty() {
                let _ = self.event_tx.send(WatcherEvent::NewMessages {
                    session_id: session_id.clone(),
                    project_path: cwd,
                    jsonl_path: key.clone(),
                    messages: cc_lines,
                    read_end_offset: session.steps.len() as u64,
                    source: AGY_CURSOR_CONSUMER.to_string(),
                });
                caught_up += new_steps.len();
            }
        }

        if caught_up > 0 {
            info!(caught_up, "agy CLI startup catchup: emitted pending steps");
        }
    }
}

/// Process a single transcript change event.
/// NOTE: Does NOT persist cursor to DB — cursor advances only via Ack from
/// ConversationLoggerWorker (see persist_cursor_ack).
async fn process_file_change(
    path: &Path,
    agy_home: &Path,
    history: &RwLock<HistoryIndex>,
    step_cursors: &RwLock<HashMap<String, usize>>,
    event_tx: &broadcast::Sender<WatcherEvent>,
) {
    let key = path.to_string_lossy().to_string();

    let Some(session) = parse_session(path).await else {
        return;
    };

    let cursor = {
        let cursors = step_cursors.read().await;
        cursors.get(&key).copied().unwrap_or(0)
    };

    let total = session.steps.len();
    if total <= cursor {
        return;
    }

    // Resolve cwd from the history index; hot-reload on a cache miss so a brand
    // new conversation's workspace is picked up without a restart.
    let cwd = {
        let idx = history.read().await;
        match idx.resolve_cwd(&session.session_id) {
            Some(cwd) => cwd,
            None => {
                drop(idx);
                let fresh = HistoryIndex::load(agy_home).await;
                let resolved = fresh.resolve_cwd(&session.session_id);
                *history.write().await = fresh;
                resolved.unwrap_or_else(|| format!("agy://{}", session.session_id))
            }
        }
    };

    let new_steps = &session.steps[cursor..];
    let cc_lines = agy_steps_to_cc(new_steps, &session.session_id, &cwd);

    if !cc_lines.is_empty() {
        debug!(
            session_id = %session.session_id,
            new = new_steps.len(),
            total,
            "AgyCli: new steps detected"
        );
        let _ = event_tx.send(WatcherEvent::NewMessages {
            session_id: session.session_id.clone(),
            project_path: cwd,
            jsonl_path: key.clone(),
            messages: cc_lines,
            read_end_offset: total as u64,
            source: AGY_CURSOR_CONSUMER.to_string(),
        });
    }
}

/// Ack-based cursor persistence loop: micro-batches every 10s into the generic
/// `consumer_watermarks` table.
async fn agy_cursor_persist_loop(
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
                    if let Err(e) = store
                        .watermark_advance_msg_id(AGY_CURSOR_CONSUMER, path, *count as i64)
                        .await
                    {
                        warn!(error = %e, path, "Failed to persist agy cursor");
                    }
                }
                debug!(count = batch.len(), "agy cursors persisted");
            }
        }
    }
}

/// Default Antigravity CLI home: $MISSIOND_AGY_CLI_HOME or ~/.gemini/antigravity-cli.
fn default_agy_home() -> PathBuf {
    if let Ok(custom) = std::env::var("MISSIOND_AGY_CLI_HOME") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".gemini").join("antigravity-cli"))
        .unwrap_or_else(|| PathBuf::from("~/.gemini/antigravity-cli"))
}

/// True if a path is an Antigravity full-trajectory transcript.
fn is_agy_transcript(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("transcript_full.jsonl")
        && session_id_from_transcript(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_full_transcript_only() {
        assert!(is_agy_transcript(Path::new(
            "/h/.gemini/antigravity-cli/brain/c1/.system_generated/logs/transcript_full.jsonl"
        )));
        // The truncated variant is ignored — we ingest the full trajectory.
        assert!(!is_agy_transcript(Path::new(
            "/h/.gemini/antigravity-cli/brain/c1/.system_generated/logs/transcript.jsonl"
        )));
        assert!(!is_agy_transcript(Path::new("/h/.gemini/projects.json")));
    }

    #[test]
    fn default_home_honors_env_override() {
        // Falls back to ~/.gemini/antigravity-cli when env is unset; exact path
        // is host-dependent, so just assert the suffix shape.
        let home = default_agy_home();
        assert!(
            home.ends_with("antigravity-cli") || home.to_string_lossy().contains("antigravity")
        );
    }

    #[tokio::test]
    async fn initial_scan_does_not_anchor_uncursored_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir
            .path()
            .join("brain")
            .join("c1")
            .join(".system_generated")
            .join("logs")
            .join("transcript_full.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(
            &transcript,
            r#"{"step_index":0,"type":"USER_INPUT","content":"hello"}"#,
        )
        .unwrap();

        let (tx, _) = broadcast::channel(4);
        let watcher = AgyCliWatcher::new(AgyCliWatcherOptions {
            agy_home: Some(dir.path().to_path_buf()),
            event_tx: tx,
            store: None,
        });

        watcher.initial_scan().await;

        let key = transcript.to_string_lossy().to_string();
        let cursors = watcher.step_cursors.read().await;
        assert!(
            !cursors.contains_key(&key),
            "uncursored AGY transcript must remain eligible for startup catchup"
        );
    }

    #[tokio::test]
    async fn startup_catchup_replays_uncursored_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir
            .path()
            .join("brain")
            .join("c1")
            .join(".system_generated")
            .join("logs")
            .join("transcript_full.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(
            &transcript,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","content":"hello"}"#,
                "\n",
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","content":"hi"}"#,
                "\n"
            ),
        )
        .unwrap();

        let (tx, mut rx) = broadcast::channel(4);
        let watcher = AgyCliWatcher::new(AgyCliWatcherOptions {
            agy_home: Some(dir.path().to_path_buf()),
            event_tx: tx,
            store: None,
        });

        watcher.initial_scan().await;
        watcher.run_startup_catchup().await;

        match rx.try_recv().unwrap() {
            WatcherEvent::NewMessages {
                session_id,
                jsonl_path,
                messages,
                read_end_offset,
                source,
                ..
            } => {
                assert_eq!(session_id, "c1");
                assert_eq!(jsonl_path, transcript.to_string_lossy());
                assert_eq!(messages.len(), 2);
                assert_eq!(read_end_offset, 2);
                assert_eq!(source, AGY_CURSOR_CONSUMER);
            }
            other => panic!("unexpected watcher event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn startup_catchup_does_not_advance_memory_cursor_before_ack() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir
            .path()
            .join("brain")
            .join("c1")
            .join(".system_generated")
            .join("logs")
            .join("transcript_full.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(
            &transcript,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","content":"hello"}"#,
                "\n",
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","content":"hi"}"#,
                "\n"
            ),
        )
        .unwrap();

        let (tx, _rx) = broadcast::channel(4);
        let watcher = AgyCliWatcher::new(AgyCliWatcherOptions {
            agy_home: Some(dir.path().to_path_buf()),
            event_tx: tx,
            store: None,
        });
        let key = transcript.to_string_lossy().to_string();

        watcher.run_startup_catchup().await;
        assert!(
            !watcher.step_cursors.read().await.contains_key(&key),
            "emitting to the broadcast channel must not mark AGY steps consumed"
        );

        watcher.persist_cursor_ack(&key, 2);
        tokio::task::yield_now().await;
        assert_eq!(watcher.step_cursors.read().await.get(&key), Some(&2));
    }
}
