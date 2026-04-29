use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::state::AppState;

use super::claim_lease::{parse_claims, scopes_overlap_pure};
use super::completion_audit::collect_string_list;
use super::log_store::{
    companion_path, project_or_target_project, read_log_file, require_str, resolve_project_root,
    LogFile,
};
use super::session_trace::{
    append_session_trace_event, resolve_session_trace_path, resolve_trace_task_id, TraceEvent,
    TraceKind,
};

// ───────────────────────────────────────────────────────────────────────
// action: preflight_commit — read-only worktree audit before scoped commit
//
// Wave 18 / Task 08. The daemon may inspect git status / diff but MUST
// NEVER stage/commit/reset/checkout. The writer agent is the only actor
// that mutates the worktree; we just project worktree state vs the
// active+released claim scopes so the writer can see scope drift before
// running its scoped commit.
//
// Pairs with `enforce_scoped_commit_completion` (wave16-06) which is the
// post-commit gate; preflight catches the same violations one step
// earlier so the writer doesn't have to roll back a bad stage.
//
// Wave 20 / Task 03 augmentation: when the caller threads
// `task_contract_path` through the preflight call, daemon also loads the
// task-contract v1 (read-only) and projects the staged set against the
// contract's `:write-scope` / `:must-not-touch` patterns. Two new
// top-level fields (`staged_out_of_scope`, `staged_forbidden`) plus
// `unstaged_in_scope` and a `task_contract_status` label surface so the
// writer learns about contract-level drift one hop earlier than the
// post-commit `task-scope-guard.mjs`. Daemon still runs no mutating git
// command — `evaluate_task_contract_for_preflight` is pure file IO + a
// glob projection.
// ───────────────────────────────────────────────────────────────────────

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
///
/// Rules:
///   - skip blank lines.
///   - rename entries (`R` / `C` in the index slot) carry the rename
///     pair on a single line as `RENAMED -> ORIG`; we record the
///     right-hand side which is the post-rename path, matching what the
///     scoped-commit audit cares about.
///   - quoted paths (porcelain c-style escapes when the path contains
///     special bytes) are forwarded verbatim with the surrounding
///     quotes — this preserves round-trip fidelity even though
///     scope-overlap matching against quoted paths will fail-by-design;
///     the violator surfaces in `out_of_scope_files` so the writer can
///     widen the claim or rename the file.
///
/// We keep this parser deliberately tiny and pure: no panics, no
/// allocations beyond the obvious `String` per path, no calls into the
/// process. That means the fail-fast contract from the task brief — the
/// daemon never spawns a mutating git command — sits one level up
/// (`run_git_status`).
pub(super) fn parse_porcelain_status(text: &str) -> Vec<PorcelainEntry> {
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.is_empty() {
            continue;
        }
        let bytes = raw.as_bytes();
        if bytes.len() < 4 {
            // Defensive: malformed line, skip silently. Porcelain v1
            // always emits at least `XY <path>` (4+ chars).
            continue;
        }
        let index_status = bytes[0] as char;
        let worktree_status = bytes[1] as char;
        let rest = &raw[3..];
        // Rename / copy pairs separate `OLD -> NEW`; we pin the new
        // path because that is what lives on disk after `git add`.
        let path = if (index_status == 'R' || index_status == 'C') && rest.contains(" -> ") {
            // unwrap is safe because contains() returned true.
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

/// Collect every claim scope on the companion log, regardless of
/// status. Mirrors `enforce_scoped_commit_completion` — both
/// active and released claims count for scope-overlap purposes
/// because `F-scoped-commit-handoff :: s7` legitimately commits inside
/// a just-released claim window.
pub(super) fn collect_all_claim_scopes(file: &LogFile) -> Vec<String> {
    parse_claims(file)
        .iter()
        .map(|c| c.scope.clone())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Restrict to the scope of a specific claim id when caller supplies
/// `claim_id`. Returns `Err` with a structured `NOT_FOUND` ToolResult
/// when the claim id does not match any record so the writer learns
/// the typo before running git.
pub(super) fn collect_specific_claim_scope(
    file: &LogFile,
    claim_id: &str,
) -> std::result::Result<Vec<String>, ToolResult> {
    let claims = parse_claims(file);
    let hit = claims.iter().find(|c| c.id == claim_id);
    match hit {
        Some(c) if !c.scope.is_empty() => Ok(vec![c.scope.clone()]),
        Some(_) => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("claim {} has no scope set", claim_id),
            )
            .with_suggestion("rerun with claim_id omitted to use the union of all claim scopes"),
        )),
        None => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("claim_id `{}` not found on companion log", claim_id),
            )
            .with_suggestion("call action=status to list active claim ids"),
        )),
    }
}

/// wave-20 / task 03 — repo-relative path-vs-pattern matcher used by the
/// task-contract scope projection in preflight. Mirrors the JS helper in
/// `scripts/lib/missiond_lisp.mjs::pathMatchesPattern` so daemon-side
/// preflight, the post-commit guard (`scripts/task-scope-guard.mjs`), and
/// the verifier (`scripts/verify-task-contract.mjs`) all key off the same
/// glob semantics. The contract is intentionally narrow:
///
///   * Patterns and paths are normalised by stripping `\\` → `/`,
///     leading `./`, and leading `/` so the comparison is repo-relative.
///   * A pattern with no glob meta-characters matches either the exact
///     path OR any file under that path when the pattern names a
///     directory prefix (e.g. `crates/` or `crates` matches
///     `crates/foo/bar.rs`).
///   * `*` matches any sequence of characters except `/`.
///   * `**` matches any sequence including `/` (folder hops).
///   * `?` matches a single character except `/`.
///   * Other regex meta-characters are escaped — the matcher is glob-only,
///     never a full regex evaluator.
///
/// Daemon-only fail-fast posture: an empty pattern OR an empty path never
/// matches. Empty inputs are a contract bug upstream; we surface them as
/// "no match" so the caller sees the path land in `staged_out_of_scope`
/// rather than silently coercing them through.
pub(super) fn pattern_matches_path(file_path: &str, pattern: &str) -> bool {
    if file_path.is_empty() || pattern.is_empty() {
        return false;
    }
    let norm_path = normalize_repo_relative(file_path);
    let pat = normalize_repo_relative(pattern);
    if !pat.contains('*') && !pat.contains('?') {
        if norm_path == pat {
            return true;
        }
        let prefix = if pat.ends_with('/') {
            pat.clone()
        } else {
            format!("{}/", pat)
        };
        return norm_path.starts_with(&prefix);
    }
    glob_to_regex(&pat).is_match(&norm_path)
}

/// Normalise a path or pattern to a repo-relative form: backslash → slash,
/// strip a single leading `./`, and any leading `/` so absolute-style
/// patterns (rare in our contracts) still match repo-relative entries.
fn normalize_repo_relative(input: &str) -> String {
    let mut s = input.replace('\\', "/");
    if let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// Compile a glob pattern into a regex anchored on both ends. Mirrors the
/// JS `globToRegExp` in `scripts/lib/missiond_lisp.mjs` so the JS guard
/// and the daemon-side preflight stay in lock-step.
fn glob_to_regex(pattern: &str) -> regex::Regex {
    let mut out = String::with_capacity(pattern.len() + 4);
    out.push('^');
    let bytes: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '*' {
            if i + 1 < bytes.len() && bytes[i + 1] == '*' {
                out.push_str(".*");
                i += 2;
                // mirror the JS swallow: a following `/` is consumed by `.*`
            } else {
                out.push_str("[^/]*");
                i += 1;
            }
        } else if c == '?' {
            out.push_str("[^/]");
            i += 1;
        } else if matches!(
            c,
            '.' | '+' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\'
        ) {
            out.push('\\');
            out.push(c);
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.push('$');
    // Pattern is glob-derived so cannot fail; build a permissive fallback
    // (matches nothing) to preserve fail-fast posture without panicking on
    // pathological contract input.
    regex::Regex::new(&out).unwrap_or_else(|_| regex::Regex::new("$.^").unwrap())
}

/// wave-20 / task 03 — pure projection of staged + changed files against a
/// task-contract v1's `:write-scope` and `:must-not-touch` patterns.
///
/// Shape (folded into the preflight response under `task_contract_scope`):
///   - `staged_out_of_scope`: staged paths that match no `:write-scope`
///      entry (and are not on `:must-not-touch`). Authoritative drift
///      signal; populates the new top-level `staged_out_of_scope` field.
///   - `staged_forbidden`: staged paths that match at least one
///      `:must-not-touch` pattern. Always considered out-of-scope.
///   - `unstaged_in_scope`: changed-but-not-staged paths that DO overlap
///      `:write-scope`. Surfaces "you edited it but forgot to stage it"
///      so the writer knows what to add.
///   - `next_step`: terse hint mirroring the wave16-06 enforcement
///      prose so a single screen tells the writer what to fix.
///   - `task_contract_status` is set by the caller (`loaded` / `missing` /
///      `malformed`) and merged on top of this projection.
///
/// Empty `write_scope` is treated as "contract declared no scope" — every
/// staged path then becomes out-of-scope, matching the verifier's
/// fail-fast posture (`scripts/verify-task-contract.mjs` rejects when
/// `:write-scope` is missing).
pub(super) fn build_contract_scope_summary(
    staged_files: &[String],
    changed_files: &[String],
    write_scope: &[String],
    must_not_touch: &[String],
) -> Value {
    let staged_forbidden: Vec<String> = staged_files
        .iter()
        .filter(|p| {
            must_not_touch
                .iter()
                .any(|pat| pattern_matches_path(p, pat))
        })
        .cloned()
        .collect();
    let staged_out_of_scope: Vec<String> = staged_files
        .iter()
        .filter(|p| !write_scope.iter().any(|pat| pattern_matches_path(p, pat)))
        .cloned()
        .collect();
    // `unstaged_in_scope` only counts paths that are changed but NOT
    // staged AND fall inside :write-scope. Lets the writer notice "edit
    // forgotten in `git add`" without flagging legitimate background
    // edits outside scope.
    let unstaged_in_scope: Vec<String> = changed_files
        .iter()
        .filter(|p| !staged_files.contains(p))
        .filter(|p| write_scope.iter().any(|pat| pattern_matches_path(p, pat)))
        .cloned()
        .collect();

    let next_step = if !staged_forbidden.is_empty() {
        format!(
            "unstage paths matching :must-not-touch before committing: {:?}",
            staged_forbidden
        )
    } else if !staged_out_of_scope.is_empty() {
        format!(
            "unstage paths outside :write-scope before committing: {:?}",
            staged_out_of_scope
        )
    } else if !unstaged_in_scope.is_empty() {
        format!(
            "stage the in-scope edits before committing: {:?}",
            unstaged_in_scope
        )
    } else if staged_files.is_empty() {
        "no staged files in scope yet — `git add` your write-scope edits".to_string()
    } else {
        "staged set respects :write-scope and :must-not-touch — proceed with scoped `git commit`"
            .to_string()
    };

    json!({
        "staged_out_of_scope": staged_out_of_scope,
        "staged_forbidden": staged_forbidden,
        "unstaged_in_scope": unstaged_in_scope,
        "write_scope": write_scope,
        "must_not_touch": must_not_touch,
        "next_step": next_step,
    })
}

/// wave-20 / task 03 — read-only contract loader for preflight. Resolves
/// relative paths against the project root, loads via the shared
/// workstation-dispatch projector, and returns the projection summary +
/// `task_contract_status` label. Failures map to `missing` (IO) /
/// `malformed` (parse) so preflight stays informational instead of
/// rejecting — the post-commit gate is the authoritative enforcement.
///
/// Returns `(status, optional_summary, optional_resolved_path,
/// optional_failure_message)`. Caller folds the tuple into the response.
pub(super) fn evaluate_task_contract_for_preflight(
    project_root: &Path,
    task_contract_path: &str,
    staged_files: &[String],
    changed_files: &[String],
) -> (&'static str, Option<Value>, Option<String>, Option<String>) {
    let raw = std::path::Path::new(task_contract_path);
    let resolved: PathBuf = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        project_root.join(raw)
    };
    let resolved_str = resolved.display().to_string();
    match super::super::workstation_dispatch::load_task_contract(&resolved) {
        Ok(contract) => {
            let summary = build_contract_scope_summary(
                staged_files,
                changed_files,
                &contract.write_scope,
                &contract.must_not_touch,
            );
            ("loaded", Some(summary), Some(resolved_str), None)
        }
        Err(err) => {
            use super::super::workstation_dispatch::TaskContractParseError as Tce;
            let (status, msg) = match &err {
                Tce::Io(detail) => (
                    "missing",
                    format!(
                        "task_contract_path `{}` is not readable: {}",
                        resolved.display(),
                        detail
                    ),
                ),
                _ => (
                    "malformed",
                    format!(
                        "task_contract_path `{}` failed schema parse: {}",
                        resolved.display(),
                        err.reason()
                    ),
                ),
            };
            (status, None, Some(resolved_str), Some(msg))
        }
    }
}

/// Pure preflight comparison: given porcelain entries + claim scopes +
/// an optional `expected_files` hint from the dispatch brief, return
/// the structured projection the action surfaces back to the caller.
///
/// Output shape (also wired into the response JSON):
///   - `changed_files`: every porcelain entry whose worktree slot is
///      non-clean (includes untracked).
///   - `staged_files`: every porcelain entry whose index slot is
///      non-clean (excludes untracked).
///   - `out_of_scope_files`: subset of (changed ∪ staged) that does
///      NOT overlap any claim scope.
///   - `expected_missing`: paths in `expected_files` that are NOT in
///      the changed/staged set. Helps the writer notice when a file the
///      brief expected to touch was forgotten.
///   - `expected_unexpected`: paths changed/staged that are NOT in
///      `expected_files`. Surfaced only when `expected_files` is supplied
///      so the writer can audit drift from the plan node's `paths`
///      hint without us hard-failing on it.
///   - `ok`: true iff `out_of_scope_files` is empty.
///   - `next_step`: human-readable hint mirroring the wave16-06
///      enforcement messages so the writer can act without re-reading
///      the contract.
pub(super) fn build_preflight_summary(
    entries: &[PorcelainEntry],
    claim_scopes: &[String],
    expected_files: Option<&[String]>,
) -> Value {
    let changed_files: Vec<String> = entries
        .iter()
        .filter(|e| e.is_changed())
        .map(|e| e.path.clone())
        .collect();
    let staged_files: Vec<String> = entries
        .iter()
        .filter(|e| e.is_staged())
        .map(|e| e.path.clone())
        .collect();

    // Union of changed + staged for scope check, dedup-preserving order.
    let mut union: Vec<String> = Vec::with_capacity(changed_files.len() + staged_files.len());
    for p in changed_files.iter().chain(staged_files.iter()) {
        if !union.contains(p) {
            union.push(p.clone());
        }
    }

    let out_of_scope_files: Vec<String> = if claim_scopes.is_empty() {
        // No claim → every touched file is out-of-scope by definition;
        // the writer must claim before committing.
        union.clone()
    } else {
        union
            .iter()
            .filter(|path| !claim_scopes.iter().any(|cs| scopes_overlap_pure(cs, path)))
            .cloned()
            .collect()
    };

    let mut summary = json!({
        "ok": out_of_scope_files.is_empty(),
        "changed_files": changed_files,
        "staged_files": staged_files,
        "out_of_scope_files": out_of_scope_files,
        "claim_scopes": claim_scopes,
    });

    if let Some(expected) = expected_files {
        let expected_missing: Vec<String> = expected
            .iter()
            .filter(|p| !changed_files.contains(p) && !staged_files.contains(p))
            .cloned()
            .collect();
        let expected_unexpected: Vec<String> = changed_files
            .iter()
            .chain(staged_files.iter())
            .filter(|p| !expected.contains(p))
            .cloned()
            .collect();
        // Dedup expected_unexpected while preserving insertion order so
        // the response is deterministic across porcelain orderings.
        let mut seen_un: Vec<String> = Vec::new();
        for p in expected_unexpected {
            if !seen_un.contains(&p) {
                seen_un.push(p);
            }
        }
        summary["expected_files"] = json!(expected);
        summary["expected_missing"] = json!(expected_missing);
        summary["expected_unexpected"] = json!(seen_un);
    }

    let next_step = if !out_of_scope_files.is_empty() {
        if claim_scopes.is_empty() {
            "open a claim covering the touched paths via `mission_execution(action=claim, scope=…)` before staging anything".to_string()
        } else {
            format!(
                "narrow staged set to claim scope, or open a new claim covering: {:?}",
                out_of_scope_files
            )
        }
    } else if staged_files.is_empty() && changed_files.is_empty() {
        "worktree clean — nothing to commit".to_string()
    } else if staged_files.is_empty() {
        "stage the in-scope edits with `git add <paths>` then re-run preflight before committing"
            .to_string()
    } else {
        "in-scope changes detected — run scoped `git commit`, then call `action=complete` with `enforce_scoped_commit=true`".to_string()
    };
    summary["next_step"] = json!(next_step);

    summary
}

/// Run `git status --porcelain=v1` under `root` (read-only). Returns the
/// raw stdout text on success, or a structured `ToolResult` error when
/// git is unavailable or refuses to operate on the path.
///
/// Safety: the only git subcommand spawned by this module is `status`
/// + `--porcelain=v1`. There is **no** `git add / commit / reset /
/// checkout` codepath in this file — grep for `Command.*git.*(add|
/// commit|reset|checkout)` over `agent_execution.rs` returns zero hits
/// (verified at PR time).
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

pub(super) async fn action_preflight_commit(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };

    // Resolve project root through the registry — same gate every other
    // action uses. Refusing unresolved roots is part of the wave18-08
    // safety contract: we never run git outside an explicitly registered
    // project (or the active CWD when no project is supplied).
    let root = match resolve_project_root(state, project_or_target_project(args)).await {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("cannot resolve project root: {}", e),
                )
                .with_suggestion(
                    "register the project via `mission_project(action=add, …)` or call from inside the project worktree",
                ),
            ));
        }
    };

    // Optional `cwd` override — must stay inside the resolved project
    // root. We canonicalize both sides so symlinks / `..` traversals
    // can't escape the project boundary. If canonicalization fails we
    // refuse rather than silently fall back to root, matching the
    // fail-fast posture of the wave16-06 enforcement gate.
    let cwd_arg = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let inspect_dir = match cwd_arg {
        Some(cwd) => {
            let candidate = PathBuf::from(cwd);
            let abs = if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            };
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.clone());
            let canon_abs = match abs.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult::structured_error(ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!("cwd `{}` does not exist or is not accessible: {}", cwd, e),
                    )));
                }
            };
            if !canon_abs.starts_with(&canon_root) {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "cwd `{}` resolves outside the project root `{}`",
                            cwd,
                            root.display()
                        ),
                    )
                    .with_suggestion("supply a path inside the project, or omit `cwd`"),
                ));
            }
            canon_abs
        }
        None => root.clone(),
    };

    // Expected_files hint from the workstation brief. Trimmed and
    // empty-filtered through the same helper as `staged_files` so the
    // writer doesn't need to pre-clean its list.
    let expected_files = collect_string_list(args, "expected_files");

    // Companion log read — same path resolution as every other action.
    // We need the claims block for scope comparison; opening the file
    // also doubles as a "did the writer pass a real execution_id?"
    // gate, mirroring the rejection shape of action_status.
    let path = companion_path(&root, execution_id);
    let file = match read_log_file(&path) {
        Ok(f) => f,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("companion log {} not readable: {}", path.display(), e),
                )
                .with_suggestion("confirm execution_id matches a previously opened companion log"),
            ));
        }
    };

    // Resolve which claim scope(s) we audit against. Default = union of
    // all claim scopes; explicit `claim_id` narrows to a single scope so
    // the writer can preflight against the exact claim it just acquired.
    let claim_scopes = if let Some(cid) = args
        .get("claim_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        match collect_specific_claim_scope(&file, cid) {
            Ok(scopes) => scopes,
            Err(err) => return Ok(err),
        }
    } else {
        collect_all_claim_scopes(&file)
    };

    // Read-only git status under the inspect_dir. The only mutating
    // codepath in this whole crate is `arch_maintenance_worker`, which
    // lives behind a feature flag the writer agent never reaches; this
    // action stays strictly to `git status --porcelain=v1`.
    let raw_status = match run_git_status(&inspect_dir) {
        Ok(s) => s,
        Err(err) => return Ok(err),
    };
    let entries = parse_porcelain_status(&raw_status);

    let mut summary = build_preflight_summary(&entries, &claim_scopes, expected_files.as_deref());

    // Echo the inputs so the writer agent can correlate the response
    // with the exact dispatch envelope it sent us. `cwd` is the
    // canonicalized form so any symlink / `..` resolution is visible.
    summary["execution_id"] = json!(execution_id);
    summary["cwd"] = json!(inspect_dir.to_string_lossy());
    summary["project_root"] = json!(root.to_string_lossy());
    if let Some(cid) = args.get("claim_id").and_then(|v| v.as_str()) {
        summary["claim_id"] = json!(cid);
    }
    // wave-20 / task 03 — when the caller threads `task_contract_path`
    // through preflight, daemon now loads it (read-only) and projects
    // staged/changed files against the contract's `:write-scope` +
    // `:must-not-touch` so the writer sees scope drift BEFORE running
    // `git commit`. Daemon never mutates the worktree here — load failures
    // surface as `task_contract_status="missing"` / `"malformed"` so the
    // writer can fix the path / file content without preflight hard-
    // rejecting (the post-commit gate at `action=complete` is the
    // authoritative enforcement).
    if let Some(tcp) = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["task_contract_path"] = json!(tcp);
        let staged: Vec<String> = summary
            .get("staged_files")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let changed: Vec<String> = summary
            .get("changed_files")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let (status, scope_summary, resolved_path, failure) =
            evaluate_task_contract_for_preflight(&root, tcp, &staged, &changed);
        summary["task_contract_status"] = json!(status);
        if let Some(rp) = resolved_path {
            summary["task_contract_resolved_path"] = json!(rp);
        }
        if let Some(scope) = scope_summary {
            // Promote the four contract-derived fields to the top level so
            // dashboards keying off `task_contract_status` can read the
            // drift signals without descending one more level. The full
            // projection (including write_scope / must_not_touch echo)
            // stays under `task_contract_scope` for inspectors that want
            // the raw inputs.
            for key in [
                "staged_out_of_scope",
                "staged_forbidden",
                "unstaged_in_scope",
            ] {
                if let Some(v) = scope.get(key) {
                    summary[key] = v.clone();
                }
            }
            // Override `next_step` with the contract-aware hint when the
            // contract added forbidden / out-of-scope drift the claim-only
            // check missed (forbidden patterns aren't a claim concept).
            // Otherwise prefer the existing claim-derived next_step.
            let has_contract_drift = scope
                .get("staged_forbidden")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
                || scope
                    .get("staged_out_of_scope")
                    .and_then(|v| v.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
            if has_contract_drift {
                if let Some(ns) = scope.get("next_step") {
                    summary["next_step"] = ns.clone();
                }
                // Flip `ok=false` because contract-level drift is at least
                // as serious as claim-level drift; downstream consumers
                // already key off `ok` for go/no-go decisions.
                summary["ok"] = json!(false);
            }
            summary["task_contract_scope"] = scope;
        } else if let Some(msg) = failure {
            summary["task_contract_error"] = json!(msg);
        }
    }

    // wave-21 / task 03 — echo the task-run verifier hint paths when
    // the caller threads them through preflight. These are advisory
    // only (the daemon does not load the report at preflight time;
    // the wave21-03 verified-gate at `action=complete` is the
    // authoritative cross-check). Surfacing them here lets the writer
    // confirm the dispatch envelope matches what the script-side
    // verifier (`scripts/verify-task-run.mjs`) will load post-commit.
    if let Some(trp) = args
        .get("task_report_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["task_report_path"] = json!(trp);
    }
    if let Some(smp) = args
        .get("shared_memory_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["shared_memory_path"] = json!(smp);
    }

    // wave23-04 — opt-in session-trace append. Preflight is informational
    // (no commit happens here) so we record it as `observation` carrying
    // the staged + ok flag in the summary text. Best-effort: failures
    // surface as `trace_warning` without flipping the preflight verdict.
    if let Some(trace_path) = resolve_session_trace_path(args, &root) {
        match resolve_trace_task_id(args, &root, execution_id) {
            Some(task_id) => {
                let ok_flag = summary.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
                let staged_count = summary
                    .get("staged_files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let changed_count = summary
                    .get("changed_files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let ev = TraceEvent {
                    task: task_id,
                    backend: "claudecode".to_string(),
                    kind: TraceKind::Observation,
                    summary: format!(
                        "mission_execution(action=preflight_commit) execution_id={} ok={} staged={} changed={}",
                        execution_id, ok_flag, staged_count, changed_count
                    ),
                    agent: None,
                    files: None,
                    commit_hash: None,
                    report_path: None,
                };
                if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                    summary["trace_warning"] = json!(w.to_string());
                }
            }
            None => {
                summary["trace_warning"] = json!(format!(
                    "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                    execution_id
                ));
            }
        }
    }

    Ok(ToolResult::json_pretty(&summary))
}
