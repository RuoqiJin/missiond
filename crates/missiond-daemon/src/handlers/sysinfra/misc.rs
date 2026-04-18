use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::helpers::ws_port;
use crate::lenient;
use crate::state::AppState;

#[derive(Deserialize)]
struct InboxArgs {
    #[serde(
        rename = "unreadOnly",
        default,
        deserialize_with = "lenient::option_bool"
    )]
    unread_only: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Info =====
        "mission_slots" => Ok(ToolResult::json(&state.mission.list_slots())),
        "mission_inbox" => {
            let InboxArgs { unread_only, limit } =
                serde_json::from_value(args).unwrap_or(InboxArgs {
                    unread_only: None,
                    limit: None,
                });
            let messages = state
                .store
                .get_inbox_messages(unread_only.unwrap_or(true), limit.unwrap_or(10) as i64)
                .await?;
            Ok(ToolResult::json(&messages))
        }

        "mission_pause" => {
            use std::sync::atomic::Ordering;
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("status");
            let home = crate::helpers::default_mission_home();
            let flag = home.join("global_paused");

            match action {
                "pause" => {
                    state.global_paused.store(true, Ordering::Relaxed);
                    let now = chrono::Utc::now().timestamp();
                    state.global_paused_at.store(now, Ordering::Relaxed);
                    let _ = std::fs::write(&flag, now.to_string());
                    warn!("Global pause activated via MCP tool");
                    Ok(ToolResult::text(
                        "✅ 全局暂停已激活。所有工位任务分派已停止。",
                    ))
                }
                "resume" => {
                    state.global_paused.store(false, Ordering::Relaxed);
                    state.global_paused_at.store(0, Ordering::Relaxed);
                    let _ = std::fs::remove_file(&flag);
                    info!("Global pause deactivated via MCP tool");
                    Ok(ToolResult::text("▶️ 全局暂停已解除。工位恢复正常工作。"))
                }
                _ => {
                    let paused = state.control_manager.current().global_paused;
                    let since = state.global_paused_at.load(Ordering::Relaxed);
                    let msg = if paused {
                        format!(
                            "⏸ 当前处于全局暂停状态 (始于 {})",
                            chrono::DateTime::from_timestamp(since, 0)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| "未知时间".to_string())
                        )
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

            // Memory extraction state (read from ControlTree)
            let memory_paused = state
                .control_manager
                .current()
                .is_domain_paused(crate::control_tree::CtlDomain::Memory);
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
                    // v2 bus: use AtomicBusMetrics for append counters in the future.
                    "publish_count": 0u64,
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
            let task = state
                .store
                .get_board_task(&args.task_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Task not found: {}", args.task_id))?;

            // Verify task has a flow phase
            let phase_str = task.flow_phase.as_deref().ok_or_else(|| {
                anyhow!(
                    "Task {} is not a flow task (flow_phase is null)",
                    args.task_id
                )
            })?;

            let phase = missiond_core::types::EngineeringPhase::from_str(phase_str)
                .ok_or_else(|| anyhow!("Unknown flow phase: {}", phase_str))?;

            // Validate artifact_type matches current phase
            let expected_artifact = match &phase {
                missiond_core::types::EngineeringPhase::Investigate => "investigation_report",
                missiond_core::types::EngineeringPhase::Plan => "execution_plan",
                missiond_core::types::EngineeringPhase::Execute => "execution_result",
                missiond_core::types::EngineeringPhase::Finalize => "commit_hash",
                other => {
                    return Ok(ToolResult::text(format!(
                    "Error: Phase '{}' is a daemon phase or Done — cannot submit artifacts for it.",
                    other.display_name()
                )))
                }
            };

            if args.artifact_type != expected_artifact {
                return Ok(ToolResult::text(format!(
                    "Error: Task is currently in '{}' phase. Expected artifact_type is '{}', but got '{}'. \
                     Please submit the correct artifact type to proceed.",
                    phase.display_name(), expected_artifact, args.artifact_type
                )));
            }

            // Load existing flow context
            let mut ctx: missiond_core::types::FlowContext = task
                .flow_context
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
            let next_phase = phase
                .next()
                .unwrap_or(missiond_core::types::EngineeringPhase::Done);
            let next_phase_str = next_phase.as_str().to_string();
            let ctx_json = serde_json::to_string(&ctx)?;

            // Atomic update: flow_context + flow_phase
            let update = missiond_core::types::UpdateBoardTaskInput {
                flow_phase: Some(next_phase_str.clone()),
                flow_context: Some(ctx_json),
                ..Default::default()
            };
            state
                .store
                .update_board_task(task.id.as_str(), &update)
                .await
                .map_err(|e| anyhow!("DB error updating flow: {}", e))?;

            // Write progress note
            let note_input = missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.to_string(),
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
            let _ = state.store.add_board_task_note(&note_input).await;

            // Hard intercept: Plan → Execute transition requires risk review
            if phase == missiond_core::types::EngineeringPhase::Plan {
                let plan_summary = if args.content.len() > 500 {
                    format!(
                        "{}...",
                        &args.content[..args
                            .content
                            .char_indices()
                            .nth(500)
                            .map(|(i, _)| i)
                            .unwrap_or(args.content.len())]
                    )
                } else {
                    args.content.clone()
                };
                let q_input = missiond_core::types::CreateAgentQuestionInput {
                    question: format!("[硬拦截] Plan→Execute 执行方案审核：{}", task.title),
                    context: Some(format!("执行方案摘要：\n{}", plan_summary)),
                    task_id: Some(task.id.to_string()),
                    slot_id: None,
                    session_id: None,
                    target: Some("master".to_string()),
                    options: None,
                    decision_type: Some("risk".to_string()),
                };
                match state.store.create_agent_question(&q_input).await {
                    Ok(q) => {
                        info!(task_id = %task.id, question_id = %q.id, "Hard intercept: Plan→Execute risk review created");
                        let _ = state
                            .bus
                            .publish_question(missiond_core::event::events::QuestionEvent::Created {
                                question_id: q.id.clone(),
                            })
                            .await;
                    }
                    Err(e) => warn!(error = %e, "Failed to create hard intercept question"),
                }
            }

            // Soft intercept: if slot flagged uncertainty, create question for Decision Engine
            if let Some(ref concern) = args.requires_master_decision {
                let q_input = missiond_core::types::CreateAgentQuestionInput {
                    question: format!(
                        "[Flow {} → {}] {}",
                        phase.display_name(),
                        next_phase.display_name(),
                        concern
                    ),
                    context: Some(format!(
                        "Slot flagged uncertainty during phase transition. Artifact: {} ({} chars)",
                        args.artifact_type,
                        args.content.len()
                    )),
                    task_id: Some(task.id.to_string()),
                    slot_id: None,
                    session_id: None,
                    target: Some("master".to_string()),
                    options: None,
                    decision_type: Some("implementation".to_string()),
                };
                match state.store.create_agent_question(&q_input).await {
                    Ok(q) => {
                        info!(task_id = %task.id, question_id = %q.id, "Soft intercept: created master decision question");
                        let _ = state
                            .bus
                            .publish_question(missiond_core::event::events::QuestionEvent::Created {
                                question_id: q.id.clone(),
                            })
                            .await;
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
                let stats = state
                    .store
                    .slot_task_stats(args.slot_id.as_deref())
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&stats))
            } else {
                let tasks = state
                    .store
                    .list_slot_tasks(
                        args.slot_id.as_deref(),
                        args.task_type.as_deref(),
                        args.status.as_deref(),
                        args.limit.unwrap_or(20),
                    )
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&tasks))
            }
        }

        // ===== Agent Questions (Pending Decisions) =====
        "mission_incident_test" => {
            let severity_str = args
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("warning");
            let severity = match severity_str {
                "critical" => missiond_core::types::IncidentSeverity::Critical,
                "high" => missiond_core::types::IncidentSeverity::High,
                _ => missiond_core::types::IncidentSeverity::Warning,
            };
            let source_str = args
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("manual");
            let source = match source_str {
                "health_check" => missiond_core::types::IncidentSource::HealthCheck,
                "deploy_center" => missiond_core::types::IncidentSource::DeployCenter,
                "sentry" => missiond_core::types::IncidentSource::Sentry,
                _ => missiond_core::types::IncidentSource::Manual,
            };
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Test incident")
                .to_string();
            let server_id = args
                .get("server_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

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

            // v2 bus: incident flows via IncidentEvent::Reported. A
            // dedicated v2 subscriber in `bus/v2_subscribers.rs` invokes
            // `aiops::process_incident` and triages the incident.
            match state
                .bus
                .publish_incident(missiond_core::event::events::IncidentEvent::Reported {
                    incident: incident.clone(),
                })
                .await
            {
                Err(e) => {
                    warn!("Failed to publish incident: {}", e);
                    Ok(ToolResult::error(format!(
                        "Failed to publish incident: {}",
                        e
                    )))
                }
                Ok(_) => Ok(ToolResult::json_pretty(&json!({
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
            let incidents = state
                .store
                .list_incidents(limit)
                .await
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
            let server_info = server
                .as_ref()
                .map(|s| json!({ "id": s.id, "host": s.host, "roles": s.roles }));

            match action {
                "status" => {
                    // Quick connectivity check via TCP probe
                    let host = server
                        .as_ref()
                        .and_then(|s| s.host.as_deref())
                        .unwrap_or(target);
                    let port: u16 = 22; // default SSH port
                    let addr = format!("{}:{}", host, port);
                    let reachable = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        tokio::net::TcpStream::connect(&addr),
                    )
                    .await
                    .is_ok();
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
                _ => Ok(ToolResult::error(format!(
                    "Unknown action: {}. Use wake/suspend/status",
                    action
                ))),
            }
        }

        // ── Jarvis Trace ──
        "mission_jarvis_logs" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let limit = limit.min(100);
            let status_filter = args
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
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

        // ===== Gemini Auth Mode Switch =====
        // Single source of truth: llm.yaml gemini_auth_mode
        // settings.json is synced as a side-effect for CLI compatibility
        "mission_gemini_auth" => {
            let mode = args
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("status");
            let llm_yaml_path = missiond_core::default_mission_home().join("llm.yaml");

            // Read llm.yaml
            let llm_content = tokio::fs::read_to_string(&llm_yaml_path)
                .await
                .map_err(|e| anyhow!("Failed to read llm.yaml: {}", e))?;
            let llm_config: serde_yaml::Value = serde_yaml::from_str(&llm_content)
                .map_err(|e| anyhow!("Failed to parse llm.yaml: {}", e))?;

            let current_mode = llm_config
                .get("gemini_auth_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("apikey");

            if mode == "status" {
                let key_preview = if current_mode == "apikey" {
                    llm_config
                        .get("gemini_api_key")
                        .and_then(|k| k.as_str())
                        .map(|k| {
                            if k.len() <= 12 {
                                "***".to_string()
                            } else {
                                format!("{}...{}", &k[..6], &k[k.len() - 4..])
                            }
                        })
                } else {
                    None
                };
                return Ok(ToolResult::json(&json!({
                    "mode": current_mode,
                    "key_preview": key_preview,
                })));
            }

            if mode != "apikey" && mode != "google" {
                return Ok(ToolResult::error(format!(
                    "Unknown mode: {}. Use: apikey, google, status",
                    mode
                )));
            }

            if mode == current_mode {
                return Ok(ToolResult::json(&json!({
                    "status": "no_change",
                    "mode": current_mode,
                    "message": format!("Already in {} mode", current_mode),
                })));
            }

            // Update llm.yaml (single source of truth)
            let new_content = if llm_content.contains("gemini_auth_mode:") {
                llm_content.replace(
                    &format!("gemini_auth_mode: {}", current_mode),
                    &format!("gemini_auth_mode: {}", mode),
                )
            } else {
                // First time: append after provider line
                llm_content.replace(
                    "provider: gemini-cli",
                    &format!("provider: gemini-cli\ngemini_auth_mode: {}", mode),
                )
            };
            tokio::fs::write(&llm_yaml_path, &new_content)
                .await
                .map_err(|e| anyhow!("Failed to write llm.yaml: {}", e))?;
            // Ensure restrictive permissions (contains API key)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &llm_yaml_path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }

            // Sync to settings.json (side-effect for CLI compatibility)
            let selected_type = if mode == "apikey" {
                "gemini-api-key"
            } else {
                "oauth-personal"
            };
            let settings_path = dirs::home_dir().map(|h| h.join(".gemini/settings.json"));
            if let Some(ref path) = settings_path {
                if let Ok(content) = tokio::fs::read_to_string(path).await {
                    if let Ok(mut settings) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(auth) = settings.pointer_mut("/security/auth") {
                            auth.as_object_mut().map(|m| {
                                m.insert("selectedType".to_string(), json!(selected_type))
                            });
                        }
                        if let Ok(json) = serde_json::to_string_pretty(&settings) {
                            let _ = tokio::fs::write(path, json).await;
                        }
                    }
                }
            }

            info!(from = current_mode, to = mode, "Gemini auth mode switched");
            Ok(ToolResult::json(&json!({
                "status": "switched",
                "from": current_mode,
                "to": mode,
            })))
        }

        // ===== Gemini Request Log =====
        "mission_gemini_trace" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let caller = args_val.get("caller").and_then(|v| v.as_str());
            let session_id = args_val.get("session_id").and_then(|v| v.as_str());
            let status = args_val.get("status").and_then(|v| v.as_str());
            let limit = args_val.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

            let rows = state
                .store
                .gemini_log_query(caller, session_id, status, limit)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "count": rows.len(),
                "requests": rows,
            })))
        }

        "mission_gemini_stats" => {
            let stats = state
                .store
                .gemini_log_stats()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&stats))
        }

        "mission_gemini_content" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let request_id = args_val
                .get("request_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing request_id"))?;
            match state
                .store
                .gemini_log_get_content(request_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
            {
                Some(content) => Ok(ToolResult::json_pretty(&content)),
                None => Ok(ToolResult::error("Request not found")),
            }
        }

        // ===== Gemini Watch (health probe) =====
        "mission_gemini_watch" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let action = args_val
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("status");
            gemini_watch(state, action).await
        }

        // ===== Jarvis Code Map Graph =====
        "mission_code_map_graph" => code_map_graph(state).await,

        _ => Err(anyhow!("Unknown misc tool: {name}")),
    }
}

/// Humanize a component name into a Chinese label and educational description.
fn humanize_component(comp: &str) -> (&str, &str) {
    match comp {
        "storage" => ("仓库管理员", "全公司唯一有权操作数据库的人。所有数据存取都必须经过他。管理：会话记录、聊天消息、任务板、代码索引、游标。"),
        "parser" => ("翻译官", "Claude Code 的工作日志(JSONL)是'外语'。翻译官逐行翻译成结构化消息——谁说了什么、用了什么工具、什么时候结束回合。"),
        "watcher" => ("巡逻员", "每2秒去 Claude Code 的日志文件夹巡逻一圈。发现新日志就通过邮差广播。还能检测代码变更(Write/Edit)触发架构审计。"),
        "awareness" => ("感知员/自我意识", "维护 S-EXP 灵魂文件和 @beacon 索引。每次 Claude 启动，把最新架构脉络注入它的脑海。每5秒检查文件变更自动刷新。"),
        "ast_indexer" => ("代码解剖师", "用 tree-sitter 解析 Rust/TypeScript，提取每个函数、结构体、枚举的签名和调用链。daemon 启动时全量扫描一次。"),
        "pty" => ("终端控制员", "直接操作 Claude Code 进程。用 bracketed paste 发消息，100ms 轮询状态机(空闲→思考→回复→工具运行→等确认)。他时刻知道 Claude 在干嘛。"),
        "session" => ("大堂经理", "管理前台(你聊天用的)和后台(Autopilot 派去干活的)两种 Claude 会话。不碰数据库，只通过邮差通知记忆部门。"),
        "autopilot" => ("后台监工", "监听'有新任务'事件，立刻认领→启动后台 Claude→等它就绪→发送任务指令。每5分钟巡检卡死任务。任务完成后杀掉后台进程。"),
        "seed_tasks" => ("播种员", "只在系统启动时干一次活：确保任务板里有 Topology Guardian 种子任务。这是 Jarvis 自进化的第一颗种子。"),
        "board" => ("任务板管理", "Claude Code 可调用的 MCP 工具：创建/查询/更新/删除任务，添加进度笔记。支持自动执行标记，让后台监工自动认领。"),
        "code_map" => ("代码透视镜", "你现在看到的这个页面的数据源！Claude 也能调用它来查询代码结构，不需要一个个文件去读。"),
        _ => (comp, ""),
    }
}

/// Humanize a pillar name into a Chinese label and educational description.
fn humanize_pillar(id: &str) -> (&str, &str) {
    match id {
        "memory" => ("记忆部门 🧠", "负责数据采集、存储和分析。包含：仓库管理员(唯一数据库入口)、翻译官(日志解析)、巡逻员(文件监听)、感知员(架构自知)、解剖师(代码索引)。"),
        "control" => ("控制部门 🎮", "管理终端进程和会话生命周期。包含：终端控制员(操作 Claude 进程)、大堂经理(管理前后台会话)、后台监工(自动执行任务)、播种员(初始化系统任务)。"),
        "tools" => ("工具部门 🔧", "提供 Claude Code 可调用的 MCP 工具。包含：任务板(任务增删改查)、代码透视镜(代码结构查询)。"),
        _ => (id, ""),
    }
}

/// Humanize a symbol name for display in the code map graph.
#[allow(dead_code)]
fn humanize_symbol(symbol: &str) -> String {
    // Strip common prefixes/suffixes for cleaner display
    let cleaned = symbol
        .trim_start_matches("handle_")
        .trim_start_matches("process_")
        .trim_end_matches("_handler")
        .trim_end_matches("_worker");
    // Title-case the first character
    let mut chars = cleaned.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

/// Build the Jarvis Code Map graph — a conceptual architecture overview
/// showing components grouped into pillars, plus infrastructure nodes.
async fn code_map_graph(_state: &AppState) -> Result<ToolResult> {
    let mut nodes: Vec<serde_json::Value> = Vec::new();
    let mut edges: Vec<serde_json::Value> = Vec::new();

    // ── Consciousness layer (future — placeholder to show the full architecture vision) ──
    nodes.push(json!({
        "id": "consciousness",
        "type": "infrastructure",
        "label": "Consciousness",
        "data": {
            "description": "State evaluation, trigger engine, decision making (future)",
            "humanLabel": "意识层 🧿",
            "humanDescription": "最高层：评估系统状态、判断用户是否卡住、检测架构偏离、主动触发任务。尚未实现——是 Jarvis 进化的终极目标。",
            "role": "consciousness"
        },
        "position": { "x": 200, "y": -100 }
    }));

    // ── Infrastructure: Event Bus ──
    nodes.push(json!({
        "id": "event-bus",
        "type": "infrastructure",
        "label": "Event Bus",
        "data": {
            "description": "Broadcast publish-subscribe event bus for cross-component communication",
            "humanLabel": "邮差/神经系统 ⚡",
            "humanDescription": "所有部门之间不直接说话，都通过邮差传递消息。watcher 发现新日志→告诉邮差→邮差广播给所有订阅者。任何部门都可以被替换或关掉，不影响其他人。",
            "role": "event_bus"
        },
        "position": { "x": 200, "y": 0 }
    }));

    // ── Infrastructure: Daemon ──
    nodes.push(json!({
        "id": "daemon",
        "type": "infrastructure",
        "label": "Daemon (main.rs)",
        "data": {
            "description": "Entry point — bootstraps all components, registers MCP tools, starts HTTP/IPC servers",
            "humanLabel": "总控中心 ⚙️",
            "humanDescription": "启动时创建所有部门、注册工具、开放 HTTP 接口(浏览器用)和 IPC 接口(Claude Code 用)。是把所有部门粘在一起的胶水。",
            "role": "daemon"
        },
        "position": { "x": 200, "y": 500 }
    }));

    // ── Pillar definitions ──
    let pillars: &[(&str, &[&str], f64)] = &[
        ("memory", &["storage", "parser", "watcher", "awareness", "ast_indexer"], 0.0),
        ("control", &["pty", "session", "autopilot", "seed_tasks"], 400.0),
        ("tools", &["board", "code_map"], 750.0),
    ];

    let y_base = 100.0;

    for &(pillar_id, components, x_base) in pillars {
        let (pillar_human_label, pillar_human_desc) = humanize_pillar(pillar_id);

        // Pillar group node
        nodes.push(json!({
            "id": pillar_id,
            "type": "pillar",
            "label": pillar_id,
            "data": {
                "description": format!("Pillar: {}", pillar_id),
                "humanLabel": pillar_human_label,
                "humanDescription": pillar_human_desc,
                "component_count": components.len()
            },
            "position": { "x": x_base, "y": y_base - 30.0 }
        }));

        // Component nodes within this pillar
        for (i, comp) in components.iter().enumerate() {
            let (comp_human_label, comp_human_desc) = humanize_component(comp);

            nodes.push(json!({
                "id": *comp,
                "type": "component",
                "label": *comp,
                "data": {
                    "description": format!("Component: {}", comp),
                    "humanLabel": comp_human_label,
                    "humanDescription": comp_human_desc,
                    "pillar": pillar_id
                },
                "position": { "x": x_base + 20.0, "y": y_base + (i as f64) * 70.0 }
            }));

            // Edge: component belongs to pillar
            edges.push(json!({
                "id": format!("e-{}-{}", pillar_id, comp),
                "source": pillar_id,
                "target": *comp,
                "label": "contains",
                "type": "hierarchy"
            }));

            // Edge: component uses event bus
            edges.push(json!({
                "id": format!("e-{}-eventbus", comp),
                "source": *comp,
                "target": "event-bus",
                "label": "publishes/subscribes",
                "type": "dependency"
            }));
        }
    }

    // ── Edge: consciousness reads from event bus ──
    edges.push(json!({
        "id": "e-consciousness-eventbus",
        "source": "consciousness",
        "target": "event-bus",
        "label": "observes",
        "type": "dependency"
    }));

    // ── Edge: daemon bootstraps pillars ──
    for &(pillar_id, _, _) in pillars {
        edges.push(json!({
            "id": format!("e-daemon-{}", pillar_id),
            "source": "daemon",
            "target": pillar_id,
            "label": "bootstraps",
            "type": "lifecycle"
        }));
    }

    // ── Edge: daemon creates event bus ──
    edges.push(json!({
        "id": "e-daemon-eventbus",
        "source": "daemon",
        "target": "event-bus",
        "label": "creates",
        "type": "lifecycle"
    }));

    // ── Cross-component data flows ──
    // watcher → parser (new JSONL lines flow to parser)
    edges.push(json!({
        "id": "e-watcher-parser",
        "source": "watcher",
        "target": "parser",
        "label": "raw lines",
        "type": "dataflow"
    }));

    // parser → storage (parsed messages stored)
    edges.push(json!({
        "id": "e-parser-storage",
        "source": "parser",
        "target": "storage",
        "label": "structured messages",
        "type": "dataflow"
    }));

    // autopilot → pty (sends task instructions)
    edges.push(json!({
        "id": "e-autopilot-pty",
        "source": "autopilot",
        "target": "pty",
        "label": "task instructions",
        "type": "dataflow"
    }));

    // session → pty (manages Claude processes)
    edges.push(json!({
        "id": "e-session-pty",
        "source": "session",
        "target": "pty",
        "label": "manages processes",
        "type": "dataflow"
    }));

    // board → storage (CRUD via store)
    edges.push(json!({
        "id": "e-board-storage",
        "source": "board",
        "target": "storage",
        "label": "task CRUD",
        "type": "dataflow"
    }));

    // code_map → storage (AST index queries)
    edges.push(json!({
        "id": "e-codemap-storage",
        "source": "code_map",
        "target": "storage",
        "label": "AST queries",
        "type": "dataflow"
    }));

    // ast_indexer → storage (writes AST index)
    edges.push(json!({
        "id": "e-ast-storage",
        "source": "ast_indexer",
        "target": "storage",
        "label": "writes index",
        "type": "dataflow"
    }));

    // awareness → watcher (watches file changes)
    edges.push(json!({
        "id": "e-awareness-watcher",
        "source": "awareness",
        "target": "watcher",
        "label": "file change events",
        "type": "dataflow"
    }));

    let result = json!({
        "nodes": nodes,
        "edges": edges,
        "node_count": nodes.len(),
        "edge_count": edges.len()
    });

    Ok(ToolResult::json_pretty(&result))
}

/// Gemini 429 recovery watch — background probe with 10-min timeout per attempt.
async fn gemini_watch(state: &AppState, action: &str) -> Result<ToolResult> {
    use std::sync::atomic::Ordering;

    match action {
        "start" => {
            if state.gemini_watch_active.load(Ordering::Relaxed) {
                let attempts = state.gemini_watch_attempts.load(Ordering::Relaxed);
                return Ok(ToolResult::text(format!(
                    "监测已在运行中 (第 {} 次探测)",
                    attempts
                )));
            }

            state.gemini_watch_active.store(true, Ordering::Relaxed);
            state.gemini_watch_attempts.store(0, Ordering::Relaxed);
            state
                .gemini_watch_started_at
                .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);

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

        _ => Ok(ToolResult::error(
            "Unknown action. Use: start, stop, status",
        )),
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
        })
        .await;

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
                timeout_secs: None,
                context_intent: None,
            };
            if let Err(e) = state.store.create_board_task(&input).await {
                warn!("Gemini watch: failed to create board task: {}", e);
            }
            break;
        }

        info!(
            attempt,
            "Gemini watch: ❌ still unavailable, waiting 10 min..."
        );
        // Wait 10 minutes before next probe (interruptible via abort)
        tokio::time::sleep(Duration::from_secs(600)).await;
    }
}
