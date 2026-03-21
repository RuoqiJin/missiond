use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use tracing::info;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::lenient;
use crate::state::{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::state::{CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES};
use crate::events_sync;
use crate::helpers::default_mission_home;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_memory
    if name == "mission_memory" {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("pending");
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

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Memory Extraction =====
        // Message-level pipeline tracking: returns pending messages with IDs.
        // State auto-committed by Daemon on extraction completion — no manual done() needed.
        "mission_memory_pending" | "mission_memory_pending_user" => {
            // De-bounce guard: if realtime extraction is in-flight and we already served
            // pending messages in this cycle, return early to prevent the agent from
            // polling the same messages repeatedly (watermark advances only on completion).
            {
                let es = state.extraction_state.read().await;
                if es.pending_served && matches!(es.phase, crate::state::ExtractionPhase::Sending | crate::state::ExtractionPhase::WaitingForSlotIdle) {
                    return Ok(ToolResult::text(
                        "本批次内容已获取。请分析已获取的消息后输出总结即可。\n\
                         ⚠️ 水位线由系统自动管理，重复调用不会返回新消息。下一批将在本次处理完成后自动调度。"
                    ));
                }
            }

            const PENDING_MSG_LIMIT: usize = 60;
            let pending = state.store.get_pending_realtime_messages_with_limit(PENDING_MSG_LIMIT).await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if pending.is_empty() {
                return Ok(ToolResult::text("没有待分析的新对话内容。"));
            }

            let mut output = String::new();
            let mut all_msg_ids: Vec<i64> = Vec::new();
            let mut user_count = 0usize;
            for (session_id, project, msgs) in &pending {
                output.push_str(&format!("## session: {} (project: {})\n\n", session_id, project));
                for msg in msgs {
                    all_msg_ids.push(msg.id);
                    if msg.role == "user" {
                        user_count += 1;
                        output.push_str(&format!("[#{}][{}] ★ user: {}\n\n", msg.id, msg.timestamp, msg.content));
                    } else if msg.role == "tool_result" {
                        // Tool results: file contents, command outputs — truncate to 1000 chars
                        let content = if msg.content.len() > 1000 {
                            let end = events_sync::floor_char_boundary(&msg.content, 1000);
                            format!("{}…({}字符)", &msg.content[..end], msg.content.len())
                        } else {
                            msg.content.clone()
                        };
                        output.push_str(&format!("[#{}][{}] tool_result: {}\n\n", msg.id, msg.timestamp, content));
                    } else {
                        // Assistant messages: truncate to reduce payload
                        let content = if msg.content.len() > 500 {
                            let end = events_sync::floor_char_boundary(&msg.content, 500);
                            format!("{}…", &msg.content[..end])
                        } else {
                            msg.content.clone()
                        };
                        output.push_str(&format!("[#{}][{}] assistant: {}\n\n", msg.id, msg.timestamp, content));
                    }
                }
            }

            let session_count = pending.len();
            let msg_count = all_msg_ids.len();
            let truncated_note = if msg_count >= PENDING_MSG_LIMIT {
                format!(" ⚠️ 已达上限 {}，可能还有更多未显示的消息。处理完当前批次后系统将自动推送下一批。", PENDING_MSG_LIMIT)
            } else {
                String::new()
            };
            let batch_id = format!("batch-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
            let header = format!(
                "[realtime-extract] [{}] {} 个会话, {} 条消息 (其中 {} 条用户消息){}\n\
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
                batch_id, session_count, msg_count, user_count, truncated_note,
            );

            // Set latch: mark pending as served for this extraction cycle
            {
                let mut es = state.extraction_state.write().await;
                if matches!(es.phase, crate::state::ExtractionPhase::Sending | crate::state::ExtractionPhase::WaitingForSlotIdle) {
                    es.pending_served = true;
                }
            }

            Ok(ToolResult::text(&format!("{}{}", header, output)))
        }

        "mission_memory_pause" => {
            #[derive(Deserialize)]
            struct Args {
                #[serde(default, deserialize_with = "lenient::option_bool")]
                paused: Option<bool>,
            }
            let args: Args = serde_json::from_value(args).unwrap_or(Args { paused: None });
            let current = state.control_manager.current()
                .is_domain_paused(crate::control_tree::CtlDomain::Memory);
            let new_val = args.paused.unwrap_or(!current); // toggle if not specified
            // Route through ControlTree (single source of truth)
            state.control_manager.set_domain(crate::control_tree::CtlDomain::Memory, new_val);
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
            let paused = state.control_manager.current()
                .is_domain_paused(crate::control_tree::CtlDomain::Memory);
            let now = chrono::Utc::now().timestamp();

            // Fast lane state
            let fast_es = state.extraction_state.read().await;
            let fast_busy = state.memory_slot_busy_since.load(std::sync::atomic::Ordering::Relaxed);
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
            });
            drop(fast_es);

            // Slow lane state
            let slow_es = state.slow_extraction_state.read().await;
            let slow_busy = state.slow_slot_busy_since.load(std::sync::atomic::Ordering::Relaxed);
            let slow_lane = serde_json::json!({
                "slotId": MEMORY_SLOW_SLOT_ID,
                "phase": format!("{:?}", slow_es.phase),
                "activeType": slow_es.active_type,
                "phaseAge": if slow_es.phase_started_at > 0 { now - slow_es.phase_started_at } else { 0 },
                "busySince": slow_busy,
                "busyDuration": if slow_busy > 0 { now - slow_busy } else { 0 },
                "currentConvId": slow_es.current_deep_conv_id,
                "currentTaskId": slow_es.current_task_id,
            });
            drop(slow_es);

            // Pending counts
            let pending_realtime = state.store.count_pending_realtime().await.unwrap_or(0);
            let pending_deep = state.store.count_pending_deep_analysis(
                CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES
            ).await.unwrap_or(0);

            // Timestamps
            let last_consolidation = state.store.last_completed_slot_task_at("kb_consolidation").await.unwrap_or(None).unwrap_or(0);
            let last_gc = state.store.daemon_state_get("last_auto_gc_at").await.unwrap_or(None).unwrap_or(0);

            // KB stats (full — includes mostAccessed, oldest, subcategories)
            let kb_stats = state.store.kb_stats().await
                .map(|s| serde_json::json!({
                    "total": s["total"],
                    "categories": s.get("categoryRollup").unwrap_or(&s["categories"]),
                    "subcategories": s["categories"],
                    "neverAccessed": s["neverAccessed"],
                    "mostAccessed": s["mostAccessed"],
                    "oldest": s["oldest"],
                }))
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
