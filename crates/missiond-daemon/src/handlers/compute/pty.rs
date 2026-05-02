use anyhow::{anyhow, Result};
use missiond_mcp::tools::{ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;
use crate::helpers::default_mission_home;
use crate::lenient;
use crate::slot_env::build_slot_tracking_env;
use crate::slot_env::capture_slot_session_uuid;
use crate::state::AppState;
use missiond_core::PTYSpawnOptions;
use std::path::PathBuf;

#[derive(Deserialize)]
struct PTYSpawnArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(
        rename = "waitForIdle",
        default,
        deserialize_with = "lenient::option_bool"
    )]
    wait_for_idle: Option<bool>,
    #[serde(rename = "timeoutSecs", default)]
    timeout_secs: Option<u64>,
    #[serde(
        rename = "autoRestart",
        default,
        deserialize_with = "lenient::option_bool"
    )]
    auto_restart: Option<bool>,
    #[serde(rename = "mcpConfigPath", default)]
    mcp_config_path: Option<String>,
}

#[derive(Deserialize)]
struct PTYSendArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    message: String,
    #[serde(
        rename = "waitForResponse",
        default,
        deserialize_with = "lenient::option_bool"
    )]
    wait_for_response: Option<bool>,
    #[serde(rename = "timeoutMs", default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct PTYKillArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYScreenArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(default)]
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct PTYHistoryArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYStatusArgs {
    #[serde(rename = "slotId")]
    slot_id: Option<String>,
}

#[derive(Deserialize)]
struct PTYConfirmArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    response: Value,
}

#[derive(Deserialize)]
struct PTYInterruptArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYLogsArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYScreenshotArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tools
    if name == "mission_pty_read" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("screen");
        return match action {
            "screen" => handle_inner(state, "mission_pty_screen", args).await,
            "history" => handle_inner(state, "mission_pty_history", args).await,
            "logs" => handle_inner(state, "mission_pty_logs", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    if name == "mission_pty_signal" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("kill");
        return match action {
            "kill" => handle_inner(state, "mission_pty_kill", args).await,
            "interrupt" => handle_inner(state, "mission_pty_interrupt", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== PTY =====
        "mission_pty_spawn" => {
            let PTYSpawnArgs {
                slot_id,
                wait_for_idle,
                timeout_secs,
                auto_restart,
                mcp_config_path,
            } = serde_json::from_value(args)?;
            let slot = state
                .mission
                .list_slots()
                .into_iter()
                .find(|s| s.config.id == slot_id)
                .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;

            // Resolve target_project_root for project-bound CLI spawn
            // (intent-worker.lisp :: invariant project-root-spawn-cwd). For
            // ClaudeCode unresolved we preserve the slot's cwd (lisp-surveyor
            // is a non-project-bound meta agent), the spawner re-checks. For
            // Gemini/Codex unresolved, the spawner returns a structured error.
            let cwd_path = slot.config.cwd.as_deref().map(PathBuf::from);
            let resolved_cwd = if let Some(ref cwd) = cwd_path {
                match crate::slot_orchestrator::project_root::resolve_target_project_root(
                    None,
                    Some(cwd),
                    None,
                    &state.project_registry,
                )
                .await
                {
                    Ok(r) => Some(r.project_root),
                    Err(_) => Some(cwd.clone()), // spawner enforces engine policy
                }
            } else {
                None
            };

            let pty_slot = missiond_core::PTYSlot {
                id: slot.config.id.clone(),
                role: slot.config.role.clone(),
                cwd: resolved_cwd,
                engine: slot.config.engine,
            };

            // NOTE: perm_injector is now invoked from inside spawn_tracked_slot
            // (Phase 3 of the permission persistence architecture upgrade) so every
            // spawn path gets coverage. No need to call it here.

            // Resolve MCP config: arg > slot config > None
            let mcp_config = mcp_config_path
                .or(slot.config.mcp_config.clone())
                .map(PathBuf::from);

            let wait = wait_for_idle.unwrap_or(false);
            let info = crate::slot_orchestrator::spawner::spawn_tracked_slot(
                &state.pty,
                &state.store,
                &state.pty_session_uuids,
                &state.project_registry,
                state.permission.learned(),
                &pty_slot,
                PTYSpawnOptions {
                    auto_restart: auto_restart.unwrap_or(false),
                    wait_for_idle: wait,
                    timeout_secs,
                    mcp_config,
                    dangerously_skip_permissions: slot
                        .config
                        .dangerously_skip_permissions
                        .unwrap_or(false),
                    model: slot.config.model.clone(),
                    reasoning_effort: slot.config.reasoning_effort.clone(),
                    search_enabled: slot.config.search_enabled.unwrap_or(false),
                    sandbox: slot.config.sandbox.clone(),
                    approval_policy: slot.config.approval_policy.clone(),
                    extra_env: std::collections::HashMap::new(),
                    initial_prompt: slot.config.initial_prompt.clone(),
                },
                slot.config.env.as_ref(),
            )
            .await?;

            Ok(ToolResult::json(&info))
        }
        "mission_pty_send" => {
            let PTYSendArgs {
                slot_id,
                message,
                wait_for_response,
                timeout_ms,
            } = serde_json::from_value(args)?;

            if wait_for_response.unwrap_or(false) {
                // Blocking mode: send and wait for response
                let runtime_config = match WorkstationRuntimeConfig::load_for_project_root(
                    slot_project_root(state, &slot_id).as_deref(),
                ) {
                    Ok(config) => config,
                    Err(err) => {
                        return Ok(ToolResult::structured_error(ToolError::new(
                            "V3_BLUEPRINT_CONFIG_ERROR",
                            err.to_string(),
                        )))
                    }
                };
                let timeout_ms = runtime_config.clamp_pty_send_timeout_ms(timeout_ms);
                let res = state.pty.send(&slot_id, &message, timeout_ms).await?;
                let state = state
                    .pty
                    .get_status(&slot_id)
                    .await
                    .map(|s| format!("{:?}", s.state))
                    .unwrap_or_else(|| "unknown".to_string());
                Ok(ToolResult::json(&serde_json::json!({
                    "delivered": true,
                    "response": res.response,
                    "durationMs": res.duration_ms,
                    "state": state,
                })))
            } else {
                // Fire-and-forget: send and return immediately
                state.pty.send_fire_and_forget(&slot_id, &message).await?;
                Ok(ToolResult::json(&serde_json::json!({
                    "delivered": true,
                    "mode": "fire-and-forget",
                    "hint": "Use pty_status to poll for completion (state returns to Idle when done)",
                })))
            }
        }
        "mission_pty_kill" => {
            let PTYKillArgs { slot_id } = serde_json::from_value(args)?;
            state.pty.kill(&slot_id).await?;
            // Clear from compact restart pending set (slot is being rebuilt)
            state
                .pending_compact_restart
                .lock()
                .unwrap()
                .remove(&slot_id);
            // Requeue any Running submit tasks assigned to this slot
            let requeued = state
                .store
                .requeue_running_tasks_for_slot(&slot_id)
                .await
                .unwrap_or(0);
            // Release any board task claims held by this slot
            let claims_released = state
                .store
                .release_board_claims_by_executor(&slot_id)
                .await
                .unwrap_or(0);
            Ok(ToolResult::json(
                &serde_json::json!({ "success": true, "slotId": slot_id, "requeuedTasks": requeued, "claimsReleased": claims_released }),
            ))
        }
        "mission_pty_screen" => {
            let PTYScreenArgs { slot_id, lines } = serde_json::from_value(args)?;
            if let Some(n) = lines {
                let last = state.pty.get_last_lines(&slot_id, n).await?;
                Ok(ToolResult::text(last.join("\n")))
            } else {
                Ok(ToolResult::text(state.pty.get_screen(&slot_id).await?))
            }
        }
        "mission_pty_history" => {
            let PTYHistoryArgs { slot_id } = serde_json::from_value(args)?;
            let history = state.pty.get_history(&slot_id).await;
            Ok(ToolResult::json(&history))
        }
        "mission_pty_status" => {
            let PTYStatusArgs { slot_id } =
                serde_json::from_value(args).unwrap_or(PTYStatusArgs { slot_id: None });
            if let Some(slot_id) = slot_id {
                let status = state.pty.get_status(&slot_id).await;
                match status {
                    Some(info) => {
                        let mut obj = serde_json::to_value(&info).unwrap_or_default();
                        // Enrich with session_uuid and JSONL activity
                        if let Ok(Some(session_uuid)) = state.store.get_slot_session(&slot_id).await
                        {
                            obj["sessionId"] = json!(session_uuid);
                            if let Ok(Some(conv)) =
                                state.store.get_conversation(&session_uuid).await
                            {
                                if let Some(ref jsonl_path) = conv.jsonl_path {
                                    if let Ok(metadata) = std::fs::metadata(jsonl_path) {
                                        if let Ok(modified) = metadata.modified() {
                                            let secs_ago =
                                                modified.elapsed().unwrap_or_default().as_secs();
                                            obj["lastActivitySecsAgo"] = json!(secs_ago);
                                        }
                                    }
                                }
                            }
                        }
                        // Attach JSONL progress stats (tool call counts, current tool)
                        {
                            let progress = state.slot_progress.read().await;
                            if let Some(sp) = progress.get(&slot_id) {
                                if sp.total_calls > 0 {
                                    obj["progress"] = serde_json::to_value(sp).unwrap_or_default();
                                }
                            }
                        }
                        // Attach last response (eliminates screen-blank misjudgment)
                        {
                            let responses = state.slot_last_responses.read().await;
                            if let Some(resp) = responses.get(&slot_id) {
                                // Truncate to 2KB for status response
                                let truncated = if resp.len() > 2048 {
                                    let mut end = 2048;
                                    while end > 0 && !resp.is_char_boundary(end) {
                                        end -= 1;
                                    }
                                    format!("{}...(truncated)", &resp[..end])
                                } else {
                                    resp.clone()
                                };
                                obj["lastResponse"] = json!(truncated);
                            }
                        }
                        Ok(ToolResult::json(&obj))
                    }
                    None => Ok(ToolResult::json(&serde_json::Value::Null)),
                }
            } else {
                let all = state.pty.get_all_status().await;
                Ok(ToolResult::json(&all))
            }
        }
        "mission_pty_confirm" => {
            let PTYConfirmArgs { slot_id, response } = serde_json::from_value(args)?;
            let response_echo = response.clone();

            // Capture pending confirm info before confirming (for auto-allow recording)
            let pending = state.pty.get_pending_confirm(&slot_id).await;

            // Map caller intent to a semantic option. The PTY layer turns this
            // into Down/Up + Enter navigation; mission_pty_confirm never sends
            // raw numeric shortcut keys to ClaudeCode/Codex/Gemini.
            let resp = confirm_response_from_value(&response)?;

            // Determine if this is an approval (Yes, Option(1), Option(2))
            let is_approval = matches!(
                resp,
                missiond_core::ConfirmResponse::Yes
                    | missiond_core::ConfirmResponse::Option(1)
                    | missiond_core::ConfirmResponse::Option(2)
            );
            // Capture "is option 2 (allowlist)" before resp is moved into pty.confirm()
            let is_allowlist_choice = matches!(resp, missiond_core::ConfirmResponse::Option(2));

            state.pty.confirm(&slot_id, resp).await?;

            // Learn permission from user's decision.
            // When user picked Option(2) "Yes, don't ask again for X", extract X as param_pattern
            // so Bash subcommand allowlists (e.g. python3:*) survive instead of being persisted as
            // a blanket "allow ALL Bash" entry. Also extract any "commands in <path>" hint and
            // learn a second entry at project scope so cross-cwd spawns pick it up.
            if let Some(ref info) = pending {
                if let Some(ref tool) = info.tool {
                    if let Some(learned) = state.permission.learned() {
                        let decision = if is_approval { "allow" } else { "deny" };
                        let extracted = if is_allowlist_choice {
                            info.options
                                .get(1)
                                .and_then(|opt| crate::permission_extract::extract_confirm(opt))
                        } else {
                            None
                        };
                        let param_pattern = extracted.as_ref().map(|e| e.pattern.clone());
                        let project_path = extracted.as_ref().and_then(|e| e.project_path.clone());
                        if let Some(status) = state.pty.get_status(&slot_id).await {
                            // Always learn at role scope.
                            if let Err(e) = learned.learn(
                                "role",
                                &status.role,
                                &tool.name,
                                decision,
                                param_pattern.as_deref(),
                            ) {
                                tracing::warn!(error = %e, "Failed to record learned permission (role)");
                            } else {
                                tracing::info!(
                                    role = %status.role,
                                    tool = %tool.name,
                                    param = ?param_pattern,
                                    decision,
                                    slot_id = %slot_id,
                                    "Permission learned from user confirmation (role scope)"
                                );
                            }
                            // If the dialog carried a project hint, also learn at project scope.
                            if let Some(path) = project_path.as_deref() {
                                let project_id = state
                                    .project_registry
                                    .read()
                                    .await
                                    .resolve(path)
                                    .map(|s| s.to_string());
                                if let Some(pid) = project_id {
                                    if let Err(e) = learned.learn(
                                        "project",
                                        &pid,
                                        &tool.name,
                                        decision,
                                        param_pattern.as_deref(),
                                    ) {
                                        tracing::warn!(error = %e, "Failed to record learned permission (project)");
                                    } else {
                                        tracing::info!(
                                            project_id = %pid,
                                            tool = %tool.name,
                                            param = ?param_pattern,
                                            decision,
                                            slot_id = %slot_id,
                                            "Permission learned from user confirmation (project scope)"
                                        );
                                    }
                                } else {
                                    tracing::debug!(
                                        path = %path,
                                        "No registered project matches confirm dialog path; skipping project-scope learn"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            Ok(ToolResult::json(&serde_json::json!({
                "success": true,
                "slotId": slot_id,
                "response": response_echo,
            })))
        }
        "mission_pty_interrupt" => {
            let PTYInterruptArgs { slot_id } = serde_json::from_value(args)?;
            state.pty.interrupt(&slot_id).await?;
            Ok(ToolResult::json(
                &serde_json::json!({ "success": true, "slotId": slot_id }),
            ))
        }
        "mission_pty_logs" => {
            let PTYLogsArgs { slot_id } = serde_json::from_value(args)?;
            let status = state.pty.get_status(&slot_id).await;
            let status = status.ok_or_else(|| anyhow!("PTY session not found"))?;
            // Find JSONL path from slot→session DB mapping
            let jsonl_path = match state.store.get_all_slot_sessions().await.ok() {
                Some(sessions) => {
                    if let Some((_, session_uuid)) =
                        sessions.into_iter().find(|(sid, _)| sid == &slot_id)
                    {
                        state
                            .store
                            .get_conversation(&session_uuid)
                            .await
                            .ok()
                            .flatten()
                            .and_then(|c| c.jsonl_path)
                    } else {
                        None
                    }
                }
                None => None,
            };
            #[cfg(unix)]
            let hint = if let Some(ref jp) = jsonl_path {
                format!("tail -f {}", jp)
            } else {
                format!("tail -f {}", status.log_file.display())
            };
            #[cfg(windows)]
            let hint = if let Some(ref jp) = jsonl_path {
                format!("Get-Content -Path \"{}\" -Wait -Tail 50", jp)
            } else {
                format!(
                    "Get-Content -Path \"{}\" -Wait -Tail 50",
                    status.log_file.display()
                )
            };

            Ok(ToolResult::json(&serde_json::json!({
                "slotId": slot_id,
                "logFile": status.log_file,
                "jsonlFile": jsonl_path,
                "hint": hint,
            })))
        }

        "mission_pty_screenshot" => {
            let PTYScreenshotArgs { slot_id } = serde_json::from_value(args)?;
            let screenshots_dir = default_mission_home().join("screenshots");
            std::fs::create_dir_all(&screenshots_dir)?;

            // Try browser-based screenshot first (real xterm.js canvas)
            let (request_id, rx) = state.screenshot_broker.request(&slot_id).await;
            let _ = &request_id; // request is already broadcast by broker

            let browser_result = tokio::time::timeout(state.screenshot_broker.timeout, rx).await;

            let (path, source) = match browser_result {
                Ok(Ok(Ok(result))) => {
                    let ts = chrono::Utc::now().timestamp_millis();
                    let filename = format!("{}-{}.png", slot_id, ts);
                    let path = screenshots_dir.join(&filename);
                    std::fs::write(&path, &result.png_data)?;
                    (path, "browser")
                }
                _ => {
                    // Fallback: backend rendering via alacritty grid
                    info!(slot_id = %slot_id, "Browser screenshot unavailable, using backend rendering");
                    let path = state.pty.screenshot(&slot_id, &screenshots_dir).await?;
                    (path, "backend")
                }
            };

            Ok(ToolResult::json(&serde_json::json!({
                "path": path.to_string_lossy(),
                "source": source,
                "hint": "Use the Read tool to view this PNG image"
            })))
        }

        _ => Err(anyhow!("Unknown pty tool: {name}")),
    }
}

fn slot_project_root(state: &AppState, slot_id: &str) -> Option<String> {
    state
        .mission
        .get_slot(slot_id)
        .and_then(|slot| slot.config.project_root.or(slot.config.cwd))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn confirm_response_from_value(response: &Value) -> anyhow::Result<missiond_core::ConfirmResponse> {
    match response {
        Value::Bool(true) => Ok(missiond_core::ConfirmResponse::Yes),
        Value::Bool(false) => Ok(missiond_core::ConfirmResponse::No),
        Value::Number(n) => Ok(confirm_response_from_option_index(
            n.as_u64().unwrap_or(1) as usize
        )),
        Value::String(s) => {
            let trimmed = s.trim();
            let lower = trimmed.to_ascii_lowercase();
            if matches!(lower.as_str(), "y" | "yes" | "allow" | "approve" | "1") {
                Ok(missiond_core::ConfirmResponse::Yes)
            } else if matches!(lower.as_str(), "n" | "no" | "deny" | "reject" | "cancel") {
                Ok(missiond_core::ConfirmResponse::No)
            } else if let Some(rest) = lower.strip_prefix("option:") {
                let n = rest
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| anyhow!("invalid mission_pty_confirm option index: {trimmed}"))?;
                Ok(confirm_response_from_option_index(n))
            } else if let Ok(n) = trimmed.parse::<usize>() {
                Ok(confirm_response_from_option_index(n))
            } else {
                Err(anyhow!(
                    "mission_pty_confirm response must be a boolean, option index, or yes/no/allow/cancel label; use mission_pty_send for raw text"
                ))
            }
        }
        _ => Ok(missiond_core::ConfirmResponse::Yes),
    }
}

fn confirm_response_from_option_index(index: usize) -> missiond_core::ConfirmResponse {
    match index {
        0 | 1 => missiond_core::ConfirmResponse::Yes,
        3 => missiond_core::ConfirmResponse::No,
        n => missiond_core::ConfirmResponse::Option(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission_extract::extract_confirm;
    use std::sync::Arc;
    use tempfile::tempdir;

    /// E2E: verify that selecting option 2 as caller intent maps to
    /// ConfirmResponse::Option(2), extracts the "don't ask" pattern from the
    /// option text, learns it via LearnedPermissions, and syncs the entry into
    /// `.claude/settings.local.json`. The PTY layer later executes this intent
    /// with arrow-key navigation, never with numeric shortcut keystrokes.
    #[test]
    fn option2_intent_learns_and_syncs_to_settings_json() {
        // Step 1: option index 2 maps to ConfirmResponse::Option(2)
        let response = Value::Number(serde_json::Number::from(2u64));
        let resp = confirm_response_from_value(&response).expect("confirm response");
        assert!(
            matches!(resp, missiond_core::ConfirmResponse::Option(2)),
            "option index 2 must map to Option(2)"
        );
        let is_allowlist = matches!(resp, missiond_core::ConfirmResponse::Option(2));
        assert!(is_allowlist);

        // Step 2: extract param pattern from the second option text
        let option_text = "Yes, and don't ask again for: python3:*";
        let extracted = extract_confirm(option_text).expect("extract");
        assert_eq!(extracted.pattern.as_str(), "python3:*");
        assert!(extracted.project_path.is_none());

        // Step 3: learn permission (simulates what mission_pty_confirm does)
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("perms.yaml");
        let learned = Arc::new(missiond_core::LearnedPermissions::new(&yaml_path).unwrap());
        learned
            .learn(
                "role",
                "coder",
                "Bash",
                "allow",
                Some(extracted.pattern.as_str()),
            )
            .expect("learn should succeed");

        // Step 4: sync to settings.local.json (simulates next slot spawn)
        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        crate::slot_orchestrator::perm_injector::sync_learned_to_local_settings(
            &cwd, "coder", None, None, &learned,
        );

        // Step 5: verify the entry appears in settings.local.json
        let settings_path = cwd.join(".claude/settings.local.json");
        assert!(
            settings_path.exists(),
            "settings.local.json must be created"
        );
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        let allow = v["permissions"]["allow"]
            .as_array()
            .expect("allow must be array");
        assert!(
            allow.iter().any(|e| e == "Bash(python3:*)"),
            "Bash(python3:*) not found in allowlist: {:?}",
            allow
        );
    }

    #[test]
    fn confirm_response_is_semantic_not_raw_text() {
        let trimmed = confirm_response_from_value(&Value::String("2 ".into()))
            .expect("trimmed option index should parse");
        assert!(matches!(trimmed, missiond_core::ConfirmResponse::Option(2)));

        let raw = confirm_response_from_value(&Value::String("type this raw text".into()));
        assert!(
            raw.is_err(),
            "mission_pty_confirm must not send arbitrary raw text; use mission_pty_send"
        );
    }
}
