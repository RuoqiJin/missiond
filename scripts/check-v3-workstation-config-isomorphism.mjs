#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';
import { maybeRunLispc } from './lib/ocaml_lispc.mjs';

const usage = `Usage:
  node scripts/check-v3-workstation-config-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 workstation-config Lisp/code isomorphism contract:
  - coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model.
  - caller model/model_profile choices are projected through compute_slot/task_delegate.
  - delegated BoardTask auto-provision starts slots idle via suppress_initial_prompt.
  - project-local Claude hooks and MISSION_IPC_ENDPOINT are injected before PTY spawn.
  - Autopilot owns pty.send, close state, timeout budget, and dispatch guard.
  - Autopilot records delegated execution-log candidates without synthesizing completion.
  - Autopilot starts pty.send concurrently across different slots within a tick.
  - Autopilot clears stale slot-dyn-* assignee pins after daemon restart.
	  - mission_cc_swarm pty.send timeout is projected from timeout-policy claudecode-swarm.
	  - mission_pty_send waitForResponse timeout is projected from timeout-policy pty-send-blocking.
	  - Autopilot tick/dispatch/consciousness windows are projected from autopilot-policy.
	  - mission_compute_slot dynamic template role/description/mcp/default cwd and cwd allow-list are projected from V3.
	  - mission_compute_slot and Claude/Gemini slot-orchestrator spawn timeouts are projected from timeout-policy dynamic-slot-spawn.
	`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  helpers: 'crates/missiond-daemon/src/helpers.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  computeSlot: 'crates/missiond-daemon/src/handlers/compute/compute_slot.rs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  slotEnv: 'crates/missiond-daemon/src/context/slot_env.rs',
  v3Runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  v3SourceFallback: 'crates/missiond-daemon/src/context/v3_blueprint_runtime/source_fallback.rs',
  server: 'crates/missiond-core/src/ws/server.rs',
  spawner: 'crates/missiond-daemon/src/slot_orchestrator/spawner.rs',
  ccController: 'crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs',
  geminiDriver: 'crates/missiond-daemon/src/llm/gemini_driver.rs',
  ccWatcher: 'crates/missiond-core/src/cc_tasks/watcher.rs',
  flowEngine: 'crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs',
  memoryScheduler: 'crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  ccTasks: 'crates/missiond-daemon/src/handlers/compute/cc_tasks.rs',
  mcpComputeSlot: 'crates/missiond-mcp/src/tools/compute/compute_slot.rs',
  mcpTaskDelegate: 'crates/missiond-mcp/src/tools/compute/task_delegate.rs',
  mcpCcTasks: 'crates/missiond-mcp/src/tools/compute/cc_tasks.rs',
};

let workstationBlueprintSemanticsOk = false;

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES, { useOcaml: !dryFixture });
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 workstation-config Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 workstation-config Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files, { useOcaml = false } = {}) {
  const diagnostics = [];
  workstationBlueprintSemanticsOk = false;
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  if (useOcaml) {
    const semantic = maybeRunLispc(
      ['check-workstation-config', '--blueprint', files.blueprint],
      { engine: 'ocaml', repoRoot: root },
    );
    if (semantic.mode !== 'ocaml' || semantic.result?.ok !== true) {
      for (const d of semantic.result?.diagnostics ?? []) {
        diagnostics.push({
          file: d.file ?? files.blueprint,
          message: `OCaml workstation-config semantic gate failed: ${d.message ?? JSON.stringify(d)}`,
        });
      }
      if ((semantic.result?.diagnostics ?? []).length === 0) {
        diagnostics.push({
          file: files.blueprint,
          message: 'OCaml workstation-config semantic gate failed without diagnostics',
        });
      }
    } else {
      workstationBlueprintSemanticsOk = true;
    }
  }

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'workstation-config',
    'workstation-policy-shards',
    'slot-lifecycle-policy',
    'delegation-contract-policy',
    'completion-authority-policy',
    'cross-project-dispatch-policy',
    'context-prefetch-policy',
    'mcp-recovery-policy',
    '(v2-item claudecode-workstation-config',
    ':status runtime-projected',
    '(tool-group workstation-entry',
    '(surface workstation-config',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/main.rs',
    'crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs',
    'crates/missiond-daemon/src/llm/gemini_driver.rs',
    'WorkstationRuntimeConfig::load_for_project_root',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'coding-default-opus-4-7',
    'quick-haiku',
    ':effective-model "Opus 4.7 with 1M context"',
    ':spawn-model-arg nil',
    'code and research dynamic slots MUST NOT hardcode --model sonnet',
    'daemon startup SlotManager ClaudeCode task configs MUST project coder/researcher model profiles from workstation-config',
    'daemon startup SlotManager task configs MUST be generated from workstation-config startup-slot entries',
    'daemon startup MUST resolve the MissionD orchestrator root from MISSIOND_PROJECT_ROOT',
    'Clean-machine daemon startup MUST create missing provider history watch directories',
    'mission_compute_slot dynamic template role/description/mcp_config/default_cwd and allowed cwd prefixes MUST project from workstation-config slot-template + cwd-policy dynamic-slot',
    'Jarvis/OpenAI-compatible chat completions default slot MUST project from workstation-config chat-completions-policy jarvis-api',
    '(startup-slot arch_maintenance',
    '(startup-slot lisp_survey',
    '(cwd-policy dynamic-slot',
    '(chat-completions-policy jarvis-api',
    ':default_slot "slot-claude-code-default"',
    ':header_override "X-Slot-Id"',
    ':allowed-prefixes ["/Users/jinchen/Projects" "/Users/jinchen/Downloads" "/Users/jinchen/Documents" "/tmp"]',
    ':description "Dynamic coder slot (ephemeral)"',
    ':default-cwd "/Users/jinchen/Projects"',
    'model_profile=coding-default-opus-4-7 both mean no CLI --model override',
    'mission_compute_slot model_profile resolution MUST use workstation-config model-profile spawn-model-arg',
    'task_delegate must pass model/model_profile through to compute_slot',
    'mission_task_delegate MUST accept structured two-stage delegation metadata',
    'mission_task_delegate and mission_swarm_run MUST accept parent_id/parentId aliases',
    'target_project_ids/targetProjectIds',
    'render target_projects',
    'merge every target root into read_scope',
    'partition target project roots across workers',
    'commit-failure-blocker',
    'Autopilot MUST NOT mark the BoardTask done',
    'read_scope',
    'must_not_touch forbids write/stage/commit',
    'structured artifact with Findings / Evidence / Recommendations / Verification',
    'Project-bound workstation spawn MUST sync MissionD Claude hooks',
    'MISSION_IPC_ENDPOINT',
    'Autopilot pty.send budget MUST project from BoardTask.timeout_secs',
    'Dynamic slot TTL and per-request extension budget MUST project from workstation-config ttl-policy dynamic-slot',
    'Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget',
    'Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold',
    'mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm',
	    'mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking',
    'mission_compute_slot and Claude/Gemini slot-orchestrator dynamic slot spawn wait_for_idle timeouts MUST project from workstation-config timeout-policy dynamic-slot-spawn',
	    'mission_swarm_run fanout defaults/caps and dynamic slot limits MUST project from workstation-config capacity-policy swarm-workers',
	    'mission_swarm_run MUST auto-provision per-Claude dynamic slots by default',
	    'Claude/Gemini slot-orchestrator spawn',
	    'autopilot-policy',
	    'AutopilotRuntimeConfig MUST load autopilot-policy',
	    'Autopilot tick windows',
	    'Autopilot dispatch windows',
	    'Autopilot consciousness windows',
	    'timeout-policy boardtask-dispatch',
    'timeout-policy claudecode-swarm',
    'timeout-policy pty-send-blocking',
    'timeout-policy dynamic-slot-spawn',
    'capacity-policy swarm-workers',
    ':default_claude_workers 8',
    ':max_claude_workers 16',
    ':default_gemini_workers 2',
    ':max_gemini_workers 6',
    ':dynamic_slot_limit 20',
    ':delegate_rate_per_minute 24',
    'ttl-policy dynamic-slot',
    ':default_secs 1800',
    ':default_secs 600',
    ':default_secs 300',
    ':min_secs 60',
    ':max_secs 7200',
    'mission_task_delegate auto-provision (compute_slot/spawner) MAY warm a dynamic slot but MUST NOT send the task objective',
    'The per-slot dispatch guard MUST be held across the entire state.pty.send call',
    'Autopilot may record an observation/candidate, but it MUST NOT synthesize task completion',
    'Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots and MUST NOT wait for worker turn completion inside the dispatch tick',
    'detached tokio task with an OwnedSlotDispatchGuard moved in',
    'Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins',
    'BoardStore::clear_board_task_assignee',
    '(claude-code-mcp-recovery',
    ':forbid-numeric-shortcut true',
    ':missing-incident-kind "claude_code_mcp_missing"',
    ':reconnect-failed-incident-kind "claude_code_mcp_reconnect_failed"',
    'node scripts/check-v3-workstation-config-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.computeSlot, sources.computeSlot, [
    'const CODING_DEFAULT_PROFILE: &str = "coding-default-opus-4-7"',
    'WorkstationRuntimeConfig::load_for_current_dir',
    'slot_template(template_name)',
    'available_slot_template_names',
    'allowed_cwd_prefixes',
    'template.default_cwd.as_str()',
    'template.role.clone()',
    'template.mcp_config.clone()',
    'pub(crate) fn resolve_model_projection',
    'pub(crate) fn effective_initial_prompt',
    'pub(crate) fn model_projection_matches',
    'spawn_model_for_profile(profile)',
    'model must be a single safe CLI token',
    'string_arg(args, &["model_profile", "modelProfile"])',
    'string_arg(args, &["initial_prompt", "initialPrompt"])',
    'suppress_initial_prompt',
    'initial_prompt_for_spawn',
    'WorkstationRuntimeConfig::load_for_project_root',
    'clamp_slot_ttl_secs',
    'dynamic_slot_spawn_timeout_secs',
    'dynamic_slot_limit',
    'timeout_secs: Some(spawn_timeout_secs)',
    'default_slot_extend_secs',
    'max_slot_extend_secs',
    'dynamic_slot_project_root',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'PTYSpawnOptions',
  ]);

  requireAll(diagnostics, files.ccWatcher, sources.ccWatcher, [
    'tokio::fs::create_dir_all(&self.projects_dir).await',
    'Failed to create Claude Code projects directory before watching',
    'watcher.watch(&self.projects_dir, RecursiveMode::Recursive)',
  ]);
  forbidAll(diagnostics, files.computeSlot, sources.computeSlot, [
    'const ALLOWED_CWD_PREFIXES',
    'TemplateConfig {',
    'timeout_secs: Some(60)',
    '"/Users/jinchen/.xjp-mission/xjp-mcp-config.json"',
  ]);

  requireAll(diagnostics, files.main, sources.main, [
    'missiond_project_root()',
    'parse_startup_slot_engine',
    'parse_startup_slot_lifecycle',
    'WorkstationRuntimeConfig::load_for_project_root',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'for startup_slot in workstation_config.startup_slots()',
    'spawn_model_for_profile(profile)',
    'slot_orchestrator::SlotTaskConfig',
    'std::time::Duration::from_secs(startup_slot.timeout_secs)',
    'skip_permissions: startup_slot.skip_permissions',
    'chat_completions_default_slot()',
    'default_chat_slot',
  ]);
  forbidAll(diagnostics, files.main, sources.main, [
    'PathBuf::from("/Users/jinchen/Projects/missiond")',
    'claude-sonnet-4-6',
    'std::time::Duration::from_secs(600)',
    'std::time::Duration::from_secs(120)',
    'std::time::Duration::from_secs(900)',
  ]);
  requireAll(diagnostics, files.helpers, sources.helpers, [
    'pub(crate) fn missiond_project_root() -> PathBuf',
    'MISSIOND_PROJECT_ROOT',
    'MISSIOND_ORCHESTRATOR_ROOT',
    'nearest_project_root_with_blueprint',
    'pub(crate) fn missiond_blueprint_path() -> Option<PathBuf>',
  ]);
  forbidAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'Path::new("/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp")',
  ]);

  requireAll(diagnostics, files.taskDelegate, sources.taskDelegate, [
    'WorkstationRuntimeConfig::load_for_project_root',
    'runtime_config.clamp_timeout_secs',
    'default_model_profile_for_template',
    'effective_model_profile',
    'model_profile_arg',
    'resolve_model_projection',
    'model_projection_matches',
    'find_and_reserve_slot',
    'target_project_root',
    'auto_provision_slot',
    'auto_provision_slot_ttl_secs',
    'runtime_config.clamp_slot_ttl_secs(None)',
    'build_compute_slot_create_args',
    'auto_provision_slots',
    '"provisioned_slots": provisioned_slots',
    'planned_task.engine_hint == "claude-code"',
    'planned_task_primary_project',
    'swarm_task_effective_write_policy',
    'swarm_read_only_lane_keeps_read_only_policy_under_lisp_first_wave',
    'swarm_single_external_target_projects_child_task_to_target_root',
    'assignee,',
    '"suppress_initial_prompt": true',
    'create_args["model_profile"]',
    'starts idle and Autopilot remains the sole task-prompt owner',
    'struct DelegationMetadata',
    'fn string_list_arg',
    'fn render_delegation_metadata_block',
    'parent_task_id',
    'parentTaskId',
    'parent_board_task_id',
    'parentBoardTaskId',
    'parent_id: parent_id.clone()',
    'parent_board_task_id',
    'swarm_task_description_carries_parent_board_task_id_when_supplied',
    'target_project_ids',
    'targetProjectIds',
    'target_projects',
    'targetProjects',
    'resolve_swarm_target_projects',
    'render_target_projects_inline',
    'swarm_read_scope_for_worker',
    'swarm_read_scope_splits_target_projects_across_workers',
    'context_pack_path',
    'read_scope',
    'scope_semantics',
    'output_contract',
    'write_scope',
    'must_not_touch',
    'acceptance',
  ]);

  requireAll(diagnostics, files.mcpTaskDelegate, sources.mcpTaskDelegate, [
    '"task_class"',
    '"pool_hint"',
    '"engine_hint"',
    '"context_pack_path"',
    '"read_scope"',
    '"readScope"',
    '"write_scope"',
    '"must_not_touch"',
    '"acceptance"',
  ]);

	  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'pub(crate) struct WorkstationRuntimeConfig',
    'CompiledRuntimeConfigPayload',
    'load_compiled_runtime_config',
    'required_compiled_runtime_config',
    'Raw V3 Lisp source fallback is not a production runtime path',
    'compiled-runtime-config.json',
    'pub(crate) struct SlotTemplateRuntimeConfig',
    'pub(crate) struct StartupSlotRuntimeConfig',
    'pub(crate) struct AutopilotRuntimeConfig',
    'startup_slots',
    'slot_templates',
    'allowed_cwd_prefixes',
    'chat_completions_default_slot',
    'model_profile_spawn_args',
    'optional_non_nil_keyword',
    'default_spawn_model_for_template',
    'parse_spawn_model_arg',
    'slot_template',
    'allowed_cwd_prefixes',
    'available_slot_template_names',
    'load_for_current_dir',
    'pub(crate) struct TimeoutPolicy',
    'pub(crate) struct SlotTtlPolicy',
    'pub(crate) struct SimpleTimeoutPolicy',
    'pub(crate) fn load_for_project_root',
	    'parse_workstation_config',
	    'parse_autopilot_policy',
	    'find_form(source, "workstation-config")',
	    'find_form(source, "autopilot-policy")',
    'timeout-policy boardtask-dispatch',
    'timeout-policy claudecode-swarm',
    'timeout-policy pty-send-blocking',
    'timeout-policy dynamic-slot-spawn',
    'ttl-policy dynamic-slot',
    'cwd-policy dynamic-slot',
    'chat-completions-policy',
    'string_list_keyword',
    'slot-template',
    'DEFAULT_MODEL_PROFILE',
    'DEFAULT_CHAT_COMPLETIONS_DEFAULT_SLOT',
    'DEFAULT_TIMEOUT_SECS',
    'MIN_TIMEOUT_SECS',
    'MAX_TIMEOUT_SECS',
    'WATCHDOG_GRACE_SECS',
    'MISSING_SESSION_PROBE_SECS',
    'DEFAULT_SLOT_TTL_SECS',
    'MIN_SLOT_TTL_SECS',
    'MAX_SLOT_TTL_SECS',
    'DEFAULT_SLOT_EXTEND_SECS',
    'MAX_SLOT_EXTEND_SECS',
    'DEFAULT_CC_SWARM_TIMEOUT_SECS',
    'MIN_CC_SWARM_TIMEOUT_SECS',
    'MAX_CC_SWARM_TIMEOUT_SECS',
	    'DEFAULT_PTY_SEND_TIMEOUT_SECS',
	    'MIN_PTY_SEND_TIMEOUT_SECS',
	    'MAX_PTY_SEND_TIMEOUT_SECS',
	    'DEFAULT_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS',
	    'MIN_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS',
	    'MAX_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS',
	    'DEFAULT_AUTOPILOT_SLOT_TASK_REAP_STALE_SECS',
	    'DEFAULT_AUTOPILOT_DEPLOY_REVIEW_TIMEOUT_SECS',
	    'DEFAULT_AUTOPILOT_RECENT_INTENTS_WINDOW_SECS',
	    'DEFAULT_AUTOPILOT_DIRECTION_SHIFT_COOLDOWN_SECS',
	    'default_slot_extend_secs',
	    'max_slot_extend_secs',
	    'clamp_cc_swarm_timeout_ms',
	    'clamp_pty_send_timeout_ms',
	    'dynamic_slot_spawn_timeout_secs',
	    'deploy_review_timeout_ms',
	    'load_blueprint_source',
	    'locate_orchestrator_blueprint',
	    'orchestrator blueprint',
	  ]);

  requireAll(diagnostics, files.v3SourceFallback, sources.v3SourceFallback, [
    'ALLOW_SOURCE_FALLBACK_ENV',
    'MISSIOND_V3_ALLOW_SOURCE_FALLBACK',
    'COMPILE_RUNTIME_ACTION',
    'node scripts/compile-v3-runtime.mjs --json',
    'cfg!(debug_assertions) || cfg!(test)',
    'return false;',
  ]);

  requireAll(diagnostics, files.server, sources.server, [
    'default_chat_slot',
    'V3-projected default slot',
    'x-slot-id',
    'unwrap_or(default_chat_slot)',
  ]);
  forbidAll(diagnostics, files.server, sources.server, [
    'unwrap_or_else(|| "slot-jarvis".to_string())',
    'default "slot-jarvis"',
  ]);

  requireAll(diagnostics, files.slotEnv, sources.slotEnv, [
    'MISSION_IPC_ENDPOINT',
    'build_slot_tracking_env',
    'sync_slot_hooks_to_local_settings',
    'ensure_claude_home_hooks',
    'write_hook_script_if_changed',
    'SESSION_REGISTER_HOOK_SCRIPT',
    'CONTEXT_INJECT_HOOK_SCRIPT',
    'settings.local.json',
    'SESSION_REGISTER_HOOK',
    'CONTEXT_INJECT_HOOK',
    'MISSIOND_CLAUDE_CONTEXT_PREFETCH',
    'SessionStart',
    'UserPromptSubmit',
    'remove_hook_command',
    'sync_slot_hooks_removes_user_prompt_context_hook_by_default',
    'sync_slot_hooks_can_opt_in_user_prompt_context_hook',
    'missiond-session-register.sh',
    'missiond-context-inject-v2.sh',
  ]);

  requireAll(diagnostics, files.spawner, sources.spawner, [
    'sync_slot_hooks_to_local_settings(cwd)',
    'build_slot_tracking_env',
    'options.extra_env.extend(tracking_env)',
    'let initial_prompt = options.initial_prompt.take()',
    'send_fire_and_forget',
    'wait_for_idle',
  ]);

  requireAll(diagnostics, files.ccController, sources.ccController, [
    'WorkstationRuntimeConfig::load_for_project_root',
    'dynamic_slot_spawn_timeout_secs',
    'timeout_secs: Some(spawn_timeout_secs)',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.ccController, sources.ccController, [
    'timeout_secs: Some(120)',
  ]);

  requireAll(diagnostics, files.geminiDriver, sources.geminiDriver, [
    'RouterRuntimeConfig::load_for_project_root',
    'router_config.flow_gemini_model.as_str()',
    'model.unwrap_or(default_model)',
    'WorkstationRuntimeConfig::load_for_project_root',
    'dynamic_slot_spawn_timeout_secs',
    'timeout_secs: Some(spawn_timeout_secs)',
    'V3_BLUEPRINT_CONFIG_ERROR',
  ]);
  forbidAll(diagnostics, files.geminiDriver, sources.geminiDriver, [
    'const GEMINI_MODEL',
    '"gemini-3.1-pro-preview"',
    'timeout_secs: Some(120)',
  ]);

  requireAll(diagnostics, files.flowEngine, sources.flowEngine, [
    'WorkstationRuntimeConfig::load_for_project_root',
    'dynamic_slot_spawn_timeout_secs',
    'timeout_secs: Some(spawn_timeout_secs)',
    'PTY spawn 失败（{}s 超时）',
  ]);
  forbidAll(diagnostics, files.flowEngine, sources.flowEngine, [
    'timeout_secs: Some(120)',
    'PTY spawn 失败（120s 超时）',
  ]);

  requireAll(diagnostics, files.memoryScheduler, sources.memoryScheduler, [
    'WorkstationRuntimeConfig::load_for_project_root',
    'dynamic_slot_spawn_timeout_secs',
    'timeout_secs: Some(spawn_timeout_secs)',
  ]);
  forbidAll(diagnostics, files.memoryScheduler, sources.memoryScheduler, [
    'timeout_secs: Some(120)',
  ]);

	  requireAll(diagnostics, files.autopilot, sources.autopilot, [
	    'AutopilotRuntimeConfig::load_for_current_dir',
	    'dispatch_board_tasks_with_config',
	    'runtime_config.slot_task_reap_stale_secs',
	    'runtime_config.recover_stale_running_minutes',
	    'runtime_config.slot_failure_throttle_secs',
	    'runtime_config.deploy_review_timeout_ms()',
	    'runtime_config.recent_intents_window_secs',
	    'runtime_config.user_stuck_cooldown_secs',
	    'runtime_config.direction_shift_cooldown_secs',
	    'runtime_config.idle_persistent_slot_secs',
	    'fn derive_pty_timeout_secs',
    'fn idle_watchdog_threshold_secs',
    'fn derive_board_task_lease_secs',
    'fn build_base_prompt',
    'fn append_board_task_id_suffix',
    'fn decide_close_action',
    'fn extract_delegated_execution_id',
    'fn maybe_complete_delegated_execution_log',
    'fn worker_final_close_blocker',
    'Autopilot blocked close',
    'status: Some("blocked".to_string())',
    'provider_final_summary_rejects_retrying_once_progress',
    'worker_final_close_blocker_detects_commit_failures',
    '"schema": "missiond.delegated-execution-log-candidate.v1"',
    '"action": "job_event"',
    'fn is_dynamic_slot_id',
    'fn should_clear_stale_dynamic_assignee',
    'clear_board_task_assignee(task.id.as_str(), id)',
    'OwnedSlotDispatchGuard::try_acquire(&state.slot_dispatch, &slot_id)',
    'state.pty.send(&slot_id, &full_prompt, timeout_ms).await',
    'tokio::spawn(async move',
    'dispatch_board_tasks_detaches_send_tail_without_joinset_drain',
    'DispatchCloseAction::AlreadySelfClosed',
    'DispatchCloseAction::PreserveBlocked',
    'DispatchCloseAction::OwnerClosesAsDone',
  ]);

  requireAll(diagnostics, files.ccTasks, sources.ccTasks, [
    'mission_cc_swarm',
    'mission_cc_trigger_swarm',
    'WorkstationRuntimeConfig::load_for_project_root',
    'slot_project_root',
    'clamp_cc_swarm_timeout_ms',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'ToolResult::structured_error',
  ]);

  requireAll(diagnostics, files.mcpComputeSlot, sources.mcpComputeSlot, [
    '"initial_prompt"',
    '"initialPrompt"',
    '"objective"',
    '"model"',
    '"model_profile"',
    '"modelProfile"',
    'coding-default-opus-4-7',
  ]);

  requireAll(diagnostics, files.mcpTaskDelegate, sources.mcpTaskDelegate, [
    '"timeout_secs"',
    '"default": 1800',
    '"model"',
    '"model_profile"',
    '"modelProfile"',
    'coding-default-opus-4-7',
  ]);

  requireAll(diagnostics, files.mcpCcTasks, sources.mcpCcTasks, [
    '"mission_cc_swarm"',
    'timeout-policy claudecode-swarm',
    '"default": 600000',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (workstationBlueprintSemanticsOk && file === DEFAULT_FILES.blueprint) {
      // Blueprint semantics are owned by missiond-lispc check-workstation-config.
      // JS remains a compatibility wrapper and Rust/TS code-anchor scanner; it
      // must not fail on exact prose wording in workstation-config invariants.
      continue;
    }
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden contract text is still present: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-workstation-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (v2-convergence-map
    (v2-item claudecode-workstation-config
      :status runtime-projected))
  (public-surface-map
    (tool-group workstation-entry
      :status runtime-projected))
  (workstation-config
    (model-profile coding-default-opus-4-7
      :effective-model "Opus 4.7 with 1M context"
      :spawn-model-arg nil)
    (model-profile quick-haiku
      :spawn-model-arg "haiku")
    (slot-template coder
      :role coder
      :description "Dynamic coder slot (ephemeral)"
      :default-model-profile coding-default-opus-4-7
      :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json"
      :default-cwd "/Users/jinchen/Projects")
    (cwd-policy dynamic-slot
      :allowed-prefixes ["/Users/jinchen/Projects" "/Users/jinchen/Downloads" "/Users/jinchen/Documents" "/tmp"])
    (chat-completions-policy jarvis-api
      :default_slot "slot-claude-code-default"
      :header_override "X-Slot-Id")
    (startup-slot arch_maintenance
      :engine claude-code
      :lifecycle persistent
      :slot_id "slot-arch-maint"
      :role arch-maint
      :model_profile coding-default-opus-4-7
      :timeout_secs 600
      :skip_permissions true)
    (startup-slot lisp_survey
      :engine claude-code
      :lifecycle persistent
      :slot_id "lisp-surveyor"
      :role coder
      :model_profile coding-default-opus-4-7
      :timeout_secs 900
      :skip_permissions true)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)
    (timeout-policy pty-send-blocking
      :default_secs 300
      :min_secs 1
      :max_secs 7200)
    (timeout-policy dynamic-slot-spawn
      :default_secs 60
      :min_secs 10
      :max_secs 600)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600)
    :invariants
      ["code and research dynamic slots MUST NOT hardcode --model sonnet"
       "daemon startup SlotManager ClaudeCode task configs MUST project coder/researcher model profiles from workstation-config"
       "daemon startup SlotManager task configs MUST be generated from workstation-config startup-slot entries"
       "mission_compute_slot dynamic template role/description/mcp_config/default_cwd and allowed cwd prefixes MUST project from workstation-config slot-template + cwd-policy dynamic-slot"
       "Jarvis/OpenAI-compatible chat completions default slot MUST project from workstation-config chat-completions-policy jarvis-api"
	       "model=\\"default\\" and model_profile=coding-default-opus-4-7 both mean no CLI --model override"
	       "mission_compute_slot model_profile resolution MUST use workstation-config model-profile spawn-model-arg"
	       "task_delegate must pass model/model_profile through to compute_slot"
	       "mission_task_delegate MUST accept structured two-stage delegation metadata"
	       "read_scope"
	       "must_not_touch forbids write/stage/commit"
	       "structured artifact with Findings / Evidence / Recommendations / Verification"
	       "Project-bound workstation spawn MUST sync MissionD Claude hooks"
       "MISSION_IPC_ENDPOINT"
       "Autopilot pty.send budget MUST project from BoardTask.timeout_secs"
       "Dynamic slot TTL and per-request extension budget MUST project from workstation-config ttl-policy dynamic-slot"
       "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget"
       "Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold"
	       "mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm"
	       "mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking"
	       "mission_compute_slot and Claude/Gemini slot-orchestrator dynamic slot spawn wait_for_idle timeouts MUST project from workstation-config timeout-policy dynamic-slot-spawn"
	       "Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins"
	       "BoardStore::clear_board_task_assignee"])
	  (autopilot-policy
	    :slot-task-reap-stale-secs 1800
	    :recover-stale-running-minutes 15
	    :slot-failure-throttle-secs 1800
	    :deploy-review-timeout-secs 600
	    :recent-intents-window-secs 1800
	    :user-stuck-cooldown-secs 1800
	    :direction-shift-cooldown-secs 3600
	    :invariants
	      ["AutopilotRuntimeConfig MUST load autopilot-policy"
	       "Autopilot tick windows"
	       "Autopilot dispatch windows"
	       "Autopilot consciousness windows"])
	    (execution-ownership delegated-boardtask
	      :prompt-owner "mission_task_delegate auto-provision (compute_slot/spawner) MAY warm a dynamic slot but MUST NOT send the task objective"
	      :dispatch-guard "The per-slot dispatch guard MUST be held across the entire state.pty.send call"
	      :execution-log-synthesis "Autopilot may record an observation/candidate, but it MUST NOT synthesize task completion"
      :concurrent-slot-dispatch "Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots and MUST NOT wait for worker turn completion inside the dispatch tick. The implementation MUST hand each ready BoardTask's send + post-send tail to a detached tokio task with an OwnedSlotDispatchGuard moved in."))
  (workstation-policy-shards
    (policy slot-lifecycle-policy)
    (policy delegation-contract-policy)
    (policy completion-authority-policy)
    (policy cross-project-dispatch-policy)
    (policy context-prefetch-policy)
    (policy mcp-recovery-policy))
  (implementation-map
    (surface workstation-config
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"
             "crates/missiond-daemon/src/llm/gemini_driver.rs"]
      :note "WorkstationRuntimeConfig::load_for_project_root projects V3 workstation-config and surfaces V3_BLUEPRINT_CONFIG_ERROR. Claude/Gemini slot-orchestrator spawn timeouts project from dynamic-slot-spawn."))
  (compression-contract
    :checks ["node scripts/check-v3-workstation-config-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.main, `
missiond_project_root();
WorkstationRuntimeConfig::load_for_project_root();
parse_startup_slot_engine();
parse_startup_slot_lifecycle();
V3_BLUEPRINT_CONFIG_ERROR;
for startup_slot in workstation_config.startup_slots() {}
spawn_model_for_profile(profile);
slot_orchestrator::SlotTaskConfig;
std::time::Duration::from_secs(startup_slot.timeout_secs);
skip_permissions: startup_slot.skip_permissions;
chat_completions_default_slot();
let default_chat_slot = String::new();`);
  writeFixture(root, DEFAULT_FILES.helpers, `
pub(crate) fn missiond_project_root() -> PathBuf {
  let _ = "MISSIOND_PROJECT_ROOT";
  let _ = "MISSIOND_ORCHESTRATOR_ROOT";
  nearest_project_root_with_blueprint();
}
pub(crate) fn missiond_blueprint_path() -> Option<PathBuf> { None }`);
  writeFixture(root, DEFAULT_FILES.server, `
let default_chat_slot = String::new();
// V3-projected default slot
headers.lines().find(|line| line.to_lowercase().starts_with("x-slot-id:"));
slot_id = maybe_slot.unwrap_or(default_chat_slot);`);
  writeFixture(root, DEFAULT_FILES.computeSlot, `
const CODING_DEFAULT_PROFILE: &str = "coding-default-opus-4-7";
WorkstationRuntimeConfig::load_for_current_dir();
slot_template(template_name);
available_slot_template_names();
allowed_cwd_prefixes();
template.default_cwd.as_str();
template.role.clone();
template.mcp_config.clone();
pub(crate) fn resolve_model_projection() {}
pub(crate) fn effective_initial_prompt() {}
pub(crate) fn model_projection_matches() {}
runtime_config.spawn_model_for_profile(profile);
const e = 'model must be a single safe CLI token';
string_arg(args, &["model_profile", "modelProfile"]);
string_arg(args, &["initial_prompt", "initialPrompt"]);
let suppress_initial_prompt = true;
let initial_prompt_for_spawn = None;
WorkstationRuntimeConfig::load_for_project_root();
clamp_slot_ttl_secs();
dynamic_slot_spawn_timeout_secs();
timeout_secs: Some(spawn_timeout_secs);
default_slot_extend_secs();
max_slot_extend_secs();
dynamic_slot_project_root();
V3_BLUEPRINT_CONFIG_ERROR;
PTYSpawnOptions;`);
  writeFixture(root, DEFAULT_FILES.taskDelegate, `
WorkstationRuntimeConfig::load_for_project_root();
runtime_config.clamp_timeout_secs();
default_model_profile_for_template();
let effective_model_profile = None;
let model_profile_arg = None;
resolve_model_projection();
model_projection_matches();
find_and_reserve_slot();
let target_project_root = None;
planned_task_primary_project();
swarm_task_effective_write_policy();
swarm_read_only_lane_keeps_read_only_policy_under_lisp_first_wave();
swarm_single_external_target_projects_child_task_to_target_root();
auto_provision_slot();
auto_provision_slot_ttl_secs();
runtime_config.clamp_slot_ttl_secs(None);
	build_compute_slot_create_args();
	json!({ "suppress_initial_prompt": true });
	create_args["model_profile"] = v;
	struct DelegationMetadata;
	fn string_list_arg() {}
	fn render_delegation_metadata_block() {}
	let _ = "parent_task_id parentTaskId parent_board_task_id parentBoardTaskId parent_id: parent_id.clone() parent_board_task_id Parent linkage swarm_task_description_carries_parent_board_task_id_when_supplied";
	let _ = "context_pack_path read_scope write_scope must_not_touch acceptance scope_semantics output_contract";
	// starts idle and Autopilot remains the sole task-prompt owner`);
	  writeFixture(root, DEFAULT_FILES.v3Runtime, `
	pub(crate) struct WorkstationRuntimeConfig {}
	pub(crate) struct SlotTemplateRuntimeConfig {}
	pub(crate) struct StartupSlotRuntimeConfig {}
	pub(crate) struct AutopilotRuntimeConfig {}
	pub(crate) struct TimeoutPolicy {}
	pub(crate) struct SlotTtlPolicy {}
	pub(crate) struct SimpleTimeoutPolicy {}
	pub(crate) fn load_for_project_root() {}
	pub(crate) fn load_for_current_dir() {}
	fn parse_workstation_config() {}
	fn parse_autopilot_policy() {}
	fn x() {
  find_form(source, "workstation-config");
  find_form(source, "autopilot-policy");
  let a = "CompiledRuntimeConfigPayload load_compiled_runtime_config required_compiled_runtime_config source_fallback::allowed() compiled-runtime-config.json startup_slots slot_templates allowed_cwd_prefixes optional_non_nil_keyword model_profile_spawn_args default_spawn_model_for_template parse_spawn_model_arg slot_template available_slot_template_names timeout-policy boardtask-dispatch timeout-policy claudecode-swarm timeout-policy pty-send-blocking timeout-policy dynamic-slot-spawn capacity-policy swarm-workers ttl-policy dynamic-slot cwd-policy dynamic-slot string_list_keyword slot-template DEFAULT_MODEL_PROFILE DEFAULT_TIMEOUT_SECS MIN_TIMEOUT_SECS MAX_TIMEOUT_SECS WATCHDOG_GRACE_SECS MISSING_SESSION_PROBE_SECS DEFAULT_SLOT_TTL_SECS MIN_SLOT_TTL_SECS MAX_SLOT_TTL_SECS DEFAULT_SLOT_EXTEND_SECS MAX_SLOT_EXTEND_SECS DEFAULT_CC_SWARM_TIMEOUT_SECS MIN_CC_SWARM_TIMEOUT_SECS MAX_CC_SWARM_TIMEOUT_SECS DEFAULT_PTY_SEND_TIMEOUT_SECS MIN_PTY_SEND_TIMEOUT_SECS MAX_PTY_SEND_TIMEOUT_SECS DEFAULT_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS MIN_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS MAX_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS SwarmCapacityPolicy clamp_swarm_claude_workers clamp_swarm_gemini_workers dynamic_slot_limit delegate_rate_per_minute DEFAULT_AUTOPILOT_SLOT_TASK_REAP_STALE_SECS DEFAULT_AUTOPILOT_DEPLOY_REVIEW_TIMEOUT_SECS DEFAULT_AUTOPILOT_RECENT_INTENTS_WINDOW_SECS DEFAULT_AUTOPILOT_DIRECTION_SHIFT_COOLDOWN_SECS default_slot_extend_secs max_slot_extend_secs clamp_cc_swarm_timeout_ms clamp_pty_send_timeout_ms dynamic_slot_spawn_timeout_secs deploy_review_timeout_ms load_blueprint_source locate_orchestrator_blueprint orchestrator blueprint";
}`);
  writeFixture(root, DEFAULT_FILES.v3SourceFallback, `
ALLOW_SOURCE_FALLBACK_ENV MISSIOND_V3_ALLOW_SOURCE_FALLBACK COMPILE_RUNTIME_ACTION
node scripts/compile-v3-runtime.mjs --json
cfg!(debug_assertions) || cfg!(test)
return false;
`);
  writeFixture(root, DEFAULT_FILES.slotEnv, `
const A = 'MISSION_IPC_ENDPOINT settings.local.json SESSION_REGISTER_HOOK CONTEXT_INJECT_HOOK MISSIOND_CLAUDE_CONTEXT_PREFETCH SessionStart UserPromptSubmit missiond-session-register.sh missiond-context-inject-v2.sh ensure_claude_home_hooks write_hook_script_if_changed SESSION_REGISTER_HOOK_SCRIPT CONTEXT_INJECT_HOOK_SCRIPT';
fn build_slot_tracking_env() {}
fn sync_slot_hooks_to_local_settings() {}
fn ensure_claude_home_hooks() {}
fn write_hook_script_if_changed() {}
fn remove_hook_command() {}
fn sync_slot_hooks_removes_user_prompt_context_hook_by_default() {}
fn sync_slot_hooks_can_opt_in_user_prompt_context_hook() {}`);
  writeFixture(root, DEFAULT_FILES.spawner, `
sync_slot_hooks_to_local_settings(cwd);
build_slot_tracking_env();
options.extra_env.extend(tracking_env);
let initial_prompt = options.initial_prompt.take();
send_fire_and_forget();
let wait_for_idle = true;`);
  writeFixture(root, DEFAULT_FILES.ccController, `
WorkstationRuntimeConfig::load_for_project_root();
V3_BLUEPRINT_CONFIG_ERROR;
let spawn_timeout_secs = runtime_config.dynamic_slot_spawn_timeout_secs();
timeout_secs: Some(spawn_timeout_secs);`);
  writeFixture(root, DEFAULT_FILES.geminiDriver, `
RouterRuntimeConfig::load_for_project_root();
router_config.flow_gemini_model.as_str();
model.unwrap_or(default_model);
WorkstationRuntimeConfig::load_for_project_root();
V3_BLUEPRINT_CONFIG_ERROR;
let spawn_timeout_secs = runtime_config.dynamic_slot_spawn_timeout_secs();
timeout_secs: Some(spawn_timeout_secs);`);
  writeFixture(root, DEFAULT_FILES.flowEngine, `
WorkstationRuntimeConfig::load_for_project_root();
let spawn_timeout_secs = workstation_config.dynamic_slot_spawn_timeout_secs();
timeout_secs: Some(spawn_timeout_secs);
format!("PTY spawn 失败（{}s 超时）", spawn_timeout_secs);`);
  writeFixture(root, DEFAULT_FILES.memoryScheduler, `
WorkstationRuntimeConfig::load_for_project_root();
let spawn_timeout_secs = workstation_config.dynamic_slot_spawn_timeout_secs();
timeout_secs: Some(spawn_timeout_secs);`);
	  writeFixture(root, DEFAULT_FILES.autopilot, `
	AutopilotRuntimeConfig::load_for_current_dir();
	dispatch_board_tasks_with_config();
	runtime_config.slot_task_reap_stale_secs;
	runtime_config.recover_stale_running_minutes;
	runtime_config.slot_failure_throttle_secs;
	runtime_config.deploy_review_timeout_ms();
	runtime_config.recent_intents_window_secs;
	runtime_config.user_stuck_cooldown_secs;
	runtime_config.direction_shift_cooldown_secs;
	runtime_config.idle_persistent_slot_secs;
	fn derive_pty_timeout_secs() {}
fn idle_watchdog_threshold_secs() {}
fn derive_board_task_lease_secs() {}
fn build_base_prompt() {}
fn append_board_task_id_suffix() {}
fn decide_close_action() {}
fn extract_delegated_execution_id() {}
fn maybe_complete_delegated_execution_log() {}
fn worker_final_close_blocker() {}
let _ = "Autopilot blocked close";
let _ = "status: Some(\"blocked\".to_string())";
provider_final_summary_rejects_retrying_once_progress();
worker_final_close_blocker_detects_commit_failures();
"schema": "missiond.delegated-execution-log-candidate.v1";
"action": "job_event";
fn is_dynamic_slot_id() {}
fn should_clear_stale_dynamic_assignee() {}
clear_board_task_assignee(task.id.as_str(), id);
OwnedSlotDispatchGuard::try_acquire(&state.slot_dispatch, &slot_id);
state.pty.send(&slot_id, &full_prompt, timeout_ms).await;
tokio::spawn(async move {});
dispatch_board_tasks_detaches_send_tail_without_joinset_drain();
DispatchCloseAction::AlreadySelfClosed;
DispatchCloseAction::PreserveBlocked;
DispatchCloseAction::OwnerClosesAsDone;`);
  writeFixture(root, DEFAULT_FILES.ccTasks, `
mission_cc_swarm;
mission_cc_trigger_swarm;
WorkstationRuntimeConfig::load_for_project_root();
slot_project_root();
clamp_cc_swarm_timeout_ms();
V3_BLUEPRINT_CONFIG_ERROR;
ToolResult::structured_error;`);
  writeFixture(root, DEFAULT_FILES.mcpComputeSlot, `
"initial_prompt" "initialPrompt" "objective" "only" "model" "model_profile" "modelProfile" "coding-default-opus-4-7"`);
 writeFixture(root, DEFAULT_FILES.mcpTaskDelegate, `
	"timeout_secs" "default": 1800 "model" "model_profile" "modelProfile" "coding-default-opus-4-7"
	"task_class" "pool_hint" "engine_hint" "context_pack_path" "read_scope" "readScope" "write_scope" "must_not_touch" "acceptance"`);
  writeFixture(root, DEFAULT_FILES.mcpCcTasks, `
"mission_cc_swarm" "timeout-policy claudecode-swarm" "default": 600000`);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
