//! Git Commit Watcher — polls monitored repos for new commits and emits timeline events.
//!
//! Collects unique `cwd` paths from slot configs, deduplicates to git repo roots,
//! and polls `git log` every 30s. New commits emit `DaemonEvent::GitCommitDetected`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::event_bus::{DaemonEvent, TimelineEntry};

/// A single parsed git commit.
#[derive(Debug)]
struct GitCommit {
    hash: String,
    short_hash: String,
    author: String,
    message: String,
    /// ISO-8601 commit timestamp.
    committed_at: String,
}

/// Resolve the git repo root for a given path. Returns None if not a git repo.
fn git_repo_root(path: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() { None } else { Some(PathBuf::from(root)) }
}

/// Get the repo short name from its path (last component).
fn repo_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Fetch recent commits after `since_hash` (exclusive). Returns newest-first.
/// If `since_hash` is None, returns only the latest commit (bootstrap).
fn fetch_new_commits(repo: &Path, since_hash: Option<&str>) -> Vec<GitCommit> {
    let format = "%H%n%h%n%an%n%s%n%aI";
    let args: Vec<String> = if let Some(since) = since_hash {
        vec![
            "log".into(), format!("--format={}", format),
            format!("{}..HEAD", since), "--".into(),
        ]
    } else {
        // Bootstrap: just get the latest commit
        vec![
            "log".into(), format!("--format={}", format),
            "-1".into(), "--".into(),
        ]
    };

    let output = match std::process::Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().collect();

    // Each commit is 5 lines: hash, short_hash, author, message, committed_at
    lines.chunks(5)
        .filter(|chunk| chunk.len() == 5)
        .map(|chunk| GitCommit {
            hash: chunk[0].to_string(),
            short_hash: chunk[1].to_string(),
            author: chunk[2].to_string(),
            message: chunk[3].to_string(),
            committed_at: chunk[4].to_string(),
        })
        .collect()
}

/// Collect unique git repo roots from slot cwds.
pub(crate) fn collect_repo_roots(slot_cwds: &[String]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for cwd in slot_cwds {
        let path = Path::new(cwd);
        if !path.exists() { continue; }
        if let Some(root) = git_repo_root(path) {
            if seen.insert(root.clone()) {
                roots.push(root);
            }
        }
    }
    roots
}

/// Run the git commit poller loop. Call from a spawned task.
pub(crate) async fn run_git_watcher(
    repos: Vec<PathBuf>,
    event_tx: mpsc::UnboundedSender<TimelineEntry>,
) {
    if repos.is_empty() {
        info!("Git watcher: no repos to monitor");
        return;
    }

    info!(repos = ?repos.iter().map(|r| repo_name(r)).collect::<Vec<_>>(), "Git watcher started");

    // Bootstrap: record current HEAD for each repo (don't emit events for old commits)
    let mut last_hash: HashMap<PathBuf, String> = HashMap::new();
    for repo in &repos {
        let commits = fetch_new_commits(repo, None);
        if let Some(c) = commits.first() {
            debug!(repo = %repo_name(repo), hash = %c.short_hash, "Git watcher: bootstrap HEAD");
            last_hash.insert(repo.clone(), c.hash.clone());
        }
    }

    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        for repo in &repos {
            let since = last_hash.get(repo).map(|s| s.as_str());
            let commits = tokio::task::spawn_blocking({
                let repo = repo.clone();
                let since = since.map(|s| s.to_string());
                move || fetch_new_commits(&repo, since.as_deref())
            })
            .await
            .unwrap_or_default();

            if commits.is_empty() {
                continue;
            }

            // Update last known hash to the newest commit
            if let Some(newest) = commits.first() {
                last_hash.insert(repo.clone(), newest.hash.clone());
            }

            let name = repo_name(repo);

            // Emit events oldest-first (commits are newest-first from git log)
            for commit in commits.into_iter().rev() {
                info!(
                    repo = %name, hash = %commit.short_hash, msg = %commit.message,
                    "Git watcher: new commit detected"
                );
                let event = DaemonEvent::GitCommitDetected {
                    repo: name.clone(),
                    hash: commit.hash,
                    short_hash: commit.short_hash,
                    author: commit.author,
                    message: commit.message,
                    committed_at: commit.committed_at,
                };
                let entry = TimelineEntry {
                    event,
                    trace_id: Some(format!("git-{}", name)),
                    span_id: uuid::Uuid::new_v4().to_string(),
                    parent_span_id: None,
                    summary: None,
                };
                if event_tx.send(entry).is_err() {
                    warn!("Git watcher: event channel closed");
                    return;
                }
            }
        }
    }
}
