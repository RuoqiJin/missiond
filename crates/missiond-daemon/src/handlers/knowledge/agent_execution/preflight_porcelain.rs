use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use std::path::Path;

/// Single-file entry from `git status --porcelain=v1`. The first byte is
/// the index (staged) status, the second is the worktree status; we
/// surface both so the caller can tell "staged but reverted in worktree"
/// from "edited but not staged".
///
/// We deliberately keep the struct minimal and plain — no path
/// canonicalization here, since rename pairs / quoted paths would require
/// shelling out to `git diff` per entry. The audit needs file paths
/// relative to the project root, which porcelain v1 already provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PorcelainEntry {
    /// Index/staged status byte (`'M'`, `'A'`, `'D'`, `'R'`, `'?'`, ` `, …).
    pub(super) index_status: char,
    /// Worktree status byte (same alphabet as `index_status`).
    pub(super) worktree_status: char,
    /// Path as reported by porcelain (rename right-hand side when applicable).
    pub(super) path: String,
}

impl PorcelainEntry {
    /// True when the index slot reflects a tracked staged change
    /// (anything but ` ` / `?` / `!`). Untracked / ignored files never
    /// count as staged because porcelain marks them with `?` / `!`.
    pub(super) fn is_staged(&self) -> bool {
        !matches!(self.index_status, ' ' | '?' | '!')
    }

    /// True when the worktree slot reflects an unstaged change OR the
    /// file is untracked — both shapes carry "would be touched by an
    /// over-broad `git add .`". Ignored files (`!`) stay out so the
    /// preflight doesn't flag `.gitignore`d build artefacts.
    pub(super) fn is_changed(&self) -> bool {
        match (self.index_status, self.worktree_status) {
            ('!', _) | (_, '!') => false,
            _ => self.index_status != ' ' || self.worktree_status != ' ',
        }
    }
}

/// Parse the textual output of `git status --porcelain=v1`. Returns an
/// owned `Vec<PorcelainEntry>` so the caller is free of any borrow on
/// the source string.
pub(super) fn parse_porcelain_status(text: &str) -> Vec<PorcelainEntry> {
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.is_empty() {
            continue;
        }
        let bytes = raw.as_bytes();
        if bytes.len() < 4 {
            continue;
        }
        let index_status = bytes[0] as char;
        let worktree_status = bytes[1] as char;
        let rest = &raw[3..];
        let path = if (index_status == 'R' || index_status == 'C') && rest.contains(" -> ") {
            rest.split(" -> ").nth(1).unwrap().to_string()
        } else {
            rest.to_string()
        };
        out.push(PorcelainEntry {
            index_status,
            worktree_status,
            path,
        });
    }
    out
}

/// Run `git status --porcelain=v1` under `root` (read-only). Returns the
/// raw stdout text on success, or a structured `ToolResult` error when
/// git is unavailable or refuses to operate on the path.
pub(super) fn run_git_status(root: &Path) -> std::result::Result<String, ToolResult> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(root)
        .output()
        .map_err(|e| {
            ToolResult::structured_error(
                ToolError::new(
                    error_codes::EXTERNAL_ERROR,
                    format!(
                        "failed to spawn `git status` under {}: {}",
                        root.display(),
                        e
                    ),
                )
                .with_suggestion("ensure git is installed and the project root is a worktree"),
            )
        })?;
    if !output.status.success() {
        return Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::EXTERNAL_ERROR,
                format!(
                    "`git status` exited non-zero under {}: {}",
                    root.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            )
            .with_suggestion("verify the project root is a git worktree (no `--git-dir` override)"),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
