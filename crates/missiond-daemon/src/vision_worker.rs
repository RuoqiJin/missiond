//! Vision Worker — async image understanding pipeline.
//!
//! Periodically scans for messages with unprocessed image placeholders,
//! sends to Gemini Vision for description, and updates message content in-place.
//! SHA-256 hash dedup ensures each unique image is only processed once.

use std::sync::Arc;
use anyhow::Result;
use sha2::{Sha256, Digest};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::embedding_worker::resolve_llm_credentials;
use crate::gemini_client::{REQUEST_CALLER, REQUEST_SESSION_ID};
use crate::state::AppState;

const VISION_PROMPT: &str = "你是一个资深研发工程师。请详细解析这张截图。\n\
    它可能是：UI界面、错误日志、终端输出、代码片段、架构图、配置文件等。\n\n\
    请提取：\n\
    1. 所有可见的关键文本（错误信息、路径、命令、配置值）\n\
    2. 界面/状态描述（哪个应用、什么页面、什么状态）\n\
    3. 核心意图总结（用户为什么发这张图）\n\n\
    用中文回答，简洁精确，不超过 800 字。";

/// Max description length (chars) to prevent context bloat
const MAX_DESC_CHARS: usize = 1500;

/// Poll interval when there are no pending images
const IDLE_INTERVAL_SECS: u64 = 60;

/// Batch size per poll cycle
const BATCH_SIZE: usize = 5;

/// Process a single message: extract images, get descriptions, update content.
async fn process_message(state: &AppState, message_id: i64, session_id: &str) -> Result<bool> {
    let db = state.mission.db();

    // 1. Get raw_content
    let raw_content = match db.get_message_raw_content(message_id)? {
        Some(rc) => rc,
        None => return Ok(false),
    };

    // 2. Parse content blocks
    let raw: Value = serde_json::from_str(&raw_content).unwrap_or(Value::String(raw_content));

    let blocks = match &raw {
        Value::Array(arr) => arr.clone(),
        Value::Object(obj) => {
            obj.get("content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default()
        }
        _ => return Ok(false),
    };

    if blocks.is_empty() {
        return Ok(false);
    }

    // 3. Rebuild content with image descriptions
    let mut new_parts: Vec<String> = Vec::new();
    let mut had_image = false;

    for block in &blocks {
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    new_parts.push(text.to_string());
                }
            }
            "image" => {
                had_image = true;
                let media_type = block
                    .pointer("/source/media_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image/png");
                let base64_data = block.pointer("/source/data").and_then(|v| v.as_str());

                if let Some(data) = base64_data {
                    let hash = format!("{:x}", Sha256::digest(data.as_bytes()));

                    // Check cache first
                    let description = if let Ok(Some(cached)) = db.get_image_description(&hash) {
                        debug!(hash = %&hash[..12], "Vision: cache hit");
                        cached
                    } else {
                        match call_gemini_vision(state, data, media_type).await {
                            Ok(desc) => {
                                let desc = truncate_description(&desc);
                                let _ = db.save_image_description(&hash, media_type, &desc);
                                info!(hash = %&hash[..12], chars = desc.len(), "Vision: described image");
                                desc
                            }
                            Err(e) => {
                                warn!(error = %e, message_id, "Vision: Gemini call failed");
                                new_parts.push(format!("[图片: {}]", media_type));
                                continue;
                            }
                        }
                    };

                    new_parts.push(format!("[图片(AI解析): {}]", description));
                } else {
                    new_parts.push(format!("[图片: {}]", media_type));
                }
            }
            _ => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    new_parts.push(text.to_string());
                }
            }
        }
    }

    if !had_image {
        return Ok(false);
    }

    // 4. Update content column
    let new_content = new_parts.join("\n");
    db.update_message_content(message_id, &new_content)?;
    debug!(message_id, session = %session_id, "Vision: content updated");

    Ok(true)
}

/// Call Gemini Vision API to describe an image.
async fn call_gemini_vision(state: &AppState, base64_data: &str, media_type: &str) -> Result<String> {
    let (base_url, jwt) = resolve_llm_credentials().await?;
    let url = format!("{}/v1/chat/completions", base_url);

    let body = serde_json::json!({
        "model": "gemini-2.0-flash",
        "max_tokens": 2048,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": VISION_PROMPT },
                {
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", media_type, base64_data)
                    }
                }
            ]
        }]
    });

    REQUEST_CALLER.scope("vision_worker".to_string(), async {
        REQUEST_SESSION_ID.scope("vision".to_string(), async {
            let resp = state.gemini.send(&state.http_client, &url, &jwt, &body).await?;
            let text = resp
                .pointer("/choices/0/message/content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if text.is_empty() {
                anyhow::bail!("Empty response from Gemini Vision");
            }
            Ok(text)
        }).await
    }).await
}

fn truncate_description(desc: &str) -> String {
    if desc.chars().count() <= MAX_DESC_CHARS {
        desc.to_string()
    } else {
        let truncated: String = desc.chars().take(MAX_DESC_CHARS).collect();
        format!("{}…", truncated)
    }
}

/// Spawn the vision worker as a periodic background task.
pub(crate) fn spawn_vision_worker(state: Arc<AppState>) {
    tokio::spawn(async move {
        info!("Vision worker started (poll interval: {}s)", IDLE_INTERVAL_SECS);
        loop {
            // Find unprocessed image messages
            let pending = match state.mission.db().find_unprocessed_image_messages(BATCH_SIZE) {
                Ok(p) => p,
                Err(e) => {
                    warn!(error = %e, "Vision worker: DB query failed");
                    tokio::time::sleep(std::time::Duration::from_secs(IDLE_INTERVAL_SECS)).await;
                    continue;
                }
            };

            if pending.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(IDLE_INTERVAL_SECS)).await;
                continue;
            }

            let batch_size = pending.len();
            let mut processed = 0;
            for (msg_id, session_id) in pending {
                match process_message(&state, msg_id, &session_id).await {
                    Ok(true) => processed += 1,
                    Ok(false) => {} // no image found / skip
                    Err(e) => {
                        warn!(message_id = msg_id, error = %e, "Vision worker: failed");
                    }
                }
                // Rate limit between Gemini calls
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }

            if processed > 0 {
                info!(processed, batch_size, "Vision worker: batch completed");
            }

            // Short sleep between batches if there were items (might be more)
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}
