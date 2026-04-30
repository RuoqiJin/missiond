use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

pub(super) fn route_beacon_action(mut args: Value) -> (&'static str, Value) {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    let legacy = match action {
        "map" => "mission_beacon_map",
        "upsert" | "tag" => "mission_beacon_tag",
        "annotate" => "mission_beacon_annotate",
        _ => "mission_beacon_list",
    };

    if legacy == "mission_beacon_tag"
        && args.get("feature").is_none()
        && args.get("name").and_then(|v| v.as_str()).is_some()
    {
        if let Some(obj) = args.as_object_mut() {
            if let Some(name) = obj.get("name").cloned() {
                obj.insert("feature".to_string(), name);
            }
        }
    }
    (legacy, args)
}

pub(super) async fn handle_beacon_list(state: &AppState) -> Result<ToolResult> {
    let beacons = state
        .store
        .beacon_list()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&beacons))
}

pub(super) async fn handle_beacon_map(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct BeaconMapArgs {
        name: String,
    }
    let BeaconMapArgs { name } = serde_json::from_value(args)?;
    let nodes = state
        .store
        .beacon_map(&name)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if nodes.is_empty() {
        return Ok(ToolResult::text(format!(
            "Beacon '{}' not found or has no nodes.",
            name
        )));
    }
    Ok(ToolResult::json_pretty(&serde_json::json!({
        "beacon": name,
        "node_count": nodes.len(),
        "files": nodes.iter().map(|n| &n.file_path).collect::<std::collections::HashSet<_>>().len(),
        "nodes": nodes,
    })))
}

pub(super) async fn handle_beacon_tag(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct BeaconTagArgs {
        file_path: String,
        symbol: String,
        feature: String,
        #[serde(default)]
        annotation: Option<String>,
    }
    let BeaconTagArgs {
        file_path,
        symbol,
        feature,
        annotation,
    } = serde_json::from_value(args)?;

    let source = std::fs::read_to_string(&file_path)
        .map_err(|e| anyhow!("Cannot read file {}: {}", file_path, e))?;

    let mut target_line = None;
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.contains(&format!("fn {}", symbol))
            || trimmed.contains(&format!("struct {}", symbol))
            || trimmed.contains(&format!("enum {}", symbol))
            || trimmed.contains(&format!("trait {}", symbol))
            || trimmed.contains(&format!("impl {}", symbol))
        {
            target_line = Some(idx);
            break;
        }
    }

    let target_line =
        target_line.ok_or_else(|| anyhow!("Symbol '{}' not found in {}", symbol, file_path))?;

    let lines: Vec<&str> = source.lines().collect();
    let already_tagged = if target_line > 0 {
        (0..target_line).rev().take(5).any(|i| {
            let l = lines[i].trim();
            l.starts_with("//") && l.contains("@beacon:") && l.contains(&feature)
        })
    } else {
        false
    };

    if already_tagged {
        return Ok(ToolResult::text(format!(
            "Symbol '{}' already tagged with beacon '{}'.",
            symbol, feature
        )));
    }

    let indent = lines[target_line].len() - lines[target_line].trim_start().len();
    let indent_str: String = lines[target_line].chars().take(indent).collect();

    let mut new_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    new_lines.insert(
        target_line,
        format!("{}// @beacon: {}", indent_str, feature),
    );

    std::fs::write(&file_path, new_lines.join("\n"))
        .map_err(|e| anyhow!("Cannot write file {}: {}", file_path, e))?;

    let beacon_id = state
        .store
        .beacon_ensure(&feature)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let repo_name = std::path::Path::new(&file_path)
        .ancestors()
        .find_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let _ = state
        .store
        .beacon_node_upsert(
            &beacon_id,
            &repo_name,
            &file_path,
            &symbol,
            annotation.as_deref(),
        )
        .await;

    Ok(ToolResult::text(format!(
        "Tagged '{}' with beacon '{}' in {}:{}",
        symbol,
        feature,
        file_path,
        target_line + 1
    )))
}

pub(super) async fn handle_beacon_annotate(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct BeaconAnnotateArgs {
        beacon_name: String,
        file_path: String,
        symbol: String,
        annotation: String,
    }
    let BeaconAnnotateArgs {
        beacon_name,
        file_path,
        symbol,
        annotation,
    } = serde_json::from_value(args)?;

    let repo_name = std::path::Path::new(&file_path)
        .ancestors()
        .find_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let updated = state
        .store
        .beacon_node_annotate(&beacon_name, &repo_name, &file_path, &symbol, &annotation)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if updated {
        Ok(ToolResult::text(format!(
            "Annotation updated for {}::{} in beacon '{}'.",
            file_path, symbol, beacon_name
        )))
    } else {
        Ok(ToolResult::text(format!(
            "No matching beacon node found for {}::{} in beacon '{}'.",
            file_path, symbol, beacon_name
        )))
    }
}
