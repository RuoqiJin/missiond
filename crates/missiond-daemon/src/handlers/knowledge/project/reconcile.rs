use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::path::Path;

use crate::state::AppState;

pub(super) async fn handle_reconcile(state: &AppState, args: Value) -> Result<ToolResult> {
    let deploy_center_root = args
        .get("deployCenterRoot")
        .and_then(|v| v.as_str())
        .unwrap_or(
            "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center",
        );
    let forge_root = args
        .get("forgeRoot")
        .and_then(|v| v.as_str())
        .unwrap_or("/Users/jinchen/Projects/jarvis-forge");

    let projects = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let mut drift = Vec::new();

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

    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.project-registry-reconcile.v1",
        "consistent": drift.is_empty(),
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
            }
        },
        "drift": drift,
    })))
}

#[derive(Debug)]
struct ProjectMatch {
    registered: bool,
    root_matches: bool,
    path_exists: bool,
    actual_roots: Vec<String>,
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
