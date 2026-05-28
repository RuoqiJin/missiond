//! Retrospective Worker — event-driven session analysis pipeline.
//!
//! L1: Rust quantitative analysis (quick+detailed) for ALL completed sessions.
//! L2: MiniMax M2.5 full-coverage triage — structured findings/severity.
//! High-severity sessions get Board tasks for manual deep-dive.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use crate::engine::control_plane_kernel::{ControlPlaneKernel, UpsertTaskContractCommand};
use crate::minimax_client::ChatMessage;
use crate::state::AppState;

use super::{BackgroundWorker, WorkerContext, WorkerKind};

/// Poll interval (legacy, kept for backfill reference).
#[allow(dead_code)]
const POLL_INTERVAL_SECS: u64 = 3600;

/// Backfill: analyze ALL sessions since a given time (no threshold filtering).
/// Returns (analyzed_count, skipped_count).
pub(crate) async fn backfill(state: &AppState, since: &str) -> anyhow::Result<(usize, usize)> {
    let sessions = state
        .store
        .get_sessions_for_retro_backfill(since, false)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    info!(
        count = sessions.len(),
        since, "Retro backfill: found sessions"
    );

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

/// Rate limit between session analyses (seconds).
const INTER_SESSION_DELAY_SECS: u64 = 10;

/// MiniMax max tokens for retrospective analysis.
const MINIMAX_MAX_TOKENS: u32 = 2000;

pub(crate) async fn process_pending(state: &AppState) -> anyhow::Result<usize> {
    let pending = state
        .store
        .get_sessions_needing_retrospective()
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    if pending.is_empty() {
        return Ok(0);
    }

    debug!(
        count = pending.len(),
        "Retro worker: found sessions needing analysis"
    );

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
                // Gemini ARB: record failure to prevent infinite retry loop
                let _ = state
                    .store
                    .save_retrospective_result(
                        session_id,
                        "error",
                        &format!("{{\"error\": \"{}\"}}", e.to_string().replace('"', "'")),
                        None,
                    )
                    .await;
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
    // Circuit breaker: runtime check — skip meta-agent sessions even if they slipped through SQL
    if let Some(conv) = state
        .store
        .get_conversation(session_id)
        .await
        .ok()
        .flatten()
    {
        if conv.conversation_type != "user" {
            info!(session_id, conv_type = %conv.conversation_type, "Retro: skipping non-user session");
            return Ok(());
        }
        if let Some(ref slot) = conv.slot_id {
            if slot.starts_with("slot-memory")
                || slot.starts_with("slot-diagnosis")
                || slot.starts_with("agent-")
            {
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
    let result =
        crate::handlers::retrospective::run_analysis(state, session_id, "detailed").await?;

    let stats_text = result
        .content
        .first()
        .map(|c| match c {
            missiond_mcp::tools::ToolContent::Text { text } => text.as_str(),
        })
        .unwrap_or("{}");

    // ── L2: Sonnet triage (all sessions) ──
    let mut triage_severity: Option<String> = None;
    let mut triage_actionable = false;
    let full_analysis = match call_sonnet_triage(state, session_id, stats_text).await {
        Ok(analysis) => {
            // Check severity + actionable for Board task creation
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&analysis) {
                let severity = parsed["severity"].as_str().unwrap_or("low");
                let actionable = parsed["actionable"].as_bool().unwrap_or(true); // default true for safety
                triage_severity = Some(severity.to_string());
                triage_actionable = actionable;
                if (severity == "high" || severity == "critical") && actionable {
                    create_anomaly_board_task(state, session_id, &trigger, &parsed).await;
                }
            }
            Some(analysis)
        }
        Err(e) => {
            warn!(session_id, error = %e, "Retro worker: Sonnet triage failed, saving L1 only");
            None
        }
    };

    // Phase 4b: Session-level utility_score feedback based on retro severity.
    // Gemini ARB: no double-penalty risk — retro only processes user sessions
    // (conversation_type="user"), while autopilot tasks have their own direct
    // feedback path via task_cited_kbs cache. kb_access_log session_ids differ
    // (user session ID vs autopilot task ID).
    if let Some(ref severity) = triage_severity {
        let cited_kb_ids = state
            .store
            .get_session_cited_kb_ids(session_id)
            .await
            .unwrap_or_default();
        if !cited_kb_ids.is_empty() {
            let feedback = match severity.as_str() {
                "low" => Some(true), // mild boost — session went well
                "high" | "critical" if triage_actionable => Some(false), // penalty — actionable failure
                _ => None, // medium or non-actionable — no adjustment
            };
            if let Some(success) = feedback {
                match state
                    .store
                    .kb_batch_apply_utility_feedback(&cited_kb_ids, success)
                    .await
                {
                    Ok(n) if n > 0 => info!(
                        session_id,
                        severity,
                        adjusted = n,
                        success,
                        "Retro 4b: session-level utility feedback applied"
                    ),
                    Err(e) => warn!(session_id, error = %e, "Retro 4b: utility feedback failed"),
                    _ => {}
                }
            }
        }
    }

    // Persist results
    state
        .store
        .save_retrospective_result(session_id, &trigger, stats_text, full_analysis.as_deref())
        .await
        .map_err(|e| anyhow::anyhow!("DB error saving retrospective: {}", e))?;

    info!(session_id, trigger = %trigger,
          has_triage = full_analysis.is_some(),
          "Retro worker: session analyzed");
    Ok(())
}

/// Call Sonnet to triage the session analysis.
/// Returns structured JSON: { findings, recommendations, severity, summary }.
async fn call_sonnet_triage(
    state: &AppState,
    session_id: &str,
    detailed_stats: &str,
) -> anyhow::Result<String> {
    let sonnet = state
        .sonnet
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Sonnet gateway not available"))?;

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

## 错误类型分类（如 errorTypeDistribution 字段存在）
数据中可能包含 `errorTypeDistribution` 字段，按错误归因分为三类：
- system_error: MCP/IPC/网络基础设施故障（非 Agent 的责任）
- client_error: 参数错误、路径不存在（Agent 可改进）
- business_error: 编译/测试/逻辑失败（业务问题）

## 严重度判定标准
- critical: 错误率>50% 或 浪费比>70% 或 flailing 策略 ≥3 个
- high: 错误率>35% 或 浪费比>50% 或 blind_retry ≥5次 或 单文件改动>20次
- medium: 浪费比>30% 或 连续同一工具≥10次重复 或 单文件改动>10次
- low: 正常范围（浪费比<30%，无明显错误模式）

## 重要规则
- 错误率阈值仅统计 client_error + business_error。system_error 单独在 findings 中报告但不升级 severity
- system_error 占总错误 >80% 时，actionable=false（Agent 无法改善基础设施问题，需独立的基建告警处理）
- 百分比阈值（错误率/浪费比）仅在总调用次数>10时严格适用。调用<10次的会话最高判 medium
- 读操作（Read/Grep/Glob）的浪费可适度宽容（探索性搜索是正常行为）；写操作（Edit/Write/Bash）连续失败应优先升级
- 如果高浪费比是任务性质决定的正常试错（如在大型遗留项目中搜索），severity 不变但 actionable=false

只输出 JSON，不要解释。"#
    );

    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];

    sonnet
        .call_briefing(
            messages,
            Some(MINIMAX_MAX_TOKENS),
            Some(session_id.to_string()),
        )
        .await
}

/// Create a Board task for high-severity anomaly sessions.
/// The task is review-only; any follow-up execution requires explicit delegation.
/// All instructions go in description (NOT prompt_template, which would hide the triage data).
async fn create_anomaly_board_task(
    state: &AppState,
    session_id: &str,
    trigger: &str,
    triage: &serde_json::Value,
) {
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
         4b. **Skill 更新建议**：如果异常与某个 Skill 的操作流程相关（如部署失败→deployment skill），调用 `mission_skill_search` 定位对应 Skill，然后在 Board note 中建议更新该 Skill 的 `# MUST` 或 `# TODO` 区\n\
         5. **判定修复状态**：\n\
            - 如果问题**已修复**（代码已有 fix）或**误报**（正常行为被误判）→ 在报告中注明，跳到步骤 9\n\
            - 如果问题**未修复**且可操作 → 创建调查+修复子任务（步骤 6-8）\n\
         6. 创建根因调查子任务：调用 `mission_board_create(title=\"[根因调查] 简述问题\", description=\"## 调查目标\\n简述需要定位的问题\\n\\n## 关键线索\\n从诊断中提取的错误消息、可疑代码路径、相关会话 ID\\n\\n## 调查步骤\\n1. mission_conversation_get 查看实际错误\\n2. 读相关 handler/DB 源码\\n3. 追踪错误产生路径\\n\\n## 约束\\nCRITICAL: 这是 INVESTIGATION ONLY 任务。禁止修改任何代码文件。只做调查和报告。\", parentId=\"{{TASK_ID}}\", category=\"investigation\", assignee=\"slot-coder-1\", autoExecute=true, priority=\"medium\", project=\"missiond\")`。记录返回的调查任务 ID\n\
         7. 创建待修复子任务：调用 `mission_board_create(title=\"[待修复] 简述问题\", description=\"根因+修复建议（调查完成后 DAG 自动注入调查报告）\", parentId=\"{{TASK_ID}}\", category=\"dev\", dependsOn=[步骤6返回的调查任务ID], priority=\"medium\", project=\"missiond\")`。注意：不要设置 assignee 和 autoExecute，由人工审核调查报告后决定\n\
         8. 确认两个子任务都已创建成功\n\
         9. 写入诊断报告（含结论: resolved_no_action / investigation_dispatched / escalated_to_human）。如果需要关闭任务，必须先写入 canonical task_result_artifact，再由 typed settle 完成。",
        session_id = session_id,
        trigger = trigger,
        severity = severity,
        triage_json = serde_json::to_string_pretty(triage).unwrap_or_default(),
    );

    let dedupe = format!("retro-anomaly-{}", session_id);
    let runtime_metadata = retro_anomaly_runtime_metadata(session_id, trigger, severity, &dedupe);

    let input = missiond_core::types::CreateBoardTaskInput {
        title,
        description: Some(description),
        priority: Some(
            if severity == "critical" {
                "high"
            } else {
                "medium"
            }
            .to_string(),
        ),
        category: Some("diagnosis".to_string()),
        project: Some("missiond".to_string()),
        server: None,
        due_date: None,
        parent_id: None,
        assignee: None,
        auto_execute: Some(false),
        prompt_template: None, // Must be None — autopilot uses title+description as prompt
        hidden: None,
        flow_template: None,
        depends_on: None,
        dedupe_key: Some(dedupe),
        timeout_secs: None,
        context_intent: None,
        runtime_metadata: Some(runtime_metadata),
    };

    match state.store.create_board_task(&input).await {
        Ok(task) => {
            upsert_retro_task_contract(state, &task).await;
            info!(task_id = %task.id, session_id, "Retro worker: created anomaly Board task")
        }
        Err(e) => warn!(session_id, error = %e, "Retro worker: failed to create Board task"),
    }
}

async fn upsert_retro_task_contract(state: &AppState, task: &missiond_core::types::BoardTask) {
    if let Err(err) = ControlPlaneKernel::new(state)
        .upsert_task_contract_command(UpsertTaskContractCommand {
            task_id: task.id.to_string(),
            project_id: task.project.clone(),
            runtime_metadata: task.runtime_metadata.clone(),
        })
        .await
    {
        warn!(
            error = %err,
            task_id = %task.id,
            "Retro worker: failed to upsert task_contracts for anomaly BoardTask"
        );
    }
}

fn retro_anomaly_runtime_metadata(
    session_id: &str,
    trigger: &str,
    severity: &str,
    dedupe_key: &str,
) -> Value {
    json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "retro_worker",
        "control_state": "task_contracts",
        "dispatch_metadata": {
            "task_class": "retro-anomaly-review",
            "session_id": session_id,
            "trigger": trigger,
            "severity": severity,
            "dedupe_key": dedupe_key,
            "completion_protocol": "review-only retrospective; provider prose cannot close tasks"
        },
        "read_scope": [
            format!("conversation-session:{session_id}"),
            "retrospective-analysis"
        ],
        "write_scope": [],
        "must_not_touch": [],
        "capability_grant_ids": [],
        "sandbox_profile": "system-learning-review",
        "projection_policy": "description_notes_are_projection_only"
    })
}

// ── BackgroundWorker wrapper ──────────────────────────────────────────

pub(crate) struct RetroWorker {
    pub notify: Arc<Notify>,
}

impl BackgroundWorker for RetroWorker {
    const KIND: WorkerKind = WorkerKind::Sonnet;

    fn name(&self) -> &'static str {
        "retro_worker"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        loop {
            self.notify.notified().await;
            ctx.wait_if_paused().await;
            ctx.begin_poll(Some(900));
            ctx.progress("processing pending retrospectives");
            if let Err(e) = process_pending(&state).await {
                ctx.record_failure();
                warn!(error = %e, "retro_worker: analysis failed");
            } else {
                ctx.record_success();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retro_anomaly_metadata_declares_task_contract_authority() {
        let metadata =
            retro_anomaly_runtime_metadata("session-1", "error-rate", "high", "retro-anomaly-1");
        assert_eq!(metadata["source"], "retro_worker");
        assert_eq!(metadata["control_state"], "task_contracts");
        assert_eq!(
            metadata["dispatch_metadata"]["task_class"],
            "retro-anomaly-review"
        );
        assert_eq!(metadata["dispatch_metadata"]["session_id"], "session-1");
        assert_eq!(metadata["write_scope"].as_array().unwrap().len(), 0);
        assert_eq!(metadata["sandbox_profile"], "system-learning-review");
    }
}
