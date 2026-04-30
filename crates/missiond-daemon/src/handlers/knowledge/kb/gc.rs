use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

use super::args::KBGCArgs;

pub(super) async fn handle_kb_gc(state: &AppState, args: Value) -> Result<ToolResult> {
    let KBGCArgs { action, days } = serde_json::from_value(args)?;
    match action.as_str() {
        "stats" => {
            let stats = state
                .store
                .kb_stats()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&stats))
        }
        "stale" => {
            let threshold = days.unwrap_or(30);
            let stale = state
                .store
                .kb_find_stale(threshold)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(&serde_json::json!({
                "threshold_days": threshold,
                "count": stale.len(),
                "entries": stale.iter().map(|e| serde_json::json!({
                    "category": e.category,
                    "key": e.key,
                    "summary": e.summary,
                    "updatedAt": e.updated_at,
                })).collect::<Vec<_>>(),
            })))
        }
        "duplicates" => {
            let dups = state
                .store
                .kb_find_duplicates()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(&serde_json::json!({
                "count": dups.len(),
                "pairs": dups.iter().map(|(a, b, sim)| serde_json::json!({
                    "similarity": format!("{:.2}", sim),
                    "a": {"category": a.category, "key": a.key, "summary": a.summary, "accessCount": a.access_count},
                    "b": {"category": b.category, "key": b.key, "summary": b.summary, "accessCount": b.access_count},
                })).collect::<Vec<_>>(),
            })))
        }
        "clean_stale" => {
            let threshold = days.unwrap_or(30);
            let stale = state
                .store
                .kb_find_stale(threshold)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let keys: Vec<String> = stale.iter().map(|e| e.key.clone()).collect();
            let count = state
                .store
                .kb_batch_forget(&keys)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(&serde_json::json!({
                "action": "clean_stale",
                "threshold_days": threshold,
                "deleted": count,
                "keys": keys,
            })))
        }
        "clean_duplicates" => {
            let dups = state
                .store
                .kb_find_duplicates()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let mut to_delete = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for (a, b, sim) in &dups {
                if seen.contains(&a.key) || seen.contains(&b.key) {
                    continue;
                }
                let loser = if a.access_count > b.access_count {
                    &b.key
                } else if b.access_count > a.access_count {
                    &a.key
                } else if a.updated_at >= b.updated_at {
                    &b.key
                } else {
                    &a.key
                };
                to_delete.push(serde_json::json!({
                    "deleted_key": loser,
                    "kept_key": if loser == &a.key { &b.key } else { &a.key },
                    "similarity": format!("{:.2}", sim),
                }));
                seen.insert(loser.clone());
            }
            let keys: Vec<String> = to_delete
                .iter()
                .filter_map(|d| d["deleted_key"].as_str().map(String::from))
                .collect();
            let count = state
                .store
                .kb_batch_forget(&keys)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(&serde_json::json!({
                "action": "clean_duplicates",
                "deleted": count,
                "details": to_delete,
            })))
        }
        _ => Ok(ToolResult::error(format!(
            "Unknown gc action: {}. Use: stats, stale, duplicates, clean_stale, clean_duplicates",
            action
        ))),
    }
}
