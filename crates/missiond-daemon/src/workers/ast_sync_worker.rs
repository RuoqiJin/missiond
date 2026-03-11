//! AST Sync Worker — incremental code indexing pipeline.
//!
//! Receives sync tasks via MPSC channel:
//! - CommitSync: git diff-tree between commits → parse changed files
//! - FullSync: scan entire repo for stale/missing files
//! - FileSync: re-parse a single file (for uncommitted changes)
//!
//! Part of P2: Holographic Context Engine.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use missiond_core::MissionControl;
use missiond_core::db::MissionDB;

/// Source code file extensions we index.
const CODE_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "py"];

/// Max files per batch during full sync (rate limiting).
const FULL_SYNC_BATCH_SIZE: usize = 20;

/// Coalescing window: wait this long for more commits before processing.
const COALESCE_WINDOW: Duration = Duration::from_secs(2);

/// AST sync tasks dispatched to the worker.
#[derive(Debug)]
pub(crate) enum AstSyncTask {
    /// Incremental: process file changes between two commits.
    CommitSync {
        repo_path: PathBuf,
        repo_name: String,
        old_hash: String,
        new_hash: String,
    },
    /// Full scan: daemon startup backfill.
    FullSync {
        repo_path: PathBuf,
        repo_name: String,
    },
    /// Single file: uncommitted change or manual trigger.
    FileSync {
        repo_path: PathBuf,
        repo_name: String,
        file_path: String,
    },
}

// @beacon: holographic
/// Run the AST sync worker loop. Receives tasks from channel, coalesces commits.
pub(crate) async fn run_ast_sync_worker(
    mut rx: mpsc::Receiver<AstSyncTask>,
    mission: std::sync::Arc<MissionControl>,
    embedding_tx: mpsc::Sender<crate::state::EmbeddingTask>,
) {
    info!("AST sync worker started");

    while let Some(task) = rx.recv().await {
        match task {
            AstSyncTask::CommitSync { repo_path, repo_name, old_hash, new_hash } => {
                // Coalesce: collect more CommitSync for same repo within window
                let (final_old, final_new) = coalesce_commits(
                    &mut rx, &repo_path, &repo_name, old_hash, new_hash,
                ).await;

                let mc = mission.clone();
                let etx = embedding_tx.clone();
                let rp = repo_path.clone();
                let rn = repo_name.clone();
                tokio::task::spawn_blocking(move || {
                    process_commit_sync(mc.db(), &rp, &rn, &final_old, &final_new, &etx);
                }).await.ok();
            }
            AstSyncTask::FullSync { repo_path, repo_name } => {
                let mc = mission.clone();
                let etx = embedding_tx.clone();
                let rp = repo_path.clone();
                let rn = repo_name.clone();
                tokio::task::spawn_blocking(move || {
                    process_full_sync(mc.db(), &rp, &rn, &etx);
                }).await.ok();
            }
            AstSyncTask::FileSync { repo_path, repo_name, file_path } => {
                let mc = mission.clone();
                let etx = embedding_tx.clone();
                let rp = repo_path.clone();
                let rn = repo_name.clone();
                let fp = file_path.clone();
                tokio::task::spawn_blocking(move || {
                    process_file_sync(mc.db(), &rp, &rn, &fp, &etx);
                }).await.ok();
            }
        }
    }

    info!("AST sync worker stopped (channel closed)");
}

/// Coalesce multiple CommitSync tasks for the same repo within a time window.
/// Returns (oldest_old_hash, newest_new_hash).
async fn coalesce_commits(
    rx: &mut mpsc::Receiver<AstSyncTask>,
    repo_path: &Path,
    _repo_name: &str,
    mut oldest_old: String,
    mut newest_new: String,
) -> (String, String) {
    loop {
        match tokio::time::timeout(COALESCE_WINDOW, rx.recv()).await {
            Ok(Some(AstSyncTask::CommitSync { repo_path: rp, old_hash, new_hash, .. }))
                if rp == repo_path =>
            {
                // Same repo — extend the range
                // Keep oldest_old (first event), update newest_new (latest)
                newest_new = new_hash;
                // If old_hash is older, update (though normally first is oldest)
                if old_hash < oldest_old {
                    oldest_old = old_hash;
                }
                debug!(repo = %_repo_name, "Coalesced commit into window");
            }
            Ok(Some(other_task)) => {
                // Different task type or different repo — we can't coalesce, but we
                // shouldn't lose it. Unfortunately mpsc doesn't support push-back.
                // For now, log and drop. FullSync will catch up.
                warn!("AST worker: dropped non-coalesceable task during coalesce window: {:?}",
                    std::mem::discriminant(&other_task));
                break;
            }
            Ok(None) => break, // Channel closed
            Err(_) => break,   // Timeout — coalesce window expired
        }
    }
    (oldest_old, newest_new)
}

/// Process incremental commit sync: git diff-tree → parse changed files.
fn process_commit_sync(
    db: &MissionDB,
    repo_path: &Path,
    repo_name: &str,
    old_hash: &str,
    new_hash: &str,
    embedding_tx: &mpsc::Sender<crate::state::EmbeddingTask>,
) {
    let changed = git_diff_files(repo_path, old_hash, new_hash);
    if changed.is_empty() {
        debug!(repo = %repo_name, "No code files changed between {}..{}", &old_hash[..8.min(old_hash.len())], &new_hash[..8.min(new_hash.len())]);
        return;
    }

    info!(
        repo = %repo_name,
        files = changed.len(),
        range = %format!("{}..{}", &old_hash[..8.min(old_hash.len())], &new_hash[..8.min(new_hash.len())]),
        "AST commit sync"
    );

    let mut node_ids = Vec::new();

    for (status, file_path) in &changed {
        match status.as_str() {
            "D" => {
                // File deleted — remove nodes
                match db.ast_delete_file(repo_name, file_path) {
                    Ok(n) => if n > 0 { debug!(file = %file_path, deleted = n, "AST: deleted nodes"); },
                    Err(e) => warn!(file = %file_path, err = %e, "AST: delete failed"),
                }
            }
            "A" | "M" | "R" | "C" | "T" => {
                // Added, Modified, Renamed, Copied, Type-changed — re-parse
                let ids = sync_single_file(db, repo_path, repo_name, file_path, new_hash);
                node_ids.extend(ids);
            }
            _ => {
                debug!(status = %status, file = %file_path, "AST: unknown git status, treating as modified");
                let ids = sync_single_file(db, repo_path, repo_name, file_path, new_hash);
                node_ids.extend(ids);
            }
        }
    }

    // Trigger embedding for new/changed nodes
    if !node_ids.is_empty() {
        trigger_embedding(embedding_tx, node_ids);
    }

    // Update topology map (module-level summaries) after commit changes
    crate::topology_map::update_module_summaries(db, repo_name);
}

/// Process full sync: scan repo for all code files, sync stale ones.
fn process_full_sync(
    db: &MissionDB,
    repo_path: &Path,
    repo_name: &str,
    embedding_tx: &mpsc::Sender<crate::state::EmbeddingTask>,
) {
    let head_hash = git_head_hash(repo_path).unwrap_or_default();
    if head_hash.is_empty() {
        warn!(repo = %repo_name, "AST full sync: cannot get HEAD hash");
        return;
    }

    let files = git_ls_code_files(repo_path);
    if files.is_empty() {
        debug!(repo = %repo_name, "AST full sync: no code files found");
        return;
    }

    info!(repo = %repo_name, total_files = files.len(), "AST full sync started");

    let mut synced = 0usize;
    let mut skipped = 0usize;
    let mut node_ids = Vec::new();

    for (batch_idx, chunk) in files.chunks(FULL_SYNC_BATCH_SIZE).enumerate() {
        for file_path in chunk {
            // Check if already up-to-date
            match db.ast_get_file_commit(repo_name, file_path) {
                Ok(Some(ref cached_hash)) if cached_hash == &head_hash => {
                    skipped += 1;
                    continue;
                }
                _ => {}
            }

            let ids = sync_single_file(db, repo_path, repo_name, file_path, &head_hash);
            node_ids.extend(ids);
            synced += 1;
        }

        // Rate limit between batches
        if batch_idx > 0 && synced > 0 {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    // Trigger embedding
    if !node_ids.is_empty() {
        trigger_embedding(embedding_tx, node_ids);
    }

    // Update topology map after full sync
    crate::topology_map::update_module_summaries(db, repo_name);

    info!(
        repo = %repo_name,
        synced, skipped,
        "AST full sync completed"
    );
}

/// Process single file sync (for uncommitted changes).
fn process_file_sync(
    db: &MissionDB,
    repo_path: &Path,
    repo_name: &str,
    file_path: &str,
    embedding_tx: &mpsc::Sender<crate::state::EmbeddingTask>,
) {
    let hash = git_head_hash(repo_path).unwrap_or_else(|| "worktree".to_string());
    let ids = sync_single_file(db, repo_path, repo_name, file_path, &hash);
    if !ids.is_empty() {
        trigger_embedding(embedding_tx, ids);
    }
}

/// Parse a single file and sync to DB. Returns node IDs of inserted nodes.
fn sync_single_file(
    db: &MissionDB,
    repo_path: &Path,
    repo_name: &str,
    file_path: &str,
    commit_hash: &str,
) -> Vec<String> {
    let full_path = repo_path.join(file_path);
    let source = match std::fs::read_to_string(&full_path) {
        Ok(s) => s,
        Err(e) => {
            debug!(file = %file_path, err = %e, "AST: cannot read file, skipping");
            return Vec::new();
        }
    };

    #[cfg(feature = "code-context")]
    let nodes = missiond_core::ast::extract_from_file(file_path, &source);
    #[cfg(not(feature = "code-context"))]
    let nodes: Vec<missiond_core::ast::CodeNode> = Vec::new();

    if nodes.is_empty() {
        // Either unsupported extension or parse failure — still update meta
        // to avoid re-attempting on next sync
        let _ = db.ast_sync_file(repo_name, file_path, commit_hash, &[]);
        return Vec::new();
    }

    // Collect IDs before sync (sync consumes nothing, but we need them for embedding)
    let ids: Vec<String> = nodes.iter()
        .map(|n| n.build_id(repo_name, file_path))
        .collect();

    match db.ast_sync_file(repo_name, file_path, commit_hash, &nodes) {
        Ok(result) => {
            debug!(
                file = %file_path,
                inserted = result.inserted,
                deleted = result.deleted,
                "AST: synced"
            );
        }
        Err(e) => {
            warn!(file = %file_path, err = %e, "AST: sync failed");
            return Vec::new();
        }
    }

    // P4: Extract @beacon: tags and sync to beacon_nodes
    // First, clean old beacon_nodes for this file (re-extract from scratch)
    let _ = db.beacon_nodes_delete_file(repo_name, file_path);
    for node in &nodes {
        if !node.beacons.is_empty() {
            for beacon_name in &node.beacons {
                match db.beacon_ensure(beacon_name) {
                    Ok(beacon_id) => {
                        let _ = db.beacon_node_upsert(&beacon_id, repo_name, file_path, &node.name, None);
                    }
                    Err(e) => {
                        debug!(beacon = %beacon_name, err = %e, "Beacon: ensure failed");
                    }
                }
            }
            debug!(
                file = %file_path, symbol = %node.name,
                beacons = ?node.beacons,
                "Beacon: tags extracted"
            );
        }
    }

    ids
}

// ── Git helpers ──

/// Get files changed between two commits, filtered to code extensions.
fn git_diff_files(repo: &Path, old_hash: &str, new_hash: &str) -> Vec<(String, String)> {
    let ext_filter: Vec<String> = CODE_EXTENSIONS.iter()
        .map(|e| format!("*.{}", e))
        .collect();

    let mut args = vec![
        "diff-tree".to_string(),
        "-r".to_string(),
        "--name-status".to_string(),
        "--no-commit-id".to_string(),
        format!("{}..{}", old_hash, new_hash),
        "--".to_string(),
    ];
    args.extend(ext_filter);

    let output = match std::process::Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(err = %stderr, "git diff-tree failed");
            return vec![];
        }
        Err(e) => {
            warn!(err = %e, "git diff-tree command error");
            return vec![];
        }
    };

    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// List all tracked code files in the repo.
fn git_ls_code_files(repo: &Path) -> Vec<String> {
    let ext_filter: Vec<String> = CODE_EXTENSIONS.iter()
        .map(|e| format!("*.{}", e))
        .collect();

    let mut args = vec![
        "ls-files".to_string(),
        "--".to_string(),
    ];
    args.extend(ext_filter);

    let output = match std::process::Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

/// Get HEAD commit hash.
fn git_head_hash(repo: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.is_empty() { None } else { Some(hash) }
}

/// Get uncommitted changed files via `git status --porcelain`.
pub(crate) fn git_status_changed_files(repo: &Path) -> Vec<String> {
    let output = match std::process::Command::new("git")
        .args(["status", "--porcelain", "--"])
        .current_dir(repo)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            // Format: "XY filename" or "XY old -> new" for renames
            if line.len() < 4 { return None; }
            let file = line[3..].trim();
            // Handle renames: "R  old -> new"
            let file = file.rsplit(" -> ").next().unwrap_or(file);
            // Filter to code extensions
            let ext = file.rsplit('.').next()?;
            if CODE_EXTENSIONS.contains(&ext) {
                Some(file.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Trigger embedding for AST node IDs via the existing embedding channel.
/// Uses try_send to avoid blocking the sync worker.
fn trigger_embedding(
    embedding_tx: &mpsc::Sender<crate::state::EmbeddingTask>,
    node_ids: Vec<String>,
) {
    if node_ids.is_empty() {
        return;
    }
    debug!(count = node_ids.len(), "AST: triggering embedding batch");
    if embedding_tx.try_send(crate::state::EmbeddingTask::ProcessAstBatch(node_ids)).is_err() {
        debug!("AST: embedding channel full, backfill will catch up");
    }
}
