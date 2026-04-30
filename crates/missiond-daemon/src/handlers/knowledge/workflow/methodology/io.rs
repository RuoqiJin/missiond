use super::*;

/// Per-process monotonic counter feeding [`unique_generated_yaml_temp_path`]
/// so two writers landing on the same generated YAML inside the same nanosecond
/// (or on a coarse-clock filesystem) still receive distinct temp file names.
static GENERATED_YAML_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Build a per-attempt temp file path that lives in the same directory as
/// `target` so the subsequent `rename` stays atomic on POSIX (same-FS).
///
/// Layout: `<leaf>.tmp.<pid>.<unix_nanos>.<seq>`. The fixed legacy extension
/// (a literal kept only as a regression marker — see the `static_temp` test
/// suffix below) is deliberately avoided because two concurrent
/// compile_methodology writers on the same `flow_id` would otherwise share
/// one temp path and corrupt each other's output before the rename.
///
/// This is a workflow.rs-local mirror of the unique-temp helper that
/// `file_artifacts` will eventually expose; we keep it private here until that
/// foundation crate publishes a stable surface (referenced by Task 4b — once
/// the shared `unique_temp_path_in_dir(target: &Path) -> PathBuf` lands, both
/// callers should converge on it).
pub(in crate::handlers::knowledge::workflow) fn unique_generated_yaml_temp_path(
    target: &Path,
) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let leaf = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "anonymous".to_string());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = GENERATED_YAML_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{leaf}.tmp.{pid}.{nanos}.{seq}"))
}

/// Atomic write for the methodology compiler's generated YAML target.
///
/// Behavior:
///   - Auto-creates parent directories.
///   - Writes to a per-attempt unique temp file in the same directory so
///     concurrent compile_methodology calls on the same `flow_id` cannot
///     trample each other's temp file (rename remains same-FS atomic).
///   - On either write or rename failure, removes ONLY this attempt's temp
///     file (path-specific cleanup) and propagates the underlying IO error.
///     The cleanup is `let _ =` because the propagated error is the real
///     signal — silent retries would mask the root cause.
pub(in crate::handlers::knowledge::workflow) fn atomic_write(
    path: &Path,
    content: &str,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = unique_generated_yaml_temp_path(path);
    if let Err(e) = std::fs::write(&tmp, content) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}
