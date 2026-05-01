#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-workstation-config-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 workstation-config Lisp/code isomorphism contract:
  - coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model.
  - caller model/model_profile choices are projected through compute_slot/task_delegate.
  - delegated BoardTask auto-provision starts slots idle via suppress_initial_prompt.
  - project-local Claude hooks and MISSION_IPC_ENDPOINT are injected before PTY spawn.
  - Autopilot owns pty.send, close state, timeout budget, and dispatch guard.
  - Autopilot starts pty.send concurrently across different slots within a tick.
  - Autopilot clears stale slot-dyn-* assignee pins after daemon restart.
	  - mission_cc_swarm pty.send timeout is projected from timeout-policy claudecode-swarm.
	  - mission_pty_send waitForResponse timeout is projected from timeout-policy pty-send-blocking.
	  - Autopilot tick/dispatch/consciousness windows are projected from autopilot-policy.
	`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  main: 'crates/missiond-daemon/src/main.rs',
  computeSlot: 'crates/missiond-daemon/src/handlers/compute/compute_slot.rs',
  taskDelegate: 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
  slotEnv: 'crates/missiond-daemon/src/context/slot_env.rs',
  v3Runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  spawner: 'crates/missiond-daemon/src/slot_orchestrator/spawner.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  ccTasks: 'crates/missiond-daemon/src/handlers/compute/cc_tasks.rs',
  mcpComputeSlot: 'crates/missiond-mcp/src/tools/compute/compute_slot.rs',
  mcpTaskDelegate: 'crates/missiond-mcp/src/tools/compute/task_delegate.rs',
  mcpCcTasks: 'crates/missiond-mcp/src/tools/compute/cc_tasks.rs',
};

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
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
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

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'workstation-config',
    '(v2-item claudecode-workstation-config',
    ':status runtime-projected',
    '(tool-group workstation-entry',
    '(surface workstation-config',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/main.rs',
    'WorkstationRuntimeConfig::load_for_project_root',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'coding-default-opus-4-7',
    ':effective-model "Opus 4.7 with 1M context"',
    ':spawn-model-arg nil',
    'code and research dynamic slots MUST NOT hardcode --model sonnet',
    'daemon startup SlotManager ClaudeCode task configs MUST project coder/researcher model profiles from workstation-config',
    'model_profile=coding-default-opus-4-7 both mean no CLI --model override',
    'task_delegate must pass model/model_profile through to compute_slot',
    'Project-bound workstation spawn MUST sync MissionD Claude hooks',
    'MISSION_IPC_ENDPOINT',
    'Autopilot pty.send budget MUST project from BoardTask.timeout_secs',
    'Dynamic slot TTL and per-request extension budget MUST project from workstation-config ttl-policy dynamic-slot',
    'Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget',
    'Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold',
	    'mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm',
	    'mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking',
	    'autopilot-policy',
	    'AutopilotRuntimeConfig MUST load autopilot-policy',
	    'Autopilot tick windows',
	    'Autopilot dispatch windows',
	    'Autopilot consciousness windows',
	    'timeout-policy boardtask-dispatch',
    'timeout-policy claudecode-swarm',
    'timeout-policy pty-send-blocking',
    'ttl-policy dynamic-slot',
    ':default_secs 1800',
    ':default_secs 600',
    ':default_secs 300',
    ':min_secs 60',
    ':max_secs 7200',
    'mission_task_delegate auto-provision (compute_slot/spawner) MAY warm a dynamic slot but MUST NOT send the task objective',
    'The per-slot dispatch guard MUST be held across the entire state.pty.send call',
    'Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots within a single dispatch tick',
    'tokio::task::JoinSet task with an OwnedSlotDispatchGuard moved in',
    'Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins',
    'BoardStore::clear_board_task_assignee',
    'node scripts/check-v3-workstation-config-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.computeSlot, sources.computeSlot, [
    'const CODING_DEFAULT_PROFILE: &str = "coding-default-opus-4-7"',
    '"coder" => Some(TemplateConfig',
    'model: None',
    '"researcher" => Some(TemplateConfig',
    '"ops" => Some(TemplateConfig',
    'model: Some("sonnet")',
    'pub(crate) fn resolve_model_projection',
    'pub(crate) fn effective_initial_prompt',
    'pub(crate) fn model_projection_matches',
    'fn model_for_profile',
    'Ok(None)',
    'model must be a single safe CLI token',
    'string_arg(args, &["model_profile", "modelProfile"])',
    'string_arg(args, &["initial_prompt", "initialPrompt"])',
    'suppress_initial_prompt',
    'initial_prompt_for_spawn',
    'WorkstationRuntimeConfig::load_for_project_root',
    'clamp_slot_ttl_secs',
    'default_slot_extend_secs',
    'max_slot_extend_secs',
    'dynamic_slot_project_root',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'PTYSpawnOptions',
  ]);

  requireAll(diagnostics, files.main, sources.main, [
    'WorkstationRuntimeConfig::load_for_project_root',
    'default_spawn_model_for_template("researcher")',
    'default_spawn_model_for_template("coder")',
    'V3_BLUEPRINT_CONFIG_ERROR',
    'slot_orchestrator::SlotTaskConfig',
    'model: researcher_model.clone()',
    'model: coder_model',
  ]);
  forbidAll(diagnostics, files.main, sources.main, [
    'claude-sonnet-4-6',
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
    '"suppress_initial_prompt": true',
    'create_args["model_profile"]',
    'starts idle and Autopilot remains the sole task-prompt owner',
  ]);

  requireAll(diagnostics, files.v3Runtime, sources.v3Runtime, [
    'pub(crate) struct WorkstationRuntimeConfig',
    'pub(crate) struct AutopilotRuntimeConfig',
    'model_profile_spawn_args',
    'default_spawn_model_for_template',
    'parse_spawn_model_arg',
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
    'ttl-policy dynamic-slot',
    'slot-template',
    'DEFAULT_MODEL_PROFILE',
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
	    'DEFAULT_AUTOPILOT_SLOT_TASK_REAP_STALE_SECS',
	    'DEFAULT_AUTOPILOT_DEPLOY_REVIEW_TIMEOUT_SECS',
	    'DEFAULT_AUTOPILOT_RECENT_INTENTS_WINDOW_SECS',
	    'DEFAULT_AUTOPILOT_DIRECTION_SHIFT_COOLDOWN_SECS',
	    'default_slot_extend_secs',
	    'max_slot_extend_secs',
	    'clamp_cc_swarm_timeout_ms',
	    'clamp_pty_send_timeout_ms',
	    'deploy_review_timeout_ms',
	    'MissingBlueprint',
	  ]);

  requireAll(diagnostics, files.slotEnv, sources.slotEnv, [
    'MISSION_IPC_ENDPOINT',
    'build_slot_tracking_env',
    'sync_slot_hooks_to_local_settings',
    'settings.local.json',
    'SESSION_REGISTER_HOOK',
    'CONTEXT_INJECT_HOOK',
    'SessionStart',
    'UserPromptSubmit',
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
    'fn is_dynamic_slot_id',
    'fn should_clear_stale_dynamic_assignee',
    'clear_board_task_assignee(task.id.as_str(), id)',
    'OwnedSlotDispatchGuard::try_acquire(&state.slot_dispatch, &slot_id)',
    'state.pty.send(&slot_id, &full_prompt, timeout_ms).await',
    'tokio::task::JoinSet',
    'send_jobs.spawn',
    'send_jobs.join_next',
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
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600)
    :invariants
      ["code and research dynamic slots MUST NOT hardcode --model sonnet"
       "daemon startup SlotManager ClaudeCode task configs MUST project coder/researcher model profiles from workstation-config"
       "model=\\"default\\" and model_profile=coding-default-opus-4-7 both mean no CLI --model override"
       "task_delegate must pass model/model_profile through to compute_slot"
       "Project-bound workstation spawn MUST sync MissionD Claude hooks"
       "MISSION_IPC_ENDPOINT"
       "Autopilot pty.send budget MUST project from BoardTask.timeout_secs"
       "Dynamic slot TTL and per-request extension budget MUST project from workstation-config ttl-policy dynamic-slot"
       "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget"
       "Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold"
	       "mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm"
	       "mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking"
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
      :concurrent-slot-dispatch "Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots within a single dispatch tick. The implementation MUST hand each ready BoardTask's send + post-send tail to a tokio::task::JoinSet task with an OwnedSlotDispatchGuard moved in."))
  (implementation-map
    (surface workstation-config
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"]
      :note "WorkstationRuntimeConfig::load_for_project_root projects V3 workstation-config and surfaces V3_BLUEPRINT_CONFIG_ERROR."))
  (compression-contract
    :checks ["node scripts/check-v3-workstation-config-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.main, `
WorkstationRuntimeConfig::load_for_project_root();
default_spawn_model_for_template("researcher");
default_spawn_model_for_template("coder");
V3_BLUEPRINT_CONFIG_ERROR;
slot_orchestrator::SlotTaskConfig;
model: researcher_model.clone();
model: coder_model;`);
  writeFixture(root, DEFAULT_FILES.computeSlot, `
const CODING_DEFAULT_PROFILE: &str = "coding-default-opus-4-7";
match name { "coder" => Some(TemplateConfig { model: None }), "researcher" => Some(TemplateConfig { model: None }), "ops" => Some(TemplateConfig { model: Some("sonnet") }) }
pub(crate) fn resolve_model_projection() {}
pub(crate) fn effective_initial_prompt() {}
pub(crate) fn model_projection_matches() {}
fn model_for_profile() { Ok(None); }
const e = 'model must be a single safe CLI token';
string_arg(args, &["model_profile", "modelProfile"]);
string_arg(args, &["initial_prompt", "initialPrompt"]);
let suppress_initial_prompt = true;
let initial_prompt_for_spawn = None;
WorkstationRuntimeConfig::load_for_project_root();
clamp_slot_ttl_secs();
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
auto_provision_slot();
auto_provision_slot_ttl_secs();
runtime_config.clamp_slot_ttl_secs(None);
build_compute_slot_create_args();
json!({ "suppress_initial_prompt": true });
create_args["model_profile"] = v;
// starts idle and Autopilot remains the sole task-prompt owner`);
	  writeFixture(root, DEFAULT_FILES.v3Runtime, `
	pub(crate) struct WorkstationRuntimeConfig {}
	pub(crate) struct AutopilotRuntimeConfig {}
	pub(crate) struct TimeoutPolicy {}
	pub(crate) struct SlotTtlPolicy {}
	pub(crate) struct SimpleTimeoutPolicy {}
	pub(crate) fn load_for_project_root() {}
	fn parse_workstation_config() {}
	fn parse_autopilot_policy() {}
	fn x() {
  find_form(source, "workstation-config");
  find_form(source, "autopilot-policy");
  let a = "model_profile_spawn_args default_spawn_model_for_template parse_spawn_model_arg timeout-policy boardtask-dispatch timeout-policy claudecode-swarm timeout-policy pty-send-blocking ttl-policy dynamic-slot slot-template DEFAULT_MODEL_PROFILE DEFAULT_TIMEOUT_SECS MIN_TIMEOUT_SECS MAX_TIMEOUT_SECS WATCHDOG_GRACE_SECS MISSING_SESSION_PROBE_SECS DEFAULT_SLOT_TTL_SECS MIN_SLOT_TTL_SECS MAX_SLOT_TTL_SECS DEFAULT_SLOT_EXTEND_SECS MAX_SLOT_EXTEND_SECS DEFAULT_CC_SWARM_TIMEOUT_SECS MIN_CC_SWARM_TIMEOUT_SECS MAX_CC_SWARM_TIMEOUT_SECS DEFAULT_PTY_SEND_TIMEOUT_SECS MIN_PTY_SEND_TIMEOUT_SECS MAX_PTY_SEND_TIMEOUT_SECS DEFAULT_AUTOPILOT_SLOT_TASK_REAP_STALE_SECS DEFAULT_AUTOPILOT_DEPLOY_REVIEW_TIMEOUT_SECS DEFAULT_AUTOPILOT_RECENT_INTENTS_WINDOW_SECS DEFAULT_AUTOPILOT_DIRECTION_SHIFT_COOLDOWN_SECS default_slot_extend_secs max_slot_extend_secs clamp_cc_swarm_timeout_ms clamp_pty_send_timeout_ms deploy_review_timeout_ms MissingBlueprint";
}`);
  writeFixture(root, DEFAULT_FILES.slotEnv, `
const A = 'MISSION_IPC_ENDPOINT settings.local.json SESSION_REGISTER_HOOK CONTEXT_INJECT_HOOK SessionStart UserPromptSubmit missiond-session-register.sh missiond-context-inject-v2.sh';
fn build_slot_tracking_env() {}
fn sync_slot_hooks_to_local_settings() {}`);
  writeFixture(root, DEFAULT_FILES.spawner, `
sync_slot_hooks_to_local_settings(cwd);
build_slot_tracking_env();
options.extra_env.extend(tracking_env);
let initial_prompt = options.initial_prompt.take();
send_fire_and_forget();
let wait_for_idle = true;`);
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
fn is_dynamic_slot_id() {}
fn should_clear_stale_dynamic_assignee() {}
clear_board_task_assignee(task.id.as_str(), id);
OwnedSlotDispatchGuard::try_acquire(&state.slot_dispatch, &slot_id);
state.pty.send(&slot_id, &full_prompt, timeout_ms).await;
let mut send_jobs: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
send_jobs.spawn(async move {});
while let Some(_) = send_jobs.join_next().await {}
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
"timeout_secs" "default": 1800 "model" "model_profile" "modelProfile" "coding-default-opus-4-7"`);
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
