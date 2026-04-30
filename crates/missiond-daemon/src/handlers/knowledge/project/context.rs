use anyhow::{anyhow, Result};
use missiond_core::types::ProjectConfig;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::handlers::knowledge::project_memory;
use crate::state::AppState;

pub(super) async fn handle_context(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = required_str(&args, "id")?;
    let project = state
        .store
        .get_project(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow!("Project not found: {}", id))?;

    let intent = build_intent_summary(&project);
    let github = build_github_info(&project.github_url);
    let conv_stats = state
        .store
        .conversation_stats_by_project(id)
        .await
        .unwrap_or_else(|_| serde_json::json!({}));
    let recent = state
        .store
        .recent_conversations_by_project(id, 10)
        .await
        .unwrap_or_default();
    let memories = project_memory::list_memories(&project.path);
    let mem_index = project_memory::read_memory_index(&project.path);
    let mem_dir = project_memory::claude_memory_dir(&project.path);
    let kb = state
        .store
        .kb_stats_by_project(id)
        .await
        .unwrap_or_else(|_| serde_json::json!({}));
    let slots_info = build_slots_info(&project, state).await;

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "project": project,
        "intent": intent,
        "github": github,
        "conversations": {
            "stats": conv_stats,
            "recent": recent,
        },
        "memories": {
            "dir": mem_dir.display().to_string(),
            "count": memories.len(),
            "index": mem_index,
            "entries": memories,
        },
        "kb": kb,
        "slots": slots_info,
    })))
}

pub(super) async fn handle_memories(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = required_str(&args, "id")?;
    let project = state
        .store
        .get_project(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow!("Project not found: {}", id))?;

    let file = args.get("file").and_then(|v| v.as_str());

    if let Some(file_name) = file {
        match project_memory::read_memory(&project.path, file_name) {
            Ok(content) => Ok(ToolResult::text(content)),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to read {}: {}",
                file_name, e
            ))),
        }
    } else {
        let memories = project_memory::list_memories(&project.path);
        let index = project_memory::read_memory_index(&project.path);
        let dir = project_memory::claude_memory_dir(&project.path);
        Ok(ToolResult::json_pretty(&serde_json::json!({
            "dir": dir.display().to_string(),
            "count": memories.len(),
            "index": index,
            "entries": memories,
        })))
    }
}

fn build_intent_summary(project: &ProjectConfig) -> serde_json::Value {
    let intent_path = match &project.intent_path {
        Some(rel) => std::path::Path::new(&project.path).join(rel),
        None => return serde_json::json!({"exists": false}),
    };

    let metadata = match std::fs::metadata(&intent_path) {
        Ok(m) => m,
        Err(_) => return serde_json::json!({"path": project.intent_path, "exists": false}),
    };

    let content = std::fs::read_to_string(&intent_path).unwrap_or_default();
    let mut survey_date = None;
    let mut last_updated = None;
    let mut pillars = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("(survey-date ") {
            survey_date = Some(rest.trim_matches(|c| c == '"' || c == ')').to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("(last-updated ") {
            last_updated = Some(rest.trim_matches(|c| c == '"' || c == ')').to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("(pillar ") {
            if let Some(name) = rest.split_whitespace().next() {
                pillars.push(name.to_string());
            }
        }
    }

    serde_json::json!({
        "path": project.intent_path,
        "exists": true,
        "size_bytes": metadata.len(),
        "pillars": pillars,
        "survey_date": survey_date,
        "last_updated": last_updated,
    })
}

fn build_github_info(github_url: &Option<String>) -> serde_json::Value {
    match github_url {
        None => serde_json::json!(null),
        Some(url) => {
            let web_url = if url.starts_with("git@github.com:") {
                let path = url.strip_prefix("git@github.com:").unwrap_or(url);
                let path = path.strip_suffix(".git").unwrap_or(path);
                Some(format!("https://github.com/{}", path))
            } else if url.starts_with("https://") {
                Some(url.strip_suffix(".git").unwrap_or(url).to_string())
            } else {
                None
            };
            serde_json::json!({
                "url": url,
                "web_url": web_url,
            })
        }
    }
}

async fn build_slots_info(project: &ProjectConfig, state: &AppState) -> serde_json::Value {
    let all_slots = state.mission.list_slots();
    let configured: Vec<&str> = project.slots.iter().map(|s| s.as_str()).collect();

    let mut active: Vec<serde_json::Value> = Vec::new();
    for s in all_slots
        .iter()
        .filter(|s| configured.contains(&s.config.id.as_str()))
    {
        let pty_status = state.pty.get_status(&s.config.id).await;
        active.push(serde_json::json!({
            "id": s.config.id,
            "session_id": s.session_id,
            "status": pty_status.map(|info| format!("{:?}", info.state)),
        }));
    }

    serde_json::json!({
        "configured": configured,
        "active": active,
    })
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("{} is required", key))
}
