//! file_artifacts — file-first SSOT writer for directive / plan / workflow
//! artifacts.
//!
//! Lisp authority:
//!   - intent-flow.lisp ::
//!       F-intent-alignment-plan-execution-loop :: :file-vs-db-contract
//!   - intent-memory.lisp :: directive-layer :: file-first-artifacts
//!   - intent-intent-layer.lisp :: unified-entry-pipeline :: :file-first-ssot
//!
//! Path convention (per intent-memory.lisp directive-layer file-first-artifacts):
//!   - alignment : <project_root>/.missiond/alignment/<topic>/intent-alignment.lisp
//!   - plan      : <project_root>/.missiond/plans/<topic>/PLAN.lisp
//!   - workflow  : <project_root>/.missiond/workflows/<topic>.lisp
//!
//! Layered surface:
//!   - Pure helpers (`artifact_path`, `atomic_write_artifact`,
//!     `unique_temp_path_in_dir`, `read_existing_metadata`,
//!     `sanitize_topic_segment`) — no `AppState`, no DB, no events. Foundation
//!     wave-11 introduced these so other writers can compose them.
//!   - `WriterContext` + `attempt_artifact_write` — the manager-side helper
//!     that resolves `project_root` through the canonical
//!     `slot_orchestrator::project_root::resolve_target_project_root` resolver,
//!     enforces the no-process-cwd-fallback contract
//!     (intent-worker.lisp :: project-root-spawn-cwd), and routes overwrite
//!     policy. Compiler actors (`directive`, `plan`, `workflow`) call this
//!     after their DB mirror commit so the file-first SSOT and the row stay
//!     in sync, with `partial`/`error` semantics surfaced to the caller.
//!
//! `attempt_artifact_write` deliberately does NOT touch DB rows and does NOT
//! publish events — that contract belongs to the calling action so the file
//! write can fail without dragging down the row, and so this module stays
//! easy to unit-test end-to-end against `tempfile`.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::slot_orchestrator::project_root::{resolve_target_project_root, ResolutionError};
use missiond_core::types::SharedProjectRegistry;

/// Process-wide monotonic counter consumed by [`unique_temp_path_in_dir`].
///
/// Two writers landing on the same target inside the same nanosecond — common
/// on coarse-clock filesystems and on macOS where wall-clock resolution can
/// dip into the microsecond range — would otherwise collide on the same temp
/// file path. The counter disambiguates them deterministically per process.
/// `Relaxed` ordering is sufficient because we only need uniqueness, never a
/// happens-before relationship with surrounding state.
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Topic segment used when sanitization yields an empty string.
///
/// Mirrors the spirit of `workflow.rs`'s `methodology-anonymous-v0` fallback:
/// we never want a writer to silently land on `.missiond/alignment//…`.
const ANONYMOUS_TOPIC: &str = "anonymous";

/// Three artifact kinds covered by intent-memory.lisp directive-layer
/// file-first-artifacts. Anything outside these three must use a different
/// helper — this module deliberately enumerates them so a typo cannot pick the
/// wrong directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ArtifactKind {
    /// `.missiond/alignment/<topic>/intent-alignment.lisp` — directive /
    /// alignment file-first SSOT (intent-memory.lisp ::
    /// `intent-alignment-artifact`).
    IntentAlignment,
    /// `.missiond/plans/<topic>/PLAN.lisp` — plan file-first SSOT
    /// (intent-memory.lisp :: `plan-file`).
    Plan,
    /// `.missiond/workflows/<topic>.lisp` — workflow methodology file-first
    /// SSOT (intent-memory.lisp :: `workflow-methodology-file`).
    Workflow,
}

impl ArtifactKind {
    /// Human-readable label for diagnostics and logs.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            ArtifactKind::IntentAlignment => "intent-alignment",
            ArtifactKind::Plan => "plan",
            ArtifactKind::Workflow => "workflow",
        }
    }
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Caller-facing description of one artifact write request.
///
/// `file_name` is reserved for kinds whose default file name does not apply
/// (currently none — alignment/plan use a fixed file under a per-topic dir;
/// workflow puts the topic itself in the file name). It is wired through so
/// future topics (e.g. multi-version `intent-alignment-v2.lisp`) can opt in
/// without touching `artifact_path` callers.
///
/// Today the directive / plan / workflow writers all construct artifact paths
/// directly via [`artifact_path`]; the spec form remains as a foundation API
/// so future multi-version writers (e.g. `intent-alignment-v2.lisp`) can opt
/// into [`artifact_path_from_spec`] without touching the existing call sites.
#[derive(Debug, Clone)]
#[allow(dead_code)] // foundation API; future multi-version writers may opt into spec form
pub(crate) struct ArtifactSpec {
    pub kind: ArtifactKind,
    pub topic: String,
    pub project_root: PathBuf,
    pub file_name: Option<String>,
}

/// Metadata returned after a successful atomic write.
///
/// `created` and `overwritten` are mutually exclusive:
///   - `created=true,  overwritten=false` — fresh file written.
///   - `created=false, overwritten=true ` — pre-existing file replaced.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct WriteOutcome {
    pub path: PathBuf,
    pub created: bool,
    pub overwritten: bool,
    pub sha256: String,
    pub bytes: u64,
}

/// Metadata snapshot of an existing artifact on disk. Only used for read-only
/// inspection (e.g. drift detection vs DB mirror).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ArtifactMetadata {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

/// Sanitize a topic into a single safe path segment.
///
/// Rules (aligned with `workflow::sanitize_id_token`):
///   - keep ASCII alphanumerics, `_`, `-`.
///   - replace any other run of characters with a single `-`.
///   - trim leading / trailing `-`.
///   - if the result is empty, fall back to [`ANONYMOUS_TOPIC`].
///
/// Stability is contractual: future writers compare topics across runs to
/// detect rewrites of the same file-first SSOT, so the function must remain
/// idempotent (`sanitize(sanitize(x)) == sanitize(x)`).
pub(crate) fn sanitize_topic_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_hyphen = false;
    for ch in input.chars() {
        let allowed = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        if allowed {
            out.push(ch);
            prev_hyphen = ch == '-';
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        ANONYMOUS_TOPIC.to_string()
    } else {
        trimmed
    }
}

/// Resolve the canonical artifact path for `(kind, topic)` under
/// `project_root`. The topic is sanitized internally; callers do not need to
/// pre-sanitize. Path layout matches intent-memory.lisp directive-layer
/// file-first-artifacts and is the single authority across the daemon.
pub(crate) fn artifact_path(project_root: &Path, kind: ArtifactKind, topic: &str) -> PathBuf {
    let safe_topic = sanitize_topic_segment(topic);
    match kind {
        ArtifactKind::IntentAlignment => project_root
            .join(".missiond")
            .join("alignment")
            .join(&safe_topic)
            .join("intent-alignment.lisp"),
        ArtifactKind::Plan => project_root
            .join(".missiond")
            .join("plans")
            .join(&safe_topic)
            .join("PLAN.lisp"),
        ArtifactKind::Workflow => project_root
            .join(".missiond")
            .join("workflows")
            .join(format!("{}.lisp", safe_topic)),
    }
}

/// Resolve an artifact path from an [`ArtifactSpec`], honoring an explicit
/// `file_name` override when supplied. Today only workflow can sensibly use
/// the override (its file name embeds the topic); for alignment/plan the
/// override replaces the default leaf file name within the per-topic dir.
#[allow(dead_code)] // foundation API
pub(crate) fn artifact_path_from_spec(spec: &ArtifactSpec) -> PathBuf {
    let default = artifact_path(&spec.project_root, spec.kind, &spec.topic);
    match (&spec.file_name, spec.kind) {
        (Some(name), _) if !name.is_empty() => match default.parent() {
            Some(parent) => parent.join(name),
            None => PathBuf::from(name),
        },
        _ => default,
    }
}

/// Build a per-attempt temp file path that lives in the same directory as
/// `target` so the subsequent `rename` stays atomic on POSIX (same filesystem).
///
/// Layout: `<leaf>.tmp.<pid>.<unix_nanos>.<seq>`. Concretely:
///   - `<leaf>` is `target.file_name()` (or `"anonymous"` if `target` has no
///     leaf — defensive only; valid artifact paths always have one).
///   - `<pid>` disambiguates across daemon restarts and across cooperating
///     processes that share a tempdir.
///   - `<unix_nanos>` is wall-clock nanoseconds, providing monotonicity at
///     coarse resolution.
///   - `<seq>` is a per-process atomic counter that disambiguates writers
///     that land in the same nanosecond (common on macOS APFS).
///
/// We deliberately do **not** use a fixed `with_extension(...)` suffix
/// because two concurrent writers on the same `target` would otherwise
/// share one temp path and corrupt each other's payload before the rename.
/// Same reason `workflow.rs` keeps a private mirror of this logic — it
/// should converge on this helper now that the foundation publishes a
/// stable surface (`pub(crate)`).
///
/// The returned path is **not** created on disk; the caller opens it.
pub(crate) fn unique_temp_path_in_dir(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let leaf = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "anonymous".to_string());
    let pid = std::process::id();
    // SystemTime::now() before UNIX_EPOCH would only happen if the system
    // clock is set to before 1970 — we treat that as 0 nanos rather than
    // panicking. The pid + counter still keep the path unique.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{leaf}.tmp.{pid}.{nanos}.{seq}"))
}

/// Compute SHA-256 over `content` and return the lowercase-hex digest.
fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut s, "{:02x}", byte);
    }
    s
}

/// Atomic write helper.
///
/// Behavior:
///   - auto-creates parent directories.
///   - if the target exists and `overwrite=false`, refuses with an error so
///     the caller can surface a structured rejection (no silent merge).
///   - writes to a per-attempt unique temp path produced by
///     [`unique_temp_path_in_dir`] (same directory as `path` so the rename
///     stays atomic on POSIX), then `rename`s onto `path`.
///   - flushes + fsyncs the temp file before rename so partial writes do not
///     leak across crashes.
///   - on failure cleans up only this attempt's temp file (path-specific,
///     never globs `*.tmp.*`). Cleanup uses `let _ =` because the propagated
///     error is the real signal — silent retries would mask the root cause.
///
/// Concurrency: two callers writing the same `path` simultaneously each get a
/// distinct temp file (pid + nanos + per-process counter). The final
/// `rename` is the only ordering point — last writer wins, which matches the
/// `overwrite=true` contract; with `overwrite=false` the pre-flight `exists`
/// check still races but that is the caller's problem (and is documented).
///
/// Returns metadata describing what happened (created vs overwritten + size +
/// hash). Callers use this to mirror the artifact into the DB row.
pub(crate) fn atomic_write_artifact(
    path: &Path,
    content: &str,
    overwrite: bool,
) -> Result<WriteOutcome> {
    let pre_exists = path.exists();
    if pre_exists && !overwrite {
        return Err(anyhow!(
            "artifact already exists at {}; pass overwrite=true to replace",
            path.display()
        ));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent dir {}", parent.display()))?;
    }

    let tmp = unique_temp_path_in_dir(path);
    {
        let mut f = fs::File::create(&tmp)
            .with_context(|| format!("create temp file {}", tmp.display()))?;
        if let Err(e) = f.write_all(content.as_bytes()) {
            // Drop the file handle, then remove the temp we just opened —
            // path-specific cleanup so we never touch a sibling temp owned by
            // a different concurrent writer.
            drop(f);
            let _ = fs::remove_file(&tmp);
            return Err(anyhow::Error::new(e).context(format!("write temp file {}", tmp.display())));
        }
        if let Err(e) = f.flush() {
            drop(f);
            let _ = fs::remove_file(&tmp);
            return Err(anyhow::Error::new(e).context(format!("flush temp file {}", tmp.display())));
        }
        // Best-effort fsync; on filesystems where it is not supported we still
        // get atomic visibility from rename. We surface the error rather than
        // silently swallow it.
        if let Err(e) = f.sync_all() {
            drop(f);
            let _ = fs::remove_file(&tmp);
            return Err(anyhow::Error::new(e).context(format!("sync temp file {}", tmp.display())));
        }
    }
    if let Err(e) = fs::rename(&tmp, path) {
        // Clean up only this attempt's temp file. We never glob "*.tmp.*"
        // because a sibling concurrent writer may own a parallel temp that we
        // must not touch.
        let _ = fs::remove_file(&tmp);
        return Err(anyhow::Error::new(e).context(format!(
            "rename {} -> {}",
            tmp.display(),
            path.display()
        )));
    }

    let bytes = content.as_bytes().len() as u64;
    let sha256 = sha256_hex(content);
    Ok(WriteOutcome {
        path: path.to_path_buf(),
        created: !pre_exists,
        overwritten: pre_exists,
        sha256,
        bytes,
    })
}

/// Read an existing artifact's metadata (sha256 + bytes) without loading the
/// content into a result struct. Returns `Ok(None)` when the file is absent
/// — callers can then choose to bootstrap.
///
/// IO errors other than `NotFound` propagate so the daemon never silently
/// downgrades a permission error into "no artifact present".
#[allow(dead_code)] // foundation API; reserved for drift-detection callers (no live consumer yet)
pub(crate) fn read_existing_metadata(path: &Path) -> Result<Option<ArtifactMetadata>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(
                anyhow::Error::new(e).context(format!("read artifact metadata {}", path.display()))
            );
        }
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut sha256 = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut sha256, "{:02x}", byte);
    }
    Ok(Some(ArtifactMetadata {
        path: path.to_path_buf(),
        sha256,
        bytes: bytes.len() as u64,
    }))
}

// ───────────────────────────────────────────────────────────────────────
// State-bound writer surface — composed by the directive / plan / workflow
// compilers. Resolves project_root via the canonical
// `slot_orchestrator::project_root::resolve_target_project_root` so every
// file-first write lands under a registered project (no process-cwd
// fallback). Returns a structured outcome the caller can splice into the
// JSON response without re-implementing the file-vs-db contract.
// ───────────────────────────────────────────────────────────────────────

/// The compiler-visible request to write one file-first artifact. All four
/// fields come from the action's args (`project` / `cwd` / `target_project`
/// + `topic` / `overwrite_file`); the writer enforces:
///   - `cwd` must be absolute when supplied (no process-cwd join).
///   - at least one project signal is required (no implicit registry guess).
///   - `topic` is sanitized via [`sanitize_topic_segment`].
///
/// The `kind` is set by the calling action and locks the on-disk layout via
/// [`artifact_path`].
#[derive(Debug, Clone)]
pub(crate) struct WriterContext<'a> {
    pub kind: ArtifactKind,
    pub topic: &'a str,
    /// Optional explicit `project=<id>` arg.
    pub project: Option<&'a str>,
    /// Optional explicit `cwd=<absolute path>` arg.
    pub cwd: Option<&'a str>,
    /// Optional fallback `target_project=<id>` arg.
    pub target_project: Option<&'a str>,
    /// `overwrite_file=true` opts into replacing an existing artifact;
    /// default policy refuses to overwrite (file-first SSOT immutability).
    pub overwrite: bool,
}

/// Outcome of an [`attempt_artifact_write`] call. The three variants mirror
/// the partial / error semantics surfaced in the action response:
///   - [`AttemptOutcome::Written`]: file was created or overwritten — caller
///     splices `file_written=true` + path/sha/bytes/created/overwritten.
///   - [`AttemptOutcome::ResolveFailed`]: project root could not be resolved;
///     the DB row was already committed, so the caller surfaces
///     `status="partial"` + `file_write_error` carrying this reason. The DB
///     row is NEVER rolled back — the file-vs-db contract requires the row
///     to remain visible so the caller can retry the file write with a
///     correct project signal without losing the compiled artifact.
///   - [`AttemptOutcome::WriteFailed`]: project root resolved, but the
///     atomic write failed (permission, disk full, racing overwrite refusal,
///     etc.). Same partial-status contract as above.
#[derive(Debug)]
pub(crate) enum AttemptOutcome {
    Written(WriteOutcome),
    ResolveFailed { reason: String },
    WriteFailed { path: PathBuf, reason: String },
}

impl AttemptOutcome {
    /// Splice the outcome into a `payload` JSON object. The caller picks the
    /// payload — typically the action's `compiled` / `dry_run` / `distilled`
    /// envelope — and we extend it with the canonical file-first fields:
    ///
    ///   - `file_written`        : bool, true only for `Written`
    ///   - `file_path`           : string display path (always set when known)
    ///   - `file_sha256`         : hex digest of the written bytes
    ///   - `file_bytes`          : number of bytes written
    ///   - `file_created`        : bool, true on a fresh file
    ///   - `file_overwritten`    : bool, true on a replaced file
    ///   - `file_write_error`    : structured reason on resolve/write failure
    ///   - `status`              : downgraded to `"partial"` on failure
    ///
    /// `status` rewrite is conservative: we only set `"partial"` when the
    /// payload already carried a non-`"partial"` status (so a caller that
    /// already declared `"partial"` for unrelated reasons keeps its label).
    pub(crate) fn splice_into(&self, payload: &mut Value) {
        let map = match payload.as_object_mut() {
            Some(m) => m,
            None => return,
        };
        match self {
            AttemptOutcome::Written(w) => {
                map.insert("file_written".to_string(), json!(true));
                map.insert("file_path".to_string(), json!(w.path.display().to_string()));
                map.insert("file_sha256".to_string(), json!(w.sha256));
                map.insert("file_bytes".to_string(), json!(w.bytes));
                map.insert("file_created".to_string(), json!(w.created));
                map.insert("file_overwritten".to_string(), json!(w.overwritten));
            }
            AttemptOutcome::ResolveFailed { reason } => {
                map.insert("file_written".to_string(), json!(false));
                map.insert("file_write_error".to_string(), json!(reason));
                downgrade_to_partial(map);
            }
            AttemptOutcome::WriteFailed { path, reason } => {
                map.insert("file_written".to_string(), json!(false));
                map.insert("file_path".to_string(), json!(path.display().to_string()));
                map.insert("file_write_error".to_string(), json!(reason));
                downgrade_to_partial(map);
            }
        }
    }
}

fn downgrade_to_partial(map: &mut serde_json::Map<String, Value>) {
    let already_partial = map
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s == "partial")
        .unwrap_or(false);
    if !already_partial {
        map.insert("status".to_string(), json!("partial"));
    }
}

/// Resolve project_root + write the artifact file atomically.
///
/// The async signature lets us hold the registry's read lock briefly via
/// [`resolve_target_project_root`] without blocking the runtime, while the
/// actual fs write stays synchronous (POSIX rename is the only ordering
/// point we need). Errors are returned as [`AttemptOutcome`] variants — we
/// deliberately do NOT bubble them into `Result` so the calling action can
/// keep its DB-then-file ordering: the row is already committed by the time
/// this is called, and a write failure must not abort the response.
///
/// Lisp authority:
///   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
///       :file-vs-db-contract (file write is best-effort post-DB; never
///       roll back the row on file failure).
///   - intent-worker.lisp :: project-root-spawn-cwd (single canonical
///       resolver, no process-cwd fallback).
///   - intent-memory.lisp :: directive-layer :: file-first-artifacts
///       (per-kind layout authority — kept here in [`artifact_path`]).
pub(crate) async fn attempt_artifact_write(
    registry: &SharedProjectRegistry,
    ctx: WriterContext<'_>,
    content: &str,
) -> AttemptOutcome {
    let project_root =
        match resolve_writer_project_root(registry, ctx.project, ctx.cwd, ctx.target_project).await
        {
            Ok(p) => p,
            Err(reason) => return AttemptOutcome::ResolveFailed { reason },
        };
    let target = artifact_path(&project_root, ctx.kind, ctx.topic);
    match atomic_write_artifact(&target, content, ctx.overwrite) {
        Ok(outcome) => AttemptOutcome::Written(outcome),
        Err(e) => AttemptOutcome::WriteFailed {
            path: target,
            reason: format!("{:#}", e),
        },
    }
}

/// Registry-bound thin layer that mirrors the canonical resolver contract
/// shared with `slot_orchestrator::spawner` / `compute::flow_run` /
/// `knowledge::plan` / `knowledge::workflow`. Pulled out of the async fn so
/// unit tests can drive the resolution policy without writing files.
pub(crate) async fn resolve_writer_project_root(
    registry: &SharedProjectRegistry,
    project: Option<&str>,
    cwd: Option<&str>,
    target_project: Option<&str>,
) -> std::result::Result<PathBuf, String> {
    // Empty-string fields must be treated as "absent", not as
    // explicit-empty-id — otherwise we'd hand the registry "" and produce a
    // confusing "project '' is not registered" error. Mirrors the same
    // pre-filter `workflow.rs` already applies for compile_methodology.
    let project = project.map(str::trim).filter(|s| !s.is_empty());
    let target_project = target_project.map(str::trim).filter(|s| !s.is_empty());
    let cwd_raw = cwd.map(str::trim);
    let cwd_path: Option<PathBuf> = cwd_raw
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    if let Some(raw) = cwd_raw.filter(|s| !s.is_empty()) {
        if cwd_path.is_none() {
            return Err(format!(
                "cwd `{}` is not absolute; file-first writer refuses to fall back to process cwd \
                 (intent-worker.lisp :: project-root-spawn-cwd). Pass an absolute cwd or supply project / target_project.",
                raw
            ));
        }
    }
    match resolve_target_project_root(project, cwd_path.as_deref(), target_project, registry).await
    {
        Ok(r) => Ok(r.project_root),
        Err(ResolutionError::NoSignal) => Err(
            "no project_id, absolute cwd, or fallback target_project supplied; \
             file-first writer refuses process-cwd fallback"
                .to_string(),
        ),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests;
