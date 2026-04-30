use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use tracing::info;

use crate::state::AppState;

pub(super) async fn handle_kb_queue_status(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
    let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
    let status_filter = args_val.get("status").and_then(|v| v.as_str());

    let ops = state
        .store
        .kb_ops_list(plan_id, status_filter)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let summary = if let Some(pid) = plan_id {
        state.store.kb_ops_plan_summary(pid).await.ok()
    } else {
        None
    };

    let mut resp = serde_json::json!({
        "operations": ops,
        "count": ops.len(),
    });
    if let Some(s) = summary {
        resp["summary"] = s;
    }
    Ok(ToolResult::json_pretty(&resp))
}

pub(super) async fn handle_kb_execute_plan(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
    let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
    let limit = args_val.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    let expired = state.store.kb_ops_expire_stale(86400).await.unwrap_or(0);
    if expired > 0 {
        info!(expired, "kb_execute_plan: expired stale pending ops");
    }

    let plan_id = plan_id.ok_or_else(|| anyhow!("plan_id is required"))?;
    let ops = state
        .store
        .kb_ops_list(Some(plan_id), Some("pending"))
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if ops.is_empty() {
        return Ok(ToolResult::text("No pending operations in queue."));
    }

    let batch: Vec<_> = ops.into_iter().take(limit).collect();
    let mut results = Vec::new();

    for op in &batch {
        let _ = state
            .store
            .kb_ops_update_status(&op.id, "running", None, None)
            .await;

        let target_keys: Vec<String> = serde_json::from_str(&op.target_keys).unwrap_or_default();
        let outcome = match op.operation.as_str() {
            "delete" => execute_delete(state, &target_keys).await,
            "update" | "category_fix" | "recategorize" => {
                execute_update(state, &target_keys, op.rationale.as_deref()).await
            }
            "merge" | "distill" => {
                execute_dispatch(
                    state,
                    op.operation.as_str(),
                    &target_keys,
                    op.rationale.as_deref(),
                )
                .await
            }
            other => Err(format!("Unknown operation: {}", other)),
        };

        match outcome {
            Ok(msg) => {
                let (status_str, result_json) = if msg.starts_with("dispatched:") {
                    let task_id = msg.strip_prefix("dispatched:task_id=").unwrap_or(&msg);
                    (
                        "dispatched",
                        serde_json::json!({
                            "id": op.id,
                            "operation": op.operation,
                            "status": "dispatched",
                            "taskId": task_id,
                        }),
                    )
                } else {
                    (
                        "done",
                        serde_json::json!({
                            "id": op.id,
                            "operation": op.operation,
                            "status": "done",
                            "result": msg,
                        }),
                    )
                };
                let _ = state
                    .store
                    .kb_ops_update_status(&op.id, status_str, Some(&msg), None)
                    .await;
                results.push(result_json);
            }
            Err(msg) => {
                let _ = state
                    .store
                    .kb_ops_update_status(&op.id, "failed", None, Some(&msg))
                    .await;
                results.push(serde_json::json!({
                    "id": op.id,
                    "operation": op.operation,
                    "status": "failed",
                    "error": msg,
                }));
            }
        }
    }

    if results
        .iter()
        .any(|r| r.get("status").and_then(|s| s.as_str()) == Some("dispatched"))
    {
        let _ = state
            .bus
            .publish_task(missiond_core::event::events::TaskEvent::Created {
                task_id: String::new(),
            })
            .await;
    }

    let remaining = state
        .store
        .kb_ops_list(Some(plan_id), Some("pending"))
        .await
        .map(|v| v.len())
        .unwrap_or(0);

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "executed": results.len(),
        "results": results,
        "remaining": remaining,
    })))
}

async fn execute_delete(
    state: &AppState,
    target_keys: &[String],
) -> std::result::Result<String, String> {
    let mut deleted = 0usize;
    for key in target_keys {
        if state.store.kb_forget(key).await.unwrap_or(false) {
            deleted += 1;
        }
    }
    Ok(format!("Deleted {}/{} keys", deleted, target_keys.len()))
}

async fn execute_update(
    state: &AppState,
    target_keys: &[String],
    rationale: Option<&str>,
) -> std::result::Result<String, String> {
    let meta: serde_json::Value = rationale
        .and_then(|r| serde_json::from_str(r).ok())
        .unwrap_or_default();
    let new_entry = meta.get("new_entry");
    let key = target_keys
        .first()
        .map(|k| k.as_str())
        .or_else(|| new_entry.and_then(|ne| ne.get("key").and_then(|v| v.as_str())));
    let category = new_entry.and_then(|ne| ne.get("category").and_then(|v| v.as_str()));
    let summary = new_entry.and_then(|ne| ne.get("summary").and_then(|v| v.as_str()));

    match (key, category) {
        (Some(key), Some(cat)) => {
            let input = missiond_core::types::KBRememberInput {
                category: cat.to_string(),
                key: key.to_string(),
                summary: summary.unwrap_or("").to_string(),
                detail: new_entry.and_then(|ne| ne.get("detail").cloned()),
                source: Some("consolidation".to_string()),
                confidence: new_entry.and_then(|ne| ne.get("confidence").and_then(|v| v.as_f64())),
                project_id: None,
            };
            match state.store.kb_remember(&input).await {
                Ok(r) => Ok(format!(
                    "Updated key={} category={} action={}",
                    key, cat, r.action
                )),
                Err(e) => Err(format!("Failed to update: {}", e)),
            }
        }
        _ => Err(
            "update operation requires new_entry with key and category in rationale".to_string(),
        ),
    }
}

async fn execute_dispatch(
    state: &AppState,
    operation: &str,
    target_keys: &[String],
    rationale: Option<&str>,
) -> std::result::Result<String, String> {
    let mut entries_text = String::new();
    for key in target_keys {
        if let Ok(Some(entry)) = state.store.kb_get(key).await {
            entries_text.push_str(&format!(
                "---\nKey: {}\nCategory: {}\nSummary: {}\nDetail: {}\n",
                entry.key,
                entry.category,
                entry.summary,
                entry
                    .detail
                    .as_ref()
                    .map(|d| d.to_string())
                    .unwrap_or_default(),
            ));
        }
    }
    if entries_text.is_empty() {
        return Err(format!("No KB entries found for keys: {:?}", target_keys));
    }

    let rationale = rationale.unwrap_or("");
    let prompt = if operation == "merge" {
        format!(
            "KB整理任务(merge):\n\n原因: {}\n\n以下KB条目内容重叠,请合并为一条。\
            保留最完整的key,用 mission_kb_remember 写入合并后的内容(category/summary/detail),\
            然后用 mission_kb_forget 删除多余的key。\n\n{}",
            rationale, entries_text
        )
    } else {
        format!(
            "KB整理任务(distill):\n\n原因: {}\n\n以下KB条目需要精炼。\
            用 mission_kb_remember 更新每条的 summary(更简洁)和 detail(保留关键信息,删除冗余)。\n\n{}",
            rationale, entries_text
        )
    };
    match crate::state::submit_task(state.store.as_ref(), "memory", &prompt).await {
        Ok(task_id) => Ok(format!("dispatched:task_id={}", task_id)),
        Err(e) => Err(format!("submit failed: {}", e)),
    }
}
