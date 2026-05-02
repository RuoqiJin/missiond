//! Project-root resolver for CLI slot spawn — implements
//! `intent-worker.lisp :: invariant project-root-spawn-cwd` resolution-order
//! and `intent-flow.lisp :: F-workflow-slot-full-lifecycle :: s1b-target-project-root`.
//!
//! Contract (project-bound ClaudeCode/Gemini/Codex CLI spawn):
//!   1. Resolve `target_project_root` before spawn or slot reuse.
//!   2. Process cwd must equal `target_project_root`.
//!   3. Caller-supplied `cwd` below a project root is context metadata only.
//!   4. Existing slot reuse only when `slot.project_root == target_project_root`.
//!   5. If the project root cannot be resolved, fail fast.
//!
//! Resolution order:
//!   1. explicit `project_id` -> registered project root
//!   2. explicit `cwd` -> longest-prefix `ProjectRegistry::resolve(cwd)`
//!   3. fallback `project_id` (board task / dynamic slot config) -> registered root
//!   4. (caller decides per-engine policy on no-signal — see callers)

use missiond_core::types::SharedProjectRegistry;
use std::path::{Path, PathBuf};

/// Resolved target project root + the original requested cwd if it differed
/// from the canonical root.
#[derive(Debug, Clone)]
pub struct ProjectRootResolution {
    pub project_id: String,
    pub project_root: PathBuf,
    /// The caller-supplied cwd, only set when it resolved to a sub-path of
    /// `project_root` (i.e. caller asked for a subdir). Not set when cwd was
    /// absent or already equal to the canonical root.
    pub requested_cwd: Option<PathBuf>,
    pub source: ResolutionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSource {
    ExplicitProjectId,
    CwdLongestPrefix,
    FallbackProjectId,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    #[error("project '{0}' is not registered (run mission_project(action=\"list\") to see ids)")]
    UnknownProjectId(String),
    #[error("cwd '{}' is not under any registered project root", .cwd.display())]
    CwdOutsideRegisteredProject { cwd: PathBuf },
    #[error("no project_id, cwd, or fallback signal supplied to resolve target_project_root")]
    NoSignal,
}

/// Resolve `target_project_root` per the lisp resolution order.
///
/// The returned `requested_cwd` is `Some(cwd)` only when the caller's cwd
/// was a strict sub-path of the canonical root. Equal cwd (or absent cwd)
/// → `None`.
pub async fn resolve_target_project_root(
    explicit_project_id: Option<&str>,
    explicit_cwd: Option<&Path>,
    fallback_project_id: Option<&str>,
    registry: &SharedProjectRegistry,
) -> Result<ProjectRootResolution, ResolutionError> {
    let reg = registry.read().await;

    if let Some(id) = explicit_project_id {
        let project = reg
            .get(id)
            .ok_or_else(|| ResolutionError::UnknownProjectId(id.to_string()))?;
        let root = PathBuf::from(&project.path);
        let requested = explicit_cwd.and_then(|c| sub_path_or_none(c, &root));
        return Ok(ProjectRootResolution {
            project_id: project.id.clone(),
            project_root: root,
            requested_cwd: requested,
            source: ResolutionSource::ExplicitProjectId,
        });
    }

    if let Some(cwd) = explicit_cwd {
        let cwd_str = cwd.to_string_lossy().to_string();
        let resolved_id = reg
            .resolve(&cwd_str)
            .map(|s| s.to_string())
            .ok_or_else(|| ResolutionError::CwdOutsideRegisteredProject {
                cwd: cwd.to_path_buf(),
            })?;
        let project = reg
            .get(&resolved_id)
            .ok_or_else(|| ResolutionError::UnknownProjectId(resolved_id.clone()))?;
        let root = PathBuf::from(&project.path);
        let requested = sub_path_or_none(cwd, &root);
        return Ok(ProjectRootResolution {
            project_id: project.id.clone(),
            project_root: root,
            requested_cwd: requested,
            source: ResolutionSource::CwdLongestPrefix,
        });
    }

    if let Some(id) = fallback_project_id {
        let project = reg
            .get(id)
            .ok_or_else(|| ResolutionError::UnknownProjectId(id.to_string()))?;
        let root = PathBuf::from(&project.path);
        return Ok(ProjectRootResolution {
            project_id: project.id.clone(),
            project_root: root,
            requested_cwd: None,
            source: ResolutionSource::FallbackProjectId,
        });
    }

    Err(ResolutionError::NoSignal)
}

/// Return `Some(cwd)` if cwd is a strict sub-path of root (not equal). The
/// resolver uses this to distinguish "cwd already at root" (requested_cwd =
/// None, no metadata to preserve) from "cwd was a subdir" (requested_cwd =
/// Some(cwd), preserved for prompt/context).
fn sub_path_or_none(cwd: &Path, root: &Path) -> Option<PathBuf> {
    if cwd == root {
        None
    } else if cwd.starts_with(root) {
        Some(cwd.to_path_buf())
    } else {
        None
    }
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
    async fn explicit_project_id_resolves_to_root() {
        let reg = registry_with(vec![project("missiond", "/Users/jin/Projects/missiond")]);
        let r = resolve_target_project_root(Some("missiond"), None, None, &reg)
            .await
            .unwrap();
        assert_eq!(r.project_id, "missiond");
        assert_eq!(
            r.project_root,
            PathBuf::from("/Users/jin/Projects/missiond")
        );
        assert!(r.requested_cwd.is_none());
        assert_eq!(r.source, ResolutionSource::ExplicitProjectId);
    }

    #[tokio::test]
    async fn cwd_subdir_resolves_to_root_and_preserves_requested() {
        let reg = registry_with(vec![project("missiond", "/Users/jin/Projects/missiond")]);
        let cwd = PathBuf::from("/Users/jin/Projects/missiond/crates/missiond-core");
        let r = resolve_target_project_root(None, Some(&cwd), None, &reg)
            .await
            .unwrap();
        assert_eq!(r.project_id, "missiond");
        assert_eq!(
            r.project_root,
            PathBuf::from("/Users/jin/Projects/missiond")
        );
        assert_eq!(r.requested_cwd, Some(cwd));
        assert_eq!(r.source, ResolutionSource::CwdLongestPrefix);
    }

    #[tokio::test]
    async fn cwd_at_root_has_no_requested_metadata() {
        let reg = registry_with(vec![project("missiond", "/Users/jin/Projects/missiond")]);
        let cwd = PathBuf::from("/Users/jin/Projects/missiond");
        let r = resolve_target_project_root(None, Some(&cwd), None, &reg)
            .await
            .unwrap();
        assert!(r.requested_cwd.is_none());
        assert_eq!(r.project_root, cwd);
    }

    #[tokio::test]
    async fn unresolved_cwd_errors() {
        let reg = registry_with(vec![project("missiond", "/Users/jin/Projects/missiond")]);
        let cwd = PathBuf::from("/Users/jin/Projects");
        let err = resolve_target_project_root(None, Some(&cwd), None, &reg)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ResolutionError::CwdOutsideRegisteredProject { .. }
        ));
    }

    #[tokio::test]
    async fn no_signal_errors() {
        let reg = registry_with(vec![project("missiond", "/Users/jin/Projects/missiond")]);
        let err = resolve_target_project_root(None, None, None, &reg)
            .await
            .unwrap_err();
        assert!(matches!(err, ResolutionError::NoSignal));
    }

    #[tokio::test]
    async fn unknown_project_id_errors() {
        let reg = registry_with(vec![project("missiond", "/Users/jin/Projects/missiond")]);
        let err = resolve_target_project_root(Some("nonexistent"), None, None, &reg)
            .await
            .unwrap_err();
        assert!(matches!(err, ResolutionError::UnknownProjectId(_)));
    }

    #[tokio::test]
    async fn fallback_project_id_used_when_no_explicit() {
        let reg = registry_with(vec![project("missiond", "/Users/jin/Projects/missiond")]);
        let r = resolve_target_project_root(None, None, Some("missiond"), &reg)
            .await
            .unwrap();
        assert_eq!(r.source, ResolutionSource::FallbackProjectId);
        assert_eq!(
            r.project_root,
            PathBuf::from("/Users/jin/Projects/missiond")
        );
    }
}
