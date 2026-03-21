use serde_json::Value;

// ===== Context Budget Manager =====
// Prevents 502/413 errors by ensuring messages don't exceed upstream payload limits.
// Architecture: Storage layer stores everything; this is the Compute/Transport layer.

/// Max HTTP payload content size for Router API calls.
/// Conservative limit to avoid 502 from upstream proxies (Caddy/Nginx/Cloudflare).
pub(crate) const MAX_ROUTER_PAYLOAD_BYTES: usize = 6 * 1024 * 1024; // 6 MB

/// Result of applying context budget to a messages array.
pub(crate) struct ContextBudgetResult {
    /// Whether any trimming was applied.
    pub(crate) trimmed: bool,
    /// Human-readable note about what was trimmed (None if within budget).
    pub(crate) note: Option<String>,
}

/// Apply context budget to a messages array for Router API calls.
///
/// Strategy when over budget:
/// 1. Keep first message (system prompt / context) + last N recent turns
/// 2. Drop middle messages, insert a note about omitted context
/// 3. Progressively reduce N until within budget
/// 4. If even 2 messages exceed budget, truncate the longest message
pub(crate) fn apply_context_budget(
    messages: &mut Vec<Value>,
    max_bytes: usize,
) -> ContextBudgetResult {
    let calc_size = |msgs: &[Value]| -> usize {
        msgs.iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .map(|s| s.len())
            .sum()
    };

    let total_bytes = calc_size(messages);
    if total_bytes <= max_bytes {
        return ContextBudgetResult {
            trimmed: false,
            note: None,
        };
    }

    let original_count = messages.len();

    // Edge case: 0-1 messages, can only truncate content
    if messages.len() <= 1 {
        if let Some(msg) = messages.first_mut() {
            truncate_message_content(msg, max_bytes);
        }
        return ContextBudgetResult {
            trimmed: true,
            note: Some(format!(
                "单条消息超出预算({:.1}MB > {:.1}MB)，已截断内容。",
                total_bytes as f64 / 1_048_576.0,
                max_bytes as f64 / 1_048_576.0
            )),
        };
    }

    // Sliding window: keep first message + last N turns
    let first = messages[0].clone();
    let mut keep_tail = messages.len() - 1;

    loop {
        let mut candidate: Vec<Value> = Vec::new();
        candidate.push(first.clone());

        let dropped = original_count - 1 - keep_tail;
        if dropped > 0 {
            candidate.push(serde_json::json!({
                "role": "system",
                "content": format!(
                    "[上下文管理] 为适应上下文窗口，已省略中间 {} 轮对话。如需回溯历史，请使用 mission_conversation_get 工具查询完整记录。",
                    dropped
                )
            }));
        }

        let tail_start = original_count - keep_tail;
        for msg in &messages[tail_start..] {
            candidate.push(msg.clone());
        }

        let size = calc_size(&candidate);
        if size <= max_bytes {
            let note = format!(
                "上下文超出预算({:.1}MB > {:.1}MB)，保留首条 + 最近 {} 轮，省略 {} 轮中间对话。",
                total_bytes as f64 / 1_048_576.0,
                max_bytes as f64 / 1_048_576.0,
                keep_tail,
                dropped
            );
            *messages = candidate;
            return ContextBudgetResult {
                trimmed: true,
                note: Some(note),
            };
        }

        if keep_tail <= 1 {
            // Even first + last message exceeds budget; truncate the longest
            let longest_idx = candidate
                .iter()
                .enumerate()
                .max_by_key(|(_, m)| {
                    m.get("content")
                        .and_then(|c| c.as_str())
                        .map(|s| s.len())
                        .unwrap_or(0)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            truncate_message_content(&mut candidate[longest_idx], max_bytes / 2);
            let note = format!(
                "上下文严重超出预算({:.1}MB > {:.1}MB)，仅保留首尾消息并截断最长内容。",
                total_bytes as f64 / 1_048_576.0,
                max_bytes as f64 / 1_048_576.0,
            );
            *messages = candidate;
            return ContextBudgetResult {
                trimmed: true,
                note: Some(note),
            };
        }

        keep_tail -= 1;
    }
}

/// Truncate a single message's content to fit within max_bytes (char-safe).
pub(crate) fn truncate_message_content(msg: &mut Value, max_bytes: usize) {
    if let Some(content) = msg
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
    {
        if content.len() > max_bytes {
            // Use char_indices for safe UTF-8 truncation
            let target_chars = max_bytes / 3; // conservative: assume ~3 bytes per char average
            let truncated: String = content.chars().take(target_chars).collect();
            msg["content"] = Value::String(format!(
                "{}\n\n[...内容因超出上下文预算被截断，原始长度 {:.1}MB...]",
                truncated,
                content.len() as f64 / 1_048_576.0
            ));
        }
    }
}

/// Extract displayable content from a Claude Code message content field.
/// Extract text content from a message's content field for the `content` DB column.
/// Content can be a plain string or an array of content blocks.
/// Storage layer: NO truncation. Full content preserved for analysis pipelines.
/// Truncation happens at the API/display layer only.
// extract_text_content, sanitize_raw_content, handle_new_events, backfill_conversation_events
// → moved to events_sync.rs

/// Format a single tool call entry for the audit trace Markdown.
pub(crate) fn format_tool_call_trace(md: &mut String, tc: &missiond_core::types::ToolCallRecord) {
    let time = if tc.timestamp.len() >= 19 {
        &tc.timestamp[11..19]
    } else {
        &tc.timestamp
    };
    let id_short = if tc.id.len() > 15 {
        format!("{}...", &tc.id[..12])
    } else {
        tc.id.clone()
    };
    let status_icon = match tc.status.as_str() {
        "success" => "✅",
        "error" => "❌",
        "pending" => "⏳",
        _ => "❓",
    };
    md.push_str(&format!("[{time}] 🛠️ {} ({id_short})\n", tc.tool_name));
    if let Some(ref summary) = tc.input_summary {
        md.push_str(&format!("  ├─ Input: {summary}\n"));
    }
    let output_display = tc
        .output_summary
        .as_deref()
        .unwrap_or(if tc.status == "pending" {
            "awaiting result"
        } else {
            "N/A"
        });
    md.push_str(&format!(
        "  └─ Output: [{status_icon}] {output_display}\n\n"
    ));
}
