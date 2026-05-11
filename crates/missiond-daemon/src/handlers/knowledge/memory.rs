use anyhow::{anyhow, Result};
use missiond_mcp::tools::{ToolError, ToolResult};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use tracing::info;

use crate::context::v3_blueprint_runtime::MemoryKbRuntimeConfig;
use crate::events_sync;
use crate::helpers::default_mission_home;
use crate::lenient;
use crate::state::AppState;
use crate::state::{CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES};
use crate::state::{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};

const MAX_PENDING_BATCH_REPLAYS: u32 = 3;

fn classify_memory_input_noise(role: &str, content: &str) -> Option<&'static str> {
    // User utterances are the source of truth for memory extraction. Keep them
    // even when they mention deployment, workers, or diagnostics.
    if role == "user" {
        return None;
    }

    let lower = content.to_ascii_lowercase();
    const DEPLOYMENT_MONITOR_NEEDLES: &[&str] = &[
        "deploy monitor",
        "deployment-monitor",
        "deployment-event-response",
        "deploy-center provenance",
        "xjp_build_wait",
        "xjp_deploy_watch",
        "xjp_deploy_status",
        "deploy_created",
        "build_started",
        "build_succeeded",
        "build_failed",
        "deploy_started",
        "deploy_succeeded",
        "deploy_failed",
        "smoke_succeeded",
        "smoke_failed",
        "rollback_started",
        "rollback_succeeded",
        "rollback_failed",
        "agent_heartbeat",
        "agent_update_started",
        "agent_update_succeeded",
        "agent_update_failed",
        "provenance_changed",
        "provenance_partial",
        "digest_resolution_failed",
        "reported_digest_missing",
        "runner_queued",
        "build_cache_unavailable",
    ];
    if DEPLOYMENT_MONITOR_NEEDLES
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return Some("deployment-monitor");
    }

    if lower.contains("lisp-code-sync")
        && (lower.contains("report")
            || lower.contains("watcher")
            || lower.contains("runtime/lisp-code-sync"))
    {
        return Some("runtime-report");
    }

    if lower.contains("matched skills")
        || lower.contains("board task id")
        || lower.contains("任务完成时")
        || lower.contains("completion protocol")
        || lower.contains("mission_board_update")
        || lower.contains("mission_board_note_add")
    {
        return Some("worker-instruction");
    }

    if lower.contains("## 预加载上下文") || lower.contains("preloaded context") {
        return Some("provider-preamble");
    }

    None
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_memory
    if name == "mission_memory" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        return match action {
            "pending" => handle_inner(state, "mission_memory_pending", args).await,
            "pause" => handle_inner(state, "mission_memory_pause", args).await,
            "token_stats" => {
                // Delegate to conversation handler which has mission_token_stats
                crate::handlers::conversation::handle(state, "mission_token_stats", args).await
            }
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

fn load_memory_kb_config() -> Result<MemoryKbRuntimeConfig> {
    MemoryKbRuntimeConfig::load_for_current_dir()
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Memory Extraction =====
        // Message-level pipeline tracking: returns pending messages with IDs.
        // State auto-committed by Daemon on extraction completion — no manual done() needed.
        "mission_memory_pending" | "mission_memory_pending_user" => {
            // De-bounce guard: if realtime extraction is in-flight and we already served
            // pending messages in this cycle, replay the cached batch a few times. This
            // keeps provider context compaction recoverable while still preventing a
            // tight polling loop from being mistaken for new work.
            {
                let mut es = state.extraction_state.write().await;
                if es.pending_served
                    && matches!(
                        es.phase,
                        crate::state::ExtractionPhase::Sending
                            | crate::state::ExtractionPhase::WaitingForSlotIdle
                    )
                {
                    if let Some(payload) = es.pending_payload.clone() {
                        if es.pending_replay_count < MAX_PENDING_BATCH_REPLAYS {
                            es.pending_replay_count += 1;
                            let batch_id = es
                                .pending_batch_id
                                .clone()
                                .unwrap_or_else(|| "unknown-batch".to_string());
                            let replay_count = es.pending_replay_count;
                            return Ok(ToolResult::text(&format!(
                                "[realtime-extract replay] batch={} replay={}/{}\n\
                                 这是一份已返回批次的缓存重放，用于恢复 provider context compaction 后丢失的上下文；请基于本批内容输出总结，不要继续轮询。\n\n{}",
                                batch_id, replay_count, MAX_PENDING_BATCH_REPLAYS, payload
                            )));
                        }
                    }
                    return Ok(ToolResult::structured_error(
                        ToolError::new(
                            "MEMORY_PENDING_ALREADY_SERVED",
                            "当前 realtime extraction 批次已经由 mission_memory_pending 返回过，且可重放缓存缺失或已达到重放上限。",
                        )
                        .with_suggestion(
                            "请基于上一轮已经返回或已重放的消息直接输出总结；水位线由系统在本轮完成后推进，下一批会自动调度。",
                        ),
                    ));
                }
            }

            let config = load_memory_kb_config()?;
            let pending_msg_limit = config.pending_message_limit;
            let pending = state
                .store
                .get_pending_realtime_messages_with_limit(pending_msg_limit)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if pending.is_empty() {
                return Ok(ToolResult::text("没有待分析的新对话内容。"));
            }

            let mut output = String::new();
            let mut all_msg_ids: Vec<i64> = Vec::new();
            let mut skip_counts: BTreeMap<&'static str, u32> = BTreeMap::new();
            let mut user_count = 0usize;
            for (session_id, project, msgs) in &pending {
                let mut session_output = String::new();
                for msg in msgs {
                    if let Some(reason) = classify_memory_input_noise(&msg.role, &msg.content) {
                        *skip_counts.entry(reason).or_insert(0) += 1;
                        continue;
                    }
                    all_msg_ids.push(msg.id);
                    if msg.role == "user" {
                        user_count += 1;
                        session_output.push_str(&format!(
                            "[#{}][{}] ★ user: {}\n\n",
                            msg.id, msg.timestamp, msg.content
                        ));
                    } else if msg.role == "tool_result" {
                        let max_chars = config.tool_result_preview_chars;
                        let content = if msg.content.len() > max_chars {
                            let end = events_sync::floor_char_boundary(&msg.content, max_chars);
                            format!("{}…({}字符)", &msg.content[..end], msg.content.len())
                        } else {
                            msg.content.clone()
                        };
                        session_output.push_str(&format!(
                            "[#{}][{}] tool_result: {}\n\n",
                            msg.id, msg.timestamp, content
                        ));
                    } else {
                        let max_chars = config.assistant_preview_chars;
                        let content = if msg.content.len() > max_chars {
                            let end = events_sync::floor_char_boundary(&msg.content, max_chars);
                            format!("{}…", &msg.content[..end])
                        } else {
                            msg.content.clone()
                        };
                        session_output.push_str(&format!(
                            "[#{}][{}] assistant: {}\n\n",
                            msg.id, msg.timestamp, content
                        ));
                    }
                }
                if !session_output.is_empty() {
                    output.push_str(&format!(
                        "## session: {} (project: {})\n\n",
                        session_id, project
                    ));
                    output.push_str(&session_output);
                }
            }
            if !skip_counts.is_empty() {
                let mut es = state.extraction_state.write().await;
                for (reason, count) in &skip_counts {
                    es.record_input_skip(reason, *count);
                }
            }

            let session_count = pending.len();
            let msg_count = all_msg_ids.len();
            let truncated_note = if msg_count >= pending_msg_limit {
                format!(
                    " ⚠️ 已达上限 {}，可能还有更多未显示的消息。处理完当前批次后系统将自动推送下一批。",
                    pending_msg_limit
                )
            } else {
                String::new()
            };
            let batch_id = format!("batch-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
            let skip_note = if skip_counts.is_empty() {
                String::new()
            } else {
                let parts = skip_counts
                    .iter()
                    .map(|(reason, count)| format!("{reason}={count}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "输入过滤诊断: 已跳过 {} 条噪声消息 ({parts})。\n",
                    skip_counts.values().sum::<u32>()
                )
            };
            let header = format!(
                "[realtime-extract] [{}] {} 个会话, {} 条消息 (其中 {} 条用户消息){}\n\
                 {}\
                 水位线由系统自动管理，处理完毕后直接输出总结即可，无需调用 done 工具。\n\n\
                 ★ = 用户原话，优先级最高。每句用户消息都是刻意的。\n\
                 assistant 消息仅提供上下文，不需逐条分析。\n\
                 tool_result 消息包含工具输出（文件内容、命令结果），提供操作上下文。\n\n\
                 提取规则:\n\
                 - 用户偏好/纠正/否定 → category: preference (最高优先)\n\
                 - 架构决策/技术事实 → category: memory 或子分类\n\
                 - 「好」「行」= 用户认可 AI 方案，记录为决策\n\
                 - 「别...」「不要...」= 高价值偏好\n\
                 - 运维痛点/调试弯路 → category: memory:ops / memory:debug\n\
                 - 不存: 纯任务指令、当天工作日志、代码提交记录\n\
                 - 存入前用 mission_kb_search 检查去重\n\n",
                batch_id, session_count, msg_count, user_count, truncated_note, skip_note,
            );
            let rendered_payload = format!("{}{}", header, output);

            // Set latch: mark pending as served for this extraction cycle
            {
                let mut es = state.extraction_state.write().await;
                if matches!(
                    es.phase,
                    crate::state::ExtractionPhase::Sending
                        | crate::state::ExtractionPhase::WaitingForSlotIdle
                ) {
                    es.mark_pending_batch_served(batch_id, rendered_payload.clone());
                }
            }

            Ok(ToolResult::text(&rendered_payload))
        }

        "mission_memory_pause" => {
            #[derive(Deserialize)]
            struct Args {
                #[serde(default, deserialize_with = "lenient::option_bool")]
                paused: Option<bool>,
            }
            let args: Args = serde_json::from_value(args).unwrap_or(Args { paused: None });
            let current = state
                .control_manager
                .current()
                .is_domain_paused(crate::control_tree::CtlDomain::Memory);
            // Route through ControlTree (single source of truth)
            let new_val = args.paused.unwrap_or(!current); // toggle if not specified
            state
                .control_manager
                .set_domain(crate::control_tree::CtlDomain::Memory, new_val);
            if new_val {
                info!("Memory extraction PAUSED by user (via ControlTree domain)");
            } else {
                // Clean up legacy flag file if it exists
                let flag = default_mission_home().join("memory_paused");
                let _ = std::fs::remove_file(&flag);
                info!("Memory extraction RESUMED by user (via ControlTree domain)");
            }
            Ok(ToolResult::text(if new_val {
                "记忆任务已暂停（2 小时后自动恢复）。调用 mission_memory_pause(paused: false) 手动恢复。"
            } else {
                "记忆任务已恢复。"
            }))
        }

        "mission_memory_status" => {
            let paused = state
                .control_manager
                .current()
                .is_domain_paused(crate::control_tree::CtlDomain::Memory);
            let now = chrono::Utc::now().timestamp();

            // Fast lane state
            let fast_es = state.extraction_state.read().await;
            let fast_busy = state
                .memory_slot_busy_since
                .load(std::sync::atomic::Ordering::Relaxed);
            let fast_lane = serde_json::json!({
                "slotId": MEMORY_SLOT_ID,
                "phase": format!("{:?}", fast_es.phase),
                "activeType": fast_es.active_type,
                "phaseAge": if fast_es.phase_started_at > 0 { now - fast_es.phase_started_at } else { 0 },
                "busySince": fast_busy,
                "busyDuration": if fast_busy > 0 { now - fast_busy } else { 0 },
                "currentTargets": fast_es.watermark_targets.iter()
                    .map(|(sid, _)| sid.clone()).collect::<Vec<_>>(),
                "currentTaskId": fast_es.current_task_id,
                "pendingServed": fast_es.pending_served,
                "pendingBatchId": fast_es.pending_batch_id,
                "pendingReplayCount": fast_es.pending_replay_count,
                "inputSkipDiagnostics": fast_es.input_skip_diagnostics(),
            });
            drop(fast_es);

            // Slow lane state
            let slow_es = state.slow_extraction_state.read().await;
            let slow_busy = state
                .slow_slot_busy_since
                .load(std::sync::atomic::Ordering::Relaxed);
            let slow_lane = serde_json::json!({
                "slotId": MEMORY_SLOW_SLOT_ID,
                "phase": format!("{:?}", slow_es.phase),
                "activeType": slow_es.active_type,
                "phaseAge": if slow_es.phase_started_at > 0 { now - slow_es.phase_started_at } else { 0 },
                "busySince": slow_busy,
                "busyDuration": if slow_busy > 0 { now - slow_busy } else { 0 },
                "currentConvId": slow_es.current_deep_conv_id,
                "currentTaskId": slow_es.current_task_id,
                "currentOutputCount": slow_es.current_output_count,
                "zeroOutputCount": slow_es.deep_analysis_zero_output_count,
                "zeroOutputFuseUntil": slow_es.deep_analysis_fuse_until,
                "zeroOutputFuseActive": slow_es.deep_analysis_fuse_active(now),
                "inputSkipDiagnostics": slow_es.input_skip_diagnostics(),
            });
            drop(slow_es);

            // Pending counts
            let pending_realtime = state.store.count_pending_realtime().await.unwrap_or(0);
            let pending_deep = state
                .store
                .count_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)
                .await
                .unwrap_or(0);

            // Timestamps
            let last_consolidation = state
                .store
                .last_completed_slot_task_at("kb_consolidation")
                .await
                .unwrap_or(None)
                .unwrap_or(0);
            let last_gc = state
                .store
                .daemon_state_get("last_auto_gc_at")
                .await
                .unwrap_or(None)
                .unwrap_or(0);

            // KB stats (full — includes mostAccessed, oldest, subcategories)
            let kb_stats = state
                .store
                .kb_stats()
                .await
                .map(|s| {
                    serde_json::json!({
                        "total": s["total"],
                        "categories": s.get("categoryRollup").unwrap_or(&s["categories"]),
                        "subcategories": s["categories"],
                        "neverAccessed": s["neverAccessed"],
                        "mostAccessed": s["mostAccessed"],
                        "oldest": s["oldest"],
                    })
                })
                .unwrap_or(serde_json::json!(null));

            // Recent memory slot tasks (last 15 across both slots)
            let mut recent: Vec<serde_json::Value> = Vec::new();
            for sid in &[MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID] {
                if let Ok(tasks) = state.store.list_slot_tasks(Some(sid), None, None, 10).await {
                    for t in tasks {
                        recent.push(serde_json::json!({
                            "id": t.id,
                            "slotId": t.slot_id,
                            "taskType": t.task_type,
                            "status": t.status,
                            "durationMs": t.duration_ms,
                            "createdAt": t.created_at,
                            "error": t.error,
                            "outputCount": t.output_count,
                            "sourceSessions": t.source_sessions,
                            "conversationId": t.conversation_id,
                        }));
                    }
                }
            }
            recent.sort_by(|a, b| {
                let ta = a["createdAt"].as_str().unwrap_or("");
                let tb = b["createdAt"].as_str().unwrap_or("");
                tb.cmp(ta)
            });
            recent.truncate(15);

            // Queue detail (per-session / per-conversation)
            let realtime_detail: Vec<serde_json::Value> = state.store.pending_realtime_detail().await
                .unwrap_or_default()
                .into_iter()
                .map(|(sid, cnt, oldest)| serde_json::json!({"sessionId": sid, "msgCount": cnt, "oldest": oldest}))
                .collect();
            let deep_detail: Vec<serde_json::Value> = state.store.pending_deep_detail(
                CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES
            ).await.unwrap_or_default()
                .into_iter()
                .map(|(id, ended, retries)| serde_json::json!({"conversationId": id, "endedAt": ended, "retries": retries}))
                .collect();

            Ok(ToolResult::json(&serde_json::json!({
                "paused": paused,
                "fastLane": fast_lane,
                "slowLane": slow_lane,
                "pendingRealtime": pending_realtime,
                "pendingDeep": pending_deep,
                "inputFilter": {
                    "slotExclusions": ["slot-memory*", "slot-diagnosis*", "agent-*"],
                    "textNoiseReasons": [
                        "deployment-monitor",
                        "runtime-report",
                        "worker-instruction",
                        "provider-preamble"
                    ],
                },
                "realtimeDetail": realtime_detail,
                "deepDetail": deep_detail,
                "lastKbConsolidation": if last_consolidation > 0 {
                    chrono::DateTime::from_timestamp(last_consolidation, 0)
                        .map(|d| d.to_rfc3339()).unwrap_or_default()
                } else { String::new() },
                "lastAutoGc": if last_gc > 0 {
                    chrono::DateTime::from_timestamp(last_gc, 0)
                        .map(|d| d.to_rfc3339()).unwrap_or_default()
                } else { String::new() },
                "kbStats": kb_stats,
                "recentTasks": recent,
            })))
        }

        _ => Err(anyhow!("Unknown memory tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::classify_memory_input_noise;

    #[test]
    fn memory_input_filter_preserves_user_utterances() {
        assert_eq!(
            classify_memory_input_noise("user", "deploy_succeeded 这个事件要记入 EventBus"),
            None
        );
    }

    #[test]
    fn memory_input_filter_classifies_deployment_monitor_noise() {
        assert_eq!(
            classify_memory_input_noise("assistant", "deploy monitor: deploy_succeeded"),
            Some("deployment-monitor")
        );
        assert_eq!(
            classify_memory_input_noise("tool_result", "agent_heartbeat from deploy-agent"),
            Some("deployment-monitor")
        );
        assert_eq!(
            classify_memory_input_noise(
                "assistant",
                "deployment-event-response observed build_started then reported_digest_missing; use xjp_deploy_watch",
            ),
            Some("deployment-monitor")
        );
        assert_eq!(
            classify_memory_input_noise(
                "tool_result",
                "deploy-center provenance_partial with agent_update_failed diagnostic",
            ),
            Some("deployment-monitor")
        );
    }

    #[test]
    fn memory_input_filter_classifies_runtime_and_worker_noise() {
        assert_eq!(
            classify_memory_input_noise("assistant", "lisp-code-sync watcher report path"),
            Some("runtime-report")
        );
        assert_eq!(
            classify_memory_input_noise("assistant", "Board Task ID: abc; mission_board_update"),
            Some("worker-instruction")
        );
    }
}
