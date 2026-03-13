//! Retrospective Worker — event-driven session analysis pipeline.
//!
//! L1: Rust quantitative analysis (quick+detailed) for ALL completed sessions.
//! L2: MiniMax M2.5 full-coverage triage — structured findings/severity.
//! High-severity sessions get Board tasks for manual deep-dive.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn, debug};

use crate::minimax_client::ChatMessage;
use crate::state::AppState;

/// Poll interval between checks (1 hour).
const POLL_INTERVAL_SECS: u64 = 3600;

/// Backfill: analyze ALL sessions since a given time (no threshold filtering).
/// Returns (analyzed_count, skipped_count).
pub(crate) async fn backfill(state: &AppState, since: &str) -> anyhow::Result<(usize, usize)> {
    let db = state.mission.db();
    let sessions = db.get_sessions_for_retro_backfill(since, false)
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    info!(count = sessions.len(), since, "Retro backfill: found sessions");

    let mut analyzed = 0;
    let mut skipped = 0;
    for (session_id, msg_count, tool_count, error_rate) in &sessions {
        match analyze_session(state, &session_id, *msg_count, *tool_count, *error_rate).await {
            Ok(_) => {
                analyzed += 1;
                tokio::time::sleep(Duration::from_secs(INTER_SESSION_DELAY_SECS)).await;
            }
            Err(e) => {
                warn!(session_id, error = %e, "Retro backfill: session failed");
                skipped += 1;
            }
        }
    }

    info!(analyzed, skipped, "Retro backfill: complete");
    Ok((analyzed, skipped))
}

/// Startup delay to let the system stabilize.
const STARTUP_DELAY_SECS: u64 = 120;

/// Rate limit between session analyses (seconds).
const INTER_SESSION_DELAY_SECS: u64 = 10;

/// MiniMax max tokens for retrospective analysis.
const MINIMAX_MAX_TOKENS: u32 = 2000;

pub(crate) struct RetroWorker;

impl super::BackgroundWorker for RetroWorker {
    fn name(&self) -> &'static str { "retro_worker" }

    async fn run(self, state: Arc<AppState>, mut ctx: super::WorkerContext) {
        info!("Retro worker started (poll: {}s, startup delay: {}s)",
              POLL_INTERVAL_SECS, STARTUP_DELAY_SECS);

        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;

        loop {
            ctx.wait_if_paused().await;

            match process_pending(&state).await {
                Ok(count) => {
                    if count > 0 {
                        info!(count, "Retro worker: analyzed sessions");
                        ctx.record_success();
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Retro worker: processing error");
                    ctx.record_failure();
                }
            }

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    }
}

async fn process_pending(state: &AppState) -> anyhow::Result<usize> {
    let db = state.mission.db();

    let pending = db.get_sessions_needing_retrospective()
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    if pending.is_empty() {
        return Ok(0);
    }

    debug!(count = pending.len(), "Retro worker: found sessions needing analysis");

    let mut analyzed = 0;
    for (session_id, msg_count, tool_count, error_rate) in &pending {
        match analyze_session(state, session_id, *msg_count, *tool_count, *error_rate).await {
            Ok(_) => {
                analyzed += 1;
                // Rate limit between sessions
                tokio::time::sleep(Duration::from_secs(INTER_SESSION_DELAY_SECS)).await;
            }
            Err(e) => {
                warn!(session_id, error = %e, "Retro worker: session analysis failed");
            }
        }
    }

    Ok(analyzed)
}

async fn analyze_session(
    state: &AppState,
    session_id: &str,
    msg_count: i64,
    tool_count: i64,
    error_rate: f64,
) -> anyhow::Result<()> {
    let db = state.mission.db();

    // Circuit breaker: runtime check — skip meta-agent sessions even if they slipped through SQL
    if let Some(conv) = db.get_conversation(session_id).ok().flatten() {
        if conv.conversation_type != "user" {
            info!(session_id, conv_type = %conv.conversation_type, "Retro: skipping non-user session");
            return Ok(());
        }
        if let Some(ref slot) = conv.slot_id {
            if slot.starts_with("slot-memory") || slot.starts_with("slot-diagnosis") || slot.starts_with("agent-") {
                info!(session_id, slot_id = %slot, "Retro: skipping meta-agent session");
                return Ok(());
            }
        }
    }

    // Determine trigger reason
    let trigger = if error_rate > 25.0 {
        format!("error_rate_{:.0}%", error_rate)
    } else if msg_count > 100 {
        format!("msg_count_{}", msg_count)
    } else if tool_count > 50 {
        format!("tool_count_{}", tool_count)
    } else {
        "duration_1h+".to_string()
    };

    debug!(session_id, trigger = %trigger, "Retro worker: analyzing session");

    // ── L1: Rust quantitative analysis (detailed = quick + file heatmap + server map + error chains) ──
    let result = crate::handlers::retrospective::run_analysis(state, session_id, "detailed").await?;

    let stats_text = result.content.first()
        .map(|c| match c {
            missiond_mcp::tools::ToolContent::Text { text } => text.as_str(),
        })
        .unwrap_or("{}");

    // ── L2: MiniMax triage (all sessions) ──
    let full_analysis = match call_minimax_triage(state, session_id, stats_text).await {
        Ok(analysis) => {
            // Check severity + actionable for Board task creation
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&analysis) {
                let severity = parsed["severity"].as_str().unwrap_or("low");
                let actionable = parsed["actionable"].as_bool().unwrap_or(true); // default true for safety
                if (severity == "high" || severity == "critical") && actionable {
                    create_anomaly_board_task(state, session_id, &trigger, &parsed);
                }
            }
            Some(analysis)
        }
        Err(e) => {
            warn!(session_id, error = %e, "Retro worker: MiniMax triage failed, saving L1 only");
            None
        }
    };

    // Persist results
    db.save_retrospective_result(
        session_id,
        &trigger,
        stats_text,
        full_analysis.as_deref(),
    ).map_err(|e| anyhow::anyhow!("DB error saving retrospective: {}", e))?;

    info!(session_id, trigger = %trigger,
          has_triage = full_analysis.is_some(),
          "Retro worker: session analyzed");
    Ok(())
}

/// Call MiniMax M2.5 to triage the session analysis.
/// Returns structured JSON: { findings, recommendations, severity, summary }.
async fn call_minimax_triage(
    state: &AppState,
    session_id: &str,
    detailed_stats: &str,
) -> anyhow::Result<String> {
    let minimax = state.minimax.as_ref()
        .ok_or_else(|| anyhow::anyhow!("MiniMax gateway not available"))?;

    let prompt = format!(
        r#"你是 AI 工程效率分析师。分析以下 Claude Code 会话的量化数据，给出结构化诊断。

## 会话 ID
{session_id}

## 量化分析数据
{detailed_stats}

## 输出要求
严格返回 JSON（不要 markdown 围栏），格式：
{{
  "severity": "low|medium|high|critical",
  "actionable": true/false,
  "summary": "一句话概括（<50字）",
  "findings": [
    {{ "type": "waste|error|pattern|efficiency", "description": "发现描述", "evidence": "数据依据" }}
  ],
  "recommendations": [
    {{ "priority": "high|medium|low", "action": "建议动作" }}
  ]
}}

## 严重度判定标准
- critical: 错误率>50% 或 浪费比>70% 或 flailing 策略 ≥3 个
- high: 错误率>35% 或 浪费比>50% 或 blind_retry ≥5次 或 单文件改动>20次
- medium: 浪费比>30% 或 连续同一工具≥10次重复 或 单文件改动>10次
- low: 正常范围（浪费比<30%，无明显错误模式）

## 重要规则
- 百分比阈值（错误率/浪费比）仅在总调用次数>10时严格适用。调用<10次的会话最高判 medium
- 读操作（Read/Grep/Glob）的浪费可适度宽容（探索性搜索是正常行为）；写操作（Edit/Write/Bash）连续失败应优先升级
- 如果高浪费比是任务性质决定的正常试错（如在大型遗留项目中搜索），severity 不变但 actionable=false

只输出 JSON，不要解释。"#
    );

    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];

    minimax.call_briefing(messages, Some(MINIMAX_MAX_TOKENS), Some(session_id.to_string())).await
}

/// Create a Board task for high-severity anomaly sessions.
/// Sets assignee=slot-memory-slow + auto_execute=true so autopilot dispatches automatically.
/// All instructions go in description (NOT prompt_template, which would hide the triage data).
fn create_anomaly_board_task(
    state: &AppState,
    session_id: &str,
    trigger: &str,
    triage: &serde_json::Value,
) {
    let db = state.mission.db();

    let summary = triage["summary"].as_str().unwrap_or("异常会话需人工复盘");
    let severity = triage["severity"].as_str().unwrap_or("high");

    let title = format!("[异常诊断] {}", summary);
    let description = format!(
        "会话 `{session_id}` 触发异常检测 (trigger: {trigger}, severity: {severity})\n\n\
         MiniMax 初步分诊摘要:\n```json\n{triage_json}\n```\n\n\
         ## 执行指南\n\
         请按以下步骤执行深度异常复盘：\n\
         1. 调用 `mission_retrospective(sessionId=\"{session_id}\", depth=\"full\")` 获取 Gemini 深度分析\n\
         2. 调用 `mission_conversation_get(sessionId=\"{session_id}\", tail=15)` 查看关键消息\n\
         3. 总结根因(Root Cause)和改进建议\n\
         4. 调用 `mission_kb_remember(category=\"memory:debug\")` 将异常原因和结论记录到知识库\n\
         5. **判定修复状态**：\n\
            - 如果问题**已修复**（代码已有 fix）或**误报**（正常行为被误判）→ 在报告中注明，跳到步骤 7\n\
            - 如果问题**未修复**且可操作 → 创建修复子任务（步骤 6）\n\
         6. 为未修复问题创建子任务：调用 `mission_board_create(title=\"[待修复] 简述问题\", description=\"根因+修复建议\", parentId=\"{{TASK_ID}}\", priority=\"medium\", category=\"dev\", project=\"missiond\")`。注意：不要设置 assignee 和 autoExecute，由人工审核后决定执行\n\
         7. CRITICAL: 调用 board_note_add 写入诊断报告（含结论: resolved_no_action / repair_task_created / escalated_to_human），然后调用 board_update 设为 done",
        session_id = session_id,
        trigger = trigger,
        severity = severity,
        triage_json = serde_json::to_string_pretty(triage).unwrap_or_default(),
    );

    let dedupe = format!("retro-anomaly-{}", session_id);

    let input = missiond_core::types::CreateBoardTaskInput {
        title,
        description: Some(description),
        priority: Some(if severity == "critical" { "high" } else { "medium" }.to_string()),
        category: Some("diagnosis".to_string()),
        project: Some("missiond".to_string()),
        server: None,
        due_date: None,
        parent_id: None,
        assignee: Some("slot-diagnosis".to_string()),
        auto_execute: Some(true),
        prompt_template: None,  // Must be None — autopilot uses title+description as prompt
        hidden: None,
        flow_template: None,
        depends_on: None,
        dedupe_key: Some(dedupe),
    };

    match db.create_board_task(&input) {
        Ok(task) => info!(task_id = %task.id, session_id, "Retro worker: created anomaly Board task"),
        Err(e) => warn!(session_id, error = %e, "Retro worker: failed to create Board task"),
    }
}
