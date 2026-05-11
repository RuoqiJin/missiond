use anyhow::{anyhow, Result};
use missiond_core::types::KnowledgeReviewInput;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::{AppState, EmbeddingTask};

use super::args::{KBBatchRememberArgs, KBRememberArgs};
use super::conflicts::detect_kb_conflicts;
use super::quality::check_content_quality;

fn conflict_similarity(conflict: &Value) -> f64 {
    conflict
        .get("similarity")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

async fn write_duplicate_review_artifact(
    state: &AppState,
    result: &missiond_core::types::KBRememberResult,
    conflicts: &[Value],
) {
    if result.action != "created" || conflicts.is_empty() || result.entry.confidence > 0.7 {
        return;
    }

    let strongest = conflicts
        .iter()
        .max_by(|a, b| {
            conflict_similarity(a)
                .partial_cmp(&conflict_similarity(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned();
    let superseded_by = strongest
        .as_ref()
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    let input = KnowledgeReviewInput {
        knowledge_id: result.entry.id.clone(),
        state: "needs-human".to_string(),
        batch_id: "kb-dedupe-gate".to_string(),
        reviewer: "missiond-kb-dedupe-gate".to_string(),
        rationale: "Low-confidence semantic duplicate candidate from shared KB write gate; retained as evidence but removed from default active retrieval until reviewed.".to_string(),
        evidence_refs: serde_json::json!({
            "new_key": result.entry.key,
            "new_category": result.entry.category,
            "conflicts": conflicts,
            "gate": "remember_one.semantic_duplicate_review_artifact"
        }),
        superseded_by,
        confidence: 0.75,
        applied_at: None,
    };

    let _ = state.store.kb_review_upsert(&input).await;
}

async fn remember_one(
    state: &AppState,
    args: KBRememberArgs,
    publish_event: bool,
) -> Result<Value> {
    if let Some(rejection) =
        check_content_quality(&args.summary, &args.detail, Some(&args.category))
    {
        return Ok(serde_json::json!({ "error": rejection }));
    }
    let category = args.category.clone();
    let input = missiond_core::types::KBRememberInput {
        category: category.clone(),
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

    if publish_event {
        let _ = state
            .bus
            .publish_memory(missiond_core::event::events::MemoryEvent::KBBatchMutated {
                count: 1,
                categories: vec![category],
                action: result.action.clone(),
            })
            .await;
    }

    let conflicts = if result.action == "created" {
        detect_kb_conflicts(state, &result.entry).await
    } else {
        vec![]
    };
    write_duplicate_review_artifact(state, &result, &conflicts).await;

    if conflicts.is_empty() {
        Ok(serde_json::to_value(&result)?)
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
        Ok(output)
    }
}

async fn record_deep_analysis_kb_output(state: &AppState, count: u32) {
    if count == 0 {
        return;
    }
    let mut es = state.slow_extraction_state.write().await;
    if es.active_type == Some("deep_analysis") {
        es.add_current_output_count(count);
    }
}

pub(super) async fn handle_kb_remember(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: KBRememberArgs = serde_json::from_value(args)?;
    let output = remember_one(state, args, true).await?;
    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        Ok(ToolResult::error(error))
    } else {
        record_deep_analysis_kb_output(state, 1).await;
        Ok(ToolResult::json_pretty(&output))
    }
}

pub(super) async fn handle_kb_remember_batch(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: KBBatchRememberArgs = serde_json::from_value(args)?;
    if args.entries.is_empty() {
        return Ok(ToolResult::error(
            "mission_kb_mutate(action=batch_remember) requires a non-empty entries array",
        ));
    }
    if args.entries.len() > 50 {
        return Ok(ToolResult::error(
            "mission_kb_mutate(action=batch_remember) accepts at most 50 entries per call",
        ));
    }

    let mut ok_count = 0usize;
    let mut categories = std::collections::BTreeSet::new();
    let mut results = Vec::with_capacity(args.entries.len());
    for (index, entry) in args.entries.into_iter().enumerate() {
        let category = entry.category.clone();
        match remember_one(state, entry, false).await {
            Ok(output) if output.get("error").is_none() => {
                ok_count += 1;
                categories.insert(category);
                results.push(serde_json::json!({
                    "index": index,
                    "ok": true,
                    "result": output,
                }));
            }
            Ok(output) => {
                results.push(serde_json::json!({
                    "index": index,
                    "ok": false,
                    "error": output.get("error").cloned().unwrap_or_else(|| serde_json::json!("unknown rejection")),
                }));
            }
            Err(err) => {
                results.push(serde_json::json!({
                    "index": index,
                    "ok": false,
                    "error": err.to_string(),
                }));
            }
        }
    }

    if ok_count > 0 {
        record_deep_analysis_kb_output(state, ok_count as u32).await;
        let _ = state
            .bus
            .publish_memory(missiond_core::event::events::MemoryEvent::KBBatchMutated {
                count: ok_count as u32,
                categories: categories.into_iter().collect(),
                action: "batch_remember".to_string(),
            })
            .await;
    }

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "schema": "missiond.kb.batch-remember.v1",
        "total": results.len(),
        "okCount": ok_count,
        "errorCount": results.len().saturating_sub(ok_count),
        "results": results,
    })))
}
