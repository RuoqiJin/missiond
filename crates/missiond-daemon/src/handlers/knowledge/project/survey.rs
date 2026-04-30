use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle_survey(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = required_str(&args, "id")?;
    let project = state
        .store
        .get_project(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow!("Project not found: {}", id))?;

    let level = args.get("level").and_then(|v| v.as_str()).unwrap_or("L3");
    let check_only = args.get("check").and_then(|v| v.as_bool()).unwrap_or(false);
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let project_path = &project.path;
    let mut cmd = std::process::Command::new("forge");
    cmd.arg("survey").arg(project_path);
    cmd.arg("--level").arg(level);
    if check_only {
        cmd.arg("--check");
    }
    if dry_run {
        cmd.arg("--dry-run");
    }

    let output = cmd.output().map_err(|e| {
        anyhow!(
            "Failed to run forge survey: {} (is forge CLI installed?)",
            e
        )
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    if success && !check_only && !dry_run {
        let sp = std::path::Path::new(project_path);
        let new_intent = super::registry::discover_intent_path(sp);
        if new_intent.is_some() && new_intent != project.intent_path {
            let mut updated = project.clone();
            updated.intent_path = new_intent;
            let _ = state.store.upsert_project(&updated).await;
        }
    }

    if success {
        Ok(ToolResult::json(&serde_json::json!({
            "id": id,
            "level": level,
            "check_only": check_only,
            "dry_run": dry_run,
            "success": true,
            "output": truncate_chars(&stdout, 2000),
        })))
    } else {
        Ok(ToolResult::json(&serde_json::json!({
            "id": id,
            "success": false,
            "exit_code": output.status.code(),
            "stdout": truncate_chars(&stdout, 2000),
            "stderr": truncate_chars(&stderr, 1000),
        })))
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = 0;
    for (idx, _) in s.char_indices() {
        if idx > max {
            break;
        }
        end = idx;
    }
    format!("{}...(truncated)", &s[..end])
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("{} is required", key))
}
