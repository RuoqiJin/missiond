use anyhow::{anyhow, Result};
use missiond_core::event::events::SlotEvent;
use missiond_core::pty::{PTYSpawnOptions, Slot as PTYSlot};
use missiond_core::types::{AsyncJob, DynamicSlot, Lifecycle, SlotConfig};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;
use crate::engine::control_plane_kernel::{
    AuditCapabilityBypassCommand, ControlPlaneKernel, RequireCapabilityCommand, StartAttemptCommand,
};
use crate::state::AppState;

const CODING_DEFAULT_PROFILE: &str = "coding-default-opus-4-7";

#[derive(Debug, Clone)]
struct TaskBoundAttemptStart {
    task_id: String,
    project_id: Option<String>,
    attempt_id: String,
    subject_kind: String,
    subject_id: String,
    capability_grant_id: String,
    sandbox_profile: String,
}

pub(crate) fn resolve_model_projection(
    template_name: &str,
    model: Option<&str>,
    model_profile: Option<&str>,
    runtime_config: &WorkstationRuntimeConfig,
) -> std::result::Result<Option<String>, String> {
    if let Some(value) = non_empty(model) {
        return normalize_model_override(value);
    }
    let profile = non_empty(model_profile)
        .or_else(|| runtime_config.default_model_profile_for_template(template_name));
    match profile {
        Some(profile) => runtime_config
            .spawn_model_for_profile(profile)
            .map_err(|err| err.to_string()),
        None => Ok(None),
    }
}

/// V3 workstation-config :: execution-ownership delegated-boardtask projection.
///
/// Returns the effective `PTYSpawnOptions.initial_prompt` for a freshly
/// provisioned slot. `objective` is slot metadata only; it must not implicitly
/// become the first executable message. Callers that want a warm-up prompt must
/// pass `initial_prompt` explicitly, and delegated BoardTask auto-provision
/// still forces `suppress=true` so Autopilot remains the sole task-prompt owner.
pub(crate) fn effective_initial_prompt(
    initial_prompt: Option<String>,
    suppress: bool,
) -> Option<String> {
    if suppress {
        None
    } else {
        initial_prompt
    }
}

pub(crate) fn model_projection_matches(
    slot_model: Option<&str>,
    requested_model: Option<&str>,
) -> bool {
    let slot = match slot_model {
        Some(value) => match normalize_model_override(value) {
            Ok(value) => value,
            Err(_) => return false,
        },
        None => None,
    };
    slot.as_deref() == requested_model
}

fn normalize_model_override(value: &str) -> std::result::Result<Option<String>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let normalized = normalize_profile(value);
    if matches!(
        normalized.as_str(),
        "default" | "claude-code-default" | "coding-default" | CODING_DEFAULT_PROFILE
    ) {
        return Ok(None);
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err("model must be a single safe CLI token".to_string());
    }
    Ok(Some(value.to_string()))
}

fn normalize_profile(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
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

fn available_templates_suggestion(config: &WorkstationRuntimeConfig) -> String {
    format!(
        "Available templates: {}",
        config.available_slot_template_names().join(", ")
    )
}

fn allowed_cwd_prefixes_suggestion(config: &WorkstationRuntimeConfig) -> String {
    let prefixes: Vec<String> = config
        .resolved_allowed_cwd_prefixes()
        .iter()
        .map(|prefix| prefix.display().to_string())
        .collect();
    format!(
        "Allowed prefixes: {:?}; registered active project roots are also allowed",
        prefixes
    )
}

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");

    match action {
        "create" => create_slot(state, &args).await,
        "terminate" => terminate_slot(state, &args).await,
        "extend" => extend_slot(state, &args).await,
        "list" => list_slots(state, &args).await,
        _ => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("Unknown action: {}", action),
            )
            .with_suggestion("Use: create, terminate, extend, list"),
        )),
    }
}

async fn create_slot(state: &AppState, args: &Value) -> Result<ToolResult> {
    let workstation_config = match WorkstationRuntimeConfig::load_for_current_dir() {
        Ok(config) => config,
        Err(err) => {
            let tool_error = ToolError::new("V3_BLUEPRINT_CONFIG_ERROR", err.to_string())
                .with_suggestion(
                    "ensure the MissionD root .missiond/v3/missiond-blueprint.lisp contains workstation-config slot-template and cwd-policy",
                );
            return Ok(ToolResult::structured_error(tool_error));
        }
    };
    let template_name = match args.get("template").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "'template' is required")
                    .with_suggestion(available_templates_suggestion(&workstation_config)),
            ));
        }
    };

    let template = match workstation_config.slot_template(template_name) {
        Some(t) => t,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("Unknown template '{}'", template_name),
                )
                .with_suggestion(available_templates_suggestion(&workstation_config)),
            ));
        }
    };
    let objective = args.get("objective").and_then(|v| v.as_str());
    let requested_slot_id = string_arg(args, &["slot_id", "slotId"]).map(str::to_string);
    if let Some(slot_id) = requested_slot_id.as_deref() {
        let valid = slot_id.starts_with("slot-dyn-")
            && slot_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !valid {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "mission_compute_slot(create) slot_id must be a safe slot-dyn-* identifier",
                )
                .with_details(json!({
                    "slot_id": slot_id,
                    "required_prefix": "slot-dyn-"
                })),
            ));
        }
    }
    let slot_id = requested_slot_id.unwrap_or_else(|| {
        let short_id = &uuid::Uuid::new_v4().to_string()[..8];
        format!("slot-dyn-{}", short_id)
    });
    let task_id = string_arg(args, &["task_id", "taskId"]).map(str::to_string);
    let mut task_contract_sandbox_profile: Option<String> = None;
    let mut task_bound_attempt_start: Option<TaskBoundAttemptStart> = None;
    if let Some(task_id) = task_id.as_deref() {
        let grant_id = string_arg(
            args,
            &[
                "grant_id",
                "grantId",
                "capability_grant_id",
                "capabilityGrantId",
            ],
        )
        .map(str::to_string)
        .or_else(|| {
            args.get("capability_grant_ids")
                .or_else(|| args.get("capabilityGrantIds"))
                .and_then(Value::as_array)
                .and_then(|values| values.iter().rev().find_map(Value::as_str))
                .map(str::to_string)
        });
        let subject_kind = string_arg(args, &["subject_kind", "subjectKind"]).unwrap_or("worker");
        let subject_id = string_arg(args, &["subject_id", "subjectId"]).unwrap_or(&slot_id);
        let spawn_grant_id = match ControlPlaneKernel::new(state)
            .require_capability_command(RequireCapabilityCommand {
                grant_id,
                subject_kind: subject_kind.to_string(),
                subject_id: subject_id.to_string(),
                operation: "spawn".to_string(),
                scope_kind: "task".to_string(),
                scope_key: task_id.to_string(),
                task_id: Some(task_id.to_string()),
                allow_system_bypass: false,
                bypass_reason: None,
                details: json!({"source": "mission_compute_slot.create"}),
            })
            .await
        {
            Ok(grant_id) => grant_id,
            Err(err) => {
                return Ok(ToolResult::structured_error(control_plane_tool_error(
                    err,
                    "spawn dynamic workers through mission_task_delegate so the worker slot carries an active spawn capability and sandbox metadata",
                )));
            }
        };
        let contract = match ControlPlaneKernel::new(state)
            .task_runtime_contract(task_id)
            .await
        {
            Ok(contract) => contract,
            Err(err) => {
                return Ok(ToolResult::structured_error(control_plane_tool_error(
                    err,
                    "backfill or create a canonical task_contracts row before spawning a worker",
                )));
            }
        };
        let derived_sandbox = contract
            .sandbox_profile
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                if contract.write_scope.is_empty() {
                    "read-only"
                } else {
                    "workspace-write"
                }
            });
        if matches!(
            derived_sandbox,
            "unsupported-write" | "danger-full-access" | "none" | "system-no-sandbox"
        ) {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::SANDBOX_POLICY_UNSUPPORTED,
                    format!(
                        "mission_compute_slot(create) refused task {task_id}: sandbox profile `{derived_sandbox}` is not valid for an ordinary worker spawn",
                    ),
                )
                .with_details(json!({
                    "task_id": task_id,
                    "sandbox_profile": derived_sandbox,
                    "source": "task_contracts",
                    "ordinary_worker": true
                }))
                .with_suggestion(
                    "use mission_task_delegate to create a task with an enforceable worker sandbox, or use confirm=true only for operator diagnostics",
                ),
            ));
        }
        if let Some(explicit_sandbox) =
            string_arg(args, &["sandbox", "sandbox_profile", "sandboxProfile"])
        {
            if explicit_sandbox != derived_sandbox {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::CAPABILITY_DENIED,
                        "mission_compute_slot(create) sandbox override does not match canonical task_contracts sandbox_profile",
                    )
                    .with_details(json!({
                        "task_id": task_id,
                        "requested_sandbox": explicit_sandbox,
                        "contract_sandbox_profile": derived_sandbox,
                        "required": "sandbox_profile from task_contracts"
                    }))
                    .with_suggestion(
                        "do not pass a sandbox override for task-bound workers; let MissionD derive it from task_contracts",
                    ),
                ));
            }
        }
        task_contract_sandbox_profile = Some(derived_sandbox.to_string());
        task_bound_attempt_start = Some(TaskBoundAttemptStart {
            task_id: task_id.to_string(),
            project_id: contract.project_id.clone(),
            attempt_id: string_arg(args, &["attempt_id", "attemptId"])
                .map(str::to_string)
                .unwrap_or_else(|| format!("attempt:{task_id}:slot:{slot_id}")),
            subject_kind: subject_kind.to_string(),
            subject_id: subject_id.to_string(),
            capability_grant_id: spawn_grant_id,
            sandbox_profile: derived_sandbox.to_string(),
        });
    } else if !bool_arg(
        args,
        &[
            "confirm",
            "operator_confirm",
            "operatorConfirm",
            "operator_confirmed",
            "operatorConfirmed",
        ],
    )
    .unwrap_or(false)
    {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::CAPABILITY_DENIED,
                "mission_compute_slot(create) requires task_id with an active worker-bound spawn capability, or an explicit operator confirm bypass",
            )
            .with_details(json!({
                "operation": "spawn",
                "scope_kind": "task",
                "required": "task_id"
            }))
            .with_suggestion(
                "use mission_task_delegate for ordinary workers; only pass confirm=true for operator-managed diagnostic slots",
            ),
        ));
    } else {
        ControlPlaneKernel::new(state)
            .audit_capability_bypass_command(AuditCapabilityBypassCommand {
                subject_kind: "operator".to_string(),
                subject_id: "mission_compute_slot".to_string(),
                operation: "spawn".to_string(),
                scope_kind: "task".to_string(),
                scope_key: "operator-confirmed-dynamic-slot".to_string(),
                reason:
                    "operator confirmed mission_compute_slot(create) without worker-bound spawn grant"
                        .to_string(),
                details: json!({
                    "template": template_name,
                    "objective": objective,
                }),
            })
            .await
            .map_err(|e| anyhow!("capability audit error: {}", e))?;
    }
    // Check slot limit
    let active_count = state
        .store
        .count_active_dynamic_slots()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let dynamic_limit = workstation_config.dynamic_slot_limit();
    if active_count >= dynamic_limit {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::LIMIT_REACHED,
                format!(
                    "Dynamic slot limit reached ({}/{})",
                    active_count, dynamic_limit
                ),
            )
            .with_suggestion("Terminate existing slots with action=terminate first"),
        ));
    }

    // Validate cwd
    let default_cwd = workstation_config.resolve_runtime_path_string(template.default_cwd.as_str());
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or(default_cwd.as_str());
    let canonical_cwd = match std::fs::canonicalize(cwd) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                format!("cwd '{}' does not exist or is not accessible", cwd),
            )));
        }
    };

    let canonical_path = std::path::Path::new(&canonical_cwd);
    let cwd_allowed_by_policy = workstation_config
        .resolved_allowed_cwd_prefixes()
        .iter()
        .any(|prefix| canonical_path.starts_with(prefix));
    let cwd_allowed_by_registry = {
        let registry = state.project_registry.read().await;
        registry.resolve(&canonical_cwd).is_some()
    };
    let cwd_allowed = cwd_allowed_by_policy || cwd_allowed_by_registry;
    if !cwd_allowed {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::PERMISSION_DENIED,
                format!("cwd '{}' is not under allowed prefixes", cwd),
            )
            .with_suggestion(allowed_cwd_prefixes_suggestion(&workstation_config)),
        ));
    }

    let explicit_initial_prompt =
        string_arg(args, &["initial_prompt", "initialPrompt"]).map(str::to_string);

    // V3 execution-ownership :: delegated-boardtask. When task_delegate
    // auto-provisions a dynamic slot for a queued BoardTask it sets
    // `suppress_initial_prompt: true` so the slot starts idle and Autopilot
    // remains the sole task-prompt owner. Direct `mission_compute_slot create`
    // callers that want warm-up behaviour must pass `initial_prompt`
    // explicitly; `objective` is metadata and is never executed implicitly.
    let suppress_initial_prompt = args
        .get("suppress_initial_prompt")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Resolve canonical_cwd to a registered project root (per
    // intent-worker.lisp :: invariant project-root-spawn-cwd). For
    // ClaudeCode this is currently the only engine wired through compute_slot,
    // but the resolver enforces project-bound semantics regardless: cwd must
    // resolve under a registered project; subdir is preserved as
    // requested_cwd metadata; process cwd is the canonical project root.
    let resolution = match crate::slot_orchestrator::project_root::resolve_target_project_root(
        None,
        Some(std::path::Path::new(&canonical_cwd)),
        None,
        &state.project_registry,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    "PROJECT_ROOT_UNRESOLVED",
                    format!(
                        "compute_slot create requires cwd under a registered project: {}",
                        e
                    ),
                )
                .with_suggestion(
                    "register the project via mission_project(action=\"init\") or pass cwd inside an existing project",
                ),
            ));
        }
    };
    let project_root_str = resolution.project_root.to_string_lossy().to_string();
    let requested_cwd = resolution
        .requested_cwd
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());

    let runtime_config = match WorkstationRuntimeConfig::load_for_project_root(Some(
        project_root_str.as_str(),
    )) {
        Ok(config) => config,
        Err(err) => {
            let tool_error = ToolError::new("V3_BLUEPRINT_CONFIG_ERROR", err.to_string())
                .with_suggestion(
                    "ensure <project>/.missiond/v3/missiond-blueprint.lisp contains workstation-config ttl-policy",
                );
            return Ok(ToolResult::structured_error(tool_error));
        }
    };
    let template = match runtime_config.slot_template(template_name) {
        Some(template) => template.clone(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("Unknown template '{}'", template_name),
                )
                .with_suggestion(available_templates_suggestion(&runtime_config)),
            ));
        }
    };
    let ttl = runtime_config.clamp_slot_ttl_secs(args.get("max_ttl").and_then(|v| v.as_i64()));
    let spawn_timeout_secs = runtime_config.dynamic_slot_spawn_timeout_secs();
    let model = match resolve_model_projection(
        template_name,
        string_arg(args, &["model"]),
        string_arg(args, &["model_profile", "modelProfile"]),
        &runtime_config,
    ) {
        Ok(model) => model,
        Err(message) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                message,
            )));
        }
    };

    // Build SlotConfig — `cwd` becomes the canonical project root so that
    // spawn_tracked_slot picks it up as process cwd. The original requested
    // path (subdir) is retained in `requested_cwd` for prompt/context only.
    let slot_config = SlotConfig {
        id: slot_id.clone(),
        role: template.role.clone(),
        description: format!(
            "[Dynamic] {} | Template: {} | Objective: {}",
            template.description,
            template_name,
            objective.unwrap_or("(none)")
        ),
        engine: Default::default(),
        cwd: Some(project_root_str.clone()),
        project_root: Some(project_root_str),
        requested_cwd,
        mcp_config: template.mcp_config.clone(),
        lifecycle: Some(Lifecycle::OnDemand),
        auto_start: None,
        dangerously_skip_permissions: Some(false), // ALWAYS false for dynamic slots
        model,
        model_profile: args
            .get("model_profile")
            .or_else(|| args.get("modelProfile"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        reasoning_effort: args
            .get("reasoning_effort")
            .or_else(|| args.get("reasoningEffort"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        search_enabled: args
            .get("search_enabled")
            .or_else(|| args.get("searchEnabled"))
            .and_then(|v| v.as_bool()),
        sandbox: task_contract_sandbox_profile.clone().or_else(|| {
            args.get("sandbox")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }),
        approval_policy: args
            .get("approval_policy")
            .or_else(|| args.get("approvalPolicy"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        tool_policy_path: args
            .get("tool_policy_path")
            .or_else(|| args.get("toolPolicyPath"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        traits: vec![],
        category: None,
        env: None,
        initial_prompt: None,
    };

    let config_json = serde_json::to_string(&slot_config)
        .map_err(|e| anyhow!("Config serialization error: {}", e))?;

    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::seconds(ttl);

    let dynamic_slot = DynamicSlot {
        id: slot_id.clone(),
        parent_slot_id: "slot-jarvis".to_string(),
        template: template_name.to_string(),
        objective: objective.map(|s| s.to_string()),
        config: config_json,
        status: "active".to_string(),
        termination_reason: None,
        created_at: now.to_rfc3339(),
        terminated_at: None,
        ttl_seconds: ttl,
        expires_at: expires_at.to_rfc3339(),
        extend_count: 0,
    };

    // Persist to DB
    state
        .store
        .create_dynamic_slot(&dynamic_slot)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    // Register in SlotManager (runtime merge)
    state.mission.register_dynamic_slot(slot_config.clone());

    // Create async job — PTY spawn can take up to 60s (wait_for_idle)
    let job_id = format!("job-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let job_id_bg = job_id.clone(); // one clone for background task
    let job = AsyncJob::new(job_id_bg.clone(), "mission_compute_slot:create".to_string());
    {
        let mut store = state.job_store.write().await;
        store.insert(job_id_bg.clone(), job);
    }

    // Spawn PTY in background tokio task
    let state_clone = state.clone();
    let slot_id_owned = slot_id.clone();
    let template_owned = template_name.to_string();
    let objective_owned = objective.map(|s| s.to_string());
    let task_bound_attempt_id = task_bound_attempt_start
        .as_ref()
        .map(|attempt| attempt.attempt_id.clone());
    let task_bound_attempt_start_for_spawn = task_bound_attempt_start.clone();
    let initial_prompt_for_spawn =
        effective_initial_prompt(explicit_initial_prompt, suppress_initial_prompt);
    let expires_at_str = expires_at.to_rfc3339();

    tokio::spawn(async move {
        let pty_slot = PTYSlot {
            id: slot_config.id.clone(),
            role: slot_config.role.clone(),
            cwd: slot_config.cwd.as_ref().map(PathBuf::from),
            engine: slot_config.engine,
        };

        // Init slot in PTYManager first (registers agent_info)
        state_clone.pty.init_slot(&pty_slot).await;

        // NOTE: perm_injector is now invoked from inside spawn_tracked_slot
        // (Phase 3 of the permission persistence architecture upgrade) so every
        // spawn path gets coverage. No need to call it here.

        let mcp_config = slot_config.mcp_config.as_ref().map(PathBuf::from);

        let result = crate::slot_orchestrator::spawner::spawn_tracked_slot(
            &state_clone.pty,
            &state_clone.store,
            &state_clone.pty_session_uuids,
            &state_clone.project_registry,
            state_clone.permission.learned(),
            &pty_slot,
            PTYSpawnOptions {
                auto_restart: false,
                wait_for_idle: true,
                timeout_secs: Some(spawn_timeout_secs),
                mcp_config,
                dangerously_skip_permissions: false,
                model: slot_config.model.clone(),
                reasoning_effort: slot_config.reasoning_effort.clone(),
                search_enabled: slot_config.search_enabled.unwrap_or(false),
                sandbox: slot_config.sandbox.clone(),
                approval_policy: slot_config.approval_policy.clone(),
                tool_policy_path: slot_config
                    .tool_policy_path
                    .clone()
                    .map(std::path::PathBuf::from),
                extra_env: HashMap::new(),
                initial_prompt: initial_prompt_for_spawn,
                command_override: None,
                ..Default::default()
            },
            slot_config.env.as_ref(),
        )
        .await;

        let mut attempt_started: Option<Value> = None;
        let mut attempt_start_failed: Option<String> = None;
        if result.is_ok() {
            if let Some(attempt) = task_bound_attempt_start_for_spawn.as_ref() {
                match ControlPlaneKernel::new(&state_clone)
                    .start_attempt_command(StartAttemptCommand {
                        task_id: attempt.task_id.clone(),
                        project_id: attempt.project_id.clone(),
                        attempt_id: attempt.attempt_id.clone(),
                        agent_id: attempt.subject_id.clone(),
                        worker_id: slot_id_owned.clone(),
                        payload: json!({
                            "source": "mission_compute_slot.create",
                            "slot_id": slot_id_owned.clone(),
                            "template": template_owned.clone(),
                            "subject_kind": attempt.subject_kind.clone(),
                            "subject_id": attempt.subject_id.clone(),
                            "capability_grant_id": attempt.capability_grant_id.clone(),
                            "sandbox_profile": attempt.sandbox_profile.clone(),
                            "model": slot_config.model.clone(),
                        }),
                    })
                    .await
                {
                    Ok(event) => {
                        attempt_started = Some(event);
                    }
                    Err(err) => {
                        attempt_start_failed = Some(control_plane_error_message(&err));
                    }
                }
            }
        }

        let mut became_idle = false;
        let mut spawn_failed = None;
        {
            let mut store = state_clone.job_store.write().await;
            if let Some(job) = store.get_mut(&job_id_bg) {
                match result {
                    Ok(_) if attempt_start_failed.is_none() => {
                        job.complete(json!({
                            "slot_id": slot_id_owned.clone(),
                            "status": "spawned",
                            "template": template_owned,
                            "model": slot_config.model.clone(),
                            "model_profile": if slot_config.model.is_none() { CODING_DEFAULT_PROFILE } else { "explicit-model" },
                            "ttl_seconds": ttl,
                            "expires_at": expires_at_str,
                            "objective": objective_owned,
                            "attempt_id": task_bound_attempt_start_for_spawn.as_ref().map(|attempt| attempt.attempt_id.clone()),
                            "attempt": attempt_started,
                        }));
                        became_idle = true;
                    }
                    Ok(_) => {
                        let message = format!(
                            "Failed to start task attempt: {}",
                            attempt_start_failed
                                .unwrap_or_else(|| "unknown attempt start error".to_string())
                        );
                        job.fail(message.clone());
                        spawn_failed = Some(message);
                    }
                    Err(e) => {
                        let message = format!("Failed to spawn slot: {}", e);
                        job.fail(message.clone());
                        spawn_failed = Some(message);
                    }
                }
            }
        }
        if spawn_failed.is_some() {
            let _ = state_clone.pty.kill(&slot_id_owned).await;
            state_clone
                .store
                .terminate_dynamic_slot(&slot_id_owned, "spawn_failed")
                .await
                .ok();
            state_clone.mission.unregister_dynamic_slot(&slot_id_owned);
        }
        if became_idle {
            let _ = state_clone
                .bus
                .publish_slot(SlotEvent::BecameIdle {
                    slot_id: slot_id_owned.clone(),
                })
                .await;
            state_clone.board_dispatch_notify.notify_one();
        }
    });

    Ok(ToolResult::job_accepted_with_metadata(
        &job_id,
        "mission_compute_slot:create",
        json!({
            "slot_id": slot_id,
            "template": template_name,
            "status_detail": "spawn_pending",
            "sandbox_profile": task_contract_sandbox_profile,
            "attempt_id": task_bound_attempt_id,
        }),
    ))
}

fn control_plane_error_message(err: &anyhow::Error) -> String {
    if let Some(control) =
        err.downcast_ref::<crate::engine::shared_memory::StructuredControlError>()
    {
        return format!("{}: {}", control.code, control.message);
    }
    err.to_string()
}

fn control_plane_tool_error(err: anyhow::Error, fallback_suggestion: &str) -> ToolError {
    if let Some(control) =
        err.downcast_ref::<crate::engine::shared_memory::StructuredControlError>()
    {
        let mut tool_error = ToolError::new(control.code, control.message.clone())
            .with_details(control.details.clone());
        if let Some(suggestion) = &control.suggestion {
            tool_error = tool_error.with_suggestion(suggestion.clone());
        } else {
            tool_error = tool_error.with_suggestion(fallback_suggestion);
        }
        return tool_error;
    }
    ToolError::new(error_codes::DB_ERROR, err.to_string()).with_suggestion(fallback_suggestion)
}

async fn terminate_slot(state: &AppState, args: &Value) -> Result<ToolResult> {
    let slot_id = match args.get("slot_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "'slot_id' is required",
            )));
        }
    };

    // Verify it's a dynamic slot
    if !slot_id.starts_with("slot-dyn-") {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                "Can only terminate dynamic slots",
            )
            .with_suggestion(
                "Dynamic slot IDs start with 'slot-dyn-'. Use action=list to see available slots",
            ),
        ));
    }

    // Kill PTY session if running
    let _ = state.pty.kill(slot_id).await;

    // Mark terminated in DB
    let terminated = state
        .store
        .terminate_dynamic_slot(slot_id, "user_terminated")
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if !terminated {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::NOT_FOUND,
            format!("Slot '{}' not found or already terminated", slot_id),
        )));
    }

    // Unregister from SlotManager
    state.mission.unregister_dynamic_slot(slot_id);

    Ok(ToolResult::json_pretty(&json!({
        "slot_id": slot_id,
        "status": "terminated",
        "reason": "user_terminated",
    })))
}

async fn extend_slot(state: &AppState, args: &Value) -> Result<ToolResult> {
    let slot_id = match args.get("slot_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "'slot_id' is required",
            )));
        }
    };

    let Some(dynamic_slot) = state
        .store
        .get_dynamic_slot(slot_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    else {
        return Ok(slot_extend_not_found(slot_id));
    };
    if dynamic_slot.status != "active" {
        return Ok(slot_extend_not_found(slot_id));
    }

    let runtime_config = match WorkstationRuntimeConfig::load_for_project_root(
        dynamic_slot_project_root(&dynamic_slot).as_deref(),
    ) {
        Ok(config) => config,
        Err(err) => {
            let tool_error = ToolError::new("V3_BLUEPRINT_CONFIG_ERROR", err.to_string())
                .with_suggestion(
                    "ensure <project>/.missiond/v3/missiond-blueprint.lisp contains workstation-config ttl-policy",
                );
            return Ok(ToolResult::structured_error(tool_error));
        }
    };

    let additional = args
        .get("additional_seconds")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| runtime_config.default_slot_extend_secs());
    let max_extend_secs = runtime_config.max_slot_extend_secs();

    if additional <= 0 || additional > max_extend_secs {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!(
                "Extension must be between 1 and {} seconds per request",
                max_extend_secs
            ),
        )));
    }

    match state
        .store
        .extend_dynamic_slot(slot_id, additional)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(new_expires) => Ok(ToolResult::json_pretty(&json!({
            "slot_id": slot_id,
            "new_expires_at": new_expires,
            "extended_by_seconds": additional,
        }))),
        None => Ok(slot_extend_not_found(slot_id)),
    }
}

fn slot_extend_not_found(slot_id: &str) -> ToolResult {
    ToolResult::structured_error(
        ToolError::new(
            error_codes::NOT_FOUND,
            format!(
                "Cannot extend '{}': slot not found, not active, or max extensions (2) reached",
                slot_id
            ),
        )
        .with_suggestion("Use action=list to check slot status"),
    )
}

fn dynamic_slot_project_root(slot: &DynamicSlot) -> Option<String> {
    serde_json::from_str::<SlotConfig>(&slot.config)
        .ok()
        .and_then(|config| config.project_root.or(config.cwd))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// V3 contract: a static slot is dispatchable iff its id appears in the V3
/// workstation-pool or startup-slots projection. When `v3_active` is false the
/// V3 blueprint failed to load, so we fall back to permissive behaviour and
/// treat every slot as dispatchable.
pub(crate) fn classify_static_slot(
    slot_id: &str,
    v3_slot_ids: &std::collections::HashSet<String>,
    v3_active: bool,
) -> StaticSlotClass {
    if !v3_active {
        return StaticSlotClass {
            legacy: false,
            dispatchable: true,
        };
    }
    let in_v3 = v3_slot_ids.contains(slot_id);
    StaticSlotClass {
        legacy: !in_v3,
        dispatchable: in_v3,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StaticSlotClass {
    pub(crate) legacy: bool,
    pub(crate) dispatchable: bool,
}

/// V3 contract: derive `status` from PTYManager so `mission_compute_slot list`
/// cannot contradict `mission_pty_status` for V3 pool slots. We only fall back
/// to the SlotManager session_id heuristic when no PTY status is available
/// (e.g. PTY never initialised yet), and even then never claim "running" for a
/// slot whose PTY is missing.
pub(crate) fn derive_static_status(pty_state: Option<&str>, has_session_id: bool) -> String {
    if let Some(state) = pty_state {
        return state.to_ascii_lowercase();
    }
    if has_session_id {
        "running".to_string()
    } else {
        "stopped".to_string()
    }
}

async fn list_slots(state: &AppState, args: &Value) -> Result<ToolResult> {
    let status_filter = args.get("status").and_then(|v| v.as_str());

    // Get dynamic slots from DB
    let dynamic_slots = state
        .store
        .list_dynamic_slots(status_filter)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    // V3 SSOT: dispatchable slot IDs come from workstation-pool + startup-slots.
    // Anything in slots.yaml that is not present here is legacy/non-dispatchable
    // and must be flagged accordingly so consumers cannot resurface it.
    let workstation_config_result = WorkstationRuntimeConfig::load_for_current_dir();
    let v3_slot_ids: std::collections::HashSet<String> = match &workstation_config_result {
        Ok(config) => config
            .startup_slots()
            .iter()
            .filter_map(|s| s.slot_id.clone())
            .chain(config.workstation_pool().iter().map(|w| w.slot_id.clone()))
            .collect(),
        Err(_) => std::collections::HashSet::new(),
    };

    // Get static slots
    let static_slots = state.mission.list_slots();

    let dynamic_entries: Vec<Value> = dynamic_slots
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "source": "dynamic",
                "template": s.template,
                "status": s.status,
                "objective": s.objective,
                "expires_at": s.expires_at,
                "extend_count": s.extend_count,
                "created_at": s.created_at,
                "termination_reason": s.termination_reason,
            })
        })
        .collect();

    // V3 contract: static-slot status MUST be derived from PTYManager so this
    // surface cannot contradict mission_pty_status for V3 pool slots. Legacy
    // non-V3 static slots are routed to a separate `legacy_static_slots` array
    // and tagged dispatchable=false so retired Sonnet workers cannot resurface.
    let mut static_entries: Vec<Value> = Vec::new();
    let mut legacy_static_entries: Vec<Value> = Vec::new();
    let v3_active = workstation_config_result.is_ok();
    for s in &static_slots {
        if s.config.id.starts_with("slot-dyn-") {
            continue;
        }
        let pty_status = state
            .pty
            .get_status(&s.config.id)
            .await
            .map(|info| format!("{:?}", info.state).to_ascii_lowercase());
        let status = derive_static_status(pty_status.as_deref(), s.session_id.is_some());
        let StaticSlotClass {
            legacy,
            dispatchable,
        } = classify_static_slot(&s.config.id, &v3_slot_ids, v3_active);
        let entry = json!({
            "id": s.config.id,
            "source": if legacy { "static-legacy" } else { "static" },
            "role": s.config.role,
            "engine": s.config.engine.to_string(),
            "model": s.config.model,
            "project_root": s.config.project_root,
            "description": s.config.description,
            "status": status,
            "pty_status": pty_status,
            "dispatchable": dispatchable,
            "legacy": legacy,
            "lifecycle": format!("{}", s.config.lifecycle.unwrap_or_default()),
        });
        if legacy {
            legacy_static_entries.push(entry);
        } else {
            static_entries.push(entry);
        }
    }

    let workstation_pool = match &workstation_config_result {
        Ok(config) => {
            let mut entries = Vec::new();
            for worker in config.workstation_pool() {
                let pty_status = state
                    .pty
                    .get_status(&worker.slot_id)
                    .await
                    .map(|info| format!("{:?}", info.state).to_ascii_lowercase());
                let runtime_slot_present = state.mission.get_slot(&worker.slot_id).is_some();
                entries.push(json!({
                    "id": worker.id,
                    "engine": worker.engine,
                    "role": worker.role,
                    "slot_id": worker.slot_id,
                    "task_type": worker.task_type,
                    "model_profile": worker.model_profile,
                    "model": worker.model,
                    "task_classes": worker.task_classes,
                    "capabilities": worker.capabilities,
                    "max_concurrency": worker.max_concurrency,
                    "timeout_secs": worker.timeout_secs,
                    "default_use": worker.default_use,
                    "accepts_boardtask": worker.accepts_boardtask,
                    "write_allowed": worker.write_allowed,
                    "runtime_slot_present": runtime_slot_present,
                    "pty_status": pty_status,
                    "status": pty_status.clone().unwrap_or_else(|| {
                        if runtime_slot_present { "stopped".to_string() } else { "missing".to_string() }
                    }),
                }));
            }
            json!(entries)
        }
        Err(err) => json!({
            "error": "V3_BLUEPRINT_CONFIG_ERROR",
            "message": err.to_string(),
        }),
    };

    Ok(ToolResult::json_pretty(&json!({
        "static_slots": static_entries,
        "legacy_static_slots": legacy_static_entries,
        "dynamic_slots": dynamic_entries,
        "workstation_pool": workstation_pool,
        "dynamic_active": dynamic_slots.iter().filter(|s| s.status == "active").count(),
        "dynamic_limit": workstation_config_result
            .as_ref()
            .map(WorkstationRuntimeConfig::dynamic_slot_limit)
            .unwrap_or(0),
        "v3_authoritative": v3_active,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coder_template_uses_claude_code_default_profile() {
        let cfg = WorkstationRuntimeConfig::default();
        assert_eq!(
            resolve_model_projection("coder", None, None, &cfg).unwrap(),
            None
        );
        assert_eq!(
            resolve_model_projection("researcher", None, None, &cfg).unwrap(),
            None
        );
    }

    #[test]
    fn ops_template_keeps_sonnet_default() {
        let cfg = WorkstationRuntimeConfig::default();
        assert_eq!(
            resolve_model_projection("ops", None, None, &cfg).unwrap(),
            Some("sonnet".to_string())
        );
    }

    #[test]
    fn caller_model_wins_over_profile() {
        let cfg = WorkstationRuntimeConfig::default();
        assert_eq!(
            resolve_model_projection("coder", Some("haiku"), Some("daily-sonnet"), &cfg).unwrap(),
            Some("haiku".to_string())
        );
    }

    #[test]
    fn default_alias_means_no_model_arg() {
        let cfg = WorkstationRuntimeConfig::default();
        assert_eq!(
            resolve_model_projection("coder", Some("claude-code-default"), None, &cfg).unwrap(),
            None
        );
        assert_eq!(
            resolve_model_projection("coder", None, Some("coding_default_opus_4_7"), &cfg).unwrap(),
            None
        );
    }

    #[test]
    fn unsafe_model_token_is_rejected() {
        let cfg = WorkstationRuntimeConfig::default();
        assert!(resolve_model_projection("coder", Some("sonnet;rm"), None, &cfg).is_err());
        assert!(resolve_model_projection("coder", Some("sonnet 4.6"), None, &cfg).is_err());
    }

    #[test]
    fn slot_model_matching_treats_default_alias_as_none() {
        assert!(model_projection_matches(None, None));
        assert!(model_projection_matches(Some("default"), None));
        assert!(!model_projection_matches(Some("sonnet"), None));
        assert!(model_projection_matches(Some("sonnet"), Some("sonnet")));
    }

    // ── V3 execution-ownership :: delegated-boardtask projection ─────────
    //
    // Tests for `effective_initial_prompt`: pure helper, no AppState. Pins
    // the rule that objective is metadata only, explicit initial_prompt is the
    // only warm-up message, and mission_task_delegate auto-provisioning
    // suppresses even that.

    #[test]
    fn effective_initial_prompt_returns_explicit_prompt() {
        let prompt = Some("warm the slot".to_string());
        assert_eq!(effective_initial_prompt(prompt.clone(), false), prompt);
    }

    #[test]
    fn effective_initial_prompt_suppresses_when_flag_set() {
        let prompt = Some("warm the slot".to_string());
        assert_eq!(effective_initial_prompt(prompt, true), None);
    }

    #[test]
    fn effective_initial_prompt_returns_none_when_prompt_absent() {
        assert_eq!(effective_initial_prompt(None, false), None);
        assert_eq!(effective_initial_prompt(None, true), None);
    }

    #[test]
    fn dynamic_slot_project_root_prefers_project_root() {
        let config = SlotConfig {
            id: "slot-dyn-test".to_string(),
            role: "coder".to_string(),
            description: "test".to_string(),
            engine: Default::default(),
            cwd: Some("/tmp/requested".to_string()),
            project_root: Some("/tmp/project".to_string()),
            requested_cwd: None,
            mcp_config: None,
            lifecycle: Some(Lifecycle::OnDemand),
            auto_start: None,
            dangerously_skip_permissions: None,
            model: None,
            model_profile: None,
            reasoning_effort: None,
            search_enabled: None,
            sandbox: None,
            approval_policy: None,
            tool_policy_path: None,
            traits: vec![],
            category: None,
            env: None,
            initial_prompt: None,
        };
        let slot = DynamicSlot {
            id: "slot-dyn-test".to_string(),
            parent_slot_id: "slot-jarvis".to_string(),
            template: "coder".to_string(),
            objective: None,
            config: serde_json::to_string(&config).unwrap(),
            status: "active".to_string(),
            termination_reason: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            terminated_at: None,
            ttl_seconds: 14400,
            expires_at: "2026-01-01T04:00:00Z".to_string(),
            extend_count: 0,
        };

        assert_eq!(
            dynamic_slot_project_root(&slot),
            Some("/tmp/project".to_string())
        );
    }

    // ── V3 workstation-pool truthful list_slots projection ──────────────
    //
    // Pin the rules that fix `mission_compute_slot list`:
    //   * dispatchable iff the slot id is in V3 workstation-pool/startup-slots,
    //     so retired Sonnet entries (autopilot/topology-guardian/etc.) are
    //     marked legacy/non-dispatchable rather than resurfacing as candidates;
    //   * status is derived from PTYManager so this surface cannot contradict
    //     `mission_pty_status` for V3 pool slots.

    fn v3_ids(ids: &[&str]) -> std::collections::HashSet<String> {
        ids.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn classify_static_slot_marks_legacy_outside_v3_pool() {
        let pool = v3_ids(&["slot-claude-code-default", "slot-gemini-ultra"]);
        let class = classify_static_slot("topology-guardian", &pool, true);
        assert!(class.legacy);
        assert!(!class.dispatchable);
    }

    #[test]
    fn classify_static_slot_keeps_v3_pool_member_dispatchable() {
        let pool = v3_ids(&["slot-claude-code-default", "slot-gemini-ultra"]);
        let class = classify_static_slot("slot-claude-code-default", &pool, true);
        assert!(!class.legacy);
        assert!(class.dispatchable);
    }

    #[test]
    fn classify_static_slot_falls_back_when_v3_absent() {
        let pool = v3_ids(&[]);
        let class = classify_static_slot("legacy-anything", &pool, false);
        assert!(!class.legacy);
        assert!(class.dispatchable);
    }

    #[test]
    fn derive_static_status_prefers_pty_state_lowercased() {
        assert_eq!(derive_static_status(Some("Idle"), false), "idle");
        assert_eq!(derive_static_status(Some("Thinking"), true), "thinking");
        assert_eq!(derive_static_status(Some("Exited"), true), "exited");
    }

    #[test]
    fn derive_static_status_no_pty_falls_back_to_session_flag() {
        assert_eq!(derive_static_status(None, false), "stopped");
        assert_eq!(derive_static_status(None, true), "running");
    }

    #[test]
    fn list_slots_does_not_echo_dynamic_slots_as_legacy_static() {
        let src = include_str!("./compute_slot.rs");
        assert!(
            src.contains("s.config.id.starts_with(\"slot-dyn-\")"),
            "dynamic runtime slots are already represented by dynamic_slots and must not be duplicated under legacy_static_slots"
        );
    }

    #[test]
    fn dynamic_slot_project_root_falls_back_to_cwd() {
        let config = SlotConfig {
            id: "slot-dyn-test".to_string(),
            role: "coder".to_string(),
            description: "test".to_string(),
            engine: Default::default(),
            cwd: Some("/tmp/project".to_string()),
            project_root: None,
            requested_cwd: None,
            mcp_config: None,
            lifecycle: Some(Lifecycle::OnDemand),
            auto_start: None,
            dangerously_skip_permissions: None,
            model: None,
            model_profile: None,
            reasoning_effort: None,
            search_enabled: None,
            sandbox: None,
            approval_policy: None,
            tool_policy_path: None,
            traits: vec![],
            category: None,
            env: None,
            initial_prompt: None,
        };
        let slot = DynamicSlot {
            id: "slot-dyn-test".to_string(),
            parent_slot_id: "slot-jarvis".to_string(),
            template: "coder".to_string(),
            objective: None,
            config: serde_json::to_string(&config).unwrap(),
            status: "active".to_string(),
            termination_reason: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            terminated_at: None,
            ttl_seconds: 14400,
            expires_at: "2026-01-01T04:00:00Z".to_string(),
            extend_count: 0,
        };

        assert_eq!(
            dynamic_slot_project_root(&slot),
            Some("/tmp/project".to_string())
        );
    }
}
