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
use crate::state::AppState;

// --- Progressive Folding: Render Tier system (Gemini-reviewed) ---

/// Render tier determines how much detail a node gets in the output.
/// Higher tiers get more detail; lower tiers save token budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RenderTier {
    /// Stage 0 beacon hits — always full resolution.
    Beacon = 2,
    /// Stage 1-4 primary search hits — full if budget allows.
    Primary = 1,
    /// Stage 5 cross-file expansion — signature only.
    Expansion = 0,
}

/// AST hit tagged with its render tier for progressive folding.
#[derive(Clone)]
struct RankedHit {
    hit: AstSearchHit,
    tier: RenderTier,
}

/// Max tokens for code context block (~40-60 nodes).
const TOKEN_BUDGET: usize = 6000;

/// Elevated budget when beacon hit confirms high signal-to-noise.
const BEACON_ELEVATED_BUDGET: usize = 6000;

/// Default interactive budget (Hook queries).
const INTERACTIVE_BUDGET: usize = 2000;

/// Estimated tokens per AST node in rendered output.
const TOKENS_PER_NODE_ESTIMATE: usize = 120;

/// Small nodes below this byte count are never folded (Gemini review: 4b).
const MIN_FOLDABLE_BYTES: usize = 120;

/// Max lines inside braces before struct/enum body gets folded.
const STRUCT_FOLD_LINE_THRESHOLD: usize = 5;

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

/// Search by raw query string — unified entry point for Context Pipeline.
pub(crate) async fn code_prefetch_query(state: &AppState, query: &str) -> Option<String> {
    if query.trim().is_empty() {
        return None;
    }
    code_prefetch_inner(state, query).await
}

async fn code_prefetch_inner(state: &AppState, query: &str) -> Option<String> {

    let mut budget_nodes = TOKEN_BUDGET / TOKENS_PER_NODE_ESTIMATE;

    // Stage 0: Beacon priority match (P4)
    // If task title/description contains a beacon name, inject full topology first
    let mut beacon_hits: Vec<AstSearchHit> = Vec::new();
    let beacon_matches = state.store.beacon_search(query)
        .await.unwrap_or_default();

    let has_beacon_hits = !beacon_matches.is_empty();
    if has_beacon_hits {
        for beacon in &beacon_matches {
            if budget_nodes == 0 { break; }
            let nodes = state.store.beacon_map(&beacon.name).await.unwrap_or_default();
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
    let fts_ranked = state.store.ast_search_ranked(query, PRIMARY_TOP_K * 3)
        .await.unwrap_or_default();

    // Stage 2: Embedding vector search (health-aware — Gemini reviewed)
    // Check embedding health via relative coverage ratio, not absolute count
    let ast_cache_len = state.ast_embedding_cache.read().await.len();
    let fts_total = fts_ranked.len();
    // Use FTS5 total as proxy for indexed nodes (avoids extra DB query in hot path).
    // If cache has vectors for a decent fraction of what FTS5 knows about, vectors are useful.
    let embedding_healthy = ast_cache_len > 0
        && (fts_total == 0 || ast_cache_len as f64 / fts_total.max(1) as f64 > 0.5);

    let query_embedding = if embedding_healthy {
        state.embedding_service.as_ref().and_then(|svc| svc.embed(query))
    } else {
        debug!(cache_len = ast_cache_len, "AST embedding cache sparse, skipping vector search");
        None
    };

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
        debug!("Code prefetch: no FTS or vector results for query {:?}", &query[..query.len().min(60)]);
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
        debug!("Code prefetch: all results below RRF threshold for query {:?}", &query[..query.len().min(60)]);
        return None;
    }

    // Stage 4: Fetch full nodes + structural boost + file cohesion
    let mut scored_hits: Vec<(String, f64, AstSearchHit)> = Vec::new();
    for (id, rrf) in &rrf_scored {
        if let Ok(Some(hit)) = state.store.ast_get_search_hit(id).await {
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
    // Tag each hit with its RenderTier (Gemini review: dedup + tier promotion)
    let beacon_keys: HashSet<String> = beacon_hits.iter()
        .map(|h| format!("{}:{}", h.file_path, h.name))
        .collect();
    primary_hits.retain(|h| !beacon_keys.contains(&format!("{}:{}", h.file_path, h.name)));

    let mut ranked: Vec<RankedHit> = beacon_hits.into_iter()
        .map(|hit| RankedHit { hit, tier: RenderTier::Beacon })
        .collect();
    ranked.extend(primary_hits.into_iter()
        .map(|hit| RankedHit { hit, tier: RenderTier::Primary }));

    if ranked.is_empty() {
        // Fallback: inject module-level topology map (Step 4, Gemini-reviewed)
        // When no micro-level AST nodes match, provide architectural navigation
        let summaries = state.store.ast_module_summaries("missiond")
            .await.unwrap_or_default();

        if !summaries.is_empty() {
            info!(
                query_prefix = &query[..query.len().min(60)],
                modules = summaries.len(),
                "Code prefetch: no AST match, injecting topology map fallback"
            );
            return crate::topology_map::render_module_navigation(&summaries, query);
        }
        return None;
    }

    // Cross-file association: expand related types (post-search, Gemini-confirmed)
    let remaining_budget = budget_nodes.saturating_sub(ranked.len());
    if remaining_budget > 0 {
        let existing_names: HashSet<String> = ranked.iter()
            .map(|r| r.hit.name.clone())
            .collect();
        let existing_ids: HashSet<String> = ranked.iter()
            .map(|r| r.hit.id.clone())
            .collect();

        let mut expansion_hits: Vec<AstSearchHit> = Vec::new();

        // Expand: impl Foo → find struct/enum/trait Foo
        for r in &ranked {
            if r.hit.node_type == "impl" && expansion_hits.len() < remaining_budget {
                if let Ok(related) = state.store.ast_find_related(&r.hit.name, 3).await {
                    for rel in related {
                        if !existing_ids.contains(&rel.id)
                            && !expansion_hits.iter().any(|e| e.id == rel.id)
                            && expansion_hits.len() < remaining_budget
                        {
                            expansion_hits.push(rel);
                        }
                    }
                }
            }
        }

        // Expand: called functions not in results
        for r in ranked.iter() {
            for call_name in &r.hit.calls {
                if !existing_names.contains(call_name)
                    && expansion_hits.len() < remaining_budget
                {
                    if let Ok(related) = state.store.ast_find_related(call_name, 1).await {
                        for rel in related {
                            if !existing_ids.contains(&rel.id)
                                && !expansion_hits.iter().any(|e| e.id == rel.id)
                            {
                                expansion_hits.push(rel);
                            }
                        }
                    }
                }
            }
        }

        ranked.extend(expansion_hits.into_iter()
            .map(|hit| RankedHit { hit, tier: RenderTier::Expansion }));
    }

    // Dynamic budget: beacon hit → elevate to full budget (Gemini review: 3)
    let token_budget = if has_beacon_hits {
        BEACON_ELEVATED_BUDGET
    } else {
        INTERACTIVE_BUDGET
    };

    // Phase 2: Pre-fetch KB memories linked to code symbols in results
    let file_paths: Vec<String> = ranked.iter()
        .map(|r| r.hit.file_path.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut symbol_memories: HashMap<String, Vec<String>> = HashMap::new();
    for fp in &file_paths {
        if let Ok(memories) = state.store.kb_get_memories_for_file(fp).await {
            for (symbol, entry) in memories {
                symbol_memories.entry(symbol)
                    .or_default()
                    .push(entry.summary);
            }
        }
    }

    // Render XML output with progressive folding + historical context
    let xml = render_code_context_tiered(&ranked, token_budget, &symbol_memories);

    info!(
        query_prefix = &query[..query.len().min(60)],
        nodes = ranked.len(),
        files = ranked.iter().map(|r| &r.hit.file_path).collect::<HashSet<_>>().len(),
        beacons = beacon_matches.len(),
        token_budget,
        "Code prefetch injected (progressive folding)"
    );

    Some(xml)
}

// --- Progressive Folding: Token estimation & rendering ---

/// Estimate token count from content bytes (~4 bytes per token for English/code).
fn estimate_tokens(content: &str) -> usize {
    content.len() / 4 + 1
}

/// Fold struct/enum body: if brace-enclosed content exceeds threshold, elide it.
/// Returns the folded content or None if no folding needed.
fn fold_structural_body(stub: &str, node_type: &str) -> Option<String> {
    if node_type != "struct" && node_type != "enum" {
        return None;
    }
    let open = stub.find('{')?;
    let close = stub.rfind('}')?;
    if close <= open + 1 {
        return None;
    }
    let body = &stub[open + 1..close];
    let body_lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    if body_lines.len() <= STRUCT_FOLD_LINE_THRESHOLD {
        return None; // Short enough — no folding needed
    }
    let mut folded = String::with_capacity(stub.len());
    folded.push_str(&stub[..open + 1]);
    folded.push_str(&format!("\n    // ... ({} fields/variants elided)\n", body_lines.len()));
    folded.push_str(&stub[close..]);
    Some(folded)
}

/// Render a single node at signature-only level (Tier 3 / budget-exhausted fallback).
fn render_signature_only(hit: &AstSearchHit) -> String {
    format!("// lines: {}-{}\n{}\n",
        hit.start_line, hit.end_line, hit.signature)
}

/// Render a single node at full level (Tier 1).
fn render_full(hit: &AstSearchHit) -> String {
    let mut out = format!("// lines: {}-{}\n", hit.start_line, hit.end_line);
    out.push_str(&hit.stub_content);
    if !hit.stub_content.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out
}

/// Render a single node with struct/enum folding (Tier 2).
fn render_folded(hit: &AstSearchHit) -> String {
    if let Some(folded) = fold_structural_body(&hit.stub_content, &hit.node_type) {
        let mut out = format!("// lines: {}-{}\n", hit.start_line, hit.end_line);
        out.push_str(&folded);
        if !folded.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
        out
    } else {
        render_full(hit)
    }
}

/// Render AST search hits with progressive folding into XML `<code_context>`.
///
/// Tier system (Gemini-reviewed):
/// - Beacon: always full resolution
/// - Primary: full if budget allows, folded for large structs/enums, signature-only when exhausted
/// - Expansion: always signature-only
///
/// High-tier nodes rendered first within each file (Gemini review: lost-in-middle).
fn render_code_context_tiered(
    ranked: &[RankedHit],
    token_budget: usize,
    symbol_memories: &HashMap<String, Vec<String>>,
) -> String {
    if ranked.is_empty() {
        return String::new();
    }

    // Group by file, preserving first-occurrence order.
    // Within each file: sort by tier (desc) then start_line (asc) — high tier first.
    let mut file_order: Vec<String> = Vec::new();
    let mut by_file: HashMap<String, Vec<&RankedHit>> = HashMap::new();
    for r in ranked {
        if !by_file.contains_key(&r.hit.file_path) {
            file_order.push(r.hit.file_path.clone());
        }
        by_file.entry(r.hit.file_path.clone()).or_default().push(r);
    }
    for nodes in by_file.values_mut() {
        nodes.sort_by(|a, b| {
            b.tier.cmp(&a.tier)
                .then_with(|| a.hit.start_line.cmp(&b.hit.start_line))
        });
    }

    let mut remaining = token_budget;
    let mut xml = String::with_capacity(4096);
    xml.push_str("<code_context>\n");
    xml.push_str("<!-- tree-sitter AST 概要 (progressive folding)。修改代码时请用 Read 获取完整源码。-->\n");

    for file_path in &file_order {
        let nodes = &by_file[file_path];
        xml.push_str(&format!("<file path=\"{}\">\n", file_path));
        for r in nodes {
            if remaining == 0 {
                break;
            }
            let rendered = match r.tier {
                RenderTier::Beacon => {
                    // Always full
                    render_full(&r.hit)
                }
                RenderTier::Primary => {
                    let est = estimate_tokens(&r.hit.stub_content);
                    // Small node exemption (Gemini review: 4b)
                    if r.hit.stub_content.len() < MIN_FOLDABLE_BYTES || remaining >= est {
                        // Try folding large structs/enums even at full budget
                        if est > remaining && r.hit.stub_content.len() >= MIN_FOLDABLE_BYTES {
                            render_folded(&r.hit)
                        } else {
                            render_full(&r.hit)
                        }
                    } else {
                        // Budget tight: try folded first, fallback to signature
                        let folded = render_folded(&r.hit);
                        if estimate_tokens(&folded) <= remaining {
                            folded
                        } else {
                            render_signature_only(&r.hit)
                        }
                    }
                }
                RenderTier::Expansion => {
                    // Always signature-only
                    render_signature_only(&r.hit)
                }
            };
            let cost = estimate_tokens(&rendered);
            remaining = remaining.saturating_sub(cost);
            xml.push_str(&rendered);

            // Phase 2: Inject historical_context — KB memories linked to this symbol
            if let Some(memories) = symbol_memories.get(&r.hit.name) {
                if !memories.is_empty() && remaining > 0 {
                    let mut hist = format!("<historical_context symbol=\"{}\">\n", r.hit.name);
                    for (i, mem) in memories.iter().take(3).enumerate() {
                        hist.push_str(&format!("  {}. {}\n", i + 1, mem));
                    }
                    hist.push_str("</historical_context>\n");
                    let hist_cost = estimate_tokens(&hist);
                    if hist_cost <= remaining {
                        remaining = remaining.saturating_sub(hist_cost);
                        xml.push_str(&hist);
                    }
                }
            }
        }
        xml.push_str("</file>\n");
    }

    xml.push_str("</code_context>");
    xml
}

/// Legacy render — kept for test compatibility.
#[cfg(test)]
fn render_code_context(hits: &[AstSearchHit]) -> String {
    let ranked: Vec<RankedHit> = hits.iter()
        .map(|h| RankedHit { hit: h.clone(), tier: RenderTier::Primary })
        .collect();
    render_code_context_tiered(&ranked, TOKEN_BUDGET, &HashMap::new())
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

    // --- Progressive Folding tests ---

    fn make_hit(id: &str, file: &str, name: &str, node_type: &str, sig: &str, stub: &str) -> AstSearchHit {
        AstSearchHit {
            id: id.into(), repo: "test".into(), file_path: file.into(),
            name: name.into(), node_type: node_type.into(), signature: sig.into(),
            start_line: 1, end_line: 20, is_exported: true, docstring: None,
            stub_content: stub.into(), calls: vec![], rank: 0.0,
        }
    }

    #[test]
    fn test_fold_structural_body_short_struct() {
        let stub = "pub struct Point {\n    pub x: f64,\n    pub y: f64,\n}";
        assert!(fold_structural_body(stub, "struct").is_none(), "Short struct should not be folded");
    }

    #[test]
    fn test_fold_structural_body_large_struct() {
        let stub = "pub struct Config {\n    pub a: u32,\n    pub b: u32,\n    pub c: u32,\n    pub d: u32,\n    pub e: u32,\n    pub f: u32,\n    pub g: u32,\n}";
        let folded = fold_structural_body(stub, "struct");
        assert!(folded.is_some(), "Large struct should be folded");
        let text = folded.unwrap();
        assert!(text.contains("elided"), "Folded text should contain 'elided'");
        assert!(text.contains("pub struct Config {"), "Should preserve header");
        assert!(text.ends_with('}'), "Should preserve closing brace");
    }

    #[test]
    fn test_fold_not_applied_to_functions() {
        let stub = "pub fn foo() {\n    line1;\n    line2;\n    line3;\n    line4;\n    line5;\n    line6;\n}";
        assert!(fold_structural_body(stub, "function").is_none());
    }

    #[test]
    fn test_tiered_rendering_beacon_always_full() {
        let hit = make_hit("a", "a.rs", "Foo", "struct", "pub struct Foo",
            "pub struct Foo {\n    pub x: u32,\n}");
        let ranked = vec![RankedHit { hit, tier: RenderTier::Beacon }];
        let xml = render_code_context_tiered(&ranked, 50, &HashMap::new()); // tiny budget
        assert!(xml.contains("pub x: u32"), "Beacon should always render full");
    }

    #[test]
    fn test_tiered_rendering_expansion_signature_only() {
        let hit = make_hit("b", "b.rs", "bar", "function",
            "pub fn bar() -> i32",
            "pub fn bar() -> i32 {\n    // Calls: baz\n    /* elided */\n}");
        let ranked = vec![RankedHit { hit, tier: RenderTier::Expansion }];
        let xml = render_code_context_tiered(&ranked, 10000, &HashMap::new());
        assert!(xml.contains("pub fn bar() -> i32"), "Should contain signature");
        assert!(!xml.contains("elided"), "Expansion should NOT contain body");
    }

    #[test]
    fn test_tiered_rendering_budget_exhaustion() {
        // First node is big (Beacon, always full), second is Primary
        let big_stub = "pub fn big() {\n".to_string()
            + &"    do_stuff();\n".repeat(100)
            + "}";
        let hit1 = make_hit("a", "a.rs", "big", "function", "pub fn big()", &big_stub);
        let hit2 = make_hit("b", "b.rs", "small", "function",
            "pub fn small()",
            "pub fn small() {\n    // Calls: x\n    /* elided */\n}");
        let ranked = vec![
            RankedHit { hit: hit1, tier: RenderTier::Beacon },
            RankedHit { hit: hit2, tier: RenderTier::Primary },
        ];
        // Budget barely covers the big node
        let xml = render_code_context_tiered(&ranked, 500, &HashMap::new());
        assert!(xml.contains("pub fn big()"), "Beacon always rendered");
        // small should still appear (signature-only or full depending on remaining budget)
        assert!(xml.contains("pub fn small()"), "Small node should still appear");
    }

    #[test]
    fn test_dynamic_budget_beacon_elevates() {
        // Verify BEACON_ELEVATED_BUDGET > INTERACTIVE_BUDGET
        assert!(BEACON_ELEVATED_BUDGET > INTERACTIVE_BUDGET);
    }
}
