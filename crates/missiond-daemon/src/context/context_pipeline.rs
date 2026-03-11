//! Context Prefetch Pipeline ("懂我")
//!
//! Unified context injection for all entry points:
//! - UserPromptSubmit Hook (Shell → IPC)
//! - iOS Jarvis (WebSocket)
//! - Autopilot (Board task dispatch)
//!
//! Fan-out parallel search with per-engine timeout + global 700ms hard timeout.
//! Fail-open: any timed-out dimension is silently skipped.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use missiond_core::embedding;
use missiond_core::types::KnowledgeEntry;

use crate::state::AppState;

// --- Timeout budgets ---
const GLOBAL_TIMEOUT_MS: u64 = 700;
const KB_TIMEOUT_MS: u64 = 300;
const SKILL_TIMEOUT_MS: u64 = 100;
const CODE_TIMEOUT_MS: u64 = 600;
const TASK_ACK_TIMEOUT_MS: u64 = 50;

/// Light token budget for interactive queries (Hook / Jarvis).
const LIGHT_CODE_BUDGET: usize = 2000;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PrefetchRequest {
    pub query: String,
    pub source: PrefetchSource,
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
}

fn default_token_budget() -> usize { 4000 }

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum PrefetchSource {
    Hook { ppid: u32 },
    Jarvis,
    Autopilot { task_id: String },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillHint {
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct KbHint {
    pub category: String,
    pub key: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TaskUpdate {
    pub id: String,
    pub slot_id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrefetchResult {
    pub skills: Vec<SkillHint>,
    pub kb_entries: Vec<KbHint>,
    pub code_context: Option<String>,
    pub task_updates: Vec<TaskUpdate>,
    /// Pre-assembled text ready for injection.
    pub assembled: String,
}

/// Execute the Context Prefetch Pipeline.
///
/// Fan-out: 4 parallel searches with individual timeouts.
/// Fan-in: assemble results into formatted text.
/// Global 700ms hard timeout with fail-open on any dimension.
pub(crate) async fn execute(state: &AppState, req: &PrefetchRequest) -> PrefetchResult {
    let query = req.query.chars().take(500).collect::<String>();
    if query.trim().is_empty() {
        return PrefetchResult::empty();
    }

    let global_timeout = Duration::from_millis(GLOBAL_TIMEOUT_MS);

    // Determine whether to include task_ack (only for Hook source)
    let ppid = match &req.source {
        PrefetchSource::Hook { ppid } => Some(*ppid),
        _ => None,
    };

    // Fan-out: 4 parallel searches
    let result = tokio::time::timeout(global_timeout, async {
        tokio::join!(
            search_skills(state, &query),
            search_kb(state, &query),
            search_code(state, &query, &req.source),
            search_task_ack(state, ppid),
        )
    }).await;

    let (skills, kb_entries, code_context, task_updates) = match result {
        Ok((s, k, c, t)) => (s, k, c, t),
        Err(_) => {
            warn!("Context prefetch global timeout ({}ms)", GLOBAL_TIMEOUT_MS);
            (vec![], vec![], None, vec![])
        }
    };

    let assembled = assemble(&skills, &kb_entries, &code_context, &task_updates);

    PrefetchResult {
        skills,
        kb_entries,
        code_context,
        task_updates,
        assembled,
    }
}

// --- Individual search functions (each with its own timeout) ---

async fn search_skills(state: &AppState, query: &str) -> Vec<SkillHint> {
    let fut = async {
        let mut topic_scores: HashMap<String, (f64, Option<usize>, Option<usize>, serde_json::Value)> =
            HashMap::new();

        // 1. Name/aka exact match → bonus +0.3
        for s in state.skills.search(query).iter().take(10) {
            topic_scores.entry(s.name.clone()).or_insert_with(|| {
                (0.3, None, None, serde_json::json!({
                    "name": s.name,
                    "path": s.path,
                }))
            });
        }

        // 2. FTS5
        let db = state.mission.db();
        if let Ok(fts_results) = db.skill_search_fts(query) {
            for (rank, r) in fts_results.iter().take(20).enumerate() {
                topic_scores.entry(r.topic.clone()).or_insert_with(|| {
                    (0.0, Some(rank), None, serde_json::json!({
                        "name": r.topic,
                        "path": r.file_path,
                    }))
                });
                if let Some(entry) = topic_scores.get_mut(&r.topic) {
                    if entry.1.is_none() { entry.1 = Some(rank); }
                }
            }
        }

        // 3. Embedding cosine similarity
        if let Some(ref emb_svc) = state.embedding_service {
            if let Some(query_vec) = emb_svc.embed(query) {
                let cache = state.skill_embedding_cache.read().await;
                if !cache.is_empty() {
                    let mut sims: Vec<(usize, f32)> = cache.iter()
                        .enumerate()
                        .map(|(i, (_, vec))| (i, embedding::cosine_similarity(&query_vec, vec)))
                        .collect();
                    sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                    for (rank, (idx, _sim)) in sims.iter().take(10).enumerate() {
                        let topic_name = &cache[*idx].0;
                        let entry = topic_scores.entry(topic_name.clone()).or_insert_with(|| {
                            (0.0, None, Some(rank), serde_json::json!({
                                "name": topic_name,
                                "path": serde_json::Value::Null,
                            }))
                        });
                        if entry.2.is_none() { entry.2 = Some(rank); }
                    }
                }
            }
        }

        // 4. RRF merge
        let mut scored: Vec<(String, f64, serde_json::Value)> = topic_scores
            .into_iter()
            .map(|(topic, (bonus, fts_rank, vec_rank, meta))| {
                let rrf = embedding::rrf_score(fts_rank, vec_rank, 60);
                (topic, bonus + rrf, meta)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored.iter().take(3).map(|(_, _, meta)| {
            SkillHint {
                name: meta.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                path: meta.get("path").and_then(|v| v.as_str()).map(|s| s.to_string()),
            }
        }).collect::<Vec<_>>()
    };

    tokio::time::timeout(Duration::from_millis(SKILL_TIMEOUT_MS), fut)
        .await
        .unwrap_or_else(|_| {
            debug!("Skill search timeout");
            vec![]
        })
}

async fn search_kb(state: &AppState, query: &str) -> Vec<KbHint> {
    let fut = async {
        let top_k = 10usize;

        // 1. FTS5 ranked (spawn_blocking)
        let q = query.to_string();
        let fts_ranked = state.db_exec.run(move |db| {
            let mut ranked = db.kb_search_fts_ranked(&q, None).unwrap_or_default();
            if ranked.is_empty() {
                ranked = db.kb_search_like_ranked(&q, None).unwrap_or_default();
            }
            Ok(ranked)
        }).await.unwrap_or_default();

        // 2. Embedding cosine similarity
        let query_embedding = state.embedding_service.as_ref()
            .and_then(|svc| svc.embed(query));
        let cache = state.kb_search_cache.read().await;
        let vec_ranked: Vec<(String, usize, f32)> = if let Some(ref qe) = query_embedding {
            let mut scores: Vec<(usize, f32)> = cache.iter()
                .enumerate()
                .map(|(i, (_, vec))| (i, embedding::cosine_similarity(qe, vec)))
                .collect();
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            scores.iter()
                .take(top_k * 3)
                .enumerate()
                .map(|(rank, (idx, sim))| (cache[*idx].0.clone(), rank, *sim))
                .collect()
        } else {
            Vec::new()
        };
        drop(cache);

        // 3. RRF merge
        let rrf_k = 60;
        let mut merged: HashMap<String, (Option<usize>, Option<usize>, Option<f32>)> = HashMap::new();
        for (id, rank) in &fts_ranked {
            merged.entry(id.clone()).or_insert((None, None, None)).0 = Some(*rank);
        }
        for (id, rank, sim) in &vec_ranked {
            let entry = merged.entry(id.clone()).or_insert((None, None, None));
            entry.1 = Some(*rank);
            entry.2 = Some(*sim);
        }
        let mut ranked: Vec<(String, f64)> = merged.into_iter()
            .map(|(id, (fts_r, vec_r, _sim))| {
                let score = embedding::rrf_score(fts_r, vec_r, rrf_k);
                (id, score)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k * 3);

        // 4. Fetch entries + temporal decay
        let db = state.mission.db();
        let now = chrono::Utc::now();
        let mut scored_entries: Vec<(KnowledgeEntry, f64)> = Vec::new();
        for (id, rrf) in &ranked {
            if let Ok(Some(entry)) = db.kb_get_by_id(id) {
                let age_days = chrono::DateTime::parse_from_rfc3339(&entry.updated_at)
                    .map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() as f64 / 24.0)
                    .unwrap_or(0.0);
                let decay = embedding::temporal_decay(&entry.category, age_days);
                scored_entries.push((entry, rrf * decay));
            }
        }
        scored_entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored_entries.truncate(top_k);

        scored_entries.into_iter().map(|(e, _)| KbHint {
            category: e.category,
            key: e.key,
            summary: e.summary,
        }).collect::<Vec<_>>()
    };

    tokio::time::timeout(Duration::from_millis(KB_TIMEOUT_MS), fut)
        .await
        .unwrap_or_else(|_| {
            debug!("KB search timeout");
            vec![]
        })
}

async fn search_code(state: &AppState, query: &str, _source: &PrefetchSource) -> Option<String> {
    let fut = crate::code_prefetch::code_prefetch_query(state, query);

    tokio::time::timeout(Duration::from_millis(CODE_TIMEOUT_MS), fut)
        .await
        .unwrap_or_else(|_| {
            debug!("Code prefetch timeout");
            None
        })
}

async fn search_task_ack(state: &AppState, ppid: Option<u32>) -> Vec<TaskUpdate> {
    let _ppid = match ppid {
        Some(p) => p,
        None => return vec![], // Only Hook source needs task ack
    };

    let fut = async {
        // Read watermark from daemon-side per-ppid cache
        // For now, delegate to existing DB function with since=last_1h fallback
        let db = state.mission.db();
        let tasks = db.ack_completed_tasks(None).unwrap_or_default();
        tasks.into_iter().map(|t| TaskUpdate {
            id: t.id[..8.min(t.id.len())].to_string(),
            slot_id: t.slot_id.unwrap_or_else(|| "?".to_string()),
            status: format!("{:?}", t.status).to_lowercase(),
            detail: match t.status {
                missiond_core::types::TaskStatus::Done =>
                    t.result.unwrap_or_else(|| "completed".to_string()).chars().take(200).collect(),
                _ =>
                    t.error.unwrap_or_else(|| "failed".to_string()).chars().take(200).collect(),
            },
        }).collect()
    };

    tokio::time::timeout(Duration::from_millis(TASK_ACK_TIMEOUT_MS), fut)
        .await
        .unwrap_or_else(|_| {
            debug!("Task ack timeout");
            vec![]
        })
}

// --- Assembly ---

fn assemble(
    skills: &[SkillHint],
    kb_entries: &[KbHint],
    code_context: &Option<String>,
    task_updates: &[TaskUpdate],
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if !skills.is_empty() {
        let mut block = "[Matched Skills — 建议先 Read 对应 Skill 文件]\n".to_string();
        for s in skills {
            block.push_str(&format!("- {}: {}\n", s.name, s.path.as_deref().unwrap_or("null")));
        }
        parts.push(block);
    }

    if !kb_entries.is_empty() {
        let mut block = "[Knowledge Base]\n".to_string();
        for e in kb_entries {
            block.push_str(&format!("- [{}] {}: {}\n", e.category, e.key, e.summary));
        }
        parts.push(block);
    }

    if let Some(ref code) = code_context {
        if !code.is_empty() {
            parts.push(format!("[Code Context]\n{}", code));
        }
    }

    if !task_updates.is_empty() {
        let mut block = "[Background Task Updates]\n".to_string();
        for t in task_updates {
            let icon = if t.status == "done" { "✅" } else { "❌" };
            block.push_str(&format!("- {} task {} (slot: {}): {}\n", icon, t.id, t.slot_id, t.detail));
        }
        parts.push(block);
    }

    parts.join("\n")
}

impl PrefetchResult {
    fn empty() -> Self {
        Self {
            skills: vec![],
            kb_entries: vec![],
            code_context: None,
            task_updates: vec![],
            assembled: String::new(),
        }
    }
}
