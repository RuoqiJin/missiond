//! Handler for mission_minimax_* MCP tools.
//! Routes through MinimaxGateway (P0: interactive priority) for unified rate limiting.

use anyhow::Result;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;
use tracing::info;

use crate::minimax_client::ChatMessage;
use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_minimax_process" => handle_process(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown minimax tool: {}", name))),
    }
}

async fn handle_process(state: &AppState, args: Value) -> Result<ToolResult> {
    let minimax = match state.minimax.as_ref() {
        Some(m) => m,
        None => return Ok(ToolResult::error("MiniMax gateway not available (API key not configured)")),
    };

    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("summarize");

    if text.is_empty() {
        return Ok(ToolResult::error("'text' is required"));
    }

    let result = match task {
        "summarize" => {
            let max_chars = args.get("maxChars").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            let prompt = format!(
                "请用不超过{}字简洁总结以下内容的核心要点。直接输出总结，不要前缀。\n\n{}",
                max_chars, text
            );
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }];
            minimax.call_interactive(messages, Some(500), "minimax_process").await
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
            minimax.call_interactive(messages, None, "minimax_process").await
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
            minimax.call_interactive(messages, None, "minimax_process").await
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
