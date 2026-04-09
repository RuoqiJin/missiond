//! Timeline Analyst — L4 Proactive Agency.
//!
//! Periodically reads system_timeline, detects operational patterns,
//! calls Gemini for deep analysis, and creates Board tasks / KB entries.
//!
//! Pattern: Same as decision_harvest.rs — periodic Gemini + structured JSON + KB/Board write.
//! Cadence: 12h via autopilot_tick (persisted in daemon_state).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tracing::{debug, info, warn};

use crate::event_bus::{DaemonEvent, TraceContext};
use crate::llm_gateway::call_gemini_for_flow;
use crate::state::AppState;
use missiond_core::db::TimelineRow;

const ANALYSIS_INTERVAL_SECS: i64 = 12 * 3600; // 12 hours

/// Check if timeline analysis is due and run it.
/// Called from autopilot_tick every 60s.
pub(crate) async fn check_timeline_analysis(state: &AppState) {
    let now = chrono::Utc::now().timestamp();
    let last = state
        .store
        .daemon_state_get("last_timeline_analysis_at")
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last < ANALYSIS_INTERVAL_SECS {
        return;
    }
    // Do NOT update timestamp here — only after success (Gemini review requirement)

    info!("Timeline Analyst: starting periodic analysis");
    match run_analysis(state).await {
        Ok(count) => {
            info!(insights = count, "Timeline Analyst: analysis complete");
            let _ = state
                .store
                .daemon_state_set("last_timeline_analysis_at", now)
                .await;
        }
        Err(e) => {
            warn!(
                error = %e,
                "Timeline Analyst: analysis failed, will retry next tick"
            );
        }
    }
}

// ── Data Collection ──

struct AnalysisData {
    total_events: i64,
    by_type: Vec<(String, i64)>,
    traced_events: i64,
    unique_traces: i64,
    gemini_p50: i64,
    gemini_p90: i64,
    gemini_p99: i64,
    t1_hits: u64,
    t2_hits: u64,
    t3_dispatched: u64,
    error_events: Vec<TimelineRow>,
    slow_gemini: Vec<TimelineRow>,
}

async fn collect_analysis_data(state: &AppState) -> Result<AnalysisData> {
    // 1. Timeline stats (12h window)
    let stats = state
        .store
        .query_timeline_stats(Some("12h"), None)
        .await
        .map_err(|e| anyhow::anyhow!("timeline stats: {}", e))?;

    let (p50, p90, p99) = match &stats.gemini_latency {
        Some(l) => (l.p50_ms, l.p90_ms, l.p99_ms),
        None => (0, 0, 0),
    };

    // 2. Error events (LIMIT 20 — Gemini review: strict limits)
    let error_events = state
        .store
        .query_timeline_search("error", Some("12h"), None, 20)
        .await
        .unwrap_or_default();

    // 3. Slow Gemini requests (>60s, from recent gemini events, max 50 → filter → take 20)
    let all_gemini = state
        .store
        .query_timeline_filtered(
            Some("gemini_request_completed"),
            None,
            Some("12h"),
            None,
            50,
            0,
        )
        .await
        .unwrap_or_default();

    let slow_gemini: Vec<TimelineRow> = all_gemini
        .into_iter()
        .filter(|r| extract_duration_ms(r) > 60_000)
        .take(20)
        .collect();

    // 4. Decision stats from atomic counters
    let t1 = state.stats.decisions_tier1_hit.load(Ordering::Relaxed);
    let t2 = state.stats.decisions_tier2_hit.load(Ordering::Relaxed);
    let t3 = state
        .stats
        .decisions_tier3_dispatched
        .load(Ordering::Relaxed);

    Ok(AnalysisData {
        total_events: stats.total_events,
        by_type: stats.by_type,
        traced_events: stats.traced_events,
        unique_traces: stats.unique_traces,
        gemini_p50: p50,
        gemini_p90: p90,
        gemini_p99: p99,
        t1_hits: t1,
        t2_hits: t2,
        t3_dispatched: t3,
        error_events,
        slow_gemini,
    })
}

fn extract_duration_ms(row: &TimelineRow) -> i64 {
    serde_json::from_str::<serde_json::Value>(&row.payload)
        .ok()
        .and_then(|v| v.get("duration_ms")?.as_i64())
        .unwrap_or(0)
}

// ── Analysis Core ──

async fn run_analysis(state: &AppState) -> Result<usize> {
    let data = collect_analysis_data(state).await?;

    // Local pre-filter: skip Gemini if insufficient data
    if data.total_events < 10 && data.error_events.is_empty() {
        debug!("Timeline Analyst: insufficient data (<10 events, no errors), skipping");
        return Ok(0);
    }

    let prompt = build_analysis_prompt(&data);
    let response = call_gemini_for_flow(state, "timeline-analysis", &prompt).await?;
    let insights = parse_insights(&response);

    if insights.is_empty() {
        info!("Timeline Analyst: Gemini returned no actionable insights");
        return Ok(0);
    }

    let count = execute_insights(state, &insights).await;
    Ok(count)
}

// ── Prompt Construction ──

fn build_analysis_prompt(data: &AnalysisData) -> String {
    // Truncate event description to max chars (Gemini review: ≤500 chars/event)
    fn truncate_event(row: &TimelineRow, max_chars: usize) -> String {
        let payload_preview: String = row.payload.chars().take(300).collect();
        let s = format!(
            "[{}] {} | {}",
            row.event_type,
            row.summary.as_deref().unwrap_or("-"),
            payload_preview
        );
        s.chars().take(max_chars).collect()
    }

    let type_distribution: String = data
        .by_type
        .iter()
        .map(|(t, c)| format!("  {} = {}", t, c))
        .collect::<Vec<_>>()
        .join("\n");

    let gemini_info = if data.gemini_p50 > 0 {
        format!(
            "- Gemini 延迟: p50={}ms, p90={}ms, p99={}ms",
            data.gemini_p50, data.gemini_p90, data.gemini_p99
        )
    } else {
        "- Gemini 延迟: 无数据".to_string()
    };

    let total_decisions = data.t1_hits + data.t2_hits + data.t3_dispatched;

    let error_summary: String = if data.error_events.is_empty() {
        "（无）".to_string()
    } else {
        data.error_events
            .iter()
            .map(|r| format!("- {}", truncate_event(r, 500)))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let slow_summary: String = if data.slow_gemini.is_empty() {
        "（无）".to_string()
    } else {
        data.slow_gemini
            .iter()
            .map(|r| format!("- {}", truncate_event(r, 500)))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"你是 MissionD 系统运维分析师。当前时间: {}。

分析以下 12 小时时间轴数据，识别模式并提出改进建议。

## 事件统计
- 总事件数: {}
- 按类型分布:
{}
- 有因果链的事件: {} / 唯一链: {}
{}

## 决策路由 (累计)
- 总决策: {}
- T1(KB) 命中: {} / T2(Gemini): {} / T3(工位): {}

## 异常事件 ({} 条)
{}

## 慢 Gemini >60s ({} 条)
{}

输出 JSON 数组（0-5 条 Insight，只输出可操作的）：
[{{"category":"routing_pattern|performance|error_trend|kb_gap|workflow","priority":"high|medium|low","title":"简洁标题","description":"问题描述+证据","action":"create_task|update_kb|log_only","action_detail":"具体建议"}}]

规则：
- 无问题 → 返回空数组 []
- T1 miss 率 > 40% → 高优先级 KB 补充建议
- Gemini p90 > 120s → 性能告警
- 错误事件 > 5 → 趋势告警
- 只报告可操作的 insight，不报告"一切正常""#,
        chrono::Utc::now().to_rfc3339(),
        data.total_events,
        type_distribution,
        data.traced_events,
        data.unique_traces,
        gemini_info,
        total_decisions,
        data.t1_hits,
        data.t2_hits,
        data.t3_dispatched,
        data.error_events.len(),
        error_summary,
        data.slow_gemini.len(),
        slow_summary,
    )
}

// ── Insight Parsing ──

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Insight {
    category: String,
    priority: String,
    title: String,
    description: String,
    action: String,
    action_detail: String,
}

fn parse_insights(response: &str) -> Vec<Insight> {
    // Strip markdown fences (```json ... ```) — same pattern as decision_harvest.rs
    let json_text = if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            &response[start..=end]
        } else {
            return vec![];
        }
    } else {
        return vec![];
    };

    match serde_json::from_str::<Vec<Insight>>(json_text) {
        Ok(insights) => insights,
        Err(e) => {
            warn!(
                error = %e,
                "Timeline Analyst: failed to parse Gemini insights JSON"
            );
            vec![]
        }
    }
}

// ── Insight Execution ──

async fn execute_insights(state: &AppState, insights: &[Insight]) -> usize {
    let mut executed = 0;
    let trace_id = format!(
        "timeline-analysis-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M")
    );

    for insight in insights {
        // Publish InsightGenerated to Timeline (feedback loop: insights enter the timeline)
        state.event_bus.publish_traced(
            DaemonEvent::InsightGenerated {
                category: insight.category.clone(),
                priority: insight.priority.clone(),
                title: insight.title.clone(),
            },
            TraceContext {
                trace_id: Some(trace_id.clone()),
                summary: Some(format!("[L4] {}", insight.title)),
                ..Default::default()
            },
        );

        match insight.action.as_str() {
            "create_task" => {
                if !should_create_task(state, &insight.title).await {
                    debug!(
                        title = %insight.title,
                        "Timeline Analyst: skipping duplicate task"
                    );
                    continue;
                }

                match state.store.create_board_task(
                    &missiond_core::types::CreateBoardTaskInput {
                        title: format!("[Proactive] {}", insight.title),
                        description: Some(format!(
                            "## Timeline Insight (L4)\n\n{}\n\n## 建议行动\n\n{}\n\n---\n_trace: {}_",
                            insight.description, insight.action_detail, trace_id
                        )),
                        priority: Some(insight.priority.clone()),
                        category: Some("ops".to_string()),
                        project: None,
                        server: None,
                        due_date: None,
                        parent_id: None,
                        assignee: None,
                        auto_execute: None,
                        prompt_template: None,
                        hidden: None,
                        flow_template: None,
                        depends_on: None,
                        dedupe_key: None,
                        timeout_secs: None,
                        context_intent: None,
                    },
                ).await {
                    Ok(task) => {
                        info!(
                            task_id = %task.id,
                            title = %insight.title,
                            "Timeline Analyst: created proactive Board task"
                        );
                        executed += 1;
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            title = %insight.title,
                            "Timeline Analyst: failed to create Board task"
                        );
                    }
                }
            }
            "update_kb" => {
                let key = slug(&insight.title);
                match state
                    .store
                    .kb_remember(&missiond_core::types::KBRememberInput {
                        category: "ops:insight".to_string(),
                        key: format!("ops-insight-{}", key),
                        summary: insight.description.clone(),
                        detail: None,
                        source: Some("timeline-analyst".to_string()),
                        confidence: Some(0.7),
                        project_id: None,
                    })
                    .await
                {
                    Ok(result) => {
                        info!(
                            action = %result.action,
                            key = %insight.title,
                            "Timeline Analyst: KB updated"
                        );
                        executed += 1;
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            "Timeline Analyst: failed to update KB"
                        );
                    }
                }
            }
            _ => {
                // "log_only" or unknown — already recorded via publish_traced above
            }
        }
    }

    executed
}

// ── Dedup (Gemini review: tag + time-based) ──

async fn should_create_task(state: &AppState, title: &str) -> bool {
    // Check if a [Proactive] task with same title exists in last 48h
    let cutoff = (chrono::Utc::now() - chrono::TimeDelta::hours(48)).to_rfc3339();
    match state.store.list_board_tasks(Some("open"), false).await {
        Ok(tasks) => !tasks.iter().any(|t| {
            t.title.starts_with("[Proactive]") && t.created_at > cutoff && t.title.contains(title)
        }),
        Err(_) => true, // DB error → allow creation
    }
}

// ── Helpers ──

fn slug(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .take(60)
        .collect()
}
