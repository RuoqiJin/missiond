use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::state::AppState;

/// Resolve a project ID to its filesystem root by consulting the project registry.
async fn resolve_project_root(state: &AppState, project_id: &str) -> Option<String> {
    let reg = state.project_registry.read().await;
    reg.get(project_id).map(|p| p.path.clone())
}

fn forge_binary() -> String {
    std::env::var("FORGE_BIN").unwrap_or_else(|_| "forge".to_string())
}

pub(crate) async fn handle_build(state: &AppState, args: Value) -> Result<ToolResult> {
    let project_id = args
        .get("project")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("'project' is required"))?
        .to_string();

    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let output_dir = args
        .get("output_dir")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let project_root = match resolve_project_root(state, &project_id).await {
        Some(p) => p,
        None => {
            return Ok(ToolResult::json_pretty(&json!({
                "status": "error",
                "error": format!("project not registered: {}", project_id)
            })));
        }
    };

    let bin = forge_binary();
    let mut cmd = Command::new(&bin);
    cmd.arg("build").arg(&project_root);
    if dry_run {
        cmd.arg("--dry-run");
    }
    if let Some(ref dir) = output_dir {
        cmd.arg("--output-dir").arg(dir);
    }

    // Build a human-readable command string for debug output.
    let mut command_str = format!("{} build {}", bin, project_root);
    if dry_run {
        command_str.push_str(" --dry-run");
    }
    if let Some(ref dir) = output_dir {
        command_str.push_str(&format!(" --output-dir {}", dir));
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| anyhow!("failed to spawn forge: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(-1);
    let status = if output.status.success() { "ok" } else { "error" };

    Ok(ToolResult::json_pretty(&json!({
        "status": status,
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "project_id": project_id,
        "project_root": project_root,
        "command": command_str,
    })))
}

pub(crate) async fn handle_lint(state: &AppState, args: Value) -> Result<ToolResult> {
    let project_id = args
        .get("project")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("'project' is required"))?
        .to_string();

    let project_root = match resolve_project_root(state, &project_id).await {
        Some(p) => p,
        None => {
            return Ok(ToolResult::json_pretty(&json!({
                "status": "error",
                "error": format!("project not registered: {}", project_id)
            })));
        }
    };

    let bin = forge_binary();
    let command_str = format!("{} lint {}", bin, project_root);

    let output = Command::new(&bin)
        .arg("lint")
        .arg(&project_root)
        .output()
        .await
        .map_err(|e| anyhow!("failed to spawn forge: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(-1);
    let status = if output.status.success() { "ok" } else { "error" };

    Ok(ToolResult::json_pretty(&json!({
        "status": status,
        "exit_code": exit_code,
        "stdout": stdout.clone(),
        "stderr": stderr,
        "violations_raw": stdout,
        "project_id": project_id,
        "project_root": project_root,
        "command": command_str,
    })))
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_forge_build" => handle_build(state, args).await,
        "mission_forge_lint" => handle_lint(state, args).await,
        other => Ok(ToolResult::error(format!("Unknown forge tool: {}", other))),
    }
}
