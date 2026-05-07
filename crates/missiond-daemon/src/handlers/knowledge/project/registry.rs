use anyhow::{anyhow, Result};
use missiond_core::types::ProjectConfig;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::context::v3_blueprint_runtime::{
    load_compiled_project_universe, CompiledProjectUniverseEntry, ProjectRegistryRuntimeConfig,
};
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
    let runtime_config = match ProjectRegistryRuntimeConfig::load_for_current_dir() {
        Ok(config) => config,
        Err(err) => {
            return Ok(ToolResult::error(format!(
                "V3_BLUEPRINT_CONFIG_ERROR: {err}"
            )));
        }
    };
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
    let intent_path = discover_intent_path(&path, &runtime_config);
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
    archive_inactive_path_aliases(state, &id, &path_str).await?;
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

async fn archive_inactive_path_aliases(
    state: &AppState,
    target_id: &str,
    target_path: &str,
) -> Result<()> {
    let projects = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    for mut project in projects {
        if project.id == target_id || project.path != target_path {
            continue;
        }
        if project.active {
            return Err(anyhow!(
                "Project path {} is already owned by active project {}",
                target_path,
                project.id
            ));
        }

        let archive_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".missiond")
            .join("project-aliases")
            .join(&project.id);
        let _ = std::fs::create_dir_all(&archive_path);
        project.path = archive_path.display().to_string();
        state
            .store
            .upsert_project(&project)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
    }

    Ok(())
}

pub(super) async fn handle_import_universe(state: &AppState, args: Value) -> Result<ToolResult> {
    let runtime_config = match ProjectRegistryRuntimeConfig::load_for_current_dir() {
        Ok(config) => config,
        Err(err) => {
            return Ok(ToolResult::error(format!(
                "V3_BLUEPRINT_CONFIG_ERROR: {err}"
            )));
        }
    };
    let explicit_manifest = args.get("manifest").and_then(|v| v.as_str()).is_some();
    if !explicit_manifest {
        if let Some(result) = import_compiled_universe(state, &runtime_config).await? {
            return Ok(ToolResult::json(&result));
        }
    }

    let manifest_path = args
        .get("manifest")
        .and_then(|v| v.as_str())
        .map(expand_tilde_path)
        .unwrap_or_else(|| {
            expand_tilde_path(
                &runtime_config
                    .env_or_default_universe_manifest()
                    .to_string_lossy(),
            )
        });

    if !manifest_path.exists() {
        return Ok(ToolResult::error(format!(
            "Manifest not found: {}",
            manifest_path.display()
        )));
    }
    let content = std::fs::read_to_string(&manifest_path)
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
            let intent_path = discover_intent_path(sp, &runtime_config);
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
        "manifestFallback": true,
    })))
}

async fn import_compiled_universe(
    state: &AppState,
    runtime_config: &ProjectRegistryRuntimeConfig,
) -> Result<Option<serde_json::Value>> {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let compiled = load_compiled_project_universe(&project_root, None);
    let Some(payload) = compiled.payload else {
        return Ok(None);
    };

    let mut imported = 0u32;
    let mut updated = 0u32;
    let mut skipped = 0u32;
    let mut imported_ids = Vec::new();

    for entry in payload.projects {
        let Some(config) = compiled_project_to_config(&project_root, &entry, runtime_config) else {
            skipped += 1;
            continue;
        };
        let existed = state
            .store
            .get_project(&config.id)
            .await
            .map(|p| p.is_some())
            .unwrap_or(false);

        state
            .store
            .upsert_project(&config)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;

        if existed {
            updated += 1;
        } else {
            imported += 1;
        }
        imported_ids.push(config.id);
    }

    reload_project_registry(state).await;

    Ok(Some(serde_json::json!({
        "source": "compiled-project-universe",
        "schema": "missiond.project-import.compiled-universe.v1",
        "imported": imported,
        "updated": updated,
        "skipped": skipped,
        "reference_noted": 0,
        "manifestFallback": false,
        "importedIds": imported_ids,
        "compiledRuntime": {
            "snapshot": compiled.snapshot.as_ref().map(|snapshot| serde_json::json!({
                "kind": snapshot.kind,
                "path": snapshot.path.display().to_string(),
                "schemaVersion": snapshot.schema_version,
                "sourceHash": snapshot.source_hash,
            })),
            "diagnostics": compiled.diagnostics,
        },
    })))
}

fn compiled_project_to_config(
    missiond_root: &Path,
    entry: &CompiledProjectUniverseEntry,
    runtime_config: &ProjectRegistryRuntimeConfig,
) -> Option<ProjectConfig> {
    let id = entry.id.as_ref()?.to_lowercase();
    let raw_root = entry.root.as_deref().or_else(|| {
        entry
            .path
            .as_deref()
            .filter(|path| !path.ends_with(".lisp"))
    })?;
    let expanded = expand_tilde_path(raw_root);
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        missiond_root.join(expanded)
    };
    let path = resolved
        .canonicalize()
        .unwrap_or_else(|_| resolved.clone())
        .display()
        .to_string();
    let root = Path::new(&path);
    let intent_path = entry
        .intent
        .clone()
        .or_else(|| discover_intent_path(root, runtime_config));
    let github_url = github_url_for_path(root);

    Some(ProjectConfig {
        id,
        path,
        intent_path,
        active: entry.status.as_deref() != Some("retired"),
        slots: vec![],
        github_url,
        kind: entry.kind.clone().unwrap_or_else(|| "managed".to_string()),
        vault_path: None,
        parent_id: None,
        created_at: None,
        updated_at: None,
    })
}

pub(super) fn discover_intent_path(
    path: &Path,
    config: &ProjectRegistryRuntimeConfig,
) -> Option<String> {
    config
        .intent_path_candidates
        .iter()
        .find(|p| path.join(p).exists())
        .map(|p| p.to_string())
}

fn expand_tilde_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    PathBuf::from(raw)
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
