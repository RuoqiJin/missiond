use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use missiond_mcp::tools::{ToolResult, ToolError, error_codes};
use missiond_core::types::CreateBoardTaskInput;
use missiond_core::pty::SessionState;

use crate::state::AppState;

/// Max timeout: 2 hours.
const MAX_TIMEOUT_SECS: i64 = 7200;
/// Default timeout: 30 minutes.
const DEFAULT_TIMEOUT_SECS: i64 = 1800;
/// Rate limit: max delegates per minute (per Jarvis session).
const MAX_DELEGATES_PER_MINUTE: usize = 5;

/// Roles excluded from auto-selection (meta agents, Jarvis itself).
const EXCLUDED_ROLES: &[&str] = &["jarvis", "memory", "supervisor", "decision"];

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let objective = match args.get("objective").and_then(|v| v.as_str()) {
        Some(o) if !o.trim().is_empty() => o.trim(),
        _ => return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::MISSING_PARAM, "'objective' is required and must be non-empty"),
        )),
    };

    let intent = args.get("intent").and_then(|v| v.as_str()).unwrap_or("general");
    let priority = args.get("priority").and_then(|v| v.as_str()).unwrap_or("medium");
    let timeout_secs = args.get("timeout_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS)
        .max(60); // min 1 minute

    let depends_on: Vec<String> = args.get("depends_on")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let context_hints: Vec<String> = args.get("context_hints")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let cwd = args.get("cwd").and_then(|v| v.as_str());

    // Intent → template mapping
    let template = match intent {
        "code" => "coder",
        "ops" => "ops",
        "research" => "researcher",
        _ => "coder",
    };

    // 1. Find idle slot matching role
    let slot_id = find_idle_slot(state, template).await;

    // 2. If no idle slot, try auto-provision dynamic slot
    let (assignee, provisioned) = match slot_id {
        Some(id) => (id, false),
        None => {
            // ops requires explicit approval — don't auto-provision
            if intent == "ops" {
                // Queue without assignee; autopilot will pick up when a slot frees
                (String::new(), false)
            } else {
                match auto_provision_slot(state, template, objective, timeout_secs, cwd).await {
                    Ok(id) => (id, true),
                    Err(e) => {
                        tracing::warn!("Auto-provision failed, queueing without assignee: {}", e);
                        (String::new(), false)
                    }
                }
            }
        }
    };

    // 3. Build context from hints
    let mut description = objective.to_string();
    if !context_hints.is_empty() {
        // Pre-build context from KB/Skills
        let keywords = context_hints.join(" ");
        if let Ok(context) = build_context(state, &keywords).await {
            if !context.is_empty() {
                description = format!("{}\n\n## 预加载上下文\n{}", objective, context);
            }
        }
    }

    // 4. Create BoardTask
    let input = CreateBoardTaskInput {
        title: truncate_title(objective),
        description: Some(description),
        priority: Some(priority.to_string()),
        category: Some("dev".to_string()),
        assignee: if assignee.is_empty() { None } else { Some(assignee.clone()) },
        auto_execute: Some(true),
        depends_on: if depends_on.is_empty() { None } else { Some(depends_on) },
        timeout_secs: Some(timeout_secs),
        context_intent: Some(intent.to_string()),
        ..Default::default()
    };

    let task_id = state.mission.db().create_board_task(&input)
        .map_err(|e| anyhow!("DB error: {}", e))?;

    // 5. Trigger immediate dispatch (don't wait 60s autopilot tick)
    state.board_dispatch_notify.notify_one();

    Ok(ToolResult::json_pretty(&json!({
        "task_id": task_id,
        "assignee": if assignee.is_empty() { Value::Null } else { Value::String(assignee) },
        "status": "queued",
        "intent": intent,
        "template": template,
        "provisioned_new_slot": provisioned,
        "timeout_secs": timeout_secs,
        "hint": "结果将通过 TaskNotification 自动回报。也可用 mission_board_get 查询进度。"
    })))
}

/// Find an idle static or dynamic slot matching the template's role.
async fn find_idle_slot(state: &AppState, template: &str) -> Option<String> {
    let target_role = match template {
        "coder" | "researcher" => "coder",
        "ops" => "operator",
        _ => "coder",
    };

    let slots = state.mission.list_slots();
    for slot in &slots {
        // Skip excluded roles
        if EXCLUDED_ROLES.iter().any(|r| slot.config.role.contains(r)) {
            continue;
        }
        if slot.config.role != target_role {
            continue;
        }
        // Check if idle
        if let Some(info) = state.pty.get_status(&slot.config.id).await {
            if info.state == SessionState::Idle {
                return Some(slot.config.id.clone());
            }
        }
    }
    None
}

/// Auto-provision a dynamic slot. Returns the new slot_id.
async fn auto_provision_slot(
    state: &AppState,
    template: &str,
    objective: &str,
    timeout_secs: i64,
    cwd: Option<&str>,
) -> Result<String> {
    // Check quota
    let active = state.mission.db().count_active_dynamic_slots()
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if active >= 5 {
        return Err(anyhow!("Dynamic slot quota full ({}/5)", active));
    }

    // TTL = max(1h, timeout_secs + 300s buffer)
    let ttl = (timeout_secs + 300).max(3600);

    // Build args for compute_slot create
    let mut create_args = json!({
        "action": "create",
        "template": template,
        "objective": objective,
        "max_ttl": ttl,
    });
    if let Some(cwd_val) = cwd {
        create_args["cwd"] = Value::String(cwd_val.to_string());
    }

    // Delegate to existing compute_slot handler
    let result = super::compute_slot::handle(state, "mission_compute_slot", create_args).await?;

    // Parse the job_accepted response to get the slot info
    // compute_slot create returns a job — we need to extract the slot_id from the job
    // The slot_id is generated inside compute_slot, but we need it now.
    // Instead of duplicating logic, let's parse the response.
    if let Some(missiond_mcp::tools::ToolContent::Text { text }) = result.content.first() {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            // Job accepted — the slot_id is in the background task
            // For now, we can't get it synchronously. Instead, queue without assignee
            // and let autopilot pick it up once the slot spawns.
            if parsed.get("job_id").is_some() {
                return Err(anyhow!("Slot spawning async (job_id: {}), task will be picked up by autopilot",
                    parsed["job_id"].as_str().unwrap_or("unknown")));
            }
        }
    }

    Err(anyhow!("Failed to parse compute_slot response"))
}

/// Build context from KB/Skills using keywords.
async fn build_context(state: &AppState, keywords: &str) -> Result<String> {
    let mut parts = Vec::new();

    // Search KB (FTS5, take first 3)
    if let Ok(entries) = state.mission.db().kb_search(keywords, None) {
        for entry in entries.iter().take(3) {
            parts.push(format!("- [KB:{}] {}", entry.key, entry.summary));
        }
    }

    // Search Skills (take first 3)
    let skill_results = state.skills.search(keywords);
    for skill in skill_results.iter().take(3) {
        let desc = skill.description.as_deref().unwrap_or("");
        parts.push(format!("- [Skill:{}] {}", skill.name, desc));
    }

    Ok(parts.join("\n"))
}

/// Truncate objective to 80 chars for title.
fn truncate_title(s: &str) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() <= 80 {
        first_line.to_string()
    } else {
        let mut end = 77;
        while end > 0 && !first_line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &first_line[..end])
    }
}
