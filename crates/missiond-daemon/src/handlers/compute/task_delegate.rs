use anyhow::{anyhow, Result};
use missiond_core::event::events::BoardEvent;
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

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_swarm_run" {
        return handle_swarm_run(state, args).await;
    }

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
    let delegation_metadata = DelegationMetadata {
        task_class: string_arg(&args, &["task_class", "taskClass"]).map(str::to_string),
        pool_hint: string_arg(&args, &["pool_hint", "poolHint"]).map(str::to_string),
        engine_hint: string_arg(&args, &["engine_hint", "engineHint"]).map(str::to_string),
        context_pack_path: string_arg(&args, &["context_pack_path", "contextPackPath"])
            .map(str::to_string),
        read_scope: string_list_arg(&args, &["read_scope", "readScope"]),
        write_scope: string_list_arg(&args, &["write_scope", "writeScope"]),
        must_not_touch: string_list_arg(&args, &["must_not_touch", "mustNotTouch"]),
        acceptance: string_list_arg(
            &args,
            &["acceptance", "acceptance_commands", "acceptanceCommands"],
        ),
    };

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
        &runtime_config,
    ) {
        Ok(model) => model,
        Err(message) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                message,
            )))
        }
    };

    // V3 workstation-pool gemini researcher binding. Research-class delegations
    // without an explicit Claude raw model token go to the read-only Gemini
    // researcher slot when one is registered. The signal is the effective
    // model_profile: research-default (or its aliases) routes to gemini, every
    // other Claude profile (coding-default-opus-4-7, daily-sonnet, quick-haiku,
    // explicit caller model) falls through to the existing Claude path.
    let routes_to_gemini_researcher = model_arg.is_none()
        && effective_model_profile
            .map(WorkstationRuntimeConfig::profile_routes_to_gemini_researcher)
            .unwrap_or(false);
    let gemini_researcher_slot_id = runtime_config
        .gemini_researcher_pool_slot_id()
        .map(str::to_string);
    let prefer_gemini_researcher =
        routes_to_gemini_researcher && gemini_researcher_slot_id.is_some();

    // Phase 6.1: Find idle slot with RAII guard (atomic check+reserve).
    // intent-flow.lisp F-task-delegate-autoprovision :: s2 requires
    // slot.project_root == target_project_root for reuse.
    let guard = if prefer_gemini_researcher {
        find_and_reserve_gemini_researcher_slot(
            state,
            gemini_researcher_slot_id.as_deref().unwrap(),
        )
        .await
    } else {
        find_and_reserve_slot(
            state,
            template,
            target_project_root.as_deref(),
            requested_model.as_deref(),
        )
        .await
    };
    let assignee = guard
        .as_ref()
        .map(|g| g.slot_id().to_string())
        .unwrap_or_default();

    // 2. If no idle slot, try auto-provision dynamic slot
    let (assignee, provisioned) = if !assignee.is_empty() {
        (assignee, false)
    } else if prefer_gemini_researcher {
        // V3: never auto-provision a dynamic Claude slot for research while a
        // gemini researcher slot is registered. Queue unassigned so the
        // autopilot can route the BoardTask to the gemini slot once idle.
        (String::new(), false)
    } else if template == "ops" {
        // Phase 6.2: Guard uses template, not intent — prevents intent escape
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
    };
    // guard is dropped here (if Some) → auto-releases slot dispatch lock

    // 3. Build context from hints (Phase 6.3: with size limits)
    let mut description = objective.to_string();
    let metadata_block = render_delegation_metadata_block(&delegation_metadata);
    if !metadata_block.is_empty() {
        description = format!(
            "{}\n\n## Dispatch metadata\n{}",
            description, metadata_block
        );
    }
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

    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let task_id = task.id.to_string();

    // 5. Emit the canonical event-bus cause. The Autopilot subscriber
    // converts BoardEvent::TaskCreated into board_dispatch_notify, so
    // delegated work is driven by the same nervous-system path as explicit
    // mission_board_create calls.
    let ev = BoardEvent::TaskCreated {
        task_id: task_id.clone(),
        title: input.title.clone(),
        category: input.category.clone().unwrap_or_else(|| "dev".to_string()),
    };
    crate::engine::master_control::notify_board_event_direct(&ev);
    let _ = state.bus.publish_board(ev).await;

    // Legacy local fast-path while older producers finish moving to bus
    // causality. Duplicate Notify wakeups are coalesced and harmless.
    state.board_dispatch_notify.notify_one();

    Ok(ToolResult::json_pretty(&json!({
        "task_id": task_id,
        "assignee": if assignee.is_empty() { Value::Null } else { Value::String(assignee) },
        "status": "queued",
        "intent": intent,
        "template": template,
        "model": requested_model,
        "model_profile": effective_model_profile,
        "task_class": delegation_metadata.task_class,
        "pool_hint": delegation_metadata.pool_hint,
        "engine_hint": delegation_metadata.engine_hint,
        "context_pack_path": delegation_metadata.context_pack_path,
        "read_scope": delegation_metadata.read_scope,
        "write_scope": delegation_metadata.write_scope,
        "must_not_touch": delegation_metadata.must_not_touch,
        "acceptance": delegation_metadata.acceptance,
        "provisioned_new_slot": provisioned,
        "timeout_secs": timeout_secs,
        "routes_to_gemini_researcher": routes_to_gemini_researcher,
        "gemini_researcher_slot": gemini_researcher_slot_id,
        "hint": "结果将通过 TaskNotification 自动回报。也可用 mission_board_get 查询进度。"
    })))
}

async fn handle_swarm_run(state: &AppState, args: Value) -> Result<ToolResult> {
    let objective = match string_arg(&args, &["objective"]) {
        Some(value) => value.to_string(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "'objective' is required and must be non-empty",
            )))
        }
    };
    let project_id = string_arg(&args, &["project_id", "projectId"])
        .unwrap_or("missiond")
        .to_string();
    let context_pack_path = string_arg(&args, &["context_pack_path", "contextPackPath"])
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!(
                ".missiond/v3/runtime/swarm/{}-context-pack.lisp",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            )
        });
    let max_claude_workers =
        clamp_usize_arg(&args, &["max_claude_workers", "maxClaudeWorkers"], 4, 0, 4);
    let max_gemini_workers =
        clamp_usize_arg(&args, &["max_gemini_workers", "maxGeminiWorkers"], 2, 0, 2);
    let write_policy = string_arg(&args, &["write_policy", "writePolicy"])
        .unwrap_or("read-only")
        .to_string();
    let dry_run = bool_arg(&args, &["dry_run", "dryRun"]).unwrap_or(true);
    let acceptance = string_list_arg(
        &args,
        &["acceptance", "acceptance_commands", "acceptanceCommands"],
    );
    let read_scope = string_list_arg(&args, &["read_scope", "readScope"]);
    let timeout_secs = args
        .get("timeout_secs")
        .or_else(|| args.get("timeoutSecs"))
        .and_then(|v| v.as_i64())
        .unwrap_or(1800)
        .clamp(60, 7200);

    let mut planned = Vec::new();
    for idx in 0..max_gemini_workers {
        planned.push(SwarmPlannedTask {
            lane: "investigate".to_string(),
            engine_hint: "gemini".to_string(),
            pool_hint: "gemini-ultra-pro".to_string(),
            task_class: "context-pack".to_string(),
            title: format!(
                "Investigate context for swarm objective ({}/{})",
                idx + 1,
                max_gemini_workers
            ),
            intent: "research".to_string(),
            read_scope: read_scope.clone(),
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
        });
    }
    for idx in 0..max_claude_workers {
        planned.push(SwarmPlannedTask {
            lane: "investigate".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "context-pack".to_string(),
            title: format!(
                "Survey exact shards for swarm objective ({}/{})",
                idx + 1,
                max_claude_workers
            ),
            // Keep Claude context-pack workers on the Claude/coder route.
            // `intent=research` is intentionally reserved for the Gemini
            // researcher lane, so using it here would ignore max_gemini_workers=0.
            intent: "code".to_string(),
            read_scope: read_scope.clone(),
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
        });
    }

    if write_policy != "read-only" {
        planned.push(SwarmPlannedTask {
            lane: "implement".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "code".to_string(),
            title: "Implement accepted swarm shard after context-pack integration".to_string(),
            intent: "code".to_string(),
            read_scope: read_scope.clone(),
            write_scope: string_list_arg(&args, &["write_scope", "writeScope"]),
            must_not_touch: string_list_arg(&args, &["must_not_touch", "mustNotTouch"]),
        });
    }

    let mut created_task_ids = Vec::new();
    if !dry_run {
        for planned_task in &planned {
            let description = render_swarm_task_description(
                &objective,
                &project_id,
                &context_pack_path,
                &write_policy,
                &acceptance,
                planned_task,
            );
            let input = CreateBoardTaskInput {
                title: planned_task.title.clone(),
                description: Some(description),
                priority: Some("medium".to_string()),
                category: Some("dev".to_string()),
                project: Some(project_id.clone()),
                auto_execute: Some(true),
                timeout_secs: Some(timeout_secs),
                context_intent: Some(planned_task.intent.clone()),
                ..Default::default()
            };
            let task = state
                .store
                .create_board_task(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let task_id = task.id.to_string();
            let ev = BoardEvent::TaskCreated {
                task_id: task_id.clone(),
                title: input.title.clone(),
                category: input.category.clone().unwrap_or_else(|| "dev".to_string()),
            };
            crate::engine::master_control::notify_board_event_direct(&ev);
            let _ = state.bus.publish_board(ev).await;
            created_task_ids.push(task_id);
        }
        state.board_dispatch_notify.notify_one();
    }

    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.swarm-run.v1",
        "ok": true,
        "dry_run": dry_run,
        "objective": objective,
        "project_id": project_id,
        "context_pack_path": context_pack_path,
        "write_policy": write_policy,
        "fanout": {
            "max_claude_workers": max_claude_workers,
            "max_gemini_workers": max_gemini_workers
        },
        "planned_tasks": planned.iter().map(SwarmPlannedTask::to_json).collect::<Vec<_>>(),
        "created_task_ids": created_task_ids,
        "conflicts": [],
        "next_action": if dry_run {
            "rerun mission_swarm_run with dry_run=false after reviewing planned_tasks"
        } else {
            "watch BoardTask lifecycle and provider durable logs before closing the swarm objective"
        }
    })))
}

#[derive(Debug, Clone)]
struct SwarmPlannedTask {
    lane: String,
    engine_hint: String,
    pool_hint: String,
    task_class: String,
    title: String,
    intent: String,
    read_scope: Vec<String>,
    write_scope: Vec<String>,
    must_not_touch: Vec<String>,
}

impl SwarmPlannedTask {
    fn to_json(&self) -> Value {
        json!({
            "lane": self.lane,
            "engine_hint": self.engine_hint,
            "pool_hint": self.pool_hint,
            "task_class": self.task_class,
            "title": self.title,
            "intent": self.intent,
            "read_scope": self.read_scope,
            "write_scope": self.write_scope,
            "must_not_touch": self.must_not_touch,
        })
    }
}

fn render_swarm_task_description(
    objective: &str,
    project_id: &str,
    context_pack_path: &str,
    write_policy: &str,
    acceptance: &[String],
    planned: &SwarmPlannedTask,
) -> String {
    let completion_protocol = if write_policy == "read-only" {
        "Completion protocol: do not edit files, do not stage, do not commit. read_scope lists readable evidence; must_not_touch is a write/stage/commit prohibition, not a read ban by itself. Return a structured artifact with Findings / Evidence / Recommendations / Verification in the final summary or BoardTask note; do not paste raw KB JSON/log blobs. The master or integrator compiles the context-pack."
    } else {
        "Completion protocol: implementation lanes may read declared read_scope, may write only declared write_scope, must not write/stage/commit forbidden paths, and must report acceptance evidence as a structured artifact."
    };

    format!(
        "{objective}\n\n## Swarm metadata\n- project_id: {project_id}\n- lane: {}\n- task_class: {}\n- pool_hint: {}\n- engine_hint: {}\n- context_pack_path: {context_pack_path}\n- write_policy: {write_policy}\n- read_scope: {}\n- write_scope: {}\n- must_not_touch: {}\n- acceptance: {}\n\n{}",
        planned.lane,
        planned.task_class,
        planned.pool_hint,
        planned.engine_hint,
        if planned.read_scope.is_empty() {
            "[]".to_string()
        } else {
            planned.read_scope.join(", ")
        },
        if planned.write_scope.is_empty() {
            "[]".to_string()
        } else {
            planned.write_scope.join(", ")
        },
        if planned.must_not_touch.is_empty() {
            "[]".to_string()
        } else {
            planned.must_not_touch.join(", ")
        },
        if acceptance.is_empty() {
            "[]".to_string()
        } else {
            acceptance.join(" && ")
        },
        completion_protocol,
    )
}

#[derive(Debug, Clone, Default)]
struct DelegationMetadata {
    task_class: Option<String>,
    pool_hint: Option<String>,
    engine_hint: Option<String>,
    context_pack_path: Option<String>,
    /// Paths the worker is explicitly allowed (and expected) to READ. Distinct
    /// from `write_scope` / `must_not_touch`: review-class tasks ship with a
    /// non-empty `read_scope` and an empty `write_scope`, making the
    /// read-only-but-must-investigate contract explicit in the worker prompt.
    read_scope: Vec<String>,
    write_scope: Vec<String>,
    must_not_touch: Vec<String>,
    acceptance: Vec<String>,
}

/// V3 workstation-pool :: gemini researcher acquire.
///
/// Atomically reserves the workstation-pool gemini researcher slot via the
/// dispatch guard if it exists and is idle. Returns the RAII guard on success;
/// None when the slot is not registered, busy, or its PTY is missing/non-idle.
/// Project-root match is intentionally not enforced — the gemini researcher
/// slot is read-only and serves any project (matches autopilot's
/// `select_workstation_pool_slot` behaviour).
async fn find_and_reserve_gemini_researcher_slot<'a>(
    state: &'a AppState,
    slot_id: &str,
) -> Option<SlotAcquireGuard<'a>> {
    if state.mission.get_slot(slot_id).is_none() {
        return None;
    }
    let guard = state.slot_dispatch.try_acquire_guard(slot_id)?;
    match state.pty.get_status(guard.slot_id()).await {
        Some(info) if info.state == SessionState::Idle => Some(guard),
        _ => None, // guard dropped here → auto-releases
    }
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

fn bool_arg(args: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        let value = args.get(*key)?;
        if let Some(value) = value.as_bool() {
            return Some(value);
        }
        value
            .as_str()
            .and_then(|text| match text.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            })
    })
}

fn clamp_usize_arg(args: &Value, keys: &[&str], default: usize, min: usize, max: usize) -> usize {
    keys.iter()
        .find_map(|key| args.get(*key))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(default)
        .clamp(min, max)
}

fn string_list_arg(args: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| args.get(*key))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::trim))
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// V3 resident-master-control :: master-delegation projection.
///
/// `mission_task_delegate` is the common BoardTask entry used by Codex master,
/// context-pack-run-wave, and direct MCP callers. Structured metadata is kept
/// visible in the durable BoardTask description so Autopilot/worker prompts can
/// carry context-pack path, write scope, must-not-touch, acceptance, model, and
/// timeout without relying on out-of-band PTY text.
fn render_delegation_metadata_block(metadata: &DelegationMetadata) -> String {
    let mut lines = Vec::new();
    if let Some(value) = &metadata.task_class {
        lines.push(format!("- task_class: {}", value));
    }
    if let Some(value) = &metadata.pool_hint {
        lines.push(format!("- pool_hint: {}", value));
    }
    if let Some(value) = &metadata.engine_hint {
        lines.push(format!("- engine_hint: {}", value));
    }
    if let Some(value) = &metadata.context_pack_path {
        lines.push(format!("- context_pack_path: {}", value));
    }
    if !metadata.read_scope.is_empty() {
        lines.push(format!("- read_scope: {}", metadata.read_scope.join(", ")));
    }
    if !metadata.write_scope.is_empty() {
        lines.push(format!(
            "- write_scope: {}",
            metadata.write_scope.join(", ")
        ));
    }
    if !metadata.must_not_touch.is_empty() {
        lines.push(format!(
            "- must_not_touch: {}",
            metadata.must_not_touch.join(", ")
        ));
    }
    if !metadata.acceptance.is_empty() {
        lines.push(format!("- acceptance: {}", metadata.acceptance.join(" | ")));
    }
    if !metadata.read_scope.is_empty()
        || !metadata.write_scope.is_empty()
        || !metadata.must_not_touch.is_empty()
    {
        lines.push(
            "- scope_semantics: read_scope is allowed/expected reading; write_scope is the only allowed write set; must_not_touch forbids write/stage/commit and is not a read ban by itself"
                .to_string(),
        );
    }
    if matches!(
        metadata.task_class.as_deref(),
        Some("review") | Some("context-pack") | Some("research")
    ) {
        lines.push(
            "- output_contract: return a structured artifact with Findings / Evidence / Recommendations / Verification; do not paste raw KB JSON or full logs as the final answer"
                .to_string(),
        );
    }
    lines.join("\n")
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

    #[test]
    fn delegation_metadata_block_projects_two_stage_worker_contract() {
        let metadata = DelegationMetadata {
            task_class: Some("context-pack".to_string()),
            pool_hint: Some("claude-code-default".to_string()),
            engine_hint: Some("claude-code".to_string()),
            context_pack_path: Some(".missiond/tasks/wave99/context-pack.lisp".to_string()),
            read_scope: vec!["crates/missiond-core/src/types/board.rs".to_string()],
            write_scope: vec!["crates/a.rs".to_string()],
            must_not_touch: vec!["packages/**".to_string()],
            acceptance: vec!["cargo test -p missiond-daemon autopilot".to_string()],
        };
        let block = render_delegation_metadata_block(&metadata);
        for expected in [
            "- task_class: context-pack",
            "- pool_hint: claude-code-default",
            "- engine_hint: claude-code",
            "- context_pack_path: .missiond/tasks/wave99/context-pack.lisp",
            "- read_scope: crates/missiond-core/src/types/board.rs",
            "- write_scope: crates/a.rs",
            "- must_not_touch: packages/**",
            "- acceptance: cargo test -p missiond-daemon autopilot",
            "- scope_semantics: read_scope is allowed/expected reading; write_scope is the only allowed write set; must_not_touch forbids write/stage/commit and is not a read ban by itself",
            "- output_contract: return a structured artifact with Findings / Evidence / Recommendations / Verification; do not paste raw KB JSON or full logs as the final answer",
        ] {
            assert!(block.contains(expected), "missing {expected}: {block}");
        }
    }

    /// Pins read-only review-class semantics: a task with a non-empty
    /// `read_scope` and an empty `write_scope` renders both fields in the
    /// metadata block. The dispatch contract is "you may READ these,
    /// you may NOT write anywhere" — distinct from "no scope declared".
    #[test]
    fn delegation_metadata_block_pins_read_only_review_contract() {
        let metadata = DelegationMetadata {
            task_class: Some("review".to_string()),
            engine_hint: Some("claude-code".to_string()),
            pool_hint: Some("claude-code-default".to_string()),
            read_scope: vec![
                "/Users/jinchen/Projects/xiaojinpro-backend".to_string(),
                "/Users/jinchen/Projects/missiond".to_string(),
            ],
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
            acceptance: vec!["git status proves no new edits".to_string()],
            ..DelegationMetadata::default()
        };
        let block = render_delegation_metadata_block(&metadata);
        assert!(
            block.contains("- read_scope: /Users/jinchen/Projects/xiaojinpro-backend, /Users/jinchen/Projects/missiond"),
            "read_scope must list both repos: {block}"
        );
        assert!(
            !block.contains("\n- write_scope:"),
            "empty write_scope must not render: {block}"
        );
        assert!(block.contains("- must_not_touch: **/*"));
        assert!(block.contains("must_not_touch forbids write/stage/commit"));
        assert!(block.contains("Findings / Evidence / Recommendations / Verification"));
    }

    #[test]
    fn string_list_arg_accepts_snake_and_camel_metadata_keys() {
        let args = json!({
            "readScope": ["docs", "src"],
            "writeScope": ["a.rs", " ", "b.rs"],
            "acceptance_commands": ["cargo test"]
        });
        assert_eq!(
            string_list_arg(&args, &["read_scope", "readScope"]),
            vec!["docs", "src"]
        );
        assert_eq!(
            string_list_arg(&args, &["write_scope", "writeScope"]),
            vec!["a.rs", "b.rs"]
        );
        assert_eq!(
            string_list_arg(&args, &["acceptance", "acceptance_commands"]),
            vec!["cargo test"]
        );
    }

    // ── V3 workstation-pool :: gemini researcher binding ─────────────────
    //
    // Pins the routing decision research-default → gemini-ultra. Pure-helper
    // tests run against `WorkstationRuntimeConfig::default()` which mirrors the
    // blueprint defaults for `researcher` slot-template and the gemini-ultra
    // workstation-pool worker, so the audit's `model_profile=research-default`
    // case stops failing and research intent without an explicit Claude pin
    // routes to slot-gemini-ultra instead of a dynamic Claude coder slot.

    #[test]
    fn research_default_profile_resolves_to_no_spawn_model() {
        let cfg = WorkstationRuntimeConfig::default();
        // Was the live failure mode: research-default was not registered.
        assert_eq!(
            cfg.spawn_model_for_profile("research-default").unwrap(),
            None
        );
        // Aliases route through the same canonical profile.
        assert_eq!(cfg.spawn_model_for_profile("research").unwrap(), None);
        assert_eq!(cfg.spawn_model_for_profile("gemini-default").unwrap(), None);
    }

    #[test]
    fn researcher_template_default_profile_is_research_default() {
        let cfg = WorkstationRuntimeConfig::default();
        assert_eq!(
            cfg.default_model_profile_for_template("researcher"),
            Some("research-default")
        );
        // Coder template stays on Claude coding-default.
        assert_eq!(
            cfg.default_model_profile_for_template("coder"),
            Some("coding-default-opus-4-7")
        );
    }

    #[test]
    fn research_intent_default_routes_to_gemini_pool() {
        let cfg = WorkstationRuntimeConfig::default();
        // intent=research with no explicit model/profile: effective profile is
        // the researcher template default (research-default) which the
        // routing helper recognizes as a Gemini pin.
        let template_default = cfg
            .default_model_profile_for_template("researcher")
            .unwrap();
        assert!(WorkstationRuntimeConfig::profile_routes_to_gemini_researcher(template_default));
        // Explicit Claude profile stays on Claude.
        assert!(
            !WorkstationRuntimeConfig::profile_routes_to_gemini_researcher(
                "coding-default-opus-4-7"
            )
        );
        // Daily Sonnet / Haiku stay on Claude.
        assert!(!WorkstationRuntimeConfig::profile_routes_to_gemini_researcher("daily-sonnet"));
        assert!(!WorkstationRuntimeConfig::profile_routes_to_gemini_researcher("quick-haiku"));
    }

    #[test]
    fn gemini_researcher_pool_slot_is_registered() {
        let cfg = WorkstationRuntimeConfig::default();
        assert_eq!(
            cfg.gemini_researcher_pool_slot_id(),
            Some("slot-gemini-ultra")
        );
    }

    #[test]
    fn explicit_coding_default_profile_for_research_intent_keeps_claude_path() {
        let cfg = WorkstationRuntimeConfig::default();
        // Caller explicitly pins coding-default-opus-4-7 on a research delegation.
        // resolve_model_projection returns None (no --model arg) so the slot
        // selector sees a Claude-coder shaped request, not a gemini ping.
        let resolved = super::super::compute_slot::resolve_model_projection(
            "researcher",
            None,
            Some("coding-default-opus-4-7"),
            &cfg,
        )
        .unwrap();
        assert_eq!(resolved, None);
        assert!(
            !WorkstationRuntimeConfig::profile_routes_to_gemini_researcher(
                "coding-default-opus-4-7"
            )
        );
    }
}
