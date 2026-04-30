use anyhow::{anyhow, Result};
use missiond_core::pty::SessionState;
use missiond_core::types::CreateBoardTaskInput;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;
use crate::slot_dispatch::SlotAcquireGuard;
use crate::state::AppState;

/// Rate limit: max delegates per minute (per Jarvis session).
const MAX_DELEGATES_PER_MINUTE: usize = 5;

/// Roles excluded from auto-selection (meta agents, Jarvis itself).
const EXCLUDED_ROLES: &[&str] = &["jarvis", "memory", "supervisor", "decision"];

/// Phase 6.2: Valid intent whitelist — reject unknown intents instead of silent fallback.
const VALID_INTENTS: &[&str] = &["code", "ops", "research", "general"];

/// Phase 6.3: Context injection size limits.
const MAX_ENTRY_CHARS: usize = 500; // Per KB/Skill entry
const MAX_CONTEXT_CHARS: usize = 2000; // Total context block
const MAX_DESCRIPTION_CHARS: usize = 16000; // Final description

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let objective = match args.get("objective").and_then(|v| v.as_str()) {
        Some(o) if !o.trim().is_empty() => o.trim(),
        _ => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "'objective' is required and must be non-empty",
            )))
        }
    };

    // Phase 6.2: Strict intent whitelist — fail-fast on unknown intent
    let intent = match args.get("intent").and_then(|v| v.as_str()) {
        Some(i) if VALID_INTENTS.contains(&i) => i,
        Some(i) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                &format!("Invalid intent '{}'. Valid: {:?}", i, VALID_INTENTS),
            )))
        }
        None => "general",
    };

    let priority = args
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let requested_timeout_secs = args.get("timeout_secs").and_then(|v| v.as_i64());

    let depends_on: Vec<String> = args
        .get("depends_on")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let context_hints: Vec<String> = args
        .get("context_hints")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let model_arg = string_arg(&args, &["model"]).map(str::to_string);
    let model_profile_arg =
        string_arg(&args, &["model_profile", "modelProfile"]).map(str::to_string);

    let cwd = args.get("cwd").and_then(|v| v.as_str());

    // Resolve target_project_root (intent-flow.lisp ::
    // F-task-delegate-autoprovision :: s1b). When cwd is supplied, reject
    // if it does not resolve under a registered project. When cwd is absent,
    // we leave target_project_root as None and the auto-provision branch will
    // surface the issue (compute_slot create requires a registered cwd).
    let target_project_root = if let Some(cwd_val) = cwd {
        match crate::slot_orchestrator::project_root::resolve_target_project_root(
            None,
            Some(std::path::Path::new(cwd_val)),
            None,
            &state.project_registry,
        )
        .await
        {
            Ok(r) => Some(r.project_root.to_string_lossy().to_string()),
            Err(e) => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        "PROJECT_ROOT_UNRESOLVED",
                        format!("task_delegate cwd unresolved: {}", e),
                    )
                    .with_suggestion(
                        "register the project via mission_project(action=\"init\") or pass cwd inside an existing project",
                    ),
                ));
            }
        }
    } else {
        None
    };

    let runtime_config = match WorkstationRuntimeConfig::load_for_project_root(
        target_project_root.as_deref(),
    ) {
        Ok(config) => config,
        Err(err) => {
            return Ok(ToolResult::structured_error(
                    ToolError::new("V3_BLUEPRINT_CONFIG_ERROR", err.to_string())
                        .with_suggestion(
                            "ensure <project>/.missiond/v3/missiond-blueprint.lisp contains workstation-config",
                        ),
                ));
        }
    };
    let timeout_secs = runtime_config.clamp_timeout_secs(requested_timeout_secs);

    // Intent → template mapping
    let template = match intent {
        "code" => "coder",
        "ops" => "ops",
        "research" => "researcher",
        _ => "coder",
    };
    let default_model_profile = if model_arg.is_none() {
        runtime_config
            .default_model_profile_for_template(template)
            .map(str::to_string)
    } else {
        None
    };
    let effective_model_profile = model_profile_arg
        .as_deref()
        .or(default_model_profile.as_deref());
    let requested_model = match super::compute_slot::resolve_model_projection(
        template,
        model_arg.as_deref(),
        effective_model_profile,
    ) {
        Ok(model) => model,
        Err(message) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                message,
            )))
        }
    };

    // Phase 6.1: Find idle slot with RAII guard (atomic check+reserve).
    // intent-flow.lisp F-task-delegate-autoprovision :: s2 requires
    // slot.project_root == target_project_root for reuse.
    let guard = find_and_reserve_slot(
        state,
        template,
        target_project_root.as_deref(),
        requested_model.as_deref(),
    )
    .await;
    let assignee = guard
        .as_ref()
        .map(|g| g.slot_id().to_string())
        .unwrap_or_default();

    // 2. If no idle slot, try auto-provision dynamic slot
    let (assignee, provisioned) = if !assignee.is_empty() {
        (assignee, false)
    } else {
        // Phase 6.2: Guard uses template, not intent — prevents intent escape
        if template == "ops" {
            // Queue without assignee; autopilot will pick up when a slot frees
            (String::new(), false)
        } else {
            match auto_provision_slot(
                state,
                template,
                objective,
                &runtime_config,
                cwd,
                model_arg.as_deref(),
                effective_model_profile,
            )
            .await
            {
                Ok(id) => (id, true),
                Err(e) => {
                    tracing::warn!("Auto-provision failed, queueing without assignee: {}", e);
                    (String::new(), false)
                }
            }
        }
    };
    // guard is dropped here (if Some) → auto-releases slot dispatch lock

    // 3. Build context from hints (Phase 6.3: with size limits)
    let mut description = objective.to_string();
    if !context_hints.is_empty() {
        let keywords = context_hints.join(" ");
        if let Ok(context) = build_context(state, &keywords).await {
            if !context.is_empty() {
                description = format!("{}\n\n## 预加载上下文\n{}", objective, context);
            }
        }
    }

    // Phase 6.3: Enforce description size limit
    if description.len() > MAX_DESCRIPTION_CHARS {
        let original_len = description.len();
        let end = crate::helpers::char_boundary_at(&description, MAX_DESCRIPTION_CHARS);
        description = format!(
            "{}...(truncated from {} bytes)",
            &description[..end],
            original_len
        );
        tracing::warn!(
            original_len,
            "task_delegate: description truncated to {}B",
            MAX_DESCRIPTION_CHARS
        );
    }

    // 4. Create BoardTask
    let input = CreateBoardTaskInput {
        title: truncate_title(objective),
        description: Some(description),
        priority: Some(priority.to_string()),
        category: Some("dev".to_string()),
        assignee: if assignee.is_empty() {
            None
        } else {
            Some(assignee.clone())
        },
        auto_execute: Some(true),
        depends_on: if depends_on.is_empty() {
            None
        } else {
            Some(depends_on)
        },
        timeout_secs: Some(timeout_secs),
        context_intent: Some(intent.to_string()),
        ..Default::default()
    };

    let task_id = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    // 5. Trigger immediate dispatch (don't wait 60s autopilot tick)
    state.board_dispatch_notify.notify_one();

    Ok(ToolResult::json_pretty(&json!({
        "task_id": task_id,
        "assignee": if assignee.is_empty() { Value::Null } else { Value::String(assignee) },
        "status": "queued",
        "intent": intent,
        "template": template,
        "model": requested_model,
        "model_profile": effective_model_profile,
        "provisioned_new_slot": provisioned,
        "timeout_secs": timeout_secs,
        "hint": "结果将通过 TaskNotification 自动回报。也可用 mission_board_get 查询进度。"
    })))
}

/// Phase 6.1: Find an idle slot and atomically reserve it via RAII guard.
/// Returns a SlotAcquireGuard that auto-releases on drop.
///
/// Slot reuse honors `intent-worker.lisp :: invariant project-root-spawn-cwd`:
/// when `target_project_root` is supplied, reject any candidate whose
/// `SlotConfig.project_root` (or fallback `cwd`) does not match. Mismatched
/// slots auto-release their guard immediately and the loop continues.
async fn find_and_reserve_slot<'a>(
    state: &'a AppState,
    template: &str,
    target_project_root: Option<&str>,
    requested_model: Option<&str>,
) -> Option<SlotAcquireGuard<'a>> {
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
        if !super::compute_slot::model_projection_matches(
            slot.config.model.as_deref(),
            requested_model,
        ) {
            continue;
        }
        // Project-root reuse check. Prefer the resolved project_root field;
        // fall back to cwd (legacy slots still set cwd directly).
        if let Some(target) = target_project_root {
            let slot_root = slot
                .config
                .project_root
                .as_deref()
                .or(slot.config.cwd.as_deref());
            match slot_root {
                Some(root) if root == target => {}
                _ => continue, // mismatch — do not reuse
            }
        }
        // Atomically: acquire guard → check idle
        let guard = match state.slot_dispatch.try_acquire_guard(&slot.config.id) {
            Some(g) => g,
            None => continue, // Another caller is dispatching to this slot
        };
        if let Some(info) = state.pty.get_status(guard.slot_id()).await {
            if info.state == SessionState::Idle {
                return Some(guard); // Guard held — caller responsible via RAII drop
            }
        }
        // guard dropped here → auto-releases
    }
    None
}

/// Auto-provision a dynamic slot. Returns the new slot_id.
async fn auto_provision_slot(
    state: &AppState,
    template: &str,
    objective: &str,
    runtime_config: &WorkstationRuntimeConfig,
    cwd: Option<&str>,
    model: Option<&str>,
    model_profile: Option<&str>,
) -> Result<String> {
    // Check quota
    let active = state
        .store
        .count_active_dynamic_slots()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if active >= 5 {
        return Err(anyhow!("Dynamic slot quota full ({}/5)", active));
    }

    // V3 workstation-config :: ttl-policy dynamic-slot projection.
    // Delegated auto-provision has no caller TTL override, so use the
    // blueprint default and clamp it through the same path as direct
    // mission_compute_slot create.
    let ttl = auto_provision_slot_ttl_secs(runtime_config);

    // Build args for compute_slot create — projects V3 workstation-config
    // execution-ownership :: delegated-boardtask. The slot is provisioned
    // idle (suppress_initial_prompt=true) so Autopilot remains the sole
    // task-prompt owner once the queued BoardTask is dispatched.
    let create_args =
        build_compute_slot_create_args(template, objective, ttl, cwd, model, model_profile);

    // Delegate to existing compute_slot handler
    let result = super::compute_slot::handle(state, "mission_compute_slot", create_args).await?;

    // Parse the job_accepted response to get the slot info
    if let Some(missiond_mcp::tools::ToolContent::Text { text }) = result.content.first() {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if parsed.get("job_id").is_some() {
                return Err(anyhow!(
                    "Slot spawning async (job_id: {}), task will be picked up by autopilot",
                    parsed["job_id"].as_str().unwrap_or("unknown")
                ));
            }
        }
    }

    Err(anyhow!("Failed to parse compute_slot response"))
}

fn auto_provision_slot_ttl_secs(runtime_config: &WorkstationRuntimeConfig) -> i64 {
    runtime_config.clamp_slot_ttl_secs(None)
}

fn string_arg<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Phase 6.3: Build context from KB/Skills with size limits.
async fn build_context(state: &AppState, keywords: &str) -> Result<String> {
    let mut parts = Vec::new();
    let mut total_len = 0;

    // Search KB (FTS5, take first 3)
    if let Ok(entries) = state.store.kb_search(keywords, None).await {
        for entry in entries.iter().take(3) {
            let summary = truncate_str(&entry.summary, MAX_ENTRY_CHARS);
            let line = format!("- [KB:{}] {}", entry.key, summary);
            total_len += line.len();
            if total_len > MAX_CONTEXT_CHARS {
                break;
            }
            parts.push(line);
        }
    }

    // Search Skills (take first 3)
    if total_len < MAX_CONTEXT_CHARS {
        let skill_results = state.skills.search(keywords);
        for skill in skill_results.iter().take(3) {
            let desc = skill.description.as_deref().unwrap_or("");
            let desc = truncate_str(desc, MAX_ENTRY_CHARS);
            let line = format!("- [Skill:{}] {}", skill.name, desc);
            total_len += line.len();
            if total_len > MAX_CONTEXT_CHARS {
                break;
            }
            parts.push(line);
        }
    }

    Ok(parts.join("\n"))
}

/// Truncate a string to max_chars, respecting char boundaries.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// V3 workstation-config :: execution-ownership delegated-boardtask projection.
///
/// Build the JSON args passed to `mission_compute_slot create` when
/// `mission_task_delegate` auto-provisions a dynamic slot for a queued
/// BoardTask. `suppress_initial_prompt` is hardcoded `true` so the slot
/// starts idle and Autopilot remains the sole task-prompt owner — see
/// `compute_slot::effective_initial_prompt`. Direct `mission_compute_slot
/// create` callers omit the flag and keep the legacy warm-up behaviour.
fn build_compute_slot_create_args(
    template: &str,
    objective: &str,
    ttl: i64,
    cwd: Option<&str>,
    model: Option<&str>,
    model_profile: Option<&str>,
) -> Value {
    let mut create_args = json!({
        "action": "create",
        "template": template,
        "objective": objective,
        "max_ttl": ttl,
        "suppress_initial_prompt": true,
    });
    if let Some(cwd_val) = cwd {
        create_args["cwd"] = Value::String(cwd_val.to_string());
    }
    if let Some(model_val) = model {
        create_args["model"] = Value::String(model_val.to_string());
    }
    if let Some(profile_val) = model_profile {
        create_args["model_profile"] = Value::String(profile_val.to_string());
    }
    create_args
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── V3 execution-ownership :: delegated-boardtask projection ─────────
    //
    // Tests for `build_compute_slot_create_args`: pure helper, no AppState.
    // Pins the rule that mission_task_delegate auto-provisioning always sets
    // `suppress_initial_prompt: true` so the dynamic slot starts idle and
    // Autopilot becomes the sole task-prompt owner.

    #[test]
    fn create_args_always_suppresses_initial_prompt() {
        let args = build_compute_slot_create_args("coder", "ship the fix", 3600, None, None, None);
        assert_eq!(args["suppress_initial_prompt"], json!(true));
    }

    #[test]
    fn auto_provision_ttl_projects_v3_dynamic_slot_default() {
        let ttl = auto_provision_slot_ttl_secs(&WorkstationRuntimeConfig::default());
        assert_eq!(
            ttl,
            crate::context::v3_blueprint_runtime::DEFAULT_SLOT_TTL_SECS
        );
        assert_ne!(ttl, 3600);
    }

    #[test]
    fn create_args_carry_template_objective_and_ttl() {
        let args =
            build_compute_slot_create_args("researcher", "investigate", 7200, None, None, None);
        assert_eq!(args["action"], json!("create"));
        assert_eq!(args["template"], json!("researcher"));
        assert_eq!(args["objective"], json!("investigate"));
        assert_eq!(args["max_ttl"], json!(7200));
    }

    #[test]
    fn create_args_pass_through_optional_fields() {
        let args = build_compute_slot_create_args(
            "ops",
            "patrol",
            3600,
            Some("/Users/jinchen/Projects/missiond"),
            Some("sonnet"),
            Some("daily-sonnet"),
        );
        assert_eq!(args["cwd"], json!("/Users/jinchen/Projects/missiond"));
        assert_eq!(args["model"], json!("sonnet"));
        assert_eq!(args["model_profile"], json!("daily-sonnet"));
        assert_eq!(args["suppress_initial_prompt"], json!(true));
    }

    #[test]
    fn create_args_omit_optional_fields_when_absent() {
        let args = build_compute_slot_create_args("coder", "x", 3600, None, None, None);
        assert!(args.get("cwd").is_none());
        assert!(args.get("model").is_none());
        assert!(args.get("model_profile").is_none());
    }
}
