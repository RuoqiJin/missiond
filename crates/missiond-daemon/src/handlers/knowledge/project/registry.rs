use anyhow::{anyhow, Result};
use missiond_core::types::ProjectConfig;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::context::v3_blueprint_runtime::{
    load_compiled_deployment_policy_snapshot, load_compiled_project_universe,
    CompiledProjectUniverseEntry, CompiledRuntimeSnapshot, CompiledServiceRuntimeEntry,
    ProjectRegistryRuntimeConfig,
};
use crate::state::AppState;

#[derive(Deserialize)]
struct SetActiveArgs {
    id: String,
    #[serde(default = "default_true")]
    active: bool,
}

#[derive(Deserialize)]
struct ResolveArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include_unregistered_candidates: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct StatusArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Clone)]
struct ResolutionCandidate {
    project_id: String,
    score: i32,
    match_kind: String,
    project: Value,
    evidence: Vec<Value>,
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
        Some(p) => {
            let mut value = db_project_to_value(&p);
            enrich_project_status_value(&mut value);
            Ok(ToolResult::json_pretty(&value))
        }
        None => match compiled_project_lookup(id) {
            Some(mut value) => {
                enrich_project_status_value(&mut value);
                Ok(ToolResult::json_pretty(&value))
            }
            None => Ok(ToolResult::error(format!("Project not found: {}", id))),
        },
    }
}

pub(super) async fn handle_status(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: StatusArgs = serde_json::from_value(args)?;
    let query = first_non_empty([
        args.query.as_deref(),
        args.id.as_deref(),
        args.path.as_deref(),
        args.cwd.as_deref(),
    ]);
    let projects = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut diagnostics = Vec::new();
    let compiled = load_resolution_universe(&mut diagnostics);
    let runtime_status = compiled_runtime_status_from_projection(&compiled.runtime);
    if runtime_status
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "compiled_runtime_stale")
    {
        diagnostics.push(serde_json::json!({
            "kind": "compiled_runtime_stale",
            "source": "active-release-manifest",
            "staleProjections": runtime_status
                .get("staleProjections")
                .cloned()
                .unwrap_or(Value::Null),
            "recovery": "Run scripts/deploy-daemon.sh after regenerating compiled runtime projections, or restart the daemon on the active release that owns these hashes."
        }));
    }

    let target = query.as_deref().and_then(|query| {
        resolve_status_target(
            query,
            args.id.as_deref(),
            args.path.as_deref(),
            args.cwd.as_deref(),
            &projects,
            &compiled,
        )
    });
    let target_status = target
        .as_ref()
        .map(|target| target.status.as_str())
        .unwrap_or("not_requested");
    let status = if target_status == "not_found"
        && runtime_status
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "compiled_runtime_stale")
    {
        "compiled_runtime_stale"
    } else {
        target_status
    };

    let production_release = production_release_projection(
        target
            .as_ref()
            .and_then(|target| target.project_id.as_deref()),
        &runtime_status,
    );

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "ok": matches!(status, "not_requested" | "resolved" | "ambiguous"),
        "schema": "missiond.project-status.v1",
        "status": status,
        "query": query,
        "matched_project_id": target.as_ref().and_then(|target| target.project_id.clone()),
        "matched_project": target.as_ref().map(|target| target.project.clone()).unwrap_or(Value::Null),
        "candidate_projects": target.as_ref().map(|target| target.candidates.clone()).unwrap_or_default(),
        "compiledRuntime": compiled.runtime,
        "runtime_status": runtime_status,
        "activeRelease": missiond_active_release_status(),
        "productionRelease": production_release,
        "diagnostics": diagnostics,
    })))
}

pub(super) async fn handle_resolve(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: ResolveArgs = serde_json::from_value(args)?;
    let query = first_non_empty([
        args.query.as_deref(),
        args.id.as_deref(),
        args.path.as_deref(),
        args.cwd.as_deref(),
    ])
    .unwrap_or_default();
    if query.is_empty() {
        return Ok(ToolResult::error(
            "mission_project resolve requires query, id, path, or cwd",
        ));
    }

    let projects = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let limit = args.limit.unwrap_or(8).clamp(1, 25);
    let lookup = LookupInput::new(
        &query,
        args.id.as_deref(),
        args.path.as_deref(),
        args.cwd.as_deref(),
    );
    let mut diagnostics = Vec::new();
    let mut candidates: HashMap<String, ResolutionCandidate> = HashMap::new();
    let mut known_project_ids: HashSet<String> =
        projects.iter().map(|p| normalize_key(&p.id)).collect();

    for project in &projects {
        match_db_project(&lookup, project, &mut candidates);
    }

    let compiled = load_resolution_universe(&mut diagnostics);
    for project in &compiled.projects {
        if let Some(id) = project.id.as_deref() {
            known_project_ids.insert(normalize_key(id));
        }
    }
    for project in &compiled.projects {
        match_compiled_project(&lookup, project, &projects, &mut candidates);
    }
    for service in &compiled.services {
        match_compiled_service(
            &lookup,
            service,
            &projects,
            &known_project_ids,
            &mut candidates,
        );
    }

    let mut candidate_values = candidates
        .into_values()
        .map(candidate_to_value)
        .collect::<Vec<_>>();
    candidate_values.sort_by(|a, b| {
        let left = a.get("score").and_then(Value::as_i64).unwrap_or(0);
        let right = b.get("score").and_then(Value::as_i64).unwrap_or(0);
        right.cmp(&left)
    });
    candidate_values.truncate(limit);

    let top_score = candidate_values
        .first()
        .and_then(|v| v.get("score"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let second_score = candidate_values
        .get(1)
        .and_then(|v| v.get("score"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let domain_queries = lookup.domain_values();
    let include_unregistered = args.include_unregistered_candidates.unwrap_or(true);
    let candidate_roots = if include_unregistered && candidate_values.is_empty() {
        discover_candidate_roots(&lookup)
    } else {
        Vec::new()
    };
    let registration_proposal = if include_unregistered && candidate_values.is_empty() {
        build_registration_proposal(&lookup, &candidate_roots)
    } else {
        Value::Null
    };
    let runtime_status = compiled_runtime_status_from_projection(&compiled.runtime);
    let runtime_stale = runtime_status
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "compiled_runtime_stale");
    if runtime_stale {
        diagnostics.push(serde_json::json!({
            "kind": "compiled_runtime_stale",
            "source": "active-release-manifest",
            "staleProjections": runtime_status
                .get("staleProjections")
                .cloned()
                .unwrap_or(Value::Null),
            "recovery": "Regenerate compiled projections and redeploy/restart MissionD so the daemon active release and compiled runtime hashes match."
        }));
    }
    let status = if candidate_values.is_empty() {
        if runtime_stale {
            "compiled_runtime_stale"
        } else if !domain_queries.is_empty() || registration_proposal.is_object() {
            "unregistered_candidate"
        } else if !diagnostics.is_empty() {
            "stale_runtime"
        } else {
            "not_found"
        }
    } else if candidate_values.len() > 1 && top_score - second_score < 8 {
        "ambiguous"
    } else {
        "resolved"
    };
    let matched_project = if status == "resolved" {
        candidate_values
            .first()
            .and_then(|candidate| candidate.get("project"))
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let matched_project_id = matched_project
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let next_actions = project_resolution_next_actions(
        status,
        matched_project_id.as_deref(),
        &registration_proposal,
    );
    let lookup_values = lookup.values.clone();

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "ok": status == "resolved" || status == "ambiguous" || status == "unregistered_candidate",
        "schema": "missiond.project-resolution.v1",
        "status": status,
        "query": query,
        "normalized": {
            "lookup_values": lookup_values,
            "domains": domain_queries,
            "cwd": args.cwd,
            "path": args.path,
        },
        "matched_project_id": matched_project_id,
        "matched_project": matched_project,
        "candidate_projects": candidate_values,
        "candidate_roots": candidate_roots,
        "registration_proposal": registration_proposal,
        "compiledRuntime": compiled.runtime,
        "runtime_status": runtime_status,
        "activeRelease": missiond_active_release_status(),
        "diagnostics": diagnostics,
        "next_actions": next_actions,
    })))
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
    let mut path_conflicts = Vec::new();

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

        if let Err(err) = archive_inactive_path_aliases(state, &config.id, &config.path).await {
            skipped += 1;
            path_conflicts.push(serde_json::json!({
                "id": config.id,
                "path": config.path,
                "error": err.to_string(),
            }));
            continue;
        }

        if let Err(err) = state.store.upsert_project(&config).await {
            skipped += 1;
            path_conflicts.push(serde_json::json!({
                "id": config.id,
                "path": config.path,
                "error": format!("DB error: {err}"),
            }));
            continue;
        }

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
        "pathConflicts": path_conflicts,
        "manifestFallback": false,
        "importedIds": imported_ids,
        "compiledRuntime": {
            "snapshot": compiled.snapshot.as_ref().map(|snapshot| serde_json::json!({
                "kind": snapshot.kind.clone(),
                "path": snapshot.path.display().to_string(),
                "schemaVersion": snapshot.schema_version.clone(),
                "sourceHash": snapshot.source_hash.clone(),
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

struct ResolutionUniverse {
    projects: Vec<CompiledProjectUniverseEntry>,
    services: Vec<CompiledServiceRuntimeEntry>,
    runtime: Value,
}

struct StatusResolutionTarget {
    status: String,
    project_id: Option<String>,
    project: Value,
    candidates: Vec<Value>,
}

struct LookupInput {
    values: Vec<String>,
    path: Option<String>,
    cwd: Option<String>,
}

impl LookupInput {
    fn new(query: &str, id: Option<&str>, path: Option<&str>, cwd: Option<&str>) -> Self {
        let mut values = Vec::new();
        push_lookup_value(&mut values, query);
        for value in [id, path, cwd].into_iter().flatten() {
            push_lookup_value(&mut values, value);
            if let Some(name) = Path::new(value).file_name().and_then(|v| v.to_str()) {
                push_lookup_value(&mut values, name);
            }
        }
        for domain in extract_domain_like_values(query) {
            push_lookup_value(&mut values, &domain);
        }
        values.sort();
        values.dedup();
        Self {
            values,
            path: path.map(str::to_string),
            cwd: cwd.map(str::to_string),
        }
    }

    fn domain_values(&self) -> Vec<String> {
        self.values
            .iter()
            .filter(|value| looks_like_domain(value))
            .cloned()
            .collect()
    }

    fn matches_exact(&self, value: &str) -> Option<String> {
        let normalized = normalize_key(value);
        if normalized.is_empty() {
            return None;
        }
        self.values
            .iter()
            .find(|lookup| **lookup == normalized)
            .cloned()
    }

    fn matches_contains(&self, value: &str) -> Option<String> {
        let normalized = normalize_key(value);
        if normalized.len() < 3 {
            return None;
        }
        self.values
            .iter()
            .find(|lookup| {
                lookup.len() >= 3 && (lookup.contains(&normalized) || normalized.contains(*lookup))
            })
            .cloned()
    }
}

fn load_resolution_universe(diagnostics: &mut Vec<Value>) -> ResolutionUniverse {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let loaded = load_compiled_project_universe(&project_root, None);
    let snapshot = loaded.snapshot.as_ref().map(compiled_snapshot_to_value);
    let deployment_policy = load_compiled_deployment_policy_snapshot(&project_root);
    let policy_snapshot = deployment_policy
        .snapshot
        .as_ref()
        .map(compiled_snapshot_to_value);
    if !loaded.diagnostics.is_empty() {
        diagnostics.push(serde_json::json!({
            "kind": "compiled_project_universe_unavailable",
            "source": "compiled-project-universe",
            "diagnostics": loaded.diagnostics,
            "snapshot": snapshot,
            "recovery": "Run node scripts/compile-v3-runtime.mjs --write from the MissionD repo. Resolver continues with DB and explicit query facts."
        }));
    }
    if !deployment_policy.diagnostics.is_empty() {
        diagnostics.push(serde_json::json!({
            "kind": "compiled_deployment_policy_unavailable",
            "source": "compiled-deployment-policy",
            "diagnostics": deployment_policy.diagnostics,
            "snapshot": policy_snapshot,
            "recovery": "Run node scripts/compile-v3-runtime.mjs --write from the MissionD repo so deployment policy hash is available."
        }));
    }
    let runtime = serde_json::json!({
        "universe": snapshot,
        "deploymentPolicy": policy_snapshot,
    });
    match loaded.payload {
        Some(payload) => ResolutionUniverse {
            projects: payload.projects,
            services: payload.services,
            runtime,
        },
        None => ResolutionUniverse {
            projects: Vec::new(),
            services: Vec::new(),
            runtime,
        },
    }
}

fn compiled_snapshot_to_value(snapshot: &CompiledRuntimeSnapshot) -> Value {
    serde_json::json!({
        "kind": snapshot.kind.clone(),
        "path": snapshot.path.display().to_string(),
        "schemaVersion": snapshot.schema_version.clone(),
        "sourceHash": snapshot.source_hash.clone(),
    })
}

fn resolve_status_target(
    query: &str,
    id: Option<&str>,
    path: Option<&str>,
    cwd: Option<&str>,
    projects: &[ProjectConfig],
    compiled: &ResolutionUniverse,
) -> Option<StatusResolutionTarget> {
    if query.trim().is_empty() {
        return None;
    }
    let lookup = LookupInput::new(query, id, path, cwd);
    let mut candidates: HashMap<String, ResolutionCandidate> = HashMap::new();
    let mut known_project_ids: HashSet<String> =
        projects.iter().map(|p| normalize_key(&p.id)).collect();

    for project in projects {
        match_db_project(&lookup, project, &mut candidates);
    }
    for project in &compiled.projects {
        if let Some(id) = project.id.as_deref() {
            known_project_ids.insert(normalize_key(id));
        }
    }
    for project in &compiled.projects {
        match_compiled_project(&lookup, project, projects, &mut candidates);
    }
    for service in &compiled.services {
        match_compiled_service(
            &lookup,
            service,
            projects,
            &known_project_ids,
            &mut candidates,
        );
    }

    let mut candidate_values = candidates
        .into_values()
        .map(candidate_to_value)
        .collect::<Vec<_>>();
    candidate_values.sort_by(|a, b| {
        let left = a.get("score").and_then(Value::as_i64).unwrap_or(0);
        let right = b.get("score").and_then(Value::as_i64).unwrap_or(0);
        right.cmp(&left)
    });
    let top_score = candidate_values
        .first()
        .and_then(|v| v.get("score"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let second_score = candidate_values
        .get(1)
        .and_then(|v| v.get("score"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let status = if candidate_values.is_empty() {
        "not_found"
    } else if candidate_values.len() > 1 && top_score - second_score < 8 {
        "ambiguous"
    } else {
        "resolved"
    };
    let matched_project = if status == "resolved" {
        candidate_values
            .first()
            .and_then(|candidate| candidate.get("project"))
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let matched_project_id = matched_project
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(StatusResolutionTarget {
        status: status.to_string(),
        project_id: matched_project_id,
        project: matched_project,
        candidates: candidate_values,
    })
}

fn enrich_project_status_value(value: &mut Value) {
    if !value.is_object() {
        return;
    }
    let mut diagnostics = Vec::new();
    let compiled = load_resolution_universe(&mut diagnostics);
    value["compiledRuntime"] = compiled.runtime.clone();
    let runtime_status = compiled_runtime_status_from_projection(&compiled.runtime);
    value["runtime_status"] = runtime_status.clone();
    value["activeRelease"] = missiond_active_release_status();
    value["productionRelease"] =
        production_release_projection(value.get("id").and_then(Value::as_str), &runtime_status);
    if !diagnostics.is_empty() {
        value["diagnostics"] = Value::Array(diagnostics);
    }
}

fn production_release_projection(project_id: Option<&str>, runtime_status: &Value) -> Value {
    let project = project_id.unwrap_or("");
    let policy_hash = runtime_status
        .get("policy_hash")
        .cloned()
        .unwrap_or(Value::Null);
    let closure_path = if project.is_empty() {
        Value::Null
    } else {
        serde_json::json!(format!("/api/deploy/closure/{}", project))
    };
    let evidence_path = if project.is_empty() {
        Value::Null
    } else {
        serde_json::json!("/api/deploy/evidence/:release_id")
    };
    serde_json::json!({
        "authority": "deploy-center",
        "runtimeFactAuthority": "deploy-center",
        "closureAuthority": "ReleaseEvidence+ClosureVerdict",
        "status": "not_queried",
        "latestClosureVerdict": Value::Null,
        "compiledDeploymentPolicyHash": policy_hash,
        "closureApi": closure_path,
        "evidenceApi": evidence_path,
        "waitEvent": {
            "domain": "system",
            "kind": "closure_verdict",
            "eventKind": "closure_verdict",
            "projectId": if project.is_empty() { Value::Null } else { serde_json::json!(project) }
        },
        "reason": "MissionD reports identity and compiled policy state only; Deploy Center release evidence and closure verdict are the runtime release authority."
    })
}

fn compiled_runtime_status_from_projection(compiled_runtime: &Value) -> Value {
    let active = missiond_active_release_status();
    let current_universe = compiled_runtime_source_hash(compiled_runtime, "universe");
    let current_policy = compiled_runtime_source_hash(compiled_runtime, "deploymentPolicy");
    let current = serde_json::json!({
        "universe": current_universe,
        "deploymentPolicy": current_policy,
    });
    let missing_current = ["universe", "deploymentPolicy"]
        .into_iter()
        .filter(|key| {
            current
                .get(*key)
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        })
        .collect::<Vec<_>>();
    if !missing_current.is_empty() {
        return serde_json::json!({
            "status": "compiled_runtime_unavailable",
            "compiled_runtime_stale": false,
            "compiled_source_hash": current.get("universe").cloned().unwrap_or(Value::Null),
            "policy_hash": current.get("deploymentPolicy").cloned().unwrap_or(Value::Null),
            "currentSourceHash": current,
            "missingCurrentProjections": missing_current,
        });
    }

    if !active.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return serde_json::json!({
            "status": "active_release_unknown",
            "compiled_runtime_stale": false,
            "compiled_source_hash": current.get("universe").cloned().unwrap_or(Value::Null),
            "policy_hash": current.get("deploymentPolicy").cloned().unwrap_or(Value::Null),
            "currentSourceHash": current,
            "activeReleaseDiagnostic": active.get("diagnostic").cloned().unwrap_or(Value::Null),
        });
    }

    let active_universe = active_release_projection_source_hash(&active, "universe");
    let active_policy = active_release_projection_source_hash(&active, "deploymentPolicy");
    let active_hashes = serde_json::json!({
        "universe": active_universe,
        "deploymentPolicy": active_policy,
    });
    let mut stale = Vec::new();
    for key in ["universe", "deploymentPolicy"] {
        let current_hash = current.get(key).and_then(Value::as_str);
        let active_hash = active_hashes.get(key).and_then(Value::as_str);
        if current_hash != active_hash {
            stale.push(serde_json::json!({
                "projection": key,
                "currentSourceHash": current_hash,
                "activeReleaseSourceHash": active_hash,
            }));
        }
    }

    serde_json::json!({
        "status": if stale.is_empty() { "current" } else { "compiled_runtime_stale" },
        "compiled_runtime_stale": !stale.is_empty(),
        "compiled_source_hash": current.get("universe").cloned().unwrap_or(Value::Null),
        "policy_hash": current.get("deploymentPolicy").cloned().unwrap_or(Value::Null),
        "currentSourceHash": current,
        "activeReleaseSourceHash": active_hashes,
        "staleProjections": stale,
    })
}

fn compiled_runtime_source_hash(compiled_runtime: &Value, key: &str) -> Option<String> {
    compiled_runtime
        .get(key)
        .and_then(|value| value.get("sourceHash"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn active_release_projection_source_hash(active: &Value, key: &str) -> Option<String> {
    active
        .get("typedLispRuntime")
        .and_then(|value| value.get("projections"))
        .and_then(|value| value.get(key))
        .and_then(|value| value.get("source_hash"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn missiond_active_release_status() -> Value {
    let install_root = std::env::var("MISSIOND_INSTALL_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|home| PathBuf::from(home).join(".xjp-mission"))
                .unwrap_or_else(|_| PathBuf::from(".xjp-mission"))
        });
    let active_link = std::env::var("MISSIOND_ACTIVE_LINK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| install_root.join("active"));
    missiond_active_release_status_for(&active_link)
}

fn missiond_active_release_status_for(active_link: &Path) -> Value {
    let active_target = std::fs::read_link(active_link).ok();
    let active_dir = active_target.as_deref().unwrap_or(active_link);
    let manifest_path = active_dir.join("release-manifest.json");
    let bytes = match std::fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            return serde_json::json!({
                "ok": false,
                "activeLink": active_link.display().to_string(),
                "activeTarget": active_target.as_ref().map(|p| p.display().to_string()),
                "manifestPath": manifest_path.display().to_string(),
                "missing": true,
                "diagnostic": err.to_string()
            });
        }
    };
    let manifest: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => {
            return serde_json::json!({
                "ok": false,
                "activeLink": active_link.display().to_string(),
                "activeTarget": active_target.as_ref().map(|p| p.display().to_string()),
                "manifestPath": manifest_path.display().to_string(),
                "missing": false,
                "diagnostic": err.to_string()
            });
        }
    };
    let typed = manifest
        .get("typed_lisp_runtime")
        .cloned()
        .unwrap_or(Value::Null);
    serde_json::json!({
        "ok": !typed.is_null(),
        "activeLink": active_link.display().to_string(),
        "activeTarget": active_target.as_ref().map(|p| p.display().to_string()),
        "manifestPath": manifest_path.display().to_string(),
        "releaseId": manifest.get("release_id").cloned().unwrap_or(Value::Null),
        "gitSha": manifest.get("git_sha").cloned().unwrap_or(Value::Null),
        "typedLispRuntimePresent": !typed.is_null(),
        "typedLispRuntime": typed,
    })
}

fn match_db_project(
    lookup: &LookupInput,
    project: &ProjectConfig,
    candidates: &mut HashMap<String, ResolutionCandidate>,
) {
    let project_value = db_project_to_value(project);
    if let Some(matched) = lookup.matches_exact(&project.id) {
        add_candidate(
            candidates,
            &project.id,
            100,
            "db_project_id_exact",
            project_value.clone(),
            evidence("db_project", "id", &project.id, &matched),
        );
    } else if let Some(matched) = lookup.matches_contains(&project.id) {
        add_candidate(
            candidates,
            &project.id,
            72,
            "db_project_id_contains",
            project_value.clone(),
            evidence("db_project", "id", &project.id, &matched),
        );
    }

    if path_matches_project(&lookup.cwd, &project.path)
        || path_matches_project(&lookup.path, &project.path)
    {
        add_candidate(
            candidates,
            &project.id,
            94,
            "db_project_root_prefix",
            project_value.clone(),
            evidence(
                "db_project",
                "path",
                &project.path,
                lookup
                    .cwd
                    .as_deref()
                    .or(lookup.path.as_deref())
                    .unwrap_or(""),
            ),
        );
    }

    if let Some(name) = Path::new(&project.path)
        .file_name()
        .and_then(|v| v.to_str())
    {
        if let Some(matched) = lookup.matches_exact(name) {
            add_candidate(
                candidates,
                &project.id,
                78,
                "db_project_root_name_exact",
                project_value.clone(),
                evidence("db_project", "root_basename", name, &matched),
            );
        }
    }

    if let Some(github_url) = project.github_url.as_deref() {
        if let Some(matched) = lookup.matches_exact(github_url) {
            add_candidate(
                candidates,
                &project.id,
                82,
                "db_project_git_remote_exact",
                project_value,
                evidence("db_project", "github_url", github_url, &matched),
            );
        }
    }
}

fn match_compiled_project(
    lookup: &LookupInput,
    project: &CompiledProjectUniverseEntry,
    db_projects: &[ProjectConfig],
    candidates: &mut HashMap<String, ResolutionCandidate>,
) {
    let Some(id) = project.id.as_deref() else {
        return;
    };
    let project_value = db_project_value_for_id(db_projects, id).unwrap_or_else(|| {
        serde_json::json!({
            "id": id,
            "source": "compiled-project-universe",
            "compiledProject": compiled_project_to_value(project),
        })
    });
    if let Some(matched) = lookup.matches_exact(id) {
        add_candidate(
            candidates,
            id,
            99,
            "compiled_project_id_exact",
            project_value.clone(),
            evidence("compiled_project_universe", "id", id, &matched),
        );
    } else if let Some(matched) = lookup.matches_contains(id) {
        add_candidate(
            candidates,
            id,
            72,
            "compiled_project_id_contains",
            project_value.clone(),
            evidence("compiled_project_universe", "id", id, &matched),
        );
    }
    for alias in &project.aliases {
        if let Some(matched) = lookup.matches_exact(alias) {
            add_candidate(
                candidates,
                id,
                96,
                "compiled_project_alias_exact",
                project_value.clone(),
                evidence("compiled_project_universe", "aliases", alias, &matched),
            );
        } else if let Some(matched) = lookup.matches_contains(alias) {
            add_candidate(
                candidates,
                id,
                84,
                "compiled_project_alias_contains",
                project_value.clone(),
                evidence("compiled_project_universe", "aliases", alias, &matched),
            );
        }
    }
    for service_id in &project.service_ids {
        if let Some(matched) = lookup.matches_exact(service_id) {
            add_candidate(
                candidates,
                id,
                91,
                "compiled_project_service_id_exact",
                project_value.clone(),
                evidence(
                    "compiled_project_universe",
                    "service_ids",
                    service_id,
                    &matched,
                ),
            );
        }
    }
    for value in [
        project.root.as_deref(),
        project.path.as_deref(),
        project.intent.as_deref(),
        project.backend.as_deref(),
        project.frontend.as_deref(),
        project.operations.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(matched) = lookup.matches_exact(value) {
            add_candidate(
                candidates,
                id,
                82,
                "compiled_project_path_exact",
                project_value.clone(),
                evidence("compiled_project_universe", "path", value, &matched),
            );
        }
    }
}

fn match_compiled_service(
    lookup: &LookupInput,
    service: &CompiledServiceRuntimeEntry,
    db_projects: &[ProjectConfig],
    known_project_ids: &HashSet<String>,
    candidates: &mut HashMap<String, ResolutionCandidate>,
) {
    let Some(service_id) = service.id.as_deref() else {
        return;
    };
    let project_id = if known_project_ids.contains(&normalize_key(service_id)) {
        service_id
    } else {
        service.project.as_deref().unwrap_or(service_id)
    };
    let mut project_value = db_project_value_for_id(db_projects, project_id).unwrap_or_else(
        || serde_json::json!({"id": project_id, "source": "compiled-service-runtime"}),
    );
    if let Value::Object(map) = &mut project_value {
        map.insert(
            "serviceRuntime".to_string(),
            compiled_service_to_value(service),
        );
    }

    for (field, value, exact_score, contains_score) in [
        ("id", Some(service_id), 93, 78),
        ("project", service.project.as_deref(), 88, 72),
        (
            "public_base_url",
            service.public_base_url.as_deref(),
            97,
            90,
        ),
        ("frontend_url", service.frontend_url.as_deref(), 96, 88),
        ("api_base_url", service.api_base_url.as_deref(), 94, 86),
        ("root", service.root.as_deref(), 82, 70),
    ] {
        let Some(value) = value else {
            continue;
        };
        if let Some(matched) = lookup.matches_exact(value) {
            add_candidate(
                candidates,
                project_id,
                exact_score,
                &format!("service_runtime_{field}_exact"),
                project_value.clone(),
                evidence("compiled_service_runtime", field, value, &matched),
            );
        } else if let Some(matched) = lookup.matches_contains(value) {
            add_candidate(
                candidates,
                project_id,
                contains_score,
                &format!("service_runtime_{field}_contains"),
                project_value.clone(),
                evidence("compiled_service_runtime", field, value, &matched),
            );
        }
    }

    for domain in &service.domains {
        if let Some(matched) = lookup.matches_exact(domain) {
            add_candidate(
                candidates,
                project_id,
                98,
                "service_runtime_domain_exact",
                project_value.clone(),
                evidence("compiled_service_runtime", "domains", domain, &matched),
            );
        } else if let Some(matched) = lookup.matches_contains(domain) {
            add_candidate(
                candidates,
                project_id,
                88,
                "service_runtime_domain_contains",
                project_value.clone(),
                evidence("compiled_service_runtime", "domains", domain, &matched),
            );
        }
    }
}

fn add_candidate(
    candidates: &mut HashMap<String, ResolutionCandidate>,
    project_id: &str,
    score: i32,
    match_kind: &str,
    project: Value,
    evidence: Value,
) {
    if project_id.trim().is_empty() {
        return;
    }
    let entry = candidates
        .entry(project_id.to_string())
        .or_insert_with(|| ResolutionCandidate {
            project_id: project_id.to_string(),
            score,
            match_kind: match_kind.to_string(),
            project: project.clone(),
            evidence: Vec::new(),
        });
    if score > entry.score {
        entry.score = score;
        entry.match_kind = match_kind.to_string();
        entry.project = project;
    }
    entry.evidence.push(evidence);
}

fn candidate_to_value(candidate: ResolutionCandidate) -> Value {
    serde_json::json!({
        "id": candidate.project_id.clone(),
        "project_id": candidate.project_id,
        "score": candidate.score,
        "match_kind": candidate.match_kind,
        "project": candidate.project,
        "evidence": candidate.evidence,
    })
}

fn evidence(source: &str, field: &str, value: &str, matched: &str) -> Value {
    serde_json::json!({
        "source": source,
        "field": field,
        "value": value,
        "matched": matched,
    })
}

fn db_project_to_value(project: &ProjectConfig) -> Value {
    let mut value = serde_json::to_value(project).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.insert(
            "source".to_string(),
            Value::String("missiond-db".to_string()),
        );
    }
    value
}

fn db_project_value_for_id(projects: &[ProjectConfig], id: &str) -> Option<Value> {
    projects
        .iter()
        .find(|project| project.id == id)
        .map(db_project_to_value)
}

fn compiled_project_to_value(project: &CompiledProjectUniverseEntry) -> Value {
    serde_json::json!({
        "id": project.id.clone(),
        "aliases": project.aliases.clone(),
        "service_ids": project.service_ids.clone(),
        "kind": project.kind.clone(),
        "management_domain": project.management_domain.clone(),
        "runtime_layer": project.runtime_layer.clone(),
        "root": project.root.clone(),
        "path": project.path.clone(),
        "intent": project.intent.clone(),
        "backend": project.backend.clone(),
        "frontend": project.frontend.clone(),
        "operations": project.operations.clone(),
        "status": project.status.clone(),
        "surface": project.surface.clone(),
        "missiond_role": project.missiond_role.clone(),
        "checks": project.checks.clone(),
    })
}

fn compiled_service_to_value(service: &CompiledServiceRuntimeEntry) -> Value {
    serde_json::json!({
        "id": service.id.clone(),
        "project": service.project.clone(),
        "root": service.root.clone(),
        "intent": service.intent.clone(),
        "backend": service.backend.clone(),
        "frontend": service.frontend.clone(),
        "operations": service.operations.clone(),
        "environment": service.environment.clone(),
        "public_base_url": service.public_base_url.clone(),
        "frontend_url": service.frontend_url.clone(),
        "api_base_url": service.api_base_url.clone(),
        "domains": service.domains.clone(),
        "health": service.health.clone(),
        "dependencies": service.dependencies.clone(),
        "ops_capability": service.ops_capability.clone(),
        "surface": service.surface.clone(),
        "supportCatalog": service.support_catalog.clone(),
    })
}

fn compiled_project_lookup(id: &str) -> Option<Value> {
    let mut diagnostics = Vec::new();
    let compiled = load_resolution_universe(&mut diagnostics);
    let normalized = normalize_key(id);
    if let Some(project) = compiled
        .projects
        .iter()
        .find(|project| compiled_project_lookup_matches(project, &normalized))
    {
        let mut value = serde_json::json!({
            "id": project.id.clone().unwrap_or_else(|| id.to_string()),
            "source": "compiled-project-universe",
            "db_status": "missing",
            "compiledProject": compiled_project_to_value(project),
        });
        if !diagnostics.is_empty() {
            value["diagnostics"] = Value::Array(diagnostics);
        }
        return Some(value);
    }

    compiled.services.iter().find_map(|service| {
        if !compiled_service_lookup_matches(service, &normalized) {
            return None;
        }
        let project_id = service
            .project
            .clone()
            .or_else(|| service.id.clone())
            .unwrap_or_else(|| id.to_string());
        let mut value = serde_json::json!({
            "id": project_id,
            "source": "compiled-service-runtime",
            "db_status": "missing",
            "serviceRuntime": compiled_service_to_value(service),
        });
        if !diagnostics.is_empty() {
            value["diagnostics"] = Value::Array(diagnostics.clone());
        }
        Some(value)
    })
}

fn compiled_project_lookup_matches(
    project: &CompiledProjectUniverseEntry,
    normalized: &str,
) -> bool {
    project.id.as_deref().map(normalize_key).as_deref() == Some(normalized)
        || project
            .aliases
            .iter()
            .any(|alias| normalize_key(alias) == normalized)
        || project
            .service_ids
            .iter()
            .any(|service_id| normalize_key(service_id) == normalized)
}

fn compiled_service_lookup_matches(
    service: &CompiledServiceRuntimeEntry,
    normalized: &str,
) -> bool {
    service.id.as_deref().map(normalize_key).as_deref() == Some(normalized)
        || service.project.as_deref().map(normalize_key).as_deref() == Some(normalized)
}

fn path_matches_project(input: &Option<String>, project_path: &str) -> bool {
    let Some(input) = input.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return false;
    };
    Path::new(input).starts_with(Path::new(project_path))
}

fn first_non_empty(values: [Option<&str>; 4]) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn push_lookup_value(values: &mut Vec<String>, raw: &str) {
    let normalized = normalize_key(raw);
    if !normalized.is_empty() {
        values.push(normalized.clone());
        if let Some(stripped) = normalized.strip_prefix("www.") {
            values.push(stripped.to_string());
        }
    }
}

fn normalize_key(raw: &str) -> String {
    let mut value = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_lowercase();
    if let Some(rest) = value.strip_prefix("http://") {
        value = rest.to_string();
    } else if let Some(rest) = value.strip_prefix("https://") {
        value = rest.to_string();
    }
    if let Some((host, _)) = value.split_once('#') {
        value = host.to_string();
    }
    if let Some((host, _)) = value.split_once('?') {
        value = host.to_string();
    }
    if let Some((host, path)) = value.split_once('/') {
        if host.contains('.') {
            value = host.to_string();
        } else if !path.is_empty() {
            value = format!("{host}/{path}");
        }
    }
    value
        .trim_matches(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    ',' | ';' | ':' | ')' | '(' | ']' | '[' | '}' | '{' | '。' | '，' | '、'
                )
        })
        .to_string()
}

fn extract_domain_like_values(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '/') {
            current.push(ch);
        } else if !current.is_empty() {
            push_if_domain_like(&mut values, &current);
            current.clear();
        }
    }
    if !current.is_empty() {
        push_if_domain_like(&mut values, &current);
    }
    values.sort();
    values.dedup();
    values
}

fn push_if_domain_like(values: &mut Vec<String>, raw: &str) {
    let normalized = normalize_key(raw);
    if looks_like_domain(&normalized) {
        values.push(normalized);
    }
}

fn looks_like_domain(value: &str) -> bool {
    let value = value.trim();
    if value.contains('/') || !value.contains('.') {
        return false;
    }
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        && parts.last().is_some_and(|part| part.len() >= 2)
}

fn discover_candidate_roots(lookup: &LookupInput) -> Vec<Value> {
    let Some(slug) = lookup_slug(lookup) else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for base in candidate_search_bases() {
        scan_candidate_base(&base, &slug, &mut roots, &mut seen);
        if roots.len() >= 12 {
            break;
        }
    }
    roots
}

fn candidate_search_bases() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/Users/jinchen"));
    vec![
        home.join("Projects"),
        home.join("Downloads/xiaojinpro-gateway/xiaojinpro-backend/services"),
        home.join("Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps"),
        home.join("Downloads/xiaojinpro-gateway/xiaojinpro-backend"),
        home.join("Downloads"),
    ]
}

fn scan_candidate_base(
    base: &Path,
    slug: &str,
    roots: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !similar_slug(name, slug) {
            continue;
        }
        let path_str = path.display().to_string();
        if !seen.insert(path_str.clone()) {
            continue;
        }
        roots.push(serde_json::json!({
            "path": path_str,
            "basename": name,
            "match": "directory_name",
            "intent_candidates": intent_candidates_for_root(&path),
        }));
    }
}

fn intent_candidates_for_root(path: &Path) -> Vec<String> {
    [
        ".missiond/intent.lisp",
        ".jarvis/intent.lisp",
        "intent.lisp",
    ]
    .into_iter()
    .filter(|candidate| path.join(candidate).exists())
    .map(str::to_string)
    .collect()
}

fn similar_slug(name: &str, slug: &str) -> bool {
    let name = slug_key(name);
    let slug = slug_key(slug);
    !name.is_empty() && !slug.is_empty() && (name.contains(&slug) || slug.contains(&name))
}

fn lookup_slug(lookup: &LookupInput) -> Option<String> {
    lookup
        .domain_values()
        .first()
        .and_then(|domain| domain.split('.').next().map(str::to_string))
        .or_else(|| {
            lookup
                .values
                .iter()
                .find(|value| value.chars().any(|ch| ch.is_ascii_alphanumeric()))
                .cloned()
        })
}

fn slug_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn build_registration_proposal(lookup: &LookupInput, candidate_roots: &[Value]) -> Value {
    let Some(slug) = lookup_slug(lookup) else {
        return Value::Null;
    };
    let domains = lookup.domain_values();
    let candidate_root = candidate_roots
        .first()
        .and_then(|root| root.get("path"))
        .and_then(Value::as_str);
    serde_json::json!({
        "schema": "missiond.project-registration-proposal.v1",
        "project_id": slug.replace('.', "-"),
        "management_domain": "product-service-layer",
        "runtime_layer": "product-fullstack",
        "domains": domains,
        "candidate_root": candidate_root,
        "mutation_required": true,
        "safe_next_tools": [
            {
                "tool": "mission_project",
                "args": {
                    "action": "survey",
                    "id": slug.replace('.', "-"),
                    "path": candidate_root,
                    "dry_run": true
                }
            },
            {
                "tool": "mission_project",
                "args": {
                    "action": "init",
                    "id": slug.replace('.', "-"),
                    "path": candidate_root
                }
            }
        ],
        "note": "resolve is read-only. Register only after confirming the candidate root and intended management/runtime classification."
    })
}

fn project_resolution_next_actions(
    status: &str,
    matched_project_id: Option<&str>,
    registration_proposal: &Value,
) -> Vec<Value> {
    match status {
        "resolved" => vec![serde_json::json!({
            "action": "use_project_id",
            "project_id": matched_project_id,
            "hint": "Pass this project_id to mission_context_gather, KB, Board, conversation, and worker dispatch calls."
        })],
        "ambiguous" => vec![serde_json::json!({
            "action": "choose_candidate",
            "hint": "Ask for disambiguation or use the highest-evidence candidate only if the user context makes it unambiguous."
        })],
        "unregistered_candidate" => vec![serde_json::json!({
            "action": "confirm_registration",
            "proposal": registration_proposal,
            "hint": "Do not conclude the project is absent. Confirm root/classification, then init/survey/register it."
        })],
        "stale_runtime" => vec![serde_json::json!({
            "action": "refresh_runtime_projection",
            "command": "node scripts/compile-v3-runtime.mjs --write",
            "hint": "Compiled universe was unavailable; registry DB and explicit query facts were still used."
        })],
        "compiled_runtime_stale" => vec![serde_json::json!({
            "action": "refresh_and_redeploy_runtime_projection",
            "commands": [
                "node scripts/compile-v3-runtime.mjs --write",
                "scripts/deploy-daemon.sh"
            ],
            "hint": "The daemon active release and compiled runtime projection hashes do not match; do not treat unregistered candidates as authoritative until the hashes converge."
        })],
        _ => vec![serde_json::json!({
            "action": "request_root_or_register",
            "hint": "No registered project or local candidate was found. Ask for the repo root or add the project to MissionD Universe."
        })],
    }
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("{} is required", key))
}

#[cfg(test)]
mod tests {
    use super::{
        compiled_project_lookup_matches, compiled_service_lookup_matches,
        CompiledProjectUniverseEntry, CompiledServiceRuntimeEntry,
    };

    fn compiled_project() -> CompiledProjectUniverseEntry {
        CompiledProjectUniverseEntry {
            id: Some("asr".to_string()),
            aliases: vec!["xjp-asr".to_string(), "speechscribe.top".to_string()],
            service_ids: vec!["speechscribe".to_string()],
            kind: None,
            management_domain: None,
            runtime_layer: None,
            root: None,
            path: None,
            intent: None,
            backend: None,
            frontend: None,
            operations: None,
            status: None,
            surface: None,
            missiond_role: None,
            checks: Vec::new(),
        }
    }

    fn compiled_service() -> CompiledServiceRuntimeEntry {
        CompiledServiceRuntimeEntry {
            id: Some("payments-api".to_string()),
            project: Some("payments".to_string()),
            root: None,
            intent: None,
            backend: None,
            frontend: None,
            operations: None,
            environment: None,
            public_base_url: None,
            frontend_url: None,
            api_base_url: None,
            domains: Vec::new(),
            health: Vec::new(),
            dependencies: Vec::new(),
            ops_capability: None,
            surface: None,
            support_catalog: None,
        }
    }

    #[test]
    fn compiled_project_lookup_matches_id_alias_and_service_id() {
        let project = compiled_project();
        assert!(compiled_project_lookup_matches(&project, "asr"));
        assert!(compiled_project_lookup_matches(&project, "xjp-asr"));
        assert!(compiled_project_lookup_matches(
            &project,
            "speechscribe.top"
        ));
        assert!(compiled_project_lookup_matches(&project, "speechscribe"));
        assert!(!compiled_project_lookup_matches(&project, "payments"));
    }

    #[test]
    fn compiled_service_lookup_matches_id_or_project() {
        let service = compiled_service();
        assert!(compiled_service_lookup_matches(&service, "payments-api"));
        assert!(compiled_service_lookup_matches(&service, "payments"));
        assert!(!compiled_service_lookup_matches(&service, "asr"));
    }
}
