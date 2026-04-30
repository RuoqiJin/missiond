use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use missiond_core::types::SharedProjectRegistry;
use missiond_mcp::tools::ToolResult;
use serde::Serialize;
use serde_json::{json, Value};
use tracing::{error, info};

use crate::engine::flow::loader;
use crate::slot_orchestrator::project_root::{
    resolve_target_project_root, ResolutionError, ResolutionSource,
};
use crate::state::AppState;

pub(crate) async fn handle(
    state: &AppState,
    _name: &str,
    args: serde_json::Value,
) -> Result<ToolResult> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("run");

    match action {
        "list" => action_list(state, &args).await,
        "status" => action_status(state, &args).await,
        "run" => action_run(state, &args).await,
        other => Ok(ToolResult::error(format!(
            "Unknown action: {}. Use: run, list, status",
            other
        ))),
    }
}

async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let project = resolve_project_root_from_args(args, &state.project_registry).await;
    let list = loader::list_flows_with_project(project.root.as_deref())?;

    Ok(ToolResult::json_pretty(&json!({
        "flows": list.merged_ids,
        "count": list.merged_ids.len(),
        "core": entries_to_json(&list.core),
        "generated": entries_to_json(&list.generated),
        "searched_paths": paths_to_strings(&list.searched_paths),
        "project_root": project.root_display(),
        "project_root_status": project.status.as_str(),
        "project_root_source": project.source.as_str(),
        "project_root_diagnostic": project.diagnostic,
    })))
}

async fn action_status(state: &AppState, args: &Value) -> Result<ToolResult> {
    let task_id = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("'task_id' required for status"))?;
    let task = state
        .store
        .get_board_task(task_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
    let ctx: Option<crate::engine::flow::FlowContext> = task
        .flow_context
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok());
    Ok(ToolResult::json_pretty(&json!({
        "task_id": task_id,
        "flow_phase": task.flow_phase,
        "status": task.status.as_str(),
        "context": ctx,
    })))
}

async fn action_run(state: &AppState, args: &Value) -> Result<ToolResult> {
    let project = resolve_project_root_from_args(args, &state.project_registry).await;
    let flow_id_arg = args
        .get("flow_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let flow_path_arg = args
        .get("flow_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    if flow_id_arg.is_none() && flow_path_arg.is_none() {
        return Err(anyhow!("'flow_id' or 'flow_path' required for run"));
    }

    let loaded = match flow_path_arg {
        Some(raw) => {
            let resolved = resolve_flow_path_arg(raw, project.root.as_deref())?;
            loader::load_flow_from_path_with_project(&resolved, project.root.as_deref())?
        }
        None => {
            let id = flow_id_arg.expect("checked above");
            loader::load_flow_with_project(id, project.root.as_deref())?
        }
    };

    let flow = &loaded.definition;

    let input = missiond_core::types::CreateBoardTaskInput {
        title: format!("Flow: {}", flow.name),
        category: Some("flow".to_string()),
        description: Some(format!("Flow v2: '{}'", flow.id)),
        flow_template: Some(flow.id.clone()),
        ..Default::default()
    };
    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let task_id = task.id.to_string();

    let mut ctx = crate::engine::flow::FlowContext::new();
    if let Some(params) = args.get("params").and_then(|v| v.as_object()) {
        for (k, v) in params {
            ctx.set(k.clone(), v.as_str().unwrap_or(&v.to_string()));
        }
    }

    let _ = state
        .store
        .update_board_task(
            &task_id,
            &missiond_core::types::UpdateBoardTaskInput {
                flow_phase: Some("running".to_string()),
                flow_context: Some(serde_json::to_string(&ctx).unwrap_or_default()),
                status: Some("running".to_string()),
                ..Default::default()
            },
        )
        .await;

    info!(
        flow_id = %flow.id,
        task_id = %task_id,
        flow_source = loaded.source.as_str(),
        flow_path = %loaded.path.display(),
        "Flow: executing"
    );
    let result = crate::engine::flow::runner::run_flow(state, flow, &mut ctx, &task_id).await;

    match result {
        Ok(()) => {
            let _ = state
                .store
                .update_board_task(
                    &task_id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("completed".to_string()),
                        status: Some("done".to_string()),
                        ..Default::default()
                    },
                )
                .await;
            Ok(ToolResult::json_pretty(&json!({
                "task_id": task_id,
                "flow_id": flow.id,
                "status": "completed",
                "completed_nodes": ctx.completed_nodes,
                "flow_source": loaded.source.as_str(),
                "flow_path": loaded.path.display().to_string(),
                "project_root": project.root_display(),
                "project_root_status": project.status.as_str(),
            })))
        }
        Err(e) => {
            error!(task_id = %task_id, error = %e, "Flow: failed");
            let _ = state
                .store
                .update_board_task(
                    &task_id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("failed".to_string()),
                        status: Some("failed".to_string()),
                        ..Default::default()
                    },
                )
                .await;
            Err(e)
        }
    }
}

fn entries_to_json(entries: &[loader::FlowEntry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "path": e.path.display().to_string(),
                    "source": e.source.as_str(),
                })
            })
            .collect(),
    )
}

fn paths_to_strings(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

fn resolve_flow_path_arg(raw: &str, project_root: Option<&Path>) -> Result<PathBuf> {
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        return Ok(candidate);
    }
    let Some(root) = project_root else {
        return Err(anyhow!(
            "relative flow_path `{}` requires resolved project_root; pass an absolute flow_path or project/target_project/cwd",
            raw
        ));
    };
    Ok(root.join(candidate))
}

// ───────────────────────────────────────────────────────────────────────
// project root resolution — pure-ish helpers exposed for unit tests.
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectRootStatus {
    /// Caller did not pass project / target_project / cwd.
    NotRequested,
    /// Caller asked for a project but we could not pin it to a path on disk
    /// (unregistered id and no path-like signal).
    Unresolved,
    /// We resolved a directory and used it for search.
    Resolved,
}

impl ProjectRootStatus {
    fn as_str(self) -> &'static str {
        match self {
            ProjectRootStatus::NotRequested => "not_requested",
            ProjectRootStatus::Unresolved => "unresolved",
            ProjectRootStatus::Resolved => "resolved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectRootSource {
    None,
    ExplicitPath,
    RegistryId,
    /// `cwd` was an absolute existing path under a registered project; the
    /// resolved root is the canonical project root (longest-prefix match
    /// in `ProjectRegistry`). Mirrors `slot_orchestrator::project_root::
    /// ResolutionSource::CwdLongestPrefix` so callers see the same source
    /// vocabulary as compute_slot / pty / process / task_delegate spawn
    /// paths.
    RegistryLongestPrefix,
}

impl ProjectRootSource {
    fn as_str(self) -> &'static str {
        match self {
            ProjectRootSource::None => "none",
            ProjectRootSource::ExplicitPath => "explicit_path",
            ProjectRootSource::RegistryId => "registry_id",
            ProjectRootSource::RegistryLongestPrefix => "registry_longest_prefix",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectRoot {
    pub root: Option<PathBuf>,
    pub status: ProjectRootStatus,
    pub source: ProjectRootSource,
    pub diagnostic: Option<String>,
}

impl ProjectRoot {
    fn root_display(&self) -> Option<String> {
        self.root.as_ref().map(|p| p.display().to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectRootArgs {
    pub project: Option<String>,
    pub target_project: Option<String>,
    pub cwd: Option<String>,
}

impl ProjectRootArgs {
    pub fn from_value(v: &Value) -> Self {
        let s = |key: &str| {
            v.get(key)
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        Self {
            project: s("project"),
            target_project: s("target_project"),
            cwd: s("cwd"),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.project.is_none() && self.target_project.is_none() && self.cwd.is_none()
    }
}

async fn resolve_project_root_from_args(
    args: &Value,
    registry: &SharedProjectRegistry,
) -> ProjectRoot {
    let raw = ProjectRootArgs::from_value(args);
    if raw.is_empty() {
        return ProjectRoot {
            root: None,
            status: ProjectRootStatus::NotRequested,
            source: ProjectRootSource::None,
            diagnostic: None,
        };
    }

    // Resolution order — aligned with intent-worker.lisp :: invariant
    // project-root-spawn-cwd resolution-order [r1 → r2 → r3]:
    //
    //   1. `project` → registered id wins; otherwise treat as absolute
    //      existing path (legacy ExplicitPath, supports "I'm pointing at
    //      a project root that isn't registered yet").
    //   2. `target_project` → same logic, alias-style fallback.
    //   3. `cwd` → must be absolute existing. Try longest-prefix match
    //      against `ProjectRegistry`; if it lands inside a registered
    //      project we resolve to the *project root* (RegistryLongestPrefix,
    //      same vocabulary as slot_orchestrator::project_root). If no
    //      registered project covers cwd, fall back to using cwd itself
    //      as the root (ExplicitPath) so unregistered project dirs still
    //      work for one-off `mission_flow_run` calls.
    //
    // Relative cwd is rejected outright — we never silently fall back to
    // the daemon's process CWD (would leak the wrong project into flow
    // discovery).
    let mut diagnostics: Vec<String> = Vec::new();
    let try_path = |raw: &str| -> Option<PathBuf> {
        let p = PathBuf::from(raw);
        if p.is_absolute() && p.exists() {
            Some(canonicalize_or_self(&p))
        } else {
            None
        }
    };

    {
        let reg = registry.read().await;
        for (label, value) in [
            ("project", raw.project.as_deref()),
            ("target_project", raw.target_project.as_deref()),
        ] {
            let Some(v) = value else { continue };
            if let Some(p) = reg.get(v) {
                return ProjectRoot {
                    root: Some(PathBuf::from(&p.path)),
                    status: ProjectRootStatus::Resolved,
                    source: ProjectRootSource::RegistryId,
                    diagnostic: None,
                };
            } else if let Some(p) = try_path(v) {
                return ProjectRoot {
                    root: Some(p),
                    status: ProjectRootStatus::Resolved,
                    source: ProjectRootSource::ExplicitPath,
                    diagnostic: None,
                };
            } else {
                diagnostics.push(format!(
                    "{}={} not registered and not an absolute existing path",
                    label, v
                ));
            }
        }
    }

    if let Some(cwd_raw) = raw.cwd.as_deref() {
        let cwd_path = match try_path(cwd_raw) {
            Some(p) => p,
            None => {
                diagnostics.push(format!(
                    "cwd={} not an absolute existing path (relative cwd is never resolved against process CWD)",
                    cwd_raw
                ));
                return ProjectRoot {
                    root: None,
                    status: ProjectRootStatus::Unresolved,
                    source: ProjectRootSource::None,
                    diagnostic: Some(diagnostics.join("; ")),
                };
            }
        };

        // Delegate to the canonical helper for longest-prefix lookup so
        // flow_run, compute_slot, pty, process and task_delegate all
        // share the same registry resolution path. If cwd is outside any
        // registered project we fall back to ExplicitPath (unregistered
        // project root usage is still legal for mission_flow_run), but
        // unknown_project_id / no_signal errors still propagate as
        // diagnostics.
        match resolve_target_project_root(None, Some(cwd_path.as_path()), None, registry).await {
            Ok(resolution) if resolution.source == ResolutionSource::CwdLongestPrefix => {
                return ProjectRoot {
                    root: Some(resolution.project_root),
                    status: ProjectRootStatus::Resolved,
                    source: ProjectRootSource::RegistryLongestPrefix,
                    diagnostic: None,
                };
            }
            Ok(_) => {
                // The canonical helper only returns ExplicitProjectId /
                // FallbackProjectId when those args are passed (we passed
                // None for both), so this arm is structurally unreachable.
                // Treat as longest-prefix anyway to stay safe.
                return ProjectRoot {
                    root: Some(cwd_path),
                    status: ProjectRootStatus::Resolved,
                    source: ProjectRootSource::RegistryLongestPrefix,
                    diagnostic: None,
                };
            }
            Err(ResolutionError::CwdOutsideRegisteredProject { .. }) => {
                // cwd is a real directory but not under any registered
                // project — preserve legacy "use cwd as root directly"
                // behavior so unregistered project dirs keep working.
                return ProjectRoot {
                    root: Some(cwd_path),
                    status: ProjectRootStatus::Resolved,
                    source: ProjectRootSource::ExplicitPath,
                    diagnostic: None,
                };
            }
            Err(e) => {
                diagnostics.push(format!("cwd resolution failed: {}", e));
            }
        }
    }

    ProjectRoot {
        root: None,
        status: ProjectRootStatus::Unresolved,
        source: ProjectRootSource::None,
        diagnostic: Some(diagnostics.join("; ")),
    }
}

fn canonicalize_or_self(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_core::types::{ProjectConfig, ProjectRegistry};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn registry_with(projects: Vec<ProjectConfig>) -> SharedProjectRegistry {
        Arc::new(RwLock::new(ProjectRegistry::new(projects)))
    }

    fn project(id: &str, path: &str) -> ProjectConfig {
        ProjectConfig {
            id: id.to_string(),
            path: path.to_string(),
            intent_path: None,
            active: true,
            slots: vec![],
            github_url: None,
            kind: "managed".to_string(),
            vault_path: None,
            parent_id: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn no_args_marks_not_requested() {
        let reg = registry_with(vec![]);
        let r = resolve_project_root_from_args(&json!({}), &reg).await;
        assert_eq!(r.status, ProjectRootStatus::NotRequested);
        assert!(r.root.is_none());
    }

    #[tokio::test]
    async fn explicit_existing_path_wins_over_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().to_path_buf();
        let reg = registry_with(vec![project("missiond", "/totally/elsewhere")]);
        let r =
            resolve_project_root_from_args(&json!({ "cwd": p.display().to_string() }), &reg).await;
        assert_eq!(r.status, ProjectRootStatus::Resolved);
        assert_eq!(r.source, ProjectRootSource::ExplicitPath);
        assert_eq!(
            r.root.unwrap().canonicalize().unwrap(),
            p.canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn registry_id_resolves_when_no_path_works() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = registry_with(vec![project("missiond", &tmp.path().display().to_string())]);
        let r = resolve_project_root_from_args(&json!({ "project": "missiond" }), &reg).await;
        assert_eq!(r.status, ProjectRootStatus::Resolved);
        assert_eq!(r.source, ProjectRootSource::RegistryId);
    }

    #[tokio::test]
    async fn unknown_signals_mark_unresolved_with_diagnostic() {
        let reg = registry_with(vec![]);
        let r = resolve_project_root_from_args(
            &json!({ "project": "ghost", "cwd": "relative/dir" }),
            &reg,
        )
        .await;
        assert_eq!(r.status, ProjectRootStatus::Unresolved);
        assert!(r.diagnostic.is_some());
        let diag = r.diagnostic.unwrap();
        assert!(diag.contains("ghost"));
        assert!(diag.contains("relative/dir"));
    }

    #[tokio::test]
    async fn relative_cwd_does_not_silently_resolve_to_process_cwd() {
        // Guard against accidentally resolving "./foo" via process cwd —
        // that would let a misconfigured caller leak the daemon's cwd into
        // flow search. We only accept absolute existing paths.
        let reg = registry_with(vec![]);
        let r = resolve_project_root_from_args(&json!({ "cwd": "./relative" }), &reg).await;
        assert_eq!(r.status, ProjectRootStatus::Unresolved);
    }

    #[test]
    fn project_root_args_filters_empty_strings() {
        let v = json!({ "project": "", "target_project": "  ", "cwd": "/tmp" });
        let r = ProjectRootArgs::from_value(&v);
        assert!(r.project.is_none());
        assert!(r.target_project.is_none());
        assert_eq!(r.cwd.as_deref(), Some("/tmp"));
        assert!(!r.is_empty());
    }

    #[test]
    fn relative_flow_path_requires_project_root() {
        let err = resolve_flow_path_arg(".missiond/generated/flows/x.yaml", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires resolved project_root"));
    }

    #[test]
    fn relative_flow_path_resolves_under_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved =
            resolve_flow_path_arg(".missiond/generated/flows/x.yaml", Some(tmp.path())).unwrap();
        assert_eq!(
            resolved,
            tmp.path().join(".missiond/generated/flows/x.yaml")
        );
    }

    #[test]
    fn absolute_flow_path_does_not_require_project_root() {
        let resolved = resolve_flow_path_arg("/tmp/x.yaml", None).unwrap();
        assert_eq!(resolved, PathBuf::from("/tmp/x.yaml"));
    }

    // ── longest-prefix lane tests (canonical helper integration) ──
    //
    // These cover the new RegistryLongestPrefix source: when a caller
    // passes `cwd` that lives under a registered project root, flow_run
    // must resolve the *project root*, not the literal cwd. Same
    // semantics as `slot_orchestrator::project_root::resolve_target_
    // project_root`, so flow_run shares the spawn-cwd vocabulary used
    // by compute_slot / pty / process / task_delegate.

    #[tokio::test]
    async fn cwd_at_project_root_resolves_via_longest_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().to_path_buf();
        let canonical = project_root.canonicalize().unwrap();
        let reg = registry_with(vec![project("alpha", &canonical.display().to_string())]);
        let r = resolve_project_root_from_args(
            &json!({ "cwd": canonical.display().to_string() }),
            &reg,
        )
        .await;
        assert_eq!(r.status, ProjectRootStatus::Resolved);
        assert_eq!(r.source, ProjectRootSource::RegistryLongestPrefix);
        assert_eq!(r.root.unwrap(), canonical);
    }

    #[tokio::test]
    async fn cwd_inside_registered_project_resolves_to_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().canonicalize().unwrap();
        let subdir = project_root.join("crates").join("inner");
        std::fs::create_dir_all(&subdir).unwrap();
        let reg = registry_with(vec![project("alpha", &project_root.display().to_string())]);
        let r =
            resolve_project_root_from_args(&json!({ "cwd": subdir.display().to_string() }), &reg)
                .await;
        assert_eq!(r.status, ProjectRootStatus::Resolved);
        assert_eq!(r.source, ProjectRootSource::RegistryLongestPrefix);
        assert_eq!(r.root.unwrap(), project_root);
    }

    #[tokio::test]
    async fn nested_projects_pick_longest_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let parent_root = tmp.path().canonicalize().unwrap();
        let child_root = parent_root.join("subprojects").join("child");
        std::fs::create_dir_all(&child_root).unwrap();
        let inside_child = child_root.join("src");
        std::fs::create_dir_all(&inside_child).unwrap();
        let reg = registry_with(vec![
            project("parent", &parent_root.display().to_string()),
            project("child", &child_root.display().to_string()),
        ]);
        let r = resolve_project_root_from_args(
            &json!({ "cwd": inside_child.display().to_string() }),
            &reg,
        )
        .await;
        assert_eq!(r.status, ProjectRootStatus::Resolved);
        assert_eq!(r.source, ProjectRootSource::RegistryLongestPrefix);
        assert_eq!(r.root.unwrap(), child_root);
    }

    #[tokio::test]
    async fn cwd_outside_any_registered_project_falls_back_to_explicit_path() {
        // When cwd is a real absolute existing dir but no registered
        // project covers it, mission_flow_run should still let the caller
        // use cwd as the project root (legacy ExplicitPath behavior).
        let tmp_proj = tempfile::tempdir().unwrap();
        let tmp_cwd = tempfile::tempdir().unwrap();
        let canonical_cwd = tmp_cwd.path().canonicalize().unwrap();
        let reg = registry_with(vec![project(
            "alpha",
            &tmp_proj
                .path()
                .canonicalize()
                .unwrap()
                .display()
                .to_string(),
        )]);
        let r = resolve_project_root_from_args(
            &json!({ "cwd": canonical_cwd.display().to_string() }),
            &reg,
        )
        .await;
        assert_eq!(r.status, ProjectRootStatus::Resolved);
        assert_eq!(r.source, ProjectRootSource::ExplicitPath);
        assert_eq!(r.root.unwrap(), canonical_cwd);
    }

    #[tokio::test]
    async fn relative_cwd_with_relative_flow_path_is_rejected_end_to_end() {
        // Combined contract: if cwd cannot resolve, a relative flow_path
        // has nothing to anchor against and resolve_flow_path_arg must
        // refuse rather than silently joining the daemon's process CWD.
        let reg = registry_with(vec![]);
        let project_root =
            resolve_project_root_from_args(&json!({ "cwd": "relative/dir" }), &reg).await;
        assert_eq!(project_root.status, ProjectRootStatus::Unresolved);
        assert!(project_root.root.is_none());
        let err = resolve_flow_path_arg(
            ".missiond/generated/flows/x.yaml",
            project_root.root.as_deref(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("requires resolved project_root"));
    }

    #[tokio::test]
    async fn no_args_still_supports_core_flow_lookup_without_root() {
        // Old behavior: callers using $MISSIOND_HOME/flows core flows
        // do not need to pass project / cwd. status = NotRequested,
        // root = None — the loader then falls through to the core
        // flow path. Guard regression where requiring a project_root
        // would break legacy core-flow callers.
        let reg = registry_with(vec![]);
        let r = resolve_project_root_from_args(&json!({}), &reg).await;
        assert_eq!(r.status, ProjectRootStatus::NotRequested);
        assert_eq!(r.source, ProjectRootSource::None);
        assert!(r.root.is_none());
        // Loader contract: load_flow_with_project(None, …) only searches
        // core. The resolver returning None is exactly what enables that.
    }

    #[tokio::test]
    async fn registry_id_via_project_arg_still_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().canonicalize().unwrap();
        let reg = registry_with(vec![project("alpha", &canonical.display().to_string())]);
        let r = resolve_project_root_from_args(&json!({ "project": "alpha" }), &reg).await;
        assert_eq!(r.status, ProjectRootStatus::Resolved);
        assert_eq!(r.source, ProjectRootSource::RegistryId);
        assert_eq!(
            r.root.unwrap(),
            PathBuf::from(canonical.display().to_string())
        );
    }
}
