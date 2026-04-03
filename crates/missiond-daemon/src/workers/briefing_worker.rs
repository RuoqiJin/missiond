//! Briefing Worker — async semantic summarization pipeline.
//!
//! Generates semantic summaries for timeline entries with long conversation messages,
//! replacing the mechanical 200-char truncation preview.
//!
//! Architecture: hybrid event-driven + polling.
//! - Event-driven: woken by `briefing_notify` when a long message is logged.
//! - Polling: fallback 120s sweep for missed/backfill entries.
//! - DB as reliable queue: entries with `summary == preview` are pending.
//!
//! Rate limiting delegated to SonnetGateway (P3: briefing priority).
//! Thinking messages use static rules (no LLM) to save quota.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::event_bus::DaemonEvent;
use crate::minimax_client::ChatMessage;
use crate::state::AppState;

/// Minimum content length (chars) to trigger briefing.
const MIN_CONTENT_CHARS: usize = 300;

/// Batch size per poll cycle.
const BATCH_SIZE: usize = 10;

/// Poll interval when idle (no pending entries).
#[allow(dead_code)]
const IDLE_INTERVAL_SECS: u64 = 120;

/// Result of processing a single entry.
enum ProcessResult {
    /// No processing done (content too short, not found, etc.)
    Skipped,
    /// Processed using a static rule or local logic (no API call).
    Local,
    /// Processed using MiniMax LLM API call.
    Llm,
}

/// Process a single timeline entry: fetch full content, generate summary, update DB.
async fn process_entry(
    state: &AppState,
    seq: i64,
    event_type: &str,
    payload: &str,
) -> Result<ProcessResult> {
    // Thinking messages: static rule, no LLM call needed
    if event_type == "thinking_message" {
        let preview = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| v.get("preview").and_then(|p| p.as_str()).map(String::from));
        let summary = match preview {
            Some(p) if !p.is_empty() => {
                // Take first sentence or first 80 chars
                let first_sentence_end = p
                    .find('。')
                    .or_else(|| p.find('.'))
                    .map(|i| i + 1)
                    .unwrap_or(p.len().min(80));
                let s: String = p.chars().take(first_sentence_end).collect();
                format!("[思考] {}", s)
            }
            _ => "[思考] ...".to_string(),
        };
        state.store.update_timeline_summary(seq, &summary).await?;
        state
            .event_bus
            .publish(DaemonEvent::BriefingSummaryGenerated {
                target_seq: seq,
                summary: summary.clone(),
                method: "static_rule".into(),
            });
        debug!(seq, "Briefing: thinking message — static rule");
        return Ok(ProcessResult::Local);
    }

    // Skip tool-only messages (preview is "[ToolName]" pattern) — no useful text to summarize
    let preview = serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| v.get("preview").and_then(|p| p.as_str()).map(String::from))
        .unwrap_or_default();
    if preview.starts_with('[') && preview.ends_with(']') && !preview.contains(' ') {
        // Mark with prefix so summary ≠ preview, breaking the re-selection loop.
        // Bug fix: setting summary = preview caused find_timeline_needing_briefing
        // to match forever (WHERE summary = preview), creating infinite loop.
        let briefed = format!("⚙ {}", preview);
        state.store.update_timeline_summary(seq, &briefed).await?;
        state
            .event_bus
            .publish(DaemonEvent::BriefingSummaryGenerated {
                target_seq: seq,
                summary: briefed.clone(),
                method: "tool_skip".into(),
            });
        debug!(seq, preview = %preview, "Briefing: tool-only message, marked as briefed");
        return Ok(ProcessResult::Local);
    }

    // Extract message_id from payload to fetch full content
    let payload_json: serde_json::Value =
        serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);

    let message_id = payload_json.get("message_id").and_then(|v| v.as_i64());

    let full_content = if let Some(msg_id) = message_id {
        state
            .store
            .get_conversation_message_by_id(msg_id)
            .await
            .ok()
            .flatten()
            .map(|m| m.content)
    } else {
        None
    };

    let text = match full_content {
        Some(ref c) if c.len() >= MIN_CONTENT_CHARS => c.as_str(),
        _ => {
            // Mark as skipped to prevent infinite re-selection.
            // Without this, entries with content_chars > 300 in payload but
            // actual content < 300 (or not found) stay in the pending queue forever.
            state
                .store
                .update_timeline_summary(seq, "<skipped>")
                .await?;
            debug!(
                seq,
                "Briefing: content too short or not found, marked skipped"
            );
            return Ok(ProcessResult::Skipped);
        }
    };

    // Truncate by lines (not chars) to avoid breaking code blocks
    let input_text = truncate_by_lines(text, 2000, 1000);

    // Role-specific prompt
    let prompt = match event_type {
        "user_message" => format!(
            "请用一句话(50字以内)提取用户的核心诉求或问题。直接输出，不要前缀。\n\n{}",
            input_text
        ),
        "assistant_message" => format!(
            "请简要总结AI的核心结论或方案。提炼1-2个要点，不超过100字。直接输出，不要前缀。\n\n{}",
            input_text
        ),
        _ => format!(
            "请用不超过100字简洁总结以下内容的核心要点。直接输出，不要前缀。\n\n{}",
            input_text
        ),
    };

    let max_chars = match event_type {
        "user_message" => 50,
        _ => 100,
    };

    let sonnet = state
        .sonnet
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Sonnet gateway not available"))?;

    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];
    let summary = sonnet.call_briefing(messages, Some(300), None).await?;

    if summary.is_empty() {
        warn!(seq, "Briefing: empty summary from Sonnet");
        return Ok(ProcessResult::Skipped);
    }

    // Truncate to limit, preferring sentence boundaries
    let summary = truncate_at_boundary(&summary, max_chars + 30);

    state.store.update_timeline_summary(seq, &summary).await?;
    state
        .event_bus
        .publish(DaemonEvent::BriefingSummaryGenerated {
            target_seq: seq,
            summary: summary.clone(),
            method: "sonnet".into(),
        });
    info!(
        seq,
        chars = summary.len(),
        event_type,
        "Briefing: summary updated"
    );

    Ok(ProcessResult::Llm)
}

/// Truncate text by lines: take first N + last M chars worth of lines.
fn truncate_by_lines(text: &str, head_chars: usize, tail_chars: usize) -> String {
    if text.len() <= head_chars + tail_chars + 100 {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();

    // Head: accumulate lines until head_chars
    let mut head = Vec::new();
    let mut head_len = 0;
    for line in &lines {
        if head_len + line.len() > head_chars && !head.is_empty() {
            break;
        }
        head.push(*line);
        head_len += line.len() + 1;
    }

    // Tail: accumulate lines from end until tail_chars
    let mut tail = Vec::new();
    let mut tail_len = 0;
    for line in lines.iter().rev() {
        if tail_len + line.len() > tail_chars && !tail.is_empty() {
            break;
        }
        tail.push(*line);
        tail_len += line.len() + 1;
    }
    tail.reverse();

    format!("{}\n...\n{}", head.join("\n"), tail.join("\n"))
}

/// Truncate at a natural boundary (sentence-ending punctuation).
fn truncate_at_boundary(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    // Look for last sentence boundary within limit
    let window: String = chars[..max_chars].iter().collect();
    for boundary in ['。', '.', '；', ';', '！', '!', '？', '?'] {
        if let Some(pos) = window.rfind(boundary) {
            if pos > max_chars / 2 {
                return window[..=pos].to_string();
            }
        }
    }
    // No good boundary — hard truncate
    format!("{}...", missiond_core::util::safe_byte_truncate(&window, max_chars.saturating_sub(3)))
}

pub(crate) struct BriefingWorker;

impl super::BackgroundWorker for BriefingWorker {
    fn name(&self) -> &'static str {
        "briefing_worker"
    }

    fn dependencies(&self) -> Vec<crate::control_tree::Dependency> {
        use crate::control_tree::{CtlProvider, Dependency};
        vec![Dependency::Provider(CtlProvider::Sonnet)]
    }

    async fn run(self, state: Arc<AppState>, mut ctx: super::WorkerContext) {
        let notify = Arc::clone(&state.briefing_notify);

        info!("Briefing worker started (event-driven, ControlTree-aware)");

        // Initial delay to let the daemon stabilize
        tokio::time::sleep(Duration::from_secs(30)).await;

        loop {
            // Block if paused (cascade: global, Sonnet gate, or direct)
            ctx.wait_if_paused().await;

            // Gemini ARB: check DB first — historical backlog may exist after restart
            let pending = match state
                .store
                .find_timeline_needing_briefing(MIN_CONTENT_CHARS, BATCH_SIZE)
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    warn!(error = %e, "Briefing worker: DB query failed");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
            };

            if pending.is_empty() {
                // Race: event notification vs control state change
                tokio::select! {
                    biased;
                    _ = ctx.wait_until_paused() => continue,
                    _ = notify.notified() => continue,
                }
            }

            let batch_size = pending.len();
            state.event_bus.publish(DaemonEvent::BriefingBatchStarted {
                pending_count: batch_size,
            });
            let mut processed = 0;
            let mut llm_calls = 0;

            for (seq, event_type, payload, _summary) in &pending {
                match process_entry(&state, *seq, event_type, payload).await {
                    Ok(ProcessResult::Llm) => {
                        processed += 1;
                        llm_calls += 1;
                    }
                    Ok(ProcessResult::Local) => {
                        processed += 1;
                    }
                    Ok(ProcessResult::Skipped) => {}
                    Err(e) => {
                        warn!(seq, error = %e, "Briefing worker: processing failed");
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                }
            }

            if processed > 0 {
                info!(
                    processed,
                    llm_calls, batch_size, "Briefing worker: batch completed"
                );
            }
        }
    }
}
