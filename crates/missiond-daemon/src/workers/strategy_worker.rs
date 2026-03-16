//! Strategic Analysis Worker — analyzes conversation logs to discover patterns,
//! user preferences, collaboration friction, and architectural insights.
//!
//! **Key design principle**: Completely stateless per call. Each analysis is an
//! independent Gemini request. The worker's "memory" lives entirely in the
//! Strategic State JSON stored in KB (key: `strategic-state`).
//!
//! Design doc: `docs/designs/arch-maintenance-and-strategic-analysis.md`

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::event_bus::{DaemonEvent, TimelineEvent};
use crate::gemini_client::REQUEST_CALLER;
use crate::embedding_worker::resolve_llm_credentials;
use crate::state::AppState;

use super::{BackgroundWorker, WorkerContext};

/// Analysis version — bump to re-analyze all sessions with a new schema.
const STRATEGY_ANALYSIS_VERSION: i32 = 1;
/// Max analysis retries before giving up on a session.
const MAX_ANALYSIS_RETRIES: i32 = 3;
/// Maximum cleaned prompt size (bytes) before truncation.
/// OS ARG_MAX is ~2MB on macOS; keep well under to avoid "Argument list too long".
const MAX_PROMPT_SIZE: usize = 1_500_000;
/// Startup delay to let other systems stabilize.
const STARTUP_DELAY_SECS: u64 = 120;
/// Polling fallback interval (if no events trigger analysis).
const POLL_INTERVAL_SECS: u64 = 300;

static RE_ANSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());
static RE_BASE64: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"data:[a-zA-Z/]+;base64,[A-Za-z0-9+/=]{100,}").unwrap());

pub(crate) struct StrategyWorker {
    pub timeline_rx: tokio::sync::broadcast::Receiver<TimelineEvent>,
}

impl BackgroundWorker for StrategyWorker {
    fn name(&self) -> &'static str { "strategy_analyst" }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        let mut rx = self.timeline_rx;

        info!("Strategy analyst worker started (delay {}s)", STARTUP_DELAY_SECS);
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;

        // Hybrid: event-driven + polling fallback
        let mut poll_interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ctx.wait_if_paused().await;

            tokio::select! {
                biased;
                result = rx.recv() => {
                    match result {
                        Ok(te) => match &te.event {
                            DaemonEvent::SlotBecameIdle { .. } => {
                                // Slot finished work — check for unanalyzed sessions
                                run_pending_analysis(&state, &mut ctx).await;
                            }
                            _ => {}
                        },
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(skipped = n, "strategy_analyst lagged");
                        }
                        Err(_) => break,
                    }
                }
                _ = poll_interval.tick() => {
                    // Fallback: periodic check for missed sessions
                    run_pending_analysis(&state, &mut ctx).await;
                }
            }
        }
    }
}

/// Find and analyze all pending sessions.
async fn run_pending_analysis(state: &AppState, ctx: &mut WorkerContext) {
    let db = state.mission.db();

    // Use existing deep analysis infrastructure to find pending sessions
    let pending = match db.get_pending_deep_analysis(STRATEGY_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "strategy_analyst: failed to query pending sessions");
            return;
        }
    };

    if pending.is_empty() {
        return;
    }

    info!(count = pending.len(), "strategy_analyst: found pending sessions");

    for conv in pending {
        let session_id = &conv.id;
        let watermark = if conv.deep_analyzed_message_id > 0 {
            Some(conv.deep_analyzed_message_id)
        } else {
            None
        };

        match analyze_session_stateless(state, session_id, watermark).await {
            Ok(analyzed_up_to) => {
                info!(session = %session_id, up_to = analyzed_up_to, "strategy_analyst: session analyzed");

                // Mark analysis complete or update checkpoint
                if conv.status == "active" {
                    // Active session: update watermark for incremental
                    if let Err(e) = db.update_deep_checkpoint(session_id, analyzed_up_to) {
                        warn!(error = %e, "strategy_analyst: failed to update checkpoint");
                    }
                } else {
                    // Completed/compacted: mark fully analyzed
                    if let Err(e) = db.mark_analysis_complete(session_id, STRATEGY_ANALYSIS_VERSION) {
                        warn!(error = %e, "strategy_analyst: failed to mark complete");
                    }
                }
                ctx.record_success();
            }
            Err(e) => {
                warn!(error = %e, session = %session_id, "strategy_analyst: analysis failed");
                let _ = db.mark_analysis_failed(session_id);
                ctx.record_failure();
            }
        }
    }
}

// ── Noise Stripping ──

/// Mechanical noise removal. NOT summarization — user words and AI reasoning preserved verbatim.
fn strip_noise(content: &str, role: &str) -> String {
    // Rule 1: ANSI escape sequences (PTY color codes)
    let content = RE_ANSI.replace_all(content, "");

    // Rule 2: base64 images → placeholder
    let content = RE_BASE64.replace_all(&content, "[图片]");

    // Rule 3: Long tool outputs → head + tail (UTF-8 safe, char-based)
    let char_count = content.chars().count();
    if role == "tool_result" && char_count > 5000 {
        let head: String = content.chars().take(2000).collect();
        let tail: String = content.chars().rev().take(2000).collect::<Vec<_>>().into_iter().rev().collect();
        return format!(
            "{}...[截断 {}字符]...{}",
            head,
            char_count - 4000,
            tail
        );
    }

    // Rules 4-5: User words + AI reasoning → preserve verbatim
    content.to_string()
}

// ── Prompt Assembly ──

/// Build the analysis prompt from session messages + Strategic State.
async fn build_analysis_prompt(
    state: &AppState,
    session_id: &str,
    since_id: Option<i64>,
) -> Result<(String, i64)> {
    let db = state.mission.db();

    // 1. Load session messages
    let messages = db.get_conversation_messages(
        session_id,
        since_id,
        50000, // generous limit
    )?;

    if messages.is_empty() {
        return Err(anyhow!("No messages found for session {}", session_id));
    }

    let last_message_id = messages.last().map(|m| m.id).unwrap_or(0);

    // 2. Noise stripping (mechanical rules)
    let cleaned: Vec<String> = messages
        .iter()
        .map(|m| format!("[{}] {}", m.role, strip_noise(&m.content, &m.role)))
        .collect();

    // 3. Extreme size guard: truncate oldest messages if > MAX_PROMPT_SIZE
    // Account for newlines from join("\n")
    let mut total_size: usize = cleaned.iter().map(|s| s.len()).sum::<usize>()
        + cleaned.len().saturating_sub(1); // newline separators
    let mut start_idx = 0;
    while total_size > MAX_PROMPT_SIZE && start_idx < cleaned.len() {
        total_size -= cleaned[start_idx].len() + 1; // +1 for newline
        start_idx += 1;
    }
    let cleaned = if start_idx > 0 {
        let mut truncated = vec![format!("[截断: 跳过最早 {} 条消息]", start_idx)];
        truncated.extend_from_slice(&cleaned[start_idx..]);
        truncated
    } else {
        cleaned
    };

    // 4. Load current Strategic State from KB
    let state_json = db
        .kb_get("strategic-state")?
        .and_then(|e| e.detail.map(|d| d.to_string()))
        .unwrap_or_else(|| "{}".to_string());

    // 5. Assemble prompt
    let cleaned_text = cleaned.join("\n");
    let prompt = format!(
        r#"你是一个顶尖的系统架构师和协作分析师。阅读下方一字不改的原始对话与操作日志。

## 当前战略状态
{state_json}

## 原始会话记录（完整保留）
{cleaned_text}

## 分析指令
基于以上原始操作记录，输出 JSON 格式的更新。严格按以下 schema：

```json
{{
  "user_profile": [
    {{"trait": "描述", "confidence": 0.9, "source": "session-id"}}
  ],
  "development_trajectory": {{
    "current_focus": "当前开发方向",
    "recent_shifts": ["方向变化"],
    "inferred_goals": ["推测目标"]
  }},
  "collaboration_patterns": [
    {{"pattern": "描述", "type": "positive|negative", "count": 1}}
  ],
  "workflow_proposals": [
    {{"action": "描述", "occurrences": 3, "status": "proposed"}}
  ],
  "friction_points": [
    {{"issue": "描述", "frequency": 2, "severity": "high|medium|low"}}
  ],
  "architectural_drifts": [
    {{"description": "描述", "affected_area": "模块/组件"}}
  ],
  "active_communication": {{
    "should_notify": false,
    "message": ""
  }}
}}
```

规则：
- user_profile 上限 20 条，与当前状态合并（更新已有、删除过时、新增发现）
- 不要复述发生了什么。直接输出 JSON，不要 markdown 包裹。
- 如果没有有意义的发现，输出 `{{}}`"#
    );

    Ok((prompt, last_message_id))
}

// ── Analysis Execution ──

/// Run a single stateless analysis for a session.
/// Returns the last analyzed message ID.
async fn analyze_session_stateless(
    state: &AppState,
    session_id: &str,
    since_id: Option<i64>,
) -> Result<i64> {
    info!(session = %session_id, since = ?since_id, "strategy_analyst: starting analysis");

    // 1. Build prompt
    let (prompt, last_message_id) = build_analysis_prompt(state, session_id, since_id).await?;

    info!(
        session = %session_id,
        prompt_chars = prompt.len(),
        messages_up_to = last_message_id,
        "strategy_analyst: calling Gemini"
    );

    // 2. Stateless LLM call
    let (base_url, jwt) = resolve_llm_credentials().await?;
    let model = "gemini-3.1-pro";
    let url = format!("{}/v1/chat/completions", base_url);
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 16384,
    });

    let result = REQUEST_CALLER
        .scope("strategy_analyst".to_string(), async {
            state
                .gemini
                .send_with_timeout(
                    &state.http_client,
                    &url,
                    &jwt,
                    &body,
                    Some(Duration::from_secs(600)),
                )
                .await
        })
        .await?;

    let content = result
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("{}")
        .to_string();

    info!(
        session = %session_id,
        response_chars = content.len(),
        "strategy_analyst: Gemini response received"
    );

    // 3. Parse and apply output
    let content = content.trim();
    if content == "{}" || content.is_empty() {
        debug!(session = %session_id, "strategy_analyst: no findings");
        return Ok(last_message_id);
    }

    // Extract JSON from response — handle preamble text and markdown code blocks
    let json_str = extract_json_from_response(content);

    match serde_json::from_str::<StrategicOutput>(&json_str) {
        Ok(output) => {
            apply_strategic_output(state, session_id, &output).await?;
            Ok(last_message_id)
        }
        Err(e) => {
            let preview: String = json_str.chars().take(200).collect();
            warn!(
                error = %e,
                session = %session_id,
                response_preview = %preview,
                "strategy_analyst: failed to parse JSON output"
            );
            // Return error so checkpoint is NOT advanced — session will be retried
            Err(anyhow!("JSON parse failed: {}", e))
        }
    }
}

// ── Output Types ──

#[derive(Debug, Deserialize, Serialize, Default)]
struct StrategicOutput {
    #[serde(default)]
    user_profile: Vec<UserTrait>,
    #[serde(default)]
    development_trajectory: Option<DevTrajectory>,
    #[serde(default)]
    collaboration_patterns: Vec<CollabPattern>,
    #[serde(default)]
    workflow_proposals: Vec<WorkflowProposal>,
    #[serde(default)]
    friction_points: Vec<FrictionPoint>,
    #[serde(default)]
    architectural_drifts: Vec<ArchDrift>,
    #[serde(default)]
    active_communication: Option<ActiveComm>,
}

#[derive(Debug, Deserialize, Serialize)]
struct UserTrait {
    #[serde(rename = "trait")]
    trait_: String,
    #[serde(default = "default_confidence")]
    confidence: f64,
    #[serde(default)]
    source: Option<String>,
}

fn default_confidence() -> f64 { 0.8 }

#[derive(Debug, Deserialize, Serialize)]
struct DevTrajectory {
    #[serde(default)]
    current_focus: Option<String>,
    #[serde(default)]
    recent_shifts: Vec<String>,
    #[serde(default)]
    inferred_goals: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CollabPattern {
    pattern: String,
    #[serde(rename = "type", default)]
    pattern_type: Option<String>,
    #[serde(default)]
    count: i64,
}

#[derive(Debug, Deserialize, Serialize)]
struct WorkflowProposal {
    action: String,
    #[serde(default)]
    occurrences: i64,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FrictionPoint {
    issue: String,
    #[serde(default)]
    frequency: i64,
    #[serde(default)]
    severity: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ArchDrift {
    description: String,
    #[serde(default)]
    affected_area: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ActiveComm {
    #[serde(default)]
    should_notify: bool,
    #[serde(default)]
    message: Option<String>,
}

// ── Output Application ──

/// Apply strategic analysis results to KB and Board.
async fn apply_strategic_output(
    state: &AppState,
    session_id: &str,
    output: &StrategicOutput,
) -> Result<()> {
    let db = state.mission.db();

    // 1. Update Strategic State JSON in KB
    let updated_state = json!({
        "version": STRATEGY_ANALYSIS_VERSION,
        "snapshot_at": chrono::Utc::now().to_rfc3339(),
        "last_session": session_id,
        "user_profile": output.user_profile,
        "development_trajectory": output.development_trajectory,
        "collaboration_patterns": output.collaboration_patterns,
        "workflow_proposals": output.workflow_proposals,
        "friction_points": output.friction_points,
        "anti_patterns": output.friction_points.iter()
            .filter(|f| f.severity.as_deref() == Some("high"))
            .map(|f| json!({"rule": f.issue, "confidence": 1.0}))
            .collect::<Vec<_>>(),
    });

    db.kb_remember(&missiond_core::types::KBRememberInput {
        category: "memory:architecture".to_string(),
        key: "strategic-state".to_string(),
        summary: "Strategic analysis state — user profile, dev trajectory, patterns".to_string(),
        detail: Some(updated_state),
        source: Some("strategy_analyst".to_string()),
        confidence: Some(1.0),
    })?;

    // 2. Write individual user preferences to KB
    for pref in &output.user_profile {
        if pref.confidence >= 0.7 {
            let key = format!("strategic-pref-{}", slug(&pref.trait_));
            db.kb_remember(&missiond_core::types::KBRememberInput {
                category: "preference".to_string(),
                key,
                summary: pref.trait_.clone(),
                detail: Some(json!({
                    "confidence": pref.confidence,
                    "source": pref.source,
                })),
                source: Some("strategy_analyst".to_string()),
                confidence: Some(pref.confidence),
            })?;
        }
    }

    // 3. Create Board tasks for workflow proposals (≥3 occurrences)
    for proposal in &output.workflow_proposals {
        if proposal.occurrences >= 3 {
            let dedupe = format!("strategy-workflow-{}", slug(&proposal.action));
            db.create_board_task(&missiond_core::types::CreateBoardTaskInput {
                title: format!("工作流自动化: {}", proposal.action),
                description: Some(format!(
                    "战略分析发现此操作出现 {} 次，建议固化为 Skill 或自动化。\n来源: session {}",
                    proposal.occurrences, session_id
                )),
                category: Some("dev".to_string()),
                priority: Some("medium".to_string()),
                dedupe_key: Some(dedupe),
                ..Default::default()
            })?;
        }
    }

    // 4. Create Board tasks for architectural drifts
    for drift in &output.architectural_drifts {
        let dedupe = format!("strategy-drift-{}", slug(&drift.description));
        db.create_board_task(&missiond_core::types::CreateBoardTaskInput {
            title: format!("架构漂移: {}", truncate(&drift.description, 60)),
            description: Some(format!(
                "战略分析发现架构偏离。\n影响范围: {}\n建议: 验证后更新 YAML manifest\n来源: session {}",
                drift.affected_area.as_deref().unwrap_or("未知"),
                session_id
            )),
            category: Some("dev".to_string()),
            priority: Some("high".to_string()),
            dedupe_key: Some(dedupe),
            ..Default::default()
        })?;
    }

    // 5. Log friction points as KB entries
    for friction in &output.friction_points {
        if friction.frequency >= 2 {
            let key = format!("strategy-friction-{}", slug(&friction.issue));
            db.kb_remember(&missiond_core::types::KBRememberInput {
                category: "memory:debug".to_string(),
                key,
                summary: format!("摩擦点({}次): {}", friction.frequency, friction.issue),
                detail: Some(json!({
                    "frequency": friction.frequency,
                    "severity": friction.severity,
                    "source_session": session_id,
                })),
                source: Some("strategy_analyst".to_string()),
                confidence: Some(0.8),
            })?;
        }
    }

    // 6. Notify via EventBus if needed
    if let Some(comm) = &output.active_communication {
        if comm.should_notify {
            if let Some(msg) = &comm.message {
                info!(message = %msg, "strategy_analyst: proactive notification");
                state.event_bus.publish(DaemonEvent::InsightGenerated {
                    category: "strategy".to_string(),
                    priority: "medium".to_string(),
                    title: msg.clone(),
                });
            }
        }
    }

    let stats = format!(
        "prefs={} patterns={} proposals={} drifts={} frictions={}",
        output.user_profile.len(),
        output.collaboration_patterns.len(),
        output.workflow_proposals.len(),
        output.architectural_drifts.len(),
        output.friction_points.len(),
    );
    info!(session = %session_id, %stats, "strategy_analyst: output applied");

    Ok(())
}

// ── Helpers ──

/// Create a URL-safe slug from text (for deduplication keys).
fn slug(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
        .collect::<String>()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// Truncate text to max chars (Unicode-safe), adding ellipsis.
fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}...", truncated)
    }
}

/// Extract JSON object from LLM response, handling:
/// - Pure JSON output
/// - Markdown code blocks (```json ... ```)
/// - Preamble text before JSON (find first `{` to last `}`)
fn extract_json_from_response(content: &str) -> String {
    let trimmed = content.trim();

    // Case 1: Already starts with `{`
    if trimmed.starts_with('{') {
        return trimmed.to_string();
    }

    // Case 2: Markdown code block anywhere in the response
    if let Some(block_start) = trimmed.find("```") {
        let after_fence = &trimmed[block_start + 3..];
        // Skip language tag line
        if let Some(newline) = after_fence.find('\n') {
            let inner = &after_fence[newline + 1..];
            if let Some(end) = inner.find("```") {
                let extracted = inner[..end].trim();
                if extracted.starts_with('{') {
                    return extracted.to_string();
                }
            }
        }
    }

    // Case 3: Find first `{` and last `}` in the content
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            return trimmed[start..=end].to_string();
        }
    }

    // Fallback: return as-is and let serde handle the error
    trimmed.to_string()
}
