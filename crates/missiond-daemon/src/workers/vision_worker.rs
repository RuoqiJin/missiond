//! Vision Worker — async image understanding pipeline.
//!
//! Periodically scans for messages with unprocessed image placeholders,
//! calls Codex CLI (GPT-5.4 Vision) to describe images, and updates
//! message content in-place.
//!
//! Architecture: base64 images from raw_content are written to temp files,
//! passed to `codex exec --json -i <file>` for native vision support.
//! SHA-256 hash dedup ensures each unique image is only processed once.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use sha2::{Sha256, Digest};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::codex_cli::CodexCli;
use crate::state::AppState;

/// Max retry attempts per message before marking as permanently failed
const MAX_RETRIES: u32 = 3;

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

/// media_type → file extension
fn extension_for_media_type(media_type: &str) -> &str {
    match media_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    }
}

/// Persistent image cache directory.
fn image_cache_dir() -> std::path::PathBuf {
    let home = std::env::var("MISSIOND_HOME")
        .or_else(|_| std::env::var("XJP_MISSION_HOME"))
        .unwrap_or_else(|_| {
            let h = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            if h.join(".missiond").exists() {
                h.join(".missiond").to_string_lossy().to_string()
            } else {
                h.join(".xjp-mission").to_string_lossy().to_string()
            }
        });
    std::path::PathBuf::from(home).join("cache/images")
}

/// Write base64-encoded image to persistent cache, return (path, hash).
fn write_image_to_cache(base64_data: &str, media_type: &str) -> Result<(std::path::PathBuf, String)> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64_data)?;
    let hash = format!("{:x}", Sha256::digest(base64_data.as_bytes()));
    let ext = extension_for_media_type(media_type);
    let dir = image_cache_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.{}", hash, ext));
    if !path.exists() {
        std::fs::write(&path, &bytes)?;
        debug!(hash = %&hash[..12], "Vision: cached image to disk");
    }
    Ok((path, hash))
}

/// Process a single message: extract images, get descriptions, update content.
async fn process_message(codex: &CodexCli, state: &AppState, message_id: i64, session_id: &str) -> Result<bool> {
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
                        match call_codex_vision(codex, data, media_type, &hash).await {
                            Ok(desc) => {
                                let desc = truncate_description(&desc);
                                let _ = db.save_image_description(&hash, media_type, &desc);
                                info!(hash = %&hash[..12], chars = desc.len(), "Vision: described image");
                                desc
                            }
                            Err(e) => {
                                warn!(error = %e, message_id, "Vision: Codex call failed");
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

/// Call Codex CLI (GPT-5.4 Vision) to describe an image.
/// Writes image to persistent cache, passes via `-i` flag.
async fn call_codex_vision(codex: &CodexCli, base64_data: &str, media_type: &str, image_hash: &str) -> Result<String> {
    let (img_path, _) = write_image_to_cache(base64_data, media_type)?;

    let resp = codex.call(VISION_PROMPT, "vision_worker", None, Some(&img_path), None, Some(image_hash)).await?;
    if resp.content.is_empty() {
        anyhow::bail!("Empty response from Codex Vision");
    }
    Ok(resp.content)
}

fn truncate_description(desc: &str) -> String {
    if desc.chars().count() <= MAX_DESC_CHARS {
        desc.to_string()
    } else {
        let truncated: String = desc.chars().take(MAX_DESC_CHARS).collect();
        format!("{}…", truncated)
    }
}

/// Mark a message as permanently failed so it no longer matches the unprocessed query.
fn mark_vision_permanently_failed(state: &AppState, message_id: i64) {
    match state.mission.db().mark_vision_permanently_failed(message_id) {
        Ok(true) => info!(message_id, "Vision worker: marked as permanently failed"),
        Ok(false) => debug!(message_id, "Vision worker: nothing to mark failed"),
        Err(e) => warn!(message_id, error = %e, "Vision worker: failed to mark"),
    }
}

/// Spawn the vision worker as a periodic background task.
pub(crate) fn spawn_vision_worker(state: Arc<AppState>) {
    let codex = CodexCli::new(
        "codex".to_string(),
        "gpt-5.4".to_string(),
        Duration::from_secs(120),
        state.event_bus.sender(),
    );

    tokio::spawn(async move {
        info!("Vision worker started (codex/gpt-5.4, poll interval: {}s)", IDLE_INTERVAL_SECS);
        let mut attempt_counts: HashMap<i64, u32> = HashMap::new();

        loop {
            // Find unprocessed image messages
            let pending = match state.mission.db().find_unprocessed_image_messages(BATCH_SIZE) {
                Ok(p) => p,
                Err(e) => {
                    warn!(error = %e, "Vision worker: DB query failed");
                    tokio::time::sleep(Duration::from_secs(IDLE_INTERVAL_SECS)).await;
                    continue;
                }
            };

            if pending.is_empty() {
                attempt_counts.clear();
                tokio::time::sleep(Duration::from_secs(IDLE_INTERVAL_SECS)).await;
                continue;
            }

            let batch_size = pending.len();
            let mut processed = 0;
            for (msg_id, session_id) in pending {
                let attempts = attempt_counts.entry(msg_id).or_insert(0);
                *attempts += 1;

                if *attempts > MAX_RETRIES {
                    warn!(message_id = msg_id, attempts = *attempts, "Vision worker: max retries exceeded");
                    mark_vision_permanently_failed(&state, msg_id);
                    attempt_counts.remove(&msg_id);
                    continue;
                }

                match process_message(&codex, &state, msg_id, &session_id).await {
                    Ok(true) => processed += 1,
                    Ok(false) => {
                        // No image found — won't reappear, clean up
                        attempt_counts.remove(&msg_id);
                    }
                    Err(e) => {
                        warn!(message_id = msg_id, attempt = *attempts, error = %e, "Vision worker: failed");
                    }
                }
                // Rate limit between Codex calls
                tokio::time::sleep(Duration::from_millis(2000)).await;
            }

            if processed > 0 {
                info!(processed, batch_size, "Vision worker: batch completed");
            }

            // Short sleep between batches if there were items (might be more)
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}
