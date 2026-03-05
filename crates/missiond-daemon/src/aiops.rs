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
pub(crate) async fn health_scan(state: &AppState) {
    let servers = &state.infra.servers;
    let mut set = tokio::task::JoinSet::new();

    for server in servers {
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

        if !healthy {
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

            if let Err(e) = state.incident_tx.try_send(incident) {
                warn!(server_id = %server_id, "Incident channel full, dropping health check: {}", e);
            }
        }
    }
}

// ============ AIOps Reactor ============

/// Process a single incident from the event bus:
/// 1. Dedupe check (5-minute window on dedupe_key)
/// 2. Insert into DB
/// 3. Create a high-priority Board task for slot-ops
pub(crate) async fn process_incident(state: &AppState, incident: missiond_core::types::MissionIncident) {
    let db = state.mission.db();
    let dedupe_key = format!("{}:{}", incident.source, incident.title);

    // PtySlot incidents: dispatch remediation to a Claude Code (Opus) slot
    if matches!(incident.source, missiond_core::types::IncidentSource::PtySlot) {
        if let Some(slot_id) = incident.raw_payload.get("slot_id").and_then(|v| v.as_str()) {
            create_pty_remediation_task(state, slot_id, &incident.title, &incident.description);
            // Still insert incident record below, but skip default Board task creation
            if let Err(e) = db.insert_incident(
                &incident.id,
                &incident.severity.to_string(),
                &incident.source.to_string(),
                &incident.title,
                &incident.description,
                incident.server_id.as_deref(),
                Some(&incident.raw_payload.to_string()),
                None,
                &dedupe_key,
            ) {
                warn!(error = %e, "AIOps: failed to insert incident record");
            }
            return;
        }
    }

    // 5-minute dedupe window — drop duplicate incidents
    match db.has_recent_incident(&dedupe_key, 300) {
        Ok(true) => {
            debug!(
                dedupe_key = %dedupe_key,
                "Incident dedupe: dropping duplicate within 5min window"
            );
            return;
        }
        Ok(false) => {}
        Err(e) => {
            warn!(error = %e, "Incident dedupe check failed, proceeding anyway");
        }
    }

    // Truncate raw_payload for prompt safety (max 2000 chars)
    let raw_str = incident.raw_payload.to_string();
    let truncated_payload = if raw_str.len() > 2000 {
        format!("{}... (truncated, {} total)", &raw_str[..2000], raw_str.len())
    } else {
        raw_str
    };

    // Build Board task description with structured context
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

    // Determine priority and auto-execute from severity
    // Warning → Board backlog only (autoExecute=false), High/Critical → auto-dispatch to slot-ops
    let (priority, auto_execute) = match incident.severity {
        missiond_core::types::IncidentSeverity::Critical => ("urgent", true),
        missiond_core::types::IncidentSeverity::High => ("high", true),
        missiond_core::types::IncidentSeverity::Warning => ("medium", false),
    };

    // Create board task
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
    };

    let board_task_id = match db.create_board_task(&task_input) {
        Ok(task) => {
            info!(
                incident_id = %incident.id,
                board_task_id = %task.id,
                severity = %incident.severity,
                title = %incident.title,
                "AIOps: incident → board task created"
            );
            Some(task.id)
        }
        Err(e) => {
            error!(error = %e, "AIOps: failed to create board task for incident");
            None
        }
    };

    // Fire submit_notify so Autopilot picks up the new task
    state.event_bus.publish(crate::event_bus::DaemonEvent::TaskCreated { task_id: String::new() });

    // Insert incident record into DB
    if let Err(e) = db.insert_incident(
        &incident.id,
        &incident.severity.to_string(),
        &incident.source.to_string(),
        &incident.title,
        &incident.description,
        incident.server_id.as_deref(),
        Some(&incident.raw_payload.to_string()),
        board_task_id.as_deref(),
        &dedupe_key,
    ) {
        warn!(error = %e, "AIOps: failed to insert incident record");
    }
}

// ============ PTY Auto-Remediation ============

/// Dispatch a remediation Board task to a Claude Code (Opus) slot.
/// The slot will use MCP tools (pty_screen, pty_send, pty_kill) to observe and fix.
pub(crate) fn create_pty_remediation_task(
    state: &AppState,
    target_slot_id: &str,
    incident_title: &str,
    incident_description: &str,
) {
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
        assignee: Some("slot-coder-1".to_string()),
        auto_execute: Some(true),
        server: None,
        project: None,
        due_date: None,
        parent_id: None,
        prompt_template: None,
        hidden: None,
        flow_template: None,
        depends_on: None,
    };

    match state.mission.db().create_board_task(&task_input) {
        Ok(task) => {
            info!(
                task_id = %task.id,
                target_slot = %target_slot_id,
                "PTY remediation: Board task created for Opus slot"
            );
            // Notify autopilot to pick up immediately
            state.event_bus.publish(crate::event_bus::DaemonEvent::TaskCreated { task_id: String::new() });
        }
        Err(e) => {
            error!(error = %e, "PTY remediation: failed to create Board task");
        }
    }
}
