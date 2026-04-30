use anyhow::{anyhow, Result};
use missiond_core::types::ProjectConfig;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::state::AppState;

#[derive(Deserialize)]
struct SetActiveArgs {
    id: String,
    #[serde(default = "default_true")]
    active: bool,
}

fn default_true() -> bool {
    true
}

pub(super) async fn handle_list(state: &AppState) -> Result<ToolResult> {
    let projects = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let enriched: Vec<serde_json::Value> = projects
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or_default();
            let path = Path::new(&p.path);
            let lisps = scan_lisp_files(path);
            v["lispFiles"] = serde_json::json!(lisps);
            v["lispCount"] = serde_json::json!(lisps.len());
            v
        })
        .collect();
    Ok(ToolResult::json_pretty(&enriched))
}

pub(super) async fn handle_get(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = required_str(&args, "id")?;
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

pub(super) async fn handle_set_active(state: &AppState, args: Value) -> Result<ToolResult> {
    let a: SetActiveArgs = serde_json::from_value(args)?;
    let updated = state
        .store
        .set_project_active(&a.id, a.active)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if updated {
        Ok(ToolResult::text(format!(
            "Project {} active={}",
            a.id, a.active
        )))
    } else {
        Ok(ToolResult::error(format!("Project not found: {}", a.id)))
    }
}

pub(super) async fn handle_sync(state: &AppState) -> Result<ToolResult> {
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

        let config = ProjectConfig {
            id: project_id,
            path: real_path.clone(),
            intent_path: None,
            active: true,
            slots: vec![],
            github_url: github_url_for_path(&real_path),
            kind: "managed".to_string(),
            vault_path: None,
            parent_id: None,
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

pub(super) async fn handle_init(state: &AppState, args: Value) -> Result<ToolResult> {
    let path = required_str(&args, "path")?;
    let path = Path::new(path)
        .canonicalize()
        .map_err(|e| anyhow!("Invalid path: {}", e))?;
    let path_str = path.display().to_string();

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

    let github_url = github_url_for_path(&path);
    let intent_path = discover_intent_path(&path);
    let slots: Vec<String> = args
        .get("slots")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let config = ProjectConfig {
        id: id.clone(),
        path: path_str.clone(),
        intent_path: intent_path.clone(),
        active: true,
        slots,
        github_url: github_url.clone(),
        kind: "managed".to_string(),
        vault_path: None,
        parent_id: None,
        created_at: None,
        updated_at: None,
    };
    state
        .store
        .upsert_project(&config)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let backfilled = state
        .store
        .backfill_project_id(&id, &format!("{}%", path_str))
        .await
        .unwrap_or(0);

    let claude_pattern = path_str.replace('/', "-");
    let backfilled2 = state
        .store
        .backfill_project_id(&id, &format!("%{}%", claude_pattern))
        .await
        .unwrap_or(0);

    reload_project_registry(state).await;

    Ok(ToolResult::json(&serde_json::json!({
        "id": id,
        "path": path_str,
        "githubUrl": github_url,
        "intentPath": intent_path,
        "backfilledConversations": backfilled + backfilled2,
        "status": "registered"
    })))
}

pub(super) async fn handle_import_universe(state: &AppState, args: Value) -> Result<ToolResult> {
    let manifest_path = args
        .get("manifest")
        .and_then(|v| v.as_str())
        .unwrap_or("~/Projects/universe.intent.lisp");
    let manifest_path = manifest_path.replace(
        "~",
        &dirs::home_dir().unwrap_or_default().display().to_string(),
    );
    let manifest_path = Path::new(&manifest_path);

    if !manifest_path.exists() {
        return Ok(ToolResult::error(format!(
            "Manifest not found: {}",
            manifest_path.display()
        )));
    }
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| anyhow!("Failed to read manifest: {}", e))?;

    let base_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut imported = 0u32;
    let mut updated = 0u32;
    let mut reference_synced = 0u32;
    let mut current_monorepo: Option<(String, String)> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("(monorepo ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == ":path" {
                let mono_id = parts[0].to_string();
                let mono_path = parts[2].trim_matches('"').to_string();
                let resolved = base_dir
                    .join(&mono_path)
                    .canonicalize()
                    .unwrap_or_else(|_| base_dir.join(&mono_path));
                current_monorepo = Some((mono_id, resolved.display().to_string()));
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("(service ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let service_id = parts[0].to_lowercase();

            let (service_path, parent_id) = if parts.len() >= 3 && parts[1] == ":subpath" {
                if let Some((ref mono_id, ref mono_path)) = current_monorepo {
                    let subpath = parts[2].trim_matches(|c| c == '"' || c == ')');
                    let resolved = Path::new(mono_path).join(subpath);
                    (resolved.display().to_string(), Some(mono_id.clone()))
                } else {
                    continue;
                }
            } else if parts.len() >= 3 && parts[1] == ":path" {
                let path = parts[2].trim_matches(|c| c == '"' || c == ')');
                let resolved = base_dir
                    .join(path)
                    .canonicalize()
                    .unwrap_or_else(|_| base_dir.join(path));
                current_monorepo = None;
                (resolved.display().to_string(), None)
            } else {
                continue;
            };

            let is_reference = service_path.contains("reference") || service_id.ends_with("-ref");
            let kind = if is_reference { "reference" } else { "managed" };
            let sp = Path::new(&service_path);
            let intent_path = discover_intent_path(sp);
            let github_url = github_url_for_path(sp);
            let existed = state
                .store
                .get_project(&service_id)
                .await
                .map(|p| p.is_some())
                .unwrap_or(false);

            let config = ProjectConfig {
                id: service_id.clone(),
                path: service_path,
                intent_path,
                active: true,
                slots: vec![],
                github_url,
                kind: kind.to_string(),
                vault_path: None,
                parent_id,
                created_at: None,
                updated_at: None,
            };
            let _ = state.store.upsert_project(&config).await;

            if existed {
                updated += 1;
            } else {
                imported += 1;
            }

            if is_reference {
                reference_synced += 1;
            }
        }
    }

    reload_project_registry(state).await;

    Ok(ToolResult::json(&serde_json::json!({
        "imported": imported,
        "updated": updated,
        "reference_noted": reference_synced,
        "manifest": manifest_path.display().to_string(),
    })))
}

pub(super) fn discover_intent_path(path: &Path) -> Option<String> {
    [
        ".missiond/intent.lisp",
        ".jarvis/intent.lisp",
        "intent.lisp",
    ]
    .iter()
    .find(|p| path.join(p).exists())
    .map(|p| p.to_string())
}

async fn reload_project_registry(state: &AppState) {
    if let Ok(projects) = state.store.list_projects().await {
        let mut reg = state.project_registry.write().await;
        *reg = missiond_core::types::ProjectRegistry::new(projects);
    }
}

fn scan_lisp_files(path: &Path) -> Vec<String> {
    let mut lisps = Vec::new();
    for depth_dirs in &[vec![path.to_path_buf()], immediate_dirs(path)] {
        for dir in depth_dirs {
            scan_lisp_files_one_level(path, dir, &mut lisps);
        }
    }
    lisps.sort();
    lisps.dedup();
    lisps
}

fn immediate_dirs(path: &Path) -> Vec<PathBuf> {
    path.read_dir()
        .ok()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn scan_lisp_files_one_level(root: &Path, dir: &Path, lisps: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let ep = entry.path();
            if ep.extension().map(|e| e == "lisp").unwrap_or(false) {
                let rel = ep.strip_prefix(root).unwrap_or(&ep);
                lisps.push(rel.display().to_string());
            }
            if ep.is_dir() {
                if let Ok(rd2) = std::fs::read_dir(&ep) {
                    for e2 in rd2.filter_map(|e| e.ok()) {
                        let p2 = e2.path();
                        if p2.extension().map(|e| e == "lisp").unwrap_or(false) {
                            let rel = p2.strip_prefix(root).unwrap_or(&p2);
                            lisps.push(rel.display().to_string());
                        }
                    }
                }
            }
        }
    }
}

fn github_url_for_path(path: impl AsRef<Path>) -> Option<String> {
    std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
        .ok()
        .and_then(|out| {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if url.is_empty() {
                None
            } else {
                Some(url)
            }
        })
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("{} is required", key))
}
