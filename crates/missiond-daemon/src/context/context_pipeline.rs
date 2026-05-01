//! Context Prefetch Pipeline v2 ("懂我")
//!
//! Unified context injection for all entry points:
//! - UserPromptSubmit Hook (Shell → IPC)
//! - iOS Jarvis (WebSocket)
//! - Autopilot (Board task dispatch)
//!
//! v2 architecture:
//! 1. Quick rule pre-screen (obvious chat/greeting → instant bypass)
//! 2. Router API intent routing (Claude semantic classification → bypass or refined queries)
//! 3. Cosine similarity cutoff (prevents "tallest dwarf" noise injection)
//! 4. Fan-out parallel search + RRF + budgeted assembly
//!
//! Fallback: if CC slot unavailable, degrades to v1 rule-based classification.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use missiond_core::embedding;
use missiond_core::types::KnowledgeEntry;

use crate::context::v3_blueprint_runtime::ConversationIngestionRuntimeConfig;
use crate::embedding_worker::resolve_llm_credentials;
use crate::state::AppState;

// --- Timeout budgets ---
const GLOBAL_TIMEOUT_MS: u64 = 700;
const KB_TIMEOUT_MS: u64 = 300;
const SKILL_TIMEOUT_MS: u64 = 100;
const CODE_TIMEOUT_MS: u64 = 600;
const TASK_ACK_TIMEOUT_MS: u64 = 50;

// --- Intent triage ---
/// Minimum RRF score for KB/Skill results. Below this → discard as noise.
/// Math: k=60, W_fts=0.4, W_vec=0.6 → pure Vec rank-1 = 0.00984, pure FTS rank-1 = 0.00656.
/// Threshold 0.008 tolerates single-side Top-1 hit.
const MIN_RRF_SCORE: f64 = 0.008;

/// Minimum cosine similarity for vector search candidates.
/// Entries below this are discarded BEFORE entering RRF ranking.
/// Prevents irrelevant queries (e.g. "what is 2+2") from injecting noise
/// via the "tallest dwarf" effect (rank-1 with low absolute similarity).
/// BGE-small-zh baseline for unrelated text is ~0.4-0.5; set conservatively.
const MIN_COSINE_SIM: f32 = 0.35;

// --- Token budget (char-based estimate: 1 token ≈ 2.5 CJK chars / 4 EN chars) ---
const GENERAL_TOKEN_BUDGET: usize = 2000;
const CODE_TOKEN_BUDGET: usize = 4000;

/// Estimate token count from text (rough char-based, no tokenizer).
fn estimate_tokens(text: &str) -> usize {
    // Mix of CJK and ASCII: ~2.5 chars per token on average
    (text.len() as f64 / 2.5) as usize
}

// --- Intent classification ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum QueryIntent {
    /// Greeting / chitchat / status check → bypass all search
    Chat,
    /// Code-related question → all dimensions
    Code,
    /// General question → KB + Skills, skip Code
    General,
}

/// Lightweight intent triage (pure rules, <0.1ms, no LLM call).
fn classify_intent(query: &str) -> QueryIntent {
    let q = query.trim();
    let q_lower = q.to_lowercase();
    let char_count = q.chars().count();

    // Greeting / chat patterns (CN + EN) — must be short
    static CHAT_PATTERNS: &[&str] = &[
        "hi",
        "hello",
        "hey",
        "你好",
        "嗨",
        "在吗",
        "收得到",
        "测试",
        "ping",
        "test",
        "谢谢",
        "thanks",
        "thank you",
        "ok",
        "好的",
        "好",
        "嗯",
        "对",
        "是的",
        "早上好",
        "晚上好",
        "早安",
        "晚安",
        "good morning",
        "good night",
    ];
    if char_count < 20 && CHAT_PATTERNS.iter().any(|p| q_lower.contains(p)) {
        return QueryIntent::Chat;
    }

    // Very short with no technical signal → Chat
    if char_count < 6 {
        let has_signal = q_lower.contains("::")
            || q_lower.contains("fn ")
            || q_lower.contains("err")
            || q_lower.contains("bug")
            || q_lower.contains("fix")
            || q_lower.contains("修")
            || q_lower.contains("改")
            || q_lower.contains("查")
            || q_lower.contains("看")
            || q_lower.contains("删")
            || q_lower.contains("加");
        if !has_signal {
            return QueryIntent::Chat;
        }
    }

    // Code signals → full pipeline
    static CODE_SIGNALS: &[&str] = &[
        "::",
        "fn ",
        "struct ",
        "impl ",
        "mod ",
        "crate",
        ".rs",
        ".ts",
        ".py",
        ".swift",
        ".go",
        "error",
        "bug",
        "fix",
        "compile",
        "build",
        "deploy",
        "refactor",
        "migration",
        "代码",
        "编译",
        "报错",
        "修复",
        "部署",
        "函数",
        "模块",
        "重构",
        "接口",
        "数据库",
    ];
    if CODE_SIGNALS.iter().any(|s| q_lower.contains(s)) {
        return QueryIntent::Code;
    }

    QueryIntent::General
}

// --- v2: Router API Intent Classification ---

/// Result of intent classification.
#[derive(Debug, Clone)]
struct IntentResult {
    /// Whether the query needs project-specific context injection.
    needs_context: bool,
    /// Classified intent type.
    intent_type: QueryIntent,
    /// Refined search queries (empty = use original query).
    search_queries: Vec<String>,
    /// How intent was determined.
    source: &'static str, // "router", "rules", "fallback"
}

/// Quick pre-screen for obvious chat (no API call needed).
/// Only catches the most obvious cases; ambiguous queries go to router.
fn is_obvious_chat(query: &str) -> bool {
    let q = query.trim();
    let q_lower = q.to_lowercase();
    let char_count = q.chars().count();

    static OBVIOUS_PATTERNS: &[&str] = &[
        "hi",
        "hello",
        "hey",
        "你好",
        "嗨",
        "在吗",
        "谢谢",
        "thanks",
        "thank you",
        "ok",
        "好的",
        "好",
        "嗯",
        "早上好",
        "晚上好",
        "早安",
        "晚安",
        "good morning",
        "good night",
        "测试",
        "ping",
        "test",
    ];
    if char_count < 20 && OBVIOUS_PATTERNS.iter().any(|p| q_lower.contains(p)) {
        return true;
    }

    if char_count < 6 {
        let has_signal = q_lower.contains("::")
            || q_lower.contains("fn ")
            || q_lower.contains("err")
            || q_lower.contains("bug")
            || q_lower.contains("fix");
        if !has_signal {
            return true;
        }
    }

    false
}

/// High-confidence DevOps heuristic — compound phrases that unambiguously indicate
/// deployment/infrastructure queries. Skips the 10s Router API call.
/// Rule: only compound phrases / proper nouns, never single generic words.
fn is_high_confidence_devops(query: &str) -> bool {
    let q = query.to_lowercase();
    const PATTERNS: &[&str] = &[
        "docker build",
        "docker compose",
        "docker image",
        "github action",
        "ci/cd",
        "k8s",
        "deploy agent",
        "deploy center",
        "backend-deploy",
        "xjp-deploy",
        "部署到",
        "发布到",
        "上线到",
    ];
    PATTERNS.iter().any(|p| q.contains(p))
}

/// Route intent via the configured XJP router API.
///
/// Stateless HTTP call — no slot contention, no conversation pollution.
/// Returns None on any error (caller falls back to rules).
async fn route_intent_via_router(state: &AppState, query: &str) -> Option<IntentResult> {
    state
        .stats
        .prefetch_router_calls
        .fetch_add(1, Ordering::Relaxed);
    let start = Instant::now();

    let result = route_intent_http(state, query).await;
    let elapsed_us = start.elapsed().as_micros() as u64;
    state.stats.prefetch_router_latency.record(elapsed_us);

    match result {
        Ok(intent) => Some(intent),
        Err(e) => {
            state
                .stats
                .prefetch_router_errors
                .fetch_add(1, Ordering::Relaxed);
            warn!(error = %e, elapsed_ms = elapsed_us / 1000, "Intent router: API call failed, falling back to rules");
            None
        }
    }
}

async fn route_intent_http(state: &AppState, query: &str) -> Result<IntentResult> {
    let (base_url, jwt) = resolve_llm_credentials().await?;
    let config = ConversationIngestionRuntimeConfig::load_for_current_dir()
        .map_err(|err| anyhow::anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))?;
    let model = config.intent_router_model.as_str();
    let timeout = config.intent_router_timeout();

    let prompt = format!(
        "Classify this user query. Respond with ONLY a JSON object, no markdown.\n\
         Query: \"{}\"\n\n\
         Output: {{\"needs_context\": bool, \"intent\": \"chat|code|general\", \"search_queries\": []}}\n\n\
         Rules:\n\
         - chat/math/greeting/general-knowledge/casual-conversation → needs_context: false, intent: \"chat\"\n\
         - project code/architecture/debug/deploy question → needs_context: true, intent: \"code\"\n\
         - project knowledge/config/ops question → needs_context: true, intent: \"general\"\n\
         - Mixed intent (greeting + technical) or unsure → needs_context: true, intent: \"general\"\n\
         - Deployment/release/containerization/CI-CD/server operations → needs_context: true, intent: \"code\"\n\
         - SEARCH QUERIES: If needs_context is true, extract 1-3 concise keywords optimized for retrieval. If false, leave empty [].",
        query.chars().take(300).collect::<String>()
    );

    let url = format!("{}/v1/chat/completions", base_url);
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 256,
    });

    let result = tokio::time::timeout(
        timeout,
        state
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", jwt))
            .json(&body)
            .send(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Intent router timeout ({}ms)", timeout.as_millis()))?
    .map_err(|e| anyhow::anyhow!("Intent router HTTP error: {}", e))?;

    let status = result.status();
    let resp: serde_json::Value = result
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Intent router: failed to parse response: {}", e))?;

    if !status.is_success() {
        let err_msg = resp
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(anyhow::anyhow!(
            "Intent router: API {} — {}",
            status,
            err_msg
        ));
    }

    let content = resp
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Intent router: no content in response"))?;

    info!(
        model = %model,
        response_preview = %content.chars().take(200).collect::<String>(),
        "Intent router: classification received"
    );

    parse_intent_response(content).ok_or_else(|| {
        anyhow::anyhow!(
            "Intent router: failed to parse JSON from: {}",
            content.chars().take(300).collect::<String>()
        )
    })
}

/// Parse JSON response from CC slot intent classification.
/// Tolerates markdown code fences and partial JSON.
fn parse_intent_response(response: &str) -> Option<IntentResult> {
    // Strip markdown code fences if present
    let json_str = response
        .trim()
        .strip_prefix("```json")
        .or_else(|| response.trim().strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(response)
        .trim();

    // Try to extract JSON object if there's surrounding text
    let json_str = if let (Some(start), Some(end)) = (json_str.find('{'), json_str.rfind('}')) {
        &json_str[start..=end]
    } else {
        json_str
    };

    #[derive(Deserialize)]
    struct RawIntent {
        needs_context: bool,
        #[serde(default)]
        intent: String,
        #[serde(default)]
        search_queries: Vec<String>,
    }

    let raw: RawIntent = serde_json::from_str(json_str).ok()?;

    let intent_type = match raw.intent.as_str() {
        "code" => QueryIntent::Code,
        "chat" => QueryIntent::Chat,
        _ => QueryIntent::General,
    };

    Some(IntentResult {
        needs_context: raw.needs_context,
        intent_type: if raw.needs_context {
            intent_type
        } else {
            QueryIntent::Chat
        },
        search_queries: raw.search_queries,
        source: "router",
    })
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PrefetchRequest {
    pub query: String,
    pub source: PrefetchSource,
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
}

fn default_token_budget() -> usize {
    4000
}

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
    pub id: String,
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
    /// Classified intent of the query.
    pub intent: Option<String>,
    /// KB entry IDs cited in this context (for causality tracking).
    pub cited_kb_ids: Vec<String>,
}

/// Execute the Context Prefetch Pipeline (v2).
///
/// Phase 1: Quick rule pre-screen (obvious chat → instant bypass)
/// Phase 2: CC slot intent routing (semantic classification → bypass or refined queries)
/// Phase 3: Selective fan-out search → score threshold → budgeted assembly
///
/// Fallback: CC unavailable → v1 rule-based classification + cosine cutoff.
pub(crate) async fn execute(state: &AppState, req: &PrefetchRequest) -> PrefetchResult {
    let query = req.query.chars().take(500).collect::<String>();
    if query.trim().is_empty() {
        return PrefetchResult::empty();
    }

    state.stats.prefetch_total.fetch_add(1, Ordering::Relaxed);

    // Phase 1: Quick rule pre-screen — obvious chat bypasses everything (no CC call)
    if is_obvious_chat(&query) {
        debug!(
            query = query.trim(),
            "Context prefetch: obvious chat → bypass"
        );
        state
            .stats
            .prefetch_intent_chat
            .fetch_add(1, Ordering::Relaxed);
        state.stats.prefetch_bypass.fetch_add(1, Ordering::Relaxed);
        return PrefetchResult {
            intent: Some("chat:rules".to_string()),
            ..PrefetchResult::empty()
        };
    }

    // Phase 1b: High-confidence DevOps heuristic — skip Router API for obvious deploy queries
    if is_high_confidence_devops(&query) {
        info!(
            query = query.trim(),
            "Context prefetch: DevOps heuristic → skip router, go to Phase 3"
        );
        state
            .stats
            .prefetch_devops_heuristic
            .fetch_add(1, Ordering::Relaxed);

        // Jump directly to Phase 3 search (Code intent, original query)
        let search_query = query.clone();
        let global_timeout = Duration::from_millis(GLOBAL_TIMEOUT_MS);
        let ppid = match &req.source {
            PrefetchSource::Hook { ppid } => Some(*ppid),
            _ => None,
        };

        state
            .stats
            .prefetch_intent_code
            .fetch_add(1, Ordering::Relaxed);

        let (skills, kb_entries, code_context, task_updates) = {
            let result = tokio::time::timeout(global_timeout, async {
                tokio::join!(
                    search_skills(state, &search_query),
                    search_kb(state, &search_query),
                    search_code(state, &query, &req.source),
                    search_task_ack(state, ppid),
                )
            })
            .await;
            match result {
                Ok((s, k, c, t)) => (s, k, c, t),
                Err(_) => {
                    warn!("Context prefetch global timeout ({}ms)", GLOBAL_TIMEOUT_MS);
                    (vec![], vec![], None, vec![])
                }
            }
        };

        let assembled = assemble_budgeted(
            &skills,
            &kb_entries,
            &code_context,
            &task_updates,
            CODE_TOKEN_BUDGET,
        );
        info!(
            intent = "code",
            routed_by = "devops_heuristic",
            skills = skills.len(),
            kb = kb_entries.len(),
            code = code_context.is_some(),
            tasks = task_updates.len(),
            assembled_len = assembled.len(),
            "Context prefetch complete (DevOps bypass)"
        );

        let cited_kb_ids = kb_entries.iter().map(|e| e.id.clone()).collect();
        return PrefetchResult {
            skills,
            kb_entries,
            code_context,
            task_updates,
            assembled,
            intent: Some("code:devops_heuristic".to_string()),
            cited_kb_ids,
        };
    }

    // Phase 2: CC slot intent routing (semantic classification)
    let intent_result = match route_intent_via_router(state, &query).await {
        Some(result) => {
            info!(
                query = query.trim(),
                needs_context = result.needs_context,
                intent = ?result.intent_type,
                search_queries = ?result.search_queries,
                "Intent router: classified"
            );
            result
        }
        None => {
            // Fallback: v1 rule-based classification
            state
                .stats
                .prefetch_fallback
                .fetch_add(1, Ordering::Relaxed);
            let rule_intent = classify_intent(&query);
            debug!(
                query = query.trim(),
                intent = ?rule_intent,
                "Intent router: fallback to rules"
            );
            IntentResult {
                needs_context: rule_intent != QueryIntent::Chat,
                intent_type: rule_intent,
                search_queries: vec![],
                source: "fallback",
            }
        }
    };

    // CC says no context needed → bypass
    if !intent_result.needs_context {
        debug!(
            source = intent_result.source,
            "Context prefetch: intent router → no context needed, bypass"
        );
        state
            .stats
            .prefetch_intent_chat
            .fetch_add(1, Ordering::Relaxed);
        state.stats.prefetch_bypass.fetch_add(1, Ordering::Relaxed);
        return PrefetchResult {
            intent: Some(format!("bypass:{}", intent_result.source)),
            ..PrefetchResult::empty()
        };
    }

    let intent = intent_result.intent_type;
    match intent {
        QueryIntent::Code => state
            .stats
            .prefetch_intent_code
            .fetch_add(1, Ordering::Relaxed),
        QueryIntent::General => state
            .stats
            .prefetch_intent_general
            .fetch_add(1, Ordering::Relaxed),
        QueryIntent::Chat => unreachable!(),
    };

    // Use refined search queries if CC provided them, otherwise use original query
    let search_query = if intent_result.search_queries.is_empty() {
        query.clone()
    } else {
        intent_result.search_queries.join(" ")
    };

    let global_timeout = Duration::from_millis(GLOBAL_TIMEOUT_MS);

    // Determine whether to include task_ack (only for Hook source)
    let ppid = match &req.source {
        PrefetchSource::Hook { ppid } => Some(*ppid),
        _ => None,
    };

    // Phase 3: Selective fan-out based on intent
    let (skills, kb_entries, code_context, task_updates) = match intent {
        QueryIntent::Code => {
            // Full pipeline: Skills + KB + Code + TaskAck
            let result = tokio::time::timeout(global_timeout, async {
                tokio::join!(
                    search_skills(state, &search_query),
                    search_kb(state, &search_query),
                    search_code(state, &query, &req.source), // code search uses original query
                    search_task_ack(state, ppid),
                )
            })
            .await;
            match result {
                Ok((s, k, c, t)) => (s, k, c, t),
                Err(_) => {
                    warn!("Context prefetch global timeout ({}ms)", GLOBAL_TIMEOUT_MS);
                    (vec![], vec![], None, vec![])
                }
            }
        }
        QueryIntent::General => {
            // KB + Skills only, skip Code (saves 600ms)
            let result = tokio::time::timeout(global_timeout, async {
                tokio::join!(
                    search_skills(state, &search_query),
                    search_kb(state, &search_query),
                    search_task_ack(state, ppid),
                )
            })
            .await;
            match result {
                Ok((s, k, t)) => (s, k, None, t),
                Err(_) => {
                    warn!("Context prefetch global timeout ({}ms)", GLOBAL_TIMEOUT_MS);
                    (vec![], vec![], None, vec![])
                }
            }
        }
        QueryIntent::Chat => unreachable!(), // handled above
    };

    let intent_str = match intent {
        QueryIntent::Code => "code",
        QueryIntent::General => "general",
        QueryIntent::Chat => "chat",
    };

    let routed_by = intent_result.source;

    let token_budget = match intent {
        QueryIntent::Code => CODE_TOKEN_BUDGET,
        QueryIntent::General => GENERAL_TOKEN_BUDGET,
        QueryIntent::Chat => 0,
    };

    let assembled = assemble_budgeted(
        &skills,
        &kb_entries,
        &code_context,
        &task_updates,
        token_budget,
    );

    info!(
        intent = intent_str,
        routed_by,
        skills = skills.len(),
        kb = kb_entries.len(),
        code = code_context.is_some(),
        tasks = task_updates.len(),
        assembled_len = assembled.len(),
        "Context prefetch complete"
    );

    let cited_kb_ids: Vec<String> = kb_entries.iter().map(|e| e.id.clone()).collect();

    // Log KB co-access for preemptive prefetch pattern learning
    // Phase 4b: pass session_id when available. Autopilot tasks use task_id as proxy —
    // each autopilot task maps 1:1 to a PTY session; retro_worker only processes user
    // sessions (different IDs), so no cross-contamination.
    if cited_kb_ids.len() >= 2 {
        let ids = cited_kb_ids.clone();
        let session_id = match &req.source {
            PrefetchSource::Autopilot { task_id } => Some(task_id.as_str()),
            _ => None,
        };
        let _ = state
            .store
            .kb_log_co_access(&ids, "prefetch", session_id)
            .await;
    }

    PrefetchResult {
        skills,
        kb_entries,
        code_context,
        task_updates,
        assembled,
        intent: Some(format!("{}:{}", intent_str, routed_by)),
        cited_kb_ids,
    }
}

// --- Individual search functions (each with its own timeout + score threshold) ---

async fn search_skills(state: &AppState, query: &str) -> Vec<SkillHint> {
    let fut = async {
        let query_lower = query.to_lowercase();
        let mut topic_scores: HashMap<
            String,
            (f64, Option<usize>, Option<usize>, serde_json::Value),
        > = HashMap::new();

        // Step 0: Trigger hardmatch — exact phrases from skill frontmatter, score=1.0
        let mut trigger_matched: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for s in state.skills.list() {
            if trigger_matched.contains(&s.name) {
                continue; // Dedup: one skill matched is enough
            }
            if let Some(ref triggers) = s.triggers {
                for trigger in triggers {
                    if query_lower.contains(&trigger.to_lowercase()) {
                        debug!(skill = %s.name, trigger = %trigger, "Skill trigger hardmatch");
                        state
                            .stats
                            .prefetch_skill_trigger_hit
                            .fetch_add(1, Ordering::Relaxed);
                        topic_scores.insert(
                            s.name.clone(),
                            (
                                1.0,
                                None,
                                None,
                                serde_json::json!({
                                    "name": s.name,
                                    "path": s.path,
                                }),
                            ),
                        );
                        trigger_matched.insert(s.name.clone());
                        break; // First trigger match per skill is enough
                    }
                }
            }
        }

        // 1. Name/aka exact match → bonus +0.3
        for s in state.skills.search(query).iter().take(10) {
            topic_scores.entry(s.name.clone()).or_insert_with(|| {
                (
                    0.3,
                    None,
                    None,
                    serde_json::json!({
                        "name": s.name,
                        "path": s.path,
                    }),
                )
            });
        }

        // 2. FTS5
        if let Ok(fts_results) = state.store.skill_search_fts(query).await {
            for (rank, r) in fts_results.iter().take(20).enumerate() {
                topic_scores.entry(r.topic.clone()).or_insert_with(|| {
                    (
                        0.0,
                        Some(rank),
                        None,
                        serde_json::json!({
                            "name": r.topic,
                            "path": r.file_path,
                        }),
                    )
                });
                if let Some(entry) = topic_scores.get_mut(&r.topic) {
                    if entry.1.is_none() {
                        entry.1 = Some(rank);
                    }
                }
            }
        }

        // 3. Embedding cosine similarity
        if let Some(ref emb_svc) = state.embedding_service {
            if let Some(query_vec) = emb_svc.embed(query) {
                let cache = state.skill_embedding_cache.read().await;
                if !cache.is_empty() {
                    let mut sims: Vec<(usize, f32)> = cache
                        .iter()
                        .enumerate()
                        .map(|(i, (_, vec))| (i, embedding::cosine_similarity(&query_vec, vec)))
                        .collect();
                    sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                    // Log top-1 sim for threshold calibration
                    if let Some(&(idx0, sim0)) = sims.first() {
                        let name0 = &cache[idx0].0;
                        debug!(query, top1_skill = %name0, top1_sim = sim0, "Skill vector top-1 sim");
                        if sim0 <= MIN_COSINE_SIM {
                            state
                                .stats
                                .prefetch_cosine_filtered
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    for (rank, (idx, _sim)) in sims
                        .iter()
                        .filter(|(_, s)| *s > MIN_COSINE_SIM)
                        .take(10)
                        .enumerate()
                    {
                        let topic_name = &cache[*idx].0;
                        let entry = topic_scores.entry(topic_name.clone()).or_insert_with(|| {
                            (
                                0.0,
                                None,
                                Some(rank),
                                serde_json::json!({
                                    "name": topic_name,
                                    "path": serde_json::Value::Null,
                                }),
                            )
                        });
                        if entry.2.is_none() {
                            entry.2 = Some(rank);
                        }
                    }
                }
            }
        }

        // 4. RRF merge + score threshold
        let mut scored: Vec<(String, f64, serde_json::Value)> = topic_scores
            .into_iter()
            .map(|(topic, (bonus, fts_rank, vec_rank, meta))| {
                let rrf = embedding::rrf_score(fts_rank, vec_rank, 60);
                (topic, bonus + rrf, meta)
            })
            .filter(|(_, score, _)| *score >= MIN_RRF_SCORE) // ← score threshold
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Top-K: 3 base + trigger-matched extras (so triggers don't squeeze out semantic results)
        let top_k = 3 + trigger_matched.len();
        scored
            .iter()
            .take(top_k)
            .map(|(_, _, meta)| SkillHint {
                name: meta
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                path: meta
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
            .collect::<Vec<_>>()
    };

    tokio::time::timeout(Duration::from_millis(SKILL_TIMEOUT_MS), fut)
        .await
        .unwrap_or_else(|_| {
            debug!("Skill search timeout");
            vec![]
        })
}

/// Category → daily decay factor for read-time utility computation.
/// daily_factor = 0.5^(1/half_life_days). Non-decaying categories return 1.0.
fn category_daily_factor(category: &str) -> f64 {
    if category == "memory:debug" {
        return 0.9517;
    } // 14d
    if category == "memory:bugfix" {
        return 0.9772;
    } // 30d
    if category == "memory:ops" {
        return 0.9675;
    } // 21d
    if category == "memory:feature" {
        return 0.9923;
    } // 90d
    if category.starts_with("memory") {
        return 0.9885;
    } // 60d
    1.0 // non-decaying
}

async fn search_kb(state: &AppState, query: &str) -> Vec<KbHint> {
    let fut = async {
        let top_k = 10usize;

        // 1. FTS5 ranked
        let fts_ranked: Vec<(String, usize, Option<String>)> = {
            let ranked = state
                .store
                .kb_search_fts_ranked(query, None)
                .await
                .unwrap_or_default();
            if ranked.is_empty() {
                let like = state
                    .store
                    .kb_search_like_ranked(query, None)
                    .await
                    .unwrap_or_default();
                like.into_iter()
                    .map(|(id, rank)| (id, rank, None))
                    .collect()
            } else {
                ranked
            }
        };

        // 2. Embedding cosine similarity
        let query_embedding = state
            .embedding_service
            .as_ref()
            .and_then(|svc| svc.embed(query));
        let cache = state.kb_search_cache.read().await;
        let vec_ranked: Vec<(String, usize, f32)> = if let Some(ref qe) = query_embedding {
            let mut scores: Vec<(usize, f32)> = cache
                .iter()
                .enumerate()
                .map(|(i, (_, vec))| (i, embedding::cosine_similarity(qe, vec)))
                .collect();
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            // Log top-1 sim for threshold calibration
            if let Some(&(idx0, sim0)) = scores.first() {
                let name0 = &cache[idx0].0;
                debug!(query, top1_kb = %name0, top1_sim = sim0, "KB vector top-1 sim");
                if sim0 <= MIN_COSINE_SIM {
                    state
                        .stats
                        .prefetch_cosine_filtered
                        .fetch_add(1, Ordering::Relaxed);
                }
            }

            scores
                .iter()
                .filter(|(_, sim)| *sim > MIN_COSINE_SIM) // cosine cutoff before ranking
                .take(top_k * 3)
                .enumerate()
                .map(|(rank, (idx, sim))| (cache[*idx].0.clone(), rank, *sim))
                .collect()
        } else {
            Vec::new()
        };
        drop(cache);

        // 3. RRF merge + score threshold
        let rrf_k = 60;
        let mut merged: HashMap<String, (Option<usize>, Option<usize>, Option<f32>)> =
            HashMap::new();
        for (id, rank, _snippet) in &fts_ranked {
            merged.entry(id.clone()).or_insert((None, None, None)).0 = Some(*rank);
        }
        for (id, rank, sim) in &vec_ranked {
            let entry = merged.entry(id.clone()).or_insert((None, None, None));
            entry.1 = Some(*rank);
            entry.2 = Some(*sim);
        }
        let mut ranked: Vec<(String, f64)> = merged
            .into_iter()
            .map(|(id, (fts_r, vec_r, _sim))| {
                let score = embedding::rrf_score(fts_r, vec_r, rrf_k);
                (id, score)
            })
            .filter(|(_, score)| *score >= MIN_RRF_SCORE) // ← score threshold
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k * 3);

        // 4. Fetch entries + temporal decay
        let now = chrono::Utc::now();
        let mut scored_entries: Vec<(KnowledgeEntry, f64)> = Vec::new();
        for (id, rrf) in &ranked {
            if let Ok(Some(entry)) = state.store.kb_get_by_id(id).await {
                // Working Memory scope: skip scratchpad entries in global retrieval
                if entry.scope_task_id.is_some() {
                    continue;
                }
                let age_days = chrono::DateTime::parse_from_rfc3339(&entry.updated_at)
                    .map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() as f64 / 24.0)
                    .unwrap_or(0.0);
                let decay = embedding::temporal_decay(&entry.category, age_days);
                // Hybrid scoring: weighted sum (Gemini ARB: MAX loses information)
                // 0.3 * temporal_decay + 0.7 * effective_utility
                // effective_utility = read-time decay of base utility_score
                let daily_factor = category_daily_factor(&entry.category);
                let effective_utility = entry.utility_score * daily_factor.powf(age_days);
                let relevance = 0.3 * decay + 0.7 * effective_utility;
                scored_entries.push((entry.clone(), rrf * relevance * entry.confidence));
            }
        }
        scored_entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored_entries.truncate(top_k);

        // Knowledge Graph expansion: follow 1-hop edges to include related entries
        let primary_ids: Vec<String> = scored_entries.iter().map(|(e, _)| e.id.clone()).collect();
        if !primary_ids.is_empty() {
            if let Ok(related_ids) = state.store.kb_expand_related(&primary_ids, 3).await {
                for rid in &related_ids {
                    if let Ok(Some(re)) = state.store.kb_get_by_id(rid).await {
                        if re.scope_task_id.is_none() {
                            scored_entries.push((re, 0.0)); // appended at lower priority
                        }
                    }
                }
            }
        }

        // Co-access preloading: preload frequently co-accessed entries
        {
            let cache = state.kb_cooccurrence_cache.read().await;
            let existing_ids: HashSet<String> =
                scored_entries.iter().map(|(e, _)| e.id.clone()).collect();
            let mut to_add: Vec<(KnowledgeEntry, f64)> = Vec::new();
            for (e, _) in scored_entries.iter() {
                if to_add.len() >= 2 {
                    break;
                }
                if let Some(co_ids) = cache.get(&e.id) {
                    for co_id in co_ids {
                        if to_add.len() >= 2 {
                            break;
                        }
                        if !existing_ids.contains(co_id) {
                            if let Ok(Some(co_entry)) = state.store.kb_get_by_id(co_id).await {
                                if co_entry.scope_task_id.is_none() {
                                    to_add.push((co_entry, 0.0));
                                }
                            }
                        }
                    }
                }
            }
            scored_entries.extend(to_add);
        }

        scored_entries
            .into_iter()
            .map(|(e, _)| KbHint {
                id: e.id,
                category: e.category,
                key: e.key,
                summary: e.summary,
            })
            .collect::<Vec<_>>()
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
        let tasks = state
            .store
            .ack_completed_tasks(None)
            .await
            .unwrap_or_default();
        tasks
            .into_iter()
            .map(|t| TaskUpdate {
                id: t.id[..8.min(t.id.len())].to_string(),
                slot_id: t.slot_id.unwrap_or_else(|| "?".to_string()),
                status: format!("{:?}", t.status).to_lowercase(),
                detail: match t.status {
                    missiond_core::types::TaskStatus::Done => t
                        .result
                        .unwrap_or_else(|| "completed".to_string())
                        .chars()
                        .take(200)
                        .collect(),
                    _ => t
                        .error
                        .unwrap_or_else(|| "failed".to_string())
                        .chars()
                        .take(200)
                        .collect(),
                },
            })
            .collect()
    };

    tokio::time::timeout(Duration::from_millis(TASK_ACK_TIMEOUT_MS), fut)
        .await
        .unwrap_or_else(|_| {
            debug!("Task ack timeout");
            vec![]
        })
}

// --- Budgeted Assembly ---

/// Assemble context with token budget control.
/// Priority: TaskAck (guaranteed) > Top-1 Skill (guaranteed) > KB > remaining Skills > Code.
fn assemble_budgeted(
    skills: &[SkillHint],
    kb_entries: &[KbHint],
    code_context: &Option<String>,
    task_updates: &[TaskUpdate],
    token_budget: usize,
) -> String {
    if token_budget == 0 {
        return String::new();
    }

    let mut parts: Vec<String> = Vec::new();
    let mut used_tokens: usize = 0;

    // 1. TaskAck — guaranteed (small, high priority)
    if !task_updates.is_empty() {
        let mut block = "[Background Task Updates]\n".to_string();
        for t in task_updates {
            let icon = if t.status == "done" { "✅" } else { "❌" };
            block.push_str(&format!(
                "- {} task {} (slot: {}): {}\n",
                icon, t.id, t.slot_id, t.detail
            ));
        }
        let tokens = estimate_tokens(&block);
        if used_tokens + tokens <= token_budget {
            used_tokens += tokens;
            parts.push(block);
        }
    }

    // 2. Skills — top-1 guaranteed, rest budget-gated
    if !skills.is_empty() {
        let mut block = "[Matched Skills — 建议先 Read 对应 Skill 文件]\n".to_string();
        for s in skills {
            block.push_str(&format!(
                "- {}: {}\n",
                s.name,
                s.path.as_deref().unwrap_or("null")
            ));
        }
        let tokens = estimate_tokens(&block);
        if used_tokens + tokens <= token_budget {
            used_tokens += tokens;
            parts.push(block);
        } else if !parts.is_empty() || used_tokens > 0 {
            // At least inject top-1 skill
            let s = &skills[0];
            let mini = format!(
                "[Matched Skills]\n- {}: {}\n",
                s.name,
                s.path.as_deref().unwrap_or("null")
            );
            let tokens = estimate_tokens(&mini);
            if used_tokens + tokens <= token_budget {
                used_tokens += tokens;
                parts.push(mini);
            }
        }
    }

    // 3. KB — budget-gated, truncate from bottom
    if !kb_entries.is_empty() {
        let header = "[Knowledge Base]\n";
        let mut block = header.to_string();
        let mut kb_tokens = estimate_tokens(header);

        for e in kb_entries {
            let line = format!(
                "- [{}] {}: {} [KB:{}]\n",
                e.category,
                e.key,
                e.summary,
                &e.id[..8.min(e.id.len())]
            );
            let line_tokens = estimate_tokens(&line);
            if used_tokens + kb_tokens + line_tokens > token_budget {
                break; // truncate remaining KB entries
            }
            kb_tokens += line_tokens;
            block.push_str(&line);
        }

        if kb_tokens > estimate_tokens(header) {
            used_tokens += kb_tokens;
            parts.push(block);
        }
    }

    // 4. Code — budget-gated (largest, lowest priority in assembly)
    if let Some(ref code) = code_context {
        if !code.is_empty() {
            let block = format!("[Code Context]\n{}", code);
            let tokens = estimate_tokens(&block);
            if used_tokens + tokens <= token_budget {
                parts.push(block);
            }
            // If code exceeds remaining budget, skip entirely
            // (code_prefetch already has its own internal budget)
        }
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
            intent: None,
            cited_kb_ids: vec![],
        }
    }
}
