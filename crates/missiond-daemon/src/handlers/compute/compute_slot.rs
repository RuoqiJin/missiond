use anyhow::{anyhow, Result};
use missiond_core::pty::{PTYSpawnOptions, Slot as PTYSlot};
use missiond_core::types::{AsyncJob, DynamicSlot, Lifecycle, SlotConfig};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::state::AppState;

/// Max dynamic slots allowed concurrently.
const MAX_DYNAMIC_SLOTS: i64 = 5;
/// Default TTL: 4 hours.
const DEFAULT_TTL_SECS: i64 = 14400;
/// Max TTL: 8 hours.
const MAX_TTL_SECS: i64 = 28800;
/// Max extension per request: 1 hour.
const MAX_EXTEND_SECS: i64 = 3600;

/// Allowed cwd prefixes (path traversal prevention).
const ALLOWED_CWD_PREFIXES: &[&str] = &[
    "/Users/jinchen/Projects",
    "/Users/jinchen/Documents",
    "/tmp",
];

const CODING_DEFAULT_PROFILE: &str = "coding-default-opus-4-7";

/// Built-in templates — derived from slots.yaml patterns.
fn get_template(name: &str) -> Option<TemplateConfig> {
    match name {
        "coder" => Some(TemplateConfig {
            role: "coder",
            description: "Dynamic coder slot (ephemeral)",
            mcp_config: Some("/Users/jinchen/.xjp-mission/xjp-mcp-config.json"),
            default_cwd: "/Users/jinchen/Projects",
            model: None,
        }),
        "ops" => Some(TemplateConfig {
            role: "operator",
            description: "Dynamic ops slot (ephemeral)",
            mcp_config: Some("/Users/jinchen/.xjp-mission/xjp-mcp-config.json"),
            default_cwd: "/Users/jinchen/Projects",
            model: Some("sonnet"),
        }),
        "researcher" => Some(TemplateConfig {
            role: "coder",
            description: "Dynamic researcher slot (read-only analysis)",
            mcp_config: Some("/Users/jinchen/.xjp-mission/xjp-mcp-config.json"),
            default_cwd: "/Users/jinchen/Projects",
            model: None,
        }),
        _ => None,
    }
}

struct TemplateConfig {
    role: &'static str,
    description: &'static str,
    mcp_config: Option<&'static str>,
    default_cwd: &'static str,
    model: Option<&'static str>,
}

pub(crate) fn resolve_model_projection(
    template_name: &str,
    model: Option<&str>,
    model_profile: Option<&str>,
) -> std::result::Result<Option<String>, String> {
    if let Some(value) = non_empty(model) {
        return normalize_model_override(value);
    }
    if let Some(profile) = non_empty(model_profile) {
        return model_for_profile(profile);
    }
    Ok(get_template(template_name).and_then(|template| template.model.map(str::to_string)))
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

fn model_for_profile(profile: &str) -> std::result::Result<Option<String>, String> {
    match normalize_profile(profile).as_str() {
        "default"
        | "claude-code-default"
        | "coding-default"
        | CODING_DEFAULT_PROFILE
        | "opus-4-7-default" => Ok(None),
        "daily-sonnet" | "sonnet" => Ok(Some("sonnet".to_string())),
        "quick-haiku" | "haiku" => Ok(Some("haiku".to_string())),
        other => Err(format!(
            "Unknown model_profile '{}'. Use {}, daily-sonnet, or quick-haiku.",
            other, CODING_DEFAULT_PROFILE
        )),
    }
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
    let template_name = match args.get("template").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "'template' is required")
                    .with_suggestion("Available templates: coder, ops, researcher"),
            ))
        }
    };

    let template = match get_template(template_name) {
        Some(t) => t,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("Unknown template '{}'", template_name),
                )
                .with_suggestion("Available templates: coder, ops, researcher"),
            ))
        }
    };
    let model = match resolve_model_projection(
        template_name,
        string_arg(args, &["model"]),
        string_arg(args, &["model_profile", "modelProfile"]),
    ) {
        Ok(model) => model,
        Err(message) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                message,
            )))
        }
    };

    // Check slot limit
    let active_count = state
        .store
        .count_active_dynamic_slots()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if active_count >= MAX_DYNAMIC_SLOTS {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::LIMIT_REACHED,
                format!(
                    "Dynamic slot limit reached ({}/{})",
                    active_count, MAX_DYNAMIC_SLOTS
                ),
            )
            .with_suggestion("Terminate existing slots with action=terminate first"),
        ));
    }

    // Validate cwd
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or(template.default_cwd);
    let canonical_cwd = match std::fs::canonicalize(cwd) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                format!("cwd '{}' does not exist or is not accessible", cwd),
            )))
        }
    };

    let canonical_path = std::path::Path::new(&canonical_cwd);
    let cwd_allowed = ALLOWED_CWD_PREFIXES
        .iter()
        .any(|prefix| canonical_path.starts_with(prefix));
    if !cwd_allowed {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::PERMISSION_DENIED,
                format!("cwd '{}' is not under allowed prefixes", cwd),
            )
            .with_suggestion(format!("Allowed prefixes: {:?}", ALLOWED_CWD_PREFIXES)),
        ));
    }

    // TTL
    let ttl = args
        .get("max_ttl")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_TTL_SECS);
    let ttl = ttl.min(MAX_TTL_SECS).max(300); // min 5 minutes

    let objective = args.get("objective").and_then(|v| v.as_str());
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

    // Generate slot ID
    let short_id = &uuid::Uuid::new_v4().to_string()[..8];
    let slot_id = format!("slot-dyn-{}", short_id);

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

    // Build SlotConfig — `cwd` becomes the canonical project root so that
    // spawn_tracked_slot picks it up as process cwd. The original requested
    // path (subdir) is retained in `requested_cwd` for prompt/context only.
    let slot_config = SlotConfig {
        id: slot_id.clone(),
        role: template.role.to_string(),
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
        mcp_config: template.mcp_config.map(|s| s.to_string()),
        lifecycle: Some(Lifecycle::OnDemand),
        auto_start: None,
        dangerously_skip_permissions: Some(false), // ALWAYS false for dynamic slots
        model,
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
                timeout_secs: Some(60),
                mcp_config,
                dangerously_skip_permissions: false,
                model: slot_config.model.clone(),
                extra_env: HashMap::new(),
                initial_prompt: initial_prompt_for_spawn,
            },
            slot_config.env.as_ref(),
        )
        .await;

        let mut store = state_clone.job_store.write().await;
        if let Some(job) = store.get_mut(&job_id_bg) {
            match result {
                Ok(_) => {
                    job.complete(json!({
                        "slot_id": slot_id_owned,
                        "status": "spawned",
                        "template": template_owned,
                        "model": slot_config.model.clone(),
                        "model_profile": if slot_config.model.is_none() { CODING_DEFAULT_PROFILE } else { "explicit-model" },
                        "ttl_seconds": ttl,
                        "expires_at": expires_at_str,
                        "objective": objective_owned,
                    }));
                }
                Err(e) => {
                    state_clone
                        .store
                        .terminate_dynamic_slot(&slot_id_owned, "spawn_failed")
                        .await
                        .ok();
                    state_clone.mission.unregister_dynamic_slot(&slot_id_owned);
                    job.fail(format!("Failed to spawn slot: {}", e));
                }
            }
        }
    });

    Ok(ToolResult::job_accepted(
        &job_id,
        "mission_compute_slot:create",
    ))
}

async fn terminate_slot(state: &AppState, args: &Value) -> Result<ToolResult> {
    let slot_id = match args.get("slot_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "'slot_id' is required",
            )))
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
            )))
        }
    };

    let additional = args
        .get("additional_seconds")
        .and_then(|v| v.as_i64())
        .unwrap_or(3600);

    if additional > MAX_EXTEND_SECS {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!("Max extension is {} seconds per request", MAX_EXTEND_SECS),
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
        None => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!(
                    "Cannot extend '{}': slot not found, not active, or max extensions (2) reached",
                    slot_id
                ),
            )
            .with_suggestion("Use action=list to check slot status"),
        )),
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

    let static_entries: Vec<Value> = static_slots
        .iter()
        .map(|s| {
            json!({
                "id": s.config.id,
                "source": "static",
                "role": s.config.role,
                "status": if s.session_id.is_some() { "running" } else { "stopped" },
                "lifecycle": format!("{}", s.config.lifecycle.unwrap_or_default()),
            })
        })
        .collect();

    Ok(ToolResult::json_pretty(&json!({
        "static_slots": static_entries,
        "dynamic_slots": dynamic_entries,
        "dynamic_active": dynamic_slots.iter().filter(|s| s.status == "active").count(),
        "dynamic_limit": MAX_DYNAMIC_SLOTS,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coder_template_uses_claude_code_default_profile() {
        assert_eq!(resolve_model_projection("coder", None, None).unwrap(), None);
        assert_eq!(
            resolve_model_projection("researcher", None, None).unwrap(),
            None
        );
    }

    #[test]
    fn ops_template_keeps_sonnet_default() {
        assert_eq!(
            resolve_model_projection("ops", None, None).unwrap(),
            Some("sonnet".to_string())
        );
    }

    #[test]
    fn caller_model_wins_over_profile() {
        assert_eq!(
            resolve_model_projection("coder", Some("haiku"), Some("daily-sonnet")).unwrap(),
            Some("haiku".to_string())
        );
    }

    #[test]
    fn default_alias_means_no_model_arg() {
        assert_eq!(
            resolve_model_projection("coder", Some("claude-code-default"), None).unwrap(),
            None
        );
        assert_eq!(
            resolve_model_projection("coder", None, Some("coding_default_opus_4_7")).unwrap(),
            None
        );
    }

    #[test]
    fn unsafe_model_token_is_rejected() {
        assert!(resolve_model_projection("coder", Some("sonnet;rm"), None).is_err());
        assert!(resolve_model_projection("coder", Some("sonnet 4.6"), None).is_err());
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
}
