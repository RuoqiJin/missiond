use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::lenient;
use crate::helpers::ws_port;

#[derive(Deserialize)]
struct InboxArgs {
    #[serde(rename = "unreadOnly", default, deserialize_with = "lenient::option_bool")]
    unread_only: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Info =====
        "mission_slots" => Ok(ToolResult::json(&state.mission.list_slots())),
        "mission_inbox" => {
            let InboxArgs {
                unread_only,
                limit,
            } = serde_json::from_value(args).unwrap_or(InboxArgs {
                unread_only: None,
                limit: None,
            });
            let messages = state
                .mission
                .get_inbox(unread_only.unwrap_or(true), limit.unwrap_or(10));
            Ok(ToolResult::json(&messages))
        }

        "mission_pause" => {
            use std::sync::atomic::Ordering;
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("status");
            let home = crate::helpers::default_mission_home();
            let flag = home.join("global_paused");

            match action {
                "pause" => {
                    state.global_paused.store(true, Ordering::Relaxed);
                    let now = chrono::Utc::now().timestamp();
                    state.global_paused_at.store(now, Ordering::Relaxed);
                    let _ = std::fs::write(&flag, now.to_string());
                    warn!("Global pause activated via MCP tool");
                    Ok(ToolResult::text("✅ 全局暂停已激活。所有工位任务分派已停止。"))
                }
                "resume" => {
                    state.global_paused.store(false, Ordering::Relaxed);
                    state.global_paused_at.store(0, Ordering::Relaxed);
                    let _ = std::fs::remove_file(&flag);
                    info!("Global pause deactivated via MCP tool");
                    Ok(ToolResult::text("▶️ 全局暂停已解除。工位恢复正常工作。"))
                }
                _ => {
                    let paused = state.global_paused.load(Ordering::Relaxed);
                    let since = state.global_paused_at.load(Ordering::Relaxed);
                    let msg = if paused {
                        format!("⏸ 当前处于全局暂停状态 (始于 {})", 
                            chrono::DateTime::from_timestamp(since, 0)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| "未知时间".to_string()))
                    } else {
                        "🟢 当前工作正常，未暂停。".to_string()
                    };
                    Ok(ToolResult::text(msg))
                }
            }
        }

        // ===== Health =====
        "mission_health" => {
            let agents = state.pty.get_all_status().await;
            let pty_status: Vec<Value> = agents
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "slotId": a.slot_id,
                        "state": a.state,
                        "pid": a.pid,
                    })
                })
                .collect();

            // Memory extraction state
            let memory_paused = state.memory_paused.load(std::sync::atomic::Ordering::Relaxed);
            let fast_lane = {
                let es = state.extraction_state.read().await;
                json!({ "phase": format!("{:?}", es.phase), "type": es.active_type })
            };
            let slow_lane = {
                let es = state.slow_extraction_state.read().await;
                json!({ "phase": format!("{:?}", es.phase), "type": es.active_type })
            };

            Ok(ToolResult::json(&serde_json::json!({
                "status": "ok",
                "ipc": "connected",
                "wsPort": ws_port(),
                "pty": pty_status,
                "uptime_epoch": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                "memory": {
                    "paused": memory_paused,
                    "fast_lane": fast_lane,
                    "slow_lane": slow_lane,
                },
                "event_bus": {
                    "publish_count": state.event_bus.publish_count.load(std::sync::atomic::Ordering::Relaxed),
                },
                "gemini_mode": if state.gemini.is_cli_mode() { "cli" } else { "http" },
                "stats": state.stats.snapshot(),
            })))
        }

        // ===== Engineering Flow =====
        "mission_submit_phase_result" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct SubmitPhaseArgs {
                task_id: String,
                artifact_type: String,
                content: String,
                /// Slot flags uncertainty — triggers Decision Engine soft intercept
                #[serde(default)]
                requires_master_decision: Option<String>,
            }
            let args: SubmitPhaseArgs = serde_json::from_value(args)?;

            // Get the task
            let task = state.mission.db()
                .get_board_task(&args.task_id)
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Task not found: {}", args.task_id))?;

            // Verify task has a flow phase
            let phase_str = task.flow_phase.as_deref()
                .ok_or_else(|| anyhow!("Task {} is not a flow task (flow_phase is null)", args.task_id))?;

            let phase = missiond_core::types::EngineeringPhase::from_str(phase_str)
                .ok_or_else(|| anyhow!("Unknown flow phase: {}", phase_str))?;

            // Validate artifact_type matches current phase
            let expected_artifact = match &phase {
                missiond_core::types::EngineeringPhase::Investigate => "investigation_report",
                missiond_core::types::EngineeringPhase::Plan => "execution_plan",
                missiond_core::types::EngineeringPhase::Execute => "execution_result",
                missiond_core::types::EngineeringPhase::Finalize => "commit_hash",
                other => return Ok(ToolResult::text(format!(
                    "Error: Phase '{}' is a daemon phase or Done — cannot submit artifacts for it.",
                    other.display_name()
                ))),
            };

            if args.artifact_type != expected_artifact {
                return Ok(ToolResult::text(format!(
                    "Error: Task is currently in '{}' phase. Expected artifact_type is '{}', but got '{}'. \
                     Please submit the correct artifact type to proceed.",
                    phase.display_name(), expected_artifact, args.artifact_type
                )));
            }

            // Load existing flow context
            let mut ctx: missiond_core::types::FlowContext = task.flow_context
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();

            // Store artifact in context
            match args.artifact_type.as_str() {
                "investigation_report" => ctx.investigation_report = Some(args.content.clone()),
                "execution_plan" => ctx.execution_plan = Some(args.content.clone()),
                "execution_result" => ctx.execution_result = Some(args.content.clone()),
                "commit_hash" => ctx.commit_hash = Some(args.content.clone()),
                _ => {}
            }

            // Advance to next phase
            let next_phase = phase.next().unwrap_or(missiond_core::types::EngineeringPhase::Done);
            let next_phase_str = next_phase.as_str().to_string();
            let ctx_json = serde_json::to_string(&ctx)?;

            // Atomic update: flow_context + flow_phase
            let update = missiond_core::types::UpdateBoardTaskInput {
                flow_phase: Some(next_phase_str.clone()),
                flow_context: Some(ctx_json),
                ..Default::default()
            };
            state.mission.db()
                .update_board_task(&task.id, &update)
                .map_err(|e| anyhow!("DB error updating flow: {}", e))?;

            // Write progress note
            let note_input = missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.clone(),
                content: format!(
                    "✅ Flow phase '{}' completed → '{}'\nArtifact: {} ({} chars)",
                    phase.display_name(),
                    next_phase.display_name(),
                    args.artifact_type,
                    args.content.len()
                ),
                note_type: Some("progress".to_string()),
                author: Some("flow-engine".to_string()),
            };
            let _ = state.mission.db().add_board_task_note(&note_input);

            // Hard intercept: Plan → Execute transition requires risk review
            if phase == missiond_core::types::EngineeringPhase::Plan {
                let plan_summary = if args.content.len() > 500 {
                    format!("{}...", &args.content[..args.content.char_indices().nth(500).map(|(i,_)| i).unwrap_or(args.content.len())])
                } else {
                    args.content.clone()
                };
                let q_input = missiond_core::types::CreateAgentQuestionInput {
                    question: format!("[硬拦截] Plan→Execute 执行方案审核：{}", task.title),
                    context: Some(format!("执行方案摘要：\n{}", plan_summary)),
                    task_id: Some(task.id.clone()),
                    slot_id: None,
                    session_id: None,
                    target: Some("master".to_string()),
                    options: None,
                    decision_type: Some("risk".to_string()),
                };
                match state.mission.db().create_agent_question(&q_input) {
                    Ok(q) => {
                        info!(task_id = %task.id, question_id = %q.id, "Hard intercept: Plan→Execute risk review created");
                        state.event_bus.publish(crate::event_bus::DaemonEvent::QuestionCreated { question_id: q.id.clone() });
                    }
                    Err(e) => warn!(error = %e, "Failed to create hard intercept question"),
                }
            }

            // Soft intercept: if slot flagged uncertainty, create question for Decision Engine
            if let Some(ref concern) = args.requires_master_decision {
                let q_input = missiond_core::types::CreateAgentQuestionInput {
                    question: format!("[Flow {} → {}] {}", phase.display_name(), next_phase.display_name(), concern),
                    context: Some(format!("Slot flagged uncertainty during phase transition. Artifact: {} ({} chars)", args.artifact_type, args.content.len())),
                    task_id: Some(task.id.clone()),
                    slot_id: None,
                    session_id: None,
                    target: Some("master".to_string()),
                    options: None,
                    decision_type: Some("implementation".to_string()),
                };
                match state.mission.db().create_agent_question(&q_input) {
                    Ok(q) => {
                        info!(task_id = %task.id, question_id = %q.id, "Soft intercept: created master decision question");
                        state.event_bus.publish(crate::event_bus::DaemonEvent::QuestionCreated { question_id: q.id.clone() });
                    }
                    Err(e) => warn!(error = %e, "Failed to create soft intercept question"),
                }
            }

            Ok(ToolResult::text(format!(
                "Phase '{}' completed. Artifact '{}' saved ({} chars). \
                 Flow advanced to '{}'. Please wait for the next instruction.",
                phase.display_name(),
                args.artifact_type,
                args.content.len(),
                next_phase.display_name()
            )))
        }

        // ===== Slot Task History =====
        "mission_slot_history" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct SlotHistoryArgs {
                slot_id: Option<String>,
                task_type: Option<String>,
                status: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_bool")]
                stats: Option<bool>,
            }
            let args: SlotHistoryArgs = serde_json::from_value(args)?;
            if args.stats.unwrap_or(false) {
                let stats = state.mission.db()
                    .slot_task_stats(args.slot_id.as_deref())
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&stats))
            } else {
                let tasks = state.mission.db()
                    .list_slot_tasks(
                        args.slot_id.as_deref(),
                        args.task_type.as_deref(),
                        args.status.as_deref(),
                        args.limit.unwrap_or(20),
                    )
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&tasks))
            }
        }

        // ===== Agent Questions (Pending Decisions) =====
        "mission_incident_test" => {
            let severity_str = args.get("severity").and_then(|v| v.as_str()).unwrap_or("warning");
            let severity = match severity_str {
                "critical" => missiond_core::types::IncidentSeverity::Critical,
                "high" => missiond_core::types::IncidentSeverity::High,
                _ => missiond_core::types::IncidentSeverity::Warning,
            };
            let source_str = args.get("source").and_then(|v| v.as_str()).unwrap_or("manual");
            let source = match source_str {
                "health_check" => missiond_core::types::IncidentSource::HealthCheck,
                "deploy_center" => missiond_core::types::IncidentSource::DeployCenter,
                "sentry" => missiond_core::types::IncidentSource::Sentry,
                _ => missiond_core::types::IncidentSource::Manual,
            };
            let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Test incident").to_string();
            let server_id = args.get("server_id").and_then(|v| v.as_str()).map(|s| s.to_string());

            let incident = missiond_core::types::MissionIncident {
                id: format!("inc-{}", uuid::Uuid::new_v4()),
                severity,
                source,
                title: title.clone(),
                description: format!("Manual test incident: {}", title),
                server_id,
                raw_payload: json!({"test": true, "injected_at": chrono::Utc::now().to_rfc3339()}),
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            if let Err(e) = state.incident_tx.try_send(incident.clone()) {
                warn!("Incident channel full, dropping incident: {}", e);
                Ok(ToolResult::error(format!("Incident channel full: {}", e)))
            } else {
                Ok(ToolResult::json_pretty(&json!({
                    "status": "injected",
                    "incident_id": incident.id,
                    "severity": severity_str,
                    "title": incident.title,
                })))
            }
        }
        "mission_incident_list" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as i64;
            let limit = limit.min(100);
            let incidents = state.mission.db().list_incidents(limit)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&incidents))
        }

        // ── Power Control (Epic 3: 算力经济学) ──
        "mission_power_control" => {
            let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");

            if target.is_empty() || action.is_empty() {
                return Ok(ToolResult::error("target and action are required"));
            }

            // Look up server from infra registry
            let server = state.infra.read().unwrap().get(target).cloned();
            let server_info = server.as_ref().map(|s| {
                json!({ "id": s.id, "host": s.host, "roles": s.roles })
            });

            match action {
                "status" => {
                    // Quick connectivity check via TCP probe
                    let host = server.as_ref().and_then(|s| s.host.as_deref()).unwrap_or(target);
                    let port: u16 = 22; // default SSH port
                    let addr = format!("{}:{}", host, port);
                    let reachable = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        tokio::net::TcpStream::connect(&addr),
                    ).await.is_ok();
                    Ok(ToolResult::json_pretty(&json!({
                        "target": target,
                        "action": "status",
                        "reachable": reachable,
                        "probe": addr,
                        "server": server_info,
                    })))
                }
                "wake" => {
                    // MVP: log intent, actual WoL/gcloud start to be wired per-target
                    info!(target, "Power control: wake requested");
                    Ok(ToolResult::json_pretty(&json!({
                        "target": target,
                        "action": "wake",
                        "status": "requested",
                        "note": "Wake-on-LAN / cloud API 唤醒已记录，具体执行需按 infra 配置补充",
                        "server": server_info,
                    })))
                }
                "suspend" => {
                    info!(target, "Power control: suspend requested");
                    Ok(ToolResult::json_pretty(&json!({
                        "target": target,
                        "action": "suspend",
                        "status": "requested",
                        "note": "休眠指令已记录，具体执行需按 infra 配置补充",
                        "server": server_info,
                    })))
                }
                _ => Ok(ToolResult::error(format!("Unknown action: {}. Use wake/suspend/status", action))),
            }
        }

        // ── Jarvis Trace ──
        "mission_jarvis_logs" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let limit = limit.min(100);
            let status_filter = args.get("status").and_then(|v| v.as_str()).map(|s| s.to_string());
            let mut traces = state.jarvis_trace.list_traces(limit).await;
            if let Some(ref sf) = status_filter {
                traces.retain(|t| t.status.to_string() == *sf);
            }
            Ok(ToolResult::json_pretty(&traces))
        }
        "mission_jarvis_trace" => {
            let trace_id = args.get("trace_id").and_then(|v| v.as_str());
            let trace = if let Some(id) = trace_id {
                state.jarvis_trace.get_trace(id).await
            } else {
                state.jarvis_trace.latest_trace().await
            };
            match trace {
                Some(t) => Ok(ToolResult::json_pretty(&t)),
                None => Ok(ToolResult::error("Trace not found")),
            }
        }

        // ===== Gemini Request Log =====
        "mission_gemini_trace" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let caller = args_val.get("caller").and_then(|v| v.as_str());
            let session_id = args_val.get("session_id").and_then(|v| v.as_str());
            let status = args_val.get("status").and_then(|v| v.as_str());
            let limit = args_val.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

            let db = state.mission.db();
            let rows = db.gemini_log_query(caller, session_id, status, limit)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "count": rows.len(),
                "requests": rows,
            })))
        }

        "mission_gemini_stats" => {
            let db = state.mission.db();
            let stats = db.gemini_log_stats()
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&stats))
        }

        "mission_gemini_content" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let request_id = args_val.get("request_id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing request_id"))?;
            let db = state.mission.db();
            match db.gemini_log_get_content(request_id).map_err(|e| anyhow!("DB error: {}", e))? {
                Some(content) => Ok(ToolResult::json_pretty(&content)),
                None => Ok(ToolResult::error("Request not found")),
            }
        }

        // ===== Gemini Watch (health probe) =====
        "mission_gemini_watch" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let action = args_val.get("action").and_then(|v| v.as_str()).unwrap_or("status");
            gemini_watch(state, action).await
        }

        _ => Err(anyhow!("Unknown misc tool: {name}")),
    }
}

/// Gemini 429 recovery watch — background probe with 10-min timeout per attempt.
async fn gemini_watch(state: &AppState, action: &str) -> Result<ToolResult> {
    use std::sync::atomic::Ordering;

    match action {
        "start" => {
            if state.gemini_watch_active.load(Ordering::Relaxed) {
                let attempts = state.gemini_watch_attempts.load(Ordering::Relaxed);
                return Ok(ToolResult::text(format!(
                    "监测已在运行中 (第 {} 次探测)", attempts
                )));
            }

            state.gemini_watch_active.store(true, Ordering::Relaxed);
            state.gemini_watch_attempts.store(0, Ordering::Relaxed);
            state.gemini_watch_started_at.store(
                chrono::Utc::now().timestamp(), Ordering::Relaxed,
            );

            let st = state.clone();
            let handle = tokio::spawn(async move {
                gemini_watch_loop(st).await;
            });

            *state.gemini_watch_handle.lock().await = Some(handle);
            info!("Gemini watch started");
            Ok(ToolResult::text("✅ Gemini 监测已启动。每 10 分钟探测一次 gemini-3.1-pro-preview，恢复后自动创建 Board 通知。"))
        }

        "stop" => {
            if !state.gemini_watch_active.load(Ordering::Relaxed) {
                return Ok(ToolResult::text("监测未在运行"));
            }
            state.gemini_watch_active.store(false, Ordering::Relaxed);
            if let Some(handle) = state.gemini_watch_handle.lock().await.take() {
                handle.abort();
            }
            state.gemini_watch_started_at.store(0, Ordering::Relaxed);
            info!("Gemini watch stopped");
            Ok(ToolResult::text("⏹ Gemini 监测已停止"))
        }

        "status" => {
            let active = state.gemini_watch_active.load(Ordering::Relaxed);
            let attempts = state.gemini_watch_attempts.load(Ordering::Relaxed);
            let started = state.gemini_watch_started_at.load(Ordering::Relaxed);
            Ok(ToolResult::json_pretty(&json!({
                "active": active,
                "attempts": attempts,
                "started_at": if started > 0 {
                    chrono::DateTime::from_timestamp(started, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                } else {
                    String::new()
                },
            })))
        }

        _ => Ok(ToolResult::error("Unknown action. Use: start, stop, status")),
    }
}

/// Background loop: probe Gemini every 10 minutes until success.
async fn gemini_watch_loop(state: AppState) {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    let model = "gemini-3.1-pro-preview";
    let probe_timeout = Duration::from_secs(600); // 10 min per attempt

    loop {
        if !state.gemini_watch_active.load(Ordering::Relaxed) {
            break;
        }

        let attempt = state.gemini_watch_attempts.fetch_add(1, Ordering::Relaxed) + 1;
        info!(attempt, model, "Gemini watch: probing...");

        // Spawn gemini CLI with 10-min timeout
        let result = tokio::time::timeout(probe_timeout, async {
            let output = tokio::process::Command::new("gemini")
                .args(["-p", "say OK", "-m", model, "-o", "text", "--yolo"])
                .output()
                .await;
            output
        }).await;

        let ok = match result {
            Ok(Ok(output)) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.to_lowercase().contains("ok")
            }
            _ => false,
        };

        if ok {
            info!(attempt, "Gemini watch: ✅ recovered!");
            state.gemini_watch_active.store(false, Ordering::Relaxed);
            state.gemini_watch_started_at.store(0, Ordering::Relaxed);

            // Create Board notification
            let input = missiond_core::types::CreateBoardTaskInput {
                title: format!("✅ Gemini {} 已恢复", model),
                description: Some(format!(
                    "经过 {} 次探测，模型已恢复可用。之前被阻塞的任务可以继续了。",
                    attempt
                )),
                category: Some("infra".to_string()),
                priority: Some("high".to_string()),
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
            };
            if let Err(e) = state.mission.db().create_board_task(&input) {
                warn!("Gemini watch: failed to create board task: {}", e);
            }
            break;
        }

        info!(attempt, "Gemini watch: ❌ still unavailable, waiting 10 min...");
        // Wait 10 minutes before next probe (interruptible via abort)
        tokio::time::sleep(Duration::from_secs(600)).await;
    }
}
