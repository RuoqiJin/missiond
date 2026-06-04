use crate::context::v3_blueprint_runtime::{
    load_compiled_project_universe, CompiledProjectUniverseEntry, CompiledRuntimeSnapshot,
};
use anyhow::{anyhow, Result};
use missiond_core::types::ProjectConfig;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::state::AppState;

pub(super) async fn handle_reconcile(state: &AppState, args: Value) -> Result<ToolResult> {
    let projects = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let deploy_center_root = args
        .get("deployCenterRoot")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| std::env::var("DEPLOY_CENTER_ROOT").ok())
        .or_else(|| registered_project_root(&projects, "deploy-center"))
        .unwrap_or_else(|| "$MISSION_HOME/projects/deploy-center".to_string());
    let forge_root = args
        .get("forgeRoot")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| std::env::var("JARVIS_FORGE_ROOT").ok())
        .or_else(|| registered_project_root(&projects, "jarvis-forge"))
        .unwrap_or_else(|| "$MISSION_HOME/projects/jarvis-forge".to_string());
    let deploy_center_root = deploy_center_root.as_str();
    let forge_root = forge_root.as_str();

    let mut drift = Vec::new();
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let compiled = load_compiled_project_universe(&project_root, None);
    let compiled_snapshot = compiled.snapshot.as_ref().map(compiled_snapshot_to_value);
    let mut registry_reconcile = RegistryReconcile::default();
    if !compiled.diagnostics.is_empty() {
        drift.push(json!({
            "kind": "compiled_project_universe_unavailable",
            "source": "compiled-project-universe",
            "diagnostics": compiled.diagnostics,
            "snapshot": compiled_snapshot,
            "recovery": "Run node scripts/compile-v3-runtime.mjs --write from the MissionD repo, then retry mission_project(action=\"reconcile\")."
        }));
    }
    if let Some(payload) = compiled.payload.as_ref() {
        registry_reconcile =
            reconcile_db_with_compiled_universe(&project_root, &projects, &payload.projects);
    }
    let infra_server_count = state.infra.read().map(|i| i.servers.len()).unwrap_or(0);
    let skill_infra_evidence = count_skill_infra_evidence(state);
    let skill_credential_risks = count_skill_credential_risks(state);

    let deploy_center = project_match(&projects, "deploy-center", deploy_center_root);
    if !deploy_center.path_exists {
        drift.push(json!({
            "kind": "deploy_fact_missing",
            "source": "deploy-center",
            "message": "canonical deploy-center root is not readable",
            "expectedRoot": deploy_center_root,
        }));
    }
    if !deploy_center.registered {
        drift.push(json!({
            "kind": "missing_in_missiond",
            "source": "deploy-center",
            "projectId": "deploy-center",
            "expectedRoot": deploy_center_root,
        }));
    } else if !deploy_center.root_matches {
        drift.push(json!({
            "kind": "root_mismatch",
            "source": "deploy-center",
            "projectId": "deploy-center",
            "expectedRoot": deploy_center_root,
            "actualRoots": deploy_center.actual_roots,
        }));
    }

    let forge = project_match(&projects, "jarvis-forge", forge_root);
    if !forge.path_exists {
        drift.push(json!({
            "kind": "forge_catalog_unavailable",
            "source": "forge",
            "message": "Forge root is not readable",
            "expectedRoot": forge_root,
        }));
    }
    if !forge.registered {
        drift.push(json!({
            "kind": "missing_in_missiond",
            "source": "forge",
            "projectId": "jarvis-forge",
            "expectedRoot": forge_root,
        }));
    } else if !forge.root_matches {
        drift.push(json!({
            "kind": "root_mismatch",
            "source": "forge",
            "projectId": "jarvis-forge",
            "expectedRoot": forge_root,
            "actualRoots": forge.actual_roots,
        }));
    }

    let stale_aliases: Vec<_> = projects
        .iter()
        .filter(|p| p.id == "xjp-deploy-center" && p.active)
        .map(|p| json!({"id": p.id, "root": p.path}))
        .collect();
    if !stale_aliases.is_empty() {
        drift.push(json!({
            "kind": "alias_conflict",
            "source": "missiond",
            "message": "historical xjp-deploy-center alias is still active",
            "aliases": stale_aliases,
        }));
    }
    if skill_infra_evidence > 0 && infra_server_count == 0 {
        drift.push(json!({
            "kind": "runtime_fact_missing",
            "source": "skill-evidence",
            "message": "skills contain infra/runtime evidence but MissionD/deploy-center runtime inventory has no configured server rows",
            "skillEvidenceItems": skill_infra_evidence,
        }));
    }
    if skill_credential_risks > 0 {
        drift.push(json!({
            "kind": "credential_inline_risk",
            "source": "skill-evidence",
            "message": "skills contain credential-like operational lines; promote only redacted secret_ref facts and migrate values to secret-store",
            "riskCount": skill_credential_risks,
        }));
    }

    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.project-registry-reconcile.v1",
        "consistent": drift.is_empty() && registry_reconcile.consistent(),
        "sources": {
            "missiond": {
                "authority": "project identity, SSOT paths, maturity, Board/workstation scheduling",
                "projectCount": projects.len(),
                "activeProjectCount": projects.iter().filter(|p| p.active).count(),
            },
            "deployCenter": {
                "authority": "deployment targets, runtime location, release provenance, agent/executor state",
                "root": deploy_center_root,
                "registered": deploy_center.registered,
                "rootMatches": deploy_center.root_matches,
                "ssotPath": format!("{deploy_center_root}/.missiond/intent.lisp"),
                "ssotExists": Path::new(deploy_center_root).join(".missiond/intent.lisp").exists(),
            },
            "forge": {
                "authority": "component/pattern catalog, code reality mirror, Universe DAG recommendations",
                "root": forge_root,
                "registered": forge.registered,
                "rootMatches": forge.root_matches,
                "ssotPath": format!("{forge_root}/.missiond/intent.lisp"),
                "ssotExists": Path::new(forge_root).join(".missiond/intent.lisp").exists(),
                "runtimeAuthority": false,
            },
            "infra": {
                "authority": "MissionD summarizes runtime targets; deploy-center owns verified runtime facts; skills are evidence only",
                "configuredServers": infra_server_count,
                "skillEvidenceItems": skill_infra_evidence,
                "skillCredentialInlineRisks": skill_credential_risks,
            }
        },
        "registry_reconcile": registry_reconcile.to_value(compiled_snapshot),
        "drift": drift,
    })))
}

#[derive(Default)]
struct RegistryReconcile {
    missing_in_db: Vec<Value>,
    missing_in_v3: Vec<Value>,
    inactive_or_legacy: Vec<Value>,
    metadata_drift: Vec<Value>,
}

impl RegistryReconcile {
    fn consistent(&self) -> bool {
        self.missing_in_db.is_empty()
            && self.missing_in_v3.is_empty()
            && self.metadata_drift.is_empty()
    }

    fn to_value(&self, snapshot: Option<Value>) -> Value {
        json!({
            "schema": "missiond.project-registry-db-v3-diff.v1",
            "source": "compiled-project-universe",
            "snapshot": snapshot,
            "consistent": self.consistent(),
            "counts": {
                "missing_in_db": self.missing_in_db.len(),
                "missing_in_v3": self.missing_in_v3.len(),
                "inactive_or_legacy": self.inactive_or_legacy.len(),
                "metadata_drift": self.metadata_drift.len(),
            },
            "missing_in_db": self.missing_in_db,
            "missing_in_v3": self.missing_in_v3,
            "inactive_or_legacy": self.inactive_or_legacy,
            "metadata_drift": self.metadata_drift,
            "recovery": {
                "missing_in_db": "Run mission_project(action=\"import_universe\") to import compiled V3 project identities into the DB registry.",
                "missing_in_v3": "Add the active DB project to .missiond/v3/shards/universe/project-registry.lisp or mark it inactive/legacy explicitly; reconcile never deletes rows silently.",
                "inactive_or_legacy": "Keep inactive/legacy rows as explicit history, or mark aliases inactive before importing V3 identities.",
                "metadata_drift": "Prefer MissionD V3 as identity authority; run import_universe for DB metadata, or update the V3 root/intent/kind if DB is the correct reality."
            }
        })
    }
}

fn reconcile_db_with_compiled_universe(
    project_root: &Path,
    db_projects: &[ProjectConfig],
    compiled_projects: &[CompiledProjectUniverseEntry],
) -> RegistryReconcile {
    let mut report = RegistryReconcile::default();
    let db_by_id: HashMap<String, &ProjectConfig> = db_projects
        .iter()
        .map(|project| (project.id.to_ascii_lowercase(), project))
        .collect();
    let mut compiled_ids = HashSet::new();
    let mut inactive_seen = HashSet::new();

    for project in compiled_projects {
        let Some(id) = project.id.as_deref().map(normalize_project_id) else {
            continue;
        };
        compiled_ids.insert(id.clone());
        let expected_path = compiled_project_expected_path(project_root, project);
        let importable = expected_path.is_some();
        let expected_active = project.status.as_deref() != Some("retired");

        let Some(db_project) = db_by_id.get(&id) else {
            if expected_active && importable {
                report.missing_in_db.push(json!({
                    "kind": "missing_in_db",
                    "project_id": id,
                    "expectedRoot": expected_path,
                    "v3Status": project.status,
                    "managementDomain": project.management_domain,
                    "runtimeLayer": project.runtime_layer,
                    "recovery": "mission_project(action=\"import_universe\")"
                }));
            } else if !expected_active {
                push_inactive_or_legacy(
                    &mut report.inactive_or_legacy,
                    &mut inactive_seen,
                    json!({
                        "kind": "inactive_or_legacy",
                        "project_id": id,
                        "source": "compiled-project-universe",
                        "v3Status": project.status,
                        "expectedRoot": expected_path,
                    }),
                );
            }
            continue;
        };

        if !db_project.active || !expected_active {
            push_inactive_or_legacy(
                &mut report.inactive_or_legacy,
                &mut inactive_seen,
                json!({
                    "kind": "inactive_or_legacy",
                    "project_id": id,
                    "source": "missiond-db+compiled-project-universe",
                    "dbActive": db_project.active,
                    "v3Active": expected_active,
                    "v3Status": project.status,
                    "dbRoot": db_project.path,
                    "expectedRoot": expected_path,
                }),
            );
        }

        let drift_fields = metadata_drift_fields(db_project, project, expected_path.as_deref());
        if !drift_fields.is_empty() {
            report.metadata_drift.push(json!({
                "kind": "metadata_drift",
                "project_id": id,
                "fields": drift_fields,
                "db": {
                    "path": db_project.path,
                    "intent_path": db_project.intent_path,
                    "active": db_project.active,
                    "kind": db_project.kind,
                },
                "v3": {
                    "root": expected_path,
                    "intent": project.intent,
                    "active": expected_active,
                    "status": project.status,
                    "kind": project.kind,
                },
                "recovery": "mission_project(action=\"import_universe\") or update V3 project-registry root/intent/kind"
            }));
        }
    }

    for db_project in db_projects {
        let id = normalize_project_id(&db_project.id);
        if compiled_ids.contains(&id) {
            continue;
        }
        if !db_project.active || is_legacy_project_alias(&id) || db_project.kind == "reference" {
            push_inactive_or_legacy(
                &mut report.inactive_or_legacy,
                &mut inactive_seen,
                json!({
                    "kind": "inactive_or_legacy",
                    "project_id": id,
                    "source": "missiond-db",
                    "dbActive": db_project.active,
                    "dbRoot": db_project.path,
                    "dbKind": db_project.kind,
                    "reason": if !db_project.active { "inactive_db_row" } else if is_legacy_project_alias(&id) { "legacy_alias" } else { "reference_project" },
                }),
            );
        } else {
            report.missing_in_v3.push(json!({
                "kind": "missing_in_v3",
                "project_id": id,
                "dbRoot": db_project.path,
                "dbKind": db_project.kind,
                "recovery": "Add a V3 project-registry entry or mark the DB row inactive/legacy explicitly."
            }));
        }
    }

    report
}

fn metadata_drift_fields(
    db_project: &ProjectConfig,
    project: &CompiledProjectUniverseEntry,
    expected_path: Option<&str>,
) -> Vec<Value> {
    let mut fields = Vec::new();
    if let Some(expected_path) = expected_path {
        let expected = normalize_path_key(expected_path);
        let actual = normalize_path_key(&db_project.path);
        if expected != actual {
            fields.push(json!({
                "field": "path",
                "expected": expected_path,
                "actual": db_project.path,
            }));
        }
    }

    if let Some(expected_intent) = project.intent.as_deref() {
        if db_project.intent_path.as_deref() != Some(expected_intent) {
            fields.push(json!({
                "field": "intent_path",
                "expected": expected_intent,
                "actual": db_project.intent_path,
            }));
        }
    }

    let expected_kind = project.kind.as_deref().unwrap_or("managed");
    if db_project.kind != expected_kind {
        fields.push(json!({
            "field": "kind",
            "expected": expected_kind,
            "actual": db_project.kind,
        }));
    }

    let expected_active = project.status.as_deref() != Some("retired");
    if db_project.active != expected_active {
        fields.push(json!({
            "field": "active",
            "expected": expected_active,
            "actual": db_project.active,
        }));
    }

    fields
}

fn push_inactive_or_legacy(bucket: &mut Vec<Value>, seen: &mut HashSet<String>, value: Value) {
    let key = value
        .get("project_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    if seen.insert(key) {
        bucket.push(value);
    }
}

fn compiled_project_expected_path(
    project_root: &Path,
    project: &CompiledProjectUniverseEntry,
) -> Option<String> {
    let raw = project.root.as_deref().or_else(|| {
        project
            .path
            .as_deref()
            .filter(|path| !path.ends_with(".lisp"))
    })?;
    let expanded = expand_tilde_path(raw);
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        project_root.join(expanded)
    };
    Some(
        resolved
            .canonicalize()
            .unwrap_or(resolved)
            .display()
            .to_string(),
    )
}

fn normalize_path_key(raw: &str) -> String {
    let expanded = expand_tilde_path(raw);
    expanded
        .canonicalize()
        .unwrap_or(expanded)
        .display()
        .to_string()
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

fn normalize_project_id(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}

fn is_legacy_project_alias(id: &str) -> bool {
    matches!(id, "xjp-deploy-center")
}

fn compiled_snapshot_to_value(snapshot: &CompiledRuntimeSnapshot) -> Value {
    json!({
        "kind": snapshot.kind.clone(),
        "path": snapshot.path.display().to_string(),
        "schemaVersion": snapshot.schema_version.clone(),
        "sourceHash": snapshot.source_hash.clone(),
    })
}

fn count_skill_infra_evidence(state: &AppState) -> usize {
    state
        .skills
        .list()
        .iter()
        .filter_map(|skill| std::fs::read_to_string(&skill.path).ok())
        .flat_map(|content| content.lines().map(str::to_string).collect::<Vec<_>>())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            [
                "12900kf",
                "hostvds",
                "deploy-agent",
                "agent_url",
                "router",
                "embedding",
                "rerank",
                "pcea",
                "ecs",
                "gcp",
                "bwg",
                "192.168.1.20",
                "104.194.81.38",
                "45.156.24.163",
                "106.15.2.17",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
        })
        .count()
}

fn count_skill_credential_risks(state: &AppState) -> usize {
    state
        .skills
        .list()
        .iter()
        .filter_map(|skill| std::fs::read_to_string(&skill.path).ok())
        .flat_map(|content| content.lines().map(str::to_string).collect::<Vec<_>>())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("sshpass")
                || lower.contains("password")
                || lower.contains("密码")
                || lower.contains("token")
                || lower.contains("api_key")
                || lower.contains("api key")
                || lower.contains("secret")
        })
        .count()
}

#[derive(Debug)]
struct ProjectMatch {
    registered: bool,
    root_matches: bool,
    path_exists: bool,
    actual_roots: Vec<String>,
}

fn registered_project_root(
    projects: &[missiond_core::types::ProjectConfig],
    id: &str,
) -> Option<String> {
    projects
        .iter()
        .find(|project| project.id == id && project.active)
        .map(|project| project.path.clone())
}

fn project_match(
    projects: &[missiond_core::types::ProjectConfig],
    id: &str,
    expected_root: &str,
) -> ProjectMatch {
    let actual_roots: Vec<String> = projects
        .iter()
        .filter(|p| p.id == id)
        .map(|p| p.path.clone())
        .collect();
    let root_matches = actual_roots.iter().any(|root| root == expected_root);
    ProjectMatch {
        registered: !actual_roots.is_empty(),
        root_matches,
        path_exists: Path::new(expected_root).exists(),
        actual_roots,
    }
}
