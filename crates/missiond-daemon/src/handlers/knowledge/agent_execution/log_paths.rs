use crate::state::AppState;
use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) const COMPANION_DIR: &str = ".missiond/v2";

pub(super) async fn resolve_project_root(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<PathBuf> {
    if let Some(id) = project_id {
        if let Some(p) = state.project_registry.read().await.get(id) {
            return Ok(PathBuf::from(&p.path));
        }
        return Err(anyhow!(
            "project '{}' not registered; run mission_project(action=\"list\") to see available ids",
            id
        ));
    }
    let cwd = std::env::current_dir().map_err(|e| anyhow!("cannot read CWD: {}", e))?;
    Ok(cwd)
}

pub(super) fn companion_path(root: &Path, execution_id: &str) -> PathBuf {
    let mut p = root.join(COMPANION_DIR);
    let mut name = execution_id.to_string();
    if !name.ends_with(".lisp") {
        name.push_str(".lisp");
    }
    p.push(name);
    p
}

/// Canonical `project` field accessor. Kept so the alias resolver below has
/// one source of truth for the strict field name.
#[allow(dead_code)]
fn project_arg(args: &Value) -> Option<&str> {
    args.get("project").and_then(|v| v.as_str())
}

/// Resolve the active project id from either the canonical `project` field or
/// the workstation-dispatch alias `target_project`. `project` always wins when
/// both are present so existing callers stay deterministic.
pub(super) fn project_or_target_project(args: &Value) -> Option<&str> {
    args.get("project")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("target_project").and_then(|v| v.as_str()))
}

pub(super) fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        ToolResult::structured_error(ToolError::new(
            error_codes::MISSING_PARAM,
            format!("missing required param `{}`", key),
        ))
    })
}
