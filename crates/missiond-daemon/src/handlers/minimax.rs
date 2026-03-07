//! Handler for mission_minimax_* MCP tools.

use anyhow::Result;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;
use tracing::info;

use crate::minimax_client::{self, ChatMessage, MiniMaxClient};
use crate::state::AppState;

/// Lazily initialize MiniMax client (cached in a static OnceLock).
fn get_client() -> Option<&'static MiniMaxClient> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<Option<MiniMaxClient>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        minimax_client::load_api_key().map(MiniMaxClient::new)
    }).as_ref()
}

pub(crate) async fn handle(_state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_minimax_process" => handle_process(args).await,
        _ => Ok(ToolResult::error(format!("Unknown minimax tool: {}", name))),
    }
}

async fn handle_process(args: Value) -> Result<ToolResult> {
    let client = match get_client() {
        Some(c) => c,
        None => return Ok(ToolResult::error("MiniMax API key not configured (xjp secret get minimax/api-key-domestic)")),
    };

    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("summarize");

    if text.is_empty() {
        return Ok(ToolResult::error("'text' is required"));
    }

    let result = match task {
        "summarize" => {
            let max_chars = args.get("maxChars").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            client.summarize(text, max_chars).await
        }
        "translate" => {
            let target_lang = args.get("targetLang").and_then(|v| v.as_str()).unwrap_or("en");
            let prompt = format!(
                "请将以下内容翻译成{}。直接输出翻译结果，不要前缀或解释。\n\n{}",
                target_lang, text
            );
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }];
            client.chat(&messages, None).await
        }
        "custom" => {
            let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
            if prompt.is_empty() {
                return Ok(ToolResult::error("'prompt' is required for task=custom"));
            }
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: format!("{}\n\n{}", prompt, text),
            }];
            client.chat(&messages, None).await
        }
        _ => return Ok(ToolResult::error(format!("Unknown task type: {}", task))),
    };

    match result {
        Ok(content) => {
            info!(task, input_chars = text.len(), output_chars = content.len(), "MiniMax process completed");
            Ok(ToolResult::text(content))
        }
        Err(e) => Ok(ToolResult::error(format!("MiniMax API error: {}", e))),
    }
}
