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
pub(crate) async fn call_gemini_for_flow(state: &AppState, task_id: &str, prompt: &str) -> Result<String> {
    let (base_url, jwt) = resolve_llm_credentials().await?;
    let model = "gemini-3.1-pro";

    // Get or create conversation for this task (maintains Gemini context across phases)
    let conv_id = state.mission.db().router_chat_get_or_create(task_id, model)
        .map_err(|e| anyhow!("Failed to get/create router chat conversation: {}", e))?;

    // Load history for context continuity
    let mut messages = state.mission.db().router_chat_load_history(&conv_id)
        .unwrap_or_default();
    let history_count = messages.len();

    // Append current user message
    messages.push(serde_json::json!({"role": "user", "content": prompt}));

    let url = format!("{}/v1/chat/completions", base_url);
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": 16384,
    });

    info!(task_id, conv_id = %conv_id, msg_count = messages.len(), "Flow engine: calling Gemini");

    let result = REQUEST_CALLER.scope("flow_engine".to_string(), async {
        state.gemini.send(&state.http_client, &url, &jwt, &body).await
    }).await?;

    let content = result
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("(empty response)")
        .to_string();

    // Save messages to conversation history
    let new_msgs: Vec<(String, String)> = messages.iter().skip(history_count)
        .map(|m| {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user").to_string();
            let msg_content = m.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            (role, msg_content)
        })
        .chain(std::iter::once(("assistant".to_string(), content.clone())))
        .collect();

    if let Err(e) = state.mission.db().router_chat_append_messages(&conv_id, &new_msgs) {
        warn!(conv_id = %conv_id, error = %e, "Flow engine: failed to save Gemini chat history");
    }

    Ok(content)
}

/// Stateless Sonnet call via Router API (cpapi-claude-sonnet endpoint).
/// Direct HTTP POST — bypasses GeminiClient to avoid CLI mode hijacking.
/// Retries on 429 with exponential backoff (max 3 attempts).
pub(crate) async fn call_sonnet_stateless(
    state: &AppState,
    system: &str,
    user_msg: &str,
    max_tokens: u32,
    caller: &str,
) -> Result<String> {
    let (base_url, jwt) = resolve_llm_credentials().await?;
    let model = "cpapi-claude-sonnet";
    let url = format!("{}/v1/chat/completions", base_url);
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user_msg},
        ],
        "max_tokens": max_tokens,
    });

    info!(caller, model, user_len = user_msg.len(), "LLM Gateway: calling Sonnet (direct HTTP)");

    let mut last_err = None;
    for attempt in 0..3u32 {
        let resp = state.http_client
            .post(&url)
            .bearer_auth(&jwt)
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await;

        match resp {
            Ok(r) if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                let wait = std::time::Duration::from_secs(2u64.pow(attempt) * 5);
                warn!(caller, attempt, "Sonnet 429, backing off {:?}", wait);
                tokio::time::sleep(wait).await;
                last_err = Some(anyhow!("429 Too Many Requests"));
                continue;
            }
            Ok(r) if r.status().is_success() => {
                let result: serde_json::Value = r.json().await
                    .map_err(|e| anyhow!("Sonnet response parse error: {}", e))?;
                let content = result
                    .pointer("/choices/0/message/content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if content.is_empty() {
                    return Err(anyhow!("Sonnet returned empty content"));
                }
                info!(caller, response_len = content.len(), "LLM Gateway: Sonnet OK");
                return Ok(content);
            }
            Ok(r) => {
                let status = r.status();
                let body_text = r.text().await.unwrap_or_default();
                return Err(anyhow!("Sonnet HTTP {}: {}", status, &body_text[..body_text.len().min(200)]));
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
pub(crate) fn determine_llm_env(task: &missiond_core::types::BoardTask, _slot_role: &str) -> HashMap<String, String> {
    let mut envs = HashMap::new();

    // Rule 1: urgent priority → Opus (critical tasks need best quality)
    if task.priority == "urgent" {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-opus-4-6".to_string());
    }
    // Rule 2: ops category → Sonnet (balanced for remediation tasks)
    else if task.category == "ops" {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-sonnet-4-6".to_string());
    }
    // Rule 3: docs / test / chore → fast & cheap Haiku
    else if task.category == "docs" || task.category == "test" || task.category == "chore" {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-haiku-4-5-20251001".to_string());
    }
    // Default: no override — let slot's own model config apply
    // (slots.yaml `model` field or Claude Code's default)

    envs
}
