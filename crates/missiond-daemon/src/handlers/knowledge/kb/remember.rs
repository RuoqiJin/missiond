use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::{AppState, EmbeddingTask};

use super::args::KBRememberArgs;
use super::conflicts::detect_kb_conflicts;
use super::quality::check_content_quality;

pub(super) async fn handle_kb_remember(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: KBRememberArgs = serde_json::from_value(args)?;
    if let Some(rejection) =
        check_content_quality(&args.summary, &args.detail, Some(&args.category))
    {
        return Ok(ToolResult::error(&rejection));
    }
    let input = missiond_core::types::KBRememberInput {
        category: args.category,
        key: args.key,
        summary: args.summary,
        detail: args.detail,
        source: args.source,
        confidence: args.confidence,
        project_id: args.project.clone(),
    };
    let result = state
        .store
        .kb_remember(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let _ = state
        .embedding_tx
        .try_send(EmbeddingTask::ProcessKBEntry(result.entry.id.clone()));

    if result.action == "created" || result.action == "updated" {
        if let Some(ref detail) = result.entry.detail {
            if let Some(from_keys) = detail.get("consolidated_from").and_then(|v| v.as_array()) {
                for key_val in from_keys {
                    if let Some(key) = key_val.as_str() {
                        if let Ok(Some(target_id)) = state.store.kb_get_id_by_key(key).await {
                            let _ = state
                                .store
                                .kb_add_edge(&result.entry.id, &target_id, "supersedes", 1.0)
                                .await;
                        }
                    }
                }
            }
        }
    }

    if result.action == "created" || result.action == "updated" {
        if let Some(ref detail) = result.entry.detail {
            let symbol = detail.get("symbol").and_then(|v| v.as_str());
            let file_hint = detail.get("file_hint").and_then(|v| v.as_str());
            if let Some(sym) = symbol {
                let ast_node_id = state
                    .store
                    .ast_find_related(sym, 1)
                    .await
                    .ok()
                    .and_then(|hits| hits.into_iter().next().map(|h| h.id));
                let _ = state
                    .store
                    .kb_add_ast_link(
                        &result.entry.id,
                        sym,
                        file_hint,
                        ast_node_id.as_deref(),
                        "related_to",
                        0.8,
                    )
                    .await;
            }
        }
    }

    let _ = state
        .bus
        .publish_memory(missiond_core::event::events::MemoryEvent::KBBatchMutated {
            count: 1,
            categories: vec![input.category.clone()],
            action: result.action.clone(),
        })
        .await;

    let conflicts = if result.action == "created" {
        detect_kb_conflicts(state, &result.entry).await
    } else {
        vec![]
    };

    if conflicts.is_empty() {
        Ok(ToolResult::json_pretty(&result))
    } else {
        let max_conflict_conf = conflicts
            .iter()
            .filter_map(|c| c["confidence"].as_f64())
            .fold(0.0f64, f64::max);
        if max_conflict_conf > result.entry.confidence {
            let reduced = (max_conflict_conf / 2.0).max(0.1);
            let _ = state
                .store
                .kb_adjust_confidence(&result.entry.id, reduced - result.entry.confidence)
                .await;
        }

        for c in &conflicts {
            if let Some(cid) = c.get("id").and_then(|v| v.as_str()) {
                let _ = state
                    .store
                    .kb_add_edge(&result.entry.id, cid, "contradicts", 0.8)
                    .await;
            }
        }

        let mut output = serde_json::to_value(&result)?;
        output["conflicts"] = serde_json::json!(conflicts);
        output["conflictWarning"] = serde_json::json!(format!(
            "⚠️ 检测到 {} 条语义相似的已有条目，可能存在规则冲突。新条目已自动降权。请用 mission_kb_update 合并或 mission_kb_forget 删除冲突方。",
            conflicts.len()
        ));
        Ok(ToolResult::json_pretty(&output))
    }
}
