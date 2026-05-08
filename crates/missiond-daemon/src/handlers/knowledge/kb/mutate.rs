use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::{AppState, EmbeddingTask};

use super::args::{KBKeyArgs, KBUpdateArgs};
use super::quality::check_content_quality;

pub(super) async fn handle_kb_forget(state: &AppState, args: Value) -> Result<ToolResult> {
    let KBKeyArgs { key, .. } = serde_json::from_value(args)?;
    // Get entry ID before deletion for cache invalidation
    let entry_id = state.store.kb_get_id_by_key(&key).await.ok().flatten();
    let deleted = state
        .store
        .kb_forget(&key)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    // Remove from embedding cache if deleted
    if deleted {
        if let Some(ref id) = entry_id {
            let mut guard = state.embedding_cache.write().await;
            guard.retain(|(eid, _)| eid != id);
            // Clean up knowledge graph edges + AST links
            let _ = state.store.kb_delete_edges_for(id).await;
            let _ = state.store.kb_delete_ast_links_for(id).await;
        }
    }
    // Emit KBBatchMutated for event-driven FTS rebuild
    if deleted {
        let _ = state
            .bus
            .publish_memory(missiond_core::event::events::MemoryEvent::KBBatchMutated {
                count: 1,
                categories: vec![],
                action: "deleted".to_string(),
            })
            .await;
    }
    Ok(ToolResult::json(&serde_json::json!({
        "deleted": deleted,
        "key": key,
    })))
}

pub(super) async fn handle_kb_batch_forget(state: &AppState, args: Value) -> Result<ToolResult> {
    let keys_val = args.get("keys").cloned().unwrap_or(Value::Array(vec![]));
    let keys: Vec<String> = match serde_json::from_value::<Vec<String>>(keys_val.clone()) {
        Ok(v) => v,
        Err(_) => {
            // Claude may pass JSON string "[\"a\",\"b\"]" or comma-separated "a,b,c"
            if let Some(s) = keys_val.as_str() {
                serde_json::from_str::<Vec<String>>(s).unwrap_or_else(|_| {
                    s.split(',')
                        .map(|k| k.trim().to_string())
                        .filter(|k| !k.is_empty())
                        .collect()
                })
            } else {
                return Ok(ToolResult::error("keys: expected array or JSON string"));
            }
        }
    };
    if keys.is_empty() {
        return Ok(ToolResult::error("keys array is empty"));
    }
    let count = state
        .store
        .kb_batch_forget(&keys)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    // Emit KBBatchMutated for event-driven consumers
    if count > 0 {
        let _ = state
            .bus
            .publish_memory(missiond_core::event::events::MemoryEvent::KBBatchMutated {
                count: count as u32,
                categories: vec![],
                action: "deleted".to_string(),
            })
            .await;
    }
    Ok(ToolResult::json(&serde_json::json!({
        "deleted_count": count,
        "requested_keys": keys.len(),
    })))
}

pub(super) async fn handle_kb_batch_set_project(
    state: &AppState,
    args: Value,
) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct Assignment {
        key: String,
        project_id: Option<String>,
    }
    #[derive(Deserialize)]
    struct BatchArgs {
        assignments: Vec<Assignment>,
    }
    let args: BatchArgs = serde_json::from_value(args)?;
    let mut updated = 0usize;
    let mut not_found = Vec::new();
    for a in &args.assignments {
        let pid = a.project_id.as_deref().filter(|s| !s.is_empty());
        match state
            .store
            .kb_update(&a.key, None, None, None, None, None, pid)
            .await
        {
            Ok(Some(_)) => updated += 1,
            Ok(None) => not_found.push(a.key.clone()),
            Err(_) => not_found.push(a.key.clone()),
        }
    }
    Ok(ToolResult::json_pretty(&serde_json::json!({
        "updated": updated,
        "not_found": not_found,
        "total": args.assignments.len(),
    })))
}

pub(super) async fn handle_kb_update(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: KBUpdateArgs = serde_json::from_value(args)?;
    // Content quality check only if summary is being updated
    if let Some(ref summary) = args.summary {
        if let Some(rejection) =
            check_content_quality(summary, &args.detail, args.category.as_deref())
        {
            return Ok(ToolResult::error(&rejection));
        }
    }
    let result = state
        .store
        .kb_update(
            &args.key,
            args.category.as_deref(),
            args.summary.as_deref(),
            args.detail.as_ref(),
            args.confidence,
            args.linked_task_id.as_deref(),
            args.project_id.as_deref(),
        )
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    match result {
        Some((entry, content_changed)) => {
            // Only re-embed if summary/detail changed
            if content_changed {
                let _ = state
                    .embedding_tx
                    .try_send(EmbeddingTask::ProcessKBEntry(entry.id.clone()));
            }
            // Emit KBBatchMutated for event-driven consumers
            let _ = state
                .bus
                .publish_memory(missiond_core::event::events::MemoryEvent::KBBatchMutated {
                    count: 1,
                    categories: vec![entry.category.clone()],
                    action: "updated".to_string(),
                })
                .await;
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "updated": true,
                "content_changed": content_changed,
                "entry": entry,
            })))
        }
        None => Ok(ToolResult::error(&format!("key '{}' not found", args.key))),
    }
}
