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
            // Enrich with lisp file scan (local readdir, fast)
            let enriched: Vec<serde_json::Value> = projects
                .iter()
                .map(|p| {
                    let mut v = serde_json::to_value(p).unwrap_or_default();
                    let path = std::path::Path::new(&p.path);
                    let mut lisps = Vec::new();
                    for depth_dirs in &[
                        vec![path.to_path_buf()],
                        path.read_dir().ok().map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default(),
                    ] {
                        for dir in depth_dirs {
                            if let Ok(rd) = std::fs::read_dir(dir) {
                                for entry in rd.filter_map(|e| e.ok()) {
                                    let ep = entry.path();
                                    if ep.extension().map(|e| e == "lisp").unwrap_or(false) {
                                        let rel = ep.strip_prefix(path).unwrap_or(&ep);
                                        lisps.push(rel.display().to_string());
                                    }
                                    if ep.is_dir() {
                                        if let Ok(rd2) = std::fs::read_dir(&ep) {
                                            for e2 in rd2.filter_map(|e| e.ok()) {
                                                let p2 = e2.path();
                                                if p2.extension().map(|e| e == "lisp").unwrap_or(false) {
                                                    let rel = p2.strip_prefix(path).unwrap_or(&p2);
                                                    lisps.push(rel.display().to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    lisps.sort();
                    lisps.dedup();
                    v["lispFiles"] = serde_json::json!(lisps);
                    v["lispCount"] = serde_json::json!(lisps.len());
                    v
                })
                .collect();
            Ok(ToolResult::json_pretty(&enriched))
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
            let updated = state
                .store
                .set_project_active(&a.id, a.active)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            if updated {
                Ok(ToolResult::text(format!("Project {} active={}", a.id, a.active)))
            } else {
                Ok(ToolResult::error(format!("Project not found: {}", a.id)))
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
                let real_path = dir_name.replace('-', "/");
                let project_id = real_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&dir_name)
                    .to_lowercase();

                if project_id.is_empty() {
                    continue;
                }

                if let Ok(Some(_)) = state.store.get_project(&project_id).await {
                    skipped += 1;
                    continue;
                }

                // Resolve github URL from git remote (one-time, on sync)
                let github_url = std::process::Command::new("git")
                    .args(["remote", "get-url", "origin"])
                    .current_dir(&real_path)
                    .output()
                    .ok()
                    .and_then(|out| {
                        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if url.is_empty() { None } else { Some(url) }
                    });
                let config = missiond_core::types::ProjectConfig {
                    id: project_id,
                    path: real_path,
                    intent_path: None,
                    active: true,
                    slots: vec![],
                    github_url,
                    created_at: None,
                    updated_at: None,
                };
                let _ = state.store.upsert_project(&config).await;
                synced += 1;
            }

            Ok(ToolResult::json(&serde_json::json!({
                "synced": synced,
                "skipped": skipped,
                "source": claude_projects_dir.display().to_string(),
            })))
        }
        "init" => {
            // One-shot project registration: path → auto-discover everything
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("path is required"))?;
            let path = std::path::Path::new(path).canonicalize()
                .map_err(|e| anyhow!("Invalid path: {}", e))?;
            let path_str = path.display().to_string();

            // 1. Derive project_id from last path component
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase()
                });

            // 2. Resolve github URL from git remote
            let github_url = std::process::Command::new("git")
                .args(["remote", "get-url", "origin"])
                .current_dir(&path)
                .output()
                .ok()
                .and_then(|out| {
                    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if url.is_empty() { None } else { Some(url) }
                });

            // 3. Scan for intent.lisp (check common locations)
            let intent_path = [".missiond/intent.lisp", ".jarvis/intent.lisp", "intent.lisp"]
                .iter()
                .find(|p| path.join(p).exists())
                .map(|p| p.to_string());

            // 4. Bind slots from args (optional)
            let slots: Vec<String> = args
                .get("slots")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();

            // 5. Upsert project
            let config = missiond_core::types::ProjectConfig {
                id: id.clone(),
                path: path_str.clone(),
                intent_path: intent_path.clone(),
                active: true,
                slots,
                github_url: github_url.clone(),
                created_at: None,
                updated_at: None,
            };
            state.store.upsert_project(&config)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // 6. Backfill project_id for existing conversations matching this path
            let backfilled = state.store
                .backfill_project_id(&id, &format!("{}%", path_str))
                .await
                .unwrap_or(0);

            // Also match Claude Code project dir format
            let claude_pattern = path_str.replace('/', "-");
            let backfilled2 = state.store
                .backfill_project_id(&id, &format!("%{}%", claude_pattern))
                .await
                .unwrap_or(0);

            // 7. Reload project registry
            if let Ok(projects) = state.store.list_projects().await {
                let mut reg = state.project_registry.write().await;
                *reg = missiond_core::types::ProjectRegistry::new(projects);
            }

            Ok(ToolResult::json(&serde_json::json!({
                "id": id,
                "path": path_str,
                "githubUrl": github_url,
                "intentPath": intent_path,
                "backfilledConversations": backfilled + backfilled2,
                "status": "registered"
            })))
        }
        "context" => {
            let id = args.get("id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("id is required"))?;
            let project = state.store.get_project(id).await
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Project not found: {}", id))?;

            // 1. Intent summary — read file, extract metadata
            let intent = build_intent_summary(&project);

            // 2. GitHub — normalize git@ to https://
            let github = build_github_info(&project.github_url);

            // 3. Conversations — DB stats + recent
            let conv_stats = state.store.conversation_stats_by_project(id).await
                .unwrap_or_else(|_| serde_json::json!({}));
            let recent = state.store.recent_conversations_by_project(id, 10).await
                .unwrap_or_default();

            // 4. Memories — filesystem scan
            let memories = super::project_memory::list_memories(&project.path);
            let mem_index = super::project_memory::read_memory_index(&project.path);
            let mem_dir = super::project_memory::claude_memory_dir(&project.path);

            // 5. KB stats
            let kb = state.store.kb_stats_by_project(id).await
                .unwrap_or_else(|_| serde_json::json!({}));

            // 6. Slots — match project's configured slots against runtime state
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
        "memories" => {
            let id = args.get("id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("id is required"))?;
            let project = state.store.get_project(id).await
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Project not found: {}", id))?;

            let file = args.get("file").and_then(|v| v.as_str());

            if let Some(file_name) = file {
                match super::project_memory::read_memory(&project.path, file_name) {
                    Ok(content) => Ok(ToolResult::text(content)),
                    Err(e) => Ok(ToolResult::error(format!("Failed to read {}: {}", file_name, e))),
                }
            } else {
                let memories = super::project_memory::list_memories(&project.path);
                let index = super::project_memory::read_memory_index(&project.path);
                let dir = super::project_memory::claude_memory_dir(&project.path);
                Ok(ToolResult::json_pretty(&serde_json::json!({
                    "dir": dir.display().to_string(),
                    "count": memories.len(),
                    "index": index,
                    "entries": memories,
                })))
            }
        }
        _ => Ok(ToolResult::error(format!(
            "Unknown project action: {}. Use: list, get, set_active, sync, init, context, memories",
            action
        ))),
    }
}

/// Extract intent.lisp metadata without loading the full file.
fn build_intent_summary(project: &missiond_core::types::ProjectConfig) -> serde_json::Value {
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

/// Normalize GitHub URL: git@github.com:X/Y.git -> https://github.com/X/Y
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

/// Build slots info: configured slots vs runtime state.
async fn build_slots_info(
    project: &missiond_core::types::ProjectConfig,
    state: &AppState,
) -> serde_json::Value {
    let all_slots = state.mission.list_slots();
    let configured: Vec<&str> = project.slots.iter().map(|s| s.as_str()).collect();

    let mut active: Vec<serde_json::Value> = Vec::new();
    for s in all_slots.iter().filter(|s| configured.contains(&s.config.id.as_str())) {
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
