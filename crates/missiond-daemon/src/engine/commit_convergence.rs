use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use missiond_core::event::events::{BoardEvent, SystemEvent};
use missiond_core::event::subscription::{CursorFlush, StartFrom, SubscriptionOpts};
use missiond_core::types::CreateBoardTaskInput;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::process::Command;
use tokio::sync::{watch, RwLock};
use tracing::{info, warn};

use crate::bus::BusServices;
use crate::state::AppState;

const COMMIT_CONVERGENCE_SUBSCRIPTION: &str = "commit_convergence_contextual_commit_v1_live";
const COMMIT_CONVERGENCE_REPORT_DIR: &str = ".missiond/v3/runtime/commit-lisp-convergence";

static COMMIT_CONVERGENCE_RUNTIME: OnceLock<Arc<CommitConvergenceRuntime>> = OnceLock::new();

#[derive(Debug)]
struct CommitConvergenceRuntime {
    events_seen: AtomicU64,
    reports_written: AtomicU64,
    backfill_tasks_created: AtomicU64,
    dedupe_hits: AtomicU64,
    last_event_seq: AtomicI64,
    last_event_at_epoch: AtomicI64,
    last_commit_hash: RwLock<Option<String>>,
    last_project_id: RwLock<Option<String>>,
    last_status: RwLock<Option<String>>,
    last_task_id: RwLock<Option<String>>,
    last_report_path: RwLock<Option<String>>,
    last_error: RwLock<Option<String>>,
}

impl Default for CommitConvergenceRuntime {
    fn default() -> Self {
        Self {
            events_seen: AtomicU64::new(0),
            reports_written: AtomicU64::new(0),
            backfill_tasks_created: AtomicU64::new(0),
            dedupe_hits: AtomicU64::new(0),
            last_event_seq: AtomicI64::new(0),
            last_event_at_epoch: AtomicI64::new(0),
            last_commit_hash: RwLock::new(None),
            last_project_id: RwLock::new(None),
            last_status: RwLock::new(None),
            last_task_id: RwLock::new(None),
            last_report_path: RwLock::new(None),
            last_error: RwLock::new(None),
        }
    }
}

impl CommitConvergenceRuntime {
    async fn record_event(&self, seq: i64, commit_hash: &str) {
        self.events_seen.fetch_add(1, Ordering::Relaxed);
        self.last_event_seq.store(seq, Ordering::Relaxed);
        self.last_event_at_epoch
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        *self.last_commit_hash.write().await = Some(commit_hash.to_string());
    }

    async fn record_result(&self, result: &CommitConvergenceResult) {
        *self.last_project_id.write().await = Some(result.project_id.clone());
        *self.last_status.write().await = Some(result.status.as_str().to_string());
        *self.last_task_id.write().await = result.backfill_task_id.clone();
        *self.last_report_path.write().await = Some(result.report_path.display().to_string());
        *self.last_error.write().await = None;
        self.reports_written.fetch_add(1, Ordering::Relaxed);
        if result.created_task {
            self.backfill_tasks_created.fetch_add(1, Ordering::Relaxed);
        } else if result.dedupe_hit {
            self.dedupe_hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn record_error(&self, err: impl Into<String>) {
        *self.last_error.write().await = Some(err.into());
    }

    async fn snapshot(&self) -> Value {
        json!({
            "schema": "missiond.commit-convergence-status.v1",
            "subscription": COMMIT_CONVERGENCE_SUBSCRIPTION,
            "eventsSeen": self.events_seen.load(Ordering::Relaxed),
            "reportsWritten": self.reports_written.load(Ordering::Relaxed),
            "backfillTasksCreated": self.backfill_tasks_created.load(Ordering::Relaxed),
            "dedupeHits": self.dedupe_hits.load(Ordering::Relaxed),
            "lastEventSeq": self.last_event_seq.load(Ordering::Relaxed),
            "lastEventAtEpoch": self.last_event_at_epoch.load(Ordering::Relaxed),
            "lastCommitHash": self.last_commit_hash.read().await.clone(),
            "lastProjectId": self.last_project_id.read().await.clone(),
            "lastStatus": self.last_status.read().await.clone(),
            "lastTaskId": self.last_task_id.read().await.clone(),
            "lastReportPath": self.last_report_path.read().await.clone(),
            "lastError": self.last_error.read().await.clone(),
        })
    }
}

fn runtime() -> Arc<CommitConvergenceRuntime> {
    COMMIT_CONVERGENCE_RUNTIME
        .get_or_init(|| Arc::new(CommitConvergenceRuntime::default()))
        .clone()
}

pub(crate) async fn status_snapshot() -> Value {
    runtime().snapshot().await
}

#[derive(Clone)]
struct CommitConvergenceService {
    bus: Arc<BusServices>,
    state: AppState,
    runtime: Arc<CommitConvergenceRuntime>,
}

pub(crate) fn start_commit_convergence_service(
    bus: &Arc<BusServices>,
    state: &AppState,
    shutdown_rx: watch::Receiver<bool>,
) {
    let service = CommitConvergenceService {
        bus: bus.clone(),
        state: state.clone(),
        runtime: runtime(),
    };
    tokio::spawn(async move { service.run(shutdown_rx).await });
    info!("commit-convergence service started (ContextualCommitDetected -> Lisp backfill)");
}

impl CommitConvergenceService {
    async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut opts = SubscriptionOpts::named(COMMIT_CONVERGENCE_SUBSCRIPTION);
        opts.start_from = StartFrom::Latest;
        opts.cursor_flush = CursorFlush::PerEvent;
        let mut sub = match self
            .bus
            .subscribe::<SystemEvent>(COMMIT_CONVERGENCE_SUBSCRIPTION, opts)
            .await
        {
            Ok(sub) => sub,
            Err(err) => {
                warn!(error = %err, "commit-convergence subscription failed");
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
                    if let SystemEvent::ContextualCommitDetected { commit_hash, .. } = &event {
                        self.runtime.record_event(seq, commit_hash).await;
                        match process_commit_event(&self.state, &event).await {
                            Ok(result) => self.runtime.record_result(&result).await,
                            Err(err) => {
                                warn!(error = %err, "commit-convergence processing failed");
                                self.runtime.record_error(err.to_string()).await;
                            }
                        }
                    }
                    ack.ack().await;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CommitCoverageStatus {
    Covered,
    NeedsBackfill,
    LispOnly,
    NoChangedFiles,
    ExternalOrUnavailableCommit,
    UnknownProject,
}

impl CommitCoverageStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Covered => "covered",
            Self::NeedsBackfill => "needs-backfill",
            Self::LispOnly => "lisp-only",
            Self::NoChangedFiles => "no-changed-files",
            Self::ExternalOrUnavailableCommit => "external-or-unavailable-commit",
            Self::UnknownProject => "unknown-project",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ChangedFileClasses {
    pub code: Vec<String>,
    pub lisp: Vec<String>,
    pub checker: Vec<String>,
    pub evidence: Vec<String>,
    pub docs: Vec<String>,
    pub other: Vec<String>,
}

#[derive(Debug, Clone)]
struct ProjectResolution {
    project_id: String,
    root: PathBuf,
}

#[derive(Debug, Clone)]
struct CommitConvergenceResult {
    project_id: String,
    status: CommitCoverageStatus,
    backfill_task_id: Option<String>,
    created_task: bool,
    dedupe_hit: bool,
    report_path: PathBuf,
}

pub(crate) fn classify_changed_files(
    files: &[String],
) -> (CommitCoverageStatus, ChangedFileClasses) {
    let mut classes = ChangedFileClasses::default();
    for file in files {
        classify_changed_file(file, &mut classes);
    }
    let has_code = !classes.code.is_empty();
    let has_lisp_coverage =
        !classes.lisp.is_empty() || !classes.checker.is_empty() || !classes.evidence.is_empty();
    let status = if files.is_empty() {
        CommitCoverageStatus::NoChangedFiles
    } else if has_code && has_lisp_coverage {
        CommitCoverageStatus::Covered
    } else if has_code {
        CommitCoverageStatus::NeedsBackfill
    } else if has_lisp_coverage {
        CommitCoverageStatus::LispOnly
    } else {
        CommitCoverageStatus::Covered
    };
    (status, classes)
}

fn classify_changed_file(file: &str, classes: &mut ChangedFileClasses) {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".lisp") {
        classes.lisp.push(file.to_string());
    } else if lower.starts_with("scripts/check-") && lower.ends_with(".mjs") {
        classes.checker.push(file.to_string());
    } else if lower.contains("/evidence/") || lower.starts_with(".missiond/evidence/") {
        classes.evidence.push(file.to_string());
    } else if is_code_file(&lower) {
        classes.code.push(file.to_string());
    } else if lower.ends_with(".md") || lower.ends_with(".txt") {
        classes.docs.push(file.to_string());
    } else {
        classes.other.push(file.to_string());
    }
}

fn is_code_file(lower: &str) -> bool {
    let ext_match = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".go", ".sql", ".toml",
        ".yaml", ".yml", ".json",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext));
    ext_match
        && (lower.starts_with("crates/")
            || lower.starts_with("packages/")
            || lower.starts_with("scripts/")
            || lower.starts_with("src/")
            || lower.starts_with("app/"))
}

async fn process_commit_event(
    state: &AppState,
    event: &SystemEvent,
) -> Result<CommitConvergenceResult> {
    let SystemEvent::ContextualCommitDetected {
        commit_hash,
        summary,
        conversation_id,
        message_id,
        session_id,
        slot_id,
        ..
    } = event
    else {
        return Err(anyhow!("not a ContextualCommitDetected event"));
    };

    let resolution =
        match resolve_project_for_commit(state, slot_id.as_deref(), conversation_id, commit_hash)
            .await
        {
            Some(resolution) => resolution,
            None => {
                let root = missiond_fallback_root(state).await;
                let report_path = write_report(
                    &root,
                    &CommitConvergenceReport {
                        project_id: "unknown".to_string(),
                        commit_hash: commit_hash.clone(),
                        status: CommitCoverageStatus::ExternalOrUnavailableCommit,
                        files: Vec::new(),
                        classes: ChangedFileClasses::default(),
                        backfill_task_id: None,
                        dedupe_key: None,
                        summary: summary.clone(),
                        conversation_id: conversation_id.clone(),
                        message_id: *message_id,
                        session_id: session_id.clone(),
                        slot_id: slot_id.clone(),
                    },
                )
                .await?;
                return Ok(CommitConvergenceResult {
                    project_id: "unknown".to_string(),
                    status: CommitCoverageStatus::ExternalOrUnavailableCommit,
                    backfill_task_id: None,
                    created_task: false,
                    dedupe_hit: false,
                    report_path,
                });
            }
        };

    let files = git_diff_tree_changed_files(&resolution.root, commit_hash).await?;
    let (status, classes) = classify_changed_files(&files);
    let dedupe_key = (status == CommitCoverageStatus::NeedsBackfill)
        .then(|| dedupe_key_for_commit(&resolution.project_id, commit_hash));
    let mut backfill_task_id = None;
    let mut created_task = false;
    let mut dedupe_hit = false;

    if let Some(dedupe_key) = dedupe_key.as_deref() {
        if let Some(existing) = state.store.find_open_task_by_dedupe_key(dedupe_key).await? {
            backfill_task_id = Some(existing.id.to_string());
            dedupe_hit = true;
        } else {
            let task = create_backfill_task(
                state,
                &resolution.project_id,
                commit_hash,
                summary,
                conversation_id,
                message_id,
                &classes,
                dedupe_key,
            )
            .await?;
            backfill_task_id = Some(task);
            created_task = true;
        }
    }

    let report = CommitConvergenceReport {
        project_id: resolution.project_id.clone(),
        commit_hash: commit_hash.clone(),
        status,
        files: files.clone(),
        classes: classes.clone(),
        backfill_task_id: backfill_task_id.clone(),
        dedupe_key,
        summary: summary.clone(),
        conversation_id: conversation_id.clone(),
        message_id: *message_id,
        session_id: session_id.clone(),
        slot_id: slot_id.clone(),
    };
    let report_path = write_report(&resolution.root, &report).await?;

    Ok(CommitConvergenceResult {
        project_id: resolution.project_id,
        status,
        backfill_task_id,
        created_task,
        dedupe_hit,
        report_path,
    })
}

async fn resolve_project_for_commit(
    state: &AppState,
    slot_id: Option<&str>,
    conversation_id: &str,
    commit_hash: &str,
) -> Option<ProjectResolution> {
    if let Some(slot_id) = slot_id {
        if let Some(slot) = state.mission.get_slot(slot_id) {
            let root = slot
                .config
                .project_root
                .or(slot.config.cwd)
                .map(PathBuf::from)?;
            if git_commit_exists(&root, commit_hash).await {
                let registry = state.project_registry.read().await;
                let project_id = registry
                    .resolve(&root.to_string_lossy())
                    .map(ToOwned::to_owned)
                    .or_else(|| infer_project_id_from_root(&root));
                if let Some(project_id) = project_id {
                    return Some(ProjectResolution { project_id, root });
                }
            }
        }
    }

    if let Some(resolution) =
        resolve_project_from_conversation(state, conversation_id, commit_hash).await
    {
        return Some(resolution);
    }

    let registry = state.project_registry.read().await;
    for project in registry.all_projects() {
        let root = PathBuf::from(&project.path);
        if git_commit_exists(&root, commit_hash).await {
            return Some(ProjectResolution {
                project_id: project.id.clone(),
                root,
            });
        }
    }
    None
}

async fn resolve_project_from_conversation(
    state: &AppState,
    conversation_id: &str,
    commit_hash: &str,
) -> Option<ProjectResolution> {
    let conversation = state.store.get_conversation(conversation_id).await.ok()??;
    let registry = state.project_registry.read().await;
    let mut candidates: Vec<(Option<String>, PathBuf)> = Vec::new();

    if let Some(project_id) = conversation.project_id.as_deref() {
        if let Some(project) = registry.get(project_id) {
            candidates.push((Some(project.id.clone()), PathBuf::from(&project.path)));
        }
    }

    if let Some(project_path) = conversation.project.as_deref() {
        candidates.push((
            registry.resolve(project_path).map(ToOwned::to_owned),
            PathBuf::from(project_path),
        ));
    }

    for (project_id, root) in candidates {
        if git_commit_exists(&root, commit_hash).await {
            let project_id = project_id
                .or_else(|| {
                    registry
                        .resolve(&root.to_string_lossy())
                        .map(ToOwned::to_owned)
                })
                .or_else(|| infer_project_id_from_root(&root))?;
            return Some(ProjectResolution { project_id, root });
        }
    }

    None
}

fn infer_project_id_from_root(root: &Path) -> Option<String> {
    root.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
}

async fn missiond_fallback_root(state: &AppState) -> PathBuf {
    let registry = state.project_registry.read().await;
    registry
        .get("missiond")
        .map(|project| PathBuf::from(&project.path))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

async fn git_commit_exists(root: &Path, commit_hash: &str) -> bool {
    let rev = format!("{commit_hash}^{{commit}}");
    command_status(root, &["cat-file", "-e", &rev]).await
}

async fn command_status(root: &Path, args: &[&str]) -> bool {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new("git").args(args).current_dir(root).output(),
    )
    .await;
    matches!(output, Ok(Ok(output)) if output.status.success())
}

async fn git_diff_tree_changed_files(root: &Path, commit_hash: &str) -> Result<Vec<String>> {
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        Command::new("git")
            .args([
                "diff-tree",
                "--root",
                "--no-commit-id",
                "-r",
                "--name-only",
                commit_hash,
            ])
            .current_dir(root)
            .output(),
    )
    .await
    .map_err(|_| anyhow!("git diff-tree timed out for {commit_hash}"))?
    .map_err(|err| anyhow!("git diff-tree failed to start: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git diff-tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

async fn create_backfill_task(
    state: &AppState,
    project_id: &str,
    commit_hash: &str,
    summary: &str,
    conversation_id: &str,
    message_id: &i64,
    classes: &ChangedFileClasses,
    dedupe_key: &str,
) -> Result<String> {
    let code_files = classes.code.join("\n");
    let description = format!(
        "Commit {commit_hash} changed code without same-commit Lisp/checker/evidence coverage.\n\nSummary: {summary}\nConversation: {conversation_id} message_id={message_id}\n\nChanged code files:\n{code_files}\n\nRequired backfill:\n1. Map the behavior to the relevant V3/project Lisp surface.\n2. Add or update checker coverage.\n3. Add concise evidence explaining why the code landed before Lisp if applicable.\n4. Re-run the project convergence checker."
    );
    let input = CreateBoardTaskInput {
        title: format!(
            "Backfill Lisp/checker for commit {}",
            short_hash(commit_hash)
        ),
        description: Some(description),
        priority: Some("medium".to_string()),
        category: Some("dev".to_string()),
        project: Some(project_id.to_string()),
        auto_execute: Some(false),
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

fn dedupe_key_for_commit(project_id: &str, commit_hash: &str) -> String {
    format!("commit-lisp-backfill:{project_id}:{commit_hash}")
}

#[derive(Debug)]
struct CommitConvergenceReport {
    project_id: String,
    commit_hash: String,
    status: CommitCoverageStatus,
    files: Vec<String>,
    classes: ChangedFileClasses,
    backfill_task_id: Option<String>,
    dedupe_key: Option<String>,
    summary: String,
    conversation_id: String,
    message_id: i64,
    session_id: String,
    slot_id: Option<String>,
}

async fn write_report(root: &Path, report: &CommitConvergenceReport) -> std::io::Result<PathBuf> {
    let dir = root.join(COMMIT_CONVERGENCE_REPORT_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{}.report.lisp", short_hash(&report.commit_hash)));
    tokio::fs::write(&path, render_report(report)).await?;
    Ok(path)
}

fn render_report(report: &CommitConvergenceReport) -> String {
    format!(
        "(commit-lisp-convergence-report\n  :schema \"missiond.commit-lisp-convergence-report.v1\"\n  :updated-at {}\n  :project {}\n  :commit {}\n  :status {}\n  :dedupe-key {}\n  :backfill-task-id {}\n  :conversation-id {}\n  :message-id {}\n  :session-id {}\n  :slot-id {}\n  :summary {}\n  :changed-files {}\n  :classes {}\n)\n",
        lisp_string(&chrono::Utc::now().to_rfc3339()),
        lisp_string(&report.project_id),
        lisp_string(&report.commit_hash),
        report.status.as_str(),
        lisp_option_string(report.dedupe_key.as_deref()),
        lisp_option_string(report.backfill_task_id.as_deref()),
        lisp_string(&report.conversation_id),
        report.message_id,
        lisp_string(&report.session_id),
        lisp_option_string(report.slot_id.as_deref()),
        lisp_string(&report.summary),
        lisp_string_list(&report.files),
        render_classes(&report.classes),
    )
}

fn render_classes(classes: &ChangedFileClasses) -> String {
    format!(
        "(:code {} :lisp {} :checker {} :evidence {} :docs {} :other {})",
        lisp_string_list(&classes.code),
        lisp_string_list(&classes.lisp),
        lisp_string_list(&classes.checker),
        lisp_string_list(&classes.evidence),
        lisp_string_list(&classes.docs),
        lisp_string_list(&classes.other),
    )
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

fn lisp_option_string(value: Option<&str>) -> String {
    value.map(lisp_string).unwrap_or_else(|| "nil".to_string())
}

fn lisp_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn lisp_string_list(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            values
                .iter()
                .map(|value| lisp_string(value))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_code_only_commit_as_needs_backfill() {
        let files = vec!["crates/missiond-daemon/src/engine/foo.rs".to_string()];
        let (status, classes) = classify_changed_files(&files);
        assert_eq!(status, CommitCoverageStatus::NeedsBackfill);
        assert_eq!(classes.code, files);
    }

    #[test]
    fn classifies_code_with_lisp_or_checker_as_covered() {
        let files = vec![
            "crates/missiond-daemon/src/engine/foo.rs".to_string(),
            ".missiond/v3/missiond-blueprint.lisp".to_string(),
            "scripts/check-v3-foo.mjs".to_string(),
        ];
        let (status, classes) = classify_changed_files(&files);
        assert_eq!(status, CommitCoverageStatus::Covered);
        assert_eq!(classes.lisp.len(), 1);
        assert_eq!(classes.checker.len(), 1);
    }

    #[test]
    fn classifies_lisp_only_commit_without_recursing() {
        let files = vec![".missiond/workflows/commit-lisp-convergence.lisp".to_string()];
        let (status, classes) = classify_changed_files(&files);
        assert_eq!(status, CommitCoverageStatus::LispOnly);
        assert_eq!(classes.lisp, files);
    }

    #[test]
    fn dedupe_key_is_project_and_commit_scoped() {
        assert_eq!(
            dedupe_key_for_commit("missiond", "abcdef"),
            "commit-lisp-backfill:missiond:abcdef"
        );
    }

    #[test]
    fn unavailable_commit_has_precise_status_label() {
        assert_eq!(
            CommitCoverageStatus::ExternalOrUnavailableCommit.as_str(),
            "external-or-unavailable-commit"
        );
    }

    #[test]
    fn report_contains_snapshot_truth_and_status() {
        let report = CommitConvergenceReport {
            project_id: "missiond".to_string(),
            commit_hash: "abcdef123456".to_string(),
            status: CommitCoverageStatus::NeedsBackfill,
            files: vec!["crates/a.rs".to_string()],
            classes: ChangedFileClasses {
                code: vec!["crates/a.rs".to_string()],
                ..Default::default()
            },
            backfill_task_id: Some("task-1".to_string()),
            dedupe_key: Some("commit-lisp-backfill:missiond:abcdef123456".to_string()),
            summary: "fix".to_string(),
            conversation_id: "conv".to_string(),
            message_id: 7,
            session_id: "sess".to_string(),
            slot_id: Some("slot".to_string()),
        };
        let rendered = render_report(&report);
        assert!(rendered.contains("commit-lisp-convergence-report"));
        assert!(rendered.contains(":status needs-backfill"));
        assert!(rendered.contains("commit-lisp-backfill:missiond:abcdef123456"));
    }
}
