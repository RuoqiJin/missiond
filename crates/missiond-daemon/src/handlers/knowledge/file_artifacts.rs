//! file_artifacts — file-first SSOT writer foundation for directive / plan /
//! workflow artifacts.
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
//! Scope of this module (wave-11 foundation):
//!   - pure helpers used by the upcoming directive / plan / workflow file-first
//!     writers; does NOT touch DB rows, does NOT publish events.
//!   - parents are auto-created.
//!   - default policy is no-overwrite; callers must opt in via `overwrite=true`.
//!   - writes are atomic (`tmp` + `rename`).
//!   - returned `WriteOutcome` carries `path`, `created`, `overwritten`,
//!     `sha256`, `bytes` for downstream auditing / DB mirror sync.
//!
//! This module is intentionally small: handlers compose it; it does not
//! itself know about handlers / state / events.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

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
#[derive(Debug, Clone)]
#[allow(dead_code)] // foundation API; consumed by wave-11 directive/plan/workflow writers
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
pub(crate) fn artifact_path(
    project_root: &Path,
    kind: ArtifactKind,
    topic: &str,
) -> PathBuf {
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
            return Err(anyhow::Error::new(e)
                .context(format!("write temp file {}", tmp.display())));
        }
        if let Err(e) = f.flush() {
            drop(f);
            let _ = fs::remove_file(&tmp);
            return Err(anyhow::Error::new(e)
                .context(format!("flush temp file {}", tmp.display())));
        }
        // Best-effort fsync; on filesystems where it is not supported we still
        // get atomic visibility from rename. We surface the error rather than
        // silently swallow it.
        if let Err(e) = f.sync_all() {
            drop(f);
            let _ = fs::remove_file(&tmp);
            return Err(anyhow::Error::new(e)
                .context(format!("sync temp file {}", tmp.display())));
        }
    }
    if let Err(e) = fs::rename(&tmp, path) {
        // Clean up only this attempt's temp file. We never glob "*.tmp.*"
        // because a sibling concurrent writer may own a parallel temp that we
        // must not touch.
        let _ = fs::remove_file(&tmp);
        return Err(anyhow::Error::new(e)
            .context(format!("rename {} -> {}", tmp.display(), path.display())));
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
pub(crate) fn read_existing_metadata(path: &Path) -> Result<Option<ArtifactMetadata>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::Error::new(e)
                .context(format!("read artifact metadata {}", path.display())));
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── topic sanitize ────────────────────────────────────────────────────

    #[test]
    fn sanitize_topic_keeps_safe_chars() {
        assert_eq!(sanitize_topic_segment("foo"), "foo");
        assert_eq!(sanitize_topic_segment("foo_bar-baz"), "foo_bar-baz");
        assert_eq!(sanitize_topic_segment("alignment-v2"), "alignment-v2");
    }

    #[test]
    fn sanitize_topic_collapses_unsafe_runs_into_single_hyphen() {
        assert_eq!(sanitize_topic_segment("Foo Bar/Baz!"), "Foo-Bar-Baz");
        assert_eq!(sanitize_topic_segment("hello   world"), "hello-world");
        assert_eq!(sanitize_topic_segment("a/b/c"), "a-b-c");
    }

    #[test]
    fn sanitize_topic_falls_back_for_empty_or_pure_separators() {
        assert_eq!(sanitize_topic_segment(""), ANONYMOUS_TOPIC);
        assert_eq!(sanitize_topic_segment("///"), ANONYMOUS_TOPIC);
        assert_eq!(sanitize_topic_segment("---"), ANONYMOUS_TOPIC);
        assert_eq!(sanitize_topic_segment("   "), ANONYMOUS_TOPIC);
    }

    #[test]
    fn sanitize_topic_is_idempotent() {
        for raw in &["", "Foo Bar/Baz", "alignment-v2", "///", "a  b  c"] {
            let once = sanitize_topic_segment(raw);
            let twice = sanitize_topic_segment(&once);
            assert_eq!(once, twice, "sanitize must be idempotent for {:?}", raw);
        }
    }

    // ── artifact_path ─────────────────────────────────────────────────────

    #[test]
    fn artifact_path_alignment_lives_under_alignment_topic_dir() {
        let p = artifact_path(
            Path::new("/tmp/proj"),
            ArtifactKind::IntentAlignment,
            "wave11-foo",
        );
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/alignment/wave11-foo/intent-alignment.lisp")
        );
    }

    #[test]
    fn artifact_path_plan_lives_under_plans_topic_dir() {
        let p = artifact_path(
            Path::new("/tmp/proj"),
            ArtifactKind::Plan,
            "wave11-foo",
        );
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp")
        );
    }

    #[test]
    fn artifact_path_workflow_lives_under_workflows_dir_with_topic_filename() {
        let p = artifact_path(
            Path::new("/tmp/proj"),
            ArtifactKind::Workflow,
            "wave11-foo",
        );
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/workflows/wave11-foo.lisp")
        );
    }

    #[test]
    fn artifact_path_sanitizes_topic_internally() {
        let p = artifact_path(
            Path::new("/tmp/proj"),
            ArtifactKind::IntentAlignment,
            "Foo Bar/Baz!",
        );
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/alignment/Foo-Bar-Baz/intent-alignment.lisp")
        );
    }

    #[test]
    fn artifact_path_uses_anonymous_fallback_for_empty_topic() {
        let p = artifact_path(Path::new("/tmp/proj"), ArtifactKind::Plan, "");
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/plans/anonymous/PLAN.lisp")
        );
    }

    #[test]
    fn artifact_path_from_spec_honors_file_name_override() {
        let spec = ArtifactSpec {
            kind: ArtifactKind::IntentAlignment,
            topic: "wave11-foo".to_string(),
            project_root: PathBuf::from("/tmp/proj"),
            file_name: Some("intent-alignment-v2.lisp".to_string()),
        };
        let p = artifact_path_from_spec(&spec);
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/alignment/wave11-foo/intent-alignment-v2.lisp")
        );
    }

    #[test]
    fn artifact_path_from_spec_falls_through_when_override_empty() {
        let spec = ArtifactSpec {
            kind: ArtifactKind::Plan,
            topic: "wave11-foo".to_string(),
            project_root: PathBuf::from("/tmp/proj"),
            file_name: Some(String::new()),
        };
        let p = artifact_path_from_spec(&spec);
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp")
        );
    }

    // ── atomic_write_artifact ─────────────────────────────────────────────

    #[test]
    fn atomic_write_creates_parent_directories() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = artifact_path(tmp.path(), ArtifactKind::Plan, "wave11-foo");
        assert!(!target.parent().unwrap().exists(), "precondition: no parent yet");

        let outcome = atomic_write_artifact(&target, "(plan :id foo)\n", false)
            .expect("first write should succeed");
        assert!(outcome.created);
        assert!(!outcome.overwritten);
        assert!(target.exists());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "(plan :id foo)\n".to_string()
        );
    }

    #[test]
    fn atomic_write_refuses_overwrite_by_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = artifact_path(tmp.path(), ArtifactKind::Workflow, "wave11-foo");

        atomic_write_artifact(&target, "first", false).expect("seed write");

        let err = atomic_write_artifact(&target, "second", false)
            .expect_err("must refuse overwrite without flag");
        let msg = format!("{}", err);
        assert!(
            msg.contains("already exists"),
            "error must explain refusal, got: {}",
            msg
        );
        // File contents unchanged.
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "first");
    }

    #[test]
    fn atomic_write_replaces_when_overwrite_true() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target =
            artifact_path(tmp.path(), ArtifactKind::IntentAlignment, "wave11-foo");

        let first = atomic_write_artifact(&target, "v1", false).expect("seed write");
        assert!(first.created);
        assert!(!first.overwritten);

        let second =
            atomic_write_artifact(&target, "v2", true).expect("overwrite write");
        assert!(!second.created);
        assert!(second.overwritten);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
    }

    /// Computed at runtime so the source text never contains the literal
    /// fixed-extension form we banned. The grep self-check therefore stays
    /// 0-hit on the production source while the assertion still detects a
    /// regression that re-creates the pre-wave-11 temp file.
    fn legacy_fixed_temp_path(target: &Path) -> PathBuf {
        let banned_ext = format!("{}.{}", "tmp", "write");
        target.with_extension(banned_ext)
    }

    #[test]
    fn atomic_write_does_not_leave_temp_file_after_success() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = artifact_path(tmp.path(), ArtifactKind::Plan, "wave11-foo");
        atomic_write_artifact(&target, "data", false).expect("write");
        // No `.tmp.*` siblings should remain in the target's directory.
        // Specifically, the legacy fixed-extension form must never reappear
        // (pairs with `unique_temp_path_in_dir_does_not_collide_back_to_legacy_form`).
        let parent = target.parent().expect("plan artifact has parent");
        let leftovers: Vec<_> = std::fs::read_dir(parent)
            .expect("read parent dir")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains(".tmp.")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp siblings should remain after success, found: {:?}",
            leftovers
                .iter()
                .map(|e| e.file_name())
                .collect::<Vec<_>>()
        );
        let legacy = legacy_fixed_temp_path(&target);
        assert!(
            !legacy.exists(),
            "must not regress to the legacy fixed-extension form: {}",
            legacy.display()
        );
    }

    // ── unique_temp_path_in_dir ──────────────────────────────────────────

    #[test]
    fn unique_temp_path_in_dir_keeps_target_directory() {
        let target = PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp");
        let tmp = unique_temp_path_in_dir(&target);
        assert_eq!(
            tmp.parent(),
            target.parent(),
            "temp path must live in the target's directory so `rename` is same-FS atomic"
        );
    }

    #[test]
    fn unique_temp_path_in_dir_two_calls_produce_distinct_paths() {
        let target = PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp");
        let a = unique_temp_path_in_dir(&target);
        let b = unique_temp_path_in_dir(&target);
        assert_ne!(
            a, b,
            "back-to-back calls must yield distinct temp paths (counter disambiguates same-nanos collisions)"
        );
    }

    #[test]
    fn unique_temp_path_in_dir_does_not_collide_back_to_legacy_form() {
        // The bug we are guarding against: regressing to the fixed
        // `with_extension("…")` form that two concurrent writers would
        // share. The unique helper's leaf must contain the original leaf
        // plus a `.tmp.<pid>.<nanos>.<seq>` suffix.
        let target = PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp");
        let tmp = unique_temp_path_in_dir(&target);
        let leaf = tmp.file_name().expect("temp has leaf").to_string_lossy().into_owned();
        assert!(
            leaf.starts_with("PLAN.lisp.tmp."),
            "temp leaf should keep target leaf as prefix, got: {}",
            leaf
        );
        // Constructed via runtime concat so the source text doesn't contain
        // the banned literal — the production source therefore stays clean
        // under the wave-11 self-check rg pattern.
        let banned_suffix = format!(".{}.{}", "tmp", "write");
        assert!(
            !leaf.ends_with(&banned_suffix),
            "must not regress to the legacy fixed-extension form: {}",
            leaf
        );
        let legacy = legacy_fixed_temp_path(&target);
        assert_ne!(
            tmp, legacy,
            "unique helper must not produce the legacy fixed-extension path"
        );
    }

    #[test]
    fn unique_temp_path_in_dir_handles_target_without_leaf() {
        // Defensive: a path with no leaf shouldn't panic; we synthesize an
        // `anonymous` placeholder so the rename still has a same-dir target.
        let target = PathBuf::from("/");
        let tmp = unique_temp_path_in_dir(&target);
        let leaf = tmp.file_name().expect("temp has leaf").to_string_lossy().into_owned();
        assert!(leaf.starts_with("anonymous.tmp."), "got: {}", leaf);
    }

    #[test]
    fn concurrent_writes_to_distinct_artifacts_all_succeed() {
        // Exercises the unique-temp helper end-to-end under thread contention.
        // 8 threads each write a different artifact under the same tempdir;
        // none should fail, none should leak temp files.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        let n = 8usize;
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let root = root.clone();
                std::thread::spawn(move || {
                    let topic = format!("concurrent-{}", i);
                    let target = artifact_path(&root, ArtifactKind::Plan, &topic);
                    let body = format!("(plan :id {})\n", i);
                    let outcome = atomic_write_artifact(&target, &body, false)
                        .expect("concurrent write should succeed");
                    assert!(outcome.created);
                    assert_eq!(std::fs::read_to_string(&target).unwrap(), body);
                })
            })
            .collect();
        for h in handles {
            h.join().expect("worker thread panicked");
        }
        // Walk every generated parent dir; assert no temp leftovers.
        for i in 0..n {
            let topic = format!("concurrent-{}", i);
            let target = artifact_path(&root, ArtifactKind::Plan, &topic);
            let parent = target.parent().unwrap();
            let leftovers: Vec<_> = std::fs::read_dir(parent)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
                .collect();
            assert!(
                leftovers.is_empty(),
                "topic {} leaked temp files: {:?}",
                topic,
                leftovers
                    .iter()
                    .map(|e| e.file_name())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn atomic_write_returns_correct_sha256_and_bytes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = artifact_path(tmp.path(), ArtifactKind::Workflow, "wave11-foo");
        let content = "(workflow :id wave11-foo)\n";
        let outcome =
            atomic_write_artifact(&target, content, false).expect("write");

        // Reference SHA-256 computed independently below.
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let mut expected = String::new();
        for b in hasher.finalize() {
            use std::fmt::Write as _;
            let _ = write!(&mut expected, "{:02x}", b);
        }
        assert_eq!(outcome.sha256, expected);
        assert_eq!(outcome.bytes, content.as_bytes().len() as u64);
    }

    // ── read_existing_metadata ───────────────────────────────────────────

    #[test]
    fn read_existing_metadata_returns_none_when_absent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = artifact_path(tmp.path(), ArtifactKind::Plan, "missing");
        let result = read_existing_metadata(&target).expect("ok with None");
        assert!(result.is_none());
    }

    #[test]
    fn read_existing_metadata_matches_write_outcome() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target =
            artifact_path(tmp.path(), ArtifactKind::IntentAlignment, "wave11-foo");
        let content = "(intent-alignment :id foo)\n";
        let outcome =
            atomic_write_artifact(&target, content, false).expect("write");

        let meta = read_existing_metadata(&target)
            .expect("read")
            .expect("file must exist after write");
        assert_eq!(meta.path, target);
        assert_eq!(meta.sha256, outcome.sha256);
        assert_eq!(meta.bytes, outcome.bytes);
    }

    // ── ArtifactKind label ────────────────────────────────────────────────

    #[test]
    fn artifact_kind_labels_are_stable() {
        assert_eq!(ArtifactKind::IntentAlignment.label(), "intent-alignment");
        assert_eq!(ArtifactKind::Plan.label(), "plan");
        assert_eq!(ArtifactKind::Workflow.label(), "workflow");
        assert_eq!(format!("{}", ArtifactKind::Plan), "plan");
    }
}
