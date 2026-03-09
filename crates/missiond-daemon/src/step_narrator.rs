//! Step Narrator Worker — async GPT-5.4 conversation step explanation pipeline.
//!
//! Periodically scans for sessions with unnarrated messages,
//! sends compressed conversation context to GPT-5.4 for step-by-step
//! explanations, and persists results to `message_narrations` table.
//!
//! Architecture: polling-based with debounce. Only processes sessions
//! where the conversation has been idle for >60s or has accumulated
//! enough unnarrated messages.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tracing::{info, warn};

use crate::codex_cli::CodexCli;
use crate::event_bus::DaemonEvent;
use crate::state::AppState;

/// Minimum unnarrated messages before triggering analysis.
const MIN_UNNARRATED: i64 = 3;

/// Poll interval between checks.
const POLL_INTERVAL_SECS: u64 = 120;

/// Startup delay to let the system stabilize.
const STARTUP_DELAY_SECS: u64 = 60;

/// Max conversation messages to send per request (cost control).
const MAX_CONTEXT_MESSAGES: usize = 200;

/// Max chars per tool result in compressed output.
const MAX_TOOL_RESULT_CHARS: usize = 500;

const NARRATOR_PROMPT: &str = r#"你是一个资深代码审查员和技术解说员。请分析这段 AI 编程助手 (Claude Code) 的操作日志，为每条消息生成精准凝练的上下文解说。

要求：
1. step_title: 一行概括这步做了什么（如"读取 Timeline 组件以了解胶囊渲染逻辑"）
2. step_intent: 这步操作的目的和意义（如"需要理解现有胶囊渲染方式才能添加点击交互"）
3. step_result: 这步操作的结果（如"确认胶囊使用 pointer-events-none，不支持点击"）

规则：
- 用中文回答
- 理解上下文后为每条消息生成解说，不要孤立地看单条消息
- 对于 user 消息，title 描述用户的意图
- 对于 assistant 工具调用消息，title 描述具体工具操作（包含文件名/命令等关键信息）
- 对于 assistant 文本回复，title 描述回复的核心内容
- 跳过没有实质内容的消息（如纯确认）

严格按以下 JSON 格式输出，不要有任何其他解释：
{"narrations":[{"message_id":123,"step_title":"...","step_intent":"...","step_result":"..."}]}"#;

/// Parsed narration from GPT response.
#[derive(Debug, Deserialize)]
struct NarrationResponse {
    narrations: Vec<NarrationEntry>,
}

#[derive(Debug, Deserialize)]
struct NarrationEntry {
    message_id: i64,
    step_title: String,
    step_intent: String,
    step_result: String,
}

pub(crate) fn spawn_step_narrator(state: Arc<AppState>) {
    let codex = CodexCli::new(
        "codex".to_string(),
        "gpt-5.4".to_string(),
        Duration::from_secs(180),
        state.event_bus.sender(),
    );

    tokio::spawn(async move {
        info!("Step Narrator started (poll interval: {POLL_INTERVAL_SECS}s, min unnarrated: {MIN_UNNARRATED})");
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;

        loop {
            if let Err(e) = process_pending(&state, &codex).await {
                warn!(error = %e, "Step Narrator: processing error");
            }
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    });
}

async fn process_pending(state: &AppState, codex: &CodexCli) -> anyhow::Result<()> {
    // Find sessions with enough unnarrated messages
    let pending = state.mission.db().get_sessions_needing_narration(MIN_UNNARRATED)?;

    if pending.is_empty() {
        return Ok(());
    }

    info!(sessions = pending.len(), "Step Narrator: found sessions needing narration");

    for (session_id, unnarrated_count) in pending {
        info!(session = %&session_id[..8.min(session_id.len())], unnarrated = unnarrated_count,
              "Step Narrator: processing session");

        let start = Instant::now();

        match narrate_session(state, codex, &session_id).await {
            Ok(count) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                info!(session = %&session_id[..8.min(session_id.len())], narrated = count, duration_ms,
                      "Step Narrator: completed");
                state.event_bus.publish(DaemonEvent::NarrationCompleted {
                    session_id: session_id.clone(),
                    narrated_count: count,
                    duration_ms,
                });
            }
            Err(e) => {
                warn!(session = %&session_id[..8.min(session_id.len())], error = %e,
                      "Step Narrator: failed");
            }
        }

        // Rate limiting between sessions
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    Ok(())
}

async fn narrate_session(state: &AppState, codex: &CodexCli, session_id: &str) -> anyhow::Result<usize> {
    let db = state.mission.db();

    // Fetch messages and existing narrations
    let messages = db.get_conversation_messages(session_id, None, MAX_CONTEXT_MESSAGES as i64)?;
    let narrations = db.get_narrations_for_session(session_id)?;
    let existing_ids: std::collections::HashSet<i64> = narrations.iter().map(|(id, _, _, _)| *id).collect();

    if messages.is_empty() {
        return Ok(0);
    }

    // Build compressed conversation for GPT
    let mut prompt_parts = Vec::new();
    prompt_parts.push(NARRATOR_PROMPT.to_string());
    prompt_parts.push("\n\n<conversation>\n".to_string());

    let mut unnarrated_ids = Vec::new();

    for msg in &messages {
        let role = msg.role.as_str();
        let content = compress_message_content(&msg.content, role);
        let marker = if existing_ids.contains(&msg.id) { " [已解说]" } else { "" };

        if !existing_ids.contains(&msg.id)
            && (role == "user" || role == "assistant")
        {
            unnarrated_ids.push(msg.id);
        }

        prompt_parts.push(format!(
            "[MSG_ID:{}] [{}]{}\n{}\n\n",
            msg.id, role, marker, content
        ));
    }

    prompt_parts.push("</conversation>\n".to_string());

    if unnarrated_ids.is_empty() {
        return Ok(0);
    }

    prompt_parts.push(format!(
        "\n请只为以下 message_id 生成解说（跳过已标记 [已解说] 的）：{:?}",
        unnarrated_ids
    ));

    let full_prompt = prompt_parts.join("");

    // Call GPT-5.4
    let resp = codex.call(
        &full_prompt,
        "step_narrator",
        None,
        None,
        Some(Duration::from_secs(180)),
        None,
    ).await?;

    // Parse JSON response
    let content = resp.content.trim();
    // Try to extract JSON from possible markdown fences
    let json_str = if let Some(start) = content.find('{') {
        if let Some(end) = content.rfind('}') {
            &content[start..=end]
        } else {
            content
        }
    } else {
        content
    };

    let parsed: NarrationResponse = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse GPT narration response: {e}\nRaw: {}", &json_str[..200.min(json_str.len())]))?;

    if parsed.narrations.is_empty() {
        return Ok(0);
    }

    // Persist narrations
    let narrations = parsed.narrations;
    let batch: Vec<(i64, &str, &str, &str, &str)> = narrations.iter()
        .map(|n| (n.message_id, session_id, n.step_title.as_str(), n.step_intent.as_str(), n.step_result.as_str()))
        .collect();
    let count = db.insert_narrations(&batch)?;

    Ok(count)
}

/// Compress message content for GPT prompt (cost control).
fn compress_message_content(content: &str, role: &str) -> String {
    if role == "user" {
        // User messages are usually short, keep as-is (truncate if huge)
        if content.len() > 2000 {
            format!("{}...[truncated {} chars]", &content[..1500], content.len() - 1500)
        } else {
            content.to_string()
        }
    } else {
        // Assistant messages: compress tool calls and long outputs
        let mut result = String::new();
        let lines: Vec<&str> = content.lines().collect();

        if lines.len() <= 30 {
            // Short enough, keep as-is
            if content.len() > MAX_TOOL_RESULT_CHARS * 4 {
                result.push_str(&content[..MAX_TOOL_RESULT_CHARS * 2]);
                result.push_str(&format!("\n...[truncated {} chars]\n", content.len() - MAX_TOOL_RESULT_CHARS * 3));
                result.push_str(&content[content.len() - MAX_TOOL_RESULT_CHARS..]);
            } else {
                return content.to_string();
            }
        } else {
            // Long content: keep first 15 and last 10 lines
            for line in &lines[..15] {
                result.push_str(line);
                result.push('\n');
            }
            result.push_str(&format!("...[{} lines truncated]\n", lines.len() - 25));
            for line in &lines[lines.len() - 10..] {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    }
}
