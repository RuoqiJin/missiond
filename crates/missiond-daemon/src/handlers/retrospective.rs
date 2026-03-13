use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use missiond_mcp::tools::ToolResult;
use std::collections::HashMap;

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_retrospective_list" {
        return handle_list(state, args).await;
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        #[serde(alias = "session_id")]
        session_id: String,
        depth: Option<String>,
    }
    let Args { session_id, depth } = serde_json::from_value(args)?;
    let depth = depth.as_deref().unwrap_or("quick");
    let db = state.mission.db();

    // 1. Session meta
    let (total_calls, total_duration, unique_tools, compact_count) = db
        .get_retrospective_meta(&session_id)
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if total_calls == 0 {
        return Ok(ToolResult::json(&json!({
            "sessionId": session_id,
            "error": "No tool calls found for this session. Check if audit data has been ingested.",
        })));
    }

    // Short-circuit: insufficient data for meaningful analysis
    if total_calls < 5 {
        return Ok(ToolResult::json(&json!({
            "sessionId": session_id,
            "depth": "skipped",
            "meta": { "totalCalls": total_calls, "totalDurationMs": total_duration },
            "note": "Too few tool calls (<5) for meaningful retrospective analysis.",
        })));
    }

    // 2. Top tools
    let top_tools = db
        .get_retrospective_tool_stats(&session_id, 15)
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let top_tools_json: Vec<Value> = top_tools
        .iter()
        .map(|(name, count, success, error, avg_dur)| {
            json!({
                "name": name,
                "count": count,
                "success": success,
                "error": error,
                "avgDurationMs": (*avg_dur as i64),
            })
        })
        .collect();

    // 3. Time black holes (top turn durations from events)
    let events = db
        .get_conversation_events(&session_id, Some("turn_duration"), 500)
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut durations: Vec<(f64, String, String)> = events
        .iter()
        .filter_map(|e| {
            let raw: Value = serde_json::from_str(e.raw_data.as_deref()?).ok()?;
            let dur_ms = raw.get("duration_ms").and_then(|v| v.as_f64())?;
            let desc = e.content.clone().unwrap_or_default();
            let ts = e.timestamp.clone();
            Some((dur_ms, ts, desc))
        })
        .collect();
    durations.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    durations.truncate(5);
    let time_black_holes: Vec<Value> = durations
        .iter()
        .map(|(dur, ts, desc)| {
            json!({
                "durationMin": format!("{:.1}", dur / 60000.0),
                "durationMs": *dur as i64,
                "timestamp": ts,
                "description": desc,
            })
        })
        .collect();

    // 4. Consecutive repeat patterns (Gaps-and-Islands)
    let repeats = db
        .get_retrospective_repeat_patterns(&session_id, 3)
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let repeat_patterns: Vec<Value> = repeats
        .iter()
        .map(|(tool, streak, start, end)| {
            json!({
                "tool": tool,
                "streak": streak,
                "start": start,
                "end": end,
            })
        })
        .collect();

    // 5. N-Gram alternating patterns (Rust-side sliding window)
    let tool_seq = db
        .get_tool_name_sequence(&session_id)
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let ngram_patterns = detect_ngram_patterns(&tool_seq, 3);

    // 6. High error rate tools
    let high_error = db
        .get_retrospective_high_error_tools(&session_id, 10.0)
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let high_error_json: Vec<Value> = high_error
        .iter()
        .map(|(name, rate, count)| {
            json!({
                "name": name,
                "errorRate": rate,
                "count": count,
            })
        })
        .collect();

    // 7. Session outcome
    let conv_info = db.get_conversation(&session_id).ok().flatten();
    let session_status = conv_info.as_ref().map(|c| c.status.as_str()).unwrap_or("unknown");

    // 8. Waste score: repeat calls / total calls
    let total_repeat_calls: i64 = repeats.iter().map(|(_, streak, _, _)| streak).sum();
    let waste_ratio = if total_calls > 0 {
        (total_repeat_calls as f64) / (total_calls as f64) * 100.0
    } else {
        0.0
    };

    let mut result = json!({
        "sessionId": session_id,
        "depth": depth,
        "meta": {
            "totalCalls": total_calls,
            "totalDurationMs": total_duration,
            "uniqueTools": unique_tools,
            "compactCount": compact_count,
            "sessionStatus": session_status,
            "wasteRatio": format!("{:.1}%", waste_ratio),
        },
        "topTools": top_tools_json,
        "timeBlackHoles": time_black_holes,
        "repeatPatterns": repeat_patterns,
        "ngramPatterns": ngram_patterns,
        "highErrorTools": high_error_json,
        "analysis": null,
    });

    // full mode: call Gemini for qualitative analysis
    if depth == "full" {
        let analysis = run_full_analysis(state, &session_id, &result).await;
        result["analysis"] = match analysis {
            Ok(a) => a,
            Err(e) => json!({ "error": format!("{}", e) }),
        };
    }

    Ok(ToolResult::json_pretty(&result))
}

/// Detect N-Gram repeating subsequences (e.g., A-B-A-B patterns)
fn detect_ngram_patterns(seq: &[String], min_occurrences: usize) -> Vec<Value> {
    let mut results = Vec::new();

    for n in 2..=3 {
        if seq.len() < n {
            continue;
        }
        let mut counts: HashMap<Vec<&str>, usize> = HashMap::new();
        for window in seq.windows(n) {
            let key: Vec<&str> = window.iter().map(|s| s.as_str()).collect();
            *counts.entry(key).or_default() += 1;
        }
        for (pattern, count) in counts {
            if count >= min_occurrences {
                results.push(json!({
                    "pattern": pattern,
                    "n": n,
                    "occurrences": count,
                }));
            }
        }
    }

    results.sort_by(|a, b| {
        let ca = a["occurrences"].as_u64().unwrap_or(0);
        let cb = b["occurrences"].as_u64().unwrap_or(0);
        cb.cmp(&ca)
    });
    results.truncate(10);
    results
}

/// Truncate string at a safe UTF-8 char boundary
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Full analysis: compose prompt from aggregated data + Gemini
async fn run_full_analysis(state: &AppState, session_id: &str, stats: &Value) -> Result<Value> {
    let db = state.mission.db();

    // Get mission objective (first user message)
    let objective = db
        .get_first_user_message(session_id)
        .ok()
        .flatten()
        .map(|msg| {
            if msg.len() > 500 {
                format!("{}...", &msg[..500])
            } else {
                msg
            }
        })
        .unwrap_or_else(|| "(无法获取)".to_string());

    // Get error samples for high-error tools
    let mut error_samples = Vec::new();
    if let Some(tools) = stats["highErrorTools"].as_array() {
        for tool in tools.iter().take(3) {
            if let Some(name) = tool["name"].as_str() {
                if let Ok(samples) = db.get_tool_error_samples(session_id, name) {
                    for (input, output, ts) in &samples {
                        error_samples.push(json!({
                            "tool": name,
                            "timestamp": ts,
                            "input": safe_truncate(input, 500),
                            "output": safe_truncate(output, 1000),
                        }));
                    }
                }
            }
        }
    }

    // Get board task if linked
    let conv = db.get_conversation(session_id).ok().flatten();
    let task_title = conv
        .as_ref()
        .and_then(|c| c.task_id.as_deref())
        .and_then(|tid| db.get_board_task(tid).ok().flatten())
        .map(|t| t.title)
        .unwrap_or_default();

    // Compose prompt
    let prompt = format!(
        r#"## 会话复盘分析请求

### 会话目标
任务: {task_title}
用户首条消息: {objective}

### 聚合统计
- 工具总调用: {total_calls} 次, 耗时 {total_dur_min:.1} 分钟, 去重工具 {unique} 个
- 上下文压缩: {compact} 次
- 浪费比: {waste}
- 会话结局: {outcome}

### 工具调用 Top 5
{top_tools}

### 重复模式 (连续 3+ 次)
{repeats}

### N-Gram 交替模式
{ngrams}

### 高错误率工具
{errors}

### 错误样本
{error_samples_str}

请分析:
1. 主要弯路及根因 (每个弯路 1 行)
2. 时间黑洞的优化建议
3. 可固化为 MCP 工具或自动化的重复操作
4. 对 MissionD 基建的改进建议

返回 JSON: {{"findings": ["..."], "recommendations": ["..."], "automatable": ["..."]}}"#,
        task_title = if task_title.is_empty() { "(无关联任务)" } else { &task_title },
        objective = objective,
        total_calls = stats["meta"]["totalCalls"],
        total_dur_min = stats["meta"]["totalDurationMs"].as_i64().unwrap_or(0) as f64 / 60000.0,
        unique = stats["meta"]["uniqueTools"],
        compact = stats["meta"]["compactCount"],
        waste = stats["meta"]["wasteRatio"].as_str().unwrap_or("0%"),
        outcome = stats["meta"]["sessionStatus"].as_str().unwrap_or("unknown"),
        top_tools = serde_json::to_string_pretty(&stats["topTools"]).unwrap_or_default(),
        repeats = serde_json::to_string_pretty(&stats["repeatPatterns"]).unwrap_or_default(),
        ngrams = serde_json::to_string_pretty(&stats["ngramPatterns"]).unwrap_or_default(),
        errors = serde_json::to_string_pretty(&stats["highErrorTools"]).unwrap_or_default(),
        error_samples_str = serde_json::to_string_pretty(&error_samples).unwrap_or_default(),
    );

    // Call Gemini via router_chat handler
    let chat_args = json!({
        "message": prompt,
        "max_tokens": 4096,
        "idle_timeout": 120,
    });
    let chat_result = super::router_chat::handle(state, "mission_router_chat", chat_args).await?;

    // Extract response text from ToolResult
    let response_text = chat_result.content.first()
        .map(|c| match c {
            missiond_mcp::tools::ToolContent::Text { text } => text.as_str(),
        })
        .unwrap_or("{}");

    // Try to parse structured JSON from Gemini response
    let analysis: Value = extract_json_from_response(response_text);
    Ok(analysis)
}

/// Extract JSON object from LLM response (handles markdown code blocks)
fn extract_json_from_response(text: &str) -> Value {
    // Try direct parse
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return v;
    }
    // Try extracting from ```json ... ```
    if let Some(start) = text.find("```json") {
        let rest = &text[start + 7..];
        if let Some(end) = rest.find("```") {
            if let Ok(v) = serde_json::from_str::<Value>(&rest[..end]) {
                return v;
            }
        }
    }
    // Try extracting from ``` ... ```
    if let Some(start) = text.find("```") {
        let rest = &text[start + 3..];
        if let Some(end) = rest.find("```") {
            let inner = rest[..end].trim();
            if let Ok(v) = serde_json::from_str::<Value>(inner) {
                return v;
            }
        }
    }
    // Try finding { ... } block
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if let Ok(v) = serde_json::from_str::<Value>(&text[start..=end]) {
                return v;
            }
        }
    }
    // Fallback: wrap raw text
    json!({ "rawResponse": text })
}

/// Handle mission_retrospective_list
async fn handle_list(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct Args {
        limit: Option<i64>,
    }
    let limit = serde_json::from_value::<Args>(args)
        .ok()
        .and_then(|a| a.limit)
        .unwrap_or(10);

    let db = state.mission.db();
    let results = db.list_retrospective_results(limit)
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let items: Vec<Value> = results.iter().map(|(sid, trigger, stats, full, created)| {
        // Parse quick_stats to extract key metrics for summary
        let stats_val: Value = serde_json::from_str(stats).unwrap_or_default();
        json!({
            "sessionId": sid,
            "trigger": trigger,
            "createdAt": created,
            "hasFull": full.is_some(),
            "meta": stats_val.get("meta"),
        })
    }).collect();

    Ok(ToolResult::json_pretty(&json!({
        "results": items,
        "count": items.len(),
    })))
}
