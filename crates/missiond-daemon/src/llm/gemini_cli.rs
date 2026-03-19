//! General-purpose Gemini CLI subprocess wrapper.
//!
//! Calls the official Gemini CLI (`@google/gemini-cli`) in headless streaming JSON mode
//! to access Google One AI Pro models without Vertex AI rate limits.
//!
//! Architecture: Uses `-o stream-json` (NDJSON) instead of `-o json` to enable
//! idle-timeout based liveness detection. As long as the CLI emits events
//! (init, message, thinking deltas, result), the process is considered alive.
//!
//! Two-tier idle timeout: Gemini CLI is agentic and calls tools (file reads,
//! code search) during reasoning. Tool execution can take much longer than text
//! generation, so we use an extended timeout (5min) between tool_use → tool_result,
//! and the normal idle timeout (120s) otherwise.
//!
//! Usage:
//! ```ignore
//! let cli = GeminiCli::new("gemini", "gemini-3.1-pro-preview", Duration::from_secs(120));
//! let resp = cli.call(&messages, None, None, None).await?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};


/// Absolute safety cap: kill process no matter what after this duration.
const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(900); // 15 minutes

/// Extended idle timeout when Gemini CLI is executing tools (agentic mode).
/// Tool execution (file reads, code search) can take much longer than text generation.
const TOOL_EXEC_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

// ===== API Key Pool =====

/// A resolved API key entry with optional alias and default flag.
#[derive(Clone, Debug)]
pub(crate) struct ApiKeyEntry {
    pub key: String,
    pub alias: Option<String>,
    pub is_default: bool,
}

/// Pool of Gemini API keys with blacklist-based rotation.
/// When a key hits quota exhaustion, it's blacklisted until next 07:00 GMT+8.
/// Keys are hot-reloaded from llm.yaml on every call — no restart needed to add/remove keys.
pub(crate) struct ApiKeyPool {
    blacklist: RwLock<HashMap<String, tokio::time::Instant>>,
}

impl ApiKeyPool {
    pub fn new() -> Self {
        Self { blacklist: RwLock::new(HashMap::new()) }
    }

    /// Returns the first non-blacklisted key. Default key is tried first, then others in order.
    /// Returns error if no default is configured and no alias is specified.
    pub async fn next_available_key(&self) -> Result<ApiKeyEntry> {
        let entries = resolve_apikey_entries();
        if entries.is_empty() {
            return Err(anyhow!("gemini_api_keys 未配置"));
        }
        if !entries.iter().any(|e| e.is_default) {
            return Err(anyhow!("gemini_api_keys 中未标记 default: true，无法确定默认 key"));
        }
        let bl = self.blacklist.read().await;
        let now = tokio::time::Instant::now();
        let is_available = |k: &str| bl.get(k).map(|exp| now >= *exp).unwrap_or(true);

        // Default key first, then others in config order
        let mut sorted = entries.clone();
        sorted.sort_by_key(|e| if e.is_default { 0 } else { 1 });

        sorted.into_iter()
            .find(|e| is_available(&e.key))
            .ok_or_else(|| {
                let total = entries.len();
                anyhow!("All {} Gemini API keys quota exhausted. Next reset ~07:00 GMT+8", total)
            })
    }

    /// Get a specific key by alias. Returns error if alias not found.
    pub async fn get_by_alias(&self, alias: &str) -> Result<ApiKeyEntry> {
        let entries = resolve_apikey_entries();
        let entry = entries.into_iter()
            .find(|e| e.alias.as_deref() == Some(alias))
            .ok_or_else(|| anyhow!("未知 API key 别名: '{}'. 可用别名: {}",
                alias,
                resolve_apikey_entries().iter()
                    .filter_map(|e| e.alias.as_deref())
                    .collect::<Vec<_>>().join(", ")
            ))?;
        // Check blacklist
        let bl = self.blacklist.read().await;
        let now = tokio::time::Instant::now();
        if let Some(exp) = bl.get(&entry.key) {
            if now < *exp {
                return Err(anyhow!("API key '{}' quota exhausted until ~07:00 GMT+8", alias));
            }
        }
        Ok(entry)
    }

    /// Mark a key as quota-exhausted until next 07:00 GMT+8.
    pub async fn mark_exhausted(&self, key: &str, alias: Option<&str>) {
        let ttl = duration_until_next_07_gmt8();
        let expiry = tokio::time::Instant::now() + ttl;
        let mut bl = self.blacklist.write().await;
        bl.insert(key.to_string(), expiry);
        info!(key = %mask_api_key(key), alias = alias.unwrap_or("unnamed"),
              ttl_secs = ttl.as_secs(), "ApiKeyPool: key blacklisted until next 07:00 GMT+8");
    }
}

impl std::fmt::Debug for ApiKeyPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyPool").field("keys", &resolve_apikey_entries().len()).finish()
    }
}

/// Calculate duration from now until next 07:00 GMT+8 (= 23:00 UTC previous day).
fn duration_until_next_07_gmt8() -> Duration {
    use chrono::{Utc, FixedOffset};
    let tz = FixedOffset::east_opt(8 * 3600).unwrap();
    let now = Utc::now().with_timezone(&tz);
    let today_07 = now.date_naive().and_hms_opt(7, 0, 0).unwrap()
        .and_local_timezone(tz).unwrap();
    let target = if now.naive_local() < today_07.naive_local() {
        today_07
    } else {
        today_07 + chrono::Duration::days(1)
    };
    let secs = (target - now).num_seconds().max(60) as u64;
    Duration::from_secs(secs)
}

/// Gemini CLI subprocess wrapper.
///
/// Supports two transport modes:
/// - **Subprocess (NDJSON)**: `gemini -o stream-json -p "prompt"` — legacy, currently broken
/// - **PTY**: `gemini --yolo -m model` in interactive terminal — active mode
#[derive(Clone, Debug)]
pub(crate) struct GeminiCli {
    binary: String,
    default_model: String,
    idle_timeout: Duration,
    api_key_pool: Option<Arc<ApiKeyPool>>,
    /// PTY-based transport. When set, all calls route through PTY instead of subprocess.
    pty_transport: Option<Arc<super::gemini_pty::GeminiPtyTransport>>,
}

/// Response from a Gemini CLI invocation.
pub(crate) struct GeminiCliResponse {
    pub content: String,
    pub model: String,
}

/// Real-time progress event emitted during CLI execution.
#[derive(Debug)]
pub(crate) struct GeminiCliProgress {
    /// Sequential index within this request (1, 2, 3...)
    pub tool_seq: u32,
    /// "tool_use" or "tool_result"
    pub activity: String,
    /// Tool name (e.g. "read_file", "grep", "bash")
    pub tool_name: Option<String>,
    /// Truncated input preview (≤200 chars)
    pub input_preview: Option<String>,
    /// Truncated result preview (≤500 chars)
    pub result_preview: Option<String>,
    /// Whether the tool result was an error
    pub is_error: bool,
}

impl GeminiCli {
    pub fn new(binary: String, default_model: String, idle_timeout: Duration, api_key_pool: Option<Arc<ApiKeyPool>>) -> Self {
        Self { binary, default_model, idle_timeout, api_key_pool, pty_transport: None }
    }

    /// Enable PTY transport mode. All subsequent calls route through PTY.
    pub fn with_pty(mut self, transport: Arc<super::gemini_pty::GeminiPtyTransport>) -> Self {
        self.pty_transport = Some(transport);
        self
    }

    /// Check if PTY transport is enabled.
    pub fn has_pty(&self) -> bool {
        self.pty_transport.is_some()
    }

    /// Core method: convert messages to prompt, call CLI with stream-json, parse NDJSON events.
    ///
    /// Idle timeout: process is killed only if no stdout line arrives within `idle_timeout`.
    /// This means long thinking (with streaming deltas) or large responses never cause timeout.
    ///
    /// `progress_tx`: optional channel to emit real-time tool activity events.
    /// `auth_override`: per-call channel override ("apikey" or "google"). Skips global config when set.
    /// `working_dir`: optional working directory for CLI (enables file-based agentic workflows).
    pub async fn call(
        &self,
        messages: &[Value],
        model: Option<&str>,
        _max_tokens: Option<u32>,
        idle_timeout_override: Option<Duration>,
        progress_tx: Option<mpsc::UnboundedSender<GeminiCliProgress>>,
        auth_override: Option<&str>,
        api_key_alias: Option<&str>,
        working_dir: Option<&std::path::Path>,
    ) -> Result<GeminiCliResponse> {
        let model = model.unwrap_or(&self.default_model);
        let idle_timeout = idle_timeout_override.unwrap_or(self.idle_timeout);
        let prompt = messages_to_prompt(messages);

        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt after message conversion"));
        }

        // PTY transport: bypass subprocess NDJSON entirely
        if let Some(ref pty) = self.pty_transport {
            info!(model, prompt_len = prompt.len(), idle_timeout_secs = idle_timeout.as_secs(),
                  "Gemini CLI: calling via PTY transport");
            let content = pty.call(&prompt, idle_timeout).await?;
            return Ok(GeminiCliResponse {
                content,
                model: model.to_string(),
            });
        }

        info!(model, prompt_len = prompt.len(), idle_timeout_secs = idle_timeout.as_secs(),
              "Gemini CLI: calling (stream-json mode)");

        // Determine resolved mode
        let is_apikey_mode = match auth_override {
            Some("apikey") => true,
            Some("google") => false,
            Some(other) => return Err(anyhow!("未知 channel: '{}', 支持 apikey/google", other)),
            None => resolve_gemini_auth_config().mode == "apikey",
        };

        // If apikey mode + pool available → key selection/rotation
        if is_apikey_mode {
            if let Some(ref pool) = self.api_key_pool {
                return if let Some(alias) = api_key_alias {
                    // Pinned key: no rotation, fail fast if exhausted
                    self.call_with_pinned_key(pool, alias, &prompt, model, idle_timeout, &progress_tx, working_dir).await
                } else {
                    // Pool rotation: default key first, failover to others
                    self.call_with_key_rotation(pool, &prompt, model, idle_timeout, &progress_tx, working_dir).await
                };
            }
        }

        // Original single-key path (google mode or no pool)
        let is_override = auth_override.is_some();
        let auth_config = match auth_override {
            Some("apikey") => resolve_apikey_config()
                .ok_or_else(|| anyhow!("channel='apikey' 但 llm.yaml 中未配置 gemini_api_key"))?,
            Some("google") => GeminiAuthConfig { mode: "google".to_string(), api_key: None },
            None => resolve_gemini_auth_config(),
            _ => unreachable!(),
        };
        info!(mode = %auth_config.mode, r#override = is_override, "Gemini CLI: using {} mode", auth_config.mode);

        self.call_once(&auth_config, &prompt, model, idle_timeout, &progress_tx, working_dir).await
    }

    /// Pinned key: use exactly the specified alias, no rotation.
    /// If the key is quota-exhausted, fail immediately.
    async fn call_with_pinned_key(
        &self,
        pool: &ApiKeyPool,
        alias: &str,
        prompt: &str,
        model: &str,
        idle_timeout: Duration,
        progress_tx: &Option<mpsc::UnboundedSender<GeminiCliProgress>>,
        working_dir: Option<&std::path::Path>,
    ) -> Result<GeminiCliResponse> {
        let entry = pool.get_by_alias(alias).await?;
        info!(alias = alias, key = %mask_api_key(&entry.key), "Gemini CLI: using pinned API key");

        let auth = GeminiAuthConfig {
            mode: "apikey".to_string(),
            api_key: Some(entry.key.clone()),
        };
        let result = self.call_once(&auth, prompt, model, idle_timeout, progress_tx, working_dir).await;

        // If quota exhausted, mark and return error (no rotation)
        if let Err(ref e) = result {
            let err_str = e.to_string();
            if is_quota_exhausted(None, &err_str) {
                pool.mark_exhausted(&entry.key, Some(alias)).await;
            }
        }
        result
    }

    /// Single attempt: spawn CLI, stream events, check exit status.
    async fn call_once(
        &self,
        auth_config: &GeminiAuthConfig,
        prompt: &str,
        model: &str,
        idle_timeout: Duration,
        progress_tx: &Option<mpsc::UnboundedSender<GeminiCliProgress>>,
        working_dir: Option<&std::path::Path>,
    ) -> Result<GeminiCliResponse> {
        let mut child = self.spawn_cli(prompt, model, auth_config, working_dir)?;
        let (events, drained_stderr) = self.stream_events(&mut child, idle_timeout, progress_tx).await?;

        let status = child.wait().await
            .map_err(|e| anyhow!("Gemini CLI process error: {}", e))?;
        if !status.success() {
            let exit_code = status.code();
            let stderr_msg: String = drained_stderr.chars().take(500).collect();

            if auth_config.mode == "google" && is_auth_error(exit_code, &stderr_msg) {
                return Err(anyhow!(
                    "Gemini CLI: OAuth auth failed (exit {}). Run `gemini` to re-authenticate or switch to apikey mode. stderr: {}",
                    exit_code.unwrap_or(-1), stderr_msg
                ));
            }

            return Err(anyhow!("Gemini CLI exited with {}: {}", status, stderr_msg));
        }

        parse_stream_events(&events, model)
    }

    /// Retry loop with API key rotation on quota exhaustion.
    /// Default key is tried first, then others in config order. On quota error (no content produced),
    /// marks the key as exhausted and tries the next.
    async fn call_with_key_rotation(
        &self,
        pool: &ApiKeyPool,
        prompt: &str,
        model: &str,
        idle_timeout: Duration,
        progress_tx: &Option<mpsc::UnboundedSender<GeminiCliProgress>>,
        working_dir: Option<&std::path::Path>,
    ) -> Result<GeminiCliResponse> {
        let mut attempted = 0usize;
        loop {
            let entry = pool.next_available_key().await?;
            let alias_str = entry.alias.as_deref().unwrap_or("unnamed");

            attempted += 1;
            let auth = GeminiAuthConfig {
                mode: "apikey".to_string(),
                api_key: Some(entry.key.clone()),
            };
            info!(alias = alias_str, key = %mask_api_key(&entry.key), attempt = attempted,
                  "Gemini CLI: trying API key from pool");

            let mut child = self.spawn_cli(prompt, model, &auth, working_dir)?;
            let (events, drained_stderr) = self.stream_events(&mut child, idle_timeout, progress_tx).await?;

            let status = child.wait().await
                .map_err(|e| anyhow!("Gemini CLI process error: {}", e))?;

            if status.success() {
                return parse_stream_events(&events, model);
            }

            let exit_code = status.code();
            let stderr_msg: String = drained_stderr.chars().take(500).collect();

            // Only rotate on quota exhaustion AND no content produced yet
            let has_content = events.iter().any(|e| {
                e.get("type").and_then(|t| t.as_str()) == Some("message")
                    && matches!(e.get("role").and_then(|r| r.as_str()), Some("assistant") | Some("model"))
                    && e.get("content").and_then(|c| c.as_str()).map(|s| !s.is_empty()).unwrap_or(false)
            });

            if !has_content && is_quota_exhausted(exit_code, &stderr_msg) {
                warn!(alias = alias_str, key = %mask_api_key(&entry.key), exit_code = exit_code.unwrap_or(-1),
                      "Gemini CLI: API key quota exhausted, rotating to next key");
                pool.mark_exhausted(&entry.key, entry.alias.as_deref()).await;
                continue;
            }

            // Non-quota error → fail fast
            return Err(anyhow!("Gemini CLI exited with {}: {}", status, stderr_msg));
        }
    }

    fn spawn_cli(&self, prompt: &str, model: &str, auth: &GeminiAuthConfig, working_dir: Option<&std::path::Path>) -> Result<tokio::process::Child> {
        let mut cmd = tokio::process::Command::new(&self.binary);
        cmd.args(["-p", prompt, "-m", model, "-o", "stream-json", "--yolo"])
            .stdin(std::process::Stdio::null())  // Prevent interactive prompts from hanging
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);  // Cancel safety: kill orphan process when future is dropped

        // Set working directory for file-based agentic workflows (e.g. strategy_analyst workspace)
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        if let Some(ref key) = auth.api_key {
            cmd.env("GEMINI_API_KEY", key);
        }

        // Per-subprocess auth override via GEMINI_CLI_HOME:
        // Gemini CLI reads settings from $GEMINI_CLI_HOME/.gemini/settings.json.
        // GEMINI_API_KEY env var alone does NOT override settings.json selectedType,
        // so both modes need a shadow home with the correct selectedType.
        match auth.mode.as_str() {
            "google" => {
                let home = ensure_auth_home("google", "oauth-personal", true)?;
                cmd.env("GEMINI_CLI_HOME", home);
            }
            "apikey" => {
                let home = ensure_auth_home("apikey", "gemini-api-key", false)?;
                cmd.env("GEMINI_CLI_HOME", home);
            }
            _ => {}
        }

        cmd.spawn()
            .map_err(|e| anyhow!("Failed to spawn gemini CLI '{}': {}", self.binary, e))
    }

    /// Stream NDJSON events from a running CLI process.
    /// Returns (events, drained_stderr) — stderr is concurrently drained to prevent pipe deadlock.
    async fn stream_events(
        &self,
        child: &mut tokio::process::Child,
        idle_timeout: Duration,
        progress_tx: &Option<mpsc::UnboundedSender<GeminiCliProgress>>,
    ) -> Result<(Vec<Value>, String)> {
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to capture stdout"))?;

        // P0 fix: concurrently drain stderr to prevent 64KB pipe buffer deadlock.
        // Without this, if Gemini CLI writes >64KB to stderr while we only read stdout,
        // the OS pipe buffer fills up and the subprocess blocks forever.
        // Capped at 1MB to prevent OOM if subprocess emits unbounded error output.
        let stderr = child.stderr.take();
        let stderr_handle = tokio::spawn(async move {
            let mut buf = String::new();
            if let Some(stderr) = stderr {
                let limited = tokio::io::AsyncReadExt::take(stderr, 1024 * 1024);
                let mut limited = tokio::io::BufReader::new(limited);
                let _ = tokio::io::AsyncReadExt::read_to_string(&mut limited, &mut buf).await;
            }
            buf
        });

        let mut lines = BufReader::new(stdout).lines();
        let mut events: Vec<Value> = Vec::new();
        let absolute_deadline = tokio::time::Instant::now() + ABSOLUTE_TIMEOUT;
        let mut awaiting_tool_result = false;
        let mut tool_seq: u32 = 0;

        let stream_result: Result<(), String> = async {
            loop {
                let remaining = absolute_deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err("absolute timeout (15min)".to_string());
                }
                let current_idle = if awaiting_tool_result { TOOL_EXEC_TIMEOUT } else { idle_timeout };
                let effective_timeout = current_idle.min(remaining);

                match tokio::time::timeout(effective_timeout, lines.next_line()).await {
                    Ok(Ok(Some(line))) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<Value>(&line) {
                            Ok(event) => {
                                let event_type = event.get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                match event_type {
                                    "tool_use" => {
                                        awaiting_tool_result = true;
                                        tool_seq += 1;
                                        if let Some(ref tx) = progress_tx {
                                            let _ = tx.send(extract_tool_use_progress(&event, tool_seq));
                                        }
                                    }
                                    "tool_result" => {
                                        awaiting_tool_result = false;
                                        tool_seq += 1;
                                        if let Some(ref tx) = progress_tx {
                                            let _ = tx.send(extract_tool_result_progress(&event, tool_seq));
                                        }
                                    }
                                    _ => {}
                                }
                                if event_type == "result" || event_type == "error" {
                                    events.push(event);
                                    return Ok(());
                                }
                                events.push(event);
                            }
                            Err(e) => {
                                warn!(line_len = line.len(), error = %e,
                                      "Gemini CLI: skipping non-JSON line");
                            }
                        }
                    }
                    Ok(Ok(None)) => return Ok(()),
                    Ok(Err(e)) => return Err(format!("IO error: {}", e)),
                    Err(_) => {
                        let mode = if awaiting_tool_result { "tool_exec" } else { "idle" };
                        return Err(format!("{} timeout ({}s no output)", mode, effective_timeout.as_secs()));
                    }
                }
            }
        }.await;

        // Collect drained stderr (best-effort, don't fail on join error)
        let drained_stderr = stderr_handle.await.unwrap_or_default();

        if let Err(ref reason) = stream_result {
            warn!(reason, stderr_len = drained_stderr.len(), "Gemini CLI: killing process");
            let _ = child.kill().await;
            let _ = child.wait().await;
            let stderr_preview: String = drained_stderr.chars().take(500).collect();
            return Err(anyhow!("Gemini CLI timed out: {}. stderr: {}", reason, stderr_preview));
        }

        Ok((events, drained_stderr))
    }

    /// Convenience: single prompt string.
    #[allow(dead_code)]
    pub async fn prompt(&self, text: &str, model: Option<&str>) -> Result<String> {
        let messages = vec![serde_json::json!({"role": "user", "content": text})];
        let resp = self.call(&messages, model, None, None, None, None, None, None).await?;
        Ok(resp.content)
    }
}

/// Default system instruction prepended to all Gemini CLI prompts.
const DEFAULT_SYSTEM_INSTRUCTION: &str = "System: 不要修改任何代码。只调查分析和回答问题。";

/// Convert OpenAI-style messages array to a single prompt string.
fn messages_to_prompt(messages: &[Value]) -> String {
    let mut parts = vec![DEFAULT_SYSTEM_INSTRUCTION.to_string()];
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        if content.is_empty() { continue; }
        let label = match role {
            "system" => "System",
            "assistant" => "Assistant",
            _ => "User",
        };
        parts.push(format!("{}: {}", label, content));
    }
    parts.join("\n\n--------\n\n")
}

/// Parse stream-json NDJSON events into a response.
///
/// Event types from Gemini CLI stream-json:
/// - `init`: session start, contains model
/// - `message` (role=user): echoed prompt
/// - `message` (role=assistant, delta=true): streaming response chunks
/// - `result`: final stats, status
/// - `error`: error occurred
fn parse_stream_events(events: &[Value], requested_model: &str) -> Result<GeminiCliResponse> {
    if events.is_empty() {
        return Err(anyhow!("Gemini CLI: no events received"));
    }

    // Collect all assistant message content (delta chunks)
    let mut content_parts: Vec<String> = Vec::new();
    let mut model = requested_model.to_string();

    for event in events {
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match event_type {
            "init" => {
                if let Some(m) = event.get("model").and_then(|v| v.as_str()) {
                    model = m.to_string();
                }
            }
            "message" => {
                let role = event.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "assistant" || role == "model" {
                    if let Some(content) = event.get("content").and_then(|v| v.as_str()) {
                        content_parts.push(content.to_string());
                    }
                }
            }
            "error" => {
                let msg = event.get("message").and_then(|v| v.as_str())
                    .or_else(|| event.get("error").and_then(|v| v.as_str()))
                    .unwrap_or("unknown error");
                return Err(anyhow!("Gemini CLI error event: {}", msg));
            }
            "result" => {
                let status = event.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
                if status != "success" {
                    return Err(anyhow!("Gemini CLI result status: {}", status));
                }
            }
            _ => {
                info!(event_type, "Gemini CLI: unhandled event type");
            }
        }
    }

    if content_parts.is_empty() {
        return Err(anyhow!("Gemini CLI: no assistant content in {} events", events.len()));
    }

    // stream-json emits incremental delta chunks — concatenate all to get full response.
    let content = content_parts.join("");

    info!(content_len = content.len(), events = events.len(), "Gemini CLI: stream complete");

    Ok(GeminiCliResponse { content, model })
}

/// Truncate a string to `max` chars, appending "…" if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end: String = s.chars().take(max).collect();
        format!("{}…", end)
    }
}

/// Extract progress event from a tool_use NDJSON event.
///
/// Gemini CLI stream-json format:
/// ```json
/// {"type":"tool_use","tool_name":"read_file","tool_id":"...","parameters":{...}}
/// ```
fn extract_tool_use_progress(event: &Value, tool_seq: u32) -> GeminiCliProgress {
    // Gemini CLI uses "tool_name"; fallback to "name" for forward compat
    let tool_name = event.get("tool_name")
        .or_else(|| event.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // Gemini CLI uses "parameters"; fallback to "input" for forward compat
    let input_preview = event.get("parameters")
        .or_else(|| event.get("input"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                truncate(s, 200)
            } else {
                truncate(&v.to_string(), 200)
            }
        });
    GeminiCliProgress {
        tool_seq,
        activity: "tool_use".to_string(),
        tool_name,
        input_preview,
        result_preview: None,
        is_error: false,
    }
}

/// Extract progress event from a tool_result NDJSON event.
///
/// Gemini CLI stream-json format:
/// ```json
/// {"type":"tool_result","tool_id":"...","status":"success","output":"..."}
/// ```
fn extract_tool_result_progress(event: &Value, tool_seq: u32) -> GeminiCliProgress {
    // Gemini CLI uses "status" string; fallback to "is_error" bool
    let is_error = event.get("status")
        .and_then(|v| v.as_str())
        .map(|s| s != "success")
        .or_else(|| event.get("is_error").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    // Gemini CLI uses "output"; fallback to "result"/"content" for forward compat
    let result_preview = event.get("output")
        .or_else(|| event.get("result"))
        .or_else(|| event.get("content"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                truncate(s, 500)
            } else {
                truncate(&v.to_string(), 500)
            }
        });
    // Gemini CLI tool_result includes tool_id but not tool_name; extract from tool_id prefix
    let tool_name = event.get("tool_id")
        .and_then(|v| v.as_str())
        .and_then(|id| id.split('_').next())
        .filter(|name| !name.is_empty())
        .map(|s| s.to_string());
    GeminiCliProgress {
        tool_seq,
        activity: "tool_result".to_string(),
        tool_name,
        input_preview: None,
        result_preview,
        is_error,
    }
}

// ===== Auth config helpers =====

/// Resolved auth configuration for a Gemini CLI subprocess.
struct GeminiAuthConfig {
    mode: String,       // "apikey" or "google"
    api_key: Option<String>,
}

/// Read llm.yaml to get gemini_api_key and gemini_auth_mode.
/// Then read ~/.gemini/settings.json as fallback for mode detection.
/// Also detects split-brain between llm.yaml and settings.json, auto-syncing if diverged.
fn resolve_gemini_auth_config() -> GeminiAuthConfig {
    let llm_yaml = missiond_core::default_mission_home().join("llm.yaml");
    let llm_config = std::fs::read_to_string(&llm_yaml).ok()
        .and_then(|c| serde_yaml::from_str::<serde_yaml::Value>(&c).ok());

    let api_key = llm_config.as_ref()
        .and_then(|c| c.get("gemini_api_key").and_then(|v| v.as_str()).map(|s| s.to_string()));

    // Mode: llm.yaml gemini_auth_mode > settings.json selectedType > default apikey
    let mode = llm_config.as_ref()
        .and_then(|c| c.get("gemini_auth_mode").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| {
            dirs::home_dir()
                .and_then(|h| std::fs::read_to_string(h.join(".gemini/settings.json")).ok())
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.pointer("/security/auth/selectedType").and_then(|t| t.as_str()).map(|s| s.to_string()))
                .map(|t| if t == "oauth-personal" { "google".to_string() } else { "apikey".to_string() })
                .unwrap_or_else(|| "apikey".to_string())
        });

    // Note: No longer mutating global ~/.gemini/settings.json (P1 audit fix).
    // Per-subprocess auth is handled via GEMINI_CLI_HOME shadow home directory,
    // so split-brain between llm.yaml and settings.json is harmless.

    if mode == "apikey" {
        if let Some(ref key) = api_key {
            info!(key_preview = mask_api_key(key), "Gemini CLI: using API key");
        }
    }

    GeminiAuthConfig {
        api_key: if mode == "apikey" { api_key } else { None },
        mode,
    }
}

/// Mask API key for logging: show first 6 and last 4 chars.
/// Uses char-based iteration to avoid panic on non-ASCII keys.
fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 12 {
        "***".to_string()
    } else {
        let prefix: String = chars[..6].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("{}...{}", prefix, suffix)
    }
}

/// Build an apikey-only config for fallback.
fn resolve_apikey_config() -> Option<GeminiAuthConfig> {
    let llm_yaml = missiond_core::default_mission_home().join("llm.yaml");
    let content = std::fs::read_to_string(&llm_yaml).ok()?;
    let config: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let key = config.get("gemini_api_key").and_then(|v| v.as_str())?.to_string();
    Some(GeminiAuthConfig { mode: "apikey".to_string(), api_key: Some(key) })
}

/// Create/maintain a shadow home directory for Gemini CLI auth isolation.
///
/// Gemini CLI resolves config via `GEMINI_CLI_HOME` env var → `$HOME/.gemini/`.
/// The GEMINI_API_KEY env var does NOT override settings.json's selectedType,
/// so both auth modes need a dedicated shadow home with the correct selectedType.
///
/// `channel`: "google" or "apikey" — determines directory name
/// `selected_type`: "oauth-personal" or "gemini-api-key" — written to settings.json
/// `symlink_oauth`: if true, symlinks OAuth credential files from ~/.gemini/
///
/// Idempotent and safe under concurrent calls:
/// - Atomic rename for settings.json prevents corruption
/// - AlreadyExists ignored on symlinks prevents TOCTOU races
fn ensure_auth_home(channel: &str, selected_type: &str, symlink_oauth: bool) -> Result<String> {
    let mission_home = missiond_core::default_mission_home();
    let home = mission_home.join(format!("gemini-{}-home", channel));
    let shadow_gemini_dir = home.join(".gemini");

    std::fs::create_dir_all(&shadow_gemini_dir)
        .map_err(|e| anyhow!("Failed to create {} auth home: {}", channel, e))?;

    let real_gemini_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot resolve home directory"))?
        .join(".gemini");

    // Atomic write: write to PID-suffixed temp file, then rename to avoid corruption
    let settings_path = shadow_gemini_dir.join("settings.json");
    let tmp_path = shadow_gemini_dir.join(format!("settings.json.{}.tmp", std::process::id()));
    let real_settings = std::fs::read_to_string(real_gemini_dir.join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

    let mut settings = real_settings.unwrap_or_else(|| serde_json::json!({}));
    if settings.get("security").is_none() {
        settings["security"] = serde_json::json!({});
    }
    if settings["security"].get("auth").is_none() {
        settings["security"]["auth"] = serde_json::json!({});
    }
    settings["security"]["auth"]["selectedType"] = serde_json::json!(selected_type);

    std::fs::write(&tmp_path, serde_json::to_string_pretty(&settings)?)
        .map_err(|e| anyhow!("Failed to write shadow settings.json: {}", e))?;
    std::fs::rename(&tmp_path, &settings_path)
        .map_err(|e| anyhow!("Failed to atomically place shadow settings.json: {}", e))?;

    // Symlink OAuth credential files (only needed for google mode)
    if symlink_oauth {
        for file in &["oauth_creds.json", "google_accounts.json", "google_account_id"] {
            let src = real_gemini_dir.join(file);
            let dst = shadow_gemini_dir.join(file);
            if src.exists() {
                #[cfg(unix)]
                match std::os::unix::fs::symlink(&src, &dst) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(anyhow!("Failed to symlink {}: {}", file, e)),
                }
                #[cfg(not(unix))]
                if !dst.exists() {
                    std::fs::copy(&src, &dst)
                        .map_err(|e| anyhow!("Failed to copy {}: {}", file, e))?;
                }
            }
        }
    }

    Ok(home.to_string_lossy().to_string())
}

/// Detect OAuth/auth-related errors from exit code and stderr.
///
/// Gemini CLI exits with code 41 for auth failures. We also check stderr for
/// specific auth-related phrases, but use anchored/specific patterns to avoid
/// false positives (e.g. "authentication" appearing in normal output).
fn is_auth_error(exit_code: Option<i32>, stderr: &str) -> bool {
    // Exit code 41 is Gemini CLI's definitive auth failure signal
    if exit_code == Some(41) {
        return true;
    }
    let lower = stderr.to_lowercase();
    lower.contains("token expired")
        || lower.contains("auth required")
        || lower.contains("authentication failed")
        || lower.contains("opening authentication page")
        || lower.contains("do you want to continue")
}

/// Detect quota/rate-limit exhaustion from exit code and stderr.
/// Gemini CLI prints "TerminalQuotaError" or "exhausted your daily quota" on quota errors.
fn is_quota_exhausted(_exit_code: Option<i32>, stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("terminalquotaerror")
        || lower.contains("exhausted your daily quota")
        || lower.contains("exhausted your capacity")
        || lower.contains("resource_exhausted")
}

/// Read API key entries from llm.yaml. Supports both formats:
/// - String array (legacy): `["AIza...", "AIzb..."]` → no alias, no default
/// - Object array (new):    `[{key: "AIza...", alias: "foo", default: true}]`
/// Mixed formats allowed in the same array for backward compatibility.
/// Also merges singular `gemini_api_key` if not already present.
pub(crate) fn resolve_apikey_entries() -> Vec<ApiKeyEntry> {
    let llm_yaml = missiond_core::default_mission_home().join("llm.yaml");
    let content = match std::fs::read_to_string(&llm_yaml) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let config: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();

    // Read plural key array (supports mixed string/object items)
    if let Some(arr) = config.get("gemini_api_keys").and_then(|v| v.as_sequence()) {
        for item in arr {
            if let Some(k) = item.as_str() {
                // Legacy string format
                if !k.is_empty() && seen_keys.insert(k.to_string()) {
                    entries.push(ApiKeyEntry { key: k.to_string(), alias: None, is_default: false });
                }
            } else if let Some(k) = item.get("key").and_then(|v| v.as_str()) {
                // Object format: {key, alias?, default?}
                if !k.is_empty() && seen_keys.insert(k.to_string()) {
                    let alias = item.get("alias").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let is_default = item.get("default").and_then(|v| v.as_bool()).unwrap_or(false);
                    entries.push(ApiKeyEntry { key: k.to_string(), alias, is_default });
                }
            }
        }
    }

    // Merge singular key if not already present
    if let Some(k) = config.get("gemini_api_key").and_then(|v| v.as_str()) {
        if !k.is_empty() && seen_keys.insert(k.to_string()) {
            entries.push(ApiKeyEntry { key: k.to_string(), alias: None, is_default: false });
        }
    }

    entries
}

/// Backward-compat wrapper: returns plain key list for startup logging.
pub(crate) fn resolve_apikey_pool() -> Vec<String> {
    resolve_apikey_entries().into_iter().map(|e| e.key).collect()
}
