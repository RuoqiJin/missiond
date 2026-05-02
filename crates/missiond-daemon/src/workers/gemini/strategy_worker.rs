//! Strategic Analysis Worker — analyzes conversation logs to discover patterns,
//! user preferences, collaboration friction, and architectural insights.
//!
//! **Architecture**: Workspace-based agentic analysis. Instead of stuffing all
//! context into the prompt, we write session data to a temporary workspace
//! directory and let Gemini CLI use its built-in tools (read_file, grep_search)
//! to selectively explore the data.
//!
//! **Key design principle**: Completely stateless per call. Each analysis is an
//! independent Gemini request. The worker's "memory" lives entirely in the
//! Strategic State JSON stored in KB (key: `strategic-state`).
//!
//! Design doc: `docs/designs/arch-maintenance-and-strategic-analysis.md`

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, LazyLock};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use crate::state::AppState;
use missiond_core::event::events::{MemoryEvent, SystemEvent};

use super::{BackgroundWorker, WorkerContext, WorkerKind};

/// Analysis version — bump to re-analyze all sessions with a new schema.
const STRATEGY_ANALYSIS_VERSION: i32 = 2; // v2: workspace-based agentic analysis
/// Max analysis retries before giving up on a session.
const MAX_ANALYSIS_RETRIES: i32 = 3;
/// Max chars per JSONL content field — prevents grep single-line explosion.
const MAX_CONTENT_CHARS: usize = 5000;
static RE_ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());
static RE_BASE64: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"data:[a-zA-Z/]+;base64,[A-Za-z0-9+/=]{100,}").unwrap());

/// Find and analyze all pending sessions.
/// Called by session_reflection_consumer on SessionCompleted events.
pub(crate) async fn run_pending_analysis(state: &AppState) {
    // Kill switch: daemon_state key "strategy_analyst_enabled" (default: enabled)
    // Set to 0 to disable: INSERT OR REPLACE INTO daemon_state(key,value) VALUES('strategy_analyst_enabled','0')
    if state
        .store
        .daemon_state_get("strategy_analyst_enabled")
        .await
        .unwrap_or(None)
        .map(|v| v == 0)
        .unwrap_or(false)
    {
        debug!("strategy_analyst: disabled via flag, skipping");
        return;
    }

    // Use existing deep analysis infrastructure to find pending sessions
    let pending = match state
        .store
        .get_pending_deep_analysis(STRATEGY_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "strategy_analyst: failed to query pending sessions");
            return;
        }
    };

    if pending.is_empty() {
        return;
    }

    info!(
        count = pending.len(),
        "strategy_analyst: found pending sessions"
    );

    for conv in pending {
        let session_id = &conv.id;
        let watermark = if conv.deep_analyzed_message_id > 0 {
            Some(conv.deep_analyzed_message_id)
        } else {
            None
        };

        // Workspace-based analysis: no chunking needed — Gemini CLI reads files selectively
        let result = analyze_session_stateless(state, session_id, watermark).await;

        match result {
            Ok(analyzed_up_to) => {
                info!(session = %session_id, up_to = analyzed_up_to, "strategy_analyst: session analyzed");

                // Mark analysis complete or update checkpoint
                if conv.status == "active" {
                    // Active session: update watermark for incremental
                    if let Err(e) = state
                        .store
                        .update_deep_checkpoint(session_id, analyzed_up_to)
                        .await
                    {
                        warn!(error = %e, "strategy_analyst: failed to update checkpoint");
                    }
                } else {
                    // Completed/compacted: mark fully analyzed
                    if let Err(e) = state
                        .store
                        .mark_analysis_complete(session_id, STRATEGY_ANALYSIS_VERSION)
                        .await
                    {
                        warn!(error = %e, "strategy_analyst: failed to mark complete");
                    }
                    // Emit DeepAnalysisCompleted for KB consolidation consumer
                    let _ = state
                        .bus
                        .publish_memory(MemoryEvent::DeepAnalysisCompleted {
                            session_id: session_id.to_string(),
                            kb_entries_created: 0,
                        })
                        .await;
                }
                // (WorkerContext stats removed — consumer-driven now)
            }
            Err(e) => {
                warn!(error = %e, session = %session_id, "strategy_analyst: analysis failed");
                let _ = state.store.mark_analysis_failed(session_id).await;
                // (WorkerContext stats removed — consumer-driven now)
            }
        }
    }
}

// ── Workspace Preparation ──

/// TASK.md template — analysis instructions for Gemini CLI.
/// Gemini reads this file first, then uses tools (read_file, grep_search)
/// to explore session data and produce JSON output.
const STRATEGY_TASK_TEMPLATE: &str = r#"# 战略分析任务

你是一个顶尖的系统架构师和协作分析师。

## 数据文件
- `state.json` — 当前累积的战略状态快照（上一次分析结果）
- `session.jsonl` — 原始会话记录（JSON Lines 格式，每行含 id/role/content 字段）
- `meta.json` — 会话元数据（消息数、角色分布、字节大小）

## 工作流指令
1. 读 `meta.json` 了解会话规模
2. 读 `state.json` 了解当前战略状态
3. 用 grep_search 检索 session.jsonl 中的关键模式：
   - 偏好纠正：`"不要"`, `"别用"`, `"改成"`, `"以后请"`, `"stop"`, `"don't"`
   - 冲突与重试：`"error"`, `"failed"`, `"重试"`, `"又报错"`, `"timeout"`
   - 架构演进：`"deploy"`, `"架构"`, `"重构"`, `"refactor"`, `"migration"`
   - 重复工作流：高频出现的工具名或命令模式
4. 对 grep 命中的区域，用 read_file 读取前后上下文
5. 综合分析后输出 JSON

## 严格约束 (CRITICAL)
- **只读原则**：只能执行读取和检索操作，严禁修改、创建或删除任何文件
- **输出格式**：分析完成后，最后一次输出必须是纯 JSON。绝不能包含 markdown 代码块标记，不能有前言或后语

## 期望输出 (Schema)
```
{
  "user_profile": [
    {"trait": "描述", "confidence": 0.9, "source": "session-id"}
  ],
  "development_trajectory": {
    "current_focus": "当前开发方向",
    "recent_shifts": ["方向变化"],
    "inferred_goals": ["推测目标"]
  },
  "collaboration_patterns": [
    {"pattern": "描述", "type": "positive|negative", "count": 1}
  ],
  "workflow_proposals": [
    {"action": "描述", "occurrences": 3, "status": "proposed"}
  ],
  "friction_points": [
    {"issue": "描述", "frequency": 2, "severity": "high|medium|low"}
  ],
  "architectural_drifts": [
    {"description": "描述", "affected_area": "模块/组件"}
  ],
  "active_communication": {
    "should_notify": false,
    "message": ""
  }
}
```

规则：
- user_profile 上限 20 条，与 state.json 合并（更新已有、删除过时、新增发现）
- 如果没有有意义的发现，输出 `{}`
"#;

/// Prepare a workspace directory with session data for Gemini CLI to explore.
/// Returns TempDir (RAII: auto-cleaned on drop) and last message ID.
async fn prepare_workspace(
    state: &AppState,
    session_id: &str,
    since_id: Option<i64>,
) -> Result<(tempfile::TempDir, i64)> {
    let messages = state
        .store
        .get_conversation_messages(session_id, since_id, 50000)
        .await?;
    if messages.is_empty() {
        return Err(anyhow!("No messages found for session {}", session_id));
    }
    let last_id = messages.last().unwrap().id;

    // RAII temp directory — cleaned on drop (even on panic/process exit)
    let base = missiond_core::default_mission_home().join("tmp");
    std::fs::create_dir_all(&base)?;
    let workspace = tempfile::Builder::new()
        .prefix(&format!(
            "strategy-{}-",
            &session_id[..8.min(session_id.len())]
        ))
        .tempdir_in(&base)
        .map_err(|e| anyhow!("Failed to create strategy workspace: {}", e))?;

    // Write session.jsonl (noise-stripped + content-truncated)
    let jsonl_path = workspace.path().join("session.jsonl");
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(&jsonl_path)
            .map_err(|e| anyhow!("Failed to create session.jsonl: {}", e))?,
    );
    let mut role_counts: HashMap<String, u64> = HashMap::new();
    for msg in &messages {
        let cleaned = strip_noise(&msg.content, &msg.role);
        let truncated = truncate_content(&cleaned, MAX_CONTENT_CHARS);
        let line = json!({"id": msg.id, "role": msg.role, "content": truncated});
        writeln!(file, "{}", line)?;
        *role_counts.entry(msg.role.clone()).or_default() += 1;
    }
    drop(file); // flush before metadata read

    // Write meta.json
    let byte_size = std::fs::metadata(&jsonl_path).map(|m| m.len()).unwrap_or(0);
    let meta = json!({
        "session_id": session_id,
        "message_count": messages.len(),
        "role_distribution": role_counts,
        "byte_size": byte_size,
    });
    std::fs::write(
        workspace.path().join("meta.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;

    // Write state.json (current strategic state from KB)
    let state_json = state
        .store
        .kb_get("strategic-state")
        .await?
        .and_then(|e| e.detail.map(|d| d.to_string()))
        .unwrap_or_else(|| "{}".to_string());
    std::fs::write(workspace.path().join("state.json"), &state_json)?;

    // Write TASK.md (analysis instructions)
    std::fs::write(workspace.path().join("TASK.md"), STRATEGY_TASK_TEMPLATE)?;

    info!(
        session = %session_id,
        msg_count = messages.len(),
        jsonl_bytes = byte_size,
        workspace = %workspace.path().display(),
        "strategy_analyst: workspace prepared"
    );

    Ok((workspace, last_id))
}

/// Truncate content to max_chars, preserving head and tail for context.
fn truncate_content(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let half = max_chars / 2;
    let head: String = s.chars().take(half).collect();
    let tail: String = s.chars().skip(count - half).collect();
    format!("{}…[截断 {} 字符]…{}", head, count - max_chars, tail)
}

// ── Noise Stripping ──

/// Mechanical noise removal. NOT summarization — user words and AI reasoning preserved verbatim.
fn strip_noise(content: &str, role: &str) -> String {
    // Rule 1: ANSI escape sequences (PTY color codes)
    let content = RE_ANSI.replace_all(content, "");

    // Rule 2: base64 images → placeholder
    let content = RE_BASE64.replace_all(&content, "[图片]");

    // Rule 3: Long tool outputs → semantic-aware truncation
    // Preserves error signals instead of blind positional head+tail truncation.
    // Gemini ARB: "保留操作头尾 + 中间错误信号行±1上下文"
    if role == "tool_result" && content.len() > 5000 {
        return truncate_tool_output(&content);
    }

    // Rules 4-5: User words + AI reasoning → preserve verbatim
    content.to_string()
}

/// Max characters per line — prevents webpack-minified megablobs from bypassing line-count checks.
const MAX_LINE_LENGTH: usize = 2000;

/// Semantic-aware truncation for tool outputs.
/// Head (command context) + Tail (final status/errors) + middle error signal lines with ±1 context.
fn truncate_tool_output(content: &str) -> String {
    // Step 1: Truncate individual mega-lines (e.g., minified JS, base64 leftovers)
    let lines: Vec<String> = content
        .lines()
        .map(|line| {
            let char_count = line.chars().count();
            if char_count > MAX_LINE_LENGTH {
                let truncated: String = line.chars().take(MAX_LINE_LENGTH).collect();
                format!("{}...[行截断]", truncated)
            } else {
                line.to_string()
            }
        })
        .collect();

    // Short output (≤80 lines): preserve fully
    if lines.len() <= 80 {
        return lines.join("\n");
    }

    // Head: first 15 lines (command + initial output context)
    let head = &lines[..15];
    // Tail: last 40 lines (final status, exit code, error summary — errors cluster at end)
    let tail_start = lines.len().saturating_sub(40);
    let tail = &lines[tail_start..];
    // Middle: extract lines with error signals + ±1 line context
    let middle = &lines[15..tail_start];

    let error_indices: Vec<usize> = middle
        .iter()
        .enumerate()
        .filter(|(_, line)| is_error_signal(line))
        .map(|(i, _)| i)
        .collect();

    // Deduplicate with ±1 context window, cap total at 60 lines
    let mut context_indices = std::collections::BTreeSet::new();
    for &idx in &error_indices {
        if idx > 0 {
            context_indices.insert(idx - 1);
        }
        context_indices.insert(idx);
        if idx + 1 < middle.len() {
            context_indices.insert(idx + 1);
        }
        if context_indices.len() >= 60 {
            break;
        }
    }

    // Assemble result
    let mut result = head.join("\n");
    if !context_indices.is_empty() {
        result.push_str(&format!(
            "\n[... 中间 {} 行折叠, 保留 {} 行错误信号+上下文 ...]\n",
            middle.len(),
            context_indices.len()
        ));
        for &idx in &context_indices {
            result.push_str(&middle[idx]);
            result.push('\n');
        }
    } else {
        result.push_str(&format!("\n[... {} 行正常输出折叠 ...]\n", middle.len()));
    }
    result.push_str(&tail.join("\n"));
    result
}

/// Check if a line contains error/diagnostic signals.
/// Covers: Rust (panic/error), Node.js (ERR!), Python (traceback/exception),
/// generic (failed/denied/timeout/refused/fatal/critical/abort).
fn is_error_signal(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || lower.contains("warning")
        || lower.contains("denied")
        || lower.contains("not found")
        || lower.contains("timeout")
        || lower.contains("refused")
        || lower.contains("exception")
        || lower.contains("fatal")
        || lower.contains("critical")
        || lower.contains("traceback")
        || lower.contains("err!")
        || lower.contains("abort")
        || lower.contains("syntaxerror")
}

// ── Analysis Execution ──

/// Run a single workspace-based analysis for a session.
/// Writes session data to a temp workspace, lets Gemini CLI explore with tools.
/// Returns the last analyzed message ID.
async fn analyze_session_stateless(
    state: &AppState,
    session_id: &str,
    since_id: Option<i64>,
) -> Result<i64> {
    info!(session = %session_id, since = ?since_id, "strategy_analyst: starting workspace-based analysis");

    // 1. Prepare workspace (TempDir — RAII auto-cleanup on drop)
    let (workspace, last_message_id) = prepare_workspace(state, session_id, since_id).await?;

    info!(
        session = %session_id,
        workspace = %workspace.path().display(),
        messages_up_to = last_message_id,
        "strategy_analyst: calling Gemini CLI with workspace"
    );

    // 2. Build prompt with absolute workspace path (PTY session has fixed cwd)
    let ws_path = workspace.path().display();
    let prompt = format!(
        "Read {ws_path}/TASK.md and follow the analysis workflow.\n\
         IMPORTANT: All files referenced in TASK.md are in {ws_path}/. \
         Use absolute paths (e.g. {ws_path}/state.json) when reading files.\n\
         Output only JSON."
    );

    // 3. Route through SlotManager → GeminiCliSlotManager → persistent Gemini PTY
    let content = state
        .slot_manager
        .execute("strategy_analyst", &prompt)
        .await?;

    info!(
        session = %session_id,
        response_chars = content.len(),
        "strategy_analyst: Gemini response received"
    );

    // 4. Parse and apply output
    let content = content.trim();
    if content == "{}" || content.is_empty() {
        debug!(session = %session_id, "strategy_analyst: no findings");
        return Ok(last_message_id);
    }

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
            Err(anyhow!("JSON parse failed: {}", e))
        }
    }
    // workspace TempDir drops here — auto-cleaned
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

fn default_confidence() -> f64 {
    0.8
}

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

    state
        .store
        .kb_remember(&missiond_core::types::KBRememberInput {
            category: "memory:architecture".to_string(),
            key: "strategic-state".to_string(),
            summary: "Strategic analysis state — user profile, dev trajectory, patterns"
                .to_string(),
            detail: Some(updated_state),
            source: Some("strategy_analyst".to_string()),
            confidence: Some(1.0),
            project_id: None,
        })
        .await?;

    // 2. Write individual user preferences to KB
    for pref in &output.user_profile {
        if pref.confidence >= 0.7 {
            let key = format!("strategic-pref-{}", slug(&pref.trait_));
            state
                .store
                .kb_remember(&missiond_core::types::KBRememberInput {
                    category: "preference".to_string(),
                    key,
                    summary: pref.trait_.clone(),
                    detail: Some(json!({
                        "confidence": pref.confidence,
                        "source": pref.source,
                    })),
                    source: Some("strategy_analyst".to_string()),
                    confidence: Some(pref.confidence),
                    project_id: None,
                })
                .await?;
        }
    }

    // 3. Create Board tasks for workflow proposals
    // Phase 2b: ≥5 occurrences → auto-generate Skill (auto_execute + assignee)
    // Phase 2a: ≥3 occurrences → human review task
    for proposal in &output.workflow_proposals {
        if proposal.occurrences >= 5 {
            // Phase 2b: High-confidence workflow → auto-dispatch to Agent for Skill generation
            let dedupe = format!("strategy-skill-gen-{}", slug(&proposal.action));
            let skill_slug = slug(&proposal.action);
            state
                .store
                .create_board_task(&missiond_core::types::CreateBoardTaskInput {
                    title: format!("自动生成 Skill: {}", truncate(&proposal.action, 50)),
                    description: Some(format!(
                        "战略分析发现此操作出现 {} 次，已达自动化阈值。\n\n\
                    请创建 Skill 文件 `~/.claude/skills/{}/SKILL.md`：\n\
                    1. 分析此工作流涉及的代码路径和工具\n\
                    2. 编写 frontmatter (name, description, allowed-tools)\n\
                    3. 编写 INDEX 表和关键章节\n\
                    4. 工作流描述: {}\n\n\
                    来源: session {}",
                        proposal.occurrences, skill_slug, proposal.action, session_id
                    )),
                    category: Some("dev".to_string()),
                    priority: Some("medium".to_string()),
                    assignee: Some("slot-memory-slow".to_string()),
                    auto_execute: Some(true),
                    dedupe_key: Some(dedupe),
                    project: Some("missiond".to_string()),
                    ..Default::default()
                })
                .await?;
            info!(action = %proposal.action, occurrences = proposal.occurrences,
                "strategy_analyst: auto-dispatching Skill generation task");
        } else if proposal.occurrences >= 3 {
            // Phase 2a: Moderate frequency → human review
            let dedupe = format!("strategy-workflow-{}", slug(&proposal.action));
            state
                .store
                .create_board_task(&missiond_core::types::CreateBoardTaskInput {
                    title: format!("工作流自动化: {}", proposal.action),
                    description: Some(format!(
                    "战略分析发现此操作出现 {} 次，建议固化为 Skill 或自动化。\n来源: session {}",
                    proposal.occurrences, session_id
                )),
                    category: Some("dev".to_string()),
                    priority: Some("medium".to_string()),
                    dedupe_key: Some(dedupe),
                    ..Default::default()
                })
                .await?;
        }
    }

    // 4. Create Board tasks for architectural drifts
    for drift in &output.architectural_drifts {
        let dedupe = format!("strategy-drift-{}", slug(&drift.description));
        state.store.create_board_task(&missiond_core::types::CreateBoardTaskInput {
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
        }).await?;
    }

    // 5. Log friction points as KB entries
    for friction in &output.friction_points {
        if friction.frequency >= 2 {
            let key = format!("strategy-friction-{}", slug(&friction.issue));
            state
                .store
                .kb_remember(&missiond_core::types::KBRememberInput {
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
                    project_id: None,
                })
                .await?;
        }
    }

    // 6. Phase 2c: Proactive communication — EventBus (WS/frontend) + Inbox (pull on next turn)
    //
    // Gemini ARB decision: NEVER send_fire_and_forget to user PTY slots.
    // Rationale: injecting into active terminal disrupts UX, wastes tokens, breaks UI state.
    // Instead: write to Inbox for pull-on-next-turn, broadcast to EventBus for frontend Toast.
    if let Some(comm) = &output.active_communication {
        if comm.should_notify {
            if let Some(msg) = &comm.message {
                info!(message = %msg, "strategy_analyst: proactive notification");

                // Path 1: bus → WS → frontend Toast/notification panel
                let _ = state
                    .bus
                    .publish_system(SystemEvent::InsightGenerated {
                        category: "strategy".to_string(),
                        priority: "medium".to_string(),
                        title: msg.clone(),
                    })
                    .await;

                // Path 2: Inbox → pulled by Context Pipeline on next user turn
                let formatted = format!("[战略洞察] {}", msg);
                let inbox_msg = missiond_core::types::InboxMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    task_id: "strategy_analyst".to_string(),
                    from_role: "system".to_string(),
                    content: formatted,
                    read: false,
                    created_at: chrono::Utc::now().timestamp_millis(),
                };
                if let Err(e) = state.store.insert_inbox_message(&inbox_msg).await {
                    warn!(error = %e, "strategy_analyst: failed to queue inbox message");
                } else {
                    info!("strategy_analyst: insight queued to inbox for next-turn injection");
                }
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

// ── BackgroundWorker wrapper ──────────────────────────────────────────

pub(crate) struct StrategyWorker {
    pub notify: Arc<Notify>,
}

impl BackgroundWorker for StrategyWorker {
    const KIND: WorkerKind = WorkerKind::Gemini;

    fn name(&self) -> &'static str {
        "strategy_worker"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        loop {
            self.notify.notified().await;
            ctx.wait_if_paused().await;
            run_pending_analysis(&state).await;
        }
    }
}
