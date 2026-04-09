use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::helpers::char_boundary_at;
use crate::helpers::default_mission_home;
use crate::state::{AppState, BackfillPhase, EmbeddingTask};
use std::path::PathBuf;
use std::sync::Arc;

// ── Message-level embedding filter ──

enum EmbedDecision {
    Embed,
    Skip(&'static str),
}

/// Determine if a message should be embedded or skipped.
/// Design: docs/designs/message-level-embedding.md
fn should_embed(role: &str, content: &str) -> EmbedDecision {
    if role == "system" {
        return EmbedDecision::Skip("system");
    }
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return EmbedDecision::Skip("empty");
    }
    // Hard ceiling: prevent OOM on mega-paste / base64 blobs
    if content.len() > 100_000 {
        return EmbedDecision::Skip("too_long");
    }
    if role == "tool" && content.len() > 2000 {
        return EmbedDecision::Skip("tool_noise");
    }
    if trimmed.chars().count() < 20 {
        return EmbedDecision::Skip("too_short");
    }
    EmbedDecision::Embed
}

/// Quick pre-filter (no allocation) for the hot path in message_handler.
/// Returns true if the message is definitely skippable without needing content.clone().
pub(crate) fn should_skip_fast(role: &str, content_len: usize) -> bool {
    role == "system" || content_len == 0 || content_len > 100_000
}

#[derive(serde::Deserialize)]
pub(crate) struct LlmConfig {
    #[serde(default = "LlmConfig::default_provider")]
    pub(crate) provider: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    auth: LlmAuth,
    #[serde(default = "LlmConfig::default_model")]
    #[allow(dead_code)]
    default_model: String,
    #[serde(default)]
    pub(crate) gemini_cli: Option<GeminiCliConfig>,
    /// Gemini API key for multimodal (video/image) File API uploads.
    /// Get from https://aistudio.google.com/app/apikey
    #[serde(default)]
    pub(crate) gemini_api_key: Option<String>,
    /// Enable embedding backfill pipeline (default: false).
    /// Set to true to run cursor-based backfill across all phases on startup.
    #[serde(default)]
    pub(crate) backfill_enabled: bool,
    /// Enable Intent Analyst (Phase 6). Default: false.
    /// When enabled, analyzes conversation turns via Sonnet to detect stuck retries, direction shifts, etc.
    #[serde(default)]
    pub(crate) intent_analyst_enabled: bool,
}

#[derive(serde::Deserialize, Clone)]
pub(crate) struct GeminiCliConfig {
    #[serde(default = "GeminiCliConfig::default_binary")]
    pub binary: String,
    #[serde(default = "GeminiCliConfig::default_model")]
    pub model: String,
    #[serde(default = "GeminiCliConfig::default_timeout")]
    pub timeout: u64,
}

impl Default for GeminiCliConfig {
    fn default() -> Self {
        Self {
            binary: Self::default_binary(),
            model: Self::default_model(),
            timeout: Self::default_timeout(),
        }
    }
}

impl GeminiCliConfig {
    fn default_binary() -> String {
        "gemini".to_string()
    }
    fn default_model() -> String {
        "gemini-3.1-pro-preview".to_string()
    }
    fn default_timeout() -> u64 {
        120
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub(crate) enum LlmAuth {
    /// Read bearer token from an environment variable.
    #[serde(rename = "bearer_env")]
    BearerEnv { env: String },
    /// Read bearer token from a JSON file (extract a specific key).
    #[serde(rename = "bearer_file")]
    BearerFile { path: String, key: String },
    /// No authentication.
    #[serde(rename = "none")]
    None,
}

impl Default for LlmAuth {
    fn default() -> Self {
        LlmAuth::None
    }
}

impl LlmConfig {
    fn default_provider() -> String {
        "xjp-router".to_string()
    }
    fn default_model() -> String {
        "gpt-4o".to_string()
    }
}

// ── Ollama Embedding Provider ────────────────────────────────────────────

/// Embedding provider that calls Ollama's `/api/embed` endpoint.
/// Preferred over FastEmbed when available (Qwen3-Embedding, higher quality).
///
/// Uses `block_in_place` + `Handle::block_on` to bridge sync trait → async HTTP.
pub(crate) struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    dimension: usize,
    provider_id_str: String,
}

impl OllamaProvider {
    /// Try to connect to Ollama and verify the embedding model is available.
    /// Returns None if Ollama is unreachable or the model is missing.
    async fn try_new(client: &reqwest::Client) -> Option<Self> {
        let base_url =
            std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("MISSIOND_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "qwen3-embedding".to_string());

        // Health check: verify Ollama is reachable
        let health = client
            .get(format!("{}/api/tags", base_url))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .ok()?;
        if !health.status().is_success() {
            return None;
        }

        // Probe: generate one embedding to verify model and detect dimension
        let probe_vec = Self::embed_one(client, &base_url, &model, "dimension probe").await?;
        let dimension = probe_vec.len();
        let provider_id_str = format!("ollama-{}", model);

        info!(
            model = %model, dimension, provider_id = %provider_id_str,
            "Ollama embedding provider initialized"
        );

        Some(Self {
            client: client.clone(),
            base_url,
            model,
            dimension,
            provider_id_str,
        })
    }

    /// Single embedding via Ollama /api/embed endpoint.
    async fn embed_one(
        client: &reqwest::Client,
        base_url: &str,
        model: &str,
        text: &str,
    ) -> Option<Vec<f32>> {
        let resp = client
            .post(format!("{}/api/embed", base_url))
            .json(&serde_json::json!({
                "model": model,
                "input": text,
            }))
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            tracing::warn!(status = %resp.status(), "Ollama embed request failed");
            return None;
        }

        let data: serde_json::Value = resp.json().await.ok()?;
        // Ollama /api/embed returns { "embeddings": [[f32, ...]] }
        data.get("embeddings")
            .and_then(|e| e.get(0))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            })
    }

    /// Batch embedding via Ollama /api/embed (supports array input).
    async fn embed_many(
        client: &reqwest::Client,
        base_url: &str,
        model: &str,
        texts: &[String],
    ) -> Vec<Option<Vec<f32>>> {
        let resp = client
            .post(format!("{}/api/embed", base_url))
            .json(&serde_json::json!({
                "model": model,
                "input": texts,
            }))
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                if let Ok(data) = r.json::<serde_json::Value>().await {
                    if let Some(embeddings) = data.get("embeddings").and_then(|e| e.as_array()) {
                        return embeddings
                            .iter()
                            .map(|emb| {
                                emb.as_array().map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                                        .collect()
                                })
                            })
                            .collect();
                    }
                }
                texts.iter().map(|_| None).collect()
            }
            _ => texts.iter().map(|_| None).collect(),
        }
    }
}

impl missiond_core::embedding::EmbeddingProvider for OllamaProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id_str
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed(&self, text: &str) -> Option<Vec<f32>> {
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let model = self.model.clone();
        let text = text.to_string();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(Self::embed_one(&client, &base_url, &model, &text))
        })
    }

    fn embed_batch(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        if texts.is_empty() {
            return Vec::new();
        }
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let model = self.model.clone();
        let texts = texts.to_vec();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(Self::embed_many(&client, &base_url, &model, &texts))
        })
    }
}

/// Initialize embedding provider: try Ollama first (better quality), fall back to FastEmbed.
///
/// Set `MISSIOND_DISABLE_EMBEDDING=1` to skip entirely (useful on CPUs without AVX2).

pub(crate) async fn init_embedding_provider(
    http_client: &reqwest::Client,
) -> Option<Arc<dyn missiond_core::embedding::EmbeddingProvider>> {
    if std::env::var("MISSIOND_DISABLE_EMBEDDING").is_ok() {
        info!("Embedding disabled via MISSIOND_DISABLE_EMBEDDING");
        return None;
    }

    // Try Ollama first
    if let Some(ollama) = OllamaProvider::try_new(http_client).await {
        return Some(Arc::new(ollama));
    }
    info!("Ollama not available, falling back to FastEmbed");

    // Fallback to FastEmbed (requires AVX2)
    missiond_core::embedding::EmbeddingService::new()
        .map(|svc| Arc::new(svc) as Arc<dyn missiond_core::embedding::EmbeddingProvider>)
}

/// Generate LLM summary + multi-topic embeddings for a conversation.
///
/// Pipeline:
/// 1. Fetch last N messages from DB
/// 2. Build conversation text
/// 3. Call Router LLM for structured topic list (JSON array of topic summaries)
/// 4. Store combined summary to DB (for display)
/// 5. Generate embedding for EACH topic independently
/// 6. Store topic vectors to DB + update TopicCache
pub(crate) async fn generate_and_store_conv_embedding(state: &AppState, session_id: &str) {
    // 0. Skip memory/agent slot conversations (content is AI thinking/tool_call, not user topics)
    if let Ok(Some(conv)) = state.store.get_conversation(session_id).await {
        if conv.conversation_type == "memory" || conv.slot_id.as_deref() == Some("slot-memory") {
            debug!(session = %session_id, "Skipping memory slot conversation for embedding");
            return;
        }
    }

    // 1. Fetch messages
    debug!(session = %session_id, "Topic pipeline: fetching messages");
    let messages = match state
        .store
        .get_conversation_messages(session_id, None, 200)
        .await
    {
        Ok(msgs) if !msgs.is_empty() => msgs,
        Ok(_) => {
            debug!(session = %session_id, "No messages for topic extraction");
            return;
        }
        Err(e) => {
            warn!(session = %session_id, error = %e, "Failed to fetch messages");
            return;
        }
    };

    // 2. Build conversation text for LLM
    let mut conv_text = String::with_capacity(8192);
    let has_timeline = if let Ok(Some(conv)) = state.store.get_conversation(session_id).await {
        if let Some(ref tl_json) = conv.session_timeline {
            if let Ok(timeline) = serde_json::from_str::<Vec<serde_json::Value>>(tl_json) {
                if !timeline.is_empty() {
                    conv_text.push_str("=== 会话历史阶段摘要 ===\n");
                    let fragments: Vec<&serde_json::Value> = if timeline.len() > 10 {
                        let mut selected: Vec<&serde_json::Value> =
                            timeline.iter().take(3).collect();
                        let mid = timeline.len() / 2;
                        if mid > 3 && mid < timeline.len() - 3 {
                            selected.push(&timeline[mid - 1]);
                            selected.push(&timeline[mid]);
                        }
                        selected.extend(
                            timeline
                                .iter()
                                .rev()
                                .take(3)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev(),
                        );
                        selected
                    } else {
                        timeline.iter().collect()
                    };
                    for entry in &fragments {
                        let idx = entry
                            .get("shard_index")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        if let Some(summary) = entry.get("summary").and_then(|v| v.as_str()) {
                            let truncated = if summary.len() > 1200 {
                                &summary[..char_boundary_at(summary, 1200)]
                            } else {
                                summary
                            };
                            conv_text.push_str(&format!("[阶段{}] {}\n", idx, truncated));
                        }
                    }
                    conv_text.push_str("=== 最近消息 ===\n");
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    // Fetch briefing summaries: semantic summaries are much more information-dense
    // than raw content, improving topic extraction quality while reducing token usage.
    let briefing_map = state
        .store
        .get_briefing_summaries_for_session(session_id)
        .await
        .unwrap_or_default();

    let msg_budget = if has_timeline { 4000 } else { 8000 };
    for msg in &messages {
        let prefix = match msg.role.as_str() {
            "user" => "U",
            "assistant" => "A",
            "tool_use" => "T",
            "tool_result" => "R",
            _ => "?",
        };
        // Prefer briefing summary over raw content ("优先 summary，兜底 raw")
        let content = if let Some(summary) = briefing_map.get(&msg.id) {
            summary.clone()
        } else if msg.content.len() > 500 {
            format!("{}…", &msg.content[..char_boundary_at(&msg.content, 500)])
        } else {
            msg.content.clone()
        };
        conv_text.push_str(&format!("[{}] {}\n", prefix, content));
        if conv_text.len() > msg_budget {
            break;
        }
    }

    // 3. Extract structured topics via LLM — fast fail, no fallback
    debug!(session = %session_id, text_len = conv_text.len(), "Topic pipeline: extracting topics");
    let topics: Vec<Topic> = match extract_conv_topics_llm(state, &conv_text, has_timeline).await {
        Some(t) => {
            debug!(session = %session_id, count = t.len(), "Topics extracted");
            t
        }
        None => {
            warn!(session = %session_id, "Topic extraction failed, skipping session");
            return;
        }
    };

    // 4. Store combined summary for display (llm_summary column)
    let combined_summary = if topics.len() == 1 {
        topics[0].as_str().to_string()
    } else {
        topics
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{}. {}", i + 1, t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if let Err(e) = state
        .store
        .set_conversation_summary(session_id, &combined_summary)
        .await
    {
        warn!(session = %session_id, error = %e, "Failed to store summary");
        return;
    }

    // 5. Generate embedding for EACH topic
    let embedding_service = match &state.embedding_service {
        Some(svc) => svc,
        None => {
            debug!(session = %session_id, "No embedding service, skipping");
            return;
        }
    };
    let provider_id = embedding_service.provider_id().to_string();

    let mut topic_vectors: Vec<(String, Vec<f32>)> = Vec::new();
    for topic in &topics {
        let svc = Arc::clone(embedding_service);
        let text = topic.as_str().to_string();
        match tokio::task::spawn_blocking(move || svc.embed(&text)).await {
            Ok(Some(vec)) => {
                topic_vectors.push((topic.as_str().to_string(), vec));
            }
            Ok(None) => {
                warn!(session = %session_id, topic = %topic, "Embedding returned None");
            }
            Err(e) => {
                warn!(session = %session_id, error = %e, "Embedding spawn_blocking panicked");
            }
        }
    }

    if topic_vectors.is_empty() {
        warn!(session = %session_id, "All topic embeddings failed");
        return;
    }

    // 6. Store topic vectors to DB
    if let Err(e) = state
        .store
        .set_conversation_topic_vectors(session_id, &topic_vectors, &provider_id)
        .await
    {
        warn!(session = %session_id, error = %e, "Failed to store topic vectors");
        return;
    }
    // Also update old single-vec for backwards compat (first topic as representative)
    let _ = state
        .store
        .set_conversation_embedding(session_id, &topic_vectors[0].1, &provider_id)
        .await;

    // TopicCache removed in P3 — pgvector HNSW replaces in-memory search
    info!(session = %session_id, topic_count = topics.len(), "Multi-topic embeddings generated");
}

// ── Per-turn embedding (Phase 5 S4) ────────────────────────────────────

/// Process per-turn embeddings for a session. Zero LLM — embeds turn content directly.
/// Input: user_content + tool_names + AI final response (denoised).
async fn process_turns_for_session(
    state: &AppState,
    session_id: &str,
    provider_id: &str,
) -> Result<()> {
    let embedding_service = state
        .embedding_service
        .as_ref()
        .ok_or_else(|| anyhow!("No embedding service"))?;

    // 1. Query turns that lack topic_vectors
    let pending = state
        .store
        .turns_pending_embedding(session_id, provider_id)
        .await
        .map_err(|e| anyhow!("turns_pending_embedding: {}", e))?;
    if pending.is_empty() {
        return Ok(());
    }

    // 2. Resolve project short name from conversation record
    let project_name = state
        .store
        .get_conversation(session_id)
        .await
        .ok()
        .flatten()
        .and_then(|c| c.project)
        .and_then(|p| p.rsplit('/').next().map(|s| s.to_string()))
        .unwrap_or_default();

    let mut topic_vectors: Vec<(String, i32, Vec<f32>)> = Vec::new();
    let mut topic_updates: Vec<(i64, String)> = Vec::new();

    for turn in &pending {
        // 3. Skip noise turns (task notifications, empty preambles)
        if let Some(ref uc) = turn.user_content {
            if uc.starts_with("<task-notification>") {
                continue;
            }
        }

        // 4. Build embedding text (keyword-dense digest for Claude Code retrieval)
        let text = build_turn_embedding_text(turn, &project_name);
        if text.trim().len() < 20 {
            continue;
        }

        // 5. Generate embedding vector
        let svc = Arc::clone(embedding_service);
        let t = text.clone();
        match tokio::task::spawn_blocking(move || svc.embed(&t)).await {
            Ok(Some(vec)) => {
                // topic = user_content truncated to 200 chars (for display)
                let topic = turn
                    .user_content
                    .as_ref()
                    .map(|s| {
                        if s.chars().count() > 200 {
                            s.chars().take(200).collect::<String>()
                        } else {
                            s.clone()
                        }
                    })
                    .unwrap_or_default();
                topic_vectors.push((topic.clone(), turn.turn_idx, vec));
                topic_updates.push((turn.id, topic));
            }
            Ok(None) => {
                warn!(session = %session_id, turn_idx = turn.turn_idx, "Turn embedding returned None");
            }
            Err(e) => {
                warn!(session = %session_id, turn_idx = turn.turn_idx, error = %e, "Turn embedding panicked");
            }
        }
    }

    if topic_vectors.is_empty() {
        return Ok(());
    }

    // 6. Batch write to conversation_topic_vectors (ON CONFLICT DO UPDATE)
    let stored = state
        .store
        .set_conversation_turn_vectors(session_id, &topic_vectors, provider_id)
        .await
        .map_err(|e| anyhow!("set_conversation_turn_vectors: {}", e))?;

    // 7. Update conversation_turns.topic
    let refs: Vec<(i64, &str)> = topic_updates
        .iter()
        .map(|(id, t)| (*id, t.as_str()))
        .collect();
    let _ = state.store.update_turn_topics_batch(&refs).await;

    // 8. Update llm_summary (concatenate turn topics, hard cap 3000 chars)
    let summary = topic_vectors
        .iter()
        .map(|(topic, idx, _)| format!("T{}: {}", idx, topic))
        .collect::<Vec<_>>()
        .join("\n");
    let final_summary = if summary.len() > 3000 {
        summary[..char_boundary_at(&summary, 3000)].to_string()
    } else {
        summary
    };
    let _ = state
        .store
        .set_conversation_summary(session_id, &final_summary)
        .await;

    // 9. Also update single-vec for backwards compat (first turn as representative)
    let _ = state
        .store
        .set_conversation_embedding(session_id, &topic_vectors[0].2, provider_id)
        .await;

    info!(session = %session_id, count = stored, "Per-turn embeddings stored");
    Ok(())
}

/// Build keyword-dense embedding digest for a single Turn.
///
/// Optimized for Claude Code MCP retrieval: maximize keyword coverage
/// so a single vector search returns the most relevant past experience.
/// No natural language — pure structured keywords for embedding model consumption.
fn build_turn_embedding_text(
    turn: &missiond_core::types::ConversationTurn,
    project_name: &str,
) -> String {
    let mut text = String::with_capacity(2048);

    // 1. Project context
    if !project_name.is_empty() {
        text.push_str("project:");
        text.push_str(project_name);
        text.push('\n');
    }

    // 2. Tools used
    if let Some(ref tools) = turn.tool_names {
        if !tools.is_empty() {
            text.push_str("tools:");
            text.push_str(tools);
            text.push('\n');
        }
    }

    // 3. Files read
    if let Some(ref fr) = turn.files_read {
        if !fr.is_empty() {
            text.push_str("files_read:");
            text.push_str(fr);
            text.push('\n');
        }
    }

    // 4. Files changed
    if let Some(ref fc) = turn.files_changed {
        if !fc.is_empty() {
            text.push_str("files_changed:");
            text.push_str(fc);
            text.push('\n');
        }
    }

    // 5. User intent (core semantic — keywords from user message)
    if let Some(ref uc) = turn.user_content {
        if !uc.is_empty() {
            text.push_str("intent:");
            text.push_str(uc);
            text.push('\n');
        }
    }

    // 6. Outcome (last assistant text — keywords from conclusion)
    if let Some(ref oc) = turn.outcome {
        if !oc.is_empty() {
            text.push_str("outcome:");
            text.push_str(oc);
            text.push('\n');
        }
    }

    // Hard cap 3000 chars (embedding model token window)
    if text.len() > 3000 {
        text.truncate(char_boundary_at(&text, 3000));
    }

    text
}

/// Build the system prompt for topic extraction.
fn topic_extraction_prompt(max_topics: usize) -> String {
    format!(
        "你是一个技术文档分析专家。请从以下对话中提取所有独立讨论的核心主题。\n\n\
        规则：\n\
        1. 每个主题用一句话总结（30-80字），包含关键技术术语、工具名、文件路径\n\
        2. 最少1个，最多{}个主题\n\
        3. 主题之间应该是不同的技术方向，不要重复\n\
        4. 必须输出合法 JSON 数组，不要其他内容\n\
        5. 不要照搬原文，必须提炼归纳\n\n\
        示例输出：\n\
        [\"排查 Router 自动 Fallback 到 Gemini 的超时机制\", \"配置 RustDesk 自建 hbbs/hbbr 服务器与 Tailscale 组网\", \"修复 GA 构建流水线 Docker 镜像标签问题\"]",
        max_topics
    )
}

/// Known LLM pollution tags (NOT generic XML — avoids false positives on <vector>, <div> etc.)
static LLM_TAG_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"</?(?:invoke|think|thought|search|boltAction|minimax:tool_call|parameter)\b[^>]*>",
    )
    .unwrap()
});

// ─── Topic Value Object ───────────────────────────────────────────────
// "Parse, don't validate" — once constructed, a Topic is guaranteed valid.
// All validation rules (length, blacklist, sanitization) live here.

const TOPIC_MIN_CHARS: usize = 10;
const TOPIC_MAX_CHARS: usize = 200;
const TOPIC_TRUNCATE_CHARS: usize = 150;

/// A validated, sanitized topic string.
/// Invariants enforced at construction:
/// - 10–200 characters (char count, not bytes)
/// - No internal newlines, no LLM pollution tags
/// - Leading list markers stripped, truncated to 150 chars if needed
#[derive(Debug, Clone)]
struct Topic(String);

impl Topic {
    /// Sole construction path. Returns None if the raw string is garbage.
    fn try_new(raw: &str) -> Option<Self> {
        let t = raw.trim();
        let char_count = t.chars().count();

        // Hard reject: too short or too long (LLM hallucination)
        if char_count < TOPIC_MIN_CHARS || char_count > TOPIC_MAX_CHARS {
            return None;
        }

        // Content blacklist: tool output / thinking leaks
        if t.contains("[Tool:") || t.contains("[Tool：") {
            return None;
        }
        if t.starts_with("[A]") || t.starts_with("[R]") || t.starts_with("[T]") {
            return None;
        }
        if t.contains('\n') {
            return None;
        }
        if LLM_TAG_RE.is_match(t) {
            return None;
        }

        // Sanitize: strip leading list markers "1. ", "- ", "* "
        let cleaned = t
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == '*')
            .trim_start();

        // Truncate overlong (tolerance zone: 150–200 chars get trimmed, >200 already rejected)
        let final_str: String = if cleaned.chars().count() > TOPIC_TRUNCATE_CHARS {
            cleaned.chars().take(TOPIC_TRUNCATE_CHARS).collect()
        } else {
            cleaned.to_string()
        };

        // Re-check after stripping (markers might have been the bulk)
        if final_str.chars().count() < TOPIC_MIN_CHARS {
            return None;
        }

        Some(Self(final_str))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parse LLM response into validated Topic objects.
/// Strips code fences, parses JSON array, filters through Topic::try_new.
fn parse_and_validate_topics(raw: &str, max_topics: usize) -> Option<Vec<Topic>> {
    let json_str = raw.trim();
    let json_str = if json_str.starts_with("```") {
        json_str
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        json_str
    };

    let raw_topics: Vec<String> = match serde_json::from_str(json_str) {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, raw_preview = &json_str[..char_boundary_at(json_str, 200)], "Topic JSON parse failed");
            return None;
        }
    };

    let valid: Vec<Topic> = raw_topics
        .into_iter()
        .filter_map(|t| Topic::try_new(&t))
        .take(max_topics)
        .collect();

    if valid.is_empty() {
        None
    } else {
        Some(valid)
    }
}

/// Extract structured topics from conversation via Sonnet.
/// Fast fail — no fallback. Returns validated Topic objects or None.
async fn extract_conv_topics_llm(
    state: &AppState,
    conv_text: &str,
    has_timeline: bool,
) -> Option<Vec<Topic>> {
    let max_topics = if has_timeline { 8 } else { 5 };
    let system_prompt = topic_extraction_prompt(max_topics);

    // Direct HTTP to Router — no fallback, fast fail
    match crate::llm_gateway::call_sonnet_stateless(
        state,
        &system_prompt,
        conv_text,
        1024,
        "embedding_topics",
    )
    .await
    {
        Ok(content) => {
            if let Some(topics) = parse_and_validate_topics(&content, max_topics) {
                info!(count = topics.len(), "Topic extraction OK (Sonnet)");
                return Some(topics);
            }
            warn!(
                raw_preview = &content[..char_boundary_at(&content, 300)],
                "Sonnet returned content but topic validation failed"
            );
            None
        }
        Err(e) => {
            warn!(error = %e, "Sonnet topic extraction failed");
            None
        }
    }
}

/// Resolve LLM credentials (base_url, bearer_token) for Router API calls.
///
/// Resolution order:
/// 1. `$MISSIOND_HOME/llm.yaml` — explicit config (preferred)
/// 2. `~/.xjp/credentials.json` — legacy fallback (jwt_token + auth_url)
///
/// Returns (base_url, bearer_token).

pub(crate) async fn resolve_llm_credentials() -> Result<(String, String)> {
    // 1. Try llm.yaml in mission home
    let llm_yaml = default_mission_home().join("llm.yaml");
    if llm_yaml.exists() {
        let content = tokio::fs::read_to_string(&llm_yaml)
            .await
            .map_err(|e| anyhow!("Failed to read {}: {}", llm_yaml.display(), e))?;
        let config: LlmConfig = serde_yaml::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse {}: {}", llm_yaml.display(), e))?;

        let token = match &config.auth {
            LlmAuth::BearerEnv { env } => std::env::var(env)
                .map_err(|_| anyhow!("Env var '{}' not set (required by llm.yaml)", env))?,
            LlmAuth::BearerFile { path, key } => {
                let expanded = if path.starts_with("~/") {
                    dirs::home_dir()
                        .ok_or_else(|| anyhow!("Cannot determine home directory"))?
                        .join(&path[2..])
                } else {
                    PathBuf::from(path)
                };
                let file_content = tokio::fs::read_to_string(&expanded)
                    .await
                    .map_err(|e| anyhow!("Failed to read {}: {}", expanded.display(), e))?;
                let json: serde_json::Value = serde_json::from_str(&file_content)
                    .map_err(|e| anyhow!("Failed to parse {}: {}", expanded.display(), e))?;
                json.get(key)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Key '{}' not found in {}", key, expanded.display()))?
                    .to_string()
            }
            LlmAuth::None => String::new(),
        };

        info!(base_url = %config.base_url, "LLM credentials resolved from llm.yaml");
        return Ok((config.base_url, token));
    }

    // 2. Legacy fallback: ~/.xjp/credentials.json
    let cred_path = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot determine home directory"))?
        .join(".xjp")
        .join("credentials.json");

    if cred_path.exists() {
        let cred_content = tokio::fs::read_to_string(&cred_path)
            .await
            .map_err(|e| anyhow!("Failed to read credentials: {}", e))?;
        let creds: serde_json::Value = serde_json::from_str(&cred_content)
            .map_err(|e| anyhow!("Failed to parse credentials: {}", e))?;
        let jwt = creds
            .get("jwt_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!("No jwt_token in credentials.json. Configure LLM credentials first.")
            })?;
        let base_url = creds
            .get("auth_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if base_url.is_empty() {
            return Err(anyhow!("No auth_url in credentials.json. Configure LLM credentials in llm.yaml or credentials.json."));
        }

        info!("LLM credentials resolved from legacy credentials.json");
        return Ok((base_url.to_string(), jwt.to_string()));
    }

    Err(anyhow!(
        "No LLM credentials found. Create llm.yaml in mission home or ~/.xjp/credentials.json."
    ))
}

// ── Embedding Loop Worker (BackgroundWorker) ──────────────────────────────

pub(crate) struct EmbeddingLoopWorker {
    pub rx: tokio::sync::mpsc::Receiver<EmbeddingTask>,
}

impl super::BackgroundWorker for EmbeddingLoopWorker {
    const KIND: super::WorkerKind = super::WorkerKind::Sonnet;

    fn name(&self) -> &'static str {
        "embedding"
    }

    fn extra_deps(&self) -> Vec<crate::control_tree::Dependency> {
        use crate::control_tree::{CtlDomain, Dependency};
        vec![Dependency::Domain(CtlDomain::Memory)]
    }

    async fn run(self, state: Arc<AppState>, mut ctx: super::WorkerContext) {
        let mut rx = self.rx;
        info!("Embedding worker started (event-driven, ControlTree-aware)");
        loop {
            // Block if paused (cascade: global, Sonnet gate, Memory domain, or direct)
            ctx.wait_if_paused().await;

            // Race: new task vs control state change (prevents phantom blocking on recv)
            let task = tokio::select! {
                biased;
                _ = ctx.wait_until_paused() => continue,
                msg = rx.recv() => match msg {
                    Some(t) => t,
                    None => break,
                },
            };
            let provider_id = state
                .embedding_service
                .as_ref()
                .map(|svc| svc.provider_id().to_string())
                .unwrap_or_else(|| missiond_core::embedding::FASTEMBED_PROVIDER_ID.to_string());

            match task {
                EmbeddingTask::ProcessSession(session_id) => {
                    // Legacy: reroute to per-turn if turns exist, otherwise use old session-level flow
                    if let Ok(turns) = state
                        .store
                        .turns_pending_embedding(&session_id, &provider_id)
                        .await
                    {
                        if !turns.is_empty() {
                            if let Err(e) =
                                process_turns_for_session(&state, &session_id, &provider_id).await
                            {
                                warn!(session = %session_id, error = %e, "Per-turn embedding failed");
                            }
                        } else {
                            // No pending turns — use legacy session-level flow
                            if tokio::time::timeout(
                                std::time::Duration::from_secs(60),
                                generate_and_store_conv_embedding(&state, &session_id),
                            )
                            .await
                            .is_err()
                            {
                                warn!(session = %session_id, "Embedding generation timed out (60s)");
                            }
                        }
                    }
                }
                EmbeddingTask::ProcessTurns { session_id } => {
                    if let Err(e) =
                        process_turns_for_session(&state, &session_id, &provider_id).await
                    {
                        warn!(session = %session_id, error = %e, "Per-turn embedding failed");
                    }
                }
                EmbeddingTask::ProcessKBEntry(id) => {
                    if let Some(ref emb_svc) = state.embedding_service {
                        if let Ok(Some(entry)) = state.store.kb_get_by_id(&id).await {
                            let detail_text = entry
                                .detail
                                .as_ref()
                                .map(|d| serde_json::to_string(d).unwrap_or_default())
                                .unwrap_or_default();
                            let embed_text =
                                format!("知识条目：{}\n详情：{}", entry.summary, detail_text);
                            let svc = Arc::clone(emb_svc);
                            if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                            )
                            .await
                            {
                                let _ = state.store.kb_set_embedding(&id, &vec, &provider_id).await;
                                if entry.category.starts_with("policy:decision") {
                                    let mut guard = state.embedding_cache.write().await;
                                    guard.retain(|(eid, _)| eid != &id);
                                    guard.push((id.clone(), vec.clone()));
                                }
                                {
                                    let mut guard = state.kb_search_cache.write().await;
                                    guard.retain(|(eid, _)| eid != &id);
                                    guard.push((id.clone(), vec));
                                }
                                debug!(kb_id = %id, "KB entry embedding updated");
                            }
                        }
                    }
                }
                EmbeddingTask::ProcessSkillTopic(topic) => {
                    if let Some(ref emb_svc) = state.embedding_service {
                        if let Ok(missing) = state.store.skill_topics_missing_embedding(1).await {
                            let embed_text = if let Some((_, text)) =
                                missing.iter().find(|(t, _)| t == &topic)
                            {
                                text.clone()
                            } else {
                                format!("技能主题：{}", topic)
                            };
                            let svc = Arc::clone(emb_svc);
                            if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                            )
                            .await
                            {
                                let _ = state
                                    .store
                                    .skill_set_topic_embedding(&topic, &vec, &provider_id)
                                    .await;
                                let mut cache = state.skill_embedding_cache.write().await;
                                cache.retain(|(t, _)| t != &topic);
                                cache.push((topic.clone(), vec));
                                debug!(topic = %topic, "Skill topic embedding updated");
                            }
                        }
                    }
                }
                EmbeddingTask::ProcessAstBatch(node_ids) => {
                    if let Some(ref emb_svc) = state.embedding_service {
                        let mut embedded = 0usize;
                        for node_id in &node_ids {
                            if let Ok(Some(node_row)) = state.store.ast_get_node(node_id).await {
                                let embed_text = node_row.node.embedding_text(&node_row.file_path);
                                let svc = Arc::clone(emb_svc);
                                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                    std::time::Duration::from_secs(30),
                                    tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                                )
                                .await
                                {
                                    let bytes = missiond_core::embedding::f32_vec_to_bytes(&vec);
                                    let _ = state
                                        .store
                                        .ast_set_embedding(node_id, &bytes, &provider_id)
                                        .await;
                                    let mut guard = state.ast_embedding_cache.write().await;
                                    guard.retain(|(eid, _)| eid != node_id);
                                    guard.push((node_id.clone(), vec));
                                    embedded += 1;
                                }
                            }
                        }
                        if embedded > 0 {
                            debug!(
                                count = embedded,
                                total = node_ids.len(),
                                "AST batch embedding completed"
                            );
                        }
                    }
                }
                EmbeddingTask::ProcessMessage {
                    message_id,
                    session_id,
                    role,
                    content,
                } => match should_embed(&role, &content) {
                    EmbedDecision::Embed => {
                        if let Some(ref emb_svc) = state.embedding_service {
                            let svc = Arc::clone(emb_svc);
                            let text = content.clone();
                            if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                tokio::task::spawn_blocking(move || svc.embed(&text)),
                            )
                            .await
                            {
                                let _ = state
                                    .store
                                    .insert_message_embedding(
                                        message_id,
                                        &session_id,
                                        &vec,
                                        &provider_id,
                                    )
                                    .await;
                                debug!(msg_id = message_id, "Message embedding stored");
                            }
                        }
                    }
                    EmbedDecision::Skip(reason) => {
                        let _ = state
                            .store
                            .insert_message_embedding_skip(message_id, reason)
                            .await;
                    }
                },
                EmbeddingTask::BackfillAll => {
                    backfill_start(&state, &provider_id).await;
                }
                EmbeddingTask::RunBackfillPhase { phase, cursor } => {
                    backfill_run_phase(&state, &provider_id, phase, cursor).await;
                }
            }
        }
        warn!("Embedding worker channel closed");
    }
}

/// Backfill entry point: determine resume phase from DB, then kick off first batch.
/// Gated by `llm.yaml` → `backfill_enabled: true`. Skips silently when disabled.
async fn backfill_start(state: &AppState, provider_id: &str) {
    if !state
        .backfill_enabled
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        debug!("Backfill: disabled by config (backfill_enabled: false)");
        return;
    }
    info!("Backfill: checking resume state");

    let resume_phase = 'find: {
        for phase in BackfillPhase::all() {
            match state.store.backfill_get_phase(phase.as_str()).await {
                Ok(Some(status)) if status.status == "completed" => continue,
                Ok(Some(status)) => {
                    info!(
                        phase = phase.as_str(),
                        cursor = status.last_cursor,
                        processed = status.processed,
                        "Backfill: resuming"
                    );
                    break 'find *phase;
                }
                _ => break 'find *phase,
            }
        }
        BackfillPhase::first()
    };

    info!(
        phase = resume_phase.as_str(),
        "Backfill: starting from phase"
    );
    let total = match resume_phase {
        BackfillPhase::ConvSummary => state
            .store
            .conversations_missing_summary_count()
            .await
            .unwrap_or(0),
        BackfillPhase::ConvTopicVectors => state
            .store
            .conversations_needing_topic_vectors_count(provider_id)
            .await
            .unwrap_or(0),
        _ => 0,
    };
    let _ = state
        .store
        .backfill_start_phase(resume_phase.as_str(), total)
        .await;
    let cursor = state
        .store
        .backfill_get_phase(resume_phase.as_str())
        .await
        .ok()
        .flatten()
        .map(|s| s.last_cursor)
        .unwrap_or(0);
    backfill_enqueue(
        state,
        EmbeddingTask::RunBackfillPhase {
            phase: resume_phase,
            cursor,
        },
    )
    .await;
}

/// Enqueue a backfill task with retry on channel full (prevents silent task loss).
async fn backfill_enqueue(state: &AppState, task: EmbeddingTask) {
    for attempt in 0..5 {
        match state.embedding_tx.try_send(task.clone()) {
            Ok(()) => return,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                warn!(attempt, "Backfill enqueue: channel full, backing off");
                tokio::time::sleep(std::time::Duration::from_millis(500 * (1 << attempt))).await;
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                warn!("Backfill enqueue: channel closed, aborting");
                return;
            }
        }
    }
    warn!("Backfill enqueue: channel still full after 5 retries, task will resume on next restart");
}

/// Run one batch of a backfill phase, then re-enqueue next batch (yield pattern).
async fn backfill_run_phase(
    state: &AppState,
    provider_id: &str,
    phase: BackfillPhase,
    cursor: i64,
) {
    const BATCH_SIZE: i64 = 20;
    const MAX_RETRIES: i64 = 3;

    let (batch_success, batch_failed, new_cursor, phase_done) = match phase {
        BackfillPhase::KbStale => backfill_kb_stale(state, provider_id).await,
        BackfillPhase::KbMissing => {
            let r = backfill_kb_missing(state, provider_id).await;
            if r.3 {
                warm_kb_caches(state).await;
            }
            r
        }
        BackfillPhase::SkillStale => backfill_skill_stale(state, provider_id).await,
        BackfillPhase::SkillMissing => {
            let r = backfill_skill_missing(state, provider_id).await;
            if r.3 {
                warm_skill_cache(state).await;
            }
            r
        }
        BackfillPhase::ConvTopicVectors => {
            backfill_conv_topic_vectors(state, provider_id, cursor, BATCH_SIZE).await
        }
        BackfillPhase::ConvSummary => backfill_conv_summary(state, cursor, BATCH_SIZE).await,
        BackfillPhase::ConvRetry => backfill_conv_retry(state, MAX_RETRIES, BATCH_SIZE).await,
        BackfillPhase::AstNodes => {
            let r = backfill_ast_nodes(state, provider_id).await;
            if r.3 {
                warm_ast_cache(state).await;
            }
            r
        }
        BackfillPhase::Timeline => backfill_timelines(state).await,
        BackfillPhase::MessageEmbeddings => {
            backfill_message_embeddings(state, provider_id, cursor, 200).await
        } // v2: larger batch, internally capped by 2MB
    };

    if batch_success > 0 || batch_failed > 0 {
        let _ = state
            .store
            .backfill_update_progress(phase.as_str(), new_cursor, batch_success, batch_failed)
            .await;
    }

    if phase_done {
        let _ = state.store.backfill_complete_phase(phase.as_str()).await;
        info!(phase = phase.as_str(), "Backfill phase completed");
        if let Some(next) = phase.next() {
            let total = match next {
                BackfillPhase::ConvSummary => state
                    .store
                    .conversations_missing_summary_count()
                    .await
                    .unwrap_or(0),
                BackfillPhase::ConvTopicVectors => state
                    .store
                    .conversations_needing_topic_vectors_count(provider_id)
                    .await
                    .unwrap_or(0),
                _ => 0,
            };
            let _ = state.store.backfill_start_phase(next.as_str(), total).await;
            backfill_enqueue(
                state,
                EmbeddingTask::RunBackfillPhase {
                    phase: next,
                    cursor: 0,
                },
            )
            .await;
        } else {
            info!("Full embedding backfill complete");
        }
    } else {
        // Backoff on batch-level failure (e.g. Ollama down): avoid tight retry loop
        if batch_success == 0 && batch_failed > 0 {
            warn!(
                phase = phase.as_str(),
                failed = batch_failed,
                "Batch-level failure, backing off 5s"
            );
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        backfill_enqueue(
            state,
            EmbeddingTask::RunBackfillPhase {
                phase,
                cursor: new_cursor,
            },
        )
        .await;
    }
}

// ── Phase implementations (each returns: success, failed, new_cursor, phase_done) ──

async fn backfill_kb_stale(state: &AppState, provider_id: &str) -> (i64, i64, i64, bool) {
    let Some(ref emb_svc) = state.embedding_service else {
        return (0, 0, 0, true);
    };
    let stale = state
        .store
        .kb_entries_stale_embedding(provider_id, 20)
        .await
        .unwrap_or_default();
    if stale.is_empty() {
        return (0, 0, 0, true);
    }
    info!(count = stale.len(), "KB stale re-embedding");
    let mut success = 0i64;
    for (id, summary, detail) in &stale {
        let embed_text = format!("知识条目：{}\n详情：{}", summary, detail);
        let svc = Arc::clone(emb_svc);
        if let Ok(Ok(Some(vec))) = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
        )
        .await
        {
            let _ = state.store.kb_set_embedding(id, &vec, provider_id).await;
            success += 1;
        }
    }
    (success, 0, 0, stale.len() < 20)
}

async fn backfill_kb_missing(state: &AppState, provider_id: &str) -> (i64, i64, i64, bool) {
    let Some(ref emb_svc) = state.embedding_service else {
        return (0, 0, 0, true);
    };
    let missing = state
        .store
        .kb_entries_missing_embedding(None)
        .await
        .unwrap_or_default();
    if missing.is_empty() {
        return (0, 0, 0, true);
    }
    info!(count = missing.len(), "KB missing embedding backfill");
    let mut success = 0i64;
    for (id, summary, detail) in &missing {
        let embed_text = format!("知识条目：{}\n详情：{}", summary, detail);
        let svc = Arc::clone(emb_svc);
        if let Ok(Ok(Some(vec))) = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
        )
        .await
        {
            let _ = state.store.kb_set_embedding(id, &vec, provider_id).await;
            success += 1;
        }
    }
    (success, 0, 0, missing.len() < 20)
}

async fn backfill_skill_stale(state: &AppState, provider_id: &str) -> (i64, i64, i64, bool) {
    let Some(ref emb_svc) = state.embedding_service else {
        return (0, 0, 0, true);
    };
    let stale = state
        .store
        .skill_topics_stale_embedding(provider_id, 20)
        .await
        .unwrap_or_default();
    if stale.is_empty() {
        return (0, 0, 0, true);
    }
    info!(count = stale.len(), "Skill stale re-embedding");
    let mut success = 0i64;
    for (topic, embed_text) in &stale {
        let svc = Arc::clone(emb_svc);
        let text = embed_text.clone();
        if let Ok(Ok(Some(vec))) = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::task::spawn_blocking(move || svc.embed(&text)),
        )
        .await
        {
            let _ = state
                .store
                .skill_set_topic_embedding(topic, &vec, provider_id)
                .await;
            success += 1;
        }
    }
    (success, 0, 0, stale.len() < 20)
}

async fn backfill_skill_missing(state: &AppState, provider_id: &str) -> (i64, i64, i64, bool) {
    let Some(ref emb_svc) = state.embedding_service else {
        return (0, 0, 0, true);
    };
    let missing = state
        .store
        .skill_topics_missing_embedding(20)
        .await
        .unwrap_or_default();
    if missing.is_empty() {
        return (0, 0, 0, true);
    }
    info!(count = missing.len(), "Skill missing embedding backfill");
    let mut success = 0i64;
    for (topic, embed_text) in &missing {
        let svc = Arc::clone(emb_svc);
        let text = embed_text.clone();
        if let Ok(Ok(Some(vec))) = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::task::spawn_blocking(move || svc.embed(&text)),
        )
        .await
        {
            let _ = state
                .store
                .skill_set_topic_embedding(topic, &vec, provider_id)
                .await;
            success += 1;
        }
    }
    (success, 0, 0, missing.len() < 20)
}

async fn backfill_conv_topic_vectors(
    state: &AppState,
    provider_id: &str,
    cursor: i64,
    batch_size: i64,
) -> (i64, i64, i64, bool) {
    let batch = state
        .store
        .conversations_needing_topic_vectors_cursor(provider_id, cursor, batch_size)
        .await
        .unwrap_or_default();
    if batch.is_empty() {
        return (0, 0, cursor, true);
    }
    info!(
        count = batch.len(),
        cursor, "Conv topic vector backfill batch"
    );
    let mut success = 0i64;
    let mut failed = 0i64;
    let mut max_rowid = cursor;
    for (rowid, session_id) in &batch {
        max_rowid = max_rowid.max(*rowid);
        match tokio::time::timeout(
            std::time::Duration::from_secs(90),
            generate_and_store_conv_embedding(state, session_id),
        )
        .await
        {
            Ok(()) => {
                success += 1;
                let _ = state
                    .store
                    .backfill_clear_failure(session_id, "conv_topic_vectors")
                    .await;
            }
            Err(_) => {
                warn!(session = %session_id, "Topic vector backfill timed out");
                let _ = state
                    .store
                    .backfill_record_failure(session_id, "conv_topic_vectors", "timeout_90s")
                    .await;
                failed += 1;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    (
        success,
        failed,
        max_rowid,
        batch.len() < batch_size as usize,
    )
}

async fn backfill_conv_summary(
    state: &AppState,
    cursor: i64,
    batch_size: i64,
) -> (i64, i64, i64, bool) {
    let batch = state
        .store
        .conversations_missing_summary_cursor(cursor, batch_size)
        .await
        .unwrap_or_default();
    if batch.is_empty() {
        return (0, 0, cursor, true);
    }
    info!(count = batch.len(), cursor, "Conv summary backfill batch");
    let mut success = 0i64;
    let mut failed = 0i64;
    let mut max_rowid = cursor;
    for (idx, (rowid, session_id)) in batch.iter().enumerate() {
        max_rowid = max_rowid.max(*rowid);
        info!(session = %session_id, idx = idx + 1, total = batch.len(), "Processing session");
        match tokio::time::timeout(
            std::time::Duration::from_secs(90),
            generate_and_store_conv_embedding(state, session_id),
        )
        .await
        {
            Ok(()) => {
                info!(session = %session_id, idx = idx + 1, "Session done");
                success += 1;
                let _ = state
                    .store
                    .backfill_clear_failure(session_id, "conv_summary")
                    .await;
            }
            Err(_) => {
                warn!(session = %session_id, idx = idx + 1, "Conv embedding timed out (90s)");
                let _ = state
                    .store
                    .backfill_record_failure(session_id, "conv_summary", "timeout_90s")
                    .await;
                failed += 1;
            }
        }
    }
    (
        success,
        failed,
        max_rowid,
        batch.len() < batch_size as usize,
    )
}

async fn backfill_conv_retry(
    state: &AppState,
    max_retries: i64,
    batch_size: i64,
) -> (i64, i64, i64, bool) {
    let mut total_success = 0i64;
    let mut total_failed = 0i64;
    let mut has_work = false;
    let mut has_cooling = false;
    for phase_name in &["conv_summary", "conv_topic_vectors"] {
        let retryable = state
            .store
            .backfill_retryable_failures(phase_name, max_retries, batch_size)
            .await
            .unwrap_or_default();
        if retryable.is_empty() {
            // Check if failures exist but are in cooldown (updated_at within last 5 min)
            let all_remaining = state
                .store
                .backfill_retryable_failures_no_cooldown(phase_name, max_retries)
                .await
                .unwrap_or(0);
            if all_remaining > 0 {
                has_cooling = true;
            }
            continue;
        }
        has_work = true;
        info!(
            count = retryable.len(),
            phase = *phase_name,
            "Retrying failed sessions"
        );
        for session_id in &retryable {
            match tokio::time::timeout(
                std::time::Duration::from_secs(120),
                generate_and_store_conv_embedding(state, session_id),
            )
            .await
            {
                Ok(()) => {
                    info!(session = %session_id, "Retry succeeded");
                    let _ = state
                        .store
                        .backfill_clear_failure(session_id, phase_name)
                        .await;
                    total_success += 1;
                }
                Err(_) => {
                    warn!(session = %session_id, "Retry timed out");
                    let _ = state
                        .store
                        .backfill_record_failure(session_id, phase_name, "retry_timeout_120s")
                        .await;
                    total_failed += 1;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    // phase_done only if no work AND nothing cooling down
    let phase_done = !has_work && !has_cooling;
    (total_success, total_failed, 0, phase_done)
}

async fn backfill_ast_nodes(state: &AppState, provider_id: &str) -> (i64, i64, i64, bool) {
    let Some(ref emb_svc) = state.embedding_service else {
        return (0, 0, 0, true);
    };
    let missing = state
        .store
        .ast_find_unembedded(20)
        .await
        .unwrap_or_default();
    if missing.is_empty() {
        return (0, 0, 0, true);
    }
    info!(count = missing.len(), "AST embedding backfill batch");
    let mut success = 0i64;
    for (node_id, _repo, file_path) in &missing {
        if let Ok(Some(node_row)) = state.store.ast_get_node(node_id).await {
            let embed_text = node_row.node.embedding_text(file_path);
            let svc = Arc::clone(emb_svc);
            if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
            )
            .await
            {
                let bytes = missiond_core::embedding::f32_vec_to_bytes(&vec);
                let _ = state
                    .store
                    .ast_set_embedding(node_id, &bytes, provider_id)
                    .await;
                success += 1;
            }
        }
    }
    (success, 0, 0, missing.len() < 20)
}

async fn backfill_timelines(state: &AppState) -> (i64, i64, i64, bool) {
    let needing = state
        .store
        .conversations_needing_timeline(50)
        .await
        .unwrap_or_default();
    if needing.is_empty() {
        return (0, 0, 0, true);
    }
    info!(
        count = needing.len(),
        "Building session timelines for compaction parents"
    );
    let mut success = 0i64;
    for parent_id in &needing {
        let fragments = match state.store.get_compaction_fragments(parent_id).await {
            Ok(f) => f,
            Err(e) => {
                warn!(parent = %parent_id, error = %e, "Failed to get compaction fragments");
                continue;
            }
        };
        if fragments.is_empty() {
            continue;
        }
        let mut timeline_entries = Vec::new();
        for (idx, (frag_id, started_at, msg_count)) in fragments.iter().enumerate() {
            let summary = state
                .store
                .get_last_assistant_content(frag_id)
                .await
                .unwrap_or(None);
            let summary_tokens = summary.as_ref().map(|s| s.len() / 4).unwrap_or(0);
            timeline_entries.push(serde_json::json!({ "fragment_id": frag_id, "shard_index": idx, "started_at": started_at, "message_count": msg_count, "summary_tokens": summary_tokens, "summary": summary, "segment_embedding_id": null }));
        }
        let timeline_json =
            serde_json::to_string(&timeline_entries).unwrap_or_else(|_| "[]".to_string());
        match state
            .store
            .set_session_timeline(parent_id, &timeline_json)
            .await
        {
            Ok(true) => {
                let _ = state.store.clear_conversation_summary(parent_id).await;
                info!(parent = %parent_id, fragments = fragments.len(), "Session timeline built");
                success += 1;
            }
            Ok(false) => {}
            Err(e) => {
                warn!(parent = %parent_id, error = %e, "Failed to set session timeline");
            }
        }
    }
    (success, 0, 0, needing.len() < 50)
}

async fn warm_kb_caches(state: &AppState) {
    if let Ok(all) = state.store.kb_load_embeddings("policy:decision").await {
        let mut guard = state.embedding_cache.write().await;
        *guard = all;
        info!(
            count = guard.len(),
            "KB policy cache refreshed after backfill"
        );
    }
    if let Ok(all) = state.store.kb_load_all_embeddings().await {
        let mut guard = state.kb_search_cache.write().await;
        *guard = all;
        info!(
            count = guard.len(),
            "KB search cache refreshed after backfill"
        );
    }
}

async fn warm_skill_cache(state: &AppState) {
    if let Ok(all) = state.store.skill_load_topic_embeddings().await {
        let mut guard = state.skill_embedding_cache.write().await;
        *guard = all;
        info!(
            count = guard.len(),
            "Skill embedding cache refreshed after backfill"
        );
    }
}

async fn warm_ast_cache(state: &AppState) {
    if let Ok(all) = state.store.ast_load_all_embeddings().await {
        let mut guard = state.ast_embedding_cache.write().await;
        *guard = all;
        info!(
            count = guard.len(),
            "AST embedding cache refreshed after backfill"
        );
    }
}

/// Backfill message-level embeddings (v2): batch embed + batch insert + failure tracking.
///
/// Architecture (docs/designs/message-embedding-arch-v2.md):
/// 1. Cursor scan (no LEFT JOIN) — O(batch_size) not O(N)
/// 2. should_embed() split → skips[] + embeddables[]
/// 3. embed_batch() — single API/local call for entire batch
/// 4. Batch INSERT via QueryBuilder
/// 5. Failures → backfill_failures table (not lost by cursor advance)
/// 6. Dynamic batch size capped by 2MB total content
async fn backfill_message_embeddings(
    state: &AppState,
    provider_id: &str,
    cursor: i64,
    batch_size: i64,
) -> (i64, i64, i64, bool) {
    let Some(ref emb_svc) = state.embedding_service else {
        return (0, 0, cursor, true);
    };
    let batch = state
        .store
        .messages_pending_embedding(cursor, batch_size)
        .await
        .unwrap_or_default();
    if batch.is_empty() {
        return (0, 0, cursor, true);
    }
    info!(
        count = batch.len(),
        cursor, "Message embedding backfill batch (v2)"
    );

    let mut max_id = cursor;

    // Phase 1: Split into skips vs embeddables (with 2MB content cap)
    let mut skips: Vec<(i64, &str)> = Vec::new();
    let mut embed_ids: Vec<i64> = Vec::new();
    let mut embed_sessions: Vec<&str> = Vec::new();
    let mut embed_texts: Vec<String> = Vec::new();
    let mut batch_bytes: usize = 0;
    const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024; // 2MB cap per Gemini ARB recommendation

    for (msg_id, session_id, role, content) in &batch {
        match should_embed(role, content) {
            EmbedDecision::Skip(reason) => {
                skips.push((*msg_id, reason));
                max_id = max_id.max(*msg_id); // Only advance after confirmed processing
            }
            EmbedDecision::Embed => {
                // Dynamic batch size: cap by total content bytes
                if batch_bytes + content.len() > MAX_BATCH_BYTES && !embed_texts.is_empty() {
                    // Don't add more — this message will be picked up next round
                    // max_id NOT updated: cursor stays before this unprocessed message
                    break;
                }
                batch_bytes += content.len();
                embed_ids.push(*msg_id);
                embed_sessions.push(session_id);
                embed_texts.push(content.clone());
                max_id = max_id.max(*msg_id); // Only advance after confirmed processing
            }
        }
    }

    // Phase 2: Batch insert skips
    let skip_count = skips.len() as i64;
    if !skips.is_empty() {
        let _ = state
            .store
            .insert_message_embedding_skips_batch(&skips)
            .await;
    }

    // Phase 3: Batch embed
    let mut embed_success = 0i64;
    let mut embed_failed = 0i64;
    if !embed_texts.is_empty() {
        let svc = Arc::clone(emb_svc);
        let texts = embed_texts.clone();
        let embed_result = tokio::time::timeout(
            std::time::Duration::from_secs(60), // Longer timeout for batch (ARB rec)
            tokio::task::spawn_blocking(move || svc.embed_batch(&texts)),
        )
        .await;

        match embed_result {
            Ok(Ok(vectors)) => {
                // Phase 4: Split results into successes and failures
                let mut insert_entries: Vec<(i64, &str, Vec<f32>, &str)> = Vec::new();
                for (i, maybe_vec) in vectors.into_iter().enumerate() {
                    match maybe_vec {
                        Some(vec) => {
                            insert_entries.push((
                                embed_ids[i],
                                embed_sessions[i],
                                vec,
                                provider_id,
                            ));
                            embed_success += 1;
                        }
                        None => {
                            // Individual failure → record for retry
                            let _ = state
                                .store
                                .backfill_record_failure(
                                    &embed_ids[i].to_string(),
                                    "message_embeddings",
                                    "embed_returned_none",
                                )
                                .await;
                            embed_failed += 1;
                        }
                    }
                }
                // Phase 5: Batch insert embeddings
                if !insert_entries.is_empty() {
                    let _ = state
                        .store
                        .insert_message_embeddings_batch(&insert_entries)
                        .await;
                }
            }
            Ok(Err(e)) => {
                // spawn_blocking panicked — record all as failures, RESET cursor
                warn!(error = %e, "embed_batch spawn_blocking panicked");
                for id in &embed_ids {
                    let _ = state
                        .store
                        .backfill_record_failure(
                            &id.to_string(),
                            "message_embeddings",
                            "spawn_panic",
                        )
                        .await;
                }
                embed_failed = embed_ids.len() as i64;
                // Reset max_id: don't advance cursor past batch-level failures
                max_id = cursor;
            }
            Err(_) => {
                // Timeout — batch-level failure, RESET cursor for retry
                warn!("embed_batch timed out (60s), recording failures");
                for id in &embed_ids {
                    let _ = state
                        .store
                        .backfill_record_failure(
                            &id.to_string(),
                            "message_embeddings",
                            "batch_timeout_60s",
                        )
                        .await;
                }
                embed_failed = embed_ids.len() as i64;
                // Reset max_id: don't advance cursor past batch-level failures
                max_id = cursor;
            }
        }
    }

    let total_success = skip_count + embed_success;
    let phase_done = batch.len() < batch_size as usize;
    (total_success, embed_failed, max_id, phase_done)
}
