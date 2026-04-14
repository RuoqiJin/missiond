//! Gemini CLI session parser — reads JSON session files and converts to CCMessageLine.
//!
//! The key transformation: Gemini's per-message `thoughts`, `toolCalls`, and nested
//! content structures are flattened into the same CCMessageLine format used by the
//! Claude Code JSONL pipeline, enabling zero-modification reuse of the three-layer
//! ingestion pipeline (Ingest → Classify → Emit).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use tokio::fs;
use tracing::{debug, warn};

use super::types::*;
use crate::cc_tasks::{CCMessage, CCMessageLine, TokenUsage};

// ============ Project mapping ============

/// Bidirectional project map: hash → (name, path) and name → path.
#[derive(Debug, Clone, Default)]
pub struct ProjectMap {
    /// projectHash → (name, absolute_path)
    hash_to_project: HashMap<String, (String, String)>,
    /// project_name → absolute_path (from projects.json values → keys)
    name_to_path: HashMap<String, String>,
}

impl ProjectMap {
    /// Load from ~/.gemini/projects.json.
    pub async fn load(gemini_home: &Path) -> Self {
        let path = gemini_home.join("projects.json");
        let mut map = Self::default();
        let Ok(data) = fs::read_to_string(&path).await else {
            warn!("Cannot read {}", path.display());
            return map;
        };
        let Ok(file) = serde_json::from_str::<GeminiProjectsFile>(&data) else {
            warn!("Cannot parse {}", path.display());
            return map;
        };
        for (abs_path, name) in &file.projects {
            let hash = sha256_hex(abs_path);
            map.hash_to_project
                .insert(hash, (name.clone(), abs_path.clone()));
            map.name_to_path.insert(name.clone(), abs_path.clone());
        }
        debug!(
            count = map.hash_to_project.len(),
            "Loaded Gemini project map"
        );
        map
    }

    /// Resolve a directory name (either project name or hash) to an absolute project path.
    pub fn resolve_dir(&self, dir_name: &str) -> Option<String> {
        // Try as project name first
        if let Some(path) = self.name_to_path.get(dir_name) {
            return Some(path.clone());
        }
        // Try as project hash
        if let Some((_, path)) = self.hash_to_project.get(dir_name) {
            return Some(path.clone());
        }
        None
    }
}

fn sha256_hex(input: &str) -> String {
    use std::fmt::Write;
    let digest = <sha2::Sha256 as sha2::Digest>::digest(input.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

// ============ Session parsing ============

/// Parse a Gemini CLI session JSON file.
pub async fn parse_session(path: &Path) -> Option<GeminiSession> {
    let data = fs::read_to_string(path).await.ok()?;
    match serde_json::from_str::<GeminiSession>(&data) {
        Ok(session) => Some(session),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Failed to parse Gemini session");
            None
        }
    }
}

/// Extract only the message count from a session file (lightweight check).
pub async fn session_message_count(path: &Path) -> Option<usize> {
    let data = fs::read_to_string(path).await.ok()?;
    // Partial parse: only need messages array length
    #[derive(serde::Deserialize)]
    struct Partial {
        messages: Vec<serde_json::Value>,
    }
    let partial: Partial = serde_json::from_str(&data).ok()?;
    Some(partial.messages.len())
}

// ============ Message transformation ============

/// Convert a slice of new Gemini messages into CCMessageLine records.
///
/// Each GeminiMessage may expand into multiple CCMessageLines:
/// - thoughts → thinking role messages
/// - main content → user/assistant message
/// - toolCalls → tool_use + tool_result pairs
pub fn gemini_messages_to_cc(
    messages: &[GeminiMessage],
    session: &GeminiSession,
    project_path: &str,
) -> Vec<CCMessageLine> {
    let mut out = Vec::new();
    for msg in messages {
        convert_single_message(msg, session, project_path, &mut out);
    }
    out
}

fn convert_single_message(
    msg: &GeminiMessage,
    session: &GeminiSession,
    project_path: &str,
    out: &mut Vec<CCMessageLine>,
) {
    // Skip info messages with empty content
    if msg.msg_type == "info" {
        let text = extract_text_content(&msg.content);
        if text.is_empty() {
            return;
        }
        // Non-empty info → system message
        out.push(make_cc_line(
            &session.session_id,
            &msg.id,
            &msg.timestamp,
            "system",
            json!(text),
            None,
            None,
            project_path,
        ));
        return;
    }

    // 1. thoughts → independent thinking messages (before main content)
    if let Some(thoughts) = &msg.thoughts {
        for (i, t) in thoughts.iter().enumerate() {
            if t.description.is_empty() && t.subject.is_empty() {
                continue;
            }
            let thought_text = if t.subject.is_empty() {
                t.description.clone()
            } else {
                format!("[{}] {}", t.subject, t.description)
            };
            out.push(make_cc_line(
                &session.session_id,
                &format!("{}-thought-{}", msg.id, i),
                &t.timestamp,
                "thinking",
                json!(thought_text),
                msg.model.clone(),
                None,
                project_path,
            ));
        }
    }

    // 2. Main message
    let role = match msg.msg_type.as_str() {
        "user" => "user",
        "gemini" => "assistant",
        _ => "system",
    };
    let content = normalize_content(&msg.content);
    let usage = msg.tokens.as_ref().map(|t| TokenUsage {
        input_tokens: t.input,
        output_tokens: t.output,
        cache_read_input_tokens: t.cached,
        ..Default::default()
    });
    out.push(make_cc_line(
        &session.session_id,
        &msg.id,
        &msg.timestamp,
        role,
        content,
        msg.model.clone(),
        usage,
        project_path,
    ));

    // 3. toolCalls → tool_use + tool_result pairs
    if let Some(calls) = &msg.tool_calls {
        for call in calls {
            let call_ts = call.timestamp.as_deref().unwrap_or(&msg.timestamp);

            // tool_use: content is an array with a single tool_use block
            let tool_use_block = json!([{
                "type": "tool_use",
                "id": call.id,
                "name": call.name,
                "input": call.args,
            }]);
            out.push(make_cc_line(
                &session.session_id,
                &format!("{}-tool-use", call.id),
                call_ts,
                "assistant",
                tool_use_block,
                msg.model.clone(),
                None,
                project_path,
            ));

            // tool_result: extract response from nested functionResponse
            let result_content = call
                .result
                .first()
                .and_then(|r| r.function_response.as_ref())
                .map(|fr| &fr.response)
                .cloned()
                .unwrap_or(json!(null));

            let tool_result_block = json!([{
                "type": "tool_result",
                "tool_use_id": call.id,
                "content": result_content,
            }]);
            out.push(make_cc_line(
                &session.session_id,
                &format!("{}-tool-result", call.id),
                call_ts,
                "tool_result",
                tool_result_block,
                None,
                None,
                project_path,
            ));
        }
    }
}

/// Normalize Gemini content to a serde_json::Value suitable for CCMessage.content.
///
/// Gemini uses two formats:
/// - String: "plain text" → pass through as json string
/// - Array: [{"text": "..."}, {"inlineData": {...}}] → pass through (compatible with CC content blocks)
fn normalize_content(content: &serde_json::Value) -> serde_json::Value {
    match content {
        serde_json::Value::String(_) => content.clone(),
        serde_json::Value::Array(arr) => {
            // If it's a single-element array with just a "text" field, simplify to string
            if arr.len() == 1 {
                if let Some(text) = arr[0].get("text").and_then(|v| v.as_str()) {
                    return json!(text);
                }
            }
            content.clone()
        }
        _ => content.clone(),
    }
}

fn extract_text_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn make_cc_line(
    session_id: &str,
    uuid: &str,
    timestamp: &str,
    role: &str,
    content: serde_json::Value,
    model: Option<String>,
    usage: Option<TokenUsage>,
    cwd: &str,
) -> CCMessageLine {
    CCMessageLine {
        message_type: "message".to_string(),
        parent_uuid: None,
        is_sidechain: false,
        cwd: cwd.to_string(),
        session_id: session_id.to_string(),
        version: "gemini-cli".to_string(),
        git_branch: None,
        message: CCMessage {
            role: role.to_string(),
            content,
            model,
            usage,
        },
        uuid: uuid.to_string(),
        timestamp: timestamp.to_string(),
        todos: None,
    }
}

// ============ Directory scanning ============

/// Discover all Gemini CLI chat directories under ~/.gemini/tmp/.
/// Returns (dir_name, chats_dir_path) pairs.
pub async fn discover_chat_dirs(gemini_home: &Path) -> Vec<(String, PathBuf)> {
    let tmp_dir = gemini_home.join("tmp");
    let mut result = Vec::new();
    let Ok(mut entries) = fs::read_dir(&tmp_dir).await else {
        return result;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let chats_dir = path.join("chats");
        if chats_dir.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                result.push((name.to_string(), chats_dir));
            }
        }
    }
    result
}

/// List all session files in a chats directory, sorted by mtime ascending.
pub async fn list_session_files(chats_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(mut entries) = fs::read_dir(chats_dir).await else {
        return files;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("session-") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_content_string() {
        let content = json!("hello world");
        assert_eq!(normalize_content(&content), json!("hello world"));
    }

    #[test]
    fn test_normalize_content_single_text_array() {
        let content = json!([{"text": "hello"}]);
        assert_eq!(normalize_content(&content), json!("hello"));
    }

    #[test]
    fn test_normalize_content_multi_array() {
        let content = json!([{"text": "a"}, {"inlineData": {"data": "", "mimeType": "image/png"}}]);
        // Multi-element array stays as-is
        assert_eq!(normalize_content(&content), content);
    }

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex("/tmp/example-project");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_convert_user_message() {
        let session = GeminiSession {
            session_id: "test-session".into(),
            project_hash: "abc123".into(),
            start_time: "2026-01-01T00:00:00Z".into(),
            last_updated: "2026-01-01T00:00:00Z".into(),
            messages: vec![],
            kind: None,
        };
        let msg = GeminiMessage {
            id: "msg-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            msg_type: "user".into(),
            content: json!([{"text": "hello"}]),
            thoughts: None,
            tokens: None,
            model: None,
            tool_calls: None,
            display_content: None,
        };
        let lines = gemini_messages_to_cc(&[msg], &session, "/test");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message.role, "user");
        assert_eq!(lines[0].message.content, json!("hello"));
        assert_eq!(lines[0].session_id, "test-session");
    }

    #[test]
    fn test_convert_gemini_message_with_thoughts() {
        let session = GeminiSession {
            session_id: "test-session".into(),
            project_hash: "abc123".into(),
            start_time: "2026-01-01T00:00:00Z".into(),
            last_updated: "2026-01-01T00:00:00Z".into(),
            messages: vec![],
            kind: None,
        };
        let msg = GeminiMessage {
            id: "msg-2".into(),
            timestamp: "2026-01-01T00:01:00Z".into(),
            msg_type: "gemini".into(),
            content: json!("The answer is 42"),
            thoughts: Some(vec![
                GeminiThought {
                    subject: "Thinking".into(),
                    description: "I need to compute".into(),
                    timestamp: "2026-01-01T00:00:50Z".into(),
                },
            ]),
            tokens: Some(GeminiTokens {
                input: 100,
                output: 50,
                cached: 10,
                thoughts: 30,
                tool: 0,
                total: 190,
            }),
            model: Some("gemini-3.1-pro-preview".into()),
            tool_calls: None,
            display_content: None,
        };
        let lines = gemini_messages_to_cc(&[msg], &session, "/test");
        // 1 thought + 1 main = 2 lines
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].message.role, "thinking");
        assert!(lines[0].message.content.as_str().unwrap().contains("Thinking"));
        assert_eq!(lines[1].message.role, "assistant");
        assert_eq!(lines[1].message.content, json!("The answer is 42"));
        let usage = lines[1].message.usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_input_tokens, 10);
    }

    #[test]
    fn test_convert_message_with_tool_calls() {
        let session = GeminiSession {
            session_id: "test-session".into(),
            project_hash: "abc123".into(),
            start_time: "2026-01-01T00:00:00Z".into(),
            last_updated: "2026-01-01T00:00:00Z".into(),
            messages: vec![],
            kind: None,
        };
        let msg = GeminiMessage {
            id: "msg-3".into(),
            timestamp: "2026-01-01T00:01:00Z".into(),
            msg_type: "gemini".into(),
            content: json!("Let me search"),
            thoughts: None,
            tokens: None,
            model: Some("gemini-3-flash-preview".into()),
            tool_calls: Some(vec![GeminiToolCall {
                id: "grep_1".into(),
                name: "grep_search".into(),
                args: json!({"pattern": "foo"}),
                result: vec![GeminiToolResult {
                    function_response: Some(GeminiFunctionResponse {
                        id: "grep_1".into(),
                        name: "grep_search".into(),
                        response: json!({"output": "bar.rs:10: foo"}),
                    }),
                }],
                status: Some("success".into()),
                timestamp: Some("2026-01-01T00:01:05Z".into()),
                result_display: Some("bar.rs:10: foo".into()),
                display_name: Some("SearchText".into()),
                description: None,
                render_output_as_markdown: None,
            }]),
            display_content: None,
        };
        let lines = gemini_messages_to_cc(&[msg], &session, "/test");
        // 1 main + 1 tool_use + 1 tool_result = 3 lines
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].message.role, "assistant");
        assert_eq!(lines[1].message.role, "assistant"); // tool_use
        assert_eq!(lines[1].uuid, "grep_1-tool-use");
        let tool_use_block = &lines[1].message.content[0];
        assert_eq!(tool_use_block["type"], "tool_use");
        assert_eq!(tool_use_block["name"], "grep_search");
        assert_eq!(lines[2].message.role, "tool_result");
        assert_eq!(lines[2].uuid, "grep_1-tool-result");
    }

    /// Integration test: parse a real Gemini CLI session file from disk.
    /// Validates that the parser handles all real-world field variations.
    #[tokio::test]
    async fn test_parse_real_session_file() {
        let gemini_home = dirs::home_dir().unwrap().join(".gemini");
        let chats_dir = gemini_home.join("tmp/missiond/chats");
        if !chats_dir.exists() {
            // Skip if no Gemini CLI data available
            return;
        }
        let files = list_session_files(&chats_dir).await;
        if files.is_empty() {
            return;
        }
        // Parse the first file
        let session = parse_session(&files[0]).await.expect("Failed to parse session file");
        assert!(!session.session_id.is_empty());
        assert!(!session.messages.is_empty());

        // Convert all messages
        let lines = gemini_messages_to_cc(&session.messages, &session, "/tmp/example-project");
        assert!(!lines.is_empty());
        // Every line must have a non-empty session_id and uuid
        for line in &lines {
            assert_eq!(line.session_id, session.session_id);
            assert!(!line.uuid.is_empty());
            assert!(!line.message.role.is_empty());
        }
    }

    /// Integration test: parse the largest session file (likely has tool calls + thoughts).
    #[tokio::test]
    async fn test_parse_large_session_with_tools() {
        let path = dirs::home_dir()
            .unwrap()
            .join(".gemini/tmp/missiond/chats/session-2026-03-19T13-06-428b37d5.json");
        if !path.exists() {
            return;
        }
        let session = parse_session(&path).await.expect("Failed to parse large session");
        assert!(session.messages.len() > 5, "Expected many messages in large session");

        let lines = gemini_messages_to_cc(&session.messages, &session, "/tmp/example-project");
        // Should expand to more lines than original messages (thoughts + tool calls)
        assert!(lines.len() >= session.messages.len());

        // Check roles are valid
        let valid_roles = ["user", "assistant", "thinking", "tool_result", "system"];
        for line in &lines {
            assert!(valid_roles.contains(&line.message.role.as_str()),
                "Unexpected role: {}", line.message.role);
        }
    }

    #[test]
    fn test_info_message_empty_skipped() {
        let session = GeminiSession {
            session_id: "test-session".into(),
            project_hash: "abc".into(),
            start_time: "2026-01-01T00:00:00Z".into(),
            last_updated: "2026-01-01T00:00:00Z".into(),
            messages: vec![],
            kind: None,
        };
        let msg = GeminiMessage {
            id: "info-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            msg_type: "info".into(),
            content: json!(""),
            thoughts: None,
            tokens: None,
            model: None,
            tool_calls: None,
            display_content: None,
        };
        let lines = gemini_messages_to_cc(&[msg], &session, "/test");
        assert_eq!(lines.len(), 0);
    }
}
