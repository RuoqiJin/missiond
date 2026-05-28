use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
use crate::engine::control_plane_kernel::{ControlPlaneKernel, UpsertTaskContractCommand};
use crate::state::AppState;

pub(super) async fn handle(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini_trace");
    match action {
        "gemini_trace" => gemini_trace(state, args).await,
        "gemini_stats" => gemini_stats(state).await,
        "gemini_watch" => {
            let mut args = args;
            if let Some(wa) = args.get("watch_action").cloned() {
                args.as_object_mut()
                    .map(|m| m.insert("action".to_string(), wa));
            }
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("status");
            gemini_watch(state, action).await
        }
        "gemini_auth" => super::auth::handle(state, args).await,
        "jarvis_logs" => jarvis_logs(state, args).await,
        "jarvis_trace" => jarvis_trace(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}

pub(super) async fn handle_legacy(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_jarvis_logs" => jarvis_logs(state, args).await,
        "mission_jarvis_trace" => jarvis_trace(state, args).await,
        "mission_gemini_trace" => gemini_trace(state, args).await,
        "mission_gemini_stats" => gemini_stats(state).await,
        "mission_gemini_content" => gemini_content(state, args).await,
        "mission_gemini_watch" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("status");
            gemini_watch(state, action).await
        }
        "mission_gemini_auth" => super::auth::handle(state, args).await,
        _ => Ok(ToolResult::error(format!(
            "Unknown LLM trace tool: {}",
            name
        ))),
    }
}

async fn jarvis_logs(state: &AppState, args: Value) -> Result<ToolResult> {
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

async fn jarvis_trace(state: &AppState, args: Value) -> Result<ToolResult> {
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

async fn gemini_trace(state: &AppState, args: Value) -> Result<ToolResult> {
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

async fn gemini_stats(state: &AppState) -> Result<ToolResult> {
    let stats = state
        .store
        .gemini_log_stats()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&stats))
}

async fn gemini_content(state: &AppState, args: Value) -> Result<ToolResult> {
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

            let router_config = RouterRuntimeConfig::load_for_current_dir()
                .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))?;
            let model = router_config.flow_gemini_model;
            let st = state.clone();
            let watch_model = model.clone();
            let handle = tokio::spawn(async move {
                gemini_watch_loop(st, watch_model).await;
            });

            *state.gemini_watch_handle.lock().await = Some(handle);
            info!("Gemini watch started");
            Ok(ToolResult::text(format!(
                "✅ Gemini 监测已启动。每 10 分钟探测一次 {}，恢复后自动创建 Board 通知。",
                model
            )))
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

async fn gemini_watch_loop(state: AppState, model: String) {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    let probe_timeout = Duration::from_secs(600);

    loop {
        if !state.gemini_watch_active.load(Ordering::Relaxed) {
            break;
        }

        let attempt = state.gemini_watch_attempts.fetch_add(1, Ordering::Relaxed) + 1;
        info!(attempt, model = %model, "Gemini watch: probing...");

        let result = tokio::time::timeout(probe_timeout, async {
            tokio::process::Command::new("gemini")
                .args(["-p", "say OK", "-m", model.as_str(), "-o", "text", "--yolo"])
                .output()
                .await
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
            info!(attempt, "Gemini watch: recovered");
            state.gemini_watch_active.store(false, Ordering::Relaxed);
            state.gemini_watch_started_at.store(0, Ordering::Relaxed);

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
                runtime_metadata: Some(gemini_recovery_runtime_metadata(&model, attempt as u64)),
            };
            match state.store.create_board_task(&input).await {
                Ok(task) => {
                    if let Err(err) = ControlPlaneKernel::new(&state)
                        .upsert_task_contract_command(UpsertTaskContractCommand {
                            task_id: task.id.to_string(),
                            project_id: task.project.clone(),
                            runtime_metadata: task.runtime_metadata.clone(),
                        })
                        .await
                    {
                        warn!(
                            "Gemini watch: failed to upsert task_contracts for recovery notice: {}",
                            err
                        );
                    }
                }
                Err(e) => {
                    warn!("Gemini watch: failed to create board task: {}", e);
                }
            }
            break;
        }

        info!(attempt, "Gemini watch: still unavailable, waiting 10 min");
        tokio::time::sleep(Duration::from_secs(600)).await;
    }
}

fn gemini_recovery_runtime_metadata(model: &str, attempt: u64) -> Value {
    json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "gemini_watch",
        "control_state": "task_contracts",
        "dispatch_metadata": {
            "task_class": "gemini-recovery-notice",
            "model": model,
            "attempt": attempt,
            "completion_protocol": "diagnostic notice only; no worker-controlled terminal state"
        },
        "read_scope": [],
        "write_scope": [],
        "must_not_touch": [],
        "capability_grant_ids": [],
        "sandbox_profile": "system-diagnostic-notice",
        "projection_policy": "description_notes_are_projection_only"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_recovery_metadata_is_diagnostic_projection() {
        let metadata = gemini_recovery_runtime_metadata("gemini-2.5-pro", 3);
        assert_eq!(metadata["source"], "gemini_watch");
        assert_eq!(metadata["control_state"], "task_contracts");
        assert_eq!(
            metadata["dispatch_metadata"]["task_class"],
            "gemini-recovery-notice"
        );
        assert_eq!(metadata["write_scope"].as_array().unwrap().len(), 0);
        assert_eq!(metadata["sandbox_profile"], "system-diagnostic-notice");
    }
}
