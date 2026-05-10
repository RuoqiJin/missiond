//! AIOps — health monitoring, incident processing, and auto-remediation.
//!
//! Extracted from decision_engine.rs (Phase 3 PR4) to separate operational
//! concerns from the decision routing logic.

use serde_json::json;
use tracing::{debug, error, info, warn};

use crate::state::AppState;

// ============ AIOps CronSensor ============

/// Phase 2: Concurrent health scan of all servers with healthEndpoint configured.
/// Uses JoinSet for parallel HTTP GET with 5s timeout per server.
/// Pre-checks internet connectivity via Google to avoid false alarms when network is down.
pub(crate) async fn health_scan(state: &AppState) {
    // Pre-check: verify internet connectivity before scanning servers.
    // If the local network is down, all health checks will fail — skip to avoid noise.
    let inet_ok = match state
        .http_client
        .get("https://connectivitycheck.gstatic.com/generate_204")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp.status().as_u16() == 204,
        Err(_) => false,
    };
    if !inet_ok {
        warn!("AIOps health_scan: internet connectivity check failed (Google unreachable), skipping all server checks");
        return;
    }

    let servers: Vec<_> = state
        .infra
        .read()
        .unwrap()
        .servers
        .iter()
        .cloned()
        .collect();
    let mut set = tokio::task::JoinSet::new();

    for server in &servers {
        let endpoint = match &server.health_endpoint {
            Some(e) => e.clone(),
            None => continue,
        };
        let client = state.http_client.clone();
        let server_id = server.id.clone();
        let server_name = server.name.clone();

        set.spawn(async move {
            let healthy = match client
                .get(&endpoint)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            };
            (server_id, server_name, endpoint, healthy)
        });
    }

    while let Some(result) = set.join_next().await {
        let (server_id, server_name, endpoint, healthy) = match result {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "Health check task panicked");
                continue;
            }
        };

        let dedupe_key = format!("health_check:{} 健康检查失败", server_name);

        if healthy {
            // Recovery: auto-close the open alert task if one exists
            match state.store.close_task_by_dedupe_key(&dedupe_key).await {
                Ok(Some(task)) => {
                    // Add recovery note
                    let _ = state
                        .store
                        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                            task_id: task.id.to_string(),
                            content: format!(
                                "✅ 已自动恢复 ({})",
                                chrono::Utc::now().format("%H:%M UTC")
                            ),
                            note_type: Some("progress".to_string()),
                            author: Some("aiops".to_string()),
                        })
                        .await;
                    info!(server_id = %server_id, task_id = %task.id, "AIOps: server recovered, auto-closed alert task");
                }
                Ok(None) => {} // No open alert — server was already healthy
                Err(e) => warn!(error = %e, "AIOps: failed to close recovery task"),
            }
        } else {
            let incident = missiond_core::types::MissionIncident {
                id: format!("inc-{}", uuid::Uuid::new_v4()),
                severity: missiond_core::types::IncidentSeverity::High,
                source: missiond_core::types::IncidentSource::HealthCheck,
                title: format!("{} 健康检查失败", server_name),
                description: format!(
                    "服务器 {} ({}) 的健康端点 {} 无响应或返回非 200。\n\n\
                     建议操作：\n\
                     1. mission_reachability(target=\"{}\") 确认网络连通性\n\
                     2. mission_os_diagnose(target=\"{}\") 检查系统状态\n\
                     3. 检查 Docker 容器状态",
                    server_name, server_id, endpoint, server_id, server_id
                ),
                server_id: Some(server_id.clone()),
                raw_payload: json!({
                    "endpoint": endpoint,
                    "server_id": server_id,
                    "server_name": server_name,
                }),
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            // v2 bus: IncidentEvent::Reported → IncidentEvent subscriber
            // triages via `aiops::process_incident`.
            if let Err(e) = state
                .bus
                .publish_incident(missiond_core::event::events::IncidentEvent::Reported {
                    incident,
                })
                .await
            {
                warn!(server_id = %server_id, error = %e, "Failed to publish health-check incident");
            }
        }
    }
}

// ============ AIOps Reactor ============

/// Parse a free-form severity string into an `IncidentSeverity`.
/// Unknown values default to `Warning` so user-supplied input stays harmless.
pub(crate) fn parse_severity(s: &str) -> missiond_core::types::IncidentSeverity {
    match s {
        "critical" => missiond_core::types::IncidentSeverity::Critical,
        "high" => missiond_core::types::IncidentSeverity::High,
        _ => missiond_core::types::IncidentSeverity::Warning,
    }
}

/// Parse a free-form source string into an `IncidentSource`.
/// Unknown values default to `Manual`.
pub(crate) fn parse_source(s: &str) -> missiond_core::types::IncidentSource {
    match s {
        "health_check" => missiond_core::types::IncidentSource::HealthCheck,
        "deploy_center" => missiond_core::types::IncidentSource::DeployCenter,
        "sentry" => missiond_core::types::IncidentSource::Sentry,
        "pty_slot" => missiond_core::types::IncidentSource::PtySlot,
        _ => missiond_core::types::IncidentSource::Manual,
    }
}

/// Build the dedupe key used to aggregate alerts about the same logical incident.
///
/// Pure helper — used both by the bus reactor and the `mission_incident`
/// `remediate`/`status`/`close` actions.
pub(crate) fn build_dedupe_key(
    source: &missiond_core::types::IncidentSource,
    title: &str,
) -> String {
    format!("{}:{}", source, title)
}

/// Whether a board task linked to an incident is safe for an aiops `close`
/// action to mark as `done`.
///
/// Conservative: only auto-/explicit-close for tasks that aiops itself
/// created. We identify those by `category == "ops"` and a non-empty
/// `dedupe_key` (aiops always sets one). User-owned tasks lack the
/// dedupe_key/ops marker and must be closed manually.
pub(crate) fn is_safe_to_close_task(
    task: &missiond_core::types::BoardTask,
) -> Result<(), &'static str> {
    if task.dedupe_key.is_none() {
        return Err("linked board task has no dedupe_key — likely user-owned, refuse auto-close");
    }
    if task.category != "ops" {
        return Err("linked board task category is not 'ops' — refuse auto-close");
    }
    Ok(())
}

/// Process a single incident: state-based alert aggregation.
///
/// Instead of time-window dedup (which races with the 5min scan interval),
/// we use the Board task lifecycle as the single source of truth:
/// - If an open task with the same dedupe_key exists → append note (aggregate)
/// - If no open task → create a new one
/// - Incident records are always inserted for full audit trail
pub(crate) async fn process_incident(
    state: &AppState,
    incident: missiond_core::types::MissionIncident,
) {
    let _ = triage_incident(state, incident).await;
}

/// Same as [`process_incident`], but returns the linked board task id if one
/// was created or found. The handler `remediate` action calls this directly
/// to obtain a deterministic return value without going through the bus.
pub(crate) async fn triage_incident(
    state: &AppState,
    incident: missiond_core::types::MissionIncident,
) -> Option<String> {
    let dedupe_key = build_dedupe_key(&incident.source, &incident.title);

    // PtySlot incidents: dispatch remediation to a Claude Code (Opus) slot
    if matches!(
        incident.source,
        missiond_core::types::IncidentSource::PtySlot
    ) {
        if let Some(slot_id) = incident.raw_payload.get("slot_id").and_then(|v| v.as_str()) {
            // Dedup: check if an active remediation task already exists for this slot+tool
            // Now checks open, in_progress, and queued tasks (not just open)
            let existing = state
                .store
                .find_open_task_by_dedupe_key(&dedupe_key)
                .await
                .ok()
                .flatten();
            let pty_task_id = if let Some(ref task) = existing {
                let note = format!(
                    "🔄 告警重复触发 +1 ({})",
                    chrono::Utc::now().format("%m-%d %H:%M UTC")
                );
                let _ = state
                    .store
                    .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.to_string(),
                        content: note,
                        note_type: Some("progress".to_string()),
                        author: Some("aiops".to_string()),
                    })
                    .await;
                debug!(task_id = %task.id, status = task.status.as_str(), "AIOps: PTY alert aggregated into existing task");
                Some(task.id.to_string())
            } else {
                create_pty_remediation_task(
                    state,
                    slot_id,
                    &incident.title,
                    &incident.description,
                    &dedupe_key,
                )
                .await
            };
            if let Err(e) = state
                .store
                .insert_incident(
                    &incident.id,
                    &incident.severity.to_string(),
                    &incident.source.to_string(),
                    &incident.title,
                    &incident.description,
                    incident.server_id.as_deref(),
                    Some(&incident.raw_payload.to_string()),
                    pty_task_id.as_deref(),
                    &dedupe_key,
                )
                .await
            {
                warn!(error = %e, "AIOps: failed to insert incident record");
            }
            return pty_task_id;
        }
    }

    // State-based aggregation: check if an open Board task already tracks this alert
    let existing_task = match state.store.find_open_task_by_dedupe_key(&dedupe_key).await {
        Ok(task) => task,
        Err(e) => {
            warn!(error = %e, "AIOps: failed to query open task by dedupe_key");
            None
        }
    };

    let board_task_id = if let Some(ref task) = existing_task {
        // Aggregate: append note to existing open task instead of creating a new one
        let note_content = format!(
            "🔄 告警重复触发 ({})",
            chrono::Utc::now().format("%m-%d %H:%M UTC"),
        );
        if let Err(e) = state
            .store
            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.to_string(),
                content: note_content,
                note_type: Some("progress".to_string()),
                author: Some("aiops".to_string()),
            })
            .await
        {
            warn!(error = %e, "AIOps: failed to append note to existing task");
        }
        // Touch updated_at so it floats to the top
        let _ = state
            .store
            .update_board_task(
                task.id.as_str(),
                &missiond_core::types::UpdateBoardTaskInput {
                    ..Default::default()
                },
            )
            .await;
        debug!(
            task_id = %task.id,
            dedupe_key = %dedupe_key,
            "AIOps: alert aggregated into existing task"
        );
        Some(task.id.to_string())
    } else {
        // No open task — create a new one with dedupe_key for future aggregation
        let raw_str = incident.raw_payload.to_string();
        let truncated_payload = if raw_str.len() > 2000 {
            let end = crate::helpers::char_boundary_at(&raw_str, 2000);
            format!(
                "{}... (truncated, {} total)",
                &raw_str[..end],
                raw_str.len()
            )
        } else {
            raw_str
        };

        let description = format!(
            "## AIOps 自动检测到异常\n\n\
             **严重等级**: {severity}\n\
             **来源**: {source}\n\
             **服务器**: {server}\n\
             **时间**: {time}\n\n\
             ### 描述\n{desc}\n\n\
             ### 原始数据\n```json\n{payload}\n```",
            severity = incident.severity,
            source = incident.source,
            server = incident.server_id.as_deref().unwrap_or("N/A"),
            time = incident.created_at,
            desc = incident.description,
            payload = truncated_payload,
        );

        let (priority, auto_execute) = match incident.severity {
            missiond_core::types::IncidentSeverity::Critical => ("urgent", true),
            missiond_core::types::IncidentSeverity::High => ("high", true),
            missiond_core::types::IncidentSeverity::Warning => ("medium", false),
        };

        let task_input = missiond_core::types::CreateBoardTaskInput {
            title: format!("[AIOps] {}", incident.title),
            description: Some(description),
            priority: Some(priority.to_string()),
            category: Some("ops".to_string()),
            assignee: Some("slot-ops".to_string()),
            auto_execute: Some(auto_execute),
            server: incident.server_id.clone(),
            project: None,
            due_date: None,
            parent_id: None,
            prompt_template: None,
            hidden: None,
            flow_template: None,
            depends_on: None,
            dedupe_key: Some(dedupe_key.clone()),
            timeout_secs: None,
            context_intent: None,
        };

        match state.store.create_board_task(&task_input).await {
            Ok(task) => {
                info!(
                    incident_id = %incident.id,
                    board_task_id = %task.id,
                    severity = %incident.severity,
                    title = %incident.title,
                    "AIOps: incident → board task created"
                );
                // Notify autopilot
                let _ = state
                    .bus
                    .publish_task(missiond_core::event::events::TaskEvent::Created {
                        task_id: String::new(),
                    })
                    .await;
                Some(task.id.to_string())
            }
            Err(e) => {
                error!(error = %e, "AIOps: failed to create board task for incident");
                None
            }
        }
    };

    // Always insert incident record for full audit trail
    if let Err(e) = state
        .store
        .insert_incident(
            &incident.id,
            &incident.severity.to_string(),
            &incident.source.to_string(),
            &incident.title,
            &incident.description,
            incident.server_id.as_deref(),
            Some(&incident.raw_payload.to_string()),
            board_task_id.as_deref(),
            &dedupe_key,
        )
        .await
    {
        warn!(error = %e, "AIOps: failed to insert incident record");
    }

    board_task_id
}

// ============ PTY Auto-Remediation ============

/// Dispatch a remediation Board task to a Claude Code (Opus) slot.
/// The slot will use MCP tools (pty_screen, pty_send, pty_kill) to observe and fix.
///
/// Returns the new board task id on success, or None if creation failed.
pub(crate) async fn create_pty_remediation_task(
    state: &AppState,
    target_slot_id: &str,
    incident_title: &str,
    incident_description: &str,
    dedupe_key: &str,
) -> Option<String> {
    if let Ok(Some(task)) = state.store.find_open_task_by_dedupe_key(dedupe_key).await {
        let note_content = format!(
            "PTY/MCP 自愈事件重复触发 ({})，已聚合到同一根因任务；target_slot={target_slot_id}; title={incident_title}",
            chrono::Utc::now().format("%m-%d %H:%M UTC"),
        );
        if let Err(e) = state
            .store
            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.to_string(),
                content: note_content,
                note_type: Some("progress".to_string()),
                author: Some("aiops".to_string()),
            })
            .await
        {
            warn!(error = %e, "PTY remediation: failed to append duplicate note");
        }
        let _ = state
            .store
            .update_board_task(
                task.id.as_str(),
                &missiond_core::types::UpdateBoardTaskInput {
                    ..Default::default()
                },
            )
            .await;
        info!(
            task_id = %task.id,
            target_slot = %target_slot_id,
            dedupe_key = %dedupe_key,
            "PTY remediation: duplicate incident aggregated into existing task"
        );
        return Some(task.id.to_string());
    }

    let description = format!(
        "## PTY 工位自愈任务\n\n\
         **目标工位**: `{target_slot}`\n\
         **问题**: {title}\n\n\
         ### 问题详情\n{desc}\n\n\
         ### 操作指南\n\
         1. `mission_pty_screen(slotId=\"{target_slot}\")` 查看目标工位当前屏幕\n\
         2. `mission_pty_status(slotId=\"{target_slot}\")` 检查工位状态\n\
         3. 根据屏幕内容判断修复方式：\n\
            - MCP 不可用 → `mission_pty_kill(slotId=\"{target_slot}\")` 重启（auto_restart 会自动恢复）\n\
            - 卡在确认提示 → `mission_pty_send(slotId=\"{target_slot}\", message=\"...\")` 发送合适回复\n\
            - 其他情况 → 先观察，再决定\n\
         4. 操作后再次 `mission_pty_screen` 确认修复效果\n\
         5. 重复 observe-act 直到目标工位恢复正常\n\n\
         **注意**: 如果无法修复，在任务中说明原因即可。",
        target_slot = target_slot_id,
        title = incident_title,
        desc = incident_description,
    );

    let task_input = missiond_core::types::CreateBoardTaskInput {
        title: format!("[自愈] {}", incident_title),
        description: Some(description),
        priority: Some("high".to_string()),
        category: Some("ops".to_string()),
        assignee: None,
        auto_execute: Some(false),
        server: None,
        project: None,
        due_date: None,
        parent_id: None,
        prompt_template: None,
        hidden: None,
        flow_template: None,
        depends_on: None,
        dedupe_key: Some(dedupe_key.to_string()),
        timeout_secs: None,
        context_intent: None,
    };

    match state.store.create_board_task(&task_input).await {
        Ok(task) => {
            let id = task.id.to_string();
            info!(
                task_id = %id,
                target_slot = %target_slot_id,
                "PTY remediation: Board incident task created for operator review"
            );
            Some(id)
        }
        Err(e) => {
            error!(error = %e, "PTY remediation: failed to create Board task");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_core::types::{
        BoardTask, BoardTaskStatus, IncidentSeverity, IncidentSource, TaskId,
    };

    fn make_task(category: &str, dedupe_key: Option<&str>) -> BoardTask {
        BoardTask {
            id: TaskId::from_trusted("task-1".to_string()),
            title: "t".into(),
            description: String::new(),
            status: BoardTaskStatus::Open,
            priority: "medium".into(),
            category: category.into(),
            project: None,
            server: None,
            due_date: None,
            parent_id: None,
            assignee: None,
            auto_execute: false,
            prompt_template: None,
            hidden: false,
            retry_count: 0,
            max_retries: 2,
            order_idx: 0,
            created_at: String::new(),
            updated_at: String::new(),
            claim_executor_id: None,
            claim_executor_type: None,
            claimed_at: None,
            flow_phase: None,
            flow_context: None,
            flow_template: None,
            depends_on: Vec::new(),
            lease_expires_at: None,
            dedupe_key: dedupe_key.map(|s| s.to_string()),
            timeout_secs: None,
            context_intent: None,
            trigger_source: None,
            notes_count: 0,
        }
    }

    #[test]
    fn dedupe_key_combines_source_and_title() {
        let key = build_dedupe_key(&IncidentSource::HealthCheck, "Disk full");
        assert_eq!(key, "health_check:Disk full");
    }

    #[test]
    fn dedupe_key_distinguishes_sources() {
        let a = build_dedupe_key(&IncidentSource::Manual, "Disk full");
        let b = build_dedupe_key(&IncidentSource::HealthCheck, "Disk full");
        assert_ne!(a, b);
    }

    #[test]
    fn parse_severity_known_values() {
        assert_eq!(parse_severity("critical"), IncidentSeverity::Critical);
        assert_eq!(parse_severity("high"), IncidentSeverity::High);
        assert_eq!(parse_severity("warning"), IncidentSeverity::Warning);
    }

    #[test]
    fn parse_severity_falls_back_to_warning() {
        assert_eq!(parse_severity("garbage"), IncidentSeverity::Warning);
        assert_eq!(parse_severity(""), IncidentSeverity::Warning);
    }

    #[test]
    fn parse_source_known_values() {
        assert!(matches!(
            parse_source("health_check"),
            IncidentSource::HealthCheck
        ));
        assert!(matches!(parse_source("pty_slot"), IncidentSource::PtySlot));
        assert!(matches!(parse_source("garbage"), IncidentSource::Manual));
    }

    #[test]
    fn safe_close_requires_dedupe_key() {
        let task = make_task("ops", None);
        assert!(is_safe_to_close_task(&task).is_err());
    }

    #[test]
    fn safe_close_requires_ops_category() {
        let task = make_task("dev", Some("k"));
        assert!(is_safe_to_close_task(&task).is_err());
    }

    #[test]
    fn safe_close_accepts_aiops_owned_task() {
        let task = make_task("ops", Some("health_check:foo"));
        assert!(is_safe_to_close_task(&task).is_ok());
    }
}
