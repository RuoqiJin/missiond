//! Step Narrator Worker — batch-based GPT-5.4 conversation step explanation pipeline.
//!
//! Architecture: cursor-driven batch processing.
//! Each session is processed in batches of BATCH_SIZE messages.
//! A cursor table tracks progress (last_processed_id, batch_index) for
//! crash-safe resume. Between batches, context is passed via the last
//! message + its narration from the previous batch.
//!
//! Flow: discover sessions → for each session → fetch next batch →
//! build prompt with context → call GPT-5.4 → commit results + advance cursor.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tracing::{info, warn};

use crate::codex_cli::CodexCli;
use crate::event_bus::{DaemonEvent, TraceContext};
use crate::state::AppState;

/// Messages per batch sent to GPT-5.4.
const BATCH_SIZE: i64 = 10;

/// Minimum unnarrated messages before triggering analysis.
const MIN_UNNARRATED: i64 = 3;

/// Poll interval between checks.
const POLL_INTERVAL_SECS: u64 = 120;

/// Startup delay to let the system stabilize.
const STARTUP_DELAY_SECS: u64 = 60;

/// Max retries per batch before marking session as error.
const MAX_RETRIES: i64 = 3;

/// Rate limit between batches (seconds).
const BATCH_DELAY_SECS: u64 = 3;

const NARRATOR_SYSTEM_PROMPT: &str = r#"你是一个资深代码审查员和技术解说员。请分析这段 AI 编程助手 (Claude Code) 的操作日志，为每条消息生成精准凝练的上下文解说。

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

pub(crate) struct StepNarratorWorker;

impl super::BackgroundWorker for StepNarratorWorker {
    fn name(&self) -> &'static str {
        "step_narrator"
    }

    const KIND: super::WorkerKind = super::WorkerKind::Codex;

    async fn run(self, state: Arc<AppState>, mut ctx: super::WorkerContext) {
        let codex = CodexCli::new(
            "codex".to_string(),
            "gpt-5.4".to_string(),
            Duration::from_secs(180),
            state.event_bus.sender(),
        );

        info!("Step Narrator started (batch_size: {BATCH_SIZE}, poll: {POLL_INTERVAL_SECS}s)");
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;

        loop {
            ctx.wait_if_paused().await;
            if let Err(e) = process_pending(&state, &codex).await {
                warn!(error = %e, "Step Narrator: processing error");
            }
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    }
}

async fn process_pending(state: &AppState, codex: &CodexCli) -> anyhow::Result<()> {
    // Discover sessions with unnarrated messages
    let pending = state
        .store
        .get_sessions_needing_narration(MIN_UNNARRATED)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    if pending.is_empty() {
        return Ok(());
    }

    info!(
        sessions = pending.len(),
        "Step Narrator: found sessions needing narration"
    );

    // Process one session at a time (strict serial)
    for (session_id, _unnarrated_count) in pending {
        process_session(state, codex, &session_id).await?;
        // Rate limit between sessions
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    Ok(())
}

/// Process a single session: iterate batches until all messages are narrated.
async fn process_session(
    state: &AppState,
    codex: &CodexCli,
    session_id: &str,
) -> anyhow::Result<()> {
    let short_id = &session_id[..8.min(session_id.len())];

    // Get or create cursor
    let (last_processed_id, batch_index, status, _retry_count, total_messages) = state
        .store
        .get_or_create_narration_cursor(session_id)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    if status == "error" {
        info!(session = %short_id, "Step Narrator: skipping errored session");
        return Ok(());
    }

    // Count already narrated
    let narrations = state
        .store
        .get_narrations_for_session(session_id)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    let already_narrated = narrations.len();

    info!(session = %short_id, total = total_messages, already = already_narrated,
          cursor = last_processed_id, batch = batch_index,
          "Step Narrator: starting session");

    let trace_ctx = || TraceContext {
        trace_id: Some(format!("narrator-{}", short_id)),
        ..Default::default()
    };

    state.event_bus.publish_traced(
        DaemonEvent::NarrationSessionStarted {
            session_id: session_id.to_string(),
            total_messages: total_messages as usize,
            already_narrated,
        },
        trace_ctx(),
    );

    let mut current_cursor = last_processed_id;
    let mut current_batch_index = batch_index;
    let mut session_narrated_count = 0usize;

    loop {
        // Fetch next batch
        let batch = state
            .store
            .fetch_narration_batch(session_id, current_cursor, BATCH_SIZE)
            .await
            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
        if batch.is_empty() {
            break; // All messages processed
        }

        let batch_size = batch.len();
        let batch_last_id = batch.last().map(|m| m.id).unwrap_or(current_cursor);

        info!(session = %short_id, batch = current_batch_index, messages = batch_size,
              "Step Narrator: processing batch");

        // Mark cursor as processing
        state
            .store
            .mark_narration_cursor_processing(session_id)
            .await
            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

        // Build prompt with context
        let prompt = build_batch_prompt(state, session_id, current_batch_index, &batch).await?;

        let batch_start = Instant::now();

        // Call GPT-5.4
        match codex
            .call(
                &prompt,
                "step_narrator",
                None,
                None,
                Some(Duration::from_secs(180)),
                None,
            )
            .await
        {
            Ok(resp) => {
                let duration_ms = batch_start.elapsed().as_millis() as u64;

                // Parse response
                match parse_narration_response(&resp.content) {
                    Ok(parsed) => {
                        let narration_batch: Vec<(i64, &str, &str, &str, &str)> = parsed
                            .narrations
                            .iter()
                            .map(|n| {
                                (
                                    n.message_id,
                                    session_id,
                                    n.step_title.as_str(),
                                    n.step_intent.as_str(),
                                    n.step_result.as_str(),
                                )
                            })
                            .collect();

                        let count = state
                            .store
                            .commit_narration_batch(session_id, batch_last_id, &narration_batch)
                            .await
                            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
                        session_narrated_count += count;

                        info!(session = %short_id, batch = current_batch_index,
                              narrated = count, duration_ms,
                              "Step Narrator: batch completed");

                        state.event_bus.publish_traced(
                            DaemonEvent::NarrationBatchCompleted {
                                session_id: session_id.to_string(),
                                batch_index: current_batch_index as usize,
                                processed_count: count,
                                total_messages: total_messages as usize,
                                duration_ms,
                            },
                            trace_ctx(),
                        );

                        current_cursor = batch_last_id;
                        current_batch_index += 1;
                    }
                    Err(e) => {
                        warn!(session = %short_id, batch = current_batch_index, error = %e,
                              "Step Narrator: failed to parse response");
                        let permanently_failed = state
                            .store
                            .mark_narration_cursor_failed(session_id, MAX_RETRIES)
                            .await
                            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
                        state.event_bus.publish_traced(
                            DaemonEvent::NarrationFailed {
                                session_id: session_id.to_string(),
                                batch_index: current_batch_index as usize,
                                error: format!("Parse error: {e}"),
                                will_retry: !permanently_failed,
                            },
                            trace_ctx(),
                        );
                        return Ok(()); // Stop this session, try next
                    }
                }
            }
            Err(e) => {
                let duration_ms = batch_start.elapsed().as_millis() as u64;
                warn!(session = %short_id, batch = current_batch_index, error = %e, duration_ms,
                      "Step Narrator: codex call failed");
                let permanently_failed = state
                    .store
                    .mark_narration_cursor_failed(session_id, MAX_RETRIES)
                    .await
                    .map_err(|e2| anyhow::anyhow!("DB error: {}", e2))?;
                state.event_bus.publish_traced(
                    DaemonEvent::NarrationFailed {
                        session_id: session_id.to_string(),
                        batch_index: current_batch_index as usize,
                        error: format!("{e}"),
                        will_retry: !permanently_failed,
                    },
                    trace_ctx(),
                );
                return Ok(()); // Stop this session, try next
            }
        }

        // Rate limit between batches
        tokio::time::sleep(Duration::from_secs(BATCH_DELAY_SECS)).await;
    }

    // Session fully processed
    if session_narrated_count > 0 {
        info!(session = %short_id, total_narrated = session_narrated_count,
              "Step Narrator: session completed");
        state.event_bus.publish_traced(
            DaemonEvent::NarrationSessionCompleted {
                session_id: session_id.to_string(),
                total_narrated: session_narrated_count,
            },
            trace_ctx(),
        );
    }

    Ok(())
}

/// Build the prompt for a single batch, including context from previous batches.
async fn build_batch_prompt(
    state: &AppState,
    session_id: &str,
    batch_index: i64,
    batch_messages: &[missiond_core::types::ConversationMessage],
) -> anyhow::Result<String> {
    let mut parts = Vec::new();

    // System prompt
    parts.push(NARRATOR_SYSTEM_PROMPT.to_string());

    // Batch context header
    parts.push(format!(
        "\n\n--- 这是第 {} 批（每批最多 {} 条消息）---\n",
        batch_index + 1,
        BATCH_SIZE
    ));

    // Previous context (for batch_index > 0)
    if batch_index > 0 {
        parts.push("<previous_context>\n".to_string());
        if let Ok(Some((last_msg_id, title, intent, result))) =
            state.store.get_last_narration(session_id).await
        {
            // Get the actual message content for anchoring
            if let Ok(Some(msg)) = state
                .store
                .get_conversation_message_by_id(last_msg_id)
                .await
            {
                parts.push(format!(
                    "上一批最后一条消息 [MSG_ID:{}] [{}]：\n{}\n\n该消息的解说：\n- step_title: {}\n- step_intent: {}\n- step_result: {}\n",
                    msg.id, msg.role, msg.content, title, intent, result
                ));
            } else {
                parts.push(format!(
                    "上一批最后一条消息的解说：\n- step_title: {}\n- step_intent: {}\n- step_result: {}\n",
                    title, intent, result
                ));
            }
        }
        parts.push("</previous_context>\n\n".to_string());
    } else {
        parts.push(
            "<previous_context>\n这是会话的开头，没有前置上下文。\n</previous_context>\n\n"
                .to_string(),
        );
    }

    // Current batch messages (原文，不压缩)
    parts.push("<conversation>\n".to_string());

    let mut msg_ids = Vec::new();
    for msg in batch_messages {
        parts.push(format!(
            "[MSG_ID:{}] [{}]\n{}\n\n",
            msg.id, msg.role, msg.content
        ));
        msg_ids.push(msg.id);
    }

    parts.push("</conversation>\n".to_string());

    // Instruction
    parts.push(format!("\n请为以下 message_id 生成解说：{:?}", msg_ids));

    Ok(parts.join(""))
}

/// Parse GPT-5.4 narration response, handling markdown fences.
fn parse_narration_response(content: &str) -> anyhow::Result<NarrationResponse> {
    let content = content.trim();
    let json_str = if let Some(start) = content.find('{') {
        if let Some(end) = content.rfind('}') {
            &content[start..=end]
        } else {
            content
        }
    } else {
        content
    };

    serde_json::from_str(json_str).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse GPT narration: {e}\nRaw: {}",
            &json_str[..200.min(json_str.len())]
        )
    })
}
