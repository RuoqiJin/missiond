use anyhow::{anyhow, Result};
use chrono::Utc;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::context::effects::{self, EffectContext};
use crate::state::AppState;

const PROJECT_VAULT_EFFECT: EffectContext =
    EffectContext::new("project-vault-sync", "project-vault-sync-write");

pub(super) async fn handle_vault_sync(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = required_str(&args, "id")?;
    let project = state
        .store
        .get_project(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow!("Project not found: {}", id))?;

    let source_root = std::path::Path::new(&project.path);
    if !source_root.exists() {
        return Ok(ToolResult::error(format!(
            "Source path does not exist: {}",
            project.path
        )));
    }

    let vault_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".missiond/vault")
        .join(id);
    std::fs::create_dir_all(&vault_dir)
        .map_err(|e| anyhow!("Failed to create vault dir: {}", e))?;

    let mut synced = 0u32;
    let scan_dirs = [
        source_root.to_path_buf(),
        source_root.join(".missiond"),
        source_root.join(".jarvis"),
    ];
    for scan_dir in &scan_dirs {
        if !scan_dir.exists() {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(scan_dir) {
            for entry in rd.filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.extension().map(|e| e == "lisp").unwrap_or(false) {
                    let rel = p.strip_prefix(source_root).unwrap_or(&p);
                    let dest = vault_dir.join(rel);
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if std::fs::copy(&p, &dest).is_ok() {
                        synced += 1;
                    }
                }
            }
        }
    }

    let meta = serde_json::json!({
        "source_path": project.path,
        "synced_at": Utc::now().to_rfc3339(),
        "file_count": synced,
    });
    let _ = effects::write_text(
        PROJECT_VAULT_EFFECT,
        &vault_dir.join("_meta.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );

    let mut updated = project.clone();
    updated.vault_path = Some(vault_dir.display().to_string());
    updated.kind = "reference".to_string();
    let _ = state.store.upsert_project(&updated).await;

    Ok(ToolResult::json(&serde_json::json!({
        "id": id,
        "vault_dir": vault_dir.display().to_string(),
        "synced_files": synced,
    })))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("{} is required", key))
}
