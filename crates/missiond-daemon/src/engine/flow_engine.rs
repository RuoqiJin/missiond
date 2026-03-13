//! Flow Engine — Board task lifecycle execution (Investigate -> Plan -> Execute -> Finalize).
//!
//! Extracted from autopilot.rs (Phase 3 PR3). Handles engineering phase state
//! machine, Gemini consultation, PTY prompt dispatch, and artifact management.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::state::AppState;
use crate::slot_env::{build_slot_tracking_env, capture_slot_session_uuid};
use crate::decision_harvest::harvest_decisions_for_task;
use crate::llm_gateway::{call_gemini_for_flow, determine_llm_env};
use crate::supervisor::{strip_prompt_echo, truncate_safe, is_auth_error};
use missiond_core::SessionState;
use missiond_core::PTYSpawnOptions;

// @beacon: orchestration
/// Execute a flow-enabled Board task through the Engineering Flow Engine.
/// Handles all phase types: slot phases (send to PTY), daemon phases (call Gemini), and Done.
pub(crate) async fn execute_flow_task(state: &AppState, task: &missiond_core::types::BoardTask, slot_id: &str) -> Result<()> {
    // Reentry guard: skip if this task is already being processed
    {
        let mut in_progress = state.flow_in_progress.lock().unwrap();
        if !in_progress.insert(task.id.clone()) {
            debug!(task_id = %task.id, "Flow engine: task already in progress, skipping");
            return Ok(());
        }
    }
    // RAII guard to remove task from in-progress set on exit
    struct FlowGuard {
        set: Arc<std::sync::Mutex<HashSet<String>>>,
        id: String,
    }
    impl Drop for FlowGuard {
        fn drop(&mut self) {
            self.set.lock().unwrap().remove(&self.id);
        }
    }
    let _guard = FlowGuard {
        set: state.flow_in_progress.clone(),
        id: task.id.clone(),
    };

    let phase_str = task.flow_phase.as_deref().unwrap_or("investigate");
    let phase = missiond_core::types::EngineeringPhase::from_str(phase_str)
        .ok_or_else(|| anyhow!("Unknown flow phase: {}", phase_str))?;

    let mut ctx: missiond_core::types::FlowContext = task.flow_context
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    info!(task_id = %task.id, phase = %phase_str, slot_id, "Flow engine: processing phase");

    match phase {
        // === Done phase: mark task complete ===
        missiond_core::types::EngineeringPhase::Done => {
            let _ = state.mission.db().update_board_task(
                &task.id,
                &missiond_core::types::UpdateBoardTaskInput {
                    status: Some("done".to_string()),
                    ..Default::default()
                },
            );
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: "✅ Flow Engine: 全部阶段完成，任务标记 done".to_string(),
                    note_type: Some("progress".to_string()),
                    author: Some("flow-engine".to_string()),
                },
            );
            info!(task_id = %task.id, "Flow engine: task completed (all phases done)");

            // Decision Engine: harvest decisions from completed task
            let state_clone = state.clone();
            let task_id = task.id.clone();
            let task_title = task.title.clone();
            tokio::spawn(async move {
                harvest_decisions_for_task(&state_clone, &task_id, &task_title).await;
            });
        }

        // === Daemon phases: call Gemini directly ===
        p if p.is_daemon_phase() => {
            // Claim task as running + set lease
            let _ = state.mission.db().update_board_task(
                &task.id,
                &missiond_core::types::UpdateBoardTaskInput {
                    status: Some("running".to_string()),
                    ..Default::default()
                },
            );
            let lease = (chrono::Utc::now() + chrono::TimeDelta::seconds(p.timeout_secs() as i64 + 60)).to_rfc3339();
            let _ = state.mission.db().set_board_task_lease(&task.id, &lease);

            let (gemini_prompt, artifact_field) = match p {
                missiond_core::types::EngineeringPhase::ConsultGemini1 => {
                    let report = ctx.investigation_report.as_deref().unwrap_or("(无调查报告)");
                    (
                        format!(
                            "# 架构咨询\n\n## 任务\n{}\n\n## 描述\n{}\n\n## 代码调查报告\n{}\n\n请给出架构层面的解决方案和建议。重点关注：\n1. 技术选型与现有架构的兼容性\n2. 潜在的风险和边界情况\n3. 推荐的实现路径",
                            task.title, task.description, report
                        ),
                        "gemini_advice_1"
                    )
                }
                missiond_core::types::EngineeringPhase::ConsultGemini2 => {
                    let plan = ctx.execution_plan.as_deref().unwrap_or("(无执行方案)");
                    let advice1 = ctx.gemini_advice_1.as_deref().unwrap_or("");
                    (
                        format!(
                            "# 执行方案审查\n\n## 任务\n{}\n\n## 第一轮架构建议\n{}\n\n## 执行方案\n{}\n\n请审查此方案，指出：\n1. 遗漏或风险点\n2. 与第一轮建议的一致性\n3. 优化建议",
                            task.title, advice1, plan
                        ),
                        "gemini_advice_2"
                    )
                }
                _ => return Ok(()),
            };

            // Call Gemini via router API
            let gemini_response = call_gemini_for_flow(state, &task.id, &gemini_prompt).await;

            match gemini_response {
                Ok(response) => {
                    // Store artifact
                    match artifact_field {
                        "gemini_advice_1" => ctx.gemini_advice_1 = Some(response.clone()),
                        "gemini_advice_2" => ctx.gemini_advice_2 = Some(response.clone()),
                        _ => {}
                    }

                    // Advance phase — unclaim so next tick can re-claim for next phase
                    let next_phase = p.next().unwrap_or(missiond_core::types::EngineeringPhase::Done);
                    let _ = state.mission.db().update_board_task(
                        &task.id,
                        &missiond_core::types::UpdateBoardTaskInput {
                            flow_phase: Some(next_phase.as_str().to_string()),
                            flow_context: Some(serde_json::to_string(&ctx).unwrap_or_default()),
                            ..Default::default()
                        },
                    );
                    let _ = state.mission.db().unclaim_board_task(&task.id);

                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!(
                                "✅ {} 完成 → 进入 {}\n\nGemini 回复摘要 (前500字):\n{}",
                                p.display_name(),
                                next_phase.display_name(),
                                &truncate_safe(&response, 500)
                            ),
                            note_type: Some("progress".to_string()),
                            author: Some("flow-engine".to_string()),
                        },
                    );

                    info!(task_id = %task.id, from = %phase_str, to = %next_phase.as_str(), "Flow engine: daemon phase completed");
                }
                Err(e) => {
                    warn!(task_id = %task.id, phase = %phase_str, error = %e, "Flow engine: Gemini call failed");
                    let _ = state.mission.db().unclaim_board_task(&task.id);
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!("❌ {} Gemini 调用失败: {}", p.display_name(), e),
                            note_type: Some("note".to_string()),
                            author: Some("flow-engine".to_string()),
                        },
                    );
                }
            }
        }

        // === Slot phases: send prompt to PTY ===
        p if p.is_slot_phase() => {
            // Claim the task
            match state.mission.db().claim_board_task(&task.id, slot_id, "pty_slot") {
                Ok(Some(_)) => {
                    // Set lease based on phase timeout (e.g. Execute = 60min)
                    let lease = (chrono::Utc::now() + chrono::TimeDelta::seconds(p.timeout_secs() as i64 + 300)).to_rfc3339();
                    let _ = state.mission.db().set_board_task_lease(&task.id, &lease);
                }
                Ok(None) => {
                    debug!(task_id = %task.id, slot_id, "Flow engine: task already claimed, skipping");
                    return Ok(());
                }
                Err(e) => {
                    warn!(task_id = %task.id, error = %e, "Flow engine: failed to claim task");
                    return Ok(());
                }
            }

            // Ensure PTY is running (flow tasks also get model routing)
            let slot_role = state.mission.get_slot(slot_id)
                .map(|s| s.config.role.clone())
                .unwrap_or_default();
            let task_env = determine_llm_env(task, &slot_role);
            if !ensure_autopilot_pty(state, task, slot_id, task_env).await {
                return Ok(());
            }

            // Link PTY session to task for audit trail
            if let Ok(Some(session_uuid)) = state.mission.db().get_slot_session(slot_id) {
                let _ = state.mission.db().set_conversation_task_id(&session_uuid, &task.id);
            }

            // Build phase-specific prompt
            let prompt = build_flow_phase_prompt(task, &p, &ctx, &state.prompts);

            // Inject answered Q&A context (Phase 0: Decision Engine prerequisite)
            let prompt = {
                let answered = state.mission.db().list_questions_for_task(&task.id).unwrap_or_default();
                if answered.is_empty() {
                    prompt
                } else {
                    let qa_block: String = answered.iter()
                        .filter(|q| q.answer.is_some())
                        .map(|q| format!("Q: {}\nA: {}", q.question, q.answer.as_deref().unwrap_or("")))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if qa_block.is_empty() {
                        prompt
                    } else {
                        format!("[决策与指示 (Decisions & Directives)]\n{}\n\n{}", qa_block, prompt)
                    }
                }
            };

            let timeout_ms = p.timeout_secs() * 1000;

            // Pre-send state verification: confirm slot is Idle before sending.
            // Guards against race where MCP init began after ensure_autopilot_pty returned.
            if let Some(pre_send_status) = state.pty.get_status(slot_id).await {
                if pre_send_status.state != SessionState::Idle {
                    debug!(task_id = %task.id, phase = %phase_str, slot_id, state = ?pre_send_status.state,
                        "Flow engine: slot not Idle pre-send, releasing task without penalty");
                    let _ = state.mission.db().unclaim_board_task(&task.id);
                    return Ok(());
                }
            }

            info!(task_id = %task.id, phase = %phase_str, timeout_ms, "Flow engine: sending phase prompt to PTY");

            // Emit dispatch event for timeline visibility
            {
                let preview = if prompt.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !prompt.is_char_boundary(end) { end -= 1; }
                    format!("{}...", &prompt[..end])
                } else { prompt.clone() };
                state.event_bus.publish(crate::event_bus::DaemonEvent::SlotTaskDispatched {
                    slot_id: slot_id.to_string(),
                    task_id: Some(task.id.clone()),
                    purpose: format!("flow_{}", phase_str),
                    prompt_chars: prompt.len(),
                    preview,
                });
            }

            match state.pty.send(slot_id, &prompt, timeout_ms).await {
                Ok(res) => {
                    // Check for auth errors
                    if is_auth_error(&res.response) {
                        warn!(task_id = %task.id, slot_id, phase = %phase_str, "Flow engine: auth error detected in PTY response");
                        let _ = state.mission.db().add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.clone(),
                                content: format!("⚠️ **Auth Error** — slot {} OAuth token 过期，Flow phase {} 中止", slot_id, p.display_name()),
                                note_type: Some("note".to_string()),
                                author: Some("flow-engine".to_string()),
                            },
                        );
                        // Back to open for retry + track failure
                        let _ = state.mission.db().unclaim_board_task(&task.id);
                        {
                            let mut fail_map = state.slot_fail_counts.lock().unwrap();
                            let entry = fail_map.entry(slot_id.to_string()).or_insert((0, 0));
                            entry.0 += 1;
                            entry.1 = chrono::Utc::now().timestamp();
                        }
                        return Ok(());
                    }

                    // Strip prompt echo from PTY response to reduce noise in board notes
                    let clean_response = strip_prompt_echo(&res.response, &prompt);
                    let _ = state.mission.db().add_board_task_note(
                        &missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.clone(),
                            content: format!(
                                "**Flow Phase {} PTY 响应** ({}ms)\n\n{}",
                                p.display_name(),
                                res.duration_ms,
                                &truncate_safe(&clean_response, 2000)
                            ),
                            note_type: Some("progress".to_string()),
                            author: Some("flow-engine".to_string()),
                        },
                    );

                    // Phase advancement is handled by submit_phase_result MCP tool.
                    // After PTY response, check if phase was already advanced.
                    let updated_task = state.mission.db().get_board_task(&task.id);
                    if let Ok(Some(updated)) = updated_task {
                        let current_phase = updated.flow_phase.as_deref().unwrap_or(phase_str);
                        if current_phase != phase_str {
                            // Phase was advanced by submit_phase_result → unclaim for next tick
                            let _ = state.mission.db().unclaim_board_task(&task.id);
                            info!(task_id = %task.id, from = %phase_str, to = %current_phase, "Flow engine: phase advanced by slot");
                        } else {
                            // Slot didn't call submit_phase_result — possible stuck
                            warn!(task_id = %task.id, phase = %phase_str, "Flow engine: slot completed PTY but didn't submit phase result");
                            let _ = state.mission.db().unclaim_board_task(&task.id);
                        }
                    }

                    // Reset slot failure count
                    {
                        let mut fail_map = state.slot_fail_counts.lock().unwrap();
                        fail_map.remove(slot_id);
                    }
                }
                Err(e) => {
                    // Use {:#} to print full anyhow error chain (prevents .context() from hiding inner message)
                    let err_msg = format!("{:#}", e);
                    let is_transient = err_msg.contains("Cannot send message in state:");

                    if is_transient {
                        // Slot not ready (Confirming/Thinking/ToolRunning) — transient failure.
                        // Just unclaim, do NOT track as slot failure. Next tick will retry.
                        debug!(task_id = %task.id, phase = %phase_str, error = %err_msg,
                            "Flow engine: slot not ready (transient), returning task to queue");
                        let _ = state.mission.db().add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.clone(),
                                content: format!("⏳ Flow Phase {} 工位暂未就绪（瞬态），已释放等待重试。\n{}", p.display_name(), err_msg),
                                note_type: Some("note".to_string()),
                                author: Some("flow-engine".to_string()),
                            },
                        );
                        let _ = state.mission.db().unclaim_board_task(&task.id);
                    } else {
                        warn!(task_id = %task.id, phase = %phase_str, error = %err_msg, "Flow engine: PTY send failed");
                        let _ = state.mission.db().add_board_task_note(
                            &missiond_core::types::AddBoardTaskNoteInput {
                                task_id: task.id.clone(),
                                content: format!("❌ Flow Phase {} PTY 失败: {}", p.display_name(), err_msg),
                                note_type: Some("note".to_string()),
                                author: Some("flow-engine".to_string()),
                            },
                        );
                        // Revert to open for retry
                        let _ = state.mission.db().unclaim_board_task(&task.id);
                        // Track failure (only real failures)
                        {
                            let mut fail_map = state.slot_fail_counts.lock().unwrap();
                            let entry = fail_map.entry(slot_id.to_string()).or_insert((0, 0));
                            entry.0 += 1;
                            entry.1 = chrono::Utc::now().timestamp();
                        }
                    }
                }
            }
        }

        _ => {}
    }

    Ok(())
}

/// Ensure a PTY session is running for the given slot (autopilot task execution).
/// Returns true if PTY is available, false if spawn failed.
/// `task_env`: task-specific env overrides (e.g., model routing) merged at spawn time.
pub(crate) async fn ensure_autopilot_pty(state: &AppState, task: &missiond_core::types::BoardTask, slot_id: &str, task_env: HashMap<String, String>) -> bool {
    // Check if session is already running
    if let Some(info) = state.pty.get_status(slot_id).await {
        if info.state != SessionState::Exited {
            // Check if model env changed — kill PTY and respawn if different
            let new_model = task_env.get("ANTHROPIC_MODEL").cloned().unwrap_or_default();
            let model_changed = {
                let models = state.slot_current_model.lock().unwrap();
                models.get(slot_id).map(|m| m != &new_model).unwrap_or(false)
            };
            if model_changed && !new_model.is_empty() {
                info!(task_id = %task.id, slot_id, new_model = %new_model, "Autopilot: model changed, killing PTY for respawn");
                let _ = state.pty.kill(slot_id).await;
                // Fall through to spawn below
            } else if info.state == SessionState::Idle {
                return true;
            } else {
                // Session exists but is busy (Thinking/ToolRunning/etc.) — wait up to 30s for Idle
                debug!(task_id = %task.id, slot_id, state = ?info.state, "Autopilot: slot busy, waiting for Idle");
                for _ in 0..30 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    if let Some(updated) = state.pty.get_status(slot_id).await {
                        if updated.state == SessionState::Idle {
                            return true;
                        }
                        if updated.state == SessionState::Exited {
                            break; // Fall through to spawn below
                        }
                    }
                }
                // Still not idle after 30s — skip this tick, don't waste retry
                if let Some(current) = state.pty.get_status(slot_id).await {
                    if current.state != SessionState::Exited {
                        debug!(task_id = %task.id, slot_id, state = ?current.state, "Autopilot: slot still busy after 30s, skipping this tick");
                        return false;
                    }
                }
                // Exited during wait — fall through to spawn
            }
        }
    }

    // Find slot config
    let slot = state.mission.list_slots()
        .into_iter()
        .find(|s| s.config.id == slot_id);

    let Some(slot) = slot else {
        warn!(task_id = %task.id, slot_id, "Autopilot: slot not found, skipping");
        // Record failure note + increment retry
        let _ = state.mission.db().add_board_task_note(
            &missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.clone(),
                content: format!("❌ Slot `{}` 不存在，无法执行任务。请检查 slots.yaml 配置。", slot_id),
                note_type: Some("note".to_string()),
                author: Some("autopilot".to_string()),
            },
        );
        let new_retry = task.retry_count + 1;
        if new_retry >= task.max_retries {
            let _ = state.mission.db().update_board_task(
                &task.id,
                &missiond_core::types::UpdateBoardTaskInput {
                    status: Some("failed".to_string()),
                    ..Default::default()
                },
            );
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: format!("🛑 Slot `{}` 连续 {} 次不可用，任务标记为 failed。", slot_id, new_retry),
                    note_type: Some("note".to_string()),
                    author: Some("autopilot".to_string()),
                },
            );
            warn!(task_id = %task.id, retries = new_retry, "Autopilot: task failed — slot not found after max retries");
        } else {
            let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
            let _ = state.mission.db().unclaim_board_task(&task.id);
        }
        return false;
    };

    let pty_slot = missiond_core::PTYSlot {
        id: slot.config.id.clone(),
        role: slot.config.role.clone(),
        cwd: slot.config.cwd.as_deref().map(PathBuf::from),
        engine: slot.config.engine,
    };
    let slot_env = slot.config.env.as_ref();
    let mcp_config = slot.config.mcp_config.clone().map(PathBuf::from);
    let (mut extra_env, session_file) = build_slot_tracking_env(slot_id, slot_env).await;

    // Merge task-level env overrides (model routing etc.) — task_env wins over slot defaults
    for (k, v) in &task_env {
        info!(task_id = %task.id, slot_id, key = %k, value = %v, "Autopilot: LLM route override");
        extra_env.insert(k.clone(), v.clone());
    }

    match state.pty.spawn(&pty_slot, PTYSpawnOptions {
        auto_restart: false,
        wait_for_idle: true,
        timeout_secs: Some(120),
        mcp_config,
        dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
        model: slot.config.model.clone(),
        extra_env,
    }).await {
        Ok(_) => {
            capture_slot_session_uuid(state, slot_id, &session_file).await;
            // Record current model for future env-change detection
            if let Some(model) = task_env.get("ANTHROPIC_MODEL") {
                state.slot_current_model.lock().unwrap().insert(slot_id.to_string(), model.clone());
            }
            info!(task_id = %task.id, slot_id, "Autopilot: PTY spawned for task");
            true
        }
        Err(e) => {
            warn!(task_id = %task.id, slot_id, error = %e, "Autopilot: failed to spawn PTY (process may still be loading)");
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: format!("⏳ PTY spawn 失败（120s 超时）。\n\n{}", e),
                    note_type: Some("note".to_string()),
                    author: Some("autopilot".to_string()),
                },
            );
            // Retry with backoff or fail
            let new_retry = task.retry_count + 1;
            if new_retry >= task.max_retries {
                let _ = state.mission.db().update_board_task(
                    &task.id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        status: Some("failed".to_string()),
                        ..Default::default()
                    },
                );
                let _ = state.mission.db().add_board_task_note(
                    &missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.clone(),
                        content: format!("🛑 PTY spawn 连续 {} 次失败，任务标记为 failed。", new_retry),
                        note_type: Some("note".to_string()),
                        author: Some("autopilot".to_string()),
                    },
                );
            } else {
                let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
                let _ = state.mission.db().unclaim_board_task(&task.id);
            }
            false
        }
    }
}

/// Write a flow artifact to a temp file, return the file path.
/// Falls back to inline if file write fails.
pub(crate) fn write_flow_artifact(task_id: &str, name: &str, content: &str) -> Option<String> {
    let dir = format!("/tmp/missiond-flow/{}", task_id);
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = format!("{}/{}.md", dir, name);
    if std::fs::write(&path, content).is_ok() {
        Some(path)
    } else {
        None
    }
}

/// Format an artifact reference: file path instruction if written, or inline fallback.
pub(crate) fn artifact_ref(task_id: &str, name: &str, label: &str, content: &str) -> String {
    if content.len() < 500 {
        // Short content: inline is fine
        return format!("### {}\n{}", label, content);
    }
    match write_flow_artifact(task_id, name, content) {
        Some(path) => format!(
            "### {}\n内容已保存到文件，请先读取：\n```\ncat {}\n```",
            label, path
        ),
        None => format!("### {}\n{}", label, content),
    }
}

pub(crate) fn build_flow_phase_prompt(
    task: &missiond_core::types::BoardTask,
    phase: &missiond_core::types::EngineeringPhase,
    ctx: &missiond_core::types::FlowContext,
    prompts: &crate::prompts::PromptStore,
) -> String {
    let task_id = &task.id;

    let mut base = match phase {
        missiond_core::types::EngineeringPhase::Investigate => {
            format!(
                r#"# 工程任务调查阶段

## 任务
**{title}**

{description}

## 你的工作
1. 阅读任务描述，理解需求
2. 调查相关代码，找到关键文件和依赖
3. 分析现有架构，识别影响范围
4. 记录发现的问题和约束条件

## 完成后
调用 `mission_submit_phase_result` 提交调查报告：
```
taskId: "{task_id}"
artifactType: "investigation_report"
content: "<你的调查报告，包含关键文件、架构分析、问题清单>"
```"#,
                title = task.title,
                description = task.description,
                task_id = task_id,
            )
        }

        missiond_core::types::EngineeringPhase::Plan => {
            let investigation = ctx.investigation_report.as_deref().unwrap_or("(无调查报告)");
            let gemini_advice = ctx.gemini_advice_1.as_deref().unwrap_or("(无 Gemini 建议)");
            let inv_ref = artifact_ref(task_id, "investigation_report", "调查报告", investigation);
            let gem_ref = artifact_ref(task_id, "gemini_advice_1", "Gemini 架构建议", gemini_advice);
            format!(
                r#"# 执行方案制定阶段

## 任务
**{title}**

{description}

## 前置信息

{inv_ref}

{gem_ref}

## 你的工作
1. 综合调查报告和 Gemini 建议，制定详细执行方案
2. 进行第二轮精确调查（验证 Gemini 建议的可行性）
3. 列出具体的代码变更清单（文件、函数、改动内容）
4. 识别风险点和回退方案

## 完成后
调用 `mission_submit_phase_result` 提交执行方案：
```
taskId: "{task_id}"
artifactType: "execution_plan"
content: "<详细执行方案，包含变更清单、风险分析、实施步骤>"
```"#,
                title = task.title,
                description = task.description,
                inv_ref = inv_ref,
                gem_ref = gem_ref,
                task_id = task_id,
            )
        }

        missiond_core::types::EngineeringPhase::Execute => {
            let plan = ctx.execution_plan.as_deref().unwrap_or("(无执行方案)");
            let gemini_advice2 = ctx.gemini_advice_2.as_deref().unwrap_or("(无 Gemini 审查意见)");
            let plan_ref = artifact_ref(task_id, "execution_plan", "执行方案", plan);
            let gem2_ref = artifact_ref(task_id, "gemini_advice_2", "Gemini 方案审查意见", gemini_advice2);
            format!(
                r#"# 执行阶段

## 任务
**{title}**

{plan_ref}

{gem2_ref}

## 你的工作
1. 根据执行方案和 Gemini 审查意见，开始编码实现
2. 遇到问题记录在 execution_result 中
3. 确保代码质量：类型安全、错误处理、测试

## 完成后
调用 `mission_submit_phase_result` 提交执行结果：
```
taskId: "{task_id}"
artifactType: "execution_result"
content: "<执行结果摘要：完成的变更、测试结果、遗留问题>"
```"#,
                title = task.title,
                plan_ref = plan_ref,
                gem2_ref = gem2_ref,
                task_id = task_id,
            )
        }

        missiond_core::types::EngineeringPhase::Finalize => {
            let result = ctx.execution_result.as_deref().unwrap_or("(无执行结果)");
            let result_ref = artifact_ref(task_id, "execution_result", "执行结果", result);
            format!(
                r#"# 收尾阶段

## 任务
**{title}**

{result_ref}

## 你的工作
1. 创建 git commit（描述清晰的 commit message）
2. 如有需要，更新相关 Skill 文件记录新知识
3. 确认所有变更已提交

## 完成后
调用 `mission_submit_phase_result` 提交 commit hash：
```
taskId: "{task_id}"
artifactType: "commit_hash"
content: "<commit hash 或提交摘要>"
```"#,
                title = task.title,
                result_ref = result_ref,
                task_id = task_id,
            )
        }

        _ => format!("Flow phase {:?} — no prompt template available", phase),
    };

    // Inject Decision Engine help protocol for all slot phases (with taskId for auto-linkage)
    if phase.is_slot_phase() {
        let protocol = prompts.help_protocol().replace("{task_id}", task_id);
        base.push_str(&protocol);
    }

    base
}
