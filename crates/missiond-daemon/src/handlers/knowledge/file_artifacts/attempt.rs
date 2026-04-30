use super::*;

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
