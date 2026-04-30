use super::*;

/// Resolve the canonical project root for any workflow.rs file write site
/// (compile_methodology persist YAML / future distill .lisp writer / etc.).
///
/// Contract (intent-flow.lisp F-intent-alignment-plan-execution-loop ::
/// :file-vs-db-contract + intent-worker.lisp :: invariant
/// `project-root-spawn-cwd`):
///   - Single canonical resolver shared with directive / plan / flow_run /
///     compute_slot / pty / task_delegate via
///     `slot_orchestrator::project_root::resolve_target_project_root`.
///   - `project` (arg)        → `explicit_project_id`.
///   - `cwd` (arg)            → `explicit_cwd`, ONLY when absolute. Relative
///     cwd is refused so the daemon never silently joins it onto its own
///     process cwd (process-cwd fallback would violate the file-vs-db
///     contract by planting the file SSOT outside the registered project).
///   - `target_project` (arg) → `fallback_project_id` (mirrors the slot
///     spawn resolution order).
///   - Missing every signal   → fail-fast (`ResolutionError::NoSignal`).
///   - Process-cwd fallback   → never. Ever.
///
/// State-bound thin wrapper used by the action handlers; the actual logic
/// lives in [`resolve_project_root_with_registry`] so unit tests can drive
/// it without reconstructing the whole `AppState` graph.
pub(super) async fn resolve_project_root_from_args(
    state: &AppState,
    args: &Value,
) -> std::result::Result<PathBuf, String> {
    resolve_project_root_with_registry(&state.project_registry, args).await
}

/// Registry-bound implementation of [`resolve_project_root_from_args`].
///
/// Returns a `String` error (instead of `anyhow::Error`) so write-side
/// callers can decide whether to wrap into a `ToolError` (compile path) or
/// fold into a `partial` payload (post-DB write path).
pub(super) async fn resolve_project_root_with_registry(
    registry: &missiond_core::types::SharedProjectRegistry,
    args: &Value,
) -> std::result::Result<PathBuf, String> {
    // Empty-string fields must be treated as "absent", not as
    // explicit-empty-id — otherwise we'd hand the registry "" and produce a
    // confusing "project '' is not registered" error.
    let project = args
        .get("project")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let target_project = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let cwd_raw = args.get("cwd").and_then(|v| v.as_str()).map(str::trim);
    // Only absolute cwd is honored. We pre-filter so a relative cwd never
    // reaches the canonical resolver as `Some(...)` and the daemon never
    // joins it onto its own process working directory.
    let cwd_path: Option<PathBuf> = cwd_raw
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    if let Some(raw) = cwd_raw.filter(|s| !s.is_empty()) {
        if cwd_path.is_none() {
            return Err(format!(
                "cwd `{}` is not absolute; workflow file writer refuses to fall back to process cwd \
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
             workflow file writer refuses process-cwd fallback"
                .to_string(),
        ),
        Err(e) => Err(e.to_string()),
    }
}
