//! mission_global_instruction — read/edit/reload manager for the global
//! Claude Code instruction file at ~/.claude/CLAUDE.md.
//!
//! Lisp authority:
//!   - intent-intent-layer.lisp :: global-claudemd-manager (status: code-alignment pending)
//!   - intent-tools.lisp :: future-surface mission_global_instruction
//!   - intent-flow.lisp :: trivial-single-step read/edit/reload
//!
//! Status (this batch):
//!   - read: full
//!   - edit: full (atomic temp+rename, timestamped backup, dry_run, allow_empty)
//!   - reload: manual — daemon does not own this file; Claude Code reads it
//!     once per session at bootstrap. We never lie about reload here.
//!
//! Backup path convention:
//!   `<dir>/CLAUDE.md.bak.<UTC timestamp Y%m%dT%H%M%SZ>` next to the original.

use anyhow::{anyhow, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::state::AppState;

/// Resolve `~/.claude/CLAUDE.md`. The tool is hard-bound to this path and
/// rejects anything else.
fn global_claude_md_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("HOME directory unavailable"))?;
    Ok(home.join(".claude").join("CLAUDE.md"))
}

pub(crate) async fn handle(_state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    if action.is_empty() {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::MISSING_PARAM, "`action` is required")
                .with_suggestion("Use one of: read, edit, reload"),
        ));
    }

    let path = match global_claude_md_path() {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                format!("cannot resolve target path: {}", e),
            )));
        }
    };

    match action {
        "read" => read_action(&path).await,
        "edit" => edit_action(&path, args).await,
        "reload" => reload_action(&path).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown action `{}`", other),
            )
            .with_suggestion("Use one of: read, edit, reload"),
        )),
    }
}

async fn read_action(path: &Path) -> Result<ToolResult> {
    Ok(ToolResult::json_pretty(&read_at(path).await))
}

async fn read_at(path: &Path) -> Value {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let exists = true;
            let size = bytes.len() as u64;
            let sha256 = sha256_hex(&bytes);
            let mtime = file_mtime_iso(path).await;
            // Best-effort UTF-8 decode; if it fails, expose lossy text + flag.
            let (content, utf8_lossy) = match std::str::from_utf8(&bytes) {
                Ok(s) => (s.to_string(), false),
                Err(_) => (String::from_utf8_lossy(&bytes).to_string(), true),
            };
            json!({
                "status": "ok",
                "exists": exists,
                "path": path.display().to_string(),
                "size": size,
                "sha256": sha256,
                "mtime": mtime,
                "utf8_lossy": utf8_lossy,
                "content": content,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({
            "status": "not_found",
            "exists": false,
            "path": path.display().to_string(),
            "size": 0,
            "sha256": null,
            "mtime": null,
            "content": "",
        }),
        Err(e) => json!({
            "status": "error",
            "exists": false,
            "path": path.display().to_string(),
            "error": e.to_string(),
        }),
    }
}

async fn edit_action(path: &Path, args: Value) -> Result<ToolResult> {
    let new_content = match args.get("new_content").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "`new_content` is required for edit",
                )
                .with_suggestion("Provide the full UTF-8 file body in `new_content`"),
            ));
        }
    };
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let allow_empty = args
        .get("allow_empty")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if new_content.is_empty() && !allow_empty {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                "refusing to write empty content without allow_empty=true",
            )
            .with_suggestion("Set allow_empty=true to confirm intentional truncation"),
        ));
    }

    let prior = tokio::fs::read(path).await.ok();
    let prior_size = prior.as_ref().map(|b| b.len() as u64).unwrap_or(0);
    let prior_sha = prior.as_ref().map(|b| sha256_hex(b));
    let new_sha = sha256_hex(new_content.as_bytes());
    let new_size = new_content.len() as u64;
    let unchanged = prior_sha.as_deref() == Some(new_sha.as_str());

    if dry_run {
        return Ok(ToolResult::json_pretty(&json!({
            "status": "dry_run",
            "would_write": !unchanged,
            "path": path.display().to_string(),
            "prior_exists": prior.is_some(),
            "prior_size": prior_size,
            "prior_sha256": prior_sha,
            "new_size": new_size,
            "new_sha256": new_sha,
            "diff_preview": diff_preview(prior.as_deref(), new_content.as_bytes()),
            "note": "dry_run=true; no backup written, no file replaced",
        })));
    }

    if unchanged {
        return Ok(ToolResult::json_pretty(&json!({
            "status": "noop",
            "path": path.display().to_string(),
            "new_sha256": new_sha,
            "note": "content identical to current file; no backup, no write",
        })));
    }

    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("target path has no parent directory: {}", path.display()))?;
    if !dir.exists() {
        tokio::fs::create_dir_all(dir).await?;
    }

    // Backup only when an existing file is being overwritten.
    let backup_path = if prior.is_some() {
        let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let bak_name = format!(
            "{}.bak.{}",
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "CLAUDE.md".to_string()),
            stamp
        );
        let bak = dir.join(bak_name);
        // Use copy (not rename) so the live inode is preserved up to the moment
        // we atomically swap it.
        tokio::fs::copy(path, &bak).await?;
        Some(bak)
    } else {
        None
    };

    // Atomic write: temp file in same dir → rename. Same-dir is required for
    // POSIX rename atomicity on the target filesystem.
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%fZ").to_string();
    let tmp = dir.join(format!(".CLAUDE.md.tmp.{}", stamp));
    if let Err(e) = tokio::fs::write(&tmp, new_content.as_bytes()).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(anyhow!(
            "failed to write temp file {}: {}",
            tmp.display(),
            e
        ));
    }
    if let Err(e) = tokio::fs::rename(&tmp, path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(anyhow!(
            "failed to rename temp into place {} → {}: {}",
            tmp.display(),
            path.display(),
            e
        ));
    }

    Ok(ToolResult::json_pretty(&json!({
        "status": "written",
        "path": path.display().to_string(),
        "prior_exists": prior.is_some(),
        "prior_size": prior_size,
        "prior_sha256": prior_sha,
        "new_size": new_size,
        "new_sha256": new_sha,
        "backup_path": backup_path.map(|p| p.display().to_string()),
        "atomic": "temp+rename",
        "reload_hint": "Claude Code reads ~/.claude/CLAUDE.md once per session bootstrap. Restart the session or run mission_global_instruction action=reload to inspect daemon-side reload status (currently manual-reload-required).",
    })))
}

async fn reload_action(path: &Path) -> Result<ToolResult> {
    // The daemon does not own the global CLAUDE.md. Claude Code reads it on
    // session startup; there is no in-memory cache here to invalidate. We
    // honestly report manual-reload-required rather than fake success.
    let exists = tokio::fs::metadata(path).await.is_ok();
    Ok(ToolResult::json_pretty(&json!({
        "status": "manual-reload-required",
        "path": path.display().to_string(),
        "exists": exists,
        "daemon_reload_supported": false,
        "reason": "Global ~/.claude/CLAUDE.md is consumed by Claude Code at session bootstrap; the missiond daemon does not cache it. Restart the Claude Code session to pick up edits.",
    })))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

async fn file_mtime_iso(path: &Path) -> Option<String> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    let mtime = meta.modified().ok()?;
    let dt: DateTime<Utc> = mtime.into();
    Some(dt.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// Tiny human-readable preview for dry_run output. Not a real unified diff —
/// just enough to let the caller eyeball the change.
fn diff_preview(prior: Option<&[u8]>, next: &[u8]) -> Value {
    let prior_text = prior
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();
    let next_text = String::from_utf8_lossy(next).to_string();

    let prior_lines: Vec<&str> = prior_text.lines().collect();
    let next_lines: Vec<&str> = next_text.lines().collect();

    json!({
        "prior_line_count": prior_lines.len(),
        "next_line_count": next_lines.len(),
        "prior_head": prior_lines.iter().take(3).copied().collect::<Vec<_>>(),
        "next_head": next_lines.iter().take(3).copied().collect::<Vec<_>>(),
        "prior_tail": prior_lines.iter().rev().take(3).rev().copied().collect::<Vec<_>>(),
        "next_tail": next_lines.iter().rev().take(3).rev().copied().collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Drive read/edit/reload against an arbitrary path so tests don't touch
    /// the real ~/.claude/CLAUDE.md. The production handler always resolves
    /// to the global path; these helpers mirror its core logic exactly.
    async fn read_at_for_test(path: &Path) -> Value {
        read_at(path).await
    }

    async fn edit_at_for_test(
        path: &Path,
        new_content: &str,
        dry_run: bool,
        allow_empty: bool,
    ) -> Result<ToolResult> {
        let args = json!({
            "new_content": new_content,
            "dry_run": dry_run,
            "allow_empty": allow_empty,
        });
        edit_action(path, args).await
    }

    fn extract_json(result: &ToolResult) -> Value {
        let text = match &result.content[0] {
            missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
        };
        serde_json::from_str(&text).expect("tool result body is JSON")
    }

    #[tokio::test]
    async fn read_missing_returns_not_found() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let v = read_at_for_test(&path).await;
        assert_eq!(v["status"], "not_found");
        assert_eq!(v["exists"], false);
        assert_eq!(v["content"], "");
    }

    #[tokio::test]
    async fn read_existing_returns_metadata() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        tokio::fs::write(&path, b"# hello\nworld\n").await.unwrap();
        let v = read_at_for_test(&path).await;
        assert_eq!(v["status"], "ok");
        assert_eq!(v["exists"], true);
        assert_eq!(v["size"], 14);
        assert!(v["sha256"].as_str().unwrap().len() == 64);
        assert!(v["content"].as_str().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn edit_dry_run_does_not_write() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        tokio::fs::write(&path, b"old").await.unwrap();
        let res = edit_at_for_test(&path, "new content", true, false)
            .await
            .unwrap();
        let v = extract_json(&res);
        assert_eq!(v["status"], "dry_run");
        assert_eq!(v["would_write"], true);
        // file unchanged
        let on_disk = tokio::fs::read(&path).await.unwrap();
        assert_eq!(on_disk, b"old");
        // no backup created
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            !entries.iter().any(|n| n.contains(".bak.")),
            "dry_run must not produce a backup, found: {:?}",
            entries
        );
    }

    #[tokio::test]
    async fn edit_writes_backup_and_replaces_atomically() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        tokio::fs::write(&path, b"old").await.unwrap();
        let res = edit_at_for_test(&path, "new content", false, false)
            .await
            .unwrap();
        let v = extract_json(&res);
        assert_eq!(v["status"], "written");
        let backup = v["backup_path"].as_str().expect("backup_path present");
        assert!(
            tokio::fs::read(backup).await.unwrap() == b"old",
            "backup should hold the prior bytes"
        );
        let on_disk = tokio::fs::read(&path).await.unwrap();
        assert_eq!(on_disk, b"new content");
    }

    #[tokio::test]
    async fn edit_rejects_empty_without_allow_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        tokio::fs::write(&path, b"old").await.unwrap();
        let res = edit_at_for_test(&path, "", false, false).await.unwrap();
        assert_eq!(res.is_error, Some(true));
        let on_disk = tokio::fs::read(&path).await.unwrap();
        assert_eq!(on_disk, b"old");
    }

    #[tokio::test]
    async fn edit_allows_empty_with_flag() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        tokio::fs::write(&path, b"old").await.unwrap();
        let res = edit_at_for_test(&path, "", false, true).await.unwrap();
        let v = extract_json(&res);
        assert_eq!(v["status"], "written");
        let on_disk = tokio::fs::read(&path).await.unwrap();
        assert!(on_disk.is_empty());
    }

    #[tokio::test]
    async fn edit_noop_when_unchanged() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        tokio::fs::write(&path, b"same").await.unwrap();
        let res = edit_at_for_test(&path, "same", false, false).await.unwrap();
        let v = extract_json(&res);
        assert_eq!(v["status"], "noop");
        // no backup created on noop
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(!entries.iter().any(|n| n.contains(".bak.")));
    }

    #[tokio::test]
    async fn edit_creates_file_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let res = edit_at_for_test(&path, "fresh", false, false)
            .await
            .unwrap();
        let v = extract_json(&res);
        assert_eq!(v["status"], "written");
        assert_eq!(v["prior_exists"], false);
        assert!(v["backup_path"].is_null());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"fresh");
    }

    #[tokio::test]
    async fn reload_returns_manual_required() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let res = reload_action(&path).await.unwrap();
        let v = extract_json(&res);
        assert_eq!(v["status"], "manual-reload-required");
        assert_eq!(v["daemon_reload_supported"], false);
        assert_eq!(v["exists"], false);
    }
}
