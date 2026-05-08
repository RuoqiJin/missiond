use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

use super::args::{KBKeyArgs, KBListArgs, KBSearchArgs};

fn review_state_hidden(state: &str) -> bool {
    matches!(
        state,
        "superseded-by-lisp"
            | "superseded-by-code"
            | "historical-evidence"
            | "duplicate"
            | "wrong-or-stale"
            | "delete-candidate"
            | "needs-human"
    )
}

async fn filter_entries_by_review(
    state: &AppState,
    entries: Vec<missiond_core::types::KnowledgeEntry>,
    include_archived: bool,
    state_filter: Option<&str>,
) -> Vec<missiond_core::types::KnowledgeEntry> {
    let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
    let review_states = state
        .store
        .kb_review_current_for_ids(&ids)
        .await
        .unwrap_or_default();

    entries
        .into_iter()
        .filter(|entry| {
            let review_state = review_states.get(&entry.id).map(|r| r.state.as_str());
            if let Some(filter) = state_filter {
                return match filter {
                    "unreviewed" => review_state.is_none(),
                    other => review_state == Some(other),
                };
            }
            if include_archived {
                return true;
            }
            !review_state.map(review_state_hidden).unwrap_or(false)
        })
        .collect()
}

pub(super) async fn handle_kb_search(state: &AppState, args: Value) -> Result<ToolResult> {
    let KBSearchArgs {
        query,
        category,
        limit,
        offset,
        search_mode,
        project,
        include_archived,
        state_filter,
    } = serde_json::from_value(args).unwrap_or(KBSearchArgs {
        query: None,
        category: None,
        limit: None,
        offset: None,
        search_mode: None,
        project: None,
        include_archived: false,
        state_filter: None,
    });
    let query = query.unwrap_or_default();
    if query.is_empty() && category.is_none() {
        let entries = state
            .store
            .kb_list(None)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        let entries =
            filter_entries_by_review(state, entries, include_archived, state_filter.as_deref())
                .await;
        return Ok(ToolResult::json_pretty(&entries));
    }

    let top_k = limit.unwrap_or(10).clamp(1, 50);
    let offset = offset.unwrap_or(0).min(100);
    let exact_mode = search_mode.as_deref() == Some("exact");

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

    let rrf_k = 60;
    let fts_snippets: std::collections::HashMap<String, String> = fts_ranked
        .iter()
        .filter_map(|(id, _, snip)| snip.as_ref().map(|s| (id.clone(), s.clone())))
        .collect();
    let mut merged: std::collections::HashMap<String, (Option<usize>, Option<usize>, Option<f32>)> =
        std::collections::HashMap::new();
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
    ranked.truncate(fetch_k);

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
    scored_entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if let Some(max_score) = scored_entries.first().map(|(_, s)| *s) {
        if max_score > 0.0 {
            let threshold = max_score * 0.5;
            scored_entries.retain(|(_, s)| *s >= threshold);
        }
    }

    scored_entries.truncate(output_k * 2);

    let mut results: Vec<missiond_core::types::KnowledgeEntry> = if exact_mode {
        scored_entries
            .iter()
            .skip(offset)
            .take(top_k)
            .map(|(e, _)| e.clone())
            .collect()
    } else {
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

        let mmr_indices = missiond_core::embedding::mmr_rerank_cosine(&candidates, output_k, 0.7);
        mmr_indices
            .iter()
            .skip(offset)
            .take(top_k)
            .filter_map(|&i| scored_entries.get(i).map(|(e, _)| e.clone()))
            .collect()
    };
    results =
        filter_entries_by_review(state, results, include_archived, state_filter.as_deref()).await;
    results.truncate(top_k);

    for entry in &mut results {
        let cat = entry.category.as_str();
        if cat.starts_with("architecture:module") {
            entry.detail = None;
            continue;
        }

        let is_core = cat.starts_with("policy")
            || cat.starts_with("memory:architecture")
            || cat.starts_with("decision");
        let max_len: usize = if is_core { 2000 } else { 800 };

        if let Some(detail) = entry.detail.take() {
            match detail {
                serde_json::Value::String(s) => {
                    if s.len() > max_len {
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
                        entry.detail = Some(serde_json::Value::String(format!(
                            "[JSON object omitted for brevity, {} chars. Use mission_kb_get(key) for full detail]",
                            s.len()
                        )));
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

    for entry in &mut results {
        if let Some(snippet) = fts_snippets.get(&entry.id) {
            entry.context_snippet = Some(snippet.clone());
            entry.detail = None;
        }
    }

    if !results.is_empty() {
        let _ = state.store.kb_update_access_stats(&results).await;
    }

    Ok(ToolResult::json_pretty(&results))
}

pub(super) async fn handle_kb_get(state: &AppState, args: Value) -> Result<ToolResult> {
    let KBKeyArgs {
        key,
        include_archived,
    } = serde_json::from_value(args)?;
    let entry = state
        .store
        .kb_get(&key)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    match entry {
        Some(e) => {
            let review = state
                .store
                .kb_review_get_by_key(&key)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            if !include_archived
                && review
                    .as_ref()
                    .map(|r| review_state_hidden(&r.state))
                    .unwrap_or(false)
            {
                return Ok(ToolResult::error(format!(
                    "Key is archived by KB review overlay: {} (state={}). Re-run with include_archived=true to inspect it.",
                    key,
                    review.map(|r| r.state).unwrap_or_else(|| "unknown".to_string())
                )));
            }
            Ok(ToolResult::json_pretty(&e))
        }
        None => Ok(ToolResult::error(format!("Key not found: {}", key))),
    }
}

pub(super) async fn handle_kb_list(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_parsed: KBListArgs = serde_json::from_value(args).unwrap_or(KBListArgs {
        category: None,
        limit: 50,
        offset: 0,
        compact: false,
        include_archived: false,
        state_filter: None,
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
    let entries = filter_entries_by_review(
        state,
        entries,
        args_parsed.include_archived,
        args_parsed.state_filter.as_deref(),
    )
    .await;

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
