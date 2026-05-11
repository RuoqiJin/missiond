use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use tracing::info;

use crate::context_budget::apply_context_budget;
use crate::context_budget::MAX_ROUTER_PAYLOAD_BYTES;
use crate::embedding_worker::resolve_llm_credentials;
use crate::gemini_client::REQUEST_CALLER;
use crate::state::AppState;

pub(super) async fn handle_kb_analyze(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
    let mode = args_val
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("overview");
    let target_category = args_val.get("target_category").and_then(|v| v.as_str());
    let limit = args_val
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(500) as u32;
    let offset = args_val.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let custom_prompt = args_val.get("custom_prompt").and_then(|v| v.as_str());
    let include_board_context = args_val
        .get("include_board_context")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let model: String = args_val
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.1-pro")
        .to_string();
    let max_tokens: u32 = args_val
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(16384) as u32;

    let entries = state
        .store
        .kb_list_paginated(target_category, limit, offset)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if entries.is_empty() {
        return Ok(ToolResult::error(
            "No KB entries found for the given filter.",
        ));
    }

    let total_count = state
        .store
        .kb_list(target_category.map(|s| s))
        .await
        .map(|v| v.len())
        .unwrap_or(0);

    let now = chrono::Utc::now();
    let mut jsonl_lines = Vec::with_capacity(entries.len());
    let include_detail = mode == "consolidation_plan";

    for e in &entries {
        if e.category == "credential" {
            continue;
        }
        let sanitized_summary = missiond_core::db::shared::redact_sensitive(&e.summary);
        let age_days = chrono::DateTime::parse_from_rfc3339(&e.updated_at)
            .map(|dt| (now - dt.with_timezone(&chrono::Utc)).num_days())
            .unwrap_or(0);

        let mut item = serde_json::json!({
            "category": e.category,
            "key": e.key,
            "summary": sanitized_summary,
            "access_count": e.access_count,
            "age_days": age_days,
            "confidence": e.confidence,
        });

        if include_detail {
            if let Some(detail) = &e.detail {
                let detail_str = match detail {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let sanitized_detail = missiond_core::db::shared::redact_sensitive(&detail_str);
                item["detail"] = serde_json::Value::String(sanitized_detail);
            }
        }

        jsonl_lines.push(serde_json::to_string(&item).unwrap_or_default());
    }
    let kb_jsonl = jsonl_lines.join("\n");

    let board_context = if include_board_context {
        let tasks = state
            .store
            .list_board_tasks(None, false)
            .await
            .unwrap_or_default();
        let mut open_lines = Vec::new();
        let mut done_lines = Vec::new();
        for t in &tasks {
            let line = format!("{} {}", &t.id.as_str()[..8], t.title);
            match t.status {
                missiond_core::types::BoardTaskStatus::Done => done_lines.push(line),
                _ => open_lines.push(line),
            }
        }
        Some(format!(
            "[Board Tasks Context]\n<open>\n{}\n<done>\n{}",
            open_lines.join("\n"),
            done_lines.join("\n")
        ))
    } else {
        None
    };

    let mut response_format: Option<serde_json::Value> = None;
    let analysis_prompt = match mode {
        "consolidation_plan" => {
            response_format = Some(serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "kb_consolidation_actions",
                    "strict": false,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "actions": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "action_type": { "type": "string", "enum": ["merge", "delete", "update", "distill"] },
                                        "target_keys": {
                                            "type": "array",
                                            "items": { "type": "string" },
                                            "description": "Keys of entries to delete or merge"
                                        },
                                        "new_entry": {
                                            "type": "object",
                                            "properties": {
                                                "category": { "type": "string" },
                                                "key": { "type": "string" },
                                                "summary": { "type": "string" },
                                                "confidence": { "type": "number" }
                                            },
                                            "required": ["category", "key", "summary"]
                                        },
                                        "linked_task_id": {
                                            "type": "string",
                                            "description": "Matched Board Task ID (short, 8-char prefix)"
                                        },
                                        "reason": { "type": "string" }
                                    },
                                    "required": ["action_type", "target_keys", "reason"]
                                }
                            }
                        },
                        "required": ["actions"]
                    }
                }
            }));

            let board_section = if let Some(ref ctx) = board_context {
                format!(
                    "\n\n=== BOARD TASKS CONTEXT ===\n{}\n===========================\n\n\
                    === 任务感知整理规则 ===\n\
                    1. 关联 Open 任务的 KB → 保护，不删不合并，仅用 update 补 linked_task_id\n\
                    2. 关联 Done 任务的 KB → distill 蒸馏收敛（提取最终结论升维到 architecture/feature，删流水账）\n\
                    3. 无关联 Orphan → preference/ops/platform 保留；老旧 debug/bugfix 可删除\n\
                    4. 每个 action 的 reason 中说明关联了哪个任务（或标注 orphan）\n",
                    ctx
                )
            } else {
                String::new()
            };

            format!(
                "你是 MissionD 的知识库自治整理引擎。分析传入的 JSONL 条目，生成可执行的清理计划。\n\n\
                规则：\n\
                1. 重复/相似合并 (merge)：相同主题不同措辞 → 合并，保留最高 confidence，汇总 summary\n\
                2. 碎片整合 (update)：松散相关条目 → 整合成连贯大条目\n\
                3. 过时清理 (delete)：基于 age_days 和 access_count，旧策略已被覆盖的条目\n\
                4. 类别修正 (update)：category 放错的条目 → 移到正确分类\n\
                5. 蒸馏收敛 (distill)：已完成项目的多条流水账 → 提取最终结论为一条精华\n\n\
                保守原则：不确定就不动。每个 action 必须有 reason。\
                {}\n\n共 {} 条（第 {} 到 {} 条）：\n{}",
                board_section,
                entries.len(),
                offset + 1,
                offset + entries.len() as u32,
                kb_jsonl
            )
        }
        "custom" => {
            format!(
                "{}\n\n知识库数据（共 {} 条，JSONL格式）：\n{}",
                custom_prompt.unwrap_or("请分析以下知识库数据。"),
                entries.len(),
                kb_jsonl
            )
        }
        _ => {
            format!(
                "作为 MissionD 核心系统的知识管理专家，请深度审查以下知识库（KB）条目。\n\n
请务必重点完成以下两项任务，并给出具体的、可操作的建议：\n\n
1. 【冗余与合并分析】\n
- 识别同一问题的多阶段记录、重复的调试流水账（尤其是频繁触发的遗留 bug 记录）。\n
- 明确列出建议合并保留的主 key 和建议删除的冗余 key 列表。\n\n
2. 【类别纠偏与升维建议】\n
- 严格对照系统的生命周期与分类约束：\n
  * preference: 用户偏好/纠正/否定 (长期保留保护)\n
  * memory:decision / memory:architecture: 架构决策/核心技术事实 (长期保留保护)\n
  * project: 项目专属上下文 (长期保留保护)\n
  * memory:bugfix: 已修复 bug 的根因分析 (30天 GC)\n
  * memory:debug: 调试弯路/临时排查经验 (短周期 GC)\n
  * memory:ops: 运维基建/CI脚本/痛点信号\n
- 找出被埋没或错放的条目。例如：误记在 debug/bugfix 中但实际是高价值架构决策的条目（面临被误删风险，应升维至 decision）；或属于用户习惯却混入普通 memory 的条目（应移至 preference）。
- 列出需要修改 category 的条目 key，并给出建议的新 category 及简短理由。\n\n
附带的知识库数据如下（JSONL格式，共 {} 条）：\n{}",
                entries.len(),
                kb_jsonl
            )
        }
    };

    let (base_url, jwt) = resolve_llm_credentials().await?;

    let mut analysis_messages: Vec<Value> =
        vec![serde_json::json!({"role": "user", "content": analysis_prompt})];
    let budget_result = apply_context_budget(&mut analysis_messages, MAX_ROUTER_PAYLOAD_BYTES);
    if budget_result.trimmed {
        info!(
            "KB analyze: context budget applied — {}",
            budget_result.note.as_deref().unwrap_or("trimmed")
        );
    }

    let url = format!("{}/v1/chat/completions", base_url);
    let mut body = serde_json::json!({
        "model": model,
        "messages": analysis_messages,
        "max_tokens": max_tokens,
    });
    if let Some(fmt) = &response_format {
        body.as_object_mut()
            .unwrap()
            .insert("response_format".to_string(), fmt.clone());
    }

    info!(
        "KB analyze [{}]: sending {} entries ({} chars) to {} via {}",
        mode,
        entries.len(),
        kb_jsonl.len(),
        model,
        url
    );

    let idle_timeout = if kb_jsonl.len() > 50_000 {
        Some(std::time::Duration::from_secs(300))
    } else {
        None
    };
    let result = REQUEST_CALLER
        .scope("kb_analyze".to_string(), async {
            state
                .gemini
                .send_with_timeout(&state.http_client, &url, &jwt, &body, idle_timeout)
                .await
        })
        .await?;

    let content = result
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("(empty response)");
    let finish_reason = result
        .pointer("/choices/0/finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let usage = result.get("usage");
    let resp_model = result
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(&model);

    let mut resp = serde_json::json!({
        "model": resp_model,
        "mode": mode,
        "entries_in_request": entries.len(),
        "total_entries": total_count,
        "offset": offset,
        "has_more": (offset as usize + entries.len()) < total_count,
        "usage": usage,
    });

    let save_plan = args_val
        .get("save_plan")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if mode == "consolidation_plan" {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
            resp["plan"] = parsed.clone();
            if save_plan {
                if let Some(actions) = parsed.get("actions").and_then(|a| a.as_array()) {
                    let plan_id = uuid::Uuid::new_v4().to_string();
                    let task_id_param = args_val.get("task_id").and_then(|v| v.as_str());
                    let ops: Vec<missiond_core::types::KBOperation> = actions
                        .iter()
                        .filter_map(|a| {
                            let operation = a
                                .get("action_type")
                                .and_then(|v| v.as_str())
                                .or_else(|| a.get("action").and_then(|v| v.as_str()))
                                .or_else(|| a.get("operation").and_then(|v| v.as_str()))?;
                            let keys: Vec<String> = a
                                .get("target_keys")
                                .and_then(|v| v.as_array())
                                .or_else(|| a.get("keys").and_then(|v| v.as_array()))
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|k| k.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .or_else(|| {
                                    a.get("key")
                                        .and_then(|v| v.as_str())
                                        .map(|k| vec![k.to_string()])
                                })?;
                            Some(missiond_core::types::KBOperation {
                                operation: operation.to_string(),
                                target_keys: keys,
                                rationale: {
                                    let reason = a
                                        .get("reason")
                                        .and_then(|v| v.as_str())
                                        .or_else(|| a.get("rationale").and_then(|v| v.as_str()));
                                    if operation == "update"
                                        || operation == "recategorize"
                                        || operation == "category_fix"
                                    {
                                        let mut meta = serde_json::Map::new();
                                        if let Some(r) = reason {
                                            meta.insert("reason".into(), serde_json::json!(r));
                                        }
                                        if let Some(ne) = a.get("new_entry") {
                                            meta.insert("new_entry".into(), ne.clone());
                                        }
                                        Some(serde_json::to_string(&meta).unwrap_or_default())
                                    } else {
                                        reason.map(|s| s.to_string())
                                    }
                                },
                            })
                        })
                        .collect();
                    if !ops.is_empty() {
                        match state
                            .store
                            .kb_ops_save_plan(&plan_id, task_id_param, &ops)
                            .await
                        {
                            Ok(n) => {
                                resp["plan_id"] = serde_json::json!(plan_id);
                                resp["operations_saved"] = serde_json::json!(n);
                                info!(plan_id = %plan_id, ops = n, "KB consolidation plan saved to queue");
                            }
                            Err(e) => {
                                resp["save_error"] =
                                    serde_json::json!(format!("Failed to save plan: {}", e));
                            }
                        }
                    }
                }
            }
        } else {
            resp["analysis"] = serde_json::Value::String(content.to_string());
            resp["parse_warning"] =
                serde_json::json!("Response was not valid JSON. Returned as text.");
        }
    } else {
        resp["analysis"] = serde_json::Value::String(content.to_string());
    }

    if finish_reason == "length" || finish_reason == "max_tokens" {
        resp["warning"] = serde_json::json!(
            "⚠️ 输出被截断：LLM 达到 max_tokens 限制。可增大 max_tokens 参数重试。"
        );
        resp["finish_reason"] = serde_json::json!(finish_reason);
    }
    if let Some(note) = budget_result.note {
        resp["context_budget"] = serde_json::json!(note);
    }
    Ok(ToolResult::json_pretty(&resp))
}
