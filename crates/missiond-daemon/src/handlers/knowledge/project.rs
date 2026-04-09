use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

#[derive(Deserialize)]
struct SetActiveArgs {
    id: String,
    #[serde(default = "default_true")]
    active: bool,
}
fn default_true() -> bool { true }

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");

    match action {
        "list" => {
            let projects = state
                .store
                .list_projects()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&projects))
        }
        "get" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("id is required"))?;
            let project = state
                .store
                .get_project(id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            match project {
                Some(p) => Ok(ToolResult::json_pretty(&p)),
                None => Ok(ToolResult::error(format!("Project not found: {}", id))),
            }
        }
        "set_active" => {
            let a: SetActiveArgs = serde_json::from_value(args)?;
            let project = state
                .store
                .set_project_active(&a.id, a.active)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            match project {
                Some(p) => Ok(ToolResult::json_pretty(&p)),
                None => Ok(ToolResult::error(format!("Project not found: {}", a.id))),
            }
        }
        "sync" => {
            let claude_projects_dir = dirs::home_dir()
                .unwrap_or_default()
                .join(".claude")
                .join("projects");
            if !claude_projects_dir.exists() {
                return Ok(ToolResult::text("~/.claude/projects/ directory not found"));
            }

            let mut synced = 0u32;
            let mut skipped = 0u32;
            let entries = std::fs::read_dir(&claude_projects_dir)
                .map_err(|e| anyhow!("Failed to read ~/.claude/projects/: {}", e))?;

            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    continue;
                }
                let dir_name = entry.file_name().to_string_lossy().to_string();
                // Convert directory name format: -Users-jinchen-Projects-XXX → <PROJECTS_ROOT>/XXX
                let real_path = dir_name.replace('-', "/");
                // Extract the last path component as project_id
                let project_id = real_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&dir_name)
                    .to_lowercase();

                if project_id.is_empty() {
                    continue;
                }

                // Check if already exists
                if let Ok(Some(_)) = state.store.get_project(&project_id).await {
                    skipped += 1;
                    continue;
                }

                let _ = state
                    .store
                    .upsert_project(&project_id, &project_id, &real_path)
                    .await;
                synced += 1;
            }

            Ok(ToolResult::json(&serde_json::json!({
                "synced": synced,
                "skipped": skipped,
                "source": claude_projects_dir.display().to_string(),
            })))
        }
        _ => Ok(ToolResult::error(format!(
            "Unknown project action: {}. Use: list, get, set_active, sync",
            action
        ))),
    }
}
