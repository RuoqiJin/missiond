//! LLM Gateway — infrastructure client for Gemini API calls.
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

/// Dynamic LLM model selector based on task characteristics and slot role.
/// Returns env var overrides that get merged into PTY spawn environment.
pub(crate) fn determine_llm_env(task: &missiond_core::types::BoardTask, slot_role: &str) -> HashMap<String, String> {
    let mut envs = HashMap::new();

    // Coder slots always use Opus for best coding quality
    if slot_role == "coder" {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-opus-4-6".to_string());
        return envs;
    }

    // Rule 1: urgent priority / ops → Opus
    if task.priority == "urgent" || task.category == "ops" {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-opus-4-6".to_string());
    }
    // Rule 2: docs / test / chore → fast & cheap Haiku
    else if task.category == "docs" || task.category == "test" || task.category == "chore" {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-haiku-4-5-20251001".to_string());
    }
    // Rule 3: very long description (>2000 chars) → complex context, upgrade
    else if task.description.len() > 2000 {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-sonnet-4-6".to_string());
    }
    // Default: sonnet (balanced cost/performance)
    else {
        envs.insert("ANTHROPIC_MODEL".to_string(), "claude-sonnet-4-6".to_string());
    }

    envs
}
