//! Handler for mission_minimax_process / mission_sonnet_process MCP tools.
//! Routes through SonnetGateway (P0: interactive priority) for unified rate limiting.
//! Legacy name `mission_minimax_process` preserved for backward compatibility.

use anyhow::Result;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;
use tracing::info;

use crate::minimax_client::ChatMessage;
use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_minimax_process" | "mission_sonnet_process" => handle_process(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown tool: {}", name))),
    }
}

async fn handle_process(state: &AppState, args: Value) -> Result<ToolResult> {
    let sonnet = match state.sonnet.as_ref() {
        Some(s) => s,
        None => return Ok(ToolResult::error("Sonnet gateway not available")),
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
            sonnet.call_interactive(messages, Some(500), "sonnet_process").await
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
            sonnet.call_interactive(messages, None, "sonnet_process").await
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
            sonnet.call_interactive(messages, None, "sonnet_process").await
        }
        _ => return Ok(ToolResult::error(format!("Unknown task type: {}", task))),
    };

    match result {
        Ok(content) => {
            info!(task, input_chars = text.len(), output_chars = content.len(), "Sonnet process completed");
            Ok(ToolResult::text(content))
        }
        Err(e) => Ok(ToolResult::error(format!("Sonnet API error: {}", e))),
    }
}
