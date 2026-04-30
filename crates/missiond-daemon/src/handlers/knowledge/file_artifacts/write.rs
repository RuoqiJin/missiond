use super::kind::ArtifactMetadata;
use super::*;

/// Process-wide monotonic counter consumed by [`unique_temp_path_in_dir`].
///
/// Two writers landing on the same target inside the same nanosecond — common
/// on coarse-clock filesystems and on macOS where wall-clock resolution can
/// dip into the microsecond range — would otherwise collide on the same temp
/// file path. The counter disambiguates them deterministically per process.
/// `Relaxed` ordering is sufficient because we only need uniqueness, never a
/// happens-before relationship with surrounding state.
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

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
