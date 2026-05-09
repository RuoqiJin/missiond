use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use missiond_core::event::events::{BoardEvent, SystemEvent};
use missiond_core::event::subscription::{CursorFlush, StartFrom, SubscriptionOpts};
use missiond_core::types::CreateBoardTaskInput;
use notify::Watcher;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio::sync::{watch, RwLock};
use tracing::{info, warn};

use crate::bus::BusServices;
use crate::state::AppState;

const LISP_CODE_SYNC_SUBSCRIPTION: &str = "lisp_code_sync_config_changed_v1_live";
const LISP_CODE_SYNC_REPORT_DIR: &str = ".missiond/v3/runtime/lisp-code-sync";
const LISP_CODE_SYNC_WATCH_ENV: &str = "MISSIOND_LISP_CODE_SYNC_WATCH";
const LISP_CODE_SYNC_DEBOUNCE_WINDOW: Duration = Duration::from_secs(5);
const LISP_CODE_SYNC_MAX_REPORTS: usize = 200;
const LISP_CODE_SYNC_MAX_REPORT_AGE_SECS: u64 = 7 * 24 * 60 * 60;

static LISP_CODE_SYNC_RUNTIME: OnceLock<Arc<LispCodeSyncRuntime>> = OnceLock::new();

#[derive(Debug)]
struct LispCodeSyncRuntime {
    events_seen: AtomicU64,
    reports_written: AtomicU64,
    sync_tasks_created: AtomicU64,
    dedupe_hits: AtomicU64,
    last_event_seq: AtomicI64,
    last_event_at_epoch: AtomicI64,
    last_project_id: RwLock<Option<String>>,
    last_path: RwLock<Option<String>>,
    last_status: RwLock<Option<String>>,
    last_task_id: RwLock<Option<String>>,
    last_report_path: RwLock<Option<String>>,
    last_error: RwLock<Option<String>>,
}

impl Default for LispCodeSyncRuntime {
    fn default() -> Self {
        Self {
            events_seen: AtomicU64::new(0),
            reports_written: AtomicU64::new(0),
            sync_tasks_created: AtomicU64::new(0),
            dedupe_hits: AtomicU64::new(0),
            last_event_seq: AtomicI64::new(0),
            last_event_at_epoch: AtomicI64::new(0),
            last_project_id: RwLock::new(None),
            last_path: RwLock::new(None),
            last_status: RwLock::new(None),
            last_task_id: RwLock::new(None),
            last_report_path: RwLock::new(None),
            last_error: RwLock::new(None),
        }
    }
}

impl LispCodeSyncRuntime {
    async fn record_event(&self, seq: i64, path: &str) {
        self.events_seen.fetch_add(1, Ordering::Relaxed);
        self.last_event_seq.store(seq, Ordering::Relaxed);
        self.last_event_at_epoch
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        *self.last_path.write().await = Some(path.to_string());
    }

    async fn record_result(&self, result: &LispCodeSyncResult) {
        *self.last_project_id.write().await = Some(result.project_id.clone());
        *self.last_status.write().await = Some(result.status.as_str().to_string());
        *self.last_task_id.write().await = result.sync_task_id.clone();
        *self.last_report_path.write().await = Some(result.report_path.display().to_string());
        *self.last_error.write().await = None;
        self.reports_written.fetch_add(1, Ordering::Relaxed);
        if result.created_task {
            self.sync_tasks_created.fetch_add(1, Ordering::Relaxed);
        } else if result.dedupe_hit {
            self.dedupe_hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn record_error(&self, err: impl Into<String>) {
        *self.last_error.write().await = Some(err.into());
    }

    async fn snapshot(&self) -> Value {
        json!({
            "schema": "missiond.lisp-code-sync-status.v1",
            "subscription": LISP_CODE_SYNC_SUBSCRIPTION,
            "watchEnabled": lisp_code_sync_watch_enabled(),
            "eventsSeen": self.events_seen.load(Ordering::Relaxed),
            "reportsWritten": self.reports_written.load(Ordering::Relaxed),
            "syncTasksCreated": self.sync_tasks_created.load(Ordering::Relaxed),
            "dedupeHits": self.dedupe_hits.load(Ordering::Relaxed),
            "lastEventSeq": self.last_event_seq.load(Ordering::Relaxed),
            "lastEventAtEpoch": self.last_event_at_epoch.load(Ordering::Relaxed),
            "lastProjectId": self.last_project_id.read().await.clone(),
            "lastPath": self.last_path.read().await.clone(),
            "lastStatus": self.last_status.read().await.clone(),
            "lastTaskId": self.last_task_id.read().await.clone(),
            "lastReportPath": self.last_report_path.read().await.clone(),
            "lastError": self.last_error.read().await.clone(),
        })
    }
}

fn runtime() -> Arc<LispCodeSyncRuntime> {
    LISP_CODE_SYNC_RUNTIME
        .get_or_init(|| Arc::new(LispCodeSyncRuntime::default()))
        .clone()
}

pub(crate) async fn status_snapshot() -> Value {
    runtime().snapshot().await
}

#[derive(Clone)]
struct LispCodeSyncService {
    bus: Arc<BusServices>,
    state: AppState,
    runtime: Arc<LispCodeSyncRuntime>,
}

pub(crate) fn start_lisp_code_sync_service(
    bus: &Arc<BusServices>,
    state: &AppState,
    shutdown_rx: watch::Receiver<bool>,
) {
    let service = LispCodeSyncService {
        bus: bus.clone(),
        state: state.clone(),
        runtime: runtime(),
    };
    tokio::spawn(service.clone().run_subscription(shutdown_rx.clone()));
    if lisp_code_sync_watch_enabled() {
        tokio::spawn(service.run_file_watcher(shutdown_rx));
        info!("lisp-code-sync service started (.missiond Lisp watcher -> EventBus -> sync report)");
    } else {
        info!(
            env = LISP_CODE_SYNC_WATCH_ENV,
            "lisp-code-sync watcher disabled; ConfigChanged subscription remains active"
        );
    }
}

fn lisp_code_sync_watch_enabled() -> bool {
    !matches!(
        std::env::var(LISP_CODE_SYNC_WATCH_ENV)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("0" | "false" | "no" | "off")
    )
}

impl LispCodeSyncService {
    async fn run_subscription(self, mut shutdown: watch::Receiver<bool>) {
        let mut opts = SubscriptionOpts::named(LISP_CODE_SYNC_SUBSCRIPTION);
        opts.start_from = StartFrom::Latest;
        opts.cursor_flush = CursorFlush::PerEvent;
        let mut sub = match self
            .bus
            .subscribe::<SystemEvent>(LISP_CODE_SYNC_SUBSCRIPTION, opts)
            .await
        {
            Ok(sub) => sub,
            Err(err) => {
                warn!(error = %err, "lisp-code-sync subscription failed");
                return;
            }
        };

        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let seq = ack.seq().0;
                    let event = ack.event().clone();
                    if let SystemEvent::ConfigChanged { path, .. } = &event {
                        if is_lisp_sync_path(Path::new(path)) {
                            self.runtime.record_event(seq, path).await;
                            match process_lisp_change(&self.state, path).await {
                                Ok(result) => self.runtime.record_result(&result).await,
                                Err(err) => {
                                    warn!(error = %err, path = %path, "lisp-code-sync processing failed");
                                    self.runtime.record_error(err.to_string()).await;
                                }
                            }
                        }
                    }
                    ack.ack().await;
                }
            }
        }
    }

    async fn run_file_watcher(self, mut shutdown: watch::Receiver<bool>) {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<notify::Event>(128);
        let mut watcher = match notify::RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            notify::Config::default(),
        ) {
            Ok(watcher) => watcher,
            Err(err) => {
                warn!(error = %err, "lisp-code-sync watcher creation failed");
                return;
            }
        };

        let roots = watched_missiond_dirs(&self.state).await;
        for dir in &roots {
            if let Err(err) =
                notify::Watcher::watch(&mut watcher, dir, notify::RecursiveMode::Recursive)
            {
                warn!(error = %err, path = %dir.display(), "lisp-code-sync failed to watch project .missiond dir");
            }
        }
        info!(
            count = roots.len(),
            "lisp-code-sync watching project .missiond directories"
        );

        let mut debounced_paths: HashMap<String, Instant> = HashMap::new();

        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                Some(event) = rx.recv() => {
                    if !is_relevant_notify_event(&event.kind) {
                        continue;
                    }
                    let kind = notify_kind(&event.kind).to_string();
                    for path in event.paths {
                        if !is_lisp_sync_path(&path) {
                            continue;
                        }
                        let display = path.display().to_string();
                        let now = Instant::now();
                        if let Some(last_seen) = debounced_paths.get(&display) {
                            if now.duration_since(*last_seen) < LISP_CODE_SYNC_DEBOUNCE_WINDOW {
                                continue;
                            }
                        }
                        debounced_paths.insert(display.clone(), now);
                        let _ = self.bus.publish_system(SystemEvent::ConfigChanged {
                            path: display,
                            kind: kind.clone(),
                        }).await;
                    }
                }
            }
        }
    }
}

async fn watched_missiond_dirs(state: &AppState) -> Vec<PathBuf> {
    let registry = state.project_registry.read().await;
    let mut dirs: Vec<PathBuf> = registry
        .active_projects()
        .into_iter()
        .map(|project| PathBuf::from(&project.path).join(".missiond"))
        .filter(|dir| dir.exists())
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn is_relevant_notify_event(kind: &notify::EventKind) -> bool {
    matches!(
        kind,
        notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
    )
}

fn notify_kind(kind: &notify::EventKind) -> &'static str {
    match kind {
        notify::EventKind::Remove(_) => "deleted",
        notify::EventKind::Create(_) => "created",
        _ => "modified",
    }
}

pub(crate) fn is_lisp_sync_path(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    if !text.contains("/.missiond/") && !text.starts_with(".missiond/") {
        return false;
    }
    if is_ignored_lisp_sync_runtime_path(&text) {
        return false;
    }
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("lisp" | "mjs")
    )
}

fn is_ignored_lisp_sync_runtime_path(path: &str) -> bool {
    path.contains("/.missiond/v3/runtime/")
        || path.starts_with(".missiond/v3/runtime/")
        || path.contains("/.missiond/runtime-state/")
        || path.starts_with(".missiond/runtime-state/")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LispCodeSyncStatus {
    Synced,
    NeedsSync,
    ObservedOnly,
    UnknownProject,
}

impl LispCodeSyncStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::NeedsSync => "needs-sync",
            Self::ObservedOnly => "observed-only",
            Self::UnknownProject => "unknown-project",
        }
    }
}

#[derive(Debug, Clone)]
struct ProjectResolution {
    project_id: String,
    root: PathBuf,
}

#[derive(Debug, Clone)]
struct LispCodeSyncResult {
    project_id: String,
    status: LispCodeSyncStatus,
    sync_task_id: Option<String>,
    created_task: bool,
    dedupe_hit: bool,
    report_path: PathBuf,
}

async fn process_lisp_change(state: &AppState, changed_path: &str) -> Result<LispCodeSyncResult> {
    let resolution = match resolve_project_for_path(state, Path::new(changed_path)).await {
        Some(resolution) => resolution,
        None => {
            let root = missiond_fallback_root(state).await;
            let report = LispCodeSyncReport {
                project_id: "unknown".to_string(),
                changed_path: changed_path.to_string(),
                status: LispCodeSyncStatus::UnknownProject,
                checker_ok: None,
                checker_command: None,
                checker_tail: None,
                sync_task_id: None,
                dedupe_key: None,
            };
            let report_path = write_report(&root, &report).await?;
            return Ok(LispCodeSyncResult {
                project_id: "unknown".to_string(),
                status: LispCodeSyncStatus::UnknownProject,
                sync_task_id: None,
                created_task: false,
                dedupe_hit: false,
                report_path,
            });
        }
    };

    let check = run_project_sync_check(&resolution).await;
    let status = match &check {
        Some(result) if result.ok => LispCodeSyncStatus::Synced,
        Some(_) => LispCodeSyncStatus::NeedsSync,
        None => LispCodeSyncStatus::ObservedOnly,
    };
    let dedupe_key = (status == LispCodeSyncStatus::NeedsSync)
        .then(|| dedupe_key_for_lisp_sync(&resolution.project_id, changed_path));
    let mut sync_task_id = None;
    let mut created_task = false;
    let mut dedupe_hit = false;

    if let Some(dedupe_key) = dedupe_key.as_deref() {
        if let Some(existing) = state.store.find_open_task_by_dedupe_key(dedupe_key).await? {
            sync_task_id = Some(existing.id.to_string());
            dedupe_hit = true;
        } else {
            let task_id =
                create_sync_task(state, &resolution, changed_path, &check, dedupe_key).await?;
            sync_task_id = Some(task_id);
            created_task = true;
        }
    }

    let report = LispCodeSyncReport {
        project_id: resolution.project_id.clone(),
        changed_path: changed_path.to_string(),
        status,
        checker_ok: check.as_ref().map(|result| result.ok),
        checker_command: check.as_ref().map(|result| result.command.clone()),
        checker_tail: check.as_ref().map(|result| result.tail.clone()),
        sync_task_id: sync_task_id.clone(),
        dedupe_key,
    };
    let report_path = write_report(&resolution.root, &report).await?;

    Ok(LispCodeSyncResult {
        project_id: resolution.project_id,
        status,
        sync_task_id,
        created_task,
        dedupe_hit,
        report_path,
    })
}

async fn resolve_project_for_path(state: &AppState, path: &Path) -> Option<ProjectResolution> {
    let abs = path
        .canonicalize()
        .ok()
        .unwrap_or_else(|| path.to_path_buf());
    let registry = state.project_registry.read().await;
    let path_text = abs.to_string_lossy();
    if let Some(project_id) = registry.resolve(&path_text) {
        let project = registry.get(project_id)?;
        return Some(ProjectResolution {
            project_id: project.id.clone(),
            root: PathBuf::from(&project.path),
        });
    }
    None
}

#[derive(Debug, Clone)]
struct SyncCheckResult {
    ok: bool,
    command: String,
    tail: String,
}

async fn run_project_sync_check(resolution: &ProjectResolution) -> Option<SyncCheckResult> {
    if resolution.project_id == "missiond" {
        let compile = run_command(
            &resolution.root,
            &["node", "scripts/compile-v3-runtime.mjs", "--json"],
            Duration::from_secs(60),
        )
        .await;
        if !compile.ok {
            return Some(compile);
        }
        return Some(
            run_command(
                &resolution.root,
                &[
                    "node",
                    "scripts/check-v3-code-isomorphism-complete.mjs",
                    "--json",
                ],
                Duration::from_secs(120),
            )
            .await,
        );
    }

    let check_sh = resolution.root.join(".missiond/check.sh");
    if check_sh.exists() {
        return Some(
            run_command(
                &resolution.root,
                &["bash", ".missiond/check.sh"],
                Duration::from_secs(120),
            )
            .await,
        );
    }
    None
}

async fn run_command(root: &Path, argv: &[&str], timeout: Duration) -> SyncCheckResult {
    let command = argv.join(" ");
    let output = tokio::time::timeout(
        timeout,
        Command::new(argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output(),
    )
    .await;
    match output {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}\n{}", tail(&stdout, 3000), tail(&stderr, 2000));
            SyncCheckResult {
                ok: output.status.success(),
                command,
                tail: combined.trim().to_string(),
            }
        }
        Ok(Err(err)) => SyncCheckResult {
            ok: false,
            command,
            tail: format!("failed to start: {err}"),
        },
        Err(_) => SyncCheckResult {
            ok: false,
            command,
            tail: "timed out".to_string(),
        },
    }
}

async fn create_sync_task(
    state: &AppState,
    resolution: &ProjectResolution,
    changed_path: &str,
    check: &Option<SyncCheckResult>,
    dedupe_key: &str,
) -> Result<String> {
    let checker = check
        .as_ref()
        .map(|result| format!("{}\n{}", result.command, result.tail))
        .unwrap_or_else(|| "no checker available".to_string());
    let description = format!(
        "Lisp-code real-time sync detected a failing SSOT/code gate.\n\nProject: {}\nChanged Lisp/checker path: {}\n\nChecker evidence:\n{}\n\nWorkflow:\n1. Read the changed Lisp/checker file and the failing gate output.\n2. Decide whether the intended change needs code, checker, or Lisp correction.\n3. Create/attach an exact accepted shard before any code implementation.\n4. Delegate implementation only with explicit write_scope, acceptance, and context_pack_path.\n5. Close this task only after the code-isomorphism gate is green.\n\nThis task exists because Lisp changed first; do not mark done by editing only the report.",
        resolution.project_id,
        changed_path,
        checker
    );
    let input = CreateBoardTaskInput {
        title: format!("Sync code for Lisp change: {}", short_path(changed_path)),
        description: Some(description),
        priority: Some("high".to_string()),
        category: Some("dev".to_string()),
        project: Some(resolution.project_id.clone()),
        auto_execute: Some(true),
        hidden: Some(false),
        dedupe_key: Some(dedupe_key.to_string()),
        context_intent: Some("code".to_string()),
        ..Default::default()
    };
    let task = state.store.create_board_task(&input).await?;
    let task_id = task.id.to_string();
    let ev = BoardEvent::TaskCreated {
        task_id: task_id.clone(),
        title: task.title.clone(),
        category: task.category.clone(),
    };
    crate::engine::master_control::notify_board_event_direct(&ev);
    let _ = state.bus.publish_board(ev).await;
    Ok(task_id)
}

fn dedupe_key_for_lisp_sync(project_id: &str, changed_path: &str) -> String {
    format!(
        "lisp-code-sync:{project_id}:{}",
        stable_hash_hex(changed_path)
    )
}

fn stable_hash_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

async fn missiond_fallback_root(state: &AppState) -> PathBuf {
    let registry = state.project_registry.read().await;
    registry
        .get("missiond")
        .map(|project| PathBuf::from(&project.path))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug)]
struct LispCodeSyncReport {
    project_id: String,
    changed_path: String,
    status: LispCodeSyncStatus,
    checker_ok: Option<bool>,
    checker_command: Option<String>,
    checker_tail: Option<String>,
    sync_task_id: Option<String>,
    dedupe_key: Option<String>,
}

async fn write_report(root: &Path, report: &LispCodeSyncReport) -> std::io::Result<PathBuf> {
    let dir = root.join(LISP_CODE_SYNC_REPORT_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let filename = format!(
        "{}-{}.report.lisp",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
        stable_hash_hex(&report.changed_path)
    );
    let path = dir.join(filename);
    tokio::fs::write(&path, render_report(report)).await?;
    if let Err(err) = prune_report_dir(&dir).await {
        warn!(error = %err, path = %dir.display(), "lisp-code-sync report retention failed");
    }
    Ok(path)
}

async fn prune_report_dir(dir: &Path) -> std::io::Result<()> {
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("lisp") {
            continue;
        }
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.ends_with(".report.lisp"))
            .unwrap_or(false)
        {
            continue;
        }
        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let modified = metadata.modified().ok();
        entries.push((path, modified));
    }

    let now = std::time::SystemTime::now();
    entries.sort_by_key(|(_, modified)| *modified);
    let keep_from_index = entries.len().saturating_sub(LISP_CODE_SYNC_MAX_REPORTS);

    for (idx, (path, modified)) in entries.into_iter().enumerate() {
        let too_many = idx < keep_from_index;
        let too_old = modified
            .and_then(|mtime| now.duration_since(mtime).ok())
            .map(|age| age.as_secs() > LISP_CODE_SYNC_MAX_REPORT_AGE_SECS)
            .unwrap_or(false);
        if too_many || too_old {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
    Ok(())
}

fn render_report(report: &LispCodeSyncReport) -> String {
    format!(
        "(lisp-code-sync-report\n  :schema \"missiond.lisp-code-sync-report.v1\"\n  :updated-at {}\n  :project {}\n  :changed-path {}\n  :status {}\n  :checker-ok {}\n  :checker-command {}\n  :checker-tail {}\n  :sync-task-id {}\n  :dedupe-key {}\n)\n",
        lisp_string(&chrono::Utc::now().to_rfc3339()),
        lisp_string(&report.project_id),
        lisp_string(&report.changed_path),
        report.status.as_str(),
        report
            .checker_ok
            .map(|ok| if ok { "true" } else { "false" })
            .unwrap_or("nil"),
        lisp_option_string(report.checker_command.as_deref()),
        lisp_option_string(report.checker_tail.as_deref()),
        lisp_option_string(report.sync_task_id.as_deref()),
        lisp_option_string(report.dedupe_key.as_deref()),
    )
}

fn short_path(path: &str) -> String {
    let path = Path::new(path);
    let parts: Vec<String> = path
        .components()
        .rev()
        .take(3)
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

fn tail(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        value.to_string()
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}

fn lisp_option_string(value: Option<&str>) -> String {
    value.map(lisp_string).unwrap_or_else(|| "nil".to_string())
}

fn lisp_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_filter_accepts_missiond_lisp_and_checker_files() {
        assert!(is_lisp_sync_path(Path::new(
            "/repo/.missiond/v3/missiond-blueprint.lisp"
        )));
        assert!(is_lisp_sync_path(Path::new(
            "/repo/.missiond/scripts/check-example.mjs"
        )));
        assert!(!is_lisp_sync_path(Path::new("/repo/src/lib.rs")));
        assert!(!is_lisp_sync_path(Path::new(
            "/repo/.missiond/evidence/readme.md"
        )));
        assert!(!is_lisp_sync_path(Path::new(
            "/repo/.missiond/v3/runtime/lisp-code-sync/20260509.report.lisp"
        )));
        assert!(!is_lisp_sync_path(Path::new(
            "/repo/.missiond/v3/runtime/compiled/compiled-v3-blueprint.json"
        )));
    }

    #[test]
    fn dedupe_key_is_stable_per_project_and_path() {
        let a = dedupe_key_for_lisp_sync("missiond", "/repo/.missiond/v3/x.lisp");
        let b = dedupe_key_for_lisp_sync("missiond", "/repo/.missiond/v3/x.lisp");
        let c = dedupe_key_for_lisp_sync("auth", "/repo/.missiond/v3/x.lisp");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("lisp-code-sync:missiond:"));
    }

    #[test]
    fn report_renders_sync_status_and_changed_path() {
        let report = LispCodeSyncReport {
            project_id: "missiond".to_string(),
            changed_path: ".missiond/v3/missiond-blueprint.lisp".to_string(),
            status: LispCodeSyncStatus::NeedsSync,
            checker_ok: Some(false),
            checker_command: Some(
                "node scripts/check-v3-code-isomorphism-complete.mjs --json".to_string(),
            ),
            checker_tail: Some("missing checker".to_string()),
            sync_task_id: Some("task-1".to_string()),
            dedupe_key: Some("lisp-code-sync:missiond:abc".to_string()),
        };
        let rendered = render_report(&report);
        assert!(rendered.contains("lisp-code-sync-report"));
        assert!(rendered.contains(":status needs-sync"));
        assert!(rendered.contains(".missiond/v3/missiond-blueprint.lisp"));
    }
}
