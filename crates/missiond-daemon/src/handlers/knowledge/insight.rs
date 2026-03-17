use anyhow::Result;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let section = args
        .get("section")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    // Read strategic-state from KB
    let entry = match state.store.kb_get("strategic-state").await {
        Ok(Some(e)) => e,
        Ok(None) => return Ok(ToolResult::text("尚无战略分析数据。StrategyWorker 需要分析至少一个完成的会话后才会生成。")),
        Err(e) => return Ok(ToolResult::error(format!("KB 读取失败: {}", e))),
    };

    let state_json = match &entry.detail {
        Some(v) => v,
        None => return Ok(ToolResult::text("strategic-state 条目存在但 detail 为空。")),
    };

    // Render human-readable Markdown report
    let report = render_report(state_json, section);
    Ok(ToolResult::text(report))
}

fn render_report(state: &Value, section: &str) -> String {
    let mut out = String::from("# MissionD 战略认知报告\n\n");

    let snapshot_at = state.get("snapshot_at").and_then(|v| v.as_str()).unwrap_or("未知");
    let last_session = state.get("last_session").and_then(|v| v.as_str()).unwrap_or("未知");
    out.push_str(&format!("*更新时间: {} | 上次分析会话: {}*\n\n", snapshot_at, last_session));

    if section == "all" || section == "profile" {
        render_profile(&mut out, state);
    }
    if section == "all" || section == "trajectory" {
        render_trajectory(&mut out, state);
    }
    if section == "all" || section == "patterns" {
        render_patterns(&mut out, state);
    }
    if section == "all" || section == "proposals" {
        render_proposals(&mut out, state);
    }
    if section == "all" || section == "friction" {
        render_friction(&mut out, state);
        render_anti_patterns(&mut out, state);
    }

    out
}

fn render_profile(out: &mut String, state: &Value) {
    out.push_str("## 用户画像\n");
    if let Some(profiles) = state.get("user_profile").and_then(|v| v.as_array()) {
        if profiles.is_empty() {
            out.push_str("（暂无数据）\n\n");
            return;
        }
        for p in profiles {
            let trait_ = p.get("trait").and_then(|v| v.as_str()).unwrap_or("?");
            let confidence = p.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let bar = if confidence >= 0.9 { "●●●" } else if confidence >= 0.7 { "●●○" } else { "●○○" };
            out.push_str(&format!("- {} [{}]\n", trait_, bar));
        }
    } else {
        out.push_str("（暂无数据）\n");
    }
    out.push('\n');
}

fn render_trajectory(out: &mut String, state: &Value) {
    out.push_str("## 开发轨迹\n");
    if let Some(traj) = state.get("development_trajectory") {
        if let Some(focus) = traj.get("current_focus").and_then(|v| v.as_str()) {
            out.push_str(&format!("- **当前焦点**: {}\n", focus));
        }
        if let Some(goals) = traj.get("inferred_goals").and_then(|v| v.as_array()) {
            for goal in goals {
                if let Some(g) = goal.as_str() {
                    out.push_str(&format!("- 推测目标: {}\n", g));
                }
            }
        }
        if let Some(shifts) = traj.get("recent_shifts").and_then(|v| v.as_array()) {
            if !shifts.is_empty() {
                out.push_str("- 近期方向变化:\n");
                for shift in shifts {
                    if let Some(s) = shift.as_str() {
                        out.push_str(&format!("  - {}\n", s));
                    }
                }
            }
        }
    } else {
        out.push_str("（暂无数据）\n");
    }
    out.push('\n');
}

fn render_patterns(out: &mut String, state: &Value) {
    out.push_str("## 协作模式\n");
    if let Some(patterns) = state.get("collaboration_patterns").and_then(|v| v.as_array()) {
        if patterns.is_empty() {
            out.push_str("（暂无数据）\n\n");
            return;
        }
        for p in patterns {
            let pat = p.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let ptype = p.get("type").and_then(|v| v.as_str()).unwrap_or("neutral");
            let icon = match ptype {
                "positive" => "✅",
                "negative" => "❌",
                _ => "○",
            };
            let count = p.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
            if count > 0 {
                out.push_str(&format!("{} {} ({}次)\n", icon, pat, count));
            } else {
                out.push_str(&format!("{} {}\n", icon, pat));
            }
        }
    } else {
        out.push_str("（暂无数据）\n");
    }
    out.push('\n');
}

fn render_proposals(out: &mut String, state: &Value) {
    out.push_str("## 工作流提案\n");
    if let Some(proposals) = state.get("workflow_proposals").and_then(|v| v.as_array()) {
        if proposals.is_empty() {
            out.push_str("（暂无数据）\n\n");
            return;
        }
        out.push_str("| 动作 | 出现次数 | 状态 |\n|------|---------|------|\n");
        for p in proposals {
            let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            let occ = p.get("occurrences").and_then(|v| v.as_i64()).unwrap_or(0);
            let status = p.get("status").and_then(|v| v.as_str()).unwrap_or("proposed");
            let status_icon = match status {
                "automated" => "🤖 已自动化",
                "skill_generated" => "📋 已生成Skill",
                _ => "💡 提案中",
            };
            out.push_str(&format!("| {} | {} | {} |\n", action, occ, status_icon));
        }
    } else {
        out.push_str("（暂无数据）\n");
    }
    out.push('\n');
}

fn render_friction(out: &mut String, state: &Value) {
    out.push_str("## 摩擦点\n");
    if let Some(frictions) = state.get("friction_points").and_then(|v| v.as_array()) {
        if frictions.is_empty() {
            out.push_str("（暂无数据）\n\n");
            return;
        }
        for f in frictions {
            let issue = f.get("issue").and_then(|v| v.as_str()).unwrap_or("?");
            let severity = f.get("severity").and_then(|v| v.as_str()).unwrap_or("medium");
            let freq = f.get("frequency").and_then(|v| v.as_i64()).unwrap_or(0);
            let icon = match severity {
                "high" => "🔴",
                "medium" => "🟡",
                _ => "🟢",
            };
            out.push_str(&format!("{} {} ({}次, {})\n", icon, issue, freq, severity));
        }
    } else {
        out.push_str("（暂无数据）\n");
    }
    out.push('\n');
}

fn render_anti_patterns(out: &mut String, state: &Value) {
    out.push_str("## 反面模式\n");
    if let Some(anti) = state.get("anti_patterns").and_then(|v| v.as_array()) {
        if anti.is_empty() {
            out.push_str("（暂无数据）\n\n");
            return;
        }
        for a in anti {
            let rule = a.get("rule").and_then(|v| v.as_str()).unwrap_or("?");
            out.push_str(&format!("🚫 {}\n", rule));
        }
    } else {
        out.push_str("（暂无数据）\n");
    }
    out.push('\n');
}
