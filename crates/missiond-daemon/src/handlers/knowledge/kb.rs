use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use tracing::info;

use crate::context_budget::apply_context_budget;
use crate::context_budget::MAX_ROUTER_PAYLOAD_BYTES;
use crate::embedding_worker::resolve_llm_credentials;
use crate::gemini_client::REQUEST_CALLER;
use crate::helpers::default_mission_home;
use crate::lenient;
use crate::state::AppState;
use crate::state::EmbeddingTask;

/// Content guard: reject verbose debug logs, stack traces, and narrative-style entries.
/// Returns Some(rejection_message) if content should be rejected, None if OK.
fn check_content_quality(
    summary: &str,
    detail: &Option<Value>,
    category: Option<&str>,
) -> Option<String> {
    // Rule 1: summary too long — architecture:summary gets 800 chars, others 400
    let max_chars = match category {
        Some(c) if c == "architecture:summary" => 800,
        _ => 400,
    };
    if summary.chars().count() > max_chars {
        return Some(format!(
            "REJECTED: summary 过长（{}字）。summary 必须 ≤ {} 字，是结论性摘要。高密度技术细节（配置/命令/代码）请存入 detail 字段（JSON）。",
            summary.chars().count(), max_chars
        ));
    }

    // Rule 1b: empty or near-empty summary
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Some("REJECTED: summary 为空。".to_string());
    }
    if trimmed.chars().count() < 5 {
        return Some(format!(
            "REJECTED: summary 过短（{}字）。至少需要 5 个字符才能构成有意义的知识。",
            trimmed.chars().count()
        ));
    }

    // Rule 1c: test/probe entries
    let lower = summary.to_lowercase();
    let garbage_patterns = ["test write", "test kb write", "probe", "test entry"];
    for pattern in &garbage_patterns {
        if lower == *pattern || lower.starts_with(&format!("{} ", pattern)) {
            return Some(format!(
                "REJECTED: summary 疑似测试条目（'{}'）。测试数据不应写入知识库。",
                summary
            ));
        }
    }

    // Rule 1d: batch log entries (e.g., "realtime-extract 批次 batch-20260315-...")
    if lower.contains("batch-") && (lower.contains("处理完成") || lower.contains("批次")) {
        return Some(
            "REJECTED: summary 是批次处理日志，不是知识。操作日志不应存入 KB。".to_string(),
        );
    }

    // Rule 2: summary contains stack trace / log indicators
    let stack_patterns = [
        "at node_modules/",
        "Caused by:",
        "stack trace",
        "panic at",
        "RUST_BACKTRACE",
        "Error:",
        "    at ",
        "线程",
        "thread '",
    ];
    for pattern in &stack_patterns {
        if summary.contains(pattern) {
            return Some(format!(
                "REJECTED: summary 包含堆栈/日志片段（'{}'）。summary 应是泛化结论，不要包含原始报错。请提炼后重试。",
                pattern
            ));
        }
    }

    // Rule 3: narrative indicators — "先...然后...最后..." pattern in summary
    let narrative_words = [
        "先查看",
        "先检查",
        "然后尝试",
        "然后发现",
        "最后发现",
        "接着",
        "第一步",
        "第二步",
        "第三步",
        "首先我",
        "我尝试",
    ];
    let narrative_count = narrative_words
        .iter()
        .filter(|w| summary.contains(*w))
        .count();
    if narrative_count >= 2 {
        return Some(
            "REJECTED: summary 是叙事体（含「先...然后...」等流水账结构）。请改写为结论性陈述：\
             【现象关键字】→【根因】→【解决方案】。"
                .to_string(),
        );
    }

    // Rule 4: detail too large (> 2000 chars of serialized JSON = likely pasting raw logs)
    if let Some(d) = detail {
        let detail_str = serde_json::to_string(d).unwrap_or_default();
        if detail_str.len() > 2000 {
            return Some(format!(
                "REJECTED: detail 过长（{}字节）。detail 应是结构化三段式 {{trigger, conclusion, action}}，不要粘贴原始日志。请精简后重试。",
                detail_str.len()
            ));
        }
    }

    None
}

#[derive(Deserialize)]
struct KBRememberArgs {
    category: String,
    key: String,
    summary: String,
    #[serde(default)]
    detail: Option<Value>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    confidence: Option<f64>,
}

#[derive(Deserialize)]
struct KBKeyArgs {
    key: String,
}

#[derive(Deserialize)]
struct KBUpdateArgs {
    key: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    detail: Option<serde_json::Value>,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
    linked_task_id: Option<String>,
}

#[derive(Deserialize)]
struct KBSearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    search_mode: Option<String>,
    #[serde(default)]
    project: Option<String>,
}

#[derive(Deserialize)]
struct KBListArgs {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct KBImportArgs {
    format: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct KBDiscoverArgs {
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    password: Option<String>,
}

#[derive(Deserialize)]
struct KBGCArgs {
    action: String,
    #[serde(default, deserialize_with = "lenient::option_i64")]
    days: Option<i64>,
}

// @beacon: knowledge
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Merged tool dispatch: map unified tool names to legacy handler names
    let (name, args) = match name {
        "mission_kb_query" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("search");
            let legacy = match action {
                "get" => "mission_kb_get",
                "list" => "mission_kb_list",
                _ => "mission_kb_search",
            };
            (legacy, args)
        }
        "mission_kb_mutate" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("forget");
            let legacy = match action {
                "update" => "mission_kb_update",
                "import" => "mission_kb_import",
                "forget" if args.get("keys").is_some() => "mission_kb_batch_forget",
                _ => "mission_kb_forget",
            };
            (legacy, args)
        }
        "mission_kb_ops" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("gc");
            if action == "compact" {
                return handle_kb_compact(state, args).await;
            }
            let legacy = match action {
                "analyze" => "mission_kb_analyze",
                "discover" => "mission_kb_discover",
                "queue_status" => "mission_kb_queue_status",
                "execute_plan" => "mission_kb_execute_plan",
                _ => "mission_kb_gc",
            };
            (legacy, args)
        }
        other => (other, args),
    };
    match name {
        // ===== Knowledge Base (Jarvis Memory) =====
        "mission_kb_remember" => {
            let args: KBRememberArgs = serde_json::from_value(args)?;
            // Content guard: reject verbose debug logs / stack traces
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
            };
            let result = state
                .store
                .kb_remember(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Trigger async embedding update via Worker (avoids block_in_place in MCP handler)
            let _ = state
                .embedding_tx
                .try_send(EmbeddingTask::ProcessKBEntry(result.entry.id.clone()));

            // Auto-edge: if detail contains consolidated_from, add supersedes edges
            if result.action == "created" || result.action == "updated" {
                if let Some(ref detail) = result.entry.detail {
                    if let Some(from_keys) =
                        detail.get("consolidated_from").and_then(|v| v.as_array())
                    {
                        for key_val in from_keys {
                            if let Some(key) = key_val.as_str() {
                                if let Ok(Some(target_id)) = state.store.kb_get_id_by_key(key).await
                                {
                                    let _ = state
                                        .store
                                        .kb_add_edge(
                                            &result.entry.id,
                                            &target_id,
                                            "supersedes",
                                            1.0,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
            }

            // Phase 2: AST-KB linking — if detail contains symbol/file_hint, create graph link
            if result.action == "created" || result.action == "updated" {
                if let Some(ref detail) = result.entry.detail {
                    let symbol = detail.get("symbol").and_then(|v| v.as_str());
                    let file_hint = detail.get("file_hint").and_then(|v| v.as_str());
                    if let Some(sym) = symbol {
                        // Search AST for this symbol to get ast_node_id
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

            // Phase 3: Emit KBBatchMutated for event-driven FTS rebuild / consolidation triggers
            state
                .event_bus
                .publish(crate::event_bus::DaemonEvent::KBBatchMutated {
                    count: 1,
                    categories: vec![input.category.clone()],
                    action: result.action.clone(),
                });

            // Conflict detection: for new entries, check semantic similarity against existing KB
            let conflicts = if result.action == "created" {
                detect_kb_conflicts(state, &result.entry).await
            } else {
                vec![]
            };

            if conflicts.is_empty() {
                Ok(ToolResult::json_pretty(&result))
            } else {
                // Auto-downweight: if a conflicting entry has higher confidence,
                // reduce the new entry's confidence to half of the highest conflicting entry.
                // This ensures the new entry ranks below established knowledge in retrieval.
                let max_conflict_conf = conflicts
                    .iter()
                    .filter_map(|c| c["confidence"].as_f64())
                    .fold(0.0f64, f64::max);
                if max_conflict_conf > result.entry.confidence {
                    let reduced = (max_conflict_conf / 2.0).max(0.1);
                    let _ = state
                        .store
                        .kb_adjust_confidence(
                            &result.entry.id,
                            reduced - result.entry.confidence, // delta to reach target
                        )
                        .await;
                }

                // Add contradicts edges for detected conflicts
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
        "mission_kb_forget" => {
            let KBKeyArgs { key } = serde_json::from_value(args)?;
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
            // Phase 3: Emit KBBatchMutated for event-driven FTS rebuild
            if deleted {
                state
                    .event_bus
                    .publish(crate::event_bus::DaemonEvent::KBBatchMutated {
                        count: 1,
                        categories: vec![],
                        action: "deleted".to_string(),
                    });
            }
            Ok(ToolResult::json(&serde_json::json!({
                "deleted": deleted,
                "key": key,
            })))
        }
        "mission_kb_batch_forget" => {
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
            // Phase 3: Emit KBBatchMutated for event-driven consumers
            if count > 0 {
                state
                    .event_bus
                    .publish(crate::event_bus::DaemonEvent::KBBatchMutated {
                        count: count as u32,
                        categories: vec![],
                        action: "deleted".to_string(),
                    });
            }
            Ok(ToolResult::json(&serde_json::json!({
                "deleted_count": count,
                "requested_keys": keys.len(),
            })))
        }
        "mission_kb_update" => {
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
                    // Phase 3: Emit KBBatchMutated for event-driven consumers
                    state
                        .event_bus
                        .publish(crate::event_bus::DaemonEvent::KBBatchMutated {
                            count: 1,
                            categories: vec![entry.category.clone()],
                            action: "updated".to_string(),
                        });
                    Ok(ToolResult::json_pretty(&serde_json::json!({
                        "updated": true,
                        "content_changed": content_changed,
                        "entry": entry,
                    })))
                }
                None => Ok(ToolResult::error(&format!("key '{}' not found", args.key))),
            }
        }
        "mission_kb_search" => {
            let KBSearchArgs {
                query,
                category,
                limit,
                offset,
                search_mode,
                project,
            } = serde_json::from_value(args).unwrap_or(KBSearchArgs {
                query: None,
                category: None,
                limit: None,
                offset: None,
                search_mode: None,
                project: None,
            });
            let query = query.unwrap_or_default();
            if query.is_empty() && category.is_none() {
                let entries = state
                    .store
                    .kb_list(None)
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                return Ok(ToolResult::json_pretty(&entries));
            }

            let top_k = limit.unwrap_or(10).clamp(1, 50);
            let offset = offset.unwrap_or(0).min(100);
            let exact_mode = search_mode.as_deref() == Some("exact");

            // 1. FTS5 ranked IDs (fallback to LIKE for Chinese)
            let fts_ranked: Vec<(String, usize, Option<String>)> = {
                let ranked = state
                    .store
                    .kb_search_fts_ranked_scoped(&query, category.as_deref(), project.as_deref())
                    .await
                    .unwrap_or_default();
                if ranked.is_empty() {
                    let like = state
                        .store
                        .kb_search_like_ranked_scoped(&query, category.as_deref(), project.as_deref())
                        .await
                        .unwrap_or_default();
                    like.into_iter()
                        .map(|(id, rank)| (id, rank, None))
                        .collect()
                } else {
                    ranked
                }
            };

            // 2. Embedding cosine similarity against kb_search_cache
            // Use floor of 60 candidates; expand for offset pagination
            let output_k = top_k + offset;
            let fetch_k = (output_k * 3).max(60);
            let query_embedding = state
                .embedding_service
                .as_ref()
                .and_then(|svc| svc.embed(&query));
            let cache = state.kb_search_cache.read().await;
            let vec_ranked: Vec<(String, usize, f32)> = if let Some(ref qe) = query_embedding {
                let mut scores: Vec<(usize, f32)> = cache
                    .iter()
                    .enumerate()
                    .map(|(i, (_, vec))| (i, missiond_core::embedding::cosine_similarity(qe, vec)))
                    .collect();
                // Pre-RRF cosine floor: discard semantically unrelated candidates.
                // NOTE: threshold assumes Cosine Similarity [-1, 1] with BGE model.
                // Revisit if switching to L2 distance or unnormalized inner product.
                scores.retain(|(_, sim)| *sim >= 0.3);
                scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                scores
                    .iter()
                    .take(fetch_k)
                    .enumerate()
                    .map(|(rank, (idx, sim))| (cache[*idx].0.clone(), rank, *sim))
                    .collect()
            } else {
                Vec::new()
            };
            drop(cache);

            // 3. RRF merge
            let rrf_k = 60;
            // Collect FTS snippets for later injection
            let fts_snippets: std::collections::HashMap<String, String> = fts_ranked
                .iter()
                .filter_map(|(id, _, snip)| snip.as_ref().map(|s| (id.clone(), s.clone())))
                .collect();
            let mut merged: std::collections::HashMap<
                String,
                (Option<usize>, Option<usize>, Option<f32>),
            > = std::collections::HashMap::new();
            for (id, rank, _snippet) in &fts_ranked {
                merged.entry(id.clone()).or_insert((None, None, None)).0 = Some(*rank);
            }
            for (id, rank, sim) in &vec_ranked {
                let entry = merged.entry(id.clone()).or_insert((None, None, None));
                entry.1 = Some(*rank);
                entry.2 = Some(*sim);
            }
            let mut ranked: Vec<(String, f64, Option<usize>, Option<usize>, Option<f32>)> = merged
                .into_iter()
                .map(|(id, (fts_r, vec_r, sim))| {
                    let score = missiond_core::embedding::rrf_score(fts_r, vec_r, rrf_k);
                    (id, score, fts_r, vec_r, sim)
                })
                .collect();
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            // Keep enlarged candidate pool so temporal decay can resurface evergreen docs
            ranked.truncate(fetch_k);

            // 4. Fetch full entries + apply temporal decay on enlarged pool
            let now = chrono::Utc::now();
            let mut scored_entries: Vec<(missiond_core::types::KnowledgeEntry, f64)> = Vec::new();
            for (id, rrf, _fts_r, _vec_r, _sim) in &ranked {
                if let Ok(Some(entry)) = state.store.kb_get_by_id(id).await {
                    let age_days = chrono::DateTime::parse_from_rfc3339(&entry.updated_at)
                        .map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() as f64 / 24.0)
                        .unwrap_or(0.0);
                    let decay = missiond_core::embedding::temporal_decay(&entry.category, age_days);
                    scored_entries.push((entry, rrf * decay));
                }
            }
            // Re-sort by decayed score
            scored_entries
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            // 4b. RRF drop-off filter: discard entries scoring < 50% of top score
            // RRF scores are compressed (rank 50 ≈ 55% of rank 1), so 0.5 cuts single-path noise
            if let Some(max_score) = scored_entries.first().map(|(_, s)| *s) {
                if max_score > 0.0 {
                    let threshold = max_score * 0.5;
                    scored_entries.retain(|(_, s)| *s >= threshold);
                }
            }

            // Trim candidate pool before final selection
            scored_entries.truncate(output_k * 2);

            // 5. Final selection: exact mode skips MMR, explore mode uses MMR diversity
            let mut results: Vec<missiond_core::types::KnowledgeEntry> = if exact_mode {
                // Exact mode: pure relevance order, skip MMR diversity injection
                scored_entries
                    .iter()
                    .skip(offset)
                    .take(top_k)
                    .map(|(e, _)| e.clone())
                    .collect()
            } else {
                // Explore mode: MMR diversity re-ranking
                let (min_s, max_s) = scored_entries
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), (_, s)| {
                        (mn.min(*s), mx.max(*s))
                    });
                let score_range = max_s - min_s;

                let cache = state.kb_search_cache.read().await;
                let emb_map: std::collections::HashMap<String, &Vec<f32>> =
                    cache.iter().map(|(id, vec)| (id.clone(), vec)).collect();

                let candidates: Vec<(usize, f64, Vec<f32>)> = scored_entries
                    .iter()
                    .enumerate()
                    .map(|(i, (e, score))| {
                        let norm_score = if score_range > 0.0 {
                            (score - min_s) / score_range
                        } else {
                            1.0
                        };
                        let emb = emb_map.get(&e.id).map(|v| (*v).clone()).unwrap_or_default();
                        (i, norm_score, emb)
                    })
                    .collect();
                drop(cache);

                // MMR selects output_k items, then skip offset for pagination
                let mmr_indices =
                    missiond_core::embedding::mmr_rerank_cosine(&candidates, output_k, 0.7);
                mmr_indices
                    .iter()
                    .skip(offset)
                    .take(top_k)
                    .filter_map(|&i| scored_entries.get(i).map(|(e, _)| e.clone()))
                    .collect()
            };

            // 6. Slim down detail field by category to reduce token usage
            for entry in &mut results {
                let cat = entry.category.as_str();

                // architecture:module — strip detail entirely (file/function lists are huge)
                if cat.starts_with("architecture:module") {
                    entry.detail = None;
                    continue;
                }

                // Core policy/architecture decisions — higher truncation threshold
                let is_core = cat.starts_with("policy")
                    || cat.starts_with("memory:architecture")
                    || cat.starts_with("decision");
                let max_len: usize = if is_core { 2000 } else { 800 };

                if let Some(detail) = entry.detail.take() {
                    match detail {
                        serde_json::Value::String(s) => {
                            if s.len() > max_len {
                                // Find a valid char boundary at or before max_len
                                let mut boundary = max_len;
                                while boundary > 0 && !s.is_char_boundary(boundary) {
                                    boundary -= 1;
                                }
                                entry.detail = Some(serde_json::Value::String(format!(
                                    "{}... (truncated)",
                                    &s[..boundary]
                                )));
                            } else {
                                entry.detail = Some(serde_json::Value::String(s));
                            }
                        }
                        serde_json::Value::Object(obj) => {
                            let s = serde_json::to_string(&obj).unwrap_or_default();
                            if s.len() > max_len {
                                // Don't break JSON structure — replace with hint
                                entry.detail = Some(serde_json::Value::String(
                                    format!("[JSON object omitted for brevity, {} chars. Use mission_kb_get(key) for full detail]", s.len())
                                ));
                            } else {
                                entry.detail = Some(serde_json::Value::Object(obj));
                            }
                        }
                        other => {
                            entry.detail = Some(other);
                        }
                    }
                }
            }

            // 7. Inject FTS snippets for entries that had FTS hits
            // When snippet exists, clear detail to avoid redundant content
            for entry in &mut results {
                if let Some(snippet) = fts_snippets.get(&entry.id) {
                    entry.context_snippet = Some(snippet.clone());
                    entry.detail = None;
                }
            }

            // Update access stats
            if !results.is_empty() {
                let _ = state.store.kb_update_access_stats(&results).await;
            }

            Ok(ToolResult::json_pretty(&results))
        }
        "mission_kb_get" => {
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
        "mission_kb_list" => {
            let KBListArgs { category } =
                serde_json::from_value(args).unwrap_or(KBListArgs { category: None });
            let entries = state
                .store
                .kb_list(category.as_deref())
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&entries))
        }
        "mission_kb_import" => {
            let KBImportArgs { format, path } = serde_json::from_value(args)?;
            match format.as_str() {
                "servers_yaml" => {
                    let yaml_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| default_mission_home().join("servers.yaml"));
                    let infra = missiond_core::InfraConfig::load(&yaml_path);
                    let mut imported = 0;
                    for server in &infra.servers {
                        let detail = serde_json::to_value(server).ok();
                        let summary = format!(
                            "{} ({}) — {}",
                            server.name,
                            server.provider,
                            server.roles.join(", ")
                        );
                        let input = missiond_core::types::KBRememberInput {
                            category: "infra".to_string(),
                            key: server.id.clone(),
                            summary,
                            detail,
                            source: Some("import".to_string()),
                            confidence: Some(1.0),
                        };
                        state
                            .store
                            .kb_remember(&input)
                            .await
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        imported += 1;
                    }
                    Ok(ToolResult::json(&serde_json::json!({
                        "imported": imported,
                        "source": yaml_path.display().to_string(),
                    })))
                }
                _ => Ok(ToolResult::error(format!(
                    "Unsupported import format: {}",
                    format
                ))),
            }
        }

        "mission_kb_discover" => {
            let KBDiscoverArgs {
                host,
                port,
                password,
            } = serde_json::from_value(args)?;

            // Resolve host: if it looks like an infra key (no @ or .), try infra registry
            let (ssh_user, ssh_host, ssh_port, ssh_pass) =
                if !host.contains('@') && !host.contains('.') {
                    // Try infra registry lookup
                    let server = state.infra.read().unwrap().get(&host).cloned();
                    let ip_owned = server
                        .and_then(|s| s.host.clone())
                        .unwrap_or_else(|| host.clone());
                    let ip = ip_owned.as_str();
                    // Look up credentials from KB
                    let cred_pass = state
                        .store
                        .kb_search(&format!("{} password", host), Some("credential"))
                        .await
                        .ok()
                        .and_then(|entries| entries.into_iter().next())
                        .and_then(|e| {
                            e.detail.as_ref().and_then(|d| {
                                d.get("password").and_then(|v| v.as_str().map(String::from))
                            })
                        });
                    (
                        "root".to_string(),
                        ip.to_string(),
                        port.unwrap_or(22),
                        password.or(cred_pass),
                    )
                } else if host.contains('@') {
                    let parts: Vec<&str> = host.splitn(2, '@').collect();
                    (
                        parts[0].to_string(),
                        parts[1].to_string(),
                        port.unwrap_or(22),
                        password,
                    )
                } else {
                    (
                        "root".to_string(),
                        host.clone(),
                        port.unwrap_or(22),
                        password,
                    )
                };

            // Build probe script (piped to remote bash to avoid quoting issues)
            let probe_script = concat!(
                "echo \"HOSTNAME=$(hostname)\"\n",
                "echo \"UNAME=$(uname -a)\"\n",
                "echo \"OS=$(. /etc/os-release 2>/dev/null && echo \"$PRETTY_NAME\" || echo unknown)\"\n",
                "echo \"CPU=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo unknown)\"\n",
                "MEM=$(LANG=C free -h 2>/dev/null | awk '/Mem:/{print $2}'); echo \"MEM=${MEM:-unknown}\"\n",
                "DISK=$(LANG=C df -h / 2>/dev/null | awk 'NR==2{print $2}'); echo \"DISK=${DISK:-unknown}\"\n",
                "echo \"UPTIME=$(uptime -p 2>/dev/null || uptime || echo unknown)\"\n",
                "DOCKER=$(docker ps --format '{{.Names}}:{{.Image}}' 2>/dev/null | tr '\\n' ','); echo \"DOCKER=${DOCKER:-none}\"\n",
                "LISTEN=$(LANG=C ss -tlnp 2>/dev/null | awk 'NR>1{print $4}' | tr '\\n' ','); echo \"LISTEN=${LISTEN:-unknown}\"\n",
            );

            // Build SSH command args (pipe probe_script to stdin)
            let mut ssh_args: Vec<String> = Vec::new();
            if let Some(ref pass) = ssh_pass {
                ssh_args.extend(["sshpass".into(), "-p".into(), pass.clone(), "ssh".into()]);
            } else {
                ssh_args.push("ssh".into());
                ssh_args.extend(["-o".into(), "BatchMode=yes".into()]);
            }
            ssh_args.extend([
                "-o".into(),
                "StrictHostKeyChecking=no".into(),
                "-o".into(),
                "ConnectTimeout=10".into(),
                "-p".into(),
                ssh_port.to_string(),
                format!("{}@{}", ssh_user, ssh_host),
                "bash".into(),
            ]);

            let program = ssh_args.remove(0);
            let mut cmd = tokio::process::Command::new(&program);
            cmd.args(&ssh_args);
            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            let mut child = cmd
                .spawn()
                .map_err(|e| anyhow!("Failed to spawn SSH: {}", e))?;

            // Write probe script to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(probe_script.as_bytes()).await.ok();
                drop(stdin);
            }

            let output = child
                .wait_with_output()
                .await
                .map_err(|e| anyhow!("SSH failed: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(ToolResult::error(format!(
                    "SSH probe failed: {}",
                    stderr.trim()
                )));
            }

            // Parse key=value output
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut detail = serde_json::Map::new();
            for line in stdout.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim().to_lowercase();
                    let val = v.trim().to_string();
                    if !val.is_empty() && val != "unknown" && val != "none" {
                        detail.insert(key, serde_json::Value::String(val));
                    }
                }
            }

            // Add connection info
            detail.insert(
                "ssh_user".to_string(),
                serde_json::Value::String(ssh_user.clone()),
            );
            detail.insert(
                "ssh_host".to_string(),
                serde_json::Value::String(ssh_host.clone()),
            );
            if ssh_port != 22 {
                detail.insert(
                    "ssh_port".to_string(),
                    serde_json::Value::Number(ssh_port.into()),
                );
            }

            // Build summary
            let hostname = detail
                .get("hostname")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let os = detail
                .get("os")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let cpu = detail.get("cpu").and_then(|v| v.as_str()).unwrap_or("?");
            let mem = detail.get("mem").and_then(|v| v.as_str()).unwrap_or("?");
            let summary = format!("{} — {} ({}C, {})", hostname, os, cpu, mem);

            // Derive a key from hostname or host
            let key = hostname.to_lowercase().replace(' ', "-");

            let input = missiond_core::types::KBRememberInput {
                category: "infra".to_string(),
                key: key.clone(),
                summary: summary.clone(),
                detail: Some(serde_json::Value::Object(detail.clone())),
                source: Some("discovery".to_string()),
                confidence: Some(1.0),
            };
            state
                .store
                .kb_remember(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            Ok(ToolResult::json(&serde_json::json!({
                "status": "discovered",
                "key": key,
                "summary": summary,
                "detail": detail,
            })))
        }

        "mission_kb_gc" => {
            let KBGCArgs { action, days } = serde_json::from_value(args)?;
            match action.as_str() {
                "stats" => {
                    let stats = state.store.kb_stats().await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&stats))
                }
                "stale" => {
                    let threshold = days.unwrap_or(30);
                    let stale = state.store.kb_find_stale(threshold).await
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
                    let dups = state.store.kb_find_duplicates().await
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
                    let stale = state.store.kb_find_stale(threshold).await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    let keys: Vec<String> = stale.iter().map(|e| e.key.clone()).collect();
                    let count = state.store.kb_batch_forget(&keys).await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "action": "clean_stale",
                        "threshold_days": threshold,
                        "deleted": count,
                        "keys": keys,
                    })))
                }
                "clean_duplicates" => {
                    let dups = state.store.kb_find_duplicates().await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    // Keep entry with higher access_count (or newer updated_at), delete the other
                    let mut to_delete = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for (a, b, sim) in &dups {
                        // Skip if either already marked for deletion
                        if seen.contains(&a.key) || seen.contains(&b.key) { continue; }
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
                    let keys: Vec<String> = to_delete.iter()
                        .filter_map(|d| d["deleted_key"].as_str().map(String::from))
                        .collect();
                    let count = state.store.kb_batch_forget(&keys).await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "action": "clean_duplicates",
                        "deleted": count,
                        "details": to_delete,
                    })))
                }
                _ => Ok(ToolResult::error(format!("Unknown gc action: {}. Use: stats, stale, duplicates, clean_stale, clean_duplicates", action))),
            }
        }

        // ===== KB Analysis (via external AI) =====
        "mission_kb_analyze" => {
            // Parse parameters
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

            // 1. Read KB entries with pagination
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

            // Also get total count for pagination info
            let total_count = state
                .store
                .kb_list(target_category.map(|s| s))
                .await
                .map(|v| v.len())
                .unwrap_or(0);

            // 2. Build JSONL with metadata (compact format for LLM)
            let now = chrono::Utc::now();
            let mut jsonl_lines = Vec::with_capacity(entries.len());
            let include_detail = mode == "consolidation_plan";

            for e in &entries {
                if e.category == "credential" {
                    continue;
                }
                let sanitized_summary = missiond_core::db::shared::redact_sensitive(&e.summary);

                // Calculate age in days
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
                        let sanitized_detail =
                            missiond_core::db::shared::redact_sensitive(&detail_str);
                        item["detail"] = serde_json::Value::String(sanitized_detail);
                    }
                }

                jsonl_lines.push(serde_json::to_string(&item).unwrap_or_default());
            }
            let kb_jsonl = jsonl_lines.join("\n");

            // 2b. Build Board context if requested
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

            // 3. Build prompt and optional response_format based on mode
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
                        format!("\n\n=== BOARD TASKS CONTEXT ===\n{}\n===========================\n\n\
                            === 任务感知整理规则 ===\n\
                            1. 关联 Open 任务的 KB → 保护，不删不合并，仅用 update 补 linked_task_id\n\
                            2. 关联 Done 任务的 KB → distill 蒸馏收敛（提取最终结论升维到 architecture/feature，删流水账）\n\
                            3. 无关联 Orphan → preference/ops/platform 保留；老旧 debug/bugfix 可删除\n\
                            4. 每个 action 的 reason 中说明关联了哪个任务（或标注 orphan）\n", ctx)
                    } else {
                        String::new()
                    };

                    format!(
                        "你是 MissionD 的知识库自治整理引擎。分析传入的 JSONL 条目，生成可执行的清理计划。\n\n\
                        规则：\n\
                        1. 重复/相似合并 (merge)：相同主题不同措辞 → 合并，保留最高 confidence，汇总 summary\n\
                        2. 碎片整合 (update)：松散相关条目 → 整合成连贯大条目\n\
                        3. 过时清理 (delete)：基于 age_days 和 access_count，旧策略已被覆盖的条目\n\
                        4. 类别修正 (update)：category 放错的条目移到正确分类\n\
                        5. 蒸馏收敛 (distill)：已完成项目的多条流水账 → 提取最终结论为一条精华\n\n\
                        保守原则：不确定就不动。每个 action 必须有 reason。\
                        {}\n\n共 {} 条（第 {} 到 {} 条）：\n{}",
                        board_section,
                        entries.len(), offset + 1, offset + entries.len() as u32, kb_jsonl
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
                    // overview - 查重+升维版
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
                        entries.len(), kb_jsonl
                    )
                }
            };

            // 4. Resolve LLM credentials
            let (base_url, jwt) = resolve_llm_credentials().await?;

            // 5. Apply context budget
            let mut analysis_messages: Vec<Value> =
                vec![serde_json::json!({"role": "user", "content": analysis_prompt})];
            let budget_result =
                apply_context_budget(&mut analysis_messages, MAX_ROUTER_PAYLOAD_BYTES);
            if budget_result.trimmed {
                info!(
                    "KB analyze: context budget applied — {}",
                    budget_result.note.as_deref().unwrap_or("trimmed")
                );
            }

            // 6. Build request body with optional response_format
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

            // 7. Call router API (rate-limited with 429 retry)
            // Large KB analysis (consolidation_plan with detail) needs extended idle timeout:
            // Gemini thinking phase for 100K+ prompts can exceed the default 120s before first token.
            let idle_timeout = if kb_jsonl.len() > 50_000 {
                Some(std::time::Duration::from_secs(300)) // 5 min for large prompts
            } else {
                None // default 120s
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

            // 8. Build response with pagination metadata
            let mut resp = serde_json::json!({
                "model": resp_model,
                "mode": mode,
                "entries_in_request": entries.len(),
                "total_entries": total_count,
                "offset": offset,
                "has_more": (offset as usize + entries.len()) < total_count,
                "usage": usage,
            });

            // For consolidation_plan, try to parse as JSON and auto-save to queue
            let save_plan = args_val
                .get("save_plan")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if mode == "consolidation_plan" {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                    resp["plan"] = parsed.clone();
                    // Auto-save plan to operation queue
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
                                            let reason =
                                                a.get("reason").and_then(|v| v.as_str()).or_else(
                                                    || a.get("rationale").and_then(|v| v.as_str()),
                                                );
                                            // For update ops, embed new_entry in rationale JSON
                                            if operation == "update"
                                                || operation == "recategorize"
                                                || operation == "category_fix"
                                            {
                                                let mut meta = serde_json::Map::new();
                                                if let Some(r) = reason {
                                                    meta.insert(
                                                        "reason".into(),
                                                        serde_json::json!(r),
                                                    );
                                                }
                                                if let Some(ne) = a.get("new_entry") {
                                                    meta.insert("new_entry".into(), ne.clone());
                                                }
                                                Some(
                                                    serde_json::to_string(&meta)
                                                        .unwrap_or_default(),
                                                )
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
                                        resp["save_error"] = serde_json::json!(format!(
                                            "Failed to save plan: {}",
                                            e
                                        ));
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

        // ===== KB Operation Queue =====
        "mission_kb_queue_status" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
            let status_filter = args_val.get("status").and_then(|v| v.as_str());

            let ops = state
                .store
                .kb_ops_list(plan_id, status_filter)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // If plan_id given, also get summary
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

        "mission_kb_execute_plan" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
            let limit = args_val.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

            // Expire stale pending ops (>24h)
            let expired = state.store.kb_ops_expire_stale(86400).await.unwrap_or(0);
            if expired > 0 {
                info!(expired, "kb_execute_plan: expired stale pending ops");
            }

            let plan_id = plan_id.ok_or_else(|| anyhow!("plan_id is required"))?;

            // Get pending operations
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
                // Mark as running
                let _ = state
                    .store
                    .kb_ops_update_status(&op.id, "running", None, None)
                    .await;

                let target_keys: Vec<String> =
                    serde_json::from_str(&op.target_keys).unwrap_or_default();
                let outcome = match op.operation.as_str() {
                    "delete" => {
                        let mut deleted = 0usize;
                        for key in &target_keys {
                            if state.store.kb_forget(key).await.unwrap_or(false) {
                                deleted += 1;
                            }
                        }
                        Ok(format!("Deleted {}/{} keys", deleted, target_keys.len()))
                    }
                    "update" | "category_fix" | "recategorize" => {
                        // Update category/summary directly from rationale (contains new_entry JSON)
                        let meta: serde_json::Value = op
                            .rationale
                            .as_deref()
                            .and_then(|r| serde_json::from_str(r).ok())
                            .unwrap_or_default();
                        let new_entry = meta.get("new_entry");
                        let key = target_keys.first().map(|k| k.as_str()).or_else(|| {
                            new_entry.and_then(|ne| ne.get("key").and_then(|v| v.as_str()))
                        });
                        let category =
                            new_entry.and_then(|ne| ne.get("category").and_then(|v| v.as_str()));
                        let summary =
                            new_entry.and_then(|ne| ne.get("summary").and_then(|v| v.as_str()));

                        match (key, category) {
                            (Some(key), Some(cat)) => {
                                let input = missiond_core::types::KBRememberInput {
                                    category: cat.to_string(),
                                    key: key.to_string(),
                                    summary: summary.unwrap_or("").to_string(),
                                    detail: new_entry.and_then(|ne| ne.get("detail").cloned()),
                                    source: Some("consolidation".to_string()),
                                    confidence: new_entry.and_then(|ne| ne.get("confidence").and_then(|v| v.as_f64())),
                                };
                                match state.store.kb_remember(&input).await {
                                    Ok(r) => Ok(format!("Updated key={} category={} action={}", key, cat, r.action)),
                                    Err(e) => Err(format!("Failed to update: {}", e)),
                                }
                            }
                            _ => Err("update operation requires new_entry with key and category in rationale".to_string()),
                        }
                    }
                    "merge" | "distill" => {
                        // Auto-dispatch: fetch entries, build prompt, submit to slot-memory-slow
                        let mut entries_text = String::new();
                        for key in &target_keys {
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
                            Err(format!("No KB entries found for keys: {:?}", target_keys))
                        } else {
                            let rationale = op.rationale.as_deref().unwrap_or("");
                            let prompt = if op.operation == "merge" {
                                format!(
                                    "KB整理任务(merge):\n\n原因: {}\n\n以下KB条目内容重叠,请合并为一条。\
                                    保留最完整的key,用 mission_kb_remember 写入合并后的内容(category/summary/detail),\
                                    然后用 mission_kb_forget 删除多余的key。\n\n{}", rationale, entries_text
                                )
                            } else {
                                format!(
                                    "KB整理任务(distill):\n\n原因: {}\n\n以下KB条目需要精炼。\
                                    用 mission_kb_remember 更新每条的 summary(更简洁)和 detail(保留关键信息,删除冗余)。\n\n{}",
                                    rationale, entries_text
                                )
                            };
                            match crate::state::submit_task(state.store.as_ref(), "memory", &prompt)
                                .await
                            {
                                Ok(task_id) => Ok(format!("dispatched:task_id={}", task_id)),
                                Err(e) => Err(format!("submit failed: {}", e)),
                            }
                        }
                    }
                    other => Err(format!("Unknown operation: {}", other)),
                };

                match outcome {
                    Ok(msg) => {
                        let (status_str, result_json) = if msg.starts_with("dispatched:") {
                            // Extract task_id from "dispatched:task_id=xxx"
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

            // Signal unified scheduler to dispatch any newly created submit tasks
            if results
                .iter()
                .any(|r| r.get("status").and_then(|s| s.as_str()) == Some("dispatched"))
            {
                state
                    .event_bus
                    .publish(crate::event_bus::DaemonEvent::TaskCreated {
                        task_id: String::new(),
                    });
            }

            // Get remaining count
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

        // ===== Holographic Beacon (P4) =====
        "mission_beacon_list" => {
            let beacons = state
                .store
                .beacon_list()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&beacons))
        }
        "mission_beacon_map" => {
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
        "mission_beacon_tag" => {
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

            // Read the file and insert `// @beacon: feature` above the symbol
            let source = std::fs::read_to_string(&file_path)
                .map_err(|e| anyhow!("Cannot read file {}: {}", file_path, e))?;

            // Find the line containing the symbol declaration
            let mut target_line = None;
            for (idx, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                // Match: fn symbol, struct symbol, enum symbol, impl symbol, trait symbol, pub ... fn symbol, etc.
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

            let target_line = target_line
                .ok_or_else(|| anyhow!("Symbol '{}' not found in {}", symbol, file_path))?;

            // Check if @beacon: feature already exists on preceding lines
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

            // Determine indentation from the target line
            let indent = lines[target_line].len() - lines[target_line].trim_start().len();
            let indent_str: String = lines[target_line].chars().take(indent).collect();

            // Insert the beacon comment
            let mut new_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            new_lines.insert(
                target_line,
                format!("{}// @beacon: {}", indent_str, feature),
            );

            std::fs::write(&file_path, new_lines.join("\n"))
                .map_err(|e| anyhow!("Cannot write file {}: {}", file_path, e))?;

            // Also immediately record in DB (sync pipeline will re-confirm on next commit)
            let beacon_id = state
                .store
                .beacon_ensure(&feature)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // Determine repo from file_path (find repo root)
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
        "mission_beacon_annotate" => {
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

            // Find repo name from file_path
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

        // ===== Code Context (P3.5) =====
        "mission_code_search" => {
            #[derive(Deserialize)]
            struct CodeSearchArgs {
                query: String,
                #[serde(default)]
                repo: Option<String>,
                #[serde(default)]
                file_path: Option<String>,
                #[serde(default)]
                node_type: Option<String>,
                #[serde(default)]
                limit: Option<usize>,
            }
            let CodeSearchArgs {
                query,
                repo,
                file_path,
                node_type,
                limit,
            } = serde_json::from_value(args)?;
            let limit = limit.unwrap_or(20).min(50);

            // FTS search
            let hits = state
                .store
                .ast_search(&query, limit * 2)
                .await
                .unwrap_or_default();

            if hits.is_empty() {
                return Ok(ToolResult::text("No code nodes found matching query."));
            }

            // Post-filter by repo, file_path prefix, node_type
            let filtered: Vec<_> = hits
                .into_iter()
                .filter(|h| {
                    if let Some(ref r) = repo {
                        if h.repo != *r {
                            return false;
                        }
                    }
                    if let Some(ref fp) = file_path {
                        if !h.file_path.starts_with(fp.as_str()) {
                            return false;
                        }
                    }
                    if let Some(ref nt) = node_type {
                        if h.node_type != *nt {
                            return false;
                        }
                    }
                    true
                })
                .take(limit)
                .collect();

            if filtered.is_empty() {
                return Ok(ToolResult::text("No code nodes matched filters."));
            }

            // Render as structured JSON
            let results: Vec<serde_json::Value> = filtered
                .iter()
                .map(|h| {
                    serde_json::json!({
                        "name": h.name,
                        "node_type": h.node_type,
                        "file_path": h.file_path,
                        "repo": h.repo,
                        "lines": format!("{}-{}", h.start_line, h.end_line),
                        "exported": h.is_exported,
                        "signature": h.signature,
                        "calls": h.calls,
                        "docstring": h.docstring,
                        "stub": h.stub_content,
                    })
                })
                .collect();

            // Also do cross-file association for top results
            let mut related_results: Vec<serde_json::Value> = Vec::new();
            for h in filtered.iter().take(5) {
                if h.node_type == "impl" {
                    if let Ok(related) = state.store.ast_find_related(&h.name, 3).await {
                        for r in related {
                            if !filtered.iter().any(|f| f.id == r.id)
                                && !related_results.iter().any(|rr| {
                                    rr.get("name").and_then(|v| v.as_str()) == Some(&r.name)
                                        && rr.get("node_type").and_then(|v| v.as_str())
                                            == Some(&r.node_type)
                                })
                            {
                                related_results.push(serde_json::json!({
                                    "name": r.name,
                                    "node_type": r.node_type,
                                    "file_path": r.file_path,
                                    "lines": format!("{}-{}", r.start_line, r.end_line),
                                    "stub": r.stub_content,
                                }));
                            }
                        }
                    }
                }
            }

            let mut output = serde_json::json!({
                "query": query,
                "count": results.len(),
                "results": results,
            });
            if !related_results.is_empty() {
                output["related"] = serde_json::json!(related_results);
            }

            Ok(ToolResult::json_pretty(&output))
        }

        _ => Err(anyhow!("Unknown kb tool: {name}")),
    }
}

/// Programmatic KB compaction: rule-based cleanup beyond auto_gc.
/// dry_run=true (default) previews what would be deleted.
async fn handle_kb_compact(state: &AppState, args: serde_json::Value) -> Result<ToolResult> {
    let dry_run = args.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(true);
    // Load all entries for rule-based filtering
    let all = state.store.kb_list(None).await?;
    let now = chrono::Utc::now();
    let mut candidates: Vec<(String, String, String, f64, &str)> = Vec::new(); // (key, category, summary, confidence, reason)

    for e in &all {
        let age_days = chrono::DateTime::parse_from_rfc3339(&e.updated_at)
            .map(|t| (now - t.with_timezone(&chrono::Utc)).num_days())
            .unwrap_or(0);

        // Exempt categories: architecture:summary, policy:decision, preference — never auto-compact
        let exempt = e.category.starts_with("architecture:summary")
            || e.category.starts_with("policy:decision")
            || e.category.starts_with("preference")
            || e.category == "infra";
        if exempt {
            continue;
        }

        // Rule 1: Low confidence (< 0.3) — feedback loop has deprioritized
        if e.confidence < 0.3 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "low_confidence",
            ));
            continue;
        }
        // Rule 2: State-type entries older than 30d with 0 access
        if e.kb_type == "state" && e.access_count == 0 && age_days > 30 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_state",
            ));
            continue;
        }
        // Rule 3: memory:ops older than 7 days
        if e.category.starts_with("memory:ops") && age_days > 7 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_ops",
            ));
            continue;
        }
        // Rule 4: memory:debug older than 30 days
        if e.category.starts_with("memory:debug") && age_days > 30 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_debug",
            ));
            continue;
        }
        // Rule 5: memory:bugfix older than 30 days with no retrieval
        if e.category.starts_with("memory:bugfix") && e.access_count == 0 && age_days > 30 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_bugfix",
            ));
            continue;
        }
        // Rule 6: Low-value facts — confidence < 0.5 and never accessed
        if e.kb_type == "fact" && e.confidence < 0.5 && e.access_count == 0 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "low_value_fact",
            ));
            continue;
        }
        // Rule 7: Expired scratchpad — Working Memory entries older than 7 days
        if e.scope_task_id.is_some() && age_days > 7 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "expired_scratchpad",
            ));
            continue;
        }
    }

    let total = candidates.len();

    if dry_run {
        let mut by_reason: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for (_, _, _, _, reason) in &candidates {
            *by_reason.entry(reason).or_default() += 1;
        }
        let preview: Vec<_> = candidates
            .iter()
            .take(50)
            .map(|(key, cat, summary, conf, reason)| {
                serde_json::json!({
                    "key": key, "category": cat, "summary": summary,
                    "confidence": conf, "reason": reason
                })
            })
            .collect();
        Ok(ToolResult::json_pretty(&serde_json::json!({
            "dryRun": true,
            "totalEntries": all.len(),
            "totalCandidates": total,
            "byReason": by_reason,
            "candidates": preview,
            "hint": "Set dryRun=false to execute deletion."
        })))
    } else {
        let keys: Vec<String> = candidates.iter().map(|(k, _, _, _, _)| k.clone()).collect();
        let deleted = state.store.kb_batch_forget(&keys).await?;
        info!(deleted, total, "KB compact: cleaned up entries");
        Ok(ToolResult::json(&serde_json::json!({
            "dryRun": false,
            "deleted": deleted,
            "total": total
        })))
    }
}

/// Detect semantically similar entries that may conflict with a newly created KB entry.
/// Uses embedding cosine similarity within the same category prefix.
async fn detect_kb_conflicts(
    state: &AppState,
    new_entry: &missiond_core::types::KnowledgeEntry,
) -> Vec<serde_json::Value> {
    const CONFLICT_SIM_THRESHOLD: f32 = 0.82;

    let svc = match state.embedding_service.as_ref() {
        Some(s) => s,
        None => return vec![],
    };

    // Embed the new entry's summary
    let new_text = format!("{} {}", new_entry.key, new_entry.summary);
    let new_vec = match svc.embed(&new_text) {
        Some(v) => v,
        None => return vec![],
    };

    // Compare against cached KB embeddings
    let cache = state.kb_search_cache.read().await;
    let category_prefix = new_entry
        .category
        .split(':')
        .next()
        .unwrap_or(&new_entry.category);

    let mut conflicts = Vec::new();
    for (id, vec) in cache.iter() {
        if id == &new_entry.id {
            continue; // skip self
        }
        let cosine = missiond_core::embedding::cosine_similarity(&new_vec, vec);
        // Hybrid conflict detection: pure cosine OR (moderate cosine + Jaccard overlap)
        // Addresses semantic dilution for long vs short entries
        let is_conflict = if cosine >= CONFLICT_SIM_THRESHOLD {
            true
        } else if cosine >= 0.6 {
            // Fetch entry to compute Jaccard on summary text
            if let Ok(Some(existing)) = state.store.kb_get_by_id(id).await {
                let existing_prefix = existing
                    .category
                    .split(':')
                    .next()
                    .unwrap_or(&existing.category);
                if existing_prefix == category_prefix {
                    let jaccard =
                        text_jaccard(&new_text, &format!("{} {}", existing.key, existing.summary));
                    jaccard >= 0.5
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        if is_conflict {
            if let Ok(Some(existing)) = state.store.kb_get_by_id(id).await {
                let existing_prefix = existing
                    .category
                    .split(':')
                    .next()
                    .unwrap_or(&existing.category);
                if existing_prefix == category_prefix {
                    conflicts.push(serde_json::json!({
                        "id": existing.id,
                        "category": existing.category,
                        "key": existing.key,
                        "summary": existing.summary,
                        "confidence": existing.confidence,
                        "similarity": format!("{:.3}", cosine),
                    }));
                }
            }
        }
    }

    // Sort by similarity descending, limit to top 5
    conflicts.sort_by(|a, b| {
        let sa = a["similarity"].as_str().unwrap_or("0");
        let sb = b["similarity"].as_str().unwrap_or("0");
        sb.partial_cmp(sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    conflicts.truncate(5);
    conflicts
}

/// Lightweight Jaccard similarity on tokenized text (CJK unigrams + ASCII words).
fn text_jaccard(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let tokenize = |text: &str| -> HashSet<String> {
        let mut tokens = HashSet::new();
        let mut word = String::new();
        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                word.push(ch.to_ascii_lowercase());
            } else {
                if word.len() >= 2 {
                    tokens.insert(word.clone());
                }
                word.clear();
                if ch as u32 > 0x2E80 {
                    tokens.insert(ch.to_string());
                }
            }
        }
        if word.len() >= 2 {
            tokens.insert(word);
        }
        tokens
    };
    let ta = tokenize(a);
    let tb = tokenize(b);
    let intersection = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}
