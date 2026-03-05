
use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::state::AppState;
use std::sync::Arc;
use std::path::PathBuf;
use crate::helpers::char_boundary_at;
use crate::helpers::default_mission_home;

#[derive(serde::Deserialize)]
pub(crate) struct LlmConfig {
    #[serde(default = "LlmConfig::default_provider")]
    pub(crate) provider: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    auth: LlmAuth,
    #[serde(default = "LlmConfig::default_model")]
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

/// Generate LLM summary + embedding for a completed conversation, store to DB and update cache.
///
/// Pipeline:
/// 1. Fetch last N messages from DB
/// 2. Call Router LLM for structured summary (fallback: first+last concatenation)
/// 3. Generate embedding via EmbeddingService
/// 4. Store summary + embedding to DB
/// 5. Update in-memory cache
pub(crate) async fn generate_and_store_conv_embedding(state: &AppState, session_id: &str) {
    let db = state.mission.db();

    // 1. Fetch messages (last 200 to stay within LLM context budget)
    debug!(session = %session_id, "Step 1: fetching messages");
    let messages = match db.get_conversation_messages(session_id, None, 200) {
        Ok(msgs) if !msgs.is_empty() => msgs,
        Ok(_) => {
            debug!(session = %session_id, "No messages for summary generation");
            return;
        }
        Err(e) => {
            warn!(session = %session_id, error = %e, "Failed to fetch messages for summary");
            return;
        }
    };

    // 2. Build conversation text for LLM (budget: ~8K chars)
    debug!(session = %session_id, msg_count = messages.len(), "Step 2: building conv_text");
    let mut conv_text = String::with_capacity(8192);

    // 2a. If parent has session_timeline, prepend compaction fragment summaries
    let has_timeline = if let Ok(Some(conv)) = db.get_conversation(session_id) {
        if let Some(ref tl_json) = conv.session_timeline {
            if let Ok(timeline) = serde_json::from_str::<Vec<serde_json::Value>>(tl_json) {
                if !timeline.is_empty() {
                    conv_text.push_str("=== 会话历史阶段摘要 (compaction fragments) ===\n");
                    let fragments: Vec<&serde_json::Value> = if timeline.len() > 10 {
                        // Degradation: first 3 + last 3 + 2 middle samples
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
                            // Truncate each fragment summary to ~300 tokens (~1200 chars)
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

    // 2b. Append recent messages (reduce budget if timeline was prepended)
    let msg_budget = if has_timeline { 4000 } else { 8000 };
    for msg in &messages {
        let prefix = match msg.role.as_str() {
            "user" => "U",
            "assistant" => "A",
            "tool_use" => "T",
            "tool_result" => "R",
            _ => "?",
        };
        let content = if msg.content.len() > 500 {
            format!("{}…", &msg.content[..char_boundary_at(&msg.content, 500)])
        } else {
            msg.content.clone()
        };
        conv_text.push_str(&format!("[{}] {}\n", prefix, content));
        if conv_text.len() > msg_budget {
            break;
        }
    }

    // 3. Call Router LLM for summary (with fallback)
    debug!(session = %session_id, text_len = conv_text.len(), has_timeline, "Step 3: calling LLM for summary");
    let summary = match generate_conv_summary_llm(state, &conv_text, has_timeline).await {
        Some(s) => {
            debug!(session = %session_id, summary_len = s.len(), "LLM summary OK");
            s
        }
        None => {
            debug!(session = %session_id, "LLM summary failed, using fallback");
            // Fallback: concatenate first 3 + last 3 messages
            let first3: Vec<&str> = messages.iter().take(3).map(|m| m.content.as_str()).collect();
            let last3: Vec<&str> = messages.iter().rev().take(3).rev().map(|m| m.content.as_str()).collect();
            let mut fallback = first3.join("\n");
            fallback.push_str("\n...\n");
            fallback.push_str(&last3.join("\n"));
            if fallback.len() > 2000 {
                fallback.truncate(char_boundary_at(&fallback, 2000));
            }
            fallback
        }
    };

    // 4. Store summary to DB
    debug!(session = %session_id, "Step 4: storing summary");
    if let Err(e) = db.set_conversation_summary(session_id, &summary) {
        warn!(session = %session_id, error = %e, "Failed to store conversation summary");
        return;
    }

    // 5. Generate embedding from summary (via spawn_blocking to avoid block_in_place deadlock)
    let embedding_service = match &state.embedding_service {
        Some(svc) => svc,
        None => {
            debug!(session = %session_id, "No embedding service, skipping vector generation");
            return;
        }
    };
    let provider_id = embedding_service.provider_id().to_string();
    debug!(session = %session_id, provider = %provider_id, "Step 5: generating embedding");
    let svc = Arc::clone(embedding_service);
    let summary_for_embed = summary.clone();
    let embedding = match tokio::task::spawn_blocking(move || svc.embed(&summary_for_embed)).await {
        Ok(Some(vec)) => vec,
        Ok(None) => {
            warn!(session = %session_id, "Embedding generation returned None");
            return;
        }
        Err(e) => {
            warn!(session = %session_id, error = %e, "Embedding spawn_blocking panicked");
            return;
        }
    };
    debug!(session = %session_id, dim = embedding.len(), "Step 5: embedding OK");

    // 6. Store embedding to DB
    debug!(session = %session_id, "Step 6: storing embedding");
    if let Err(e) = db.set_conversation_embedding(session_id, &embedding, &provider_id) {
        warn!(session = %session_id, error = %e, "Failed to store conversation embedding");
        return;
    }

    // 7. Update in-memory cache
    debug!(session = %session_id, "Step 7: updating cache");
    let mut cache = state.conversation_embedding_cache.write().await;
    // Remove old entry if exists, then push new
    cache.retain(|(id, _)| id != session_id);
    cache.push((session_id.to_string(), embedding));

    info!(session = %session_id, "Conversation summary + embedding generated");
}

/// Call Router LLM to generate a structured conversation summary.
/// Returns None if Router is unreachable or response is invalid.
pub(crate) async fn generate_conv_summary_llm(state: &AppState, conv_text: &str, has_timeline: bool) -> Option<String> {
    let (base_url, jwt) = resolve_llm_credentials().await.ok()?;
    let url = format!("{}/v1/chat/completions", base_url);

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

    let body = serde_json::json!({
        "model": "gemini-3.1-flash-lite",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": conv_text}
        ],
        "max_tokens": 512,
    });

    let result = state.gemini.send_best_effort(
        &state.http_client, &url, &jwt, &body,
        std::time::Duration::from_secs(60),
    ).await?;
    let content = result
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    if let Some(ref s) = content {
        debug!(len = s.len(), "LLM summary generated");
    }
    content
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
