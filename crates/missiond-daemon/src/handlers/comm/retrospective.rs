use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_retrospective_list" {
        return handle_list(state, args).await;
    }
    if name == "mission_retrospective_backfill" {
        return handle_backfill(state, args).await;
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

    run_analysis(state, &session_id, depth).await
}

/// Core analysis logic — called by both the MCP handler and retro_worker.
/// Factored out of `handle()` to break circular Send dependency
/// (handle → handle_backfill → spawn(backfill) → analyze_session → handle).
pub(crate) async fn run_analysis(
    state: &AppState,
    session_id: &str,
    depth: &str,
) -> Result<ToolResult> {
    // 1. Session meta
    let (total_calls, total_duration, unique_tools, compact_count) = state
        .store
        .get_retrospective_meta(&session_id)
        .await
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
    let top_tools = state
        .store
        .get_retrospective_tool_stats(&session_id, 15)
        .await
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
    let events = state
        .store
        .get_conversation_events(&session_id, Some("turn_duration"), 500)
        .await
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
    let repeats = state
        .store
        .get_retrospective_repeat_patterns(&session_id, 3)
        .await
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
    let tool_seq = state
        .store
        .get_tool_name_sequence(&session_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let ngram_patterns = detect_ngram_patterns(&tool_seq, 3);

    // 6. High error rate tools
    let high_error = state
        .store
        .get_retrospective_high_error_tools(&session_id, 10.0)
        .await
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
    let conv_info = state
        .store
        .get_conversation(&session_id)
        .await
        .ok()
        .flatten();
    let session_status = conv_info
        .as_ref()
        .map(|c| c.status.as_str())
        .unwrap_or("unknown");

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

    // detailed / full mode: fine-grained analysis dimensions
    if depth == "detailed" || depth == "full" {
        let detailed = run_detailed_analysis(state, &session_id).await;
        match detailed {
            Ok(d) => {
                result["fileHeatmap"] = d.file_heatmap;
                result["serverMap"] = d.server_map;
                result["errorRecoveryChains"] = d.error_chains;
                result["errorTypeDistribution"] = d.error_type_distribution;
            }
            Err(e) => {
                result["detailedError"] = json!(format!("{}", e));
            }
        }
    }

    // full mode: call Gemini for qualitative analysis (with detailed data if available)
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

// ============ Error Type Classification ============

/// Classify error type from tool output (heuristic, no DB schema change).
/// - system_error: MCP/IPC/network infrastructure failures (not Agent's fault)
/// - client_error: bad parameters, wrong paths (Agent can improve)
/// - business_error: compile/test/logic failures (needs human decision)
fn classify_error_type(output: &str) -> &'static str {
    let lower = output.to_lowercase();
    // System errors: infrastructure / MCP / IPC / network
    if lower.contains("no such tool")
        || lower.contains("mcp error")
        || lower.contains("tool_use_error")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("connection reset")
        || lower.contains("unexpected eof")
        || lower.contains("ipc error")
        || lower.contains("timed out")
        || lower.contains("daemon closed")
        || lower.contains("socket closed")
        || lower.contains("bad gateway")
        || lower.contains("rate limit")
    {
        return "system_error";
    }
    // Client errors: bad parameters, wrong paths
    if lower.contains("no such file or directory")
        || lower.contains("file not found")
        || lower.contains("is not unique")
        || lower.contains("not found in file")
        || lower.contains("missing required")
        || lower.contains("invalid argument")
        || lower.contains("does not exist")
    {
        return "client_error";
    }
    // Everything else: compile fails, test fails, logic errors
    "business_error"
}

/// Build error type distribution from tool calls
fn build_error_type_distribution(calls: &[(String, String, String, String)]) -> Value {
    let mut system: Vec<(&str, &str)> = Vec::new();
    let mut client: Vec<(&str, &str)> = Vec::new();
    let mut business: Vec<(&str, &str)> = Vec::new();

    for (tool_name, _input, output, status) in calls {
        if status != "error" {
            continue;
        }
        let etype = classify_error_type(output);
        let bucket = match etype {
            "system_error" => &mut system,
            "client_error" => &mut client,
            _ => &mut business,
        };
        bucket.push((tool_name.as_str(), output.as_str()));
    }

    let summarize = |items: &[(&str, &str)]| -> Value {
        if items.is_empty() {
            return json!(null);
        }
        let mut tool_counts: HashMap<&str, u32> = HashMap::new();
        for (tool, _) in items {
            *tool_counts.entry(tool).or_default() += 1;
        }
        let samples: Vec<Value> = items
            .iter()
            .take(3)
            .map(|(tool, output)| json!({ "tool": tool, "sample": safe_truncate(output, 100) }))
            .collect();
        json!({ "count": items.len(), "tools": tool_counts, "samples": samples })
    };

    let mut result = json!({});
    if !system.is_empty() {
        result["system_error"] = summarize(&system);
    }
    if !client.is_empty() {
        result["client_error"] = summarize(&client);
    }
    if !business.is_empty() {
        result["business_error"] = summarize(&business);
    }
    result
}

// ============ Detailed Analysis ============

struct DetailedAnalysis {
    file_heatmap: Value,
    server_map: Value,
    error_chains: Value,
    error_type_distribution: Value,
}

/// Build detailed analysis: file heatmap, server map, error recovery chains
async fn run_detailed_analysis(state: &AppState, session_id: &str) -> Result<DetailedAnalysis> {
    let tool_calls = state
        .store
        .get_tool_calls_for_detailed_analysis(session_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let timeline = state
        .store
        .get_tool_calls_with_status_timeline(session_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let file_heatmap = build_file_heatmap(&tool_calls);
    let server_map = build_server_map(&tool_calls);
    let error_chains = build_error_recovery_chains(&timeline);
    let error_type_distribution = build_error_type_distribution(&tool_calls);

    Ok(DetailedAnalysis {
        file_heatmap,
        server_map,
        error_chains,
        error_type_distribution,
    })
}

/// Extract file paths from tool call input_summary and track operations
fn build_file_heatmap(calls: &[(String, String, String, String)]) -> Value {
    // Track per-file: reads, edits, writes, and reread-after-edit
    struct FileStats {
        reads: u32,
        edits: u32,
        writes: u32,
        rereads_after_edit: u32,
        last_op: String,
    }

    let mut files: BTreeMap<String, FileStats> = BTreeMap::new();

    for (tool_name, input_summary, _output, _status) in calls {
        let path = match tool_name.as_str() {
            "Read" | "read" => extract_file_path(input_summary),
            "Edit" | "edit" => extract_file_path(input_summary),
            "Write" | "write" => extract_file_path(input_summary),
            "Glob" | "glob" | "Grep" | "grep" => None, // skip search tools
            _ => None,
        };
        if let Some(path) = path {
            let short = shorten_path(&path);
            let stats = files.entry(short).or_insert(FileStats {
                reads: 0,
                edits: 0,
                writes: 0,
                rereads_after_edit: 0,
                last_op: String::new(),
            });
            match tool_name.as_str() {
                "Read" | "read" => {
                    if stats.last_op == "edit" || stats.last_op == "write" {
                        stats.rereads_after_edit += 1;
                    }
                    stats.reads += 1;
                    stats.last_op = "read".to_string();
                }
                "Edit" | "edit" => {
                    stats.edits += 1;
                    stats.last_op = "edit".to_string();
                }
                "Write" | "write" => {
                    stats.writes += 1;
                    stats.last_op = "write".to_string();
                }
                _ => {}
            }
        }
    }

    // Sort by total ops descending, take top 15
    let mut sorted: Vec<_> = files.into_iter().collect();
    sorted.sort_by(|a, b| {
        let total_a = a.1.reads + a.1.edits + a.1.writes;
        let total_b = b.1.reads + b.1.edits + b.1.writes;
        total_b.cmp(&total_a)
    });
    sorted.truncate(15);

    let items: Vec<Value> = sorted
        .iter()
        .map(|(path, s)| {
            let total = s.reads + s.edits + s.writes;
            let churn = if total > 0 {
                s.rereads_after_edit as f64 / total as f64
            } else {
                0.0
            };
            json!({
                "file": path,
                "reads": s.reads,
                "edits": s.edits,
                "writes": s.writes,
                "rereadsAfterEdit": s.rereads_after_edit,
                "totalOps": total,
                "churnRatio": format!("{:.2}", churn),
            })
        })
        .collect();
    json!(items)
}

/// Extract server/agent operations from xjp_agent_exec calls
fn build_server_map(calls: &[(String, String, String, String)]) -> Value {
    // Track per-server: command categories
    let mut servers: HashMap<String, HashMap<String, u32>> = HashMap::new();

    for (tool_name, input_summary, _output, _status) in calls {
        if !tool_name.contains("agent_exec")
            && !tool_name.contains("agent_file")
            && !tool_name.contains("agent_container")
            && !tool_name.contains("agent_service")
            && !tool_name.contains("agent_logs")
        {
            continue;
        }
        let server = extract_field(input_summary, "agent_url")
            .or_else(|| extract_field(input_summary, "agentUrl"))
            .or_else(|| extract_field(input_summary, "agent"))
            .unwrap_or_else(|| "unknown".to_string());

        let category = if tool_name.contains("container") {
            "docker"
        } else if tool_name.contains("service") {
            "service"
        } else if tool_name.contains("logs") {
            "logs"
        } else {
            // Classify command from input_summary
            let cmd = extract_field(input_summary, "command").unwrap_or_default();
            classify_command(&cmd)
        };

        *servers
            .entry(server)
            .or_default()
            .entry(category.to_string())
            .or_default() += 1;
    }

    let items: Vec<Value> = servers
        .iter()
        .map(|(server, cmds)| {
            let total: u32 = cmds.values().sum();
            json!({
                "server": server,
                "totalOps": total,
                "commands": cmds,
            })
        })
        .collect();
    json!(items)
}

/// Trace error→recovery sequences and classify strategies
fn build_error_recovery_chains(timeline: &[(String, String, String, String)]) -> Value {
    let mut chains: Vec<Value> = Vec::new();
    let mut i = 0;

    while i < timeline.len() {
        let (ref tool, ref status, ref input, ref ts) = timeline[i];
        if status != "error" {
            i += 1;
            continue;
        }

        // Found an error; trace recovery
        let error_tool = tool.clone();
        let error_ts = ts.clone();
        let error_input = safe_truncate(input, 200).to_string();
        let mut recovery_steps: Vec<Value> = Vec::new();
        let mut j = i + 1;
        let mut resolved = false;

        // Look ahead up to 20 steps for recovery
        while j < timeline.len() && j - i <= 20 {
            let (ref rt, ref rs, ref ri, ref rts) = timeline[j];
            recovery_steps.push(json!({
                "tool": rt,
                "status": rs,
                "input": safe_truncate(ri, 100),
                "timestamp": rts,
            }));
            // Resolution: same tool succeeds, or a different fix tool succeeds
            if rs == "success"
                && (rt == &error_tool
                    || rt == "Edit"
                    || rt == "Write"
                    || rt == "edit"
                    || rt == "write")
            {
                resolved = true;
                break;
            }
            j += 1;
        }

        let strategy = classify_recovery_strategy(&error_tool, &recovery_steps, resolved);
        let steps_count = recovery_steps.len();

        chains.push(json!({
            "errorTool": error_tool,
            "errorInput": error_input,
            "errorTimestamp": error_ts,
            "recoverySteps": steps_count,
            "resolved": resolved,
            "strategy": strategy,
        }));

        i = if resolved { j + 1 } else { j.max(i + 1) };
    }

    // Sort by recovery steps descending (costliest first), take top 10
    chains.sort_by(|a, b| {
        let sa = a["recoverySteps"].as_u64().unwrap_or(0);
        let sb = b["recoverySteps"].as_u64().unwrap_or(0);
        sb.cmp(&sa)
    });
    chains.truncate(10);
    json!(chains)
}

/// Classify recovery strategy
fn classify_recovery_strategy(error_tool: &str, steps: &[Value], resolved: bool) -> &'static str {
    if !resolved && steps.len() > 10 {
        return "flailing";
    }
    if !resolved {
        return "abandoned";
    }
    if steps.len() <= 2 {
        return "quick_fix";
    }
    // Check if it's a blind retry (same tool repeated)
    let retry_count = steps
        .iter()
        .filter(|s| s["tool"].as_str() == Some(error_tool))
        .count();
    if retry_count > steps.len() / 2 {
        return "blind_retry";
    }
    "diagnose_then_fix"
}

/// Extract file path from input_summary (e.g., "file_path: /foo/bar.rs, ...")
fn extract_file_path(input: &str) -> Option<String> {
    // Try "file_path: /..." pattern
    if let Some(idx) = input.find("file_path:") {
        let rest = &input[idx + 10..];
        let rest = rest.trim_start();
        let end = rest
            .find(',')
            .or_else(|| rest.find('}'))
            .unwrap_or(rest.len());
        let path = rest[..end].trim().trim_matches('"');
        if !path.is_empty() && path.starts_with('/') {
            return Some(path.to_string());
        }
    }
    // Try "path: /..." pattern
    if let Some(idx) = input.find("path:") {
        let rest = &input[idx + 5..];
        let rest = rest.trim_start();
        let end = rest
            .find(',')
            .or_else(|| rest.find('}'))
            .unwrap_or(rest.len());
        let path = rest[..end].trim().trim_matches('"');
        if !path.is_empty() && path.starts_with('/') {
            return Some(path.to_string());
        }
    }
    None
}

/// Extract a key-value field from input_summary
fn extract_field(input: &str, field: &str) -> Option<String> {
    let pattern = format!("{}:", field);
    if let Some(idx) = input.find(&pattern) {
        let rest = &input[idx + pattern.len()..];
        let rest = rest.trim_start();
        let end = rest
            .find(',')
            .or_else(|| rest.find('}'))
            .unwrap_or(rest.len());
        let val = rest[..end].trim().trim_matches('"').trim();
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }
    None
}

/// Shorten absolute paths for readability.
///
/// Strips the user's HOME prefix so log lines don't leak absolute filesystem
/// paths, and also strips `/home/` and `/Users/` so multi-host traces stay
/// compact.
fn shorten_path(path: &str) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path.strip_prefix(&*home_str) {
            return rest.trim_start_matches('/').to_string();
        }
    }
    for prefix in &["/home/", "/Users/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    path.to_string()
}

/// Classify a shell command into a category
fn classify_command(cmd: &str) -> &'static str {
    let cmd_lower = cmd.to_lowercase();
    if cmd_lower.contains("docker") || cmd_lower.contains("container") {
        "docker"
    } else if cmd_lower.contains("curl") || cmd_lower.contains("wget") {
        "http"
    } else if cmd_lower.contains("git") {
        "git"
    } else if cmd_lower.contains("cargo") || cmd_lower.contains("npm") || cmd_lower.contains("make")
    {
        "build"
    } else if cmd_lower.contains("cat") || cmd_lower.contains("ls") || cmd_lower.contains("find") {
        "inspect"
    } else if cmd_lower.contains("ssh") || cmd_lower.contains("scp") {
        "ssh"
    } else if cmd_lower.contains("systemctl") || cmd_lower.contains("service") {
        "service"
    } else {
        "other"
    }
}

/// Full analysis: compose prompt from aggregated data + Gemini
async fn run_full_analysis(state: &AppState, session_id: &str, stats: &Value) -> Result<Value> {
    // Get mission objective (first user message)
    let objective = state
        .store
        .get_first_user_message(session_id)
        .await
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
                if let Ok(samples) = state.store.get_tool_error_samples(session_id, name).await {
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
    let conv = state
        .store
        .get_conversation(session_id)
        .await
        .ok()
        .flatten();
    let task_title = if let Some(tid) = conv.as_ref().and_then(|c| c.task_id.as_deref()) {
        state
            .store
            .get_board_task(tid)
            .await
            .ok()
            .flatten()
            .map(|t| t.title)
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Build detailed sections for prompt (if available)
    let file_heatmap_str = stats
        .get("fileHeatmap")
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .filter(|s| s != "[]" && s != "null")
        .map(|s| format!("\n### 文件热力图（操作频次/Churn 比）\n{s}\n"))
        .unwrap_or_default();

    let server_map_str = stats
        .get("serverMap")
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .filter(|s| s != "[]" && s != "null")
        .map(|s| format!("\n### 服务器操作分布\n{s}\n"))
        .unwrap_or_default();

    let error_chains_str = stats
        .get("errorRecoveryChains")
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .filter(|s| s != "[]" && s != "null")
        .map(|s| format!("\n### 错误恢复链（策略分类）\n{s}\n"))
        .unwrap_or_default();

    let error_type_str = stats
        .get("errorTypeDistribution")
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .filter(|s| s != "{}" && s != "null")
        .map(|s| format!("\n### 错误类型分布（system/client/business）\n{s}\n"))
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
{file_heatmap_str}{server_map_str}{error_chains_str}{error_type_str}
请分析:
1. 主要弯路及根因 (每个弯路 1 行)
2. 时间黑洞的优化建议
3. 文件 Churn 高的根因及改进建议
4. 错误恢复策略评估（哪些是 flailing、哪些可自动化）
5. 可固化为 MCP 工具或自动化的重复操作
6. 对 MissionD 基建的改进建议

返回 JSON: {{"findings": ["..."], "recommendations": ["..."], "automatable": ["..."]}}"#,
        task_title = if task_title.is_empty() {
            "(无关联任务)"
        } else {
            &task_title
        },
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
    let response_text = chat_result
        .content
        .first()
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

    let results = state
        .store
        .list_retrospective_results(limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let items: Vec<Value> = results
        .iter()
        .map(|(sid, trigger, stats, full, created)| {
            // Parse quick_stats to extract key metrics for summary
            let stats_val: Value = serde_json::from_str(stats).unwrap_or_default();
            json!({
                "sessionId": sid,
                "trigger": trigger,
                "createdAt": created,
                "hasFull": full.is_some(),
                "meta": stats_val.get("meta"),
            })
        })
        .collect();

    Ok(ToolResult::json_pretty(&json!({
        "results": items,
        "count": items.len(),
    })))
}

/// Handle mission_retrospective_backfill — analyze ALL sessions since a given time.
async fn handle_backfill(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct Args {
        since: String,
    }
    let Args { since } = serde_json::from_value(args)?;

    // Count sessions first (before spawning)
    let count = state
        .store
        .get_sessions_for_retro_backfill(&since, false)
        .await
        .map(|v| v.len())
        .unwrap_or(0);

    if count == 0 {
        return Ok(ToolResult::text(
            "没有需要回填的会话（全部已有复盘结果或无符合条件的会话）。",
        ));
    }

    let msg = format!(
        "回填任务已启动（后台执行）。预计分析 {} 个会话（since: {}）。每个间隔 10s，总耗时约 {} 分钟。",
        count, since, (count * 10) / 60 + 1
    );

    // Spawn background task — calls retro_worker::backfill which uses run_analysis
    // (not handle), breaking the circular Send dependency.
    let state_clone = state.clone();
    let since_clone = since.clone();
    tokio::spawn(async move {
        match crate::workers::sonnet::retro_worker::backfill(&state_clone, &since_clone).await {
            Ok((analyzed, skipped)) => {
                tracing::info!(analyzed, skipped, since = %since_clone, "Retrospective backfill completed");
            }
            Err(e) => {
                tracing::warn!(error = %e, since = %since_clone, "Retrospective backfill failed");
            }
        }
    });

    Ok(ToolResult::text(&msg))
}
