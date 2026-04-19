//! Intent Analyst — Phase 6 of the Cognitive Pipeline.
//!
//! Deep intent analysis from conversation_turns via Sonnet LLM.
//! Detects stuck retries, architecture exploration, refactoring shifts, and scope creep.
//!
//! Trigger: TurnExtracted events (mixed: 5-min debounce OR 5 accumulated turns).
//! Output: user_intents table rows + conversation_turns.intent_group_id back-reference.
//!
//! Design doc: `docs/designs/phase6-intent-analyst.md`

use std::time::Duration;

use anyhow::Result;
use tracing::{info, warn};

use crate::helpers::char_boundary_at;
use crate::minimax_client::ChatMessage;
use crate::state::AppState;
use missiond_core::event::events::MemoryEvent;
use missiond_core::types::ConversationTurn;

/// Number of turns accumulated before triggering analysis. Exposed so
/// `bus::v2_subscribers` keeps parity with v1 semantics.
pub(crate) const MAX_ACCUMULATION: usize = 5; // fire after 5 unseen turns
/// Debounce window for the intent-analyst subscription in seconds.
pub(crate) const DEBOUNCE_SECS: u64 = 300; // 5 minutes
const MIN_TURNS_FOR_ANALYSIS: usize = 2; // need >=2 turns to detect patterns

/// Startup backfill: process completed sessions that have turns but no intents.
#[allow(dead_code)]
pub(crate) async fn intent_startup_backfill(state: &AppState) {
    if !state.intent_analyst_enabled {
        return;
    }

    let sessions = match state.store.sessions_pending_intent_analysis(50).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "IntentAnalyst: backfill query failed");
            return;
        }
    };

    if sessions.is_empty() {
        return;
    }

    let total = sessions.len();
    let mut success = 0usize;
    for session_id in &sessions {
        match process_session_intents(state, session_id).await {
            Ok(c) if c > 0 => success += 1,
            Ok(_) => {}
            Err(e) => warn!(session = %session_id, error = %e, "IntentAnalyst: backfill failed"),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    info!(total, success, "IntentAnalyst: startup backfill complete");
}

/// Core analysis: fetch uncovered turns, call Sonnet, write intents.
pub(crate) async fn process_session_intents(state: &AppState, session_id: &str) -> Result<usize> {
    // 1. Incremental cursor: get max turn_range_end already analyzed
    let covered_end = state
        .store
        .get_intent_coverage(session_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_intent_coverage: {}", e))?
        .unwrap_or(-1);

    // 2. Fetch uncovered turns
    let mut turns = state
        .store
        .get_turns_after(session_id, covered_end)
        .await
        .map_err(|e| anyhow::anyhow!("get_turns_after: {}", e))?;

    if turns.len() < MIN_TURNS_FOR_ANALYSIS {
        return Ok(0);
    }

    // Gemini audit fix: forward pagination — only analyze the next 15 turns per batch.
    // Without this, a 50-turn session would trigger build_analysis_context's reverse truncation,
    // causing the LLM to only see turns 31-50 while cursor advances to 50 — permanently
    // skipping turns 0-30 (the "sliding window black hole" bug).
    // With truncation, this batch covers turns 0-14, next batch covers 15-29, etc.
    const MAX_TURNS_PER_BATCH: usize = 15;
    if turns.len() > MAX_TURNS_PER_BATCH {
        turns.truncate(MAX_TURNS_PER_BATCH);
    }

    // 3. Build analysis context
    let context = build_analysis_context(&turns);
    if context.trim().is_empty() {
        return Ok(0);
    }

    // 4. Call Sonnet via P4 intent channel
    let sonnet = match &state.sonnet {
        Some(s) => s,
        None => return Err(anyhow::anyhow!("Sonnet gateway not available")),
    };

    let system = intent_analysis_prompt();
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: context,
        },
    ];

    let raw = sonnet
        .call_intent(messages, Some(1024), None)
        .await
        .map_err(|e| anyhow::anyhow!("Sonnet intent call failed: {}", e))?;

    // 5. Parse response
    let intents = parse_intent_response(&raw, &turns)?;
    if intents.is_empty() {
        return Ok(0);
    }

    // 6. Write to DB and update back-references
    let mut count = 0usize;
    for intent in &intents {
        let id = state
            .store
            .insert_user_intent(
                session_id,
                intent.turn_start,
                intent.turn_end,
                &intent.intent_type,
                intent.confidence,
                Some(&intent.summary),
                None,
                None,
            )
            .await
            .map_err(|e| anyhow::anyhow!("insert_user_intent: {}", e))?;

        let _ = state
            .store
            .update_turns_intent_group(session_id, intent.turn_start, intent.turn_end, id)
            .await;

        // Emit event
        let _ = state
            .bus
            .publish_memory(MemoryEvent::IntentAnalyzed {
                session_id: session_id.to_string(),
                intent_type: intent.intent_type.clone(),
            })
            .await;

        count += 1;
    }

    Ok(count)
}

/// Build analysis context from turns. Newest turns prioritized when truncating.
fn build_analysis_context(turns: &[ConversationTurn]) -> String {
    let mut ctx = String::with_capacity(6000);

    for turn in turns {
        let entry = format!(
            "Turn {} [{} → {}]:\n  用户: {}\n  工具: {} ({}次调用)\n  代码变更: {} | MCP: {}\n\n",
            turn.turn_idx,
            turn.started_at.as_deref().unwrap_or("?"),
            turn.ended_at.as_deref().unwrap_or("?"),
            turn.user_content.as_deref().unwrap_or("(无)"),
            turn.tool_names.as_deref().unwrap_or("无"),
            turn.tool_call_count,
            if turn.has_code_change { "是" } else { "否" },
            if turn.has_mcp_call { "是" } else { "否" },
        );
        ctx.push_str(&entry);
    }

    // Gemini review: when over 6K, keep newest turns (most important for intent analysis)
    if ctx.len() > 6000 {
        let mut trimmed = String::with_capacity(6000);
        for turn in turns.iter().rev() {
            let entry = format!(
                "Turn {} [{} → {}]:\n  用户: {}\n  工具: {} ({}次调用)\n  代码变更: {} | MCP: {}\n\n",
                turn.turn_idx,
                turn.started_at.as_deref().unwrap_or("?"),
                turn.ended_at.as_deref().unwrap_or("?"),
                turn.user_content.as_deref().unwrap_or("(无)"),
                turn.tool_names.as_deref().unwrap_or("无"),
                turn.tool_call_count,
                if turn.has_code_change { "是" } else { "否" },
                if turn.has_mcp_call { "是" } else { "否" },
            );
            if trimmed.len() + entry.len() > 6000 {
                break;
            }
            trimmed.insert_str(0, &entry);
        }
        return trimmed;
    }

    ctx
}

fn intent_analysis_prompt() -> String {
    "你是 MissionD 的意图分析引擎。分析连续的 Claude Code 对话 Turn，推导用户的深层意图。\n\n\
     每组连续 Turn 输出一条意图记录，JSON 数组格式:\n\
     [{\"turn_start\": 3, \"turn_end\": 5, \"type\": \"stuck_retry\", \"confidence\": 0.85, \
       \"summary\": \"用户连续3轮尝试修复CORS跨域错误\"}]\n\n\
     意图类型:\n\
     - normal_progress: 正常推进，无异常\n\
     - stuck_retry: 反复 debug/重试同一问题 (>=2 turns 相似错误模式)\n\
     - architecture_explore: 探索新架构/偏离当前项目主线\n\
     - refactor_shift: 从功能开发转向重构\n\
     - scope_creep: 目标频繁变更/范围蔓延\n\n\
     规则:\n\
     1. confidence 0.0-1.0，仅当证据充分时 >0.7\n\
     2. summary 中文，一句话，包含具体技术细节\n\
     3. 相邻的 normal_progress turns 可合并为一条\n\
     4. 输出纯 JSON，无 markdown fence\n\
     5. turn_start 和 turn_end 必须在输入 Turn 范围内"
        .to_string()
}

#[derive(Debug)]
struct ParsedIntent {
    turn_start: i32,
    turn_end: i32,
    intent_type: String,
    confidence: f32,
    summary: String,
}

fn parse_intent_response(raw: &str, turns: &[ConversationTurn]) -> Result<Vec<ParsedIntent>> {
    let json_str = raw.trim();
    let json_str = if json_str.starts_with("```") {
        json_str
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        json_str
    };

    let items: Vec<serde_json::Value> = serde_json::from_str(json_str).map_err(|e| {
        anyhow::anyhow!(
            "Intent JSON parse failed: {} (raw: {})",
            e,
            &json_str[..char_boundary_at(json_str, 200)]
        )
    })?;

    let valid_types = [
        "normal_progress",
        "stuck_retry",
        "architecture_explore",
        "refactor_shift",
        "scope_creep",
    ];
    let min_idx = turns.first().map(|t| t.turn_idx).unwrap_or(0);
    let max_idx = turns.last().map(|t| t.turn_idx).unwrap_or(0);

    let mut result = Vec::new();
    for item in &items {
        let turn_start = item
            .get("turn_start")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1) as i32;
        let turn_end = item.get("turn_end").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        let intent_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let confidence = item
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5) as f32;
        let summary = item
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Validate
        if turn_start < min_idx || turn_end > max_idx || turn_start > turn_end {
            continue;
        }
        if !valid_types.contains(&intent_type.as_str()) {
            continue;
        }
        if summary.is_empty() {
            continue;
        }
        if confidence < 0.0 || confidence > 1.0 {
            continue;
        }

        result.push(ParsedIntent {
            turn_start,
            turn_end,
            intent_type,
            confidence,
            summary,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_intent_response_basic() {
        let turns = vec![make_turn(0), make_turn(1), make_turn(2), make_turn(3)];
        let raw = r#"[
            {"turn_start": 0, "turn_end": 1, "type": "normal_progress", "confidence": 0.9, "summary": "正常推进部署配置"},
            {"turn_start": 2, "turn_end": 3, "type": "stuck_retry", "confidence": 0.8, "summary": "反复修复CORS跨域错误"}
        ]"#;

        let result = parse_intent_response(raw, &turns).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].intent_type, "normal_progress");
        assert_eq!(result[1].intent_type, "stuck_retry");
    }

    #[test]
    fn test_parse_intent_response_invalid_type() {
        let turns = vec![make_turn(0), make_turn(1)];
        let raw = r#"[{"turn_start": 0, "turn_end": 1, "type": "invalid_type", "confidence": 0.5, "summary": "test"}]"#;
        let result = parse_intent_response(raw, &turns).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_intent_response_out_of_range() {
        let turns = vec![make_turn(2), make_turn(3)];
        let raw = r#"[{"turn_start": 0, "turn_end": 5, "type": "normal_progress", "confidence": 0.5, "summary": "test"}]"#;
        let result = parse_intent_response(raw, &turns).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_intent_response_code_fence() {
        let turns = vec![make_turn(0), make_turn(1)];
        let raw = "```json\n[{\"turn_start\": 0, \"turn_end\": 1, \"type\": \"normal_progress\", \"confidence\": 0.7, \"summary\": \"正常推进\"}]\n```";
        let result = parse_intent_response(raw, &turns).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_build_analysis_context_short() {
        let turns = vec![make_turn(0), make_turn(1)];
        let ctx = build_analysis_context(&turns);
        assert!(ctx.contains("Turn 0"));
        assert!(ctx.contains("Turn 1"));
    }

    fn make_turn(idx: i32) -> ConversationTurn {
        ConversationTurn {
            id: idx as i64,
            session_id: "test".to_string(),
            turn_idx: idx,
            start_message_id: idx as i64 * 10,
            end_message_id: idx as i64 * 10 + 5,
            user_content: Some(format!("Turn {} 用户指令", idx)),
            tool_names: Some("Bash,Read".to_string()),
            tool_call_count: 2,
            message_count: 5,
            has_code_change: false,
            has_mcp_call: false,
            started_at: Some("2026-03-22T10:00:00Z".to_string()),
            ended_at: Some("2026-03-22T10:05:00Z".to_string()),
            topic: None,
            intent_group_id: None,
            files_read: None,
            files_changed: None,
            outcome: None,
            skeleton: None,
        }
    }
}
