
use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::state::{AppState, EmbeddingTask};
use std::sync::Arc;
use std::path::PathBuf;
use crate::helpers::char_boundary_at;
use crate::helpers::default_mission_home;
use crate::minimax_client::ChatMessage;

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
    fn default_binary() -> String { "gemini".to_string() }
    fn default_model() -> String { "gemini-3.1-pro-preview".to_string() }
    fn default_timeout() -> u64 { 120 }
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
    fn default_provider() -> String { "xjp-router".to_string() }
    fn default_model() -> String { "gpt-4o".to_string() }
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
        let base_url = std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("MISSIOND_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "qwen3-embedding".to_string());

        // Health check: verify Ollama is reachable
        let health = client.get(format!("{}/api/tags", base_url))
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
        let resp = client.post(format!("{}/api/embed", base_url))
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
            .map(|arr| arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
    }

    /// Batch embedding via Ollama /api/embed (supports array input).
    async fn embed_many(
        client: &reqwest::Client,
        base_url: &str,
        model: &str,
        texts: &[String],
    ) -> Vec<Option<Vec<f32>>> {
        let resp = client.post(format!("{}/api/embed", base_url))
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
                        return embeddings.iter().map(|emb| {
                            emb.as_array().map(|arr| {
                                arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect()
                            })
                        }).collect();
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
            tokio::runtime::Handle::current().block_on(
                Self::embed_one(&client, &base_url, &model, &text)
            )
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
            tokio::runtime::Handle::current().block_on(
                Self::embed_many(&client, &base_url, &model, &texts)
            )
        })
    }
}

/// Initialize embedding provider: try Ollama first (better quality), fall back to FastEmbed.

pub(crate) async fn init_embedding_provider(
    http_client: &reqwest::Client,
) -> Option<Arc<dyn missiond_core::embedding::EmbeddingProvider>> {
    // Try Ollama first
    if let Some(ollama) = OllamaProvider::try_new(http_client).await {
        return Some(Arc::new(ollama));
    }
    info!("Ollama not available, falling back to FastEmbed");

    // Fallback to FastEmbed
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
    let db = state.mission.db();

    // 1. Fetch messages
    debug!(session = %session_id, "Topic pipeline: fetching messages");
    let messages = match db.get_conversation_messages(session_id, None, 200) {
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
    let has_timeline = if let Ok(Some(conv)) = db.get_conversation(session_id) {
        if let Some(ref tl_json) = conv.session_timeline {
            if let Ok(timeline) = serde_json::from_str::<Vec<serde_json::Value>>(tl_json) {
                if !timeline.is_empty() {
                    conv_text.push_str("=== 会话历史阶段摘要 ===\n");
                    let fragments: Vec<&serde_json::Value> = if timeline.len() > 10 {
                        let mut selected: Vec<&serde_json::Value> = timeline.iter().take(3).collect();
                        let mid = timeline.len() / 2;
                        if mid > 3 && mid < timeline.len() - 3 {
                            selected.push(&timeline[mid - 1]);
                            selected.push(&timeline[mid]);
                        }
                        selected.extend(timeline.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev());
                        selected
                    } else {
                        timeline.iter().collect()
                    };
                    for entry in &fragments {
                        let idx = entry.get("shard_index").and_then(|v| v.as_u64()).unwrap_or(0);
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
                } else { false }
            } else { false }
        } else { false }
    } else { false };

    // Fetch briefing summaries: semantic summaries are much more information-dense
    // than raw content, improving topic extraction quality while reducing token usage.
    let briefing_map = db.get_briefing_summaries_for_session(session_id)
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

    // 3. Extract structured topics via LLM
    debug!(session = %session_id, text_len = conv_text.len(), "Topic pipeline: extracting topics");
    let topics = match extract_conv_topics_llm(state, &conv_text, has_timeline).await {
        Some(t) if !t.is_empty() => {
            debug!(session = %session_id, count = t.len(), "Topics extracted: {:?}", &t);
            t
        }
        _ => {
            debug!(session = %session_id, "Topic extraction failed, using single-summary fallback");
            // Fallback: try old-style summary as single topic
            let fallback = generate_conv_summary_llm_legacy(state, &conv_text, has_timeline).await
                .unwrap_or_else(|| {
                    let first3: Vec<&str> = messages.iter().take(3).map(|m| m.content.as_str()).collect();
                    let last3: Vec<&str> = messages.iter().rev().take(3).rev().map(|m| m.content.as_str()).collect();
                    let mut fb = first3.join("\n");
                    fb.push_str("\n...\n");
                    fb.push_str(&last3.join("\n"));
                    if fb.len() > 2000 {
                        fb.truncate(char_boundary_at(&fb, 2000));
                    }
                    fb
                });
            vec![fallback]
        }
    };

    // 4. Store combined summary for display (llm_summary column)
    let combined_summary = if topics.len() == 1 {
        topics[0].clone()
    } else {
        topics.iter()
            .enumerate()
            .map(|(i, t)| format!("{}. {}", i + 1, t))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if let Err(e) = db.set_conversation_summary(session_id, &combined_summary) {
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
        let text = topic.clone();
        match tokio::task::spawn_blocking(move || svc.embed(&text)).await {
            Ok(Some(vec)) => {
                topic_vectors.push((topic.clone(), vec));
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
    if let Err(e) = db.set_conversation_topic_vectors(session_id, &topic_vectors, &provider_id) {
        warn!(session = %session_id, error = %e, "Failed to store topic vectors");
        return;
    }
    // Also update old single-vec for backwards compat (first topic as representative)
    let _ = db.set_conversation_embedding(session_id, &topic_vectors[0].1, &provider_id);

    // 7. Update TopicCache
    let vecs_only: Vec<Vec<f32>> = topic_vectors.into_iter().map(|(_, v)| v).collect();
    let mut cache = state.conversation_topic_cache.write().await;
    cache.retain(|(id, _)| id != session_id);
    cache.push((session_id.to_string(), vecs_only));

    info!(session = %session_id, topics = topics.len(), "Multi-topic embeddings generated");
}

/// Extract structured topics from conversation via MiniMax M2.5 (through Gateway).
/// Returns a Vec of topic summary strings (each 30-80 chars, optimized for embedding).
async fn extract_conv_topics_llm(
    state: &AppState,
    conv_text: &str,
    has_timeline: bool,
) -> Option<Vec<String>> {
    let handle = state.minimax.as_ref()?;

    let max_topics = if has_timeline { 8 } else { 5 };

    let system_prompt = format!(
        "你是一个技术文档分析专家。请从以下对话中提取所有独立讨论的核心主题。\n\n\
        规则：\n\
        1. 每个主题用一句话总结（30-80字），包含关键技术术语、工具名、文件路径\n\
        2. 最少1个，最多{}个主题\n\
        3. 主题之间应该是不同的技术方向，不要重复\n\
        4. 必须输出合法 JSON 数组，不要其他内容\n\n\
        示例输出：\n\
        [\"排查 Router 自动 Fallback 到 Gemini 的超时机制\", \"配置 RustDesk 自建 hbbs/hbbr 服务器与 Tailscale 组网\", \"修复 GA 构建流水线 Docker 镜像标签问题\"]",
        max_topics
    );

    let messages = vec![
        ChatMessage { role: "system".into(), content: system_prompt },
        ChatMessage { role: "user".into(), content: conv_text.to_string() },
    ];

    let content = match handle.call_embedding(messages, Some(1024), None).await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "MiniMax topic extraction failed");
            return None;
        }
    };

    // Parse JSON array from response (handle markdown code fences)
    let json_str = content.trim();
    let json_str = if json_str.starts_with("```") {
        json_str
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        json_str
    };

    match serde_json::from_str::<Vec<String>>(json_str) {
        Ok(topics) if !topics.is_empty() => {
            let topics: Vec<String> = topics.into_iter()
                .filter(|t| !t.trim().is_empty())
                .take(max_topics)
                .collect();
            debug!(count = topics.len(), "Topic extraction OK (MiniMax Gateway)");
            Some(topics)
        }
        Ok(_) => {
            debug!("Topic extraction returned empty array");
            None
        }
        Err(e) => {
            debug!(error = %e, raw = %json_str, "Topic extraction JSON parse failed");
            None
        }
    }
}

/// Legacy single-summary LLM call (fallback when topic extraction fails).
async fn generate_conv_summary_llm_legacy(
    state: &AppState,
    conv_text: &str,
    has_timeline: bool,
) -> Option<String> {
    let handle = state.minimax.as_ref()?;

    let system_prompt = if has_timeline {
        "作为技术专家，请用 300 字以内总结以下长会话的完整生命周期。\n\
        会话包含多个阶段摘要(compaction fragments)和最近消息。请覆盖所有阶段，必须包含：\n\
        1. 会话的整体目标和演进过程\n\
        2. 各阶段的关键成果或决策\n\
        3. 涉及的核心技术栈和文件\n\
        4. 最终结论或未完成事项\n\
        输出纯文本，不要 Markdown 格式。"
    } else {
        "作为技术专家，请用 200 字以内总结以下排查/开发会话，必须包含：\n\
        1. 遇到的核心问题或 Bug 表现\n\
        2. 涉及的代码方法名、文件路径或技术栈\n\
        3. 最终的解决思路或结论\n\
        输出纯文本，不要 Markdown 格式。"
    };

    let messages = vec![
        ChatMessage { role: "system".into(), content: system_prompt.to_string() },
        ChatMessage { role: "user".into(), content: conv_text.to_string() },
    ];

    match handle.call_embedding(messages, Some(512), None).await {
        Ok(content) => {
            let trimmed = content.trim().to_string();
            debug!(len = trimmed.len(), "Legacy LLM summary generated (MiniMax)");
            Some(trimmed)
        }
        Err(e) => {
            warn!(error = %e, "MiniMax legacy summary failed");
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
        let content = tokio::fs::read_to_string(&llm_yaml).await
            .map_err(|e| anyhow!("Failed to read {}: {}", llm_yaml.display(), e))?;
        let config: LlmConfig = serde_yaml::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse {}: {}", llm_yaml.display(), e))?;

        let token = match &config.auth {
            LlmAuth::BearerEnv { env } => {
                std::env::var(env)
                    .map_err(|_| anyhow!("Env var '{}' not set (required by llm.yaml)", env))?
            }
            LlmAuth::BearerFile { path, key } => {
                let expanded = if path.starts_with("~/") {
                    dirs::home_dir()
                        .ok_or_else(|| anyhow!("Cannot determine home directory"))?
                        .join(&path[2..])
                } else {
                    PathBuf::from(path)
                };
                let file_content = tokio::fs::read_to_string(&expanded).await
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
        let cred_content = tokio::fs::read_to_string(&cred_path).await
            .map_err(|e| anyhow!("Failed to read credentials: {}", e))?;
        let creds: serde_json::Value = serde_json::from_str(&cred_content)
            .map_err(|e| anyhow!("Failed to parse credentials: {}", e))?;
        let jwt = creds.get("jwt_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("No jwt_token in credentials.json. Configure LLM credentials first."))?;
        let base_url = creds.get("auth_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if base_url.is_empty() {
            return Err(anyhow!("No auth_url in credentials.json. Configure LLM credentials in llm.yaml or credentials.json."));
        }

        info!("LLM credentials resolved from legacy credentials.json");
        return Ok((base_url.to_string(), jwt.to_string()));
    }

    Err(anyhow!("No LLM credentials found. Create llm.yaml in mission home or ~/.xjp/credentials.json."))
}

// ── Embedding Loop Worker (BackgroundWorker) ──────────────────────────────

pub(crate) struct EmbeddingLoopWorker {
    pub rx: tokio::sync::mpsc::Receiver<EmbeddingTask>,
}

impl super::BackgroundWorker for EmbeddingLoopWorker {
    fn name(&self) -> &'static str { "embedding" }

    async fn run(self, state: Arc<AppState>, _ctx: super::WorkerContext) {
        let mut rx = self.rx;
        info!("Embedding worker started (event-driven)");
        while let Some(task) = rx.recv().await {
            let db = state.mission.db();
            let provider_id = state.embedding_service.as_ref()
                .map(|svc| svc.provider_id().to_string())
                .unwrap_or_else(|| missiond_core::embedding::FASTEMBED_PROVIDER_ID.to_string());

            match task {
                EmbeddingTask::ProcessSession(session_id) => {
                    if tokio::time::timeout(
                        std::time::Duration::from_secs(60),
                        generate_and_store_conv_embedding(&state, &session_id),
                    ).await.is_err() {
                        warn!(session = %session_id, "Embedding generation timed out (60s)");
                    }
                }
                EmbeddingTask::ProcessKBEntry(id) => {
                    if let Some(ref emb_svc) = state.embedding_service {
                        if let Ok(Some(entry)) = db.kb_get_by_id(&id) {
                            let detail_text = entry.detail.as_ref()
                                .map(|d| serde_json::to_string(d).unwrap_or_default())
                                .unwrap_or_default();
                            let embed_text = format!("知识条目：{}\n详情：{}", entry.summary, detail_text);
                            let svc = Arc::clone(emb_svc);
                            if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                            ).await {
                                let _ = db.kb_set_embedding(&id, &vec, &provider_id);
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
                        if let Ok(missing) = db.skill_topics_missing_embedding(1) {
                            let embed_text = if let Some((_, text)) = missing.iter().find(|(t, _)| t == &topic) {
                                text.clone()
                            } else {
                                format!("技能主题：{}", topic)
                            };
                            let svc = Arc::clone(emb_svc);
                            if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                            ).await {
                                let _ = db.skill_set_topic_embedding(&topic, &vec, &provider_id);
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
                            if let Ok(Some(node_row)) = db.ast_get_node(node_id) {
                                let embed_text = node_row.node.embedding_text(&node_row.file_path);
                                let svc = Arc::clone(emb_svc);
                                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                    std::time::Duration::from_secs(30),
                                    tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                                ).await {
                                    let bytes = missiond_core::embedding::f32_vec_to_bytes(&vec);
                                    let _ = db.ast_set_embedding(node_id, &bytes, &provider_id);
                                    let mut guard = state.ast_embedding_cache.write().await;
                                    guard.retain(|(eid, _)| eid != node_id);
                                    guard.push((node_id.clone(), vec));
                                    embedded += 1;
                                }
                            }
                        }
                        if embedded > 0 {
                            debug!(count = embedded, total = node_ids.len(), "AST batch embedding completed");
                        }
                    }
                }
                EmbeddingTask::BackfillAll => {
                    run_backfill_all(&state, db, &provider_id).await;
                }
            }
        }
        warn!("Embedding worker channel closed");
    }
}

/// Full embedding backfill: KB + Skills + Conversations + Timelines + AST nodes.
async fn run_backfill_all(state: &AppState, db: &missiond_core::db::MissionDB, provider_id: &str) {
    info!("Full embedding backfill triggered");

    if let Some(ref emb_svc) = state.embedding_service {
        // ── Phase 1: KB stale re-embed ──
        loop {
            let stale = db.kb_entries_stale_embedding(provider_id, 20).unwrap_or_default();
            if stale.is_empty() { break; }
            info!(count = stale.len(), "KB stale re-embedding");
            for (id, summary, detail) in &stale {
                let embed_text = format!("知识条目：{}\n详情：{}", summary, detail);
                let svc = Arc::clone(emb_svc);
                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                ).await {
                    let _ = db.kb_set_embedding(id, &vec, provider_id);
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // ── Phase 2: KB missing embed ──
        loop {
            let missing = db.kb_entries_missing_embedding(None).unwrap_or_default();
            if missing.is_empty() { break; }
            info!(count = missing.len(), "KB missing embedding backfill");
            for (id, summary, detail) in &missing {
                let embed_text = format!("知识条目：{}\n详情：{}", summary, detail);
                let svc = Arc::clone(emb_svc);
                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                ).await {
                    let _ = db.kb_set_embedding(id, &vec, provider_id);
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // Warm KB caches after backfill
        if let Ok(all) = db.kb_load_embeddings("policy:decision") {
            let mut guard = state.embedding_cache.write().await;
            *guard = all;
            info!(count = guard.len(), "KB policy cache refreshed after backfill");
        }
        if let Ok(all) = db.kb_load_all_embeddings() {
            let mut guard = state.kb_search_cache.write().await;
            *guard = all;
            info!(count = guard.len(), "KB search cache refreshed after backfill");
        }

        // ── Phase 3: Skill stale + missing ──
        loop {
            let stale = db.skill_topics_stale_embedding(provider_id, 20).unwrap_or_default();
            if stale.is_empty() { break; }
            info!(count = stale.len(), "Skill stale re-embedding");
            for (topic, embed_text) in &stale {
                let svc = Arc::clone(emb_svc);
                let text = embed_text.clone();
                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    tokio::task::spawn_blocking(move || svc.embed(&text)),
                ).await {
                    let _ = db.skill_set_topic_embedding(topic, &vec, provider_id);
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        loop {
            let missing = db.skill_topics_missing_embedding(20).unwrap_or_default();
            if missing.is_empty() { break; }
            info!(count = missing.len(), "Skill missing embedding backfill");
            for (topic, embed_text) in &missing {
                let svc = Arc::clone(emb_svc);
                let text = embed_text.clone();
                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    tokio::task::spawn_blocking(move || svc.embed(&text)),
                ).await {
                    let _ = db.skill_set_topic_embedding(topic, &vec, provider_id);
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // Warm Skill cache after backfill
        if let Ok(all) = db.skill_load_topic_embeddings() {
            let mut guard = state.skill_embedding_cache.write().await;
            *guard = all;
            info!(count = guard.len(), "Skill embedding cache refreshed after backfill");
        }

        // ── Phase 4: Conversation topic vector backfill ──
        loop {
            let needing = db.conversations_needing_topic_vectors(provider_id, 20).unwrap_or_default();
            if needing.is_empty() { break; }
            info!(count = needing.len(), "Conv topic vector backfill");
            for session_id in &needing {
                if tokio::time::timeout(
                    std::time::Duration::from_secs(90),
                    generate_and_store_conv_embedding(state, session_id),
                ).await.is_err() {
                    warn!(session = %session_id, "Topic vector backfill timed out");
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    // ── Phase 4.5: Build session timelines for compaction fragments ──
    {
        let needing = db.conversations_needing_timeline(50).unwrap_or_default();
        if !needing.is_empty() {
            info!(count = needing.len(), "Building session timelines for compaction parents");
        }
        for parent_id in &needing {
            let fragments = match db.get_compaction_fragments(parent_id) {
                Ok(f) => f,
                Err(e) => {
                    warn!(parent = %parent_id, error = %e, "Failed to get compaction fragments");
                    continue;
                }
            };
            if fragments.is_empty() { continue; }

            let mut timeline_entries = Vec::new();
            for (idx, (frag_id, started_at, msg_count)) in fragments.iter().enumerate() {
                let summary = db.get_last_assistant_content(frag_id).unwrap_or(None);
                let summary_tokens = summary.as_ref().map(|s| s.len() / 4).unwrap_or(0);
                timeline_entries.push(serde_json::json!({
                    "fragment_id": frag_id,
                    "shard_index": idx,
                    "started_at": started_at,
                    "message_count": msg_count,
                    "summary_tokens": summary_tokens,
                    "summary": summary,
                    "segment_embedding_id": null,
                }));
            }

            let timeline_json = serde_json::to_string(&timeline_entries)
                .unwrap_or_else(|_| "[]".to_string());

            match db.set_session_timeline(parent_id, &timeline_json) {
                Ok(true) => {
                    let _ = db.clear_conversation_summary(parent_id);
                    info!(
                        parent = %parent_id,
                        fragments = fragments.len(),
                        "Session timeline built, summary cleared for regeneration"
                    );
                }
                Ok(false) => {}
                Err(e) => {
                    warn!(parent = %parent_id, error = %e, "Failed to set session timeline");
                }
            }
        }
    }

    // ── Phase 5: Conversation missing summary+embed ──
    loop {
        let missing = db.conversations_missing_summary(20).unwrap_or_default();
        if missing.is_empty() { break; }
        info!(count = missing.len(), "Conv backfill batch");
        for (idx, session_id) in missing.iter().enumerate() {
            info!(session = %session_id, idx = idx + 1, total = missing.len(), "Processing session");
            match tokio::time::timeout(
                std::time::Duration::from_secs(60),
                generate_and_store_conv_embedding(state, session_id),
            ).await {
                Ok(()) => {
                    info!(session = %session_id, idx = idx + 1, "Session done");
                }
                Err(_) => {
                    warn!(session = %session_id, idx = idx + 1, "Conv embedding timed out (60s), skipping");
                    let _ = db.set_conversation_summary(session_id, "[timeout]");
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // ── Phase 6: AST node embedding backfill ──
    if let Some(ref emb_svc) = state.embedding_service {
        loop {
            let missing = db.ast_find_unembedded(20).unwrap_or_default();
            if missing.is_empty() { break; }
            info!(count = missing.len(), "AST embedding backfill batch");
            for (node_id, _repo, file_path) in &missing {
                if let Ok(Some(node_row)) = db.ast_get_node(node_id) {
                    let embed_text = node_row.node.embedding_text(file_path);
                    let svc = Arc::clone(emb_svc);
                    if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                        std::time::Duration::from_secs(30),
                        tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                    ).await {
                        let bytes = missiond_core::embedding::f32_vec_to_bytes(&vec);
                        let _ = db.ast_set_embedding(node_id, &bytes, &provider_id);
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        // Refresh AST embedding cache after backfill
        if let Ok(all) = db.ast_load_all_embeddings() {
            let mut guard = state.ast_embedding_cache.write().await;
            *guard = all;
            info!(count = guard.len(), "AST embedding cache refreshed after backfill");
        }
    }

    info!("Full embedding backfill complete");
}
