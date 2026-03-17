//! Gemini File API client for multimodal uploads (video, image, PDF).
//!
//! Uploads files to Google's Gemini File API, polls until ACTIVE,
//! caches results in SQLite to avoid re-uploading unchanged files.
//!
//! Requires a Gemini API key (set `gemini_api_key` in llm.yaml).
//! Get one from: https://aistudio.google.com/app/apikey

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com";
/// Max inline base64 size (4 MB) — larger files use File API upload.
const INLINE_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// File API: poll interval while waiting for ACTIVE state.
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// File API: max wait time for file processing.
const POLL_TIMEOUT: Duration = Duration::from_secs(300); // 5 min

/// MIME type detection by file extension.
pub(crate) fn detect_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "mp4" => "video/mp4",
        "mov" | "qt" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

/// Compute SHA-256 hash of file content (hex string).
pub(crate) fn file_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Gemini File API client.
pub(crate) struct GeminiFileApi {
    api_key: String,
    http: reqwest::Client,
}

/// Result of preparing a file for Gemini — either inline or via File API.
pub(crate) enum PreparedFile {
    /// Small file: base64 inline data (< 4MB).
    Inline {
        mime_type: String,
        base64_data: String,
    },
    /// Large file: uploaded via File API, reference by URI.
    FileRef {
        mime_type: String,
        file_uri: String,
    },
}

impl GeminiFileApi {
    pub fn new(api_key: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(600)) // 10 min for large uploads
            .build()
            .expect("Failed to build HTTP client");
        Self { api_key, http }
    }

    /// Prepare a binary file for Gemini: either inline base64 or File API upload.
    /// Checks cache first to avoid re-uploading.
    pub async fn prepare_file(
        &self,
        path: &Path,
        store: &dyn missiond_core::db::traits::ObservabilityStore,
    ) -> Result<PreparedFile> {
        let data = tokio::fs::read(path).await
            .map_err(|e| anyhow!("Failed to read '{}': {}", path.display(), e))?;

        let mime_type = detect_mime(path).to_string();
        let hash = file_hash(&data);

        // Small files (< 4MB non-video): inline base64
        if data.len() < INLINE_MAX_BYTES as usize && !mime_type.starts_with("video/") {
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
            info!(path = %path.display(), mime = %mime_type, size = data.len(), "Gemini: inline base64");
            return Ok(PreparedFile::Inline {
                mime_type,
                base64_data: b64,
            });
        }

        // Check cache
        if let Some(uri) = store.gemini_file_cache_get(&hash).await
            .map_err(|e| anyhow!("DB error: {}", e))? {
            info!(hash = %hash, uri = %uri, "Gemini File API: cache hit");
            return Ok(PreparedFile::FileRef { mime_type, file_uri: uri });
        }

        // Upload via File API
        let file_uri = self.upload_file(&data, &mime_type, path).await?;

        // Cache the result (40h TTL, File API keeps files for 48h)
        let expires_at = chrono::Utc::now().timestamp() + 40 * 3600;
        store.gemini_file_cache_put(&hash, &file_uri, &mime_type, expires_at).await
            .map_err(|e| anyhow!("DB error: {}", e))?;

        Ok(PreparedFile::FileRef { mime_type, file_uri })
    }

    /// Upload file to Gemini File API and poll until ACTIVE.
    async fn upload_file(&self, data: &[u8], mime_type: &str, path: &Path) -> Result<String> {
        let display_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("upload")
            .to_string();

        info!(
            path = %path.display(),
            mime = %mime_type,
            size_mb = format!("{:.1}", data.len() as f64 / 1_048_576.0),
            "Gemini File API: uploading"
        );

        // Step 1: Initiate resumable upload
        let init_url = format!(
            "{}/upload/v1beta/files?key={}",
            GEMINI_API_BASE, self.api_key
        );

        let metadata = serde_json::json!({
            "file": { "displayName": display_name }
        });

        let resp = self.http.post(&init_url)
            .header("X-Goog-Upload-Protocol", "resumable")
            .header("X-Goog-Upload-Command", "start")
            .header("X-Goog-Upload-Header-Content-Length", data.len().to_string())
            .header("X-Goog-Upload-Header-Content-Type", mime_type)
            .header("Content-Type", "application/json")
            .body(metadata.to_string())
            .send()
            .await
            .map_err(|e| anyhow!("File API init failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("File API init returned {}: {}", status, body));
        }

        let upload_url = resp.headers()
            .get("x-goog-upload-url")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow!("File API: missing upload URL in response headers"))?
            .to_string();

        // Step 2: Upload the actual data
        let resp = self.http.put(&upload_url)
            .header("X-Goog-Upload-Command", "upload, finalize")
            .header("X-Goog-Upload-Offset", "0")
            .header("Content-Type", mime_type)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| anyhow!("File API upload failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("File API upload returned {}: {}", status, body));
        }

        let result: Value = resp.json().await
            .map_err(|e| anyhow!("File API: invalid JSON response: {}", e))?;

        let file_name = result.pointer("/file/name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("File API: missing file.name in response: {}", result))?
            .to_string();
        let file_uri = result.pointer("/file/uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("File API: missing file.uri in response: {}", result))?
            .to_string();
        let state = result.pointer("/file/state")
            .and_then(|v| v.as_str())
            .unwrap_or("PROCESSING");

        info!(file_name = %file_name, state = %state, "Gemini File API: upload complete");

        // Step 3: Poll until ACTIVE (video processing can take minutes)
        if state != "ACTIVE" {
            self.poll_until_active(&file_name).await?;
        }

        Ok(file_uri)
    }

    /// Poll file status until ACTIVE or timeout.
    async fn poll_until_active(&self, file_name: &str) -> Result<()> {
        let url = format!(
            "{}/v1beta/{}?key={}",
            GEMINI_API_BASE, file_name, self.api_key
        );
        let deadline = tokio::time::Instant::now() + POLL_TIMEOUT;

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(anyhow!("File API: timeout waiting for ACTIVE state ({})", file_name));
            }

            tokio::time::sleep(POLL_INTERVAL).await;

            let resp = self.http.get(&url).send().await
                .map_err(|e| anyhow!("File API poll failed: {}", e))?;

            if !resp.status().is_success() {
                warn!(file = %file_name, status = %resp.status(), "File API poll: non-200");
                continue;
            }

            let result: Value = resp.json().await.unwrap_or_default();
            let state = result.get("state").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");

            match state {
                "ACTIVE" => {
                    info!(file = %file_name, "Gemini File API: file is ACTIVE");
                    return Ok(());
                }
                "FAILED" => {
                    let error = result.get("error").cloned().unwrap_or_default();
                    return Err(anyhow!("File API: processing FAILED for {}: {}", file_name, error));
                }
                _ => {
                    info!(file = %file_name, state = %state, "Gemini File API: still processing...");
                }
            }
        }
    }

    /// Call Gemini generateContent API directly with multimodal parts.
    /// Used when files need to bypass Router/CLI (video, images).
    pub async fn generate_content(
        &self,
        model: &str,
        text_prompt: &str,
        files: &[PreparedFile],
        max_tokens: u32,
    ) -> Result<String> {
        let mut parts: Vec<Value> = Vec::new();

        // Add file parts first
        for file in files {
            match file {
                PreparedFile::Inline { mime_type, base64_data } => {
                    parts.push(serde_json::json!({
                        "inlineData": {
                            "mimeType": mime_type,
                            "data": base64_data,
                        }
                    }));
                }
                PreparedFile::FileRef { mime_type, file_uri } => {
                    parts.push(serde_json::json!({
                        "fileData": {
                            "mimeType": mime_type,
                            "fileUri": file_uri,
                        }
                    }));
                }
            }
        }

        // Add text prompt last
        parts.push(serde_json::json!({ "text": text_prompt }));

        let body = serde_json::json!({
            "contents": [{ "parts": parts }],
            "generationConfig": {
                "maxOutputTokens": max_tokens,
            }
        });

        // Use the actual Gemini model name (strip any router aliases)
        let gemini_model = if model.starts_with("gemini-") {
            model.to_string()
        } else {
            "gemini-2.5-flash".to_string()
        };

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            GEMINI_API_BASE, gemini_model, self.api_key
        );

        info!(model = %gemini_model, parts = parts.len(), "Gemini File API: generateContent");

        let resp = self.http.post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("Gemini generateContent failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Gemini generateContent returned {}: {}", status, err_body));
        }

        let result: Value = resp.json().await
            .map_err(|e| anyhow!("Gemini generateContent: invalid JSON: {}", e))?;

        // Extract text from response
        let content = result.pointer("/candidates/0/content/parts/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("(empty response)");

        Ok(content.to_string())
    }
}
