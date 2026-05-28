use anyhow::{anyhow, Result};
use missiond_core::event::events::BoardEvent;
use missiond_core::pty::SessionState;
use missiond_core::types::CreateBoardTaskInput;
use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::process::Command;

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;
use crate::engine::control_plane_kernel::{
    ControlPlaneKernel, GrantTaskCapabilitiesCommand, RecordObservationCommand,
    ReleaseLeaseCommand, SettleTaskCommand, StartAttemptCommand,
};
use crate::engine::shared_memory::{StructuredControlError, TaskRuntimeContract};
use crate::engine::task_completion_evidence::TaskCompletionEvidenceInput;
use crate::slot_dispatch::SlotAcquireGuard;
use crate::state::AppState;

/// Roles excluded from auto-selection (meta agents, Jarvis itself).
const EXCLUDED_ROLES: &[&str] = &["jarvis", "memory", "supervisor", "decision"];

/// Phase 6.2: Valid intent whitelist — reject unknown intents instead of silent fallback.
const VALID_INTENTS: &[&str] = &["code", "ops", "deploy-ops", "research", "general"];

/// Phase 6.3: Context injection size limits.
const MAX_ENTRY_CHARS: usize = 500; // Per KB/Skill entry
const MAX_CONTEXT_CHARS: usize = 2000; // Total context block
const MAX_DESCRIPTION_CHARS: usize = 16000; // Final description
static SWARM_CONTEXT_PACK_COUNTER: AtomicU64 = AtomicU64::new(0);

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
            )));
        }
    };

    // Phase 6.2: Strict intent whitelist — fail-fast on unknown intent
    let intent = match args.get("intent").and_then(|v| v.as_str()) {
        Some(i) if VALID_INTENTS.contains(&i) => i,
        Some(i) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                &format!("Invalid intent '{}'. Valid: {:?}", i, VALID_INTENTS),
            )));
        }
        None => "general",
    };

    let priority = args
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let requested_timeout_secs = args.get("timeout_secs").and_then(|v| v.as_i64());
    let parent_id = string_arg(
        &args,
        &[
            "parent_id",
            "parentId",
            "parent_task_id",
            "parentTaskId",
            "parent_board_task_id",
            "parentBoardTaskId",
        ],
    )
    .map(str::to_string);

    let depends_on: Vec<String> = args
        .get("depends_on")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let _context_hints: Vec<String> = args
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
    let source_id = string_arg(
        &args,
        &[
            "source_id",
            "sourceId",
            "source_board_task_id",
            "sourceBoardTaskId",
        ],
    )
    .map(str::to_string);
    let allow_duplicate_code_worker = bool_arg(
        &args,
        &[
            "allow_duplicate_code_worker",
            "allowDuplicateCodeWorker",
            "force_duplicate_code_worker",
            "forceDuplicateCodeWorker",
        ],
    )
    .unwrap_or(false);
    let mut delegation_metadata = DelegationMetadata {
        task_class: string_arg(&args, &["task_class", "taskClass"]).map(str::to_string),
        pool_hint: string_arg(&args, &["pool_hint", "poolHint"]).map(str::to_string),
        engine_hint: string_arg(&args, &["engine_hint", "engineHint"]).map(str::to_string),
        context_pack_path: string_arg(&args, &["context_pack_path", "contextPackPath"])
            .map(str::to_string),
        accepted_shard_id: string_arg(&args, &["accepted_shard_id", "acceptedShardId"])
            .map(str::to_string),
        read_scope: string_list_arg(&args, &["read_scope", "readScope"]),
        write_scope: string_list_arg(&args, &["write_scope", "writeScope"]),
        must_not_touch: string_list_arg(&args, &["must_not_touch", "mustNotTouch"]),
        acceptance: string_list_arg(
            &args,
            &["acceptance", "acceptance_commands", "acceptanceCommands"],
        ),
        grounding_context_id: string_arg(&args, &["grounding_context_id", "groundingContextId"])
            .map(str::to_string),
        grounding_sources: string_list_arg(&args, &["grounding_sources", "groundingSources"]),
        grounding_evidence_refs_count: args
            .get("grounding_evidence_refs_count")
            .or_else(|| args.get("groundingEvidenceRefsCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        shared_claim_ids: Vec::new(),
        source_id: source_id.clone(),
    };
    if intent == "deploy-ops" {
        delegation_metadata
            .task_class
            .get_or_insert_with(|| "deploy-ops".to_string());
        delegation_metadata
            .pool_hint
            .get_or_insert_with(|| "claude-code-deploy-ops".to_string());
        delegation_metadata
            .engine_hint
            .get_or_insert_with(|| "claude-code".to_string());
    }
    let mechanic_config = match parse_mechanic_run_config(&args, &delegation_metadata) {
        Ok(config) => config,
        Err(error) => return Ok(error),
    };
    let xjpcode_worker = engine_hint_is_xjpcode(delegation_metadata.engine_hint.as_deref());
    let xjpcode_worker_endpoint = if xjpcode_worker {
        match xjpcode_worker_endpoint_from_args_or_env(&args) {
            Ok(endpoint) => endpoint,
            Err(error) => return Ok(error),
        }
    } else {
        None
    };
    if xjpcode_worker && !delegation_metadata.write_scope.is_empty() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "XJPCODE_WRITE_MODE_NOT_ENABLED",
                "mission_task_delegate refused xjpcode write mode because the portable worker write lane is still gated",
            )
            .with_suggestion(
                "rerun as read-only, or keep write work on Codex/ClaudeCode until xjpcode implements accepted_shard/write_lease/apply_patch",
            ),
        ));
    }
    if xjpcode_worker && xjpcode_worker_endpoint.is_none() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "XJPCODE_WORKER_NOT_CONFIGURED",
                "mission_task_delegate refused xjpcode dispatch because MISSIOND_XJPCODE_WORKER_URL is not configured",
            )
            .with_suggestion(
                "start xjpcode with --serve and set MISSIOND_XJPCODE_WORKER_URL to its base URL before dispatching xjpcode workers",
            ),
        ));
    }
    let delegate_sandbox_profile = sandbox_profile_for_worker(
        delegation_metadata.engine_hint.as_deref(),
        !delegation_metadata.write_scope.is_empty(),
    );
    if matches!(
        delegate_sandbox_profile,
        "unsupported-write" | "plan-policy-write"
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::SANDBOX_POLICY_UNSUPPORTED,
                format!(
                    "mission_task_delegate refused write-scoped task for engine {:?}: no enforceable path write sandbox",
                    delegation_metadata.engine_hint
                ),
            )
            .with_suggestion(
                "route write-scoped work to Codex/ClaudeCode with explicit write_scope, or rerun as read-only",
            ),
        ));
    }

    let explicit_exact_shard_ready =
        bool_arg(&args, &["exact_shard_ready", "exactShardReady"]).unwrap_or(false);
    if !explicit_exact_shard_ready {
        if let Some(error) = exact_shard_contract_error(intent, &delegation_metadata) {
            return Ok(error);
        }
    } else if intent == "code" {
        delegation_metadata
            .task_class
            .get_or_insert_with(|| "implementation".to_string());
    }

    let cwd = args.get("cwd").and_then(|v| v.as_str());

    // Resolve target_project_root (intent-flow.lisp ::
    // F-task-delegate-autoprovision :: s1b). When cwd is supplied, reject
    // if it does not resolve under a registered project. When cwd is absent,
    // we leave target_project_root as None and the auto-provision branch will
    // surface the issue (compute_slot create requires a registered cwd).
    let target_project_resolution = if let Some(cwd_val) = cwd {
        match crate::slot_orchestrator::project_root::resolve_target_project_root(
            None,
            Some(std::path::Path::new(cwd_val)),
            None,
            &state.project_registry,
        )
        .await
        {
            Ok(r) => Some(r),
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
    let target_project_root = target_project_resolution
        .as_ref()
        .map(|r| r.project_root.to_string_lossy().to_string());
    if mechanic_config.is_some() && target_project_root.is_none() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "MECHANIC_PROJECT_ROOT_REQUIRED",
                "mission_task_delegate refused engine_hint=mechanic without a registered cwd/project root",
            )
            .with_suggestion(
                "pass cwd inside the target project root so mechanic can run in a detached worktree",
            ),
        ));
    }

    let emergency_code_first =
        bool_arg(&args, &["emergency_code_first", "emergencyCodeFirst"]).unwrap_or(false);
    if dispatch_grounding_required(&delegation_metadata, emergency_code_first) {
        let project_id = target_project_resolution
            .as_ref()
            .map(|resolution| resolution.project_id.as_str());
        let grounding = match gather_dispatch_grounding(
            state,
            &args,
            objective,
            project_id,
            source_id.as_deref().or(parent_id.as_deref()),
        )
        .await?
        {
            Ok(value) => value,
            Err(error) => return Ok(error),
        };
        apply_grounding_to_metadata(&mut delegation_metadata, &grounding);
    }

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
        "ops" | "deploy-ops" => "ops",
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
            )));
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

    // Default-on guard: when this delegation would spawn a CODE worker that
    // shares the same semantic parent/source as an active BoardTask AND its
    // declared write_scope overlaps the existing one, refuse the spawn so
    // two concurrent workers cannot race on the same files. Read-only /
    // context-pack / research delegations never trip the guard because
    // `dedup_applies` requires both `intent=code` (or `task_class=code`) AND
    // a non-empty write_scope. Callers can override with
    // `allow_duplicate_code_worker=true` when the overlap is intentional.
    let dedup_check = DuplicateCodeCheck {
        parent_id: parent_id.as_deref(),
        source_id: source_id.as_deref(),
        project_id: target_project_resolution
            .as_ref()
            .map(|resolution| resolution.project_id.as_str()),
        intent,
        task_class: delegation_metadata.task_class.as_deref(),
        write_scope: &delegation_metadata.write_scope,
    };
    if !allow_duplicate_code_worker {
        if let Some(dup) = match find_overlapping_active_code_worker(state, &dedup_check).await {
            Ok(dup) => dup,
            Err(err) => {
                let (code, details) =
                    if let Some(control) = err.downcast_ref::<StructuredControlError>() {
                        (control.code, control.details.clone())
                    } else {
                        (error_codes::DB_ERROR, json!({}))
                    };
                return Ok(ToolResult::structured_error(
                    ToolError::new(code, format!("mission_task_delegate dedup guard failed: {err}"))
                        .with_details(details)
                        .with_suggestion(
                            "backfill task_contracts for active legacy BoardTasks before delegating overlapping code work",
                        ),
                ));
            }
        } {
            let note_attached = attach_duplicate_delegation_note(
                state,
                &dup,
                objective,
                parent_id.as_deref(),
                source_id.as_deref(),
                &delegation_metadata.write_scope,
            )
            .await;
            return Ok(build_duplicate_code_worker_refusal(
                &dup,
                parent_id.as_deref(),
                source_id.as_deref(),
                note_attached,
            ));
        }
    }

    if code_worker_requires_write_lease(intent, &delegation_metadata) {
        let owner_seed = source_id
            .as_deref()
            .or(parent_id.as_deref())
            .unwrap_or(objective)
            .chars()
            .take(80)
            .collect::<String>();
        let claims = state
            .shared_memory
            .claim_write_scope(
                target_project_resolution
                    .as_ref()
                    .map(|resolution| resolution.project_id.clone()),
                None,
                format!("mission_task_delegate:{owner_seed}"),
                &delegation_metadata.write_scope,
                delegation_metadata.accepted_shard_id.clone(),
            )
            .await
            .map_err(|e| anyhow!("shared-memory claim error: {}", e))?;
        let conflicts = claims
            .iter()
            .filter(|claim| claim.get("ok").and_then(|v| v.as_bool()) == Some(false))
            .cloned()
            .collect::<Vec<_>>();
        if !conflicts.is_empty() {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    "SHARED_MEMORY_WRITE_LEASE_CONFLICT",
                    format!(
                        "mission_task_delegate refused implementation worker because {} write_scope lease(s) are already active",
                        conflicts.len()
                    ),
                )
                .with_suggestion(
                    "inspect mission_claim_status for the conflicting scope, then wait, split the shard, or release the stale claim",
                ),
            ));
        }
        delegation_metadata.shared_claim_ids = claims
            .iter()
            .filter_map(|claim| {
                claim
                    .pointer("/claim/id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .collect();
    }

    // Phase 6.1: Find idle slot with RAII guard (atomic check+reserve).
    // intent-flow.lisp F-task-delegate-autoprovision :: s2 requires
    // slot.project_root == target_project_root for reuse.
    let guard = if mechanic_config.is_some() || xjpcode_worker {
        None
    } else if prefer_gemini_researcher {
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
    let (mut assignee, mut provisioned, should_auto_provision_slot) = if xjpcode_worker {
        ("xjpcode-readonly-worker".to_string(), false, false)
    } else if !assignee.is_empty() {
        (assignee, false, false)
    } else if mechanic_config.is_some() {
        (String::new(), false, false)
    } else if prefer_gemini_researcher {
        // V3: never auto-provision a dynamic Claude slot for research while a
        // gemini researcher slot is registered. Queue unassigned so the
        // autopilot can route the BoardTask to the gemini slot once idle.
        (String::new(), false, false)
    } else if template == "ops" {
        // Phase 6.2: Guard uses template, not intent — prevents intent escape
        // Queue without assignee; autopilot will pick up when a slot frees
        (String::new(), false, false)
    } else {
        (String::new(), false, true)
    };
    // guard is dropped here (if Some) → auto-releases slot dispatch lock
    let preallocated_slot_id = should_auto_provision_slot.then(new_dynamic_slot_id);

    // 3. Build description. `context_hints` is accepted for API
    // compatibility, but default worker prompts must not auto-prefetch KB or
    // Skill snippets while those stores are still noisy. Explicit context
    // belongs in read_scope/context-pack paths, not hidden prompt injection.
    let mut description = objective.to_string();
    let metadata_block = render_delegation_metadata_block(&delegation_metadata);
    if !metadata_block.is_empty() {
        description = format!(
            "{}\n\n## Dispatch metadata\n{}",
            description, metadata_block
        );
    }
    if let Some(parent) = parent_id.as_deref() {
        description = format!(
            "{}\n\n## Parent linkage\n- parent_board_task_id: {}",
            description, parent
        );
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
    let category = if matches!(intent, "ops" | "deploy-ops") {
        "ops"
    } else {
        "dev"
    };
    let runtime_metadata = delegation_runtime_metadata(
        &delegation_metadata,
        target_project_resolution
            .as_ref()
            .map(|resolution| resolution.project_id.as_str()),
        target_project_root.as_deref(),
        parent_id.as_deref(),
    );
    let input = CreateBoardTaskInput {
        title: truncate_title(objective),
        description: Some(description),
        priority: Some(priority.to_string()),
        category: Some(category.to_string()),
        project: target_project_resolution
            .as_ref()
            .map(|r| r.project_id.clone()),
        assignee: if assignee.is_empty() {
            None
        } else {
            Some(assignee.clone())
        },
        auto_execute: Some(!mechanic_config.is_some() && !xjpcode_worker),
        depends_on: if depends_on.is_empty() {
            None
        } else {
            Some(depends_on)
        },
        timeout_secs: Some(timeout_secs),
        context_intent: Some(intent.to_string()),
        parent_id: parent_id.clone(),
        runtime_metadata: Some(runtime_metadata.clone()),
        ..Default::default()
    };

    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let task_id = task.id.to_string();
    let sandbox_profile = delegate_sandbox_profile;
    let grant_subject_id = preallocated_slot_id
        .as_deref()
        .or_else(|| (!assignee.is_empty()).then_some(assignee.as_str()))
        .or_else(|| mechanic_config.is_some().then_some("mechanic"));
    let capability_grant_ids = if let Some(subject_id) = grant_subject_id {
        ControlPlaneKernel::new(state)
            .grant_task_capabilities_command(GrantTaskCapabilitiesCommand {
                project_id: target_project_resolution
                    .as_ref()
                    .map(|resolution| resolution.project_id.clone()),
                task_id: task_id.clone(),
                subject_kind: "worker".to_string(),
                subject_id: subject_id.to_string(),
                read_scope: delegation_metadata.read_scope.clone(),
                write_scope: delegation_metadata.write_scope.clone(),
                must_not_touch: delegation_metadata.must_not_touch.clone(),
                issuer: "mission_task_delegate".to_string(),
            })
            .await
            .map_err(|e| anyhow!("capability grant error: {}", e))?
    } else {
        Vec::new()
    };
    let runtime_metadata = enrich_runtime_metadata_with_control_facts(
        runtime_metadata,
        &task_id,
        &capability_grant_ids,
        sandbox_profile,
    );
    let _ = state
        .store
        .update_board_task(
            &task_id,
            &missiond_core::types::UpdateBoardTaskInput {
                runtime_metadata: Some(runtime_metadata.clone()),
                ..Default::default()
            },
        )
        .await;
    state
        .shared_memory
        .upsert_task_contract_from_metadata(
            &task_id,
            target_project_resolution
                .as_ref()
                .map(|resolution| resolution.project_id.as_str()),
            &runtime_metadata,
        )
        .await
        .map_err(|e| anyhow!("task contract error: {}", e))?;
    if should_auto_provision_slot {
        match auto_provision_slot(
            state,
            template,
            objective,
            &runtime_config,
            cwd,
            model_arg.as_deref(),
            effective_model_profile,
            Some(delegate_sandbox_profile),
            Some(&task_id),
            Some(&capability_grant_ids),
            preallocated_slot_id.as_deref(),
        )
        .await
        {
            Ok(id) => {
                assignee = id;
                provisioned = true;
                let _ = state
                    .store
                    .update_board_task(
                        &task_id,
                        &missiond_core::types::UpdateBoardTaskInput {
                            assignee: Some(assignee.clone()),
                            ..Default::default()
                        },
                    )
                    .await;
            }
            Err(e) => {
                tracing::warn!("Auto-provision failed, queueing without assignee: {}", e);
            }
        }
    }

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
    // Mechanic BoardTasks are visible records for the subprocess executor
    // lane; they must not enter the normal Autopilot/PTY dispatcher.
    if mechanic_config.is_none() && !xjpcode_worker {
        state.board_dispatch_notify.notify_one();
    }

    if xjpcode_worker {
        spawn_xjpcode_readonly_worker(
            state.clone(),
            XjpcodeWorkerRun {
                task_id: task_id.clone(),
                endpoint: xjpcode_worker_endpoint.clone().unwrap_or_default(),
                project_id: target_project_resolution
                    .as_ref()
                    .map(|resolution| resolution.project_id.clone())
                    .unwrap_or_else(|| "missiond".to_string()),
                project_root: target_project_root.clone(),
                objective: objective.to_string(),
                metadata: delegation_metadata.clone(),
                capability_grant_ids: capability_grant_ids.clone(),
                write_task_grant_id: task_write_grant_id(
                    &delegation_metadata,
                    &capability_grant_ids,
                ),
                settle_task_grant_id: task_settle_grant_id(
                    &delegation_metadata,
                    &capability_grant_ids,
                ),
                timeout_secs,
            },
        );
    }

    if let Some(config) = mechanic_config.clone() {
        spawn_mechanic_repair(
            state.clone(),
            MechanicRepairRun {
                task_id: task_id.clone(),
                project_id: target_project_resolution
                    .as_ref()
                    .map(|resolution| resolution.project_id.clone()),
                project_root: target_project_root.clone().unwrap_or_default(),
                objective: objective.to_string(),
                metadata: delegation_metadata.clone(),
                capability_grant_ids: capability_grant_ids.clone(),
                write_task_grant_id: task_write_grant_id(
                    &delegation_metadata,
                    &capability_grant_ids,
                ),
                settle_task_grant_id: task_settle_grant_id(
                    &delegation_metadata,
                    &capability_grant_ids,
                ),
                claim_task_grant_id: task_claim_grant_id(
                    &delegation_metadata,
                    &capability_grant_ids,
                ),
                attempt_id: format!(
                    "attempt:{}:mechanic:{}",
                    task_id,
                    chrono::Utc::now().timestamp_millis()
                ),
                config,
                timeout_secs,
            },
        );
    }

    Ok(ToolResult::json_pretty(&json!({
        "task_id": task_id,
        "assignee": if assignee.is_empty() { Value::Null } else { Value::String(assignee) },
        "status": if xjpcode_worker { "xjpcode-dispatched" } else if mechanic_config.is_some() { "mechanic-dispatched" } else { "queued" },
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
        "capability_grant_ids": capability_grant_ids,
        "sandbox_profile": sandbox_profile,
        "task_contract_id": format!("board-task:{task_id}"),
        "shared_claim_ids": delegation_metadata.shared_claim_ids,
        "mechanic": mechanic_config.as_ref().map(MechanicRunConfig::to_json),
        "provisioned_new_slot": provisioned,
        "timeout_secs": timeout_secs,
        "parent_id": parent_id,
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
            )));
        }
    };
    let project_id = string_arg(&args, &["project_id", "projectId"])
        .unwrap_or("missiond")
        .to_string();
    let parent_id = string_arg(
        &args,
        &[
            "parent_id",
            "parentId",
            "parent_task_id",
            "parentTaskId",
            "parent_board_task_id",
            "parentBoardTaskId",
        ],
    )
    .map(str::to_string);
    let project_root =
        match crate::slot_orchestrator::project_root::resolve_target_project_root(
            Some(&project_id),
            None,
            None,
            &state.project_registry,
        )
        .await
        {
            Ok(resolution) => resolution.project_root.to_string_lossy().to_string(),
            Err(err) => return Ok(ToolResult::structured_error(
                ToolError::new(
                    "PROJECT_ROOT_UNRESOLVED",
                    format!("mission_swarm_run project_id unresolved: {}", err),
                )
                .with_suggestion(
                    "register the target project before spawning external-project swarm workers",
                ),
            )),
        };
    let target_project_ids = string_list_arg(
        &args,
        &[
            "target_project_ids",
            "targetProjectIds",
            "target_projects",
            "targetProjects",
        ],
    );
    let target_projects =
        match resolve_swarm_target_projects(state, &project_id, &project_root, &target_project_ids)
            .await
        {
            Ok(targets) => targets,
            Err(err) => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        "SWARM_TARGET_PROJECT_UNRESOLVED",
                        format!("mission_swarm_run target project unresolved: {}", err),
                    )
                    .with_suggestion(
                        "register every target project before spawning multi-project swarm workers",
                    ),
                ));
            }
        };
    let missiond_root = match crate::slot_orchestrator::project_root::resolve_target_project_root(
        Some("missiond"),
        None,
        None,
        &state.project_registry,
    )
    .await
    {
        Ok(resolution) => resolution.project_root,
        Err(err) => {
            let tool_error = ToolError::new(
                "MISSIOND_ROOT_UNRESOLVED",
                format!("mission_swarm_run missiond root unresolved: {}", err),
            )
            .with_suggestion(
                "register the missiond project so shared context-pack paths never depend on daemon cwd",
            );
            return Ok(ToolResult::structured_error(tool_error));
        }
    };
    let context_pack_path = string_arg(&args, &["context_pack_path", "contextPackPath"])
        .map(|value| normalize_context_pack_path_for_worker(value, Some(&missiond_root)))
        .unwrap_or_else(|| default_swarm_context_pack_path(Some(&missiond_root)));
    let runtime_config = match WorkstationRuntimeConfig::load_for_project_root(Some(
        &missiond_root.to_string_lossy(),
    )) {
        Ok(config) => config,
        Err(err) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("V3_BLUEPRINT_CONFIG_ERROR", err.to_string()).with_suggestion(
                    "ensure MissionD .missiond/v3/missiond-blueprint.lisp contains workstation-config capacity-policy swarm-workers",
                ),
            ));
        }
    };
    let max_claude_workers = runtime_config.clamp_swarm_claude_workers(optional_usize_arg(
        &args,
        &["max_claude_workers", "maxClaudeWorkers"],
    ));
    let max_gemini_workers = runtime_config.clamp_swarm_gemini_workers(optional_usize_arg(
        &args,
        &["max_gemini_workers", "maxGeminiWorkers"],
    ));
    let write_policy = string_arg(&args, &["write_policy", "writePolicy"])
        .unwrap_or("read-only")
        .to_string();
    let dry_run = bool_arg(&args, &["dry_run", "dryRun"]).unwrap_or(true);
    let auto_provision_slots =
        bool_arg(&args, &["auto_provision_slots", "autoProvisionSlots"]).unwrap_or(true);
    let acceptance = string_list_arg(
        &args,
        &["acceptance", "acceptance_commands", "acceptanceCommands"],
    );
    let implement_write_scope = string_list_arg(&args, &["write_scope", "writeScope"]);
    let implement_must_not_touch = string_list_arg(&args, &["must_not_touch", "mustNotTouch"]);
    let accepted_shard_id =
        string_arg(&args, &["accepted_shard_id", "acceptedShardId"]).map(str::to_string);
    if swarm_policy_requires_implement_write_scope(&write_policy)
        && implement_write_scope.is_empty()
    {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "SWARM_IMPLEMENT_WRITE_SCOPE_REQUIRED",
                "mission_swarm_run refused to plan implement workers without an explicit write_scope",
            )
            .with_suggestion(
                "rerun with write_policy=read-only for investigation only, or provide exact disjoint write_scope entries for every implement shard",
            ),
        ));
    }
    if swarm_policy_requires_implement_write_scope(&write_policy) && accepted_shard_id.is_none() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "SWARM_ACCEPTED_SHARD_REQUIRED",
                "mission_swarm_run refused to create implementation workers without accepted_shard_id",
            )
            .with_suggestion(
                "run an investigation/synthesis pass first, then rerun with accepted_shard_id pointing at the accepted exact shard",
            ),
        ));
    }
    let caller_read_scope = string_list_arg(&args, &["read_scope", "readScope"]);
    let mut read_scope = caller_read_scope.clone();
    let target_roots = target_projects
        .iter()
        .map(|target| target.root.clone())
        .collect::<Vec<_>>();
    if read_scope.is_empty() {
        read_scope = target_roots.clone();
    } else {
        append_unique_strings(&mut read_scope, target_roots);
    }
    let timeout_secs = args
        .get("timeout_secs")
        .or_else(|| args.get("timeoutSecs"))
        .and_then(|v| v.as_i64())
        .unwrap_or(1800)
        .clamp(60, 7200);
    let mut grounding_context_id =
        string_arg(&args, &["grounding_context_id", "groundingContextId"]).map(str::to_string);
    let mut grounding_sources = string_list_arg(&args, &["grounding_sources", "groundingSources"]);
    let mut grounding_evidence_refs_count = args
        .get("grounding_evidence_refs_count")
        .or_else(|| args.get("groundingEvidenceRefsCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let mut grounding_payload: Option<Value> = None;

    if swarm_policy_requires_implement_write_scope(&write_policy) && grounding_context_id.is_none()
    {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "SWARM_GROUNDING_CONTEXT_REQUIRED",
                "mission_swarm_run refused implementation workers because accepted shards must reference an existing grounding_context_id",
            )
            .with_suggestion(
                "run a read-only investigation/synthesis pass first, then rerun with grounding_context_id plus accepted_shard_id/write_scope",
            ),
        ));
    }
    if !dry_run
        && !swarm_policy_requires_implement_write_scope(&write_policy)
        && grounding_context_id.is_none()
    {
        let grounding = match gather_dispatch_grounding(
            state,
            &args,
            &objective,
            Some(&project_id),
            parent_id.as_deref(),
        )
        .await?
        {
            Ok(value) => value,
            Err(error) => return Ok(error),
        };
        grounding_context_id = grounding
            .get("grounding_context_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        grounding_sources = grounding
            .get("sources_used")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        grounding_evidence_refs_count = grounding
            .get("evidence_refs")
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0);
        grounding_payload = Some(grounding);
    }

    let mut planned = Vec::new();
    for idx in 0..max_gemini_workers {
        let worker_count = max_gemini_workers.max(1);
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
            read_scope: swarm_read_scope_for_worker(
                idx,
                worker_count,
                &read_scope,
                &target_projects,
                !target_project_ids.is_empty(),
            ),
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
            accepted_shard_id: None,
            shared_claim_ids: Vec::new(),
        });
    }
    for idx in 0..max_claude_workers {
        let worker_count = max_claude_workers.max(1);
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
            read_scope: swarm_read_scope_for_worker(
                idx,
                worker_count,
                &read_scope,
                &target_projects,
                !target_project_ids.is_empty(),
            ),
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
            accepted_shard_id: None,
            shared_claim_ids: Vec::new(),
        });
    }

    if swarm_policy_requires_implement_write_scope(&write_policy) {
        planned.push(SwarmPlannedTask {
            lane: "implement".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "code".to_string(),
            title: "Implement accepted swarm shard after context-pack integration".to_string(),
            intent: "code".to_string(),
            read_scope: read_scope.clone(),
            write_scope: implement_write_scope.clone(),
            must_not_touch: implement_must_not_touch.clone(),
            accepted_shard_id: accepted_shard_id.clone(),
            shared_claim_ids: Vec::new(),
        });
    }

    let conflicts = detect_swarm_write_conflicts(&planned);
    if !conflicts.is_empty() && !dry_run {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "SWARM_WRITE_SCOPE_CONFLICT",
                "mission_swarm_run refused to create overlapping write-scope implementers",
            )
            .with_suggestion(
                "split the objective into disjoint write_scope shards or rerun dry_run=true and inspect conflicts",
            ),
        ));
    }

    if !dry_run && swarm_policy_requires_implement_write_scope(&write_policy) {
        for planned_task in planned
            .iter_mut()
            .filter(|task| !task.write_scope.is_empty())
        {
            let claims = state
                .shared_memory
                .claim_write_scope(
                    Some(project_id.clone()),
                    parent_id.clone(),
                    format!(
                        "mission_swarm_run:{}:{}",
                        project_id,
                        planned_task
                            .accepted_shard_id
                            .as_deref()
                            .unwrap_or("accepted-shard")
                    ),
                    &planned_task.write_scope,
                    planned_task.accepted_shard_id.clone(),
                )
                .await
                .map_err(|e| anyhow!("shared-memory claim error: {}", e))?;
            let conflicts = claims
                .iter()
                .filter(|claim| claim.get("ok").and_then(|v| v.as_bool()) == Some(false))
                .cloned()
                .collect::<Vec<_>>();
            if !conflicts.is_empty() {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        "SWARM_SHARED_MEMORY_WRITE_LEASE_CONFLICT",
                        format!(
                            "mission_swarm_run refused implementation shard because {} write_scope lease(s) are already active",
                            conflicts.len()
                        ),
                    )
                    .with_suggestion(
                        "inspect mission_claim_status, wait/release stale claims, or split the accepted shard into disjoint write_scope entries",
                    ),
                ));
            }
            planned_task.shared_claim_ids = claims
                .iter()
                .filter_map(|claim| {
                    claim
                        .pointer("/claim/id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
                .collect();
        }
    }

    let context_pack_materialized = if dry_run {
        false
    } else {
        match materialize_swarm_context_pack(
            &context_pack_path,
            &objective,
            &project_id,
            &project_root,
            &target_projects,
            parent_id.as_deref(),
            &write_policy,
            &acceptance,
            &planned,
            grounding_context_id.as_deref(),
            &grounding_sources,
            grounding_evidence_refs_count,
        )
        .await
        {
            Ok(()) => true,
            Err(err) => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        "SWARM_CONTEXT_PACK_WRITE_FAILED",
                        format!(
                            "failed to materialize swarm context_pack_path {}: {}",
                            context_pack_path, err
                        ),
                    )
                    .with_suggestion(
                        "ensure the MissionD project root is writable and rerun mission_swarm_run",
                    ),
                ));
            }
        }
    };

    let workflow_run_id = if dry_run {
        None
    } else {
        Some(
            string_arg(&args, &["workflow_run_id", "workflowRunId"])
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "swarm-run:{}:{}",
                        project_id,
                        chrono::Utc::now().timestamp_millis()
                    )
                }),
        )
    };
    let workflow_start_result = if let Some(id) = workflow_run_id.as_deref() {
        Some(
            state
                .storage()
                .shared_memory
                .workflow_start_typed(&json!({
                    "workflow_run_id": id,
                    "workflow_id": "mission_swarm_run",
                    "workflow_path": ".missiond/workflows/swarm-run.lisp",
                    "project_id": project_id.as_str(),
                    "parent_task_id": parent_id.as_deref(),
                    "max_inflight": max_claude_workers + max_gemini_workers,
                    "cursor": { "planned_count": planned.len() },
                    "checkpoint": {
                        "objective": objective.as_str(),
                        "context_pack_path": context_pack_path.as_str(),
                        "write_policy": write_policy.as_str()
                    }
                }))
                .await,
        )
    } else {
        None
    };

    let mut created_task_ids = Vec::new();
    let mut provisioned_slots = Vec::new();
    if !dry_run {
        for planned_task in &planned {
            let (task_project_id, task_project_root) = planned_task_primary_project(
                &project_id,
                &project_root,
                &target_projects,
                planned_task,
            );
            let assignee: Option<String> = None;
            let should_auto_provision_child =
                auto_provision_slots && planned_task.engine_hint == "claude-code";
            let preallocated_slot_id = should_auto_provision_child.then(new_dynamic_slot_id);
            let description = render_swarm_task_description(
                &objective,
                &task_project_id,
                &task_project_root,
                &target_projects,
                &context_pack_path,
                parent_id.as_deref(),
                &write_policy,
                &acceptance,
                planned_task,
                grounding_context_id.as_deref(),
                &grounding_sources,
                grounding_evidence_refs_count,
            );
            let runtime_metadata = swarm_task_runtime_metadata(
                &task_project_id,
                &task_project_root,
                &target_projects,
                &context_pack_path,
                parent_id.as_deref(),
                &write_policy,
                &acceptance,
                planned_task,
                grounding_context_id.as_deref(),
                &grounding_sources,
                grounding_evidence_refs_count,
            );
            let input = CreateBoardTaskInput {
                title: planned_task.title.clone(),
                description: Some(description),
                priority: Some("medium".to_string()),
                category: Some("dev".to_string()),
                project: Some(task_project_id.clone()),
                auto_execute: Some(true),
                assignee,
                parent_id: parent_id.clone(),
                timeout_secs: Some(timeout_secs),
                context_intent: Some(planned_task.intent.clone()),
                runtime_metadata: Some(runtime_metadata.clone()),
                ..Default::default()
            };
            let task = state
                .store
                .create_board_task(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let task_id = task.id.to_string();
            let sandbox_profile = sandbox_profile_for_worker(
                Some(&planned_task.engine_hint),
                !planned_task.write_scope.is_empty(),
            );
            let capability_grant_ids = if let Some(subject_id) = preallocated_slot_id.as_deref() {
                ControlPlaneKernel::new(state)
                    .grant_task_capabilities_command(GrantTaskCapabilitiesCommand {
                        project_id: Some(task_project_id.clone()),
                        task_id: task_id.clone(),
                        subject_kind: "worker".to_string(),
                        subject_id: subject_id.to_string(),
                        read_scope: planned_task.read_scope.clone(),
                        write_scope: planned_task.write_scope.clone(),
                        must_not_touch: planned_task.must_not_touch.clone(),
                        issuer: "mission_swarm_run".to_string(),
                    })
                    .await
                    .map_err(|e| anyhow!("capability grant error: {}", e))?
            } else {
                Vec::new()
            };
            let runtime_metadata = enrich_runtime_metadata_with_control_facts(
                runtime_metadata,
                &task_id,
                &capability_grant_ids,
                sandbox_profile,
            );
            let _ = state
                .store
                .update_board_task(
                    &task_id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        runtime_metadata: Some(runtime_metadata.clone()),
                        ..Default::default()
                    },
                )
                .await;
            state
                .shared_memory
                .upsert_task_contract_from_metadata(
                    &task_id,
                    Some(&task_project_id),
                    &runtime_metadata,
                )
                .await
                .map_err(|e| anyhow!("task contract error: {}", e))?;
            if should_auto_provision_child {
                match auto_provision_slot(
                    state,
                    "coder",
                    &planned_task.title,
                    &runtime_config,
                    Some(&task_project_root),
                    None,
                    Some("coding-default-opus-4-7"),
                    Some(sandbox_profile),
                    Some(&task_id),
                    Some(&capability_grant_ids),
                    preallocated_slot_id.as_deref(),
                )
                .await
                {
                    Ok(slot_id) => {
                        let _ = state
                            .store
                            .update_board_task(
                                &task_id,
                                &missiond_core::types::UpdateBoardTaskInput {
                                    assignee: Some(slot_id.clone()),
                                    ..Default::default()
                                },
                            )
                            .await;
                        provisioned_slots.push(json!({
                            "task_title": planned_task.title,
                            "task_id": task_id,
                            "slot_id": slot_id,
                            "engine_hint": planned_task.engine_hint,
                            "pool_hint": planned_task.pool_hint,
                            "status": "spawn_pending",
                        }));
                    }
                    Err(err) => {
                        tracing::warn!(
                            project_id = %project_id,
                            title = %planned_task.title,
                            error = %err,
                            "mission_swarm_run: Claude dynamic slot auto-provision failed; child task will queue unassigned"
                        );
                        provisioned_slots.push(json!({
                            "task_title": planned_task.title,
                            "task_id": task_id,
                            "engine_hint": planned_task.engine_hint,
                            "pool_hint": planned_task.pool_hint,
                            "status": "provision_failed",
                            "error": err.to_string(),
                        }));
                    }
                }
            }
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

    let workflow_checkpoint_result =
        if let (false, Some(id)) = (dry_run, workflow_run_id.as_deref()) {
            Some(
                state
                    .storage()
                    .shared_memory
                    .workflow_checkpoint_typed(&json!({
                        "workflow_run_id": id,
                        "status": "running",
                        "cursor": { "created_task_ids": created_task_ids.clone() },
                        "checkpoint": {
                            "objective": objective.as_str(),
                            "context_pack_path": context_pack_path.as_str(),
                            "created_task_count": created_task_ids.len()
                        },
                        "active_task_ids": created_task_ids.clone(),
                        "artifact_hashes": []
                    }))
                    .await,
            )
        } else {
            None
        };

    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.swarm-run.v1",
        "ok": true,
        "dry_run": dry_run,
        "objective": objective,
        "project_id": project_id,
        "project_root": project_root,
        "target_projects": target_projects.iter().map(SwarmTargetProject::to_json).collect::<Vec<_>>(),
        "context_pack_path": context_pack_path,
        "grounding_context_id": grounding_context_id,
        "grounding_sources": grounding_sources,
        "grounding_evidence_refs_count": grounding_evidence_refs_count,
        "grounding_artifact": grounding_payload,
        "accepted_shard_id": accepted_shard_id,
        "context_pack_materialized": context_pack_materialized,
        "parent_id": parent_id,
        "write_policy": write_policy,
        "fanout": {
            "max_claude_workers": max_claude_workers,
            "max_gemini_workers": max_gemini_workers,
            "dynamic_slot_limit": runtime_config.dynamic_slot_limit(),
            "delegate_rate_per_minute": runtime_config.delegate_rate_per_minute()
        },
        "auto_provision_slots": auto_provision_slots,
        "provisioned_slots": provisioned_slots,
        "planned_tasks": planned.iter().map(SwarmPlannedTask::to_json).collect::<Vec<_>>(),
        "created_task_ids": created_task_ids,
        "workflow_run_id": workflow_run_id,
        "workflow_start": workflow_start_result.map(|result| result.unwrap_or_else(|err| json!({ "ok": false, "error": err.to_string() }))),
        "workflow_checkpoint": workflow_checkpoint_result.map(|result| result.unwrap_or_else(|err| json!({ "ok": false, "error": err.to_string() }))),
        "conflicts": conflicts,
        "next_action": if dry_run {
            "rerun mission_swarm_run with dry_run=false after reviewing planned_tasks"
        } else {
            "watch BoardTask lifecycle and provider durable logs before closing the swarm objective"
        }
    })))
}

async fn materialize_swarm_context_pack(
    path: &str,
    objective: &str,
    project_id: &str,
    project_root: &str,
    target_projects: &[SwarmTargetProject],
    parent_id: Option<&str>,
    write_policy: &str,
    acceptance: &[String],
    planned: &[SwarmPlannedTask],
    grounding_context_id: Option<&str>,
    grounding_sources: &[String],
    grounding_evidence_refs_count: usize,
) -> Result<()> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let source = render_swarm_context_pack(
        objective,
        project_id,
        project_root,
        target_projects,
        parent_id,
        write_policy,
        acceptance,
        planned,
        grounding_context_id,
        grounding_sources,
        grounding_evidence_refs_count,
    );
    tokio::fs::write(path, source).await?;
    Ok(())
}

fn render_swarm_context_pack(
    objective: &str,
    project_id: &str,
    project_root: &str,
    target_projects: &[SwarmTargetProject],
    parent_id: Option<&str>,
    write_policy: &str,
    acceptance: &[String],
    planned: &[SwarmPlannedTask],
    grounding_context_id: Option<&str>,
    grounding_sources: &[String],
    grounding_evidence_refs_count: usize,
) -> String {
    let mut out = String::new();
    out.push_str("(swarm-context-pack\n");
    out.push_str("  :schema \"missiond.swarm-context-pack.v1\"\n");
    out.push_str(&format!(
        "  :created_at {}\n",
        lisp_string(&chrono::Utc::now().to_rfc3339())
    ));
    out.push_str(&format!("  :project_id {}\n", lisp_string(project_id)));
    out.push_str(&format!("  :project_root {}\n", lisp_string(project_root)));
    out.push_str("  :target_projects\n  (\n");
    for target in target_projects {
        out.push_str(&format!(
            "    (project :id {} :root {})\n",
            lisp_string(&target.id),
            lisp_string(&target.root)
        ));
    }
    out.push_str("  )\n");
    if let Some(parent) = parent_id {
        out.push_str(&format!(
            "  :parent_board_task_id {}\n",
            lisp_string(parent)
        ));
    }
    out.push_str(&format!("  :write_policy {}\n", lisp_string(write_policy)));
    if let Some(id) = grounding_context_id {
        out.push_str(&format!("  :grounding_context_id {}\n", lisp_string(id)));
    }
    out.push_str(&format!(
        "  :grounding_sources {}\n",
        lisp_string_vector(grounding_sources)
    ));
    out.push_str(&format!(
        "  :grounding_evidence_refs_count {}\n",
        grounding_evidence_refs_count
    ));
    out.push_str(&format!(
        "  :acceptance {}\n",
        lisp_string_vector(acceptance)
    ));
    out.push_str(&format!("  :objective {}\n", lisp_string(objective)));
    out.push_str("  :accepted_shards\n  (\n");
    for task in planned.iter().filter(|task| !task.write_scope.is_empty()) {
        if let Some(shard_id) = task.accepted_shard_id.as_deref() {
            out.push_str(&format!(
                "    (shard :id {} :lane {} :task_class {} :write_scope {} :shared_claim_ids {} :acceptance {})\n",
                lisp_string(shard_id),
                lisp_string(&task.lane),
                lisp_string(&task.task_class),
                lisp_string_vector(&task.write_scope),
                lisp_string_vector(&task.shared_claim_ids),
                lisp_string_vector(acceptance),
            ));
        }
    }
    out.push_str("  )\n");
    out.push_str("  :tasks\n  (\n");
    for (idx, task) in planned.iter().enumerate() {
        out.push_str(&format!(
            "    (task :index {} :lane {} :engine_hint {} :pool_hint {} :task_class {} :title {} :intent {}\n",
            idx,
            lisp_string(&task.lane),
            lisp_string(&task.engine_hint),
            lisp_string(&task.pool_hint),
            lisp_string(&task.task_class),
            lisp_string(&task.title),
            lisp_string(&task.intent),
        ));
        out.push_str(&format!(
            "          :read_scope {} :write_scope {} :must_not_touch {}",
            lisp_string_vector(&task.read_scope),
            lisp_string_vector(&task.write_scope),
            lisp_string_vector(&task.must_not_touch),
        ));
        if let Some(shard_id) = task.accepted_shard_id.as_deref() {
            out.push_str(&format!(" :accepted_shard_id {}", lisp_string(shard_id)));
        }
        if !task.shared_claim_ids.is_empty() {
            out.push_str(&format!(
                " :shared_claim_ids {}",
                lisp_string_vector(&task.shared_claim_ids)
            ));
        }
        out.push_str(")\n");
    }
    out.push_str("  ))\n");
    out
}

fn lisp_string_vector(values: &[String]) -> String {
    if values.is_empty() {
        return "[]".to_string();
    }
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| lisp_string(value))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn lisp_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
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
    accepted_shard_id: Option<String>,
    shared_claim_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct SwarmTargetProject {
    id: String,
    root: String,
}

impl SwarmTargetProject {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "root": self.root,
        })
    }
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
            "accepted_shard_id": self.accepted_shard_id,
            "shared_claim_ids": self.shared_claim_ids,
        })
    }
}

fn swarm_policy_requires_implement_write_scope(write_policy: &str) -> bool {
    !write_policy.eq_ignore_ascii_case("read-only")
}

fn detect_swarm_write_conflicts(planned: &[SwarmPlannedTask]) -> Vec<Value> {
    let mut conflicts = Vec::new();
    for (left_idx, left) in planned.iter().enumerate() {
        if left.write_scope.is_empty() {
            continue;
        }
        for (right_idx, right) in planned.iter().enumerate().skip(left_idx + 1) {
            if right.write_scope.is_empty() {
                continue;
            }
            for left_path in &left.write_scope {
                for right_path in &right.write_scope {
                    if write_scopes_overlap(left_path, right_path) {
                        conflicts.push(json!({
                            "left_index": left_idx,
                            "left_title": left.title,
                            "left_path": left_path,
                            "right_index": right_idx,
                            "right_title": right.title,
                            "right_path": right_path,
                            "reason": "overlapping write_scope"
                        }));
                    }
                }
            }
        }
    }
    conflicts
}

fn write_scopes_overlap(left: &str, right: &str) -> bool {
    let left = normalize_scope_for_overlap(left);
    let right = normalize_scope_for_overlap(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == "**/*" || right == "**/*" || left == "*" || right == "*" {
        return true;
    }
    if left == right {
        return true;
    }
    let left_prefix = glob_static_prefix(&left);
    let right_prefix = glob_static_prefix(&right);
    if left_prefix.is_empty() || right_prefix.is_empty() {
        return false;
    }
    left_prefix == right_prefix
        || left_prefix.starts_with(&format!("{right_prefix}/"))
        || right_prefix.starts_with(&format!("{left_prefix}/"))
}

fn normalize_scope_for_overlap(scope: &str) -> String {
    scope
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("./")
        .to_string()
}

fn glob_static_prefix(scope: &str) -> String {
    let wildcard = scope
        .char_indices()
        .find_map(|(idx, ch)| matches!(ch, '*' | '?' | '[' | '{').then_some(idx));
    let prefix = wildcard.map(|idx| &scope[..idx]).unwrap_or(scope);
    prefix
        .trim_end_matches('/')
        .trim_end_matches("/**")
        .trim_end_matches("/*")
        .trim_end_matches('/')
        .to_string()
}

fn render_swarm_task_description(
    objective: &str,
    project_id: &str,
    project_root: &str,
    target_projects: &[SwarmTargetProject],
    context_pack_path: &str,
    parent_id: Option<&str>,
    write_policy: &str,
    acceptance: &[String],
    planned: &SwarmPlannedTask,
    grounding_context_id: Option<&str>,
    grounding_sources: &[String],
    grounding_evidence_refs_count: usize,
) -> String {
    let task_write_policy = swarm_task_effective_write_policy(write_policy, planned);
    let (interaction_preamble, completion_protocol) = if task_write_policy == "read-only" {
        (
            "请审视这个目标和上下文，比较现有 SSOT/代码证据，找出缺口与更优雅的设计空间。不要直接改文件；把发现整理为后续 context-pack。",
            "Completion protocol: this is a read-only investigation lane; do not edit files, do not stage, do not commit. read_scope lists readable evidence; must_not_touch is a write/stage/commit prohibition, not a read ban by itself. Return a structured artifact with Findings / Evidence / Recommendations / Verification in the final summary or BoardTask note; do not paste raw KB JSON/log blobs. The master or integrator compiles the context-pack.",
        )
    } else {
        (
            "基于已接受 shard 和上下文，请完成这个最小同构改动；优先保持现有行为，只在 declared write_scope 内修改。",
            "Completion protocol: this is an implementation lane. You may read declared read_scope, may write only declared write_scope, must not write/stage/commit forbidden paths, must not create internal ClaudeCode Task/TaskCreate/TaskUpdate subagents, and must report acceptance evidence as a structured artifact.",
        )
    };
    let parent_line = parent_id
        .map(|id| format!("- parent_board_task_id: {id}\n"))
        .unwrap_or_default();

    format!(
        "{interaction_preamble}\n\nObjective:\n{objective}\n\n## Swarm metadata\n- project_id: {project_id}\n- project_root: {project_root}\n- target_projects: {}\n{parent_line}- lane: {}\n- task_class: {}\n- pool_hint: {}\n- engine_hint: {}\n- context_pack_path: {context_pack_path}\n- grounding_context_id: {}\n- grounding_sources: {}\n- grounding_evidence_refs_count: {}\n- accepted_shard_id: {}\n- shared_claim_ids: {}\n- write_policy: {task_write_policy}\n- read_scope: {}\n- write_scope: {}\n- must_not_touch: {}\n- acceptance: {}\n\n{}",
        render_target_projects_inline(target_projects),
        planned.lane,
        planned.task_class,
        planned.pool_hint,
        planned.engine_hint,
        grounding_context_id.unwrap_or("-"),
        if grounding_sources.is_empty() {
            "[]".to_string()
        } else {
            grounding_sources.join(", ")
        },
        grounding_evidence_refs_count,
        planned.accepted_shard_id.as_deref().unwrap_or("-"),
        if planned.shared_claim_ids.is_empty() {
            "[]".to_string()
        } else {
            planned.shared_claim_ids.join(", ")
        },
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

fn swarm_task_runtime_metadata(
    project_id: &str,
    project_root: &str,
    target_projects: &[SwarmTargetProject],
    context_pack_path: &str,
    parent_id: Option<&str>,
    write_policy: &str,
    acceptance: &[String],
    planned: &SwarmPlannedTask,
    grounding_context_id: Option<&str>,
    grounding_sources: &[String],
    grounding_evidence_refs_count: usize,
) -> Value {
    json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "mission_swarm_run",
        "swarm_metadata": {
            "project_id": project_id,
            "project_root": project_root,
            "target_projects": target_projects.iter().map(SwarmTargetProject::to_json).collect::<Vec<_>>(),
            "parent_board_task_id": parent_id,
            "lane": planned.lane,
            "task_class": planned.task_class,
            "pool_hint": planned.pool_hint,
            "engine_hint": planned.engine_hint,
            "context_pack_path": context_pack_path,
            "grounding_context_id": grounding_context_id,
            "grounding_sources": grounding_sources,
            "grounding_evidence_refs_count": grounding_evidence_refs_count,
            "accepted_shard_id": planned.accepted_shard_id,
            "shared_claim_ids": planned.shared_claim_ids,
            "write_policy": swarm_task_effective_write_policy(write_policy, planned),
            "read_scope": planned.read_scope,
            "write_scope": planned.write_scope,
            "must_not_touch": planned.must_not_touch,
            "acceptance": acceptance,
            "authority": {
                "control_state": "task_contracts",
                "description": "prompt_projection"
            }
        }
    })
}

fn swarm_task_effective_write_policy<'a>(
    global_write_policy: &'a str,
    planned: &SwarmPlannedTask,
) -> &'a str {
    if planned.write_scope.is_empty() {
        "read-only"
    } else {
        global_write_policy
    }
}

fn planned_task_primary_project(
    fallback_project_id: &str,
    fallback_project_root: &str,
    target_projects: &[SwarmTargetProject],
    planned: &SwarmPlannedTask,
) -> (String, String) {
    let mut matches = Vec::<&SwarmTargetProject>::new();
    for target in target_projects {
        let root = target.root.trim_end_matches('/');
        let in_read_scope = planned
            .read_scope
            .iter()
            .any(|path| path.trim_end_matches('/') == root);
        let in_write_scope = planned
            .write_scope
            .iter()
            .any(|path| path == &target.root || path.starts_with(&format!("{}/", root)));
        if in_read_scope || in_write_scope {
            matches.push(target);
        }
    }
    matches.dedup_by(|left, right| left.id == right.id);
    if matches.len() == 1 {
        (matches[0].id.clone(), matches[0].root.clone())
    } else {
        (
            fallback_project_id.to_string(),
            fallback_project_root.to_string(),
        )
    }
}

fn render_target_projects_inline(target_projects: &[SwarmTargetProject]) -> String {
    if target_projects.is_empty() {
        return "[]".to_string();
    }
    target_projects
        .iter()
        .map(|target| format!("{}={}", target.id, target.root))
        .collect::<Vec<_>>()
        .join(", ")
}

async fn resolve_swarm_target_projects(
    state: &AppState,
    project_id: &str,
    project_root: &str,
    target_project_ids: &[String],
) -> Result<Vec<SwarmTargetProject>> {
    if target_project_ids.is_empty() {
        return Ok(vec![SwarmTargetProject {
            id: project_id.to_string(),
            root: project_root.to_string(),
        }]);
    }

    let mut targets = Vec::new();
    for target_id in target_project_ids {
        let resolution = crate::slot_orchestrator::project_root::resolve_target_project_root(
            Some(target_id),
            None,
            None,
            &state.project_registry,
        )
        .await
        .map_err(|err| anyhow!("{}: {}", target_id, err))?;
        let target = SwarmTargetProject {
            id: target_id.clone(),
            root: resolution.project_root.to_string_lossy().to_string(),
        };
        if !targets
            .iter()
            .any(|existing: &SwarmTargetProject| existing.id == target.id)
        {
            targets.push(target);
        }
    }
    Ok(targets)
}

fn append_unique_strings(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

fn swarm_read_scope_for_worker(
    worker_index: usize,
    worker_count: usize,
    full_read_scope: &[String],
    target_projects: &[SwarmTargetProject],
    split_targets: bool,
) -> Vec<String> {
    if !split_targets || target_projects.is_empty() || worker_count <= 1 {
        return full_read_scope.to_vec();
    }

    let target_roots = target_projects
        .iter()
        .map(|target| target.root.as_str())
        .collect::<Vec<_>>();
    let mut read_scope = Vec::new();
    for (target_index, target) in target_projects.iter().enumerate() {
        if target_index % worker_count == worker_index {
            read_scope.push(target.root.clone());
        }
    }
    if read_scope.is_empty() {
        read_scope.extend(target_projects.iter().map(|target| target.root.clone()));
    }
    for path in full_read_scope {
        if !target_roots.iter().any(|root| root == &path.as_str())
            && !read_scope.iter().any(|existing| existing == path)
        {
            read_scope.push(path.clone());
        }
    }
    read_scope
}

#[derive(Debug, Clone, Default)]
struct DelegationMetadata {
    task_class: Option<String>,
    pool_hint: Option<String>,
    engine_hint: Option<String>,
    context_pack_path: Option<String>,
    accepted_shard_id: Option<String>,
    /// Paths the worker is explicitly allowed (and expected) to READ. Distinct
    /// from `write_scope` / `must_not_touch`: review-class tasks ship with a
    /// non-empty `read_scope` and an empty `write_scope`, making the
    /// read-only-but-must-investigate contract explicit in the worker prompt.
    read_scope: Vec<String>,
    write_scope: Vec<String>,
    must_not_touch: Vec<String>,
    acceptance: Vec<String>,
    grounding_context_id: Option<String>,
    grounding_sources: Vec<String>,
    grounding_evidence_refs_count: usize,
    shared_claim_ids: Vec<String>,
    /// Upstream BoardTask whose objective spawned this delegation. Distinct
    /// from `parent_id`: callers may chain through several master/plan layers
    /// before reaching `mission_task_delegate`, and `source_id` keeps the
    /// original anchor visible so dedup can refuse a second concurrent code
    /// worker even when the immediate parent shifts.
    source_id: Option<String>,
}

fn delegation_runtime_metadata(
    metadata: &DelegationMetadata,
    project_id: Option<&str>,
    project_root: Option<&str>,
    parent_id: Option<&str>,
) -> Value {
    json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "mission_task_delegate",
        "dispatch_metadata": {
            "task_class": metadata.task_class,
            "pool_hint": metadata.pool_hint,
            "engine_hint": metadata.engine_hint,
            "context_pack_path": metadata.context_pack_path,
            "accepted_shard_id": metadata.accepted_shard_id,
            "read_scope": metadata.read_scope,
            "write_scope": metadata.write_scope,
            "must_not_touch": metadata.must_not_touch,
            "acceptance": metadata.acceptance,
            "grounding_context_id": metadata.grounding_context_id,
            "grounding_sources": metadata.grounding_sources,
            "grounding_evidence_refs_count": metadata.grounding_evidence_refs_count,
            "shared_claim_ids": metadata.shared_claim_ids,
            "source_board_task_id": metadata.source_id,
            "parent_board_task_id": parent_id,
            "project_id": project_id,
            "project_root": project_root,
            "authority": {
                "control_state": "task_contracts",
                "description": "prompt_projection"
            }
        }
    })
}

fn sandbox_profile_for_worker(engine_hint: Option<&str>, write_enabled: bool) -> &'static str {
    let engine = engine_hint.unwrap_or("claude-code").to_ascii_lowercase();
    if !write_enabled {
        return "read-only";
    }
    if engine.contains("codex") {
        "workspace-write"
    } else if engine.contains("gemini") {
        "plan-policy-write"
    } else if engine.contains("agy") || engine.contains("xjpcode") {
        "unsupported-write"
    } else {
        "workspace-write-policy"
    }
}

fn enrich_runtime_metadata_with_control_facts(
    mut metadata: Value,
    task_id: &str,
    capability_grant_ids: &[String],
    sandbox_profile: &str,
) -> Value {
    metadata["task_contract_id"] = json!(format!("board-task:{task_id}"));
    metadata["capability_grant_ids"] = json!(capability_grant_ids);
    metadata["sandbox_profile"] = json!(sandbox_profile);
    if let Some(dispatch) = metadata
        .get_mut("dispatch_metadata")
        .and_then(Value::as_object_mut)
    {
        dispatch.insert(
            "task_contract_id".to_string(),
            json!(format!("board-task:{task_id}")),
        );
        dispatch.insert(
            "capability_grant_ids".to_string(),
            json!(capability_grant_ids),
        );
        dispatch.insert("sandbox_profile".to_string(), json!(sandbox_profile));
    }
    metadata
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MechanicMode {
    DryRun,
    Repair,
}

impl MechanicMode {
    fn as_str(&self) -> &'static str {
        match self {
            MechanicMode::DryRun => "dry-run",
            MechanicMode::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone)]
struct MechanicRunConfig {
    mode: MechanicMode,
    target: String,
    bin: String,
    max_turns: Option<i64>,
    model: Option<String>,
}

impl MechanicRunConfig {
    fn to_json(&self) -> Value {
        json!({
            "mode": self.mode.as_str(),
            "target": self.target,
            "bin": self.bin,
            "max_turns": self.max_turns,
            "model": self.model,
        })
    }
}

#[derive(Debug, Clone)]
struct MechanicRepairRun {
    task_id: String,
    project_id: Option<String>,
    project_root: String,
    objective: String,
    metadata: DelegationMetadata,
    capability_grant_ids: Vec<String>,
    write_task_grant_id: Option<String>,
    settle_task_grant_id: Option<String>,
    claim_task_grant_id: Option<String>,
    attempt_id: String,
    config: MechanicRunConfig,
    timeout_secs: i64,
}

#[derive(Debug, Clone)]
struct XjpcodeWorkerRun {
    task_id: String,
    endpoint: String,
    project_id: String,
    project_root: Option<String>,
    objective: String,
    metadata: DelegationMetadata,
    capability_grant_ids: Vec<String>,
    write_task_grant_id: Option<String>,
    settle_task_grant_id: Option<String>,
    timeout_secs: i64,
}

fn engine_hint_is_mechanic(metadata: &DelegationMetadata) -> bool {
    matches!(
        metadata.engine_hint.as_deref(),
        Some("mechanic") | Some("jarvis-mechanic")
    ) || matches!(
        metadata.pool_hint.as_deref(),
        Some("mechanic") | Some("jarvis-mechanic")
    )
}

fn engine_hint_is_xjpcode(engine_hint: Option<&str>) -> bool {
    engine_hint
        .map(|hint| {
            let hint = hint.trim().to_ascii_lowercase();
            hint == "xjpcode" || hint == "xjpcode-readonly-worker"
        })
        .unwrap_or(false)
}

fn xjpcode_worker_endpoint_from_env() -> Option<String> {
    std::env::var("MISSIOND_XJPCODE_WORKER_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .map(xjpcode_worker_endpoint_normalize)
}

fn xjpcode_worker_endpoint_normalize(base: String) -> String {
    if base.ends_with("/worker/v1/work-orders") {
        base
    } else {
        format!("{base}/worker/v1/work-orders")
    }
}

fn xjpcode_worker_endpoint_from_args(args: &Value) -> Result<Option<String>, ToolResult> {
    let Some(raw) = string_arg(args, &["xjpcode_worker_url", "xjpcodeWorkerUrl"]) else {
        return Ok(None);
    };
    let value = raw.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return Ok(None);
    }
    let lower = value.to_ascii_lowercase();
    let is_loopback = lower.starts_with("http://127.0.0.1:")
        || lower.starts_with("http://localhost:")
        || lower == "http://127.0.0.1"
        || lower == "http://localhost";
    let remote_override_allowed = std::env::var("MISSIOND_XJPCODE_ALLOW_REMOTE_ARG")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    if !is_loopback && !remote_override_allowed {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "XJPCODE_WORKER_URL_UNTRUSTED",
                "mission_task_delegate refused xjpcode_worker_url because only loopback dev/smoke overrides are accepted",
            )
            .with_suggestion(
                "configure remote xjpcode workers through MISSIOND_XJPCODE_WORKER_URL in deploy/launchd, or pass http://127.0.0.1:<port> for local smoke",
            ),
        ));
    }
    Ok(Some(xjpcode_worker_endpoint_normalize(value)))
}

fn xjpcode_worker_endpoint_from_args_or_env(args: &Value) -> Result<Option<String>, ToolResult> {
    match xjpcode_worker_endpoint_from_args(args)? {
        Some(endpoint) => Ok(Some(endpoint)),
        None => Ok(xjpcode_worker_endpoint_from_env()),
    }
}

fn parse_xjpcode_sse_frames(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let data = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
            if data.is_empty() || data.starts_with("event:") || data == "[DONE]" {
                return None;
            }
            serde_json::from_str::<Value>(data).ok()
        })
        .collect()
}

fn xjpcode_artifact_from_frames(frames: &[Value]) -> Option<Value> {
    frames.iter().find_map(|frame| {
        (frame.get("type").and_then(Value::as_str) == Some("task_result_artifact"))
            .then(|| frame.get("artifact").cloned())
            .flatten()
    })
}

fn xjpcode_final_status_from_frames(frames: &[Value]) -> Option<String> {
    frames.iter().rev().find_map(|frame| {
        (frame.get("type").and_then(Value::as_str) == Some("final"))
            .then(|| {
                frame
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .flatten()
    })
}

fn xjpcode_result_status_for_artifact(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "done" | "completed" | "complete" | "success" | "succeeded" => "completed",
        "blocked" => "blocked",
        "skipped" => "skipped",
        _ => "failed",
    }
}

fn xjpcode_status_for_worker_settle(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "done" | "completed" | "complete" | "success" | "succeeded" => "done",
        "blocked" => "blocked",
        "skipped" => "skipped",
        _ => "failed",
    }
}

fn task_write_grant_id(metadata: &DelegationMetadata, grant_ids: &[String]) -> Option<String> {
    let index = metadata.read_scope.len() + metadata.write_scope.len();
    grant_ids.get(index).cloned()
}

fn task_settle_grant_id(metadata: &DelegationMetadata, grant_ids: &[String]) -> Option<String> {
    let index = metadata.read_scope.len() + metadata.write_scope.len() + 1;
    grant_ids.get(index).cloned()
}

fn task_claim_grant_id(metadata: &DelegationMetadata, grant_ids: &[String]) -> Option<String> {
    let index = metadata.read_scope.len() + metadata.write_scope.len() + 2;
    grant_ids.get(index).cloned()
}

fn parse_mechanic_run_config(
    args: &Value,
    metadata: &DelegationMetadata,
) -> std::result::Result<Option<MechanicRunConfig>, ToolResult> {
    if !engine_hint_is_mechanic(metadata) {
        return Ok(None);
    }
    let mode = match string_arg(args, &["mechanic_mode", "mechanicMode"]).unwrap_or("dry-run") {
        "dry-run" | "dry_run" | "diagnostic" | "diagnostics" => MechanicMode::DryRun,
        "repair" => MechanicMode::Repair,
        other => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "MECHANIC_MODE_INVALID",
                    format!("unsupported mechanic_mode `{other}`; expected dry-run or repair"),
                )
                .with_suggestion(
                    "use mechanic_mode=dry-run for diagnostics or mechanic_mode=repair for an approved exact repair shard",
                ),
            ));
        }
    };
    let target = string_arg(args, &["mechanic_target", "mechanicTarget"])
        .or(metadata.accepted_shard_id.as_deref())
        .map(str::to_string)
        .ok_or_else(|| {
            ToolResult::structured_error(
                ToolError::new(
                    "MECHANIC_TARGET_REQUIRED",
                    "engine_hint=mechanic requires mechanic_target or accepted_shard_id",
                )
                .with_suggestion(
                    "compile an accepted exact shard first, then pass accepted_shard_id or mechanic_target",
                ),
            )
        })?;
    let bin = string_arg(args, &["mechanic_bin", "mechanicBin"])
        .map(str::to_string)
        .or_else(|| std::env::var("MISSIOND_MECHANIC_BIN").ok())
        .unwrap_or_else(|| "mechanic".to_string());
    let max_turns = args
        .get("mechanic_max_turns")
        .or_else(|| args.get("mechanicMaxTurns"))
        .and_then(Value::as_i64)
        .filter(|value| *value > 0);
    let model = string_arg(args, &["mechanic_model", "mechanicModel"]).map(str::to_string);
    Ok(Some(MechanicRunConfig {
        mode,
        target,
        bin,
        max_turns,
        model,
    }))
}

fn exact_shard_contract_error(intent: &str, metadata: &DelegationMetadata) -> Option<ToolResult> {
    if !exact_shard_contract_required(intent, metadata) {
        return None;
    }
    if metadata
        .context_pack_path
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return Some(ToolResult::structured_error(
            ToolError::new(
                "EXACT_SHARD_CONTEXT_PACK_REQUIRED",
                "mission_task_delegate refused an implementation worker without context_pack_path",
            )
            .with_suggestion(
                "run investigation/synthesis first and pass the materialized context_pack_path with the accepted exact shard",
            ),
        ));
    }
    if metadata
        .accepted_shard_id
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return Some(ToolResult::structured_error(
            ToolError::new(
                "EXACT_SHARD_ID_REQUIRED",
                "mission_task_delegate refused an implementation worker without accepted_shard_id",
            )
            .with_suggestion(
                "compile accepted_shards in the context pack, then pass accepted_shard_id/acceptedShardId for this code worker",
            ),
        ));
    }
    if metadata.write_scope.is_empty() {
        return Some(ToolResult::structured_error(
            ToolError::new(
                "EXACT_SHARD_WRITE_SCOPE_REQUIRED",
                "mission_task_delegate refused an implementation worker without write_scope",
            )
            .with_suggestion(
                "compile an accepted exact shard with a declared write_scope before dispatching an implementation worker",
            ),
        ));
    }
    None
}

fn exact_shard_contract_required(intent: &str, metadata: &DelegationMetadata) -> bool {
    let is_implementation_class = matches!(
        metadata.task_class.as_deref(),
        Some("code") | Some("implementation") | Some("implement") | Some("implementer")
    );
    let is_code_or_implementation = intent == "code" || is_implementation_class;
    let mechanic_implementation = engine_hint_is_mechanic(metadata) && is_code_or_implementation;
    mechanic_implementation || is_code_or_implementation
}

fn code_worker_requires_write_lease(intent: &str, metadata: &DelegationMetadata) -> bool {
    let is_implementation_class = matches!(
        metadata.task_class.as_deref(),
        Some("code") | Some("implementation") | Some("implement") | Some("implementer")
    );
    (intent == "code" || is_implementation_class) && !metadata.write_scope.is_empty()
}

fn exact_shard_ready(metadata: &DelegationMetadata) -> bool {
    metadata
        .context_pack_path
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        && metadata
            .accepted_shard_id
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        && !metadata.write_scope.is_empty()
}

fn dispatch_grounding_required(metadata: &DelegationMetadata, emergency_code_first: bool) -> bool {
    if emergency_code_first || exact_shard_ready(metadata) {
        return false;
    }
    metadata
        .grounding_context_id
        .as_deref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
}

async fn gather_dispatch_grounding(
    state: &AppState,
    args: &Value,
    objective: &str,
    project_id: Option<&str>,
    source_id: Option<&str>,
) -> Result<std::result::Result<Value, ToolResult>> {
    let unknowns = args.get("unknowns").cloned().unwrap_or_else(|| {
        json!([
            "What project, SSOT, skill, memory, infra, deploy, or tool facts are required before dispatching this objective?"
        ])
    });
    let result = crate::handlers::knowledge::context_gather::handle(
        state,
        "mission_context_gather",
        json!({
            "query": objective,
            "project_id": project_id,
            "source_id": source_id,
            "unknowns": unknowns,
            "persist": true,
        }),
    )
    .await?;
    if result.is_error.unwrap_or(false) {
        return Ok(Err(ToolResult::structured_error(
            ToolError::new(
                "GROUNDING_REQUIRED",
                "worker dispatch refused because mission_context_gather returned an error",
            )
            .with_suggestion(
                "fix the grounding source diagnostic or pass exact_shard_ready fields: accepted_shard_id, context_pack_path, and write_scope",
            ),
        )));
    }
    let Some(value) = tool_result_json(&result) else {
        return Ok(Err(ToolResult::structured_error(
            ToolError::new(
                "GROUNDING_REQUIRED",
                "worker dispatch refused because mission_context_gather did not return JSON",
            )
            .with_suggestion("inspect mission_context_gather and retry with explicit unknowns"),
        )));
    };
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Ok(Err(ToolResult::structured_error(
            ToolError::new(
                "GROUNDING_REQUIRED",
                format!(
                    "worker dispatch refused because grounding diagnostics were returned: {}",
                    value
                        .get("diagnostics")
                        .cloned()
                        .unwrap_or(Value::Null)
                ),
            )
            .with_suggestion(
                "resolve source-specific diagnostics before dispatching a worker; do not let the worker guess missing context",
            ),
        )));
    }
    if value
        .get("grounding_context_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Ok(Err(ToolResult::structured_error(
            ToolError::new(
                "GROUNDING_REQUIRED",
                "worker dispatch refused because persisted grounding did not produce grounding_context_id",
            )
            .with_suggestion("rerun mission_context_gather with persist=true"),
        )));
    }
    Ok(Ok(value))
}

fn apply_grounding_to_metadata(metadata: &mut DelegationMetadata, grounding: &Value) {
    if metadata.grounding_context_id.is_none() {
        metadata.grounding_context_id = grounding
            .get("grounding_context_id")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    if metadata.context_pack_path.is_none() {
        metadata.context_pack_path = grounding
            .get("context_pack_path")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    if metadata.grounding_sources.is_empty() {
        metadata.grounding_sources = grounding
            .get("sources_used")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
    }
    if metadata.grounding_evidence_refs_count == 0 {
        metadata.grounding_evidence_refs_count = grounding
            .get("evidence_refs")
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0);
    }
}

fn tool_result_json(result: &ToolResult) -> Option<Value> {
    let Some(ToolContent::Text { text }) = result.content.first() else {
        return None;
    };
    serde_json::from_str(text).ok()
}

/// Inputs for the duplicate-code-worker dedup guard. Built from the parsed
/// `mission_task_delegate` args before slot reservation; the guard refuses
/// to spawn a second concurrent code worker when an active BoardTask shares
/// the same semantic parent/source AND declares an overlapping write_scope.
#[derive(Debug, Clone, Copy)]
struct DuplicateCodeCheck<'a> {
    parent_id: Option<&'a str>,
    source_id: Option<&'a str>,
    project_id: Option<&'a str>,
    intent: &'a str,
    task_class: Option<&'a str>,
    write_scope: &'a [String],
}

#[derive(Debug, Clone)]
struct DuplicateCodeWorker {
    task_id: String,
    title: String,
    status: String,
    /// Pairs of (requested_path, existing_path) that triggered the overlap.
    overlap: Vec<(String, String)>,
    /// Why the candidate was bound to the same chain — `parent`, `source`, or
    /// `parent+source`. Surfaces the linkage the guard used so callers can
    /// reason about whether the dedup decision is what they intended.
    linkage: String,
}

/// True iff this delegation is a code-class request that declares a write
/// scope. Read-only / context-pack / research delegations short-circuit out
/// because they cannot collide with another worker on the same files.
fn dedup_applies(check: &DuplicateCodeCheck<'_>) -> bool {
    let is_code = check.intent == "code" || matches!(check.task_class, Some("code"));
    is_code && !check.write_scope.is_empty()
}

/// Scan active BoardTasks (`open` / `running` / `verifying`) for one that
/// shares the same parent OR source and has at least one overlapping
/// write_scope path. Returns `None` for read-only callers, callers without
/// a parent/source linkage, or when no overlap is found.
async fn find_overlapping_active_code_worker(
    state: &AppState,
    check: &DuplicateCodeCheck<'_>,
) -> Result<Option<DuplicateCodeWorker>> {
    if !dedup_applies(check) {
        return Ok(None);
    }
    if check.parent_id.is_none() && check.source_id.is_none() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    for status in ["open", "running", "verifying"] {
        match state.store.list_board_tasks(Some(status), true).await {
            Ok(tasks) => candidates.extend(tasks),
            Err(err) => {
                tracing::warn!(
                    status = status,
                    error = %err,
                    "task_delegate dedup: list_board_tasks failed; skipping status"
                );
            }
        }
    }

    for task in candidates {
        let contract = state
            .storage()
            .shared_memory
            .task_runtime_contract(task.id.as_str())
            .await?;
        let parent_match = match check.parent_id {
            Some(parent_id) => {
                task.parent_id
                    .as_ref()
                    .is_some_and(|candidate| candidate.as_ref() == parent_id)
                    || task_contract_references_parent(&contract, parent_id)
            }
            None => false,
        };
        let source_match = match check.source_id {
            Some(src) => task_contract_references_source(&contract, src),
            None => false,
        };
        if !(parent_match || source_match) {
            continue;
        }
        let candidate_scope = contract.write_scope.clone();
        if candidate_scope.is_empty() {
            continue;
        }
        if !same_project_or_absolute_scope(
            check.project_id,
            task.project.as_deref(),
            check.write_scope,
            &candidate_scope,
        ) {
            continue;
        }
        let overlap = compute_scope_overlap(check.write_scope, &candidate_scope);
        if overlap.is_empty() {
            continue;
        }
        let linkage = match (parent_match, source_match) {
            (true, true) => "parent+source",
            (true, false) => "parent",
            (false, true) => "source",
            _ => "unknown",
        };
        return Ok(Some(DuplicateCodeWorker {
            task_id: task.id.to_string(),
            title: task.title.clone(),
            status: task.status.as_str().to_string(),
            overlap,
            linkage: linkage.to_string(),
        }));
    }
    Ok(None)
}

/// Relative write scopes only collide inside the same registered project. A
/// parent swarm may legitimately run several project-local `.missiond/check.sh`
/// edits in parallel; treating those relative paths as global would serialize
/// unrelated repos. Absolute scopes remain globally comparable because they
/// already carry their project root.
fn same_project_or_absolute_scope(
    requested_project: Option<&str>,
    candidate_project: Option<&str>,
    requested: &[String],
    existing: &[String],
) -> bool {
    match (requested_project, candidate_project) {
        (Some(left), Some(right))
            if left != right
                && requested.iter().all(|scope| !scope_is_absolute(scope))
                && existing.iter().all(|scope| !scope_is_absolute(scope)) =>
        {
            false
        }
        _ => true,
    }
}

fn scope_is_absolute(scope: &str) -> bool {
    scope.starts_with('/') || scope.starts_with("~/")
}

#[cfg(test)]
fn metadata_string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "[]")
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
fn board_task_write_scope(task: &missiond_core::types::BoardTask) -> Vec<String> {
    for key in ["write_scope", "writeScope"] {
        let top = metadata_string_list(task.runtime_metadata.get(key));
        if !top.is_empty() {
            return top;
        }
    }
    for nested in ["dispatch_metadata", "swarm_metadata", "metadata"] {
        if let Some(fields) = task.runtime_metadata.get(nested).and_then(Value::as_object) {
            for key in ["write_scope", "writeScope"] {
                let values = metadata_string_list(fields.get(key));
                if !values.is_empty() {
                    return values;
                }
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
fn metadata_string_value(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
fn board_task_references_source(task: &missiond_core::types::BoardTask, source_id: &str) -> bool {
    for key in ["source_board_task_id", "source_id", "parent_board_task_id"] {
        if metadata_string_value(task.runtime_metadata.get(key)).as_deref() == Some(source_id) {
            return true;
        }
    }
    for nested in ["dispatch_metadata", "swarm_metadata", "metadata"] {
        if let Some(fields) = task.runtime_metadata.get(nested).and_then(Value::as_object) {
            for key in ["source_board_task_id", "source_id", "parent_board_task_id"] {
                if metadata_string_value(fields.get(key)).as_deref() == Some(source_id) {
                    return true;
                }
            }
        }
    }
    false
}

fn task_contract_references_source(contract: &TaskRuntimeContract, source_id: &str) -> bool {
    contract
        .source_board_task_id
        .as_deref()
        .is_some_and(|candidate| candidate == source_id)
}

fn task_contract_references_parent(contract: &TaskRuntimeContract, parent_id: &str) -> bool {
    contract
        .parent_board_task_id
        .as_deref()
        .is_some_and(|candidate| candidate == parent_id)
}

/// Cross-product of requested vs existing write scopes; returns every pair
/// that overlaps under the same prefix-matching rule
/// `mission_swarm_run` already uses for its conflict detector.
fn compute_scope_overlap(requested: &[String], existing: &[String]) -> Vec<(String, String)> {
    let mut overlaps = Vec::new();
    for left in requested {
        for right in existing {
            if write_scopes_overlap(left, right) {
                overlaps.push((left.clone(), right.clone()));
            }
        }
    }
    overlaps
}

/// Attach a `note` to the existing active BoardTask describing the refused
/// delegation. The note carries the new objective excerpt + the overlap
/// summary so an operator (or the autopilot) can see what got merged in.
async fn attach_duplicate_delegation_note(
    state: &AppState,
    dup: &DuplicateCodeWorker,
    objective: &str,
    parent_id: Option<&str>,
    source_id: Option<&str>,
    requested_scope: &[String],
) -> bool {
    let preview_end = crate::helpers::char_boundary_at(objective, 600);
    let preview = &objective[..preview_end];
    let overlap_summary = dup
        .overlap
        .iter()
        .map(|(req, exist)| format!("requested `{}` ⇆ existing `{}`", req, exist))
        .collect::<Vec<_>>()
        .join("; ");
    let scope_summary = if requested_scope.is_empty() {
        "[]".to_string()
    } else {
        requested_scope.join(", ")
    };
    let content = format!(
        "🛑 task_delegate refused a duplicate code-worker spawn (linkage={}).\n\
         New objective excerpt:\n{}\n\n\
         Requested write_scope: {}\n\
         Overlap with this task: {}\n\
         Source linkage — parent_id: {} | source_id: {}\n\
         Override hint: rerun mission_task_delegate with allow_duplicate_code_worker=true if this overlap is intentional.",
        dup.linkage,
        preview,
        scope_summary,
        overlap_summary,
        parent_id.unwrap_or("-"),
        source_id.unwrap_or("-"),
    );
    match state
        .store
        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
            task_id: dup.task_id.clone(),
            content,
            note_type: Some("note".to_string()),
            author: Some("task_delegate-dedup".to_string()),
        })
        .await
    {
        Ok(_) => true,
        Err(err) => {
            tracing::warn!(
                task_id = %dup.task_id,
                error = %err,
                "task_delegate dedup: failed to append duplicate-delegation note"
            );
            false
        }
    }
}

/// Build the structured refusal returned to a caller whose code-worker
/// delegation got blocked. Mirrors the `ToolError` JSON shape so existing
/// dashboards can keep parsing `error_code`/`reason`/`suggestion`, then
/// layers extra fields (`existing_task_*`, `overlap`, `note_attached`) so
/// the caller can audit what the guard saw and decide whether to override.
fn build_duplicate_code_worker_refusal(
    dup: &DuplicateCodeWorker,
    parent_id: Option<&str>,
    source_id: Option<&str>,
    note_attached: bool,
) -> ToolResult {
    let reason = format!(
        "active BoardTask {} (`{}`, status={}, linkage={}) already covers an overlapping write_scope; refusing to spawn a second concurrent code worker",
        dup.task_id, dup.title, dup.status, dup.linkage,
    );
    let payload = json!({
        "error_code": "DUPLICATE_CODE_WORKER_BLOCKED",
        "reason": reason,
        "suggestion": "wait for the active task to finish, split the write_scope into disjoint shards, or rerun with allow_duplicate_code_worker=true when the overlap is intentional",
        "existing_task_id": dup.task_id,
        "existing_task_title": dup.title,
        "existing_task_status": dup.status,
        "linkage": dup.linkage,
        "parent_id": parent_id,
        "source_id": source_id,
        "overlap": dup
            .overlap
            .iter()
            .map(|(req, exist)| json!({"requested": req, "existing": exist}))
            .collect::<Vec<_>>(),
        "note_attached": note_attached,
    });
    ToolResult {
        content: vec![missiond_mcp::tools::ToolContent::Text {
            text: serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
        }],
        is_error: Some(true),
    }
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
    sandbox_profile: Option<&str>,
    task_id: Option<&str>,
    capability_grant_ids: Option<&[String]>,
    slot_id: Option<&str>,
) -> Result<String> {
    // Check quota
    let active = state
        .store
        .count_active_dynamic_slots()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let limit = runtime_config.dynamic_slot_limit();
    if active >= limit {
        return Err(anyhow!("Dynamic slot quota full ({}/{})", active, limit));
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
    let create_args = build_compute_slot_create_args(
        template,
        objective,
        ttl,
        cwd,
        model,
        model_profile,
        sandbox_profile,
        task_id,
        capability_grant_ids,
        slot_id,
    );

    // Delegate to existing compute_slot handler
    let result = super::compute_slot::handle(state, "mission_compute_slot", create_args).await?;

    parse_auto_provision_slot_id(&result)
}

fn parse_auto_provision_slot_id(result: &ToolResult) -> Result<String> {
    if let Some(missiond_mcp::tools::ToolContent::Text { text }) = result.content.first() {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if let Some(slot_id) = parsed
                .get("slot_id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Ok(slot_id.to_string());
            }
            if parsed.get("job_id").is_some() {
                return Err(anyhow!(
                    "Slot spawning async did not return slot_id (job_id: {}); cannot bind BoardTask to dynamic slot",
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

fn new_dynamic_slot_id() -> String {
    let short_id = &uuid::Uuid::new_v4().to_string()[..8];
    format!("slot-dyn-{}", short_id)
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

fn optional_usize_arg(args: &Value, keys: &[&str]) -> Option<usize> {
    keys.iter()
        .find_map(|key| args.get(*key))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
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

/// V3 swarm-dispatch-policy :: external-project context-pack projection.
///
/// Context packs produced by MissionD live under the MissionD workspace, while
/// external-project workers run with cwd set to the target project root. A
/// relative `.missiond/...` path would therefore point at the wrong project.
/// Render worker-facing context_pack_path as an absolute MissionD path unless
/// the caller already provided an absolute path.
fn normalize_context_pack_path_for_worker(path: &str, missiond_root: Option<&Path>) -> String {
    let trimmed = path.trim();
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return trimmed.to_string();
    }
    let root = missiond_root
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    root.join(candidate).to_string_lossy().to_string()
}

fn default_swarm_context_pack_path(missiond_root: Option<&Path>) -> String {
    let now = chrono::Utc::now();
    let sequence = SWARM_CONTEXT_PACK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let rel = format!(
        ".missiond/v3/runtime/swarm/{}-{:09}-{:06}-context-pack.lisp",
        now.format("%Y%m%dT%H%M%SZ"),
        now.timestamp_subsec_nanos(),
        sequence
    );
    normalize_context_pack_path_for_worker(&rel, missiond_root)
}

/// V3 resident-master-control :: master-delegation projection.
///
/// `mission_task_delegate` is the common BoardTask entry used by Codex master,
/// context-pack-run-wave, and direct MCP callers. Metadata is kept visible in
/// the durable BoardTask description only as a prompt projection; runtime
/// control reads canonical `task_contracts`.
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
    if let Some(value) = &metadata.accepted_shard_id {
        lines.push(format!("- accepted_shard_id: {}", value));
    }
    if let Some(value) = &metadata.grounding_context_id {
        lines.push(format!("- grounding_context_id: {}", value));
    }
    if !metadata.grounding_sources.is_empty() {
        lines.push(format!(
            "- grounding_sources: {}",
            metadata.grounding_sources.join(", ")
        ));
    }
    if metadata.grounding_evidence_refs_count > 0 {
        lines.push(format!(
            "- grounding_evidence_refs_count: {}",
            metadata.grounding_evidence_refs_count
        ));
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
    if !metadata.shared_claim_ids.is_empty() {
        lines.push(format!(
            "- shared_claim_ids: {}",
            metadata.shared_claim_ids.join(", ")
        ));
    }
    if let Some(value) = &metadata.source_id {
        lines.push(format!("- source_board_task_id: {}", value));
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
    if matches!(
        metadata.task_class.as_deref(),
        Some("code") | Some("implementation") | Some("implement") | Some("implementer")
    ) {
        lines.push(
            "- implementation_contract: accepted_shard_id is the only implementation target; do not create internal ClaudeCode Task/TaskCreate/TaskUpdate subagents"
                .to_string(),
        );
    }
    if matches!(metadata.task_class.as_deref(), Some("deploy-ops")) {
        lines.push(
            "- deployment_contract: query deploy-center/provenance first; structured smoke evidence is required; use xjp_build_wait/xjp_deploy_watch or deploy-center events for CI/build waiting; raw gh api polling loops are forbidden; do not mutate production, DNS, Cloudflare, or secrets without explicit approval"
                .to_string(),
        );
    }
    lines.join("\n")
}

fn spawn_xjpcode_readonly_worker(state: AppState, run: XjpcodeWorkerRun) {
    tokio::spawn(async move {
        if let Err(err) = run_xjpcode_readonly_worker(state, run).await {
            tracing::warn!(error = %err, "xjpcode readonly worker dispatch failed");
        }
    });
}

async fn run_xjpcode_readonly_worker(state: AppState, run: XjpcodeWorkerRun) -> Result<()> {
    let endpoint = run.endpoint.clone();
    let read_scope = if run.metadata.read_scope.is_empty() {
        run.project_root
            .as_ref()
            .map(|root| vec![root.clone()])
            .unwrap_or_default()
    } else {
        run.metadata.read_scope.clone()
    };

    let request = json!({
        "schema": "missiond.work-order-request.v1",
        "task_id": run.task_id,
        "project_id": run.project_id,
        "mode": "read_only",
        "objective": run.objective,
        "project_root": run.project_root,
        "context_capsule_lisp": run.metadata.context_pack_path.unwrap_or_default(),
        "accepted_shard_id": run.metadata.accepted_shard_id,
        "read_scope": read_scope,
        "write_scope": [],
        "must_not_touch": run.metadata.must_not_touch,
        "capability_grant_ids": run.capability_grant_ids,
        "tool_policy": {
            "source": "mission_task_delegate",
            "mode": "read_only",
            "scope_authority": "MissionD capability grants"
        },
        "artifact_contract": {
            "schema": "missiond.xjpcode-worker-artifact-contract.v1",
            "requires": ["task-result-artifact", "final"],
            "completion": "task-result-artifact before final"
        },
        "event_sink": "missiond.shared-memory"
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(run.timeout_secs.clamp(30, 7200) as u64))
        .build()?;
    let response = client.post(endpoint.as_str()).json(&request).send().await?;
    let status = response.status();
    let response_body = response.text().await.unwrap_or_default();
    let response_json = serde_json::from_str::<Value>(&response_body).unwrap_or_else(|_| {
        json!({
            "raw_response": response_body
        })
    });
    ControlPlaneKernel::new(&state)
        .record_observation_command(RecordObservationCommand {
            task_id: request["task_id"].as_str().unwrap_or("unknown").to_string(),
            project_id: request["project_id"].as_str().map(str::to_string),
            producer_id: "xjpcode-readonly-worker".to_string(),
            payload: json!({
                "schema": "missiond.xjpcode-worker-observation.v1",
                "status": status.as_u16(),
                "endpoint": endpoint,
                "response": response_json.clone()
            }),
        })
        .await?;
    let frames = parse_xjpcode_sse_frames(&response_body);
    let (task_result, worker_final_status) = if status.is_success() {
        (
            xjpcode_artifact_from_frames(&frames).unwrap_or_else(|| {
                json!({
                    "schema": "xjpcode.task-result-artifact.v1",
                    "task_id": request["task_id"],
                    "project_id": request["project_id"],
                    "status": "failed",
                    "mode": "read_only",
                    "summary": "xjpcode worker response did not contain task_result_artifact.",
                    "files_read": [],
                    "files_changed": [],
                    "commands_run": [],
                    "diagnostics": [{
                        "code": "XJPCODE_ARTIFACT_MISSING",
                        "message": "xjpcode worker response did not contain task_result_artifact."
                    }],
                    "model_usage": null
                })
            }),
            xjpcode_final_status_from_frames(&frames).unwrap_or_else(|| "failed".to_string()),
        )
    } else {
        (
            json!({
                "schema": "xjpcode.task-result-artifact.v1",
                "task_id": request["task_id"],
                "project_id": request["project_id"],
                "status": "failed",
                "mode": "read_only",
                "summary": format!("xjpcode worker HTTP request failed with status {status}."),
                "files_read": [],
                "files_changed": [],
                "commands_run": [],
                "diagnostics": [{
                    "code": "XJPCODE_HTTP_ERROR",
                    "message": format!("xjpcode worker HTTP request failed with status {status}.")
                }],
                "model_usage": null
            }),
            "failed".to_string(),
        )
    };
    let Some(write_grant_id) = run.write_task_grant_id.as_deref() else {
        tracing::warn!(
            task_id = %request["task_id"],
            "xjpcode worker returned artifact but no write task grant was available"
        );
        return Ok(());
    };
    let Some(settle_grant_id) = run.settle_task_grant_id.as_deref() else {
        tracing::warn!(
            task_id = %request["task_id"],
            "xjpcode worker returned artifact but no settle task grant was available"
        );
        return Ok(());
    };
    let summary = task_result
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("xjpcode read-only worker completed.");
    let missiond_result_status = xjpcode_result_status_for_artifact(
        task_result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or(worker_final_status.as_str()),
    );
    let missiond_settle_status = xjpcode_status_for_worker_settle(
        task_result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or(worker_final_status.as_str()),
    );
    let content = task_result
        .get("content")
        .cloned()
        .unwrap_or_else(|| Value::String(summary.to_string()));
    let created_at = chrono::Utc::now().to_rfc3339();
    let task_id = request["task_id"]
        .as_str()
        .ok_or_else(|| anyhow!("xjpcode worker artifact requires task_id"))?;
    let project_id = request["project_id"].as_str().map(str::to_string);
    let artifact = ControlPlaneKernel::new(&state)
        .write_completion_artifact(TaskCompletionEvidenceInput {
            task_id: task_id.to_string(),
            project_id: project_id.clone(),
            slot_id: None,
            conversation_id: None,
            provider: "xjpcode".to_string(),
            result_status: missiond_result_status.to_string(),
            summary: summary.to_string(),
            content: Some(
                content
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| content.to_string()),
            ),
            json: json!({
                "schema": "missiond.xjpcode-worker-result.v1",
                "response": task_result,
                "sse_frames": frames
            }),
            accepted_shard_id: None,
            attempt_id: None,
            capability_grant_id: Some(write_grant_id.to_string()),
            subject_kind: Some("worker".to_string()),
            subject_id: Some("xjpcode-readonly-worker".to_string()),
            confirm: None,
            producer: Some(json!({
                "kind": "portable-worker",
                "id": "xjpcode-readonly-worker",
                "created_at": created_at
            })),
            raw_evidence: Some(json!({
                "kind": "xjpcode-work-order-response",
                "response": task_result,
                "sse_frames": frames
            })),
            evidence_refs: Some(json!([{
                "kind": "xjpcode-work-order-response",
                "task_id": task_id,
                "created_at": created_at
            }])),
            created_at: Some(created_at),
        })
        .await?;
    ControlPlaneKernel::new(&state)
        .settle_task_command(SettleTaskCommand {
            task_id: task_id.to_string(),
            project_id,
            slot_id: Some("xjpcode-readonly-worker".to_string()),
            conversation_id: None,
            artifact_hash: Some(artifact.artifact_hash),
            status: missiond_settle_status.to_string(),
            summary: Some(summary.to_string()),
            grant_id: Some(settle_grant_id.to_string()),
            subject_kind: "worker".to_string(),
            subject_id: "xjpcode-readonly-worker".to_string(),
            attempt_id: None,
            allow_system_bypass: false,
        })
        .await?;
    Ok(())
}

fn spawn_mechanic_repair(state: AppState, run: MechanicRepairRun) {
    // Mechanic is a subprocess executor lane: its final output is normalized
    // into the canonical task-result-artifact via worker_settle. It is not a
    // PTY slot and it never becomes a resident orchestrator.
    tokio::spawn(async move {
        if let Err(err) = ControlPlaneKernel::new(&state)
            .start_attempt_command(StartAttemptCommand {
                task_id: run.task_id.clone(),
                project_id: run.project_id.clone(),
                attempt_id: run.attempt_id.clone(),
                agent_id: "mechanic".to_string(),
                worker_id: "mechanic".to_string(),
                payload: json!({
                    "source": "mission_task_delegate.mechanic",
                    "engine_hint": "mechanic",
                    "mode": run.config.mode.as_str(),
                    "target": run.config.target.as_str(),
                    "capability_grant_ids": run.capability_grant_ids,
                    "accepted_shard_id": run.metadata.accepted_shard_id,
                    "write_scope": run.metadata.write_scope,
                    "must_not_touch": run.metadata.must_not_touch,
                }),
            })
            .await
        {
            tracing::warn!(task_id = %run.task_id, error = %err, "mechanic repair attempt.started event failed");
        }
        let result = run_mechanic_repair_subprocess(&run).await;
        let (status, summary, content) = match result {
            Ok(content) => {
                let exit_code = content
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .unwrap_or(-1);
                let status = if exit_code == 0 { "done" } else { "failed" };
                let summary = if exit_code == 0 {
                    format!(
                        "Mechanic {} completed for accepted shard {}.",
                        run.config.mode.as_str(),
                        run.metadata
                            .accepted_shard_id
                            .as_deref()
                            .unwrap_or(run.config.target.as_str())
                    )
                } else {
                    format!(
                        "Mechanic {} failed for accepted shard {} (exit {}).",
                        run.config.mode.as_str(),
                        run.metadata
                            .accepted_shard_id
                            .as_deref()
                            .unwrap_or(run.config.target.as_str()),
                        exit_code
                    )
                };
                (status, summary, content)
            }
            Err(err) => (
                "failed",
                format!(
                    "Mechanic {} could not start or timed out: {}",
                    run.config.mode.as_str(),
                    err
                ),
                json!({
                    "schema": "missiond.mechanic-repair-result.v1",
                    "ok": false,
                    "error": err.to_string(),
                    "mode": run.config.mode.as_str(),
                    "target": run.config.target,
                    "project_root": run.project_root,
                }),
            ),
        };

        let artifact_hash = if status == "done" {
            let mut artifact_details = match content.clone() {
                Value::Object(map) => Value::Object(map),
                other => json!({ "result": other }),
            };
            let mechanic_exit_code = artifact_details
                .get("exit_code")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if let Some(obj) = artifact_details.as_object_mut() {
                obj.entry("changed_paths".to_string())
                    .or_insert_with(|| json!(run.metadata.write_scope));
                obj.entry("verification".to_string()).or_insert_with(|| {
                    json!({
                        "status": "passed",
                        "source": "mechanic-exit-code",
                        "exit_code": mechanic_exit_code
                    })
                });
            }
            let created_at = chrono::Utc::now().to_rfc3339();
            let artifact_input = TaskCompletionEvidenceInput {
                task_id: run.task_id.clone(),
                project_id: run.project_id.clone(),
                slot_id: Some("mechanic".to_string()),
                conversation_id: None,
                provider: "mechanic".to_string(),
                result_status: "completed".to_string(),
                summary: summary.clone(),
                content: Some(summary.clone()),
                json: artifact_details,
                accepted_shard_id: run.metadata.accepted_shard_id.clone(),
                attempt_id: Some(run.attempt_id.clone()),
                capability_grant_id: run.write_task_grant_id.clone(),
                subject_kind: Some("worker".to_string()),
                subject_id: Some("mechanic".to_string()),
                confirm: None,
                producer: Some(json!({
                    "kind": "mechanic-subprocess",
                    "id": "mechanic",
                    "created_at": created_at
                })),
                raw_evidence: Some(json!({
                    "kind": "mechanic-run",
                    "mode": run.config.mode.as_str(),
                    "target": run.config.target,
                    "project_root": run.project_root
                })),
                evidence_refs: Some(json!([{
                    "kind": "mechanic-run",
                    "task_id": run.task_id,
                    "created_at": created_at
                }])),
                created_at: Some(created_at),
            };
            match ControlPlaneKernel::new(&state)
                .write_completion_artifact(artifact_input)
                .await
            {
                Ok(value) => Some(value.artifact_hash),
                Err(err) => {
                    tracing::warn!(
                        task_id = %run.task_id,
                        error = %err,
                        "mechanic repair task_result_put failed; refusing done settle"
                    );
                    None
                }
            }
        } else {
            None
        };
        if status == "done" && artifact_hash.is_none() {
            for claim_id in &run.metadata.shared_claim_ids {
                let release = ControlPlaneKernel::new(&state)
                    .release_lease_command(ReleaseLeaseCommand {
                        claim_id: claim_id.clone(),
                        owner_id: None,
                        grant_id: None,
                        subject_kind: "daemon".to_string(),
                        subject_id: "mission_task_delegate.mechanic".to_string(),
                        details: json!({
                            "claim_task_grant_id": run.claim_task_grant_id.as_deref()
                        }),
                    })
                    .await;
                if let Err(err) = release {
                    tracing::warn!(task_id = %run.task_id, claim_id = %claim_id, error = %err, "mechanic repair claim release failed");
                }
            }
            return;
        }
        let settle = ControlPlaneKernel::new(&state)
            .settle_task_command(SettleTaskCommand {
                task_id: run.task_id.clone(),
                project_id: run.project_id.clone(),
                slot_id: Some("mechanic".to_string()),
                conversation_id: None,
                artifact_hash,
                status: status.to_string(),
                summary: Some(summary),
                grant_id: run.settle_task_grant_id.clone(),
                subject_kind: "worker".to_string(),
                subject_id: "mechanic".to_string(),
                attempt_id: Some(run.attempt_id.clone()),
                allow_system_bypass: false,
            })
            .await;
        if let Err(err) = settle {
            tracing::warn!(task_id = %run.task_id, error = %err, "mechanic repair worker_settle failed");
        }

        for claim_id in &run.metadata.shared_claim_ids {
            let release = ControlPlaneKernel::new(&state)
                .release_lease_command(ReleaseLeaseCommand {
                    claim_id: claim_id.clone(),
                    owner_id: None,
                    grant_id: None,
                    subject_kind: "daemon".to_string(),
                    subject_id: "mission_task_delegate.mechanic".to_string(),
                    details: json!({
                        "claim_task_grant_id": run.claim_task_grant_id.as_deref()
                    }),
                })
                .await;
            if let Err(err) = release {
                tracing::warn!(task_id = %run.task_id, claim_id = %claim_id, error = %err, "mechanic repair claim release failed");
            }
        }
    });
}

async fn run_mechanic_repair_subprocess(run: &MechanicRepairRun) -> Result<Value> {
    let mut cmd = Command::new(&run.config.bin);
    cmd.arg("repair").arg(&run.project_root);
    if run.config.mode == MechanicMode::DryRun {
        cmd.arg("--dry-run");
    }
    cmd.arg("--target").arg(&run.config.target);
    if let Some(model) = &run.config.model {
        cmd.arg("--model").arg(model);
    }
    if let Some(max_turns) = run.config.max_turns {
        cmd.arg("--max-turns").arg(max_turns.to_string());
    }
    cmd.stdin(std::process::Stdio::null());

    let timeout = std::time::Duration::from_secs(run.timeout_secs.clamp(30, 7200) as u64);
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| anyhow!("mechanic subprocess timed out after {}s", timeout.as_secs()))?
        .map_err(|err| anyhow!("mechanic subprocess spawn failed: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(json!({
        "schema": "missiond.mechanic-repair-result.v1",
        "ok": output.status.success(),
        "exit_code": output.status.code().unwrap_or(-1),
        "mode": run.config.mode.as_str(),
        "target": run.config.target,
        "project_root": run.project_root,
        "objective": run.objective,
        "accepted_shard_id": run.metadata.accepted_shard_id,
        "context_pack_path": run.metadata.context_pack_path,
        "write_scope": run.metadata.write_scope,
        "acceptance": run.metadata.acceptance,
        "stdout_tail": tail_chars(&stdout, 8000),
        "stderr_tail": tail_chars(&stderr, 8000),
    }))
}

fn tail_chars(input: &str, max_chars: usize) -> String {
    let len = input.chars().count();
    if len <= max_chars {
        return input.to_string();
    }
    input.chars().skip(len - max_chars).collect()
}

/// Legacy opt-in helper for KB/Skill context assembly.
#[allow(dead_code)]
async fn build_context(state: &AppState, keywords: &str) -> Result<String> {
    let mut parts = Vec::new();
    let mut total_len = 0;

    // Search KB (Postgres FTS, take first 3)
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
    sandbox_profile: Option<&str>,
    task_id: Option<&str>,
    capability_grant_ids: Option<&[String]>,
    slot_id: Option<&str>,
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
    if let Some(sandbox) = sandbox_profile {
        create_args["sandbox"] = Value::String(sandbox.to_string());
    }
    if let Some(task_id) = task_id {
        create_args["task_id"] = Value::String(task_id.to_string());
    }
    if let Some(grant_ids) = capability_grant_ids {
        create_args["capability_grant_ids"] = json!(grant_ids);
    }
    if let Some(slot_id) = slot_id {
        create_args["slot_id"] = Value::String(slot_id.to_string());
        create_args["subject_kind"] = Value::String("worker".to_string());
        create_args["subject_id"] = Value::String(slot_id.to_string());
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
        let args = build_compute_slot_create_args(
            "coder",
            "ship the fix",
            3600,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
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
        let args = build_compute_slot_create_args(
            "researcher",
            "investigate",
            7200,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(args["action"], json!("create"));
        assert_eq!(args["template"], json!("researcher"));
        assert_eq!(args["objective"], json!("investigate"));
        assert_eq!(args["max_ttl"], json!(7200));
    }

    #[test]
    fn parse_auto_provision_slot_id_accepts_async_job_metadata() {
        let result = ToolResult::job_accepted_with_metadata(
            "job-abc12345",
            "mission_compute_slot:create",
            json!({ "slot_id": "slot-dyn-abc12345" }),
        );
        assert_eq!(
            parse_auto_provision_slot_id(&result).unwrap(),
            "slot-dyn-abc12345"
        );
    }

    #[test]
    fn parse_auto_provision_slot_id_rejects_job_without_slot_id() {
        let result = ToolResult::job_accepted("job-abc12345", "mission_compute_slot:create");
        let err = parse_auto_provision_slot_id(&result)
            .expect_err("slot_id-less async job must not be accepted");
        assert!(
            err.to_string().contains("did not return slot_id"),
            "unexpected error: {err}"
        );
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
            Some("workspace-write"),
            Some("task-1"),
            Some(&["grant-1".to_string(), "grant-2".to_string()]),
            Some("slot-dyn-fixed1"),
        );
        assert_eq!(args["cwd"], json!("/Users/jinchen/Projects/missiond"));
        assert_eq!(args["model"], json!("sonnet"));
        assert_eq!(args["model_profile"], json!("daily-sonnet"));
        assert_eq!(args["sandbox"], json!("workspace-write"));
        assert_eq!(args["task_id"], json!("task-1"));
        assert_eq!(args["capability_grant_ids"], json!(["grant-1", "grant-2"]));
        assert_eq!(args["slot_id"], json!("slot-dyn-fixed1"));
        assert_eq!(args["subject_kind"], json!("worker"));
        assert_eq!(args["subject_id"], json!("slot-dyn-fixed1"));
        assert_eq!(args["suppress_initial_prompt"], json!(true));
    }

    #[test]
    fn create_args_omit_optional_fields_when_absent() {
        let args = build_compute_slot_create_args(
            "coder", "x", 3600, None, None, None, None, None, None, None,
        );
        assert!(args.get("cwd").is_none());
        assert!(args.get("model").is_none());
        assert!(args.get("model_profile").is_none());
    }

    #[test]
    fn xjpcode_engine_hint_detector_is_readonly_only() {
        assert!(engine_hint_is_xjpcode(Some("xjpcode")));
        assert!(engine_hint_is_xjpcode(Some("xjpcode-readonly-worker")));
        assert!(!engine_hint_is_xjpcode(Some("xjpcode-code-worker")));
        assert!(!engine_hint_is_xjpcode(Some("claude-code")));
        assert!(!engine_hint_is_xjpcode(None));
    }

    #[test]
    fn xjpcode_sse_parser_extracts_artifact_and_final() {
        let body = r#"
data: {"type":"accepted","task_id":"t1","project_id":"p","mode":"read_only","at":"now"}

data: {"type":"task_result_artifact","task_id":"t1","artifact":{"schema":"xjpcode.task-result-artifact.v1","task_id":"t1","project_id":"p","status":"done","mode":"read_only","summary":"ok","files_read":[],"files_changed":[],"commands_run":[],"diagnostics":[],"model_usage":null},"at":"now"}

data: {"type":"final","task_id":"t1","status":"done","at":"now"}
"#;
        let frames = parse_xjpcode_sse_frames(body);
        assert_eq!(frames.len(), 3);
        let artifact = xjpcode_artifact_from_frames(&frames).expect("artifact frame");
        assert_eq!(artifact["summary"], json!("ok"));
        assert_eq!(
            xjpcode_final_status_from_frames(&frames).as_deref(),
            Some("done")
        );
        assert_eq!(xjpcode_result_status_for_artifact("done"), "completed");
        assert_eq!(xjpcode_status_for_worker_settle("done"), "done");
        assert_eq!(xjpcode_result_status_for_artifact("blocked"), "blocked");
        assert_eq!(xjpcode_status_for_worker_settle("blocked"), "blocked");
    }

    #[test]
    fn task_capability_grant_ids_follow_delegate_order() {
        let metadata = DelegationMetadata {
            read_scope: vec!["/repo".to_string(), "/docs".to_string()],
            write_scope: vec!["/repo/src".to_string()],
            ..Default::default()
        };
        let grants = vec![
            "read-1".to_string(),
            "read-2".to_string(),
            "write-path".to_string(),
            "write-task".to_string(),
            "settle-task".to_string(),
            "claim-task".to_string(),
            "spawn-task".to_string(),
        ];
        assert_eq!(
            task_write_grant_id(&metadata, &grants).as_deref(),
            Some("write-task")
        );
        assert_eq!(
            task_settle_grant_id(&metadata, &grants).as_deref(),
            Some("settle-task")
        );
        assert_eq!(
            task_claim_grant_id(&metadata, &grants).as_deref(),
            Some("claim-task")
        );
    }

    #[test]
    fn delegation_metadata_block_projects_two_stage_worker_contract() {
        let metadata = DelegationMetadata {
            task_class: Some("context-pack".to_string()),
            pool_hint: Some("claude-code-default".to_string()),
            engine_hint: Some("claude-code".to_string()),
            context_pack_path: Some(".missiond/tasks/wave99/context-pack.lisp".to_string()),
            accepted_shard_id: None,
            read_scope: vec!["crates/missiond-core/src/types/board.rs".to_string()],
            write_scope: vec!["crates/a.rs".to_string()],
            must_not_touch: vec!["packages/**".to_string()],
            acceptance: vec!["cargo test -p missiond-daemon autopilot".to_string()],
            shared_claim_ids: Vec::new(),
            grounding_context_id: Some("context-gather:test".to_string()),
            grounding_sources: vec!["project_registry".to_string()],
            grounding_evidence_refs_count: 1,
            source_id: None,
        };
        let block = render_delegation_metadata_block(&metadata);
        for expected in [
            "- task_class: context-pack",
            "- pool_hint: claude-code-default",
            "- engine_hint: claude-code",
            "- context_pack_path: .missiond/tasks/wave99/context-pack.lisp",
            "- grounding_context_id: context-gather:test",
            "- grounding_sources: project_registry",
            "- grounding_evidence_refs_count: 1",
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
                "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend".to_string(),
                "/Users/jinchen/Projects/missiond".to_string(),
            ],
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
            acceptance: vec!["git status proves no new edits".to_string()],
            ..DelegationMetadata::default()
        };
        let block = render_delegation_metadata_block(&metadata);
        assert!(
            block.contains("- read_scope: /Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend, /Users/jinchen/Projects/missiond"),
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
    fn deploy_ops_intent_and_metadata_are_first_class() {
        assert!(VALID_INTENTS.contains(&"deploy-ops"));
        let metadata = DelegationMetadata {
            task_class: Some("deploy-ops".to_string()),
            engine_hint: Some("claude-code".to_string()),
            pool_hint: Some("claude-code-deploy-ops".to_string()),
            read_scope: vec!["deploy-center provenance".to_string()],
            write_scope: Vec::new(),
            must_not_touch: vec!["production DNS".to_string(), "secrets".to_string()],
            acceptance: vec!["deploy-center provenance queried".to_string()],
            ..DelegationMetadata::default()
        };
        let block = render_delegation_metadata_block(&metadata);
        for expected in [
            "- task_class: deploy-ops",
            "- pool_hint: claude-code-deploy-ops",
            "- engine_hint: claude-code",
            "- deployment_contract: query deploy-center/provenance first",
        ] {
            assert!(block.contains(expected), "missing {expected}: {block}");
        }
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

    #[test]
    fn swarm_write_scope_conflicts_detect_overlap_before_dispatch() {
        let planned = vec![
            SwarmPlannedTask {
                lane: "implement".to_string(),
                engine_hint: "claude-code".to_string(),
                pool_hint: "claude-code-default".to_string(),
                task_class: "code".to_string(),
                title: "Shard A".to_string(),
                intent: "code".to_string(),
                read_scope: vec!["/repo".to_string()],
                write_scope: vec!["src/auth".to_string()],
                must_not_touch: Vec::new(),
                accepted_shard_id: Some("shard-a".to_string()),
                shared_claim_ids: Vec::new(),
            },
            SwarmPlannedTask {
                lane: "implement".to_string(),
                engine_hint: "claude-code".to_string(),
                pool_hint: "claude-code-default".to_string(),
                task_class: "code".to_string(),
                title: "Shard B".to_string(),
                intent: "code".to_string(),
                read_scope: vec!["/repo".to_string()],
                write_scope: vec!["src/auth/routes.rs".to_string()],
                must_not_touch: Vec::new(),
                accepted_shard_id: Some("shard-b".to_string()),
                shared_claim_ids: Vec::new(),
            },
        ];
        let conflicts = detect_swarm_write_conflicts(&planned);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0]["left_title"], json!("Shard A"));
        assert_eq!(conflicts[0]["right_title"], json!("Shard B"));
    }

    #[test]
    fn swarm_write_scope_conflicts_ignore_read_only_lanes() {
        let planned = vec![
            SwarmPlannedTask {
                lane: "investigate".to_string(),
                engine_hint: "gemini".to_string(),
                pool_hint: "gemini-ultra-pro".to_string(),
                task_class: "context-pack".to_string(),
                title: "Survey".to_string(),
                intent: "research".to_string(),
                read_scope: vec!["src/auth".to_string()],
                write_scope: Vec::new(),
                must_not_touch: vec!["**/*".to_string()],
                accepted_shard_id: None,
                shared_claim_ids: Vec::new(),
            },
            SwarmPlannedTask {
                lane: "implement".to_string(),
                engine_hint: "claude-code".to_string(),
                pool_hint: "claude-code-default".to_string(),
                task_class: "code".to_string(),
                title: "Implement".to_string(),
                intent: "code".to_string(),
                read_scope: vec!["src/auth".to_string()],
                write_scope: vec!["src/auth".to_string()],
                must_not_touch: Vec::new(),
                accepted_shard_id: Some("shard-impl".to_string()),
                shared_claim_ids: Vec::new(),
            },
        ];
        assert!(detect_swarm_write_conflicts(&planned).is_empty());
    }

    #[test]
    fn swarm_implement_policy_requires_explicit_write_scope() {
        assert!(!swarm_policy_requires_implement_write_scope("read-only"));
        assert!(!swarm_policy_requires_implement_write_scope("READ-ONLY"));
        assert!(swarm_policy_requires_implement_write_scope("lisp-first"));
        assert!(swarm_policy_requires_implement_write_scope("scoped-write"));
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

    // ── V3 swarm-dispatch-policy :: external-project context-pack projection ─
    //
    // Pins the rule that mission_swarm_run renders the resolved project_root
    // into Swarm metadata so Autopilot/dispatch can spawn provider PTYs under
    // the target project root and workers see the correct read-scope anchor.
    // See `.missiond/v3/missiond-blueprint.lisp ::
    // "mission_swarm_run MUST resolve project_id to a registered project_root"`.

    #[test]
    fn swarm_task_description_includes_resolved_project_root() {
        let planned = SwarmPlannedTask {
            lane: "investigate".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "context-pack".to_string(),
            title: "Survey shards".to_string(),
            intent: "code".to_string(),
            read_scope: vec!["/Users/jin/Projects/jarvis".to_string()],
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
            accepted_shard_id: None,
            shared_claim_ids: Vec::new(),
        };
        let description = render_swarm_task_description(
            "M6 wave",
            "jarvis",
            "/Users/jin/Projects/jarvis",
            &[SwarmTargetProject {
                id: "jarvis".to_string(),
                root: "/Users/jin/Projects/jarvis".to_string(),
            }],
            ".missiond/v3/runtime/swarm/test.lisp",
            None,
            "read-only",
            &[],
            &planned,
            Some("context-gather:test"),
            &["project_registry".to_string()],
            1,
        );
        assert!(
            description.contains("- project_id: jarvis"),
            "swarm metadata missing project_id line:\n{description}"
        );
        assert!(
            description.contains("- project_root: /Users/jin/Projects/jarvis"),
            "swarm metadata missing project_root line — Autopilot/cwd-override depends on it:\n{description}"
        );
        assert!(
            description.contains("- read_scope: /Users/jin/Projects/jarvis"),
            "swarm metadata missing target-project read_scope:\n{description}"
        );
    }

    #[test]
    fn swarm_read_only_lane_keeps_read_only_policy_under_lisp_first_wave() {
        let planned = SwarmPlannedTask {
            lane: "investigate".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "context-pack".to_string(),
            title: "Survey shards".to_string(),
            intent: "code".to_string(),
            read_scope: vec!["/repo/semantic-terminal".to_string()],
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
            accepted_shard_id: None,
            shared_claim_ids: Vec::new(),
        };
        let description = render_swarm_task_description(
            "M7 wave",
            "semantic-terminal",
            "/repo/semantic-terminal",
            &[SwarmTargetProject {
                id: "semantic-terminal".to_string(),
                root: "/repo/semantic-terminal".to_string(),
            }],
            "/missiond/.missiond/v3/runtime/swarm/test.lisp",
            None,
            "lisp-first",
            &[],
            &planned,
            Some("context-gather:test"),
            &["project_registry".to_string()],
            1,
        );
        assert!(
            description.contains("- write_policy: read-only"),
            "read-only/context-pack lane must not inherit lisp-first write permission:\n{description}"
        );
        assert!(
            description.contains("do not edit files, do not stage, do not commit"),
            "read-only lane must render the strict completion protocol:\n{description}"
        );
        assert!(
            description.contains("请审视这个目标和上下文"),
            "read-only lane must start with a heuristic investigation question:\n{description}"
        );
        assert!(
            description.contains("更优雅的设计空间"),
            "read-only lane should ask for architecture/design gaps rather than only command execution:\n{description}"
        );
    }

    #[test]
    fn swarm_single_external_target_projects_child_task_to_target_root() {
        let target = SwarmTargetProject {
            id: "semantic-terminal".to_string(),
            root: "/Users/jinchen/Projects/semantic-terminal".to_string(),
        };
        let planned = SwarmPlannedTask {
            lane: "implement".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "code".to_string(),
            title: "Implement shard".to_string(),
            intent: "code".to_string(),
            read_scope: vec![
                "/Users/jinchen/Projects/semantic-terminal".to_string(),
                "/Users/jinchen/Projects/missiond/scripts/check-project-maturity.mjs".to_string(),
            ],
            write_scope: vec!["/Users/jinchen/Projects/semantic-terminal/.missiond/**".to_string()],
            must_not_touch: Vec::new(),
            accepted_shard_id: Some("shard-semantic-terminal-impl".to_string()),
            shared_claim_ids: Vec::new(),
        };
        let (project_id, project_root) = planned_task_primary_project(
            "missiond",
            "/Users/jinchen/Projects/missiond",
            &[target],
            &planned,
        );
        assert_eq!(project_id, "semantic-terminal");
        assert_eq!(project_root, "/Users/jinchen/Projects/semantic-terminal");
    }

    #[test]
    fn swarm_task_description_includes_multi_project_targets() {
        let planned = SwarmPlannedTask {
            lane: "investigate".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "context-pack".to_string(),
            title: "Survey shards".to_string(),
            intent: "code".to_string(),
            read_scope: vec![
                "/Users/jin/Projects/jarvis".to_string(),
                "/Users/jin/Projects/xjpcode".to_string(),
            ],
            write_scope: Vec::new(),
            must_not_touch: vec!["**/*".to_string()],
            accepted_shard_id: None,
            shared_claim_ids: Vec::new(),
        };
        let targets = vec![
            SwarmTargetProject {
                id: "jarvis".to_string(),
                root: "/Users/jin/Projects/jarvis".to_string(),
            },
            SwarmTargetProject {
                id: "xjpcode".to_string(),
                root: "/Users/jin/Projects/xjpcode".to_string(),
            },
        ];
        let description = render_swarm_task_description(
            "M6 universe wave",
            "missiond",
            "/Users/jin/Projects/missiond",
            &targets,
            ".missiond/v3/runtime/swarm/test.lisp",
            None,
            "read-only",
            &[],
            &planned,
            Some("context-gather:test"),
            &["project_registry".to_string(), "ssot-intent".to_string()],
            2,
        );
        assert!(
            description.contains(
                "- target_projects: jarvis=/Users/jin/Projects/jarvis, xjpcode=/Users/jin/Projects/xjpcode"
            ),
            "multi-project swarm prompt must expose resolved targets:\n{description}"
        );
        assert!(
            description
                .contains("- read_scope: /Users/jin/Projects/jarvis, /Users/jin/Projects/xjpcode"),
            "multi-project swarm prompt must expose all target roots as read_scope:\n{description}"
        );
    }

    #[test]
    fn swarm_read_scope_splits_target_projects_across_workers() {
        let targets = vec![
            SwarmTargetProject {
                id: "jarvis".to_string(),
                root: "/repo/jarvis".to_string(),
            },
            SwarmTargetProject {
                id: "forge".to_string(),
                root: "/repo/forge".to_string(),
            },
            SwarmTargetProject {
                id: "auth".to_string(),
                root: "/repo/auth".to_string(),
            },
            SwarmTargetProject {
                id: "pcea".to_string(),
                root: "/repo/pcea".to_string(),
            },
        ];
        let all = vec![
            "/repo/jarvis".to_string(),
            "/repo/forge".to_string(),
            "/repo/auth".to_string(),
            "/repo/pcea".to_string(),
            "/Users/jinchen/Projects/missiond".to_string(),
        ];
        assert_eq!(
            swarm_read_scope_for_worker(0, 2, &all, &targets, true),
            vec![
                "/repo/jarvis".to_string(),
                "/repo/auth".to_string(),
                "/Users/jinchen/Projects/missiond".to_string(),
            ]
        );
        assert_eq!(
            swarm_read_scope_for_worker(1, 2, &all, &targets, true),
            vec![
                "/repo/forge".to_string(),
                "/repo/pcea".to_string(),
                "/Users/jinchen/Projects/missiond".to_string(),
            ]
        );
        assert_eq!(
            swarm_read_scope_for_worker(0, 2, &all, &targets, false),
            all
        );
    }

    #[test]
    fn swarm_task_description_carries_parent_board_task_id_when_supplied() {
        let planned = SwarmPlannedTask {
            lane: "implement".to_string(),
            engine_hint: "claude-code".to_string(),
            pool_hint: "claude-code-default".to_string(),
            task_class: "code".to_string(),
            title: "Implement shard".to_string(),
            intent: "code".to_string(),
            read_scope: vec!["/repo".to_string()],
            write_scope: vec![".missiond/evidence/m6.md".to_string()],
            must_not_touch: vec!["src/**".to_string()],
            accepted_shard_id: Some("shard-m6-evidence".to_string()),
            shared_claim_ids: Vec::new(),
        };
        let description = render_swarm_task_description(
            "M6 closure",
            "jarvis-forge",
            "/repo",
            &[SwarmTargetProject {
                id: "jarvis-forge".to_string(),
                root: "/repo".to_string(),
            }],
            "/missiond/.missiond/v3/runtime/swarm/context.lisp",
            Some("parent-task-123"),
            "scoped-write",
            &["node scripts/check.js".to_string()],
            &planned,
            Some("context-gather:test"),
            &["project_registry".to_string(), "ssot-intent".to_string()],
            2,
        );
        assert!(
            description.contains("- parent_board_task_id: parent-task-123"),
            "swarm metadata must carry parent id so worker notes and UI hierarchy can close the objective:\n{description}"
        );
        assert!(
            description.contains("基于已接受 shard 和上下文"),
            "implementation lane must use context-prepared prompt style:\n{description}"
        );
        assert!(
            description.contains("this is an implementation lane"),
            "implementation lane must still carry structured runtime completion protocol:\n{description}"
        );
    }

    #[test]
    fn swarm_context_pack_path_is_absolute_for_external_project_workers() {
        let root = std::path::Path::new("/Users/jinchen/Projects/missiond");
        let normalized = normalize_context_pack_path_for_worker(
            ".missiond/v3/runtime/swarm/test-context-pack.lisp",
            Some(root),
        );
        assert_eq!(
            normalized,
            "/Users/jinchen/Projects/missiond/.missiond/v3/runtime/swarm/test-context-pack.lisp"
        );

        let absolute =
            normalize_context_pack_path_for_worker("/tmp/missiond/context-pack.lisp", Some(root));
        assert_eq!(absolute, "/tmp/missiond/context-pack.lisp");
    }

    #[test]
    fn default_swarm_context_pack_path_is_anchored_to_missiond_root() {
        let root = std::path::Path::new("/Users/jinchen/Projects/missiond");
        let path = default_swarm_context_pack_path(Some(root));
        assert!(
            path.starts_with("/Users/jinchen/Projects/missiond/.missiond/v3/runtime/swarm/"),
            "default context-pack path must not depend on daemon cwd: {path}"
        );
        assert!(
            !path.starts_with("/.missiond/"),
            "launchd cwd must never leak into context-pack paths: {path}"
        );
        assert!(path.ends_with("-context-pack.lisp"));
    }

    #[test]
    fn default_swarm_context_pack_path_is_collision_resistant_for_fanout() {
        let root = std::path::Path::new("/Users/jinchen/Projects/missiond");
        let first = default_swarm_context_pack_path(Some(root));
        let second = default_swarm_context_pack_path(Some(root));
        assert_ne!(
            first, second,
            "multiple mission_swarm_run calls in the same second must not overwrite one context pack"
        );
        assert!(first.ends_with("-context-pack.lisp"));
        assert!(second.ends_with("-context-pack.lisp"));
    }

    #[test]
    fn swarm_run_auto_provisions_claude_children_by_default() {
        let src = include_str!("./task_delegate.rs");
        assert!(
            src.contains("auto_provision_slots"),
            "mission_swarm_run must expose an explicit diagnostic override for slot preallocation"
        );
        assert!(
            src.contains("auto_provision_slots && planned_task.engine_hint == \"claude-code\""),
            "non-dry-run Claude swarm children must preallocate dynamic slots by default"
        );
        assert!(
            src.contains("assignee,"),
            "the preallocated dynamic slot id must be persisted as CreateBoardTaskInput.assignee"
        );
        assert!(
            src.contains("\"provisioned_slots\": provisioned_slots"),
            "mission_swarm_run response must report slot fanout results for operator monitoring"
        );
    }

    #[test]
    fn swarm_context_pack_materializes_worker_contract() {
        let planned = vec![
            SwarmPlannedTask {
                lane: "investigate".to_string(),
                engine_hint: "gemini".to_string(),
                pool_hint: "gemini-ultra-pro".to_string(),
                task_class: "context-pack".to_string(),
                title: "Investigate".to_string(),
                intent: "research".to_string(),
                read_scope: vec!["/repo".to_string()],
                write_scope: Vec::new(),
                must_not_touch: vec!["**/*".to_string()],
                accepted_shard_id: None,
                shared_claim_ids: Vec::new(),
            },
            SwarmPlannedTask {
                lane: "implement".to_string(),
                engine_hint: "claude-code".to_string(),
                pool_hint: "claude-code-default".to_string(),
                task_class: "code".to_string(),
                title: "Patch \"quoted\"".to_string(),
                intent: "code".to_string(),
                read_scope: vec!["/repo".to_string()],
                write_scope: vec!["src/auth.rs".to_string()],
                must_not_touch: vec!["target/**".to_string()],
                accepted_shard_id: Some("shard-auth-rs".to_string()),
                shared_claim_ids: Vec::new(),
            },
        ];
        let source = render_swarm_context_pack(
            "Objective with\nnewline",
            "missiond",
            "/repo",
            &[SwarmTargetProject {
                id: "missiond".to_string(),
                root: "/repo".to_string(),
            }],
            Some("parent-1"),
            "scoped-write",
            &["cargo test".to_string()],
            &planned,
            Some("context-gather:test"),
            &["project_registry".to_string()],
            1,
        );
        assert!(source.contains("(swarm-context-pack"));
        assert!(source.contains(":schema \"missiond.swarm-context-pack.v1\""));
        assert!(source.contains(":target_projects"));
        assert!(source.contains("(project :id \"missiond\" :root \"/repo\")"));
        assert!(source.contains(":parent_board_task_id \"parent-1\""));
        assert!(source.contains(":read_scope [\"/repo\"]"));
        assert!(source.contains(":write_scope [\"src/auth.rs\"]"));
        assert!(source.contains(":must_not_touch [\"target/**\"]"));
        assert!(source.contains(":title \"Patch \\\"quoted\\\"\""));
        assert!(source.contains(":objective \"Objective with\\nnewline\""));
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

    // ── Duplicate code-worker dedup guard ──────────────────────────────────
    //
    // BoardTask 31a99a30 :: prevent two concurrent code workers from racing on
    // the same files. The guard short-circuits on read-only/context-pack/
    // research delegations and only fires when:
    //   * intent == "code" OR task_class == "code", AND
    //   * write_scope is non-empty.
    // These tests pin the pure helpers (no AppState) — DB-touching paths are
    // covered by the integration suite via mocked board task fixtures.

    #[test]
    fn dedup_skips_read_only_research_delegations() {
        let scope = vec!["crates/foo.rs".to_string()];
        let check = DuplicateCodeCheck {
            parent_id: Some("parent-1"),
            source_id: None,
            project_id: Some("project-a"),
            intent: "research",
            task_class: Some("research"),
            write_scope: &scope,
        };
        assert!(!dedup_applies(&check));
    }

    #[test]
    fn dedup_skips_context_pack_class_even_when_scope_set() {
        let scope = vec!["crates/foo.rs".to_string()];
        let check = DuplicateCodeCheck {
            parent_id: Some("parent-1"),
            source_id: None,
            project_id: Some("project-a"),
            intent: "general",
            task_class: Some("context-pack"),
            write_scope: &scope,
        };
        assert!(!dedup_applies(&check));
    }

    #[test]
    fn dedup_skips_code_class_when_write_scope_is_empty() {
        let empty: Vec<String> = Vec::new();
        let check = DuplicateCodeCheck {
            parent_id: Some("parent-1"),
            source_id: None,
            project_id: Some("project-a"),
            intent: "code",
            task_class: Some("code"),
            write_scope: &empty,
        };
        assert!(
            !dedup_applies(&check),
            "code-class with empty write_scope is read-only-by-default; dedup must not block it"
        );
    }

    #[test]
    fn dedup_fires_on_code_class_with_write_scope() {
        let scope = vec!["crates/foo.rs".to_string()];
        let check = DuplicateCodeCheck {
            parent_id: Some("parent-1"),
            source_id: None,
            project_id: Some("project-a"),
            intent: "code",
            task_class: Some("code"),
            write_scope: &scope,
        };
        assert!(dedup_applies(&check));
    }

    #[test]
    fn dedup_fires_when_only_intent_is_code() {
        let scope = vec!["crates/foo.rs".to_string()];
        let check = DuplicateCodeCheck {
            parent_id: Some("parent-1"),
            source_id: None,
            project_id: Some("project-a"),
            intent: "code",
            task_class: None,
            write_scope: &scope,
        };
        assert!(dedup_applies(&check));
    }

    #[test]
    fn relative_write_scopes_do_not_overlap_across_projects() {
        let requested = vec![
            ".missiond/check.sh".to_string(),
            ".missiond/evidence/current-code-mapping.md".to_string(),
        ];
        let existing = vec![
            ".missiond/check.sh".to_string(),
            ".missiond/evidence/current-code-mapping.md".to_string(),
        ];
        assert!(!same_project_or_absolute_scope(
            Some("router"),
            Some("payments"),
            &requested,
            &existing,
        ));
        assert!(same_project_or_absolute_scope(
            Some("router"),
            Some("router"),
            &requested,
            &existing,
        ));
    }

    #[test]
    fn absolute_write_scopes_still_compare_across_projects() {
        let requested = vec!["/Users/jinchen/Projects/x/app.rs".to_string()];
        let existing = vec!["/Users/jinchen/Projects/x".to_string()];
        assert!(same_project_or_absolute_scope(
            Some("left"),
            Some("right"),
            &requested,
            &existing,
        ));
    }

    fn board_task_fixture(
        description: &str,
        runtime_metadata: Value,
    ) -> missiond_core::types::BoardTask {
        missiond_core::types::BoardTask {
            id: missiond_core::types::TaskId::from_trusted("task-1".to_string()),
            title: "Task".to_string(),
            description: description.to_string(),
            status: missiond_core::types::BoardTaskStatus::Running,
            priority: "medium".to_string(),
            category: "dev".to_string(),
            project: Some("missiond".to_string()),
            server: None,
            due_date: None,
            parent_id: None,
            assignee: None,
            auto_execute: true,
            prompt_template: None,
            hidden: false,
            retry_count: 0,
            max_retries: 2,
            order_idx: 0,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            claim_executor_id: None,
            claim_executor_type: None,
            claimed_at: None,
            flow_phase: None,
            flow_context: None,
            flow_template: None,
            depends_on: Vec::new(),
            lease_expires_at: None,
            dedupe_key: None,
            timeout_secs: None,
            context_intent: Some("code".to_string()),
            trigger_source: None,
            runtime_metadata,
            notes_count: 0,
        }
    }

    #[test]
    fn board_task_write_scope_prefers_runtime_metadata_over_description() {
        let task = board_task_fixture(
            "objective\n\n## Dispatch metadata\n- write_scope: legacy.rs",
            json!({
                "dispatch_metadata": {
                    "write_scope": ["src/runtime.rs", "src/control.rs"]
                }
            }),
        );
        assert_eq!(
            board_task_write_scope(&task),
            vec!["src/runtime.rs".to_string(), "src/control.rs".to_string()],
        );
    }

    #[test]
    fn board_task_source_reference_uses_runtime_metadata_without_description_fallback() {
        let metadata_task = board_task_fixture(
            "objective",
            json!({
                "dispatch_metadata": {
                    "source_board_task_id": "source-1"
                }
            }),
        );
        assert!(board_task_references_source(&metadata_task, "source-1"));

        let legacy_task = board_task_fixture(
            "objective\n\n## Dispatch metadata\n- source_board_task_id: legacy-source",
            json!({}),
        );
        assert!(
            !board_task_references_source(&legacy_task, "legacy-source"),
            "hard cutover forbids parsing BoardTask description as control metadata"
        );
    }

    #[test]
    fn duplicate_worker_source_reference_uses_task_contracts() {
        let contract = TaskRuntimeContract {
            parent_board_task_id: Some("parent-1".to_string()),
            source_board_task_id: Some("source-1".to_string()),
            write_scope: vec!["src/runtime.rs".to_string()],
            ..Default::default()
        };
        assert!(task_contract_references_parent(&contract, "parent-1"));
        assert!(!task_contract_references_parent(&contract, "parent-2"));
        assert!(task_contract_references_source(&contract, "source-1"));
        assert!(!task_contract_references_source(&contract, "source-2"));
    }

    #[test]
    fn compute_scope_overlap_reports_each_pair() {
        let requested = vec!["crates/auth".to_string(), "crates/router".to_string()];
        let existing = vec!["crates/auth/routes.rs".to_string(), "docs/**".to_string()];
        let overlap = compute_scope_overlap(&requested, &existing);
        assert_eq!(overlap.len(), 1);
        assert_eq!(overlap[0].0, "crates/auth");
        assert_eq!(overlap[0].1, "crates/auth/routes.rs");
    }

    #[test]
    fn compute_scope_overlap_reports_no_pairs_when_disjoint() {
        let requested = vec!["crates/foo".to_string()];
        let existing = vec!["crates/bar".to_string()];
        assert!(compute_scope_overlap(&requested, &existing).is_empty());
    }

    /// Refusal payload pins the structured schema callers / dashboards rely
    /// on. Critical fields:
    ///   * `error_code` ⇒ `DUPLICATE_CODE_WORKER_BLOCKED` (machine-readable)
    ///   * `existing_task_id` / `existing_task_status` / `linkage` so the
    ///     caller knows which active task collided and how it was matched
    ///   * `overlap` lists every requested-vs-existing pair
    ///   * `is_error: true` (consistent with other structured errors)
    #[test]
    fn build_duplicate_code_worker_refusal_emits_full_diagnostic_payload() {
        let dup = DuplicateCodeWorker {
            task_id: "btk-active-1".to_string(),
            title: "Refactor auth".to_string(),
            status: "running".to_string(),
            overlap: vec![(
                "crates/auth".to_string(),
                "crates/auth/routes.rs".to_string(),
            )],
            linkage: "parent".to_string(),
        };
        let result =
            build_duplicate_code_worker_refusal(&dup, Some("parent-1"), Some("source-9"), true);
        assert_eq!(result.is_error, Some(true));
        let text = match result.content.first() {
            Some(missiond_mcp::tools::ToolContent::Text { text }) => text.clone(),
            _ => panic!("refusal must emit text content"),
        };
        let payload: Value = serde_json::from_str(&text).expect("refusal must be valid JSON");
        assert_eq!(
            payload["error_code"],
            json!("DUPLICATE_CODE_WORKER_BLOCKED")
        );
        assert_eq!(payload["existing_task_id"], json!("btk-active-1"));
        assert_eq!(payload["existing_task_status"], json!("running"));
        assert_eq!(payload["linkage"], json!("parent"));
        assert_eq!(payload["parent_id"], json!("parent-1"));
        assert_eq!(payload["source_id"], json!("source-9"));
        assert_eq!(payload["note_attached"], json!(true));
        assert_eq!(payload["overlap"][0]["requested"], json!("crates/auth"));
        assert_eq!(
            payload["overlap"][0]["existing"],
            json!("crates/auth/routes.rs")
        );
        assert!(
            payload["suggestion"]
                .as_str()
                .unwrap()
                .contains("allow_duplicate_code_worker=true"),
            "suggestion must surface the override flag"
        );
    }

    /// Pins the worker-facing prompt projection. Runtime dedup reads
    /// `runtime_metadata`; this text remains only for operator/worker context.
    #[test]
    fn delegation_metadata_block_renders_source_id_for_dedup_readback() {
        let metadata = DelegationMetadata {
            task_class: Some("code".to_string()),
            write_scope: vec!["crates/foo.rs".to_string()],
            source_id: Some("source-9".to_string()),
            ..DelegationMetadata::default()
        };
        let block = render_delegation_metadata_block(&metadata);
        assert!(
            block.contains("- source_board_task_id: source-9"),
            "metadata block should keep source context visible in prompt projection:\n{block}"
        );
    }

    #[test]
    fn exact_shard_contract_blocks_code_write_without_context_pack_or_shard_id() {
        let missing_both = DelegationMetadata {
            task_class: Some("code".to_string()),
            write_scope: vec!["crates/foo.rs".to_string()],
            ..DelegationMetadata::default()
        };
        assert!(exact_shard_contract_error("code", &missing_both).is_some());

        let missing_shard = DelegationMetadata {
            task_class: Some("implementation".to_string()),
            context_pack_path: Some("/tmp/context-pack.lisp".to_string()),
            write_scope: vec!["crates/foo.rs".to_string()],
            ..DelegationMetadata::default()
        };
        assert!(exact_shard_contract_error("code", &missing_shard).is_some());

        let accepted = DelegationMetadata {
            task_class: Some("implementation".to_string()),
            context_pack_path: Some("/tmp/context-pack.lisp".to_string()),
            accepted_shard_id: Some("shard-auth-001".to_string()),
            write_scope: vec!["crates/foo.rs".to_string()],
            ..DelegationMetadata::default()
        };
        assert!(exact_shard_contract_error("code", &accepted).is_none());
    }

    #[test]
    fn mechanic_implementation_requires_exact_shard_metadata_even_without_write_scope() {
        let broad_mechanic = DelegationMetadata {
            task_class: Some("implementation".to_string()),
            engine_hint: Some("mechanic".to_string()),
            ..DelegationMetadata::default()
        };
        assert!(
            exact_shard_contract_error("code", &broad_mechanic).is_some(),
            "mechanic implementation must not accept a broad objective without exact shard metadata"
        );

        let missing_write_scope = DelegationMetadata {
            task_class: Some("implementation".to_string()),
            engine_hint: Some("mechanic".to_string()),
            context_pack_path: Some(".missiond/workflows/mechanic-repair-lane.lisp".to_string()),
            accepted_shard_id: Some("shard-mechanic-noop".to_string()),
            ..DelegationMetadata::default()
        };
        assert!(
            exact_shard_contract_error("code", &missing_write_scope).is_some(),
            "mechanic implementation must carry a concrete write_scope"
        );

        let accepted = DelegationMetadata {
            task_class: Some("implementation".to_string()),
            engine_hint: Some("mechanic".to_string()),
            context_pack_path: Some(".missiond/workflows/mechanic-repair-lane.lisp".to_string()),
            accepted_shard_id: Some("shard-mechanic-noop".to_string()),
            write_scope: vec![".missiond/workflows/mechanic-repair-lane.lisp".to_string()],
            ..DelegationMetadata::default()
        };
        assert!(exact_shard_contract_error("code", &accepted).is_none());
    }

    #[test]
    fn mechanic_engine_hint_is_opt_in_executor_lane() {
        let mechanic = DelegationMetadata {
            engine_hint: Some("mechanic".to_string()),
            ..DelegationMetadata::default()
        };
        assert!(engine_hint_is_mechanic(&mechanic));

        let jarvis_mechanic = DelegationMetadata {
            pool_hint: Some("jarvis-mechanic".to_string()),
            ..DelegationMetadata::default()
        };
        assert!(engine_hint_is_mechanic(&jarvis_mechanic));

        let claude = DelegationMetadata {
            engine_hint: Some("claude-code".to_string()),
            ..DelegationMetadata::default()
        };
        assert!(!engine_hint_is_mechanic(&claude));
    }

    #[test]
    fn mechanic_config_defaults_to_dry_run_and_accepted_shard_target() {
        let args = json!({
            "engine_hint": "mechanic",
        });
        let metadata = DelegationMetadata {
            engine_hint: Some("mechanic".to_string()),
            accepted_shard_id: Some("shard-fix-auth".to_string()),
            ..DelegationMetadata::default()
        };
        let config = parse_mechanic_run_config(&args, &metadata)
            .expect("valid mechanic args")
            .expect("mechanic config should be present");
        assert_eq!(config.mode, MechanicMode::DryRun);
        assert_eq!(config.target, "shard-fix-auth");
        assert_eq!(config.bin, "mechanic");
    }

    #[test]
    fn mechanic_config_requires_explicit_target_or_shard() {
        let args = json!({
            "engine_hint": "mechanic",
        });
        let metadata = DelegationMetadata {
            engine_hint: Some("mechanic".to_string()),
            ..DelegationMetadata::default()
        };
        assert!(parse_mechanic_run_config(&args, &metadata).is_err());
    }

    #[test]
    fn mechanic_config_rejects_unknown_mode() {
        let args = json!({
            "engine_hint": "mechanic",
            "mechanic_mode": "autonomous",
        });
        let metadata = DelegationMetadata {
            engine_hint: Some("mechanic".to_string()),
            accepted_shard_id: Some("shard-a".to_string()),
            ..DelegationMetadata::default()
        };
        assert!(parse_mechanic_run_config(&args, &metadata).is_err());
    }
}
