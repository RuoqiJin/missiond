//! LLM Gateway — infrastructure client for multi-model API calls.
//!
//! Pure infrastructure layer (anti-corruption boundary). Accepts pre-formatted
//! prompts and returns raw strings. No knowledge of Tasks, Decisions, or other
//! MissionD business concepts.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tracing::{info, warn};

use crate::embedding_worker::resolve_llm_credentials;
use crate::gemini_client::REQUEST_CALLER;
use crate::state::AppState;

/// Call Gemini via the router API for flow daemon phases.
/// Uses the same HTTP client + credentials as mission_router_chat.
pub(crate) async fn call_gemini_for_flow(
    state: &AppState,
    task_id: &str,
    prompt: &str,
) -> Result<String> {
    // Gate check removed: GeminiClient::send_with_timeout() is the sole choke point (P3)
    let (base_url, jwt) = resolve_llm_credentials().await?;
    let model = "gemini-3.1-pro";

    // Get or create conversation for this task (maintains Gemini context across phases)
    let conv_id = state
        .store
        .router_chat_get_or_create(task_id, model)
        .await
        .map_err(|e| anyhow!("Failed to get/create router chat conversation: {}", e))?;

    // Rolling summary + active history (avoids sending unbounded full history)
    let (summary_opt, cursor) = state
        .store
        .router_chat_get_summary(&conv_id)
        .await
        .unwrap_or((None, 0));
    let active_history = state
        .store
        .router_chat_load_active_history(&conv_id, cursor)
        .await
        .unwrap_or_default();

    let mut messages = Vec::new();
    if let Some(ref summary) = summary_opt {
        messages.push(serde_json::json!({
            "role": "system",
            "content": format!("[对话历史摘要] 以下是之前对话的核心要点：\n\n{}", summary)
        }));
    }
    let prepended_count = messages.len() + active_history.len();
    messages.extend(active_history);

    // Append current user message
    messages.push(serde_json::json!({"role": "user", "content": prompt}));

    let url = format!("{}/v1/chat/completions", base_url);
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": 16384,
    });

    info!(task_id, conv_id = %conv_id, msg_count = messages.len(), "Flow engine: calling Gemini");

    let result = REQUEST_CALLER
        .scope("flow_engine".to_string(), async {
            state
                .gemini
                .send(&state.http_client, &url, &jwt, &body)
                .await
        })
        .await?;

    let content = result
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("(empty response)")
        .to_string();

    // Save only new messages (skip prepended summary + active history)
    let new_msgs: Vec<(String, String)> = messages
        .iter()
        .skip(prepended_count)
        .map(|m| {
            let role = m
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user")
                .to_string();
            let msg_content = m
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (role, msg_content)
        })
        .chain(std::iter::once(("assistant".to_string(), content.clone())))
        .collect();

    if let Err(e) = state
        .store
        .router_chat_append_messages(&conv_id, &new_msgs)
        .await
    {
        warn!(conv_id = %conv_id, error = %e, "Flow engine: failed to save Gemini chat history");
    }

    Ok(content)
}

/// Stateless Sonnet call via Router API (claude-sonnet endpoint).
/// Direct HTTP POST — bypasses GeminiClient to avoid CLI mode hijacking.
/// Retries on 429 with exponential backoff (max 3 attempts).
pub(crate) async fn call_sonnet_stateless(
    state: &AppState,
    system: &str,
    user_msg: &str,
    max_tokens: u32,
    caller: &str,
) -> Result<String> {
    // Kill switch: reject immediately when sonnet gate is closed (AtomicBool — zero I/O)
    crate::llm_gate::check(crate::llm_gate::LlmProvider::Sonnet)?;
    let (base_url, mut jwt) = resolve_llm_credentials().await?;
    let model = "claude-sonnet";
    let url = format!("{}/v1/chat/completions", base_url);
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user_msg},
        ],
        "max_tokens": max_tokens,
    });

    info!(
        caller,
        model,
        user_len = user_msg.len(),
        "LLM Gateway: calling Sonnet (direct HTTP)"
    );

    let mut last_err = None;
    for attempt in 0..3u32 {
        // Refresh JWT on retries in case token expires during backoff
        if attempt > 0 {
            jwt = resolve_llm_credentials().await?.1;
        }

        let resp = state
            .http_client
            .post(&url)
            .bearer_auth(&jwt)
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await;

        match resp {
            Ok(r)
                if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || r.status().is_server_error() =>
            {
                let wait = std::time::Duration::from_secs(2u64.pow(attempt) * 5);
                warn!(caller, attempt, status = %r.status(), "Sonnet retryable error, backing off {:?}", wait);
                tokio::time::sleep(wait).await;
                last_err = Some(anyhow!("{} (attempt {})", r.status(), attempt));
                continue;
            }
            Ok(r) if r.status().is_success() => {
                let result: serde_json::Value = r
                    .json()
                    .await
                    .map_err(|e| anyhow!("Sonnet response parse error: {}", e))?;
                let content = result
                    .pointer("/choices/0/message/content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if content.is_empty() {
                    return Err(anyhow!("Sonnet returned empty content"));
                }
                info!(
                    caller,
                    response_len = content.len(),
                    "LLM Gateway: Sonnet OK"
                );
                return Ok(content);
            }
            Ok(r) => {
                let status = r.status();
                let body_text = r.text().await.unwrap_or_default();
                let preview_end = body_text
                    .char_indices()
                    .nth(200)
                    .map(|(i, _)| i)
                    .unwrap_or(body_text.len());
                return Err(anyhow!(
                    "Sonnet HTTP {}: {}",
                    status,
                    &body_text[..preview_end]
                ));
            }
            Err(e) if e.is_timeout() || e.is_connect() => {
                let wait = std::time::Duration::from_secs(2u64.pow(attempt) * 5);
                warn!(caller, attempt, error = %e, "Sonnet network error, backing off {:?}", wait);
                tokio::time::sleep(wait).await;
                last_err = Some(anyhow!("network: {} (attempt {})", e, attempt));
                continue;
            }
            Err(e) => {
                return Err(anyhow!("Sonnet request failed: {}", e));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("Sonnet call exhausted retries")))
}

/// Dynamic LLM model selector based on task characteristics and slot role.
/// Returns env var overrides that get merged into PTY spawn environment.
///
/// NOTE: Does NOT blindly override to Opus for coder slots. The slot's own
/// `model` config in slots.yaml is respected (applied at PTY spawn). This
/// function only sets task-type-based routing. If no override is set here,
/// the slot uses its configured model (or Claude Code's default).
pub(crate) fn determine_llm_env(
    task: &missiond_core::types::BoardTask,
    _slot_role: &str,
) -> HashMap<String, String> {
    let mut envs = HashMap::new();

    // Rule 1: urgent priority → Opus (critical tasks need best quality)
    if task.priority == "urgent" {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-opus-4-6".to_string());
    }
    // Rule 2: ops category → Sonnet (balanced for remediation tasks)
    else if task.category == "ops" {
        envs.insert(
            "ANTHROPIC_MODEL".to_string(),
            "claude-sonnet-4-6".to_string(),
        );
    }
    // Rule 3: docs / test / chore → fast & cheap Haiku
    else if task.category == "docs" || task.category == "test" || task.category == "chore" {
        envs.insert(
            "ANTHROPIC_MODEL".to_string(),
            "claude-haiku-4-5-20251001".to_string(),
        );
    }
    // Default: no override — let slot's own model config apply
    // (slots.yaml `model` field or Claude Code's default)

    envs
}
