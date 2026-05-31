//! Antigravity (`agy`) CLI transcript parser — reads `transcript_full.jsonl`
//! and the `history.jsonl` index, converting steps into `CCMessageLine`.
//!
//! The transformation flattens Antigravity's planner/tool/user steps into the
//! same `CCMessageLine` format used by the Claude Code JSONL pipeline, enabling
//! zero-modification reuse of the three-layer ingestion pipeline
//! (Ingest → Classify → Emit).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tokio::fs;
use tracing::debug;

use super::types::{AgySession, AgyStep};
use crate::cc_tasks::{CCMessage, CCMessageLine};

// ============ history.jsonl index (conversationId → workspace) ============

/// Maps Antigravity conversationId → workspace cwd, loaded from
/// `~/.gemini/antigravity-cli/history.jsonl`. Used to set CCMessageLine.cwd so
/// slot expectation-ticket binding and project resolution work like other
/// providers.
#[derive(Debug, Clone, Default)]
pub struct HistoryIndex {
    by_conversation: HashMap<String, String>,
}

impl HistoryIndex {
    /// Load from `<agy_home>/history.jsonl`. Missing/garbled lines are skipped.
    pub async fn load(agy_home: &Path) -> Self {
        let path = agy_home.join("history.jsonl");
        let mut idx = Self::default();
        let Ok(data) = fs::read_to_string(&path).await else {
            return idx;
        };
        for line in data.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let cid = value.get("conversationId").and_then(|v| v.as_str());
            let ws = value.get("workspace").and_then(|v| v.as_str());
            if let (Some(cid), Some(ws)) = (cid, ws) {
                idx.by_conversation.insert(cid.to_string(), ws.to_string());
            }
        }
        debug!(
            count = idx.by_conversation.len(),
            "Loaded agy history index"
        );
        idx
    }

    /// Resolve a conversationId to its recorded workspace cwd.
    pub fn resolve_cwd(&self, conversation_id: &str) -> Option<String> {
        self.by_conversation.get(conversation_id).cloned()
    }
}

// ============ Session discovery + parsing ============

/// Extract the conversationId from a transcript path:
/// `.../brain/<conversationId>/.system_generated/logs/transcript_full.jsonl`.
pub fn session_id_from_transcript(path: &Path) -> Option<String> {
    path.parent()? // logs/
        .parent()? // .system_generated/
        .parent()? // <conversationId>/
        .file_name()
        .and_then(|n| n.to_str())
        .map(ToString::to_string)
}

/// Discover all agy sessions under `<brain_root>`, returning
/// (conversationId, transcript_full.jsonl path) pairs.
pub async fn discover_sessions(brain_root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(mut entries) = fs::read_dir(brain_root).await else {
        return out;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(session_id) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let transcript = dir
            .join(".system_generated")
            .join("logs")
            .join("transcript_full.jsonl");
        if transcript.is_file() {
            out.push((session_id.to_string(), transcript));
        }
    }
    out
}

/// Parse a `transcript_full.jsonl` into an `AgySession`. Unparseable lines are
/// skipped (the trajectory is append-only and may be mid-write).
pub async fn parse_session(path: &Path) -> Option<AgySession> {
    let session_id = session_id_from_transcript(path)?;
    let data = fs::read_to_string(path).await.ok()?;
    let mut steps = Vec::new();
    for line in data.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<AgyStep>(line) {
            Ok(step) => steps.push(step),
            Err(e) => debug!(path = %path.display(), error = %e, "Skipping unparseable agy step"),
        }
    }
    Some(AgySession { session_id, steps })
}

/// Lightweight step-count probe (mirrors gemini's session_message_count).
pub async fn session_step_count(path: &Path) -> Option<usize> {
    parse_session(path).await.map(|s| s.steps.len())
}

// ============ Step → CCMessageLine transformation ============

/// Convert a slice of new Antigravity steps into CCMessageLine records.
///
/// Each step may expand into multiple CCMessageLines:
/// - thinking → a `thinking` message (before the main line)
/// - USER_INPUT → `user`
/// - PLANNER_RESPONSE content → `assistant`; tool_calls → `assistant` tool_use
/// - tool-execution steps (LIST_DIRECTORY/VIEW_FILE/GREP_SEARCH/…) → `tool_result`
/// - CONVERSATION_HISTORY → skipped (replay of prior context)
pub fn agy_steps_to_cc(steps: &[AgyStep], session_id: &str, cwd: &str) -> Vec<CCMessageLine> {
    let mut out = Vec::new();
    for step in steps {
        convert_step(step, session_id, cwd, &mut out);
    }
    out
}

fn convert_step(step: &AgyStep, session_id: &str, cwd: &str, out: &mut Vec<CCMessageLine>) {
    let ts = step.created_at.clone().unwrap_or_default();
    let base_uuid = format!("{session_id}:{}", step.step_index);

    // 1. thinking → independent thinking message (before main content)
    if let Some(thinking) = &step.thinking {
        let text = extract_text(thinking);
        if !text.trim().is_empty() {
            out.push(make_cc_line(
                session_id,
                &format!("{base_uuid}-thinking"),
                &ts,
                "thinking",
                json!(text),
                cwd,
            ));
        }
    }

    match step.step_type.as_str() {
        "USER_INPUT" => {
            let raw = step.content.as_ref().map(extract_text).unwrap_or_default();
            let text = strip_user_request_wrapper(&raw);
            if !text.trim().is_empty() {
                out.push(make_cc_line(
                    session_id,
                    &base_uuid,
                    &ts,
                    "user",
                    json!(text),
                    cwd,
                ));
            }
        }
        "PLANNER_RESPONSE" => {
            // A planner step carries EITHER a text answer OR tool invocations.
            if let Some(content) = &step.content {
                let text = extract_text(content);
                if !text.trim().is_empty() {
                    out.push(make_cc_line(
                        session_id,
                        &base_uuid,
                        &ts,
                        "assistant",
                        json!(text),
                        cwd,
                    ));
                }
            }
            if let Some(calls) = &step.tool_calls {
                for (i, call) in calls.iter().enumerate() {
                    let tool_use_id = format!("{base_uuid}-tool-{i}");
                    let block = json!([{
                        "type": "tool_use",
                        "id": tool_use_id,
                        "name": call.name,
                        "input": call.args,
                    }]);
                    out.push(make_cc_line(
                        session_id,
                        &tool_use_id,
                        &ts,
                        "assistant",
                        block,
                        cwd,
                    ));
                }
            }
        }
        // Replay of prior conversation context — already ingested, skip.
        "CONVERSATION_HISTORY" => {}
        // Tool-execution result steps (LIST_DIRECTORY, VIEW_FILE, GREP_SEARCH,
        // RUN_COMMAND, …): record the output as a tool_result line.
        _ => {
            if let Some(content) = &step.content {
                let text = extract_text(content);
                if !text.trim().is_empty() {
                    out.push(make_cc_line(
                        session_id,
                        &base_uuid,
                        &ts,
                        "tool_result",
                        json!(text),
                        cwd,
                    ));
                }
            }
        }
    }
}

/// Antigravity wraps the user prompt as
/// `<USER_REQUEST>\n{prompt}\n</USER_REQUEST>\n<ADDITIONAL_METADATA>…`.
/// Keep only the inner request text so the durable conversation stores the real
/// prompt and slot expectation-ticket binding can match the dispatched text.
fn strip_user_request_wrapper(raw: &str) -> String {
    const OPEN: &str = "<USER_REQUEST>";
    const CLOSE: &str = "</USER_REQUEST>";
    if let Some(start) = raw.find(OPEN) {
        let after = &raw[start + OPEN.len()..];
        if let Some(end) = after.find(CLOSE) {
            return after[..end].trim().to_string();
        }
    }
    raw.trim().to_string()
}

/// Flatten an Antigravity content value to plain text. Content is usually a
/// string; arrays of `{text}` blocks are joined.
fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(|t| t.as_str())
                    .map(ToString::to_string)
                    .or_else(|| item.as_str().map(ToString::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn make_cc_line(
    session_id: &str,
    uuid: &str,
    timestamp: &str,
    role: &str,
    content: Value,
    cwd: &str,
) -> CCMessageLine {
    CCMessageLine {
        message_type: "message".to_string(),
        parent_uuid: None,
        is_sidechain: false,
        cwd: cwd.to_string(),
        session_id: session_id.to_string(),
        version: "agy-cli".to_string(),
        git_branch: None,
        message: CCMessage {
            role: role.to_string(),
            content,
            model: None,
            usage: None,
            stop_reason: None,
        },
        uuid: uuid.to_string(),
        timestamp: timestamp.to_string(),
        todos: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(step_index: i64, step_type: &str, content: Option<Value>) -> AgyStep {
        AgyStep {
            step_index,
            source: "MODEL".into(),
            step_type: step_type.into(),
            status: Some("DONE".into()),
            created_at: Some("2026-05-27T05:11:30Z".into()),
            content,
            thinking: None,
            tool_calls: None,
        }
    }

    #[test]
    fn user_input_unwraps_request_envelope() {
        let raw = "<USER_REQUEST>\n评价一下这个项目\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nlocal time\n</ADDITIONAL_METADATA>";
        let lines = agy_steps_to_cc(
            &[step(0, "USER_INPUT", Some(json!(raw)))],
            "conv-1",
            "/Users/x/Projects/missiond",
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message.role, "user");
        assert_eq!(lines[0].message.content, json!("评价一下这个项目"));
        assert_eq!(lines[0].cwd, "/Users/x/Projects/missiond");
        assert_eq!(lines[0].session_id, "conv-1");
    }

    #[test]
    fn planner_response_text_is_assistant() {
        let lines = agy_steps_to_cc(
            &[step(
                5,
                "PLANNER_RESPONSE",
                Some(json!("https://github.com/x/y")),
            )],
            "conv-1",
            "/cwd",
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message.role, "assistant");
        assert_eq!(lines[0].message.content, json!("https://github.com/x/y"));
        assert_eq!(lines[0].uuid, "conv-1:5");
    }

    #[test]
    fn planner_response_tool_call_is_assistant_tool_use() {
        let mut s = step(3, "PLANNER_RESPONSE", None);
        s.tool_calls = Some(vec![super::super::types::AgyToolCall {
            name: "list_dir".into(),
            args: json!({"path": "."}),
        }]);
        let lines = agy_steps_to_cc(&[s], "conv-1", "/cwd");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message.role, "assistant");
        assert_eq!(lines[0].message.content[0]["type"], "tool_use");
        assert_eq!(lines[0].message.content[0]["name"], "list_dir");
        assert_eq!(lines[0].uuid, "conv-1:3-tool-0");
    }

    #[test]
    fn tool_execution_step_is_tool_result() {
        let lines = agy_steps_to_cc(
            &[step(7, "VIEW_FILE", Some(json!("fn main() {}")))],
            "conv-1",
            "/cwd",
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message.role, "tool_result");
        assert_eq!(lines[0].message.content, json!("fn main() {}"));
    }

    #[test]
    fn conversation_history_is_skipped() {
        let lines = agy_steps_to_cc(
            &[step(0, "CONVERSATION_HISTORY", Some(json!("old context")))],
            "conv-1",
            "/cwd",
        );
        assert!(lines.is_empty());
    }

    #[test]
    fn thinking_emits_separate_line_before_main() {
        let mut s = step(2, "PLANNER_RESPONSE", Some(json!("final")));
        s.thinking = Some(json!("let me think"));
        let lines = agy_steps_to_cc(&[s], "conv-1", "/cwd");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].message.role, "thinking");
        assert_eq!(lines[0].message.content, json!("let me think"));
        assert_eq!(lines[1].message.role, "assistant");
    }

    #[test]
    fn session_id_from_transcript_extracts_conversation_dir() {
        let p = Path::new(
            "/home/u/.gemini/antigravity-cli/brain/abc-123/.system_generated/logs/transcript_full.jsonl",
        );
        assert_eq!(session_id_from_transcript(p).as_deref(), Some("abc-123"));
    }

    /// Integration: parse the real on-disk Antigravity transcript if present.
    /// Validates that the final assistant line is recoverable from the jsonl
    /// (the whole point — results come from jsonl, not screen scraping).
    #[tokio::test]
    async fn parse_real_transcript_if_present() {
        let Some(home) = dirs::home_dir() else { return };
        let brain = home.join(".gemini/antigravity-cli/brain");
        if !brain.exists() {
            return;
        }
        let sessions = discover_sessions(&brain).await;
        let Some((session_id, transcript)) = sessions.into_iter().next() else {
            return;
        };
        let parsed = parse_session(&transcript).await.expect("parse session");
        assert_eq!(parsed.session_id, session_id);
        assert!(!parsed.steps.is_empty());

        let lines = agy_steps_to_cc(&parsed.steps, &parsed.session_id, "/tmp");
        assert!(!lines.is_empty());
        let valid_roles = ["user", "assistant", "thinking", "tool_result", "system"];
        for line in &lines {
            assert_eq!(line.session_id, session_id);
            assert!(!line.uuid.is_empty());
            assert!(
                valid_roles.contains(&line.message.role.as_str()),
                "unexpected role: {}",
                line.message.role
            );
        }
        // At least one user prompt and one assistant turn must survive.
        assert!(lines.iter().any(|l| l.message.role == "user"));
        assert!(lines.iter().any(|l| l.message.role == "assistant"));
    }
}
