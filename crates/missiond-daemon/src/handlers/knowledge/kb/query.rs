use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

use super::args::{KBKeyArgs, KBListArgs};

pub(super) async fn handle_kb_get(state: &AppState, args: Value) -> Result<ToolResult> {
    let KBKeyArgs { key } = serde_json::from_value(args)?;
    let entry = state
        .store
        .kb_get(&key)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    match entry {
        Some(e) => Ok(ToolResult::json_pretty(&e)),
        None => Ok(ToolResult::error(format!("Key not found: {}", key))),
    }
}

pub(super) async fn handle_kb_list(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_parsed: KBListArgs = serde_json::from_value(args).unwrap_or(KBListArgs {
        category: None,
        limit: 50,
        offset: 0,
        compact: false,
    });
    let entries = state
        .store
        .kb_list_paginated(
            args_parsed.category.as_deref(),
            args_parsed.limit,
            args_parsed.offset,
        )
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if args_parsed.compact {
        let compact: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "category": e.category,
                    "summary": if e.summary.chars().count() > 120 {
                        format!("{}...", e.summary.chars().take(120).collect::<String>())
                    } else {
                        e.summary.clone()
                    },
                    "updatedAt": e.updated_at,
                    "projectId": e.project_id,
                })
            })
            .collect();
        Ok(ToolResult::json_pretty(&serde_json::json!({
            "total": compact.len(),
            "compact": true,
            "entries": compact,
        })))
    } else {
        Ok(ToolResult::json_pretty(&entries))
    }
}
