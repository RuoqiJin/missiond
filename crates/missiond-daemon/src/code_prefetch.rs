//! Code Prefetch Engine — AST hybrid search + token-budgeted context injection.
//!
//! Part of P3: Holographic Context Engine.
//! Searches AST nodes via FTS5 + embedding vector (RRF merge),
//! applies structural weights, MMR diversity, cross-file association,
//! and renders XML `<code_context>` block for autopilot prompt injection.

use std::collections::{HashMap, HashSet};

use tracing::{debug, info};

use missiond_core::db::ast::AstSearchHit;
use missiond_core::embedding;
use missiond_core::types::BoardTask;

use crate::state::AppState;

/// Max tokens for code context block (~40-60 nodes).
const TOKEN_BUDGET: usize = 6000;

/// Estimated tokens per AST node in rendered output.
const TOKENS_PER_NODE_ESTIMATE: usize = 120;

/// Max nodes from primary search (before cross-file expansion).
const PRIMARY_TOP_K: usize = 40;

/// RRF merge constant.
const RRF_K: usize = 60;

/// MMR diversity parameter (0.7 = 70% relevance, 30% diversity).
const MMR_LAMBDA: f64 = 0.7;

/// Minimum RRF score threshold — below this, discard (noise reduction).
const MIN_RRF_SCORE: f64 = 0.002;

/// Max nodes from a single file (diversity cap before MMR).
const MAX_PER_FILE: usize = 8;

/// Structural weight boost factors (Gemini-reviewed).
fn structural_boost(node_type: &str, is_exported: bool) -> f64 {
    match (node_type, is_exported) {
        ("struct", _) | ("enum", _) | ("trait", _) => 1.3,
        ("function", true) => 1.2,
        ("impl", _) => 1.1,
        (_, true) => 1.1,
        _ => 1.0,
    }
}

/// File cohesion boost: if a file has multiple hits, boost all of them.
fn apply_file_cohesion(scored: &mut [(String, f64, AstSearchHit)]) {
    // Count hits per file
    let mut file_counts: HashMap<String, usize> = HashMap::new();
    for (_, _, hit) in scored.iter() {
        *file_counts.entry(hit.file_path.clone()).or_default() += 1;
    }
    // Apply boost: 2 hits → 1.1x, 3+ hits → 1.2x
    for (_, score, hit) in scored.iter_mut() {
        let count = file_counts.get(&hit.file_path).copied().unwrap_or(0);
        if count >= 3 {
            *score *= 1.2;
        } else if count >= 2 {
            *score *= 1.1;
        }
    }
}

/// Build the search query from a Board task.
fn build_query(task: &BoardTask) -> String {
    let mut q = task.title.clone();
    if !task.description.is_empty() {
        q.push(' ');
        q.push_str(&task.description);
    }
    // Truncate to avoid FTS query explosion on very long descriptions
    if q.len() > 500 {
        q.truncate(500);
    }
    q
}

// @beacon: holographic
/// Main entry point: search + rank + render code context block.
/// Returns None if no relevant code found or AST index is empty.
pub(crate) async fn code_prefetch(state: &AppState, task: &BoardTask) -> Option<String> {
    let query = build_query(task);
    if query.trim().is_empty() {
        return None;
    }

    let mut budget_nodes = TOKEN_BUDGET / TOKENS_PER_NODE_ESTIMATE;

    // Stage 0: Beacon priority match (P4)
    // If task title/description contains a beacon name, inject full topology first
    let mut beacon_hits: Vec<AstSearchHit> = Vec::new();
    let q = query.clone();
    let beacon_matches = state.db_exec.run(move |db| {
        db.beacon_search(&q)
    }).await.unwrap_or_default();

    if !beacon_matches.is_empty() {
        let db = state.mission.db();
        for beacon in &beacon_matches {
            if budget_nodes == 0 { break; }
            let nodes = db.beacon_map(&beacon.name).unwrap_or_default();
            for bn in &nodes {
                if budget_nodes == 0 { break; }
                if let (Some(ref stub), Some(start), Some(end)) = (&bn.stub_content, bn.start_line, bn.end_line) {
                    beacon_hits.push(AstSearchHit {
                        id: format!("beacon-{}-{}", beacon.name, bn.symbol_name),
                        repo: bn.repo.clone(),
                        file_path: bn.file_path.clone(),
                        name: bn.symbol_name.clone(),
                        node_type: bn.node_type.clone().unwrap_or_default(),
                        signature: bn.signature.clone().unwrap_or_default(),
                        start_line: start,
                        end_line: end,
                        is_exported: false,
                        docstring: bn.annotation.clone(),
                        stub_content: stub.clone(),
                        calls: Vec::new(),
                        rank: 0.0,
                    });
                    budget_nodes = budget_nodes.saturating_sub(1);
                }
            }
            if !nodes.is_empty() {
                info!(beacon = %beacon.name, nodes = nodes.len(), "Beacon match injected");
            }
        }
    }

    // Stage 1: FTS5 ranked IDs
    let q = query.clone();
    let fts_ranked = state.db_exec.run(move |db| {
        db.ast_search_ranked(&q, PRIMARY_TOP_K * 3)
    }).await.unwrap_or_default();

    // Stage 2: Embedding vector search
    let query_embedding = state.embedding_service.as_ref()
        .and_then(|svc| svc.embed(&query));

    let cache = state.ast_embedding_cache.read().await;
    let vec_ranked: Vec<(String, usize, f32)> = if let Some(ref qe) = query_embedding {
        let mut scores: Vec<(usize, f32)> = cache.iter()
            .enumerate()
            .map(|(i, (_, vec))| (i, embedding::cosine_similarity(qe, vec)))
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.iter()
            .take(PRIMARY_TOP_K * 3)
            .enumerate()
            .map(|(rank, (idx, sim))| (cache[*idx].0.clone(), rank, *sim))
            .collect()
    } else {
        Vec::new()
    };
    drop(cache);

    if fts_ranked.is_empty() && vec_ranked.is_empty() {
        debug!("Code prefetch: no FTS or vector results for task {}", task.id);
        return None;
    }

    // Stage 3: RRF merge
    let mut merged: HashMap<String, (Option<usize>, Option<usize>, Option<f32>)> = HashMap::new();
    for (id, rank) in &fts_ranked {
        merged.entry(id.clone()).or_insert((None, None, None)).0 = Some(*rank);
    }
    for (id, rank, sim) in &vec_ranked {
        let entry = merged.entry(id.clone()).or_insert((None, None, None));
        entry.1 = Some(*rank);
        entry.2 = Some(*sim);
    }

    let mut rrf_scored: Vec<(String, f64)> = merged.into_iter()
        .map(|(id, (fts_r, vec_r, _sim))| {
            let score = embedding::rrf_score(fts_r, vec_r, RRF_K);
            (id, score)
        })
        .filter(|(_, score)| *score >= MIN_RRF_SCORE)
        .collect();
    rrf_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    rrf_scored.truncate(PRIMARY_TOP_K * 2);

    if rrf_scored.is_empty() {
        debug!("Code prefetch: all results below RRF threshold for task {}", task.id);
        return None;
    }

    // Stage 4: Fetch full nodes + structural boost + file cohesion
    let db = state.mission.db();
    let mut scored_hits: Vec<(String, f64, AstSearchHit)> = Vec::new();
    for (id, rrf) in &rrf_scored {
        if let Ok(Some(hit)) = db.ast_get_search_hit(id) {
            let boost = structural_boost(&hit.node_type, hit.is_exported);
            scored_hits.push((id.clone(), rrf * boost, hit));
        }
    }

    apply_file_cohesion(&mut scored_hits);

    // Re-sort after boosts
    scored_hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Stage 5: MMR diversity re-ranking
    let cache = state.ast_embedding_cache.read().await;
    let emb_map: HashMap<String, &Vec<f32>> = cache.iter()
        .map(|(id, vec)| (id.clone(), vec))
        .collect();

    // Normalize scores for MMR
    let (min_s, max_s) = scored_hits.iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), (_, s, _)| (mn.min(*s), mx.max(*s)));
    let score_range = max_s - min_s;

    let candidates: Vec<(usize, f64, Vec<f32>)> = scored_hits.iter()
        .enumerate()
        .map(|(i, (id, score, _))| {
            let norm = if score_range > 0.0 { (score - min_s) / score_range } else { 1.0 };
            let emb = emb_map.get(id).map(|v| (*v).clone()).unwrap_or_default();
            (i, norm, emb)
        })
        .collect();
    drop(cache);

    let mmr_indices = embedding::mmr_rerank_cosine(&candidates, budget_nodes, MMR_LAMBDA);
    let mut primary_hits: Vec<AstSearchHit> = mmr_indices.iter()
        .filter_map(|&i| scored_hits.get(i).map(|(_, _, hit)| hit.clone()))
        .collect();

    // Per-file cap: ensure no single file dominates
    let mut file_counts: HashMap<String, usize> = HashMap::new();
    primary_hits.retain(|hit| {
        let count = file_counts.entry(hit.file_path.clone()).or_default();
        *count += 1;
        *count <= MAX_PER_FILE
    });

    // Merge: beacon hits (priority) + hybrid search hits (deduped)
    let beacon_ids: HashSet<String> = beacon_hits.iter()
        .map(|h| format!("{}:{}", h.file_path, h.name))
        .collect();
    primary_hits.retain(|h| !beacon_ids.contains(&format!("{}:{}", h.file_path, h.name)));

    let mut all_hits = beacon_hits;
    all_hits.extend(primary_hits);

    if all_hits.is_empty() {
        return None;
    }

    // Cross-file association: expand related types (post-search, Gemini-confirmed)
    let remaining_budget = budget_nodes.saturating_sub(all_hits.len());
    if remaining_budget > 0 {
        let existing_names: HashSet<String> = all_hits.iter()
            .map(|h| h.name.clone())
            .collect();
        let existing_ids: HashSet<String> = all_hits.iter()
            .map(|h| h.id.clone())
            .collect();

        let mut expansion_hits: Vec<AstSearchHit> = Vec::new();

        // Expand: impl Foo → find struct/enum/trait Foo
        for hit in &all_hits {
            if hit.node_type == "impl" && expansion_hits.len() < remaining_budget {
                if let Ok(related) = db.ast_find_related(&hit.name, 3) {
                    for r in related {
                        if !existing_ids.contains(&r.id)
                            && !expansion_hits.iter().any(|e| e.id == r.id)
                            && expansion_hits.len() < remaining_budget
                        {
                            expansion_hits.push(r);
                        }
                    }
                }
            }
        }

        // Expand: called functions not in results
        for hit in all_hits.iter() {
            for call_name in &hit.calls {
                if !existing_names.contains(call_name)
                    && expansion_hits.len() < remaining_budget
                {
                    if let Ok(related) = db.ast_find_related(call_name, 1) {
                        for r in related {
                            if !existing_ids.contains(&r.id)
                                && !expansion_hits.iter().any(|e| e.id == r.id)
                            {
                                expansion_hits.push(r);
                            }
                        }
                    }
                }
            }
        }

        all_hits.extend(expansion_hits);
    }

    // Render XML output
    let xml = render_code_context(&all_hits);

    info!(
        task_id = %task.id,
        nodes = all_hits.len(),
        files = all_hits.iter().map(|h| &h.file_path).collect::<HashSet<_>>().len(),
        beacons = beacon_matches.len(),
        "Code prefetch injected"
    );

    Some(xml)
}

/// Render AST search hits into XML `<code_context>` block, grouped by file.
fn render_code_context(hits: &[AstSearchHit]) -> String {
    if hits.is_empty() {
        return String::new();
    }

    // Group by file path, preserving order of first occurrence
    let mut file_order: Vec<String> = Vec::new();
    let mut by_file: HashMap<String, Vec<&AstSearchHit>> = HashMap::new();
    for hit in hits {
        if !by_file.contains_key(&hit.file_path) {
            file_order.push(hit.file_path.clone());
        }
        by_file.entry(hit.file_path.clone()).or_default().push(hit);
    }

    // Sort nodes within each file by start_line
    for nodes in by_file.values_mut() {
        nodes.sort_by_key(|n| n.start_line);
    }

    let mut xml = String::with_capacity(4096);
    xml.push_str("<code_context>\n");
    xml.push_str("<!-- tree-sitter 提取的代码结构概要。修改代码时请用 Read(file, offset, limit) 获取完整源码。-->\n");

    for file_path in &file_order {
        let nodes = &by_file[file_path];
        xml.push_str(&format!("<file path=\"{}\">\n", file_path));
        for node in nodes {
            // Line range comment
            xml.push_str(&format!("// lines: {}-{}\n", node.start_line, node.end_line));
            // Stub content (already contains signature + doc + calls placeholder)
            xml.push_str(&node.stub_content);
            if !node.stub_content.ends_with('\n') {
                xml.push('\n');
            }
            xml.push('\n');
        }
        xml.push_str("</file>\n");
    }

    xml.push_str("</code_context>");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structural_boost() {
        assert_eq!(structural_boost("struct", false), 1.3);
        assert_eq!(structural_boost("enum", true), 1.3);
        assert_eq!(structural_boost("trait", false), 1.3);
        assert_eq!(structural_boost("function", true), 1.2);
        assert_eq!(structural_boost("function", false), 1.0);
        assert_eq!(structural_boost("impl", false), 1.1);
        assert_eq!(structural_boost("const", true), 1.1);
    }

    #[test]
    fn test_render_code_context() {
        let hits = vec![
            AstSearchHit {
                id: "a".into(),
                repo: "test".into(),
                file_path: "src/main.rs".into(),
                name: "main".into(),
                node_type: "function".into(),
                signature: "fn main()".into(),
                start_line: 1,
                end_line: 10,
                is_exported: false,
                docstring: None,
                stub_content: "fn main() {\n    // Calls: run\n}".into(),
                calls: vec!["run".into()],
                rank: 0.0,
            },
            AstSearchHit {
                id: "b".into(),
                repo: "test".into(),
                file_path: "src/lib.rs".into(),
                name: "run".into(),
                node_type: "function".into(),
                signature: "pub fn run() -> Result<()>".into(),
                start_line: 5,
                end_line: 20,
                is_exported: true,
                docstring: Some("Run the app".into()),
                stub_content: "/// Run the app\npub fn run() -> Result<()> {\n    // Calls: init\n}".into(),
                calls: vec!["init".into()],
                rank: 0.0,
            },
        ];

        let xml = render_code_context(&hits);
        assert!(xml.contains("<code_context>"));
        assert!(xml.contains("<file path=\"src/main.rs\">"));
        assert!(xml.contains("<file path=\"src/lib.rs\">"));
        assert!(xml.contains("// lines: 1-10"));
        assert!(xml.contains("// lines: 5-20"));
        assert!(xml.contains("fn main()"));
        assert!(xml.contains("pub fn run()"));
        assert!(xml.contains("</code_context>"));
    }

    #[test]
    fn test_file_cohesion() {
        let hit = AstSearchHit {
            id: String::new(), repo: String::new(), file_path: "a.rs".into(),
            name: String::new(), node_type: String::new(), signature: String::new(),
            start_line: 0, end_line: 0, is_exported: false, docstring: None,
            stub_content: String::new(), calls: vec![], rank: 0.0,
        };
        let mut scored = vec![
            ("1".into(), 1.0, AstSearchHit { id: "1".into(), file_path: "a.rs".into(), ..hit.clone() }),
            ("2".into(), 1.0, AstSearchHit { id: "2".into(), file_path: "a.rs".into(), ..hit.clone() }),
            ("3".into(), 1.0, AstSearchHit { id: "3".into(), file_path: "a.rs".into(), ..hit.clone() }),
            ("4".into(), 1.0, AstSearchHit { id: "4".into(), file_path: "b.rs".into(), ..hit.clone() }),
        ];
        apply_file_cohesion(&mut scored);
        // a.rs has 3 hits → 1.2x boost
        assert!((scored[0].1 - 1.2).abs() < 0.001);
        // b.rs has 1 hit → no boost
        assert!((scored[3].1 - 1.0).abs() < 0.001);
    }
}
