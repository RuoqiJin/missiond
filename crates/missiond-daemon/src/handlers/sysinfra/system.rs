use anyhow::Result;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;
use std::path::{Path, PathBuf};

use crate::state::AppState;

/// Allowed config files (whitelist for safety)
const ALLOWED_CONFIGS: &[&str] = &["slots.yaml", "llm.yaml", "permissions.yaml"];

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_sys_logs" => sys_logs(args).await,
        "mission_sys_config" => sys_config(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown system tool: {}", name))),
    }
}

// ── sys_config: structured config read/patch ──

async fn sys_config(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args.get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("get");

    match action {
        "get" => sys_config_get(args).await,
        "patch" => sys_config_patch(state, args).await,
        "list" => {
            Ok(ToolResult::text(format!("Available config files: {}", ALLOWED_CONFIGS.join(", "))))
        }
        _ => Ok(ToolResult::error(format!("Unknown sys_config action: {}. Use: get, patch, list", action))),
    }
}

fn resolve_config_path(file: &str) -> std::result::Result<PathBuf, String> {
    // Strip path components — only allow bare filenames from whitelist
    let basename = Path::new(file)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if !ALLOWED_CONFIGS.contains(&basename.as_str()) {
        return Err(format!(
            "Config file '{}' not in whitelist. Allowed: {}",
            file,
            ALLOWED_CONFIGS.join(", ")
        ));
    }

    let home = missiond_core::ipc::default_mission_home();
    Ok(home.join(&basename))
}

async fn sys_config_get(args: Value) -> Result<ToolResult> {
    let file = args.get("file")
        .and_then(|v| v.as_str())
        .unwrap_or("slots.yaml");

    let path = match resolve_config_path(file) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };

    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => return Ok(ToolResult::error(format!("Cannot read {}: {}", path.display(), e))),
    };

    // Parse YAML → JSON for structured access
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(v) => v,
        Err(e) => return Ok(ToolResult::error(format!("YAML parse error: {}", e))),
    };

    let json_value: Value = serde_json::to_value(&yaml_value)
        .unwrap_or(Value::String(content));

    Ok(ToolResult::json_pretty(&json_value))
}

async fn sys_config_patch(state: &AppState, args: Value) -> Result<ToolResult> {
    let file = match args.get("file").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return Ok(ToolResult::error("'file' is required for patch")),
    };
    let pointer = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Ok(ToolResult::error("'path' (JSON Pointer, e.g. /slots/0/description) is required")),
    };
    let new_value = match args.get("value") {
        Some(v) => v.clone(),
        None => return Ok(ToolResult::error("'value' is required for patch")),
    };

    let path = match resolve_config_path(file) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };

    // Acquire per-file lock to prevent concurrent TOCTOU races
    let file_lock = {
        let mut locks = state.config_file_locks.lock().await;
        locks.entry(file.to_string())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _guard = file_lock.lock().await;

    // Read current content
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => return Ok(ToolResult::error(format!("Cannot read {}: {}", path.display(), e))),
    };

    // Parse YAML → JSON, apply patch, serialize back
    // NOTE: YAML comments and formatting are lost in round-trip. This is accepted;
    // fsnotify-triggered hot-reload ensures the daemon picks up changes immediately.
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(v) => v,
        Err(e) => return Ok(ToolResult::error(format!("YAML parse error: {}", e))),
    };

    let mut json_doc: Value = serde_json::to_value(&yaml_value)
        .map_err(|e| anyhow::anyhow!("YAML→JSON conversion error: {}", e))?;

    // Apply JSON Pointer patch
    let target = match json_doc.pointer_mut(pointer) {
        Some(t) => t,
        None => return Ok(ToolResult::error(format!("JSON Pointer '{}' not found in {}", pointer, file))),
    };
    *target = new_value.clone();

    // Serialize JSON back to YAML
    let new_yaml = match serde_yaml::to_string(&json_doc) {
        Ok(y) => y,
        Err(e) => return Ok(ToolResult::error(format!("JSON→YAML serialization error: {}", e))),
    };

    // Write back
    if let Err(e) = tokio::fs::write(&path, &new_yaml).await {
        return Ok(ToolResult::error(format!("Cannot write {}: {}", path.display(), e)));
    }

    // Trigger hot-reload (fsnotify will pick it up, but also signal directly)
    if file.contains("slots") {
        if let Ok(result) = state.mission.reload_slots_config() {
            if result.has_changes() {
                return Ok(ToolResult::text(format!(
                    "Patched {} at '{}' → {:?}. Slots reloaded: {} added, {} removed, {} updated. (⚠️ YAML comments lost in round-trip)",
                    file, pointer, new_value,
                    result.added.len(), result.removed.len(), result.updated.len()
                )));
            }
        }
    }

    Ok(ToolResult::text(format!(
        "Patched {} at '{}' → {:?}. File saved (hot-reload via fsnotify). (⚠️ YAML comments lost in round-trip)",
        file, pointer, new_value
    )))
}

// ── sys_logs: daemon log introspection ──

async fn sys_logs(args: Value) -> Result<ToolResult> {
    let lines = args.get("lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(500) as usize;
    let level = args.get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let grep = args.get("grep")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    // Find log file in ~/.xjp-mission/logs/missiond.log.*
    let log_dir = std::env::var("MISSIOND_LOG_FILE").unwrap_or_else(|_| {
        let home = missiond_core::ipc::default_mission_home();
        home.join("logs").to_string_lossy().to_string()
    });

    let log_path = match find_latest_log(&log_dir) {
        Some(p) => p,
        None => return Ok(ToolResult::error(format!("No log files found in {}", log_dir))),
    };

    let content = match tokio::fs::read_to_string(&log_path).await {
        Ok(c) => c,
        Err(e) => return Ok(ToolResult::error(format!("Cannot read log file {}: {}", log_path, e))),
    };

    let all_lines: Vec<&str> = content.lines().collect();
    let tail_start = all_lines.len().saturating_sub(lines * 3);
    let tail = &all_lines[tail_start..];

    let filtered: Vec<&str> = tail.iter().copied().filter(|line| {
        let level_ok = match level {
            "error" => line.contains("ERROR") || line.contains(" error ") || line.contains("error:"),
            "warn" => line.contains("WARN") || line.contains("ERROR")
                || line.contains(" warn ") || line.contains(" error "),
            _ => true,
        };
        let grep_ok = match &grep {
            Some(kw) => line.to_lowercase().contains(kw),
            None => true,
        };
        level_ok && grep_ok
    }).collect();

    let result: Vec<&str> = filtered.into_iter()
        .rev().take(lines).collect::<Vec<_>>()
        .into_iter().rev().collect();

    if result.is_empty() {
        Ok(ToolResult::text(format!("No log entries found (file: {}, level: {}, lines: {})", log_path, level, lines)))
    } else {
        Ok(ToolResult::text(format!(
            "[{} lines from {}]\n{}",
            result.len(),
            log_path,
            result.join("\n")
        )))
    }
}

/// Find the most recent log file in the logs directory.
fn find_latest_log(log_dir: &str) -> Option<String> {
    let dir = Path::new(log_dir);
    if !dir.is_dir() {
        return None;
    }
    let mut logs: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("missiond.log")
        })
        .collect();
    logs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    logs.first().map(|e| e.path().to_string_lossy().to_string())
}
