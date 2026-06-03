#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';
import {
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
} from './lib/v3_resolved_lisp_compat.mjs';

const usage = `Usage:
  node scripts/check-v3-workstation-pool-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 workstation-pool Lisp/code isomorphism contract:
  - V3 declares Claude Code Default, Claude fast-patch, Gemini, Codex, and Agy workers.
  - Claude coding default remains coding-default-opus-4-7, which omits --model.
  - Gemini remains read-only until a separate write smoke promotes it.
  - Rust projects the pool into SlotManager, MissionControl runtime slots,
    Autopilot BoardTask routing, and mission_compute_slot observability.
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  evidence: '.missiond/v3/evidence/workstation-pool.lisp',
  geminiPolicy: '.missiond/v3/policies/gemini-readonly-policy.toml',
  runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  genericCli: 'crates/missiond-daemon/src/slot_orchestrator/generic_cli.rs',
  controlTree: 'crates/missiond-daemon/src/control_tree.rs',
  ptySession: 'crates/missiond-pty/src/session.rs',
  ptyRecognition: 'crates/missiond-pty/src/pty_recognition.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  computeSlot: 'crates/missiond-daemon/src/handlers/compute/compute_slot.rs',
  slotTool: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  wsServer: 'crates/missiond-core/src/ws/server.rs',
  slotManager: 'crates/missiond-core/src/core/slot_manager.rs',
  missionControl: 'crates/missiond-core/src/core/mission_control.rs',
  projectTypes: 'crates/missiond-core/src/types/project.rs',
  supervisor: 'crates/missiond-daemon/src/supervisor.rs',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
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

  const root = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(root);
  const result = { ok: diagnostics.length === 0, files: Object.keys(FILES).length, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 workstation-pool Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(
      `v3 workstation-pool Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }
  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(path.join(root, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  validateBlueprint(FILES.blueprint, sources.blueprint, diagnostics);

  requireAll(diagnostics, FILES.evidence, sources.evidence, [
    'workstation-pool-evidence',
    'single-login-phase',
    'claude-code-default',
    'claude-code-deploy-ops',
    'claude-code-fast-patch',
    'gemini-ultra-pro',
    'gemini-fast-survey',
    'codex-master-control',
    'codex-code-worker',
    'codex-review-worker',
    'codex-intent-author',
    'codex-plan-author',
    'agy-research',
    'read-only',
    ':tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"',
    'mission_compute_slot action=list exposes workstation_pool',
  ]);
  requireAll(diagnostics, FILES.geminiPolicy, sources.geminiPolicy, [
    'toolName = ["generalist", "codebase_investigator", "invoke_agent", "invoke_subagent"]',
    'toolName = [',
    '"replace"',
    '"write_file"',
    '"run_shell_command"',
    '"exit_plan_mode"',
    'decision = "deny"',
    'modes = ["plan"]',
  ]);

  requireAll(diagnostics, FILES.runtime, sources.runtime, [
    'pub(crate) struct WorkstationPoolRuntimeConfig',
    'workstation_pool: Vec<WorkstationPoolRuntimeConfig>',
    'pub(crate) fn workstation_pool(&self) -> &[WorkstationPoolRuntimeConfig]',
    'pub(crate) fn boardtask_pool_candidates',
    'find_form(source, "workstation-pool")',
    'workstation-pool must include a Claude Code default BoardTask worker',
    'workstation-pool must include a read-only Gemini BoardTask worker',
    'workstation-pool must include a read-only Agy BoardTask worker',
    'workstation-pool must include a non-shard Codex master-control worker',
    'workstation-pool must include at least one Codex non-master worker lane',
    '"claude-code-default"',
    '"claude-code-deploy-ops"',
    '"gemini-ultra-pro"',
    '"codex-master-control"',
    'worker.engine == "agy"',
    'worker.engine == "codex"',
    'worker.role != "orchestrator"',
    'reasoning_effort',
    'search_enabled',
    'approval_policy',
    'tool_policy_path',
    'read-only Gemini workstation-pool workers must declare :tool-policy-path',
  ]);

  requireAll(diagnostics, FILES.main, sources.main, [
    'fn workstation_pool_model',
    'fn jarvis_intent_author_config',
    'fn jarvis_plan_author_config',
    'GenericCliSlotManager::new',
    'CliEngine::Agy',
    '"agy" | "agy-cli"',
    'fn startup_slot_config',
    'fn workstation_pool_slot_config',
    'reasoning_effort: worker.reasoning_effort.clone()',
    'search_enabled: Some(worker.search_enabled).filter',
    'async fn register_and_init_runtime_slot',
    'state.pty.init_slot(&pty_slot).await',
    'for worker in workstation_config.workstation_pool()',
    'fn missiond_managed_skip_permissions',
    'dangerously_skip_permissions: Some(missiond_managed_skip_permissions(engine, false))',
    'tool_policy_path: worker.tool_policy_path.clone()',
    'reasoning_effort: worker.reasoning_effort.clone()',
    'search_enabled: worker.search_enabled',
    'skip_permissions: missiond_managed_skip_permissions(engine, false)',
    'state.mission.register_runtime_slot(slot_config)',
    'SlotManager: workstation pool registered from V3',
    'missiond_project_root()',
    'Project registry: overlaying missiond root from runtime environment',
    'Project registry: adding runtime missiond root overlay',
  ]);

  requireAll(diagnostics, FILES.projectTypes, sources.projectTypes, [
    'use std::path::Path;',
    'let cwd_path = Path::new(cwd);',
    'cwd_path.starts_with(Path::new(prefix))',
    'fn resolve_does_not_match_sibling_by_string_prefix',
    '/Users/rickyhq/Projects/missiond-clean',
  ]);

  requireAll(diagnostics, FILES.genericCli, sources.genericCli, [
    'GenericCliSlotManager',
    'PTYSpawnOptions',
    'reasoning_effort: req.reasoning_effort.clone()',
    'search_enabled: req.search_enabled',
    'sandbox: req.sandbox.clone()',
    'approval_policy: req.approval_policy.clone()',
    'tool_policy_path: req.tool_policy_path.clone()',
    'canonical_source_for_engine(self.engine)',
    'TextComplete',
  ]);

  requireAll(diagnostics, FILES.controlTree, sources.controlTree, [
    'Agy,',
    'Self::Agy => None',
  ]);

  requireAll(diagnostics, FILES.ptySession, sources.ptySession, [
    'CliEngine::Gemini',
    'CliEngine::Agy',
    '"agy".to_string()',
    '--approval-mode plan',
    '--policy',
    '--approval-mode yolo',
    'tool_policy_path: Option<&std::path::Path>',
    'Gemini CLI: read-only tool policy enabled',
    'Gemini CLI: plan/read-only approval mode enabled',
    'gemini_command_uses_plan_mode_unless_permissions_are_skipped',
  ]);
  requireAll(diagnostics, FILES.ptyRecognition, sources.ptyRecognition, [
    'CliEngine::Agy => recognize_agy(lines)',
    'fn recognize_agy',
    'agy_idle_screen_is_idle',
    'agy_auth_or_quota_error_is_blocked',
  ]);
  forbidAll(diagnostics, FILES.ptySession, sources.ptySession, [
    'parts.push_str(" --yolo")',
  ]);

  requireAll(diagnostics, FILES.autopilot, sources.autopilot, [
    'fn task_contract_workstation_class',
    'deploy-ops',
    'async fn select_workstation_pool_slot',
    'runtime_contract: &TaskRuntimeContract',
    'let task_class = task_contract_workstation_class(task, runtime_contract);',
    '.boardtask_pool_candidates(task_class)',
    'let engine_hint = runtime_contract.engine_hint.clone();',
    'let pool_hint = runtime_contract.pool_hint.clone();',
    '.task_runtime_contract(task.id.as_str())',
    'legacy BoardTask.runtime_metadata fallback is disabled',
    'fn dispatch_hint_eq',
    ".replace('_', \"-\")",
    'fn workstation_worker_matches_dispatch_hints',
    'workstation_config\n            .workstation_pool()',
    'matching_candidates',
    'explicit_dispatch_hints_are_hard_constraints_when_worker_exists',
    'explicit_dispatch_hints_search_full_pool_before_task_class_fallback',
    'dispatch_hint_matching_normalizes_underscore_and_hyphen',
    'fn conversation_is_open_for_dispatch',
    'fn conversation_ended_before_claim',
    'Autopilot: skipped dispatch-time conversation rebind for completed historical slot session',
    'conversation_ended_before_claim_rejects_stale_final',
    'dispatch_rebind_skips_completed_conversation',
    'engine_hint_alone_does_not_widen_code_class_to_fast_patch',
    'engine_hint=claude-code alone must not pull claude-code-fast-patch into a complex code shard',
    'struct WorkstationSlotSelection',
    'reroute_reason',
    'Workstation dispatch reroute recorded',
    'plan-mode-no-write',
    'worker_final_close_blocker_detects_plan_mode_no_write',
    'task_class == "code" && !worker.write_allowed',
    'SessionState::Exited',
    'SessionState::Error',
    'Autopilot: selected V3 workstation-pool slot',
    'ensure_autopilot_pty(state, &task, &slot_id, task_env).await',
    'state.store.unclaim_board_task(task.id.as_str()).await',
  ]);
  forbidAll(diagnostics, FILES.autopilot, sources.autopilot, [
    'extract_dispatch_metadata_field(&task.description, field)',
    'extract_board_task_dispatch_metadata_field(task, "engine_hint")',
    'extract_board_task_dispatch_metadata_field(task, "pool_hint")',
  ]);

  requireAll(diagnostics, FILES.computeSlot, sources.computeSlot, [
    'WorkstationRuntimeConfig::load_for_current_dir',
    '"workstation_pool": workstation_pool',
    '"runtime_slot_present": runtime_slot_present',
    '"V3_BLUEPRINT_CONFIG_ERROR"',
    'fn classify_static_slot',
    'fn derive_static_status',
    'StaticSlotClass',
    '"legacy_static_slots"',
    '"dispatchable":',
    '"legacy":',
    '"v3_authoritative"',
    's.config.id.starts_with("slot-dyn-")',
    'state\n            .pty\n            .get_status(&s.config.id)',
  ]);
  forbidAll(diagnostics, FILES.computeSlot, sources.computeSlot, [
    'if s.session_id.is_some() { "running" } else { "stopped" }',
  ]);
  requireAll(diagnostics, FILES.slotTool, sources.slotTool, [
    'fn projected_mission_slots',
    'WorkstationRuntimeConfig::load_for_current_dir',
    'list_board_tasks(Some("running"), true)',
    'fn active_board_task_for_slot',
    '"activeBoardTaskId"',
    '"currentTaskId"',
    '"activeBoardTask"',
    'get_running_slot_task(&slot.config.id)',
    'fn is_stopped_legacy_sonnet_residual',
    'model.contains("sonnet")',
    'v3_slot_ids.contains(&slot.config.id)',
  ]);
  requireAll(diagnostics, FILES.wsServer, sources.wsServer, [
    'fn handle_slot_status',
    'list_board_tasks(Some("running"), true)',
    'fn active_board_task_for_slot_status',
    'fn board_task_status_summary_json',
    '"activeBoardTaskId"',
    '"currentTaskId"',
    '"activeBoardTask"',
  ]);

  requireAll(diagnostics, FILES.slotManager, sources.slotManager, [
    'pub fn register_runtime_slot(&self, mut config: SlotConfig)',
    'Runtime slot registered',
  ]);
  requireAll(diagnostics, FILES.missionControl, sources.missionControl, [
    'pub fn register_runtime_slot(&self, config: SlotConfig)',
    'self.slot_manager.register_runtime_slot(config)',
  ]);
  requireAll(diagnostics, FILES.blueprint, sources.blueprint, [
    'Supervisor patrol (slot-supervisor) is gated on V3 workstation-pool / runtime-config registration',
    'V3 workstation-pool (plus startup-slots) is authoritative for dispatchable slots',
    'mission_compute_slot list status MUST derive from PTYManager',
    'mission_slots and /api/slots MUST project activeBoardTaskId/currentTaskId and activeBoardTask',
    'Codex master-control is a resident orchestrator lane',
    'Read-only Gemini pool workers MUST project to Gemini CLI `--approval-mode plan --policy .missiond/v3/policies/gemini-readonly-policy.toml`',
    'ProjectRegistry path resolution MUST be path-component aware rather than raw string prefix matching',
    'Daemon startup MUST overlay the active missiond project root from MISSIOND_PROJECT_ROOT/current blueprint root',
  ]);
  requireAll(diagnostics, FILES.supervisor, sources.supervisor, [
    'fn schedule_supervisor_patrol',
    'let supervisor_registered',
    '.mission',
    '.list_slots()',
    's.config.id == SUPERVISOR_SLOT_ID',
    'if !supervisor_registered',
  ]);
  requireAll(diagnostics, FILES.aggregate, sources.aggregate, [
    "'workstation-pool'",
    'scripts/check-v3-workstation-pool-isomorphism.mjs',
  ]);

  return diagnostics;
}

function validateBlueprint(file, source, diagnostics) {
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    diagnostics.push({ file, message: `cannot parse Lisp: ${err.message}` });
    return;
  }
  const root = forms.find((form) => isList(form) && head(form) === 'missiond-blueprint');
  if (!root) {
    diagnostics.push({ file, message: 'missing missiond-blueprint root' });
    return;
  }
  const pool = root.children.find((form) => isList(form) && head(form) === 'workstation-pool');
  if (!pool) {
    diagnostics.push({ file, message: 'missing top-level (workstation-pool ...)' });
    return;
  }
  const poolProps = readKeywordProps(pool);
  if (keywordPropText(poolProps, ':evidence') !== '.missiond/v3/evidence/workstation-pool.lisp') {
    diagnostics.push({ file, message: 'workstation-pool must point to evidence sidecar' });
  }
  if (keywordPropText(poolProps, ':checker') !== 'node scripts/check-v3-workstation-pool-isomorphism.mjs') {
    diagnostics.push({ file, message: 'workstation-pool must pin its checker command' });
  }
  const workers = pool.children.filter((child) => isList(child) && head(child) === 'worker');
  const byId = new Map(workers.map((worker) => [nodeText(worker.children[1]), worker]));
  validateClaudeWorker(file, byId.get('claude-code-default'), diagnostics);
  validateDeployOpsWorker(file, byId.get('claude-code-deploy-ops'), diagnostics);
  validateFastPatchWorker(file, byId.get('claude-code-fast-patch'), diagnostics);
  validateGeminiWorker(file, byId.get('gemini-ultra-pro'), diagnostics);
  validateGeminiFastWorker(file, byId.get('gemini-fast-survey'), diagnostics);
  validateCodexMasterWorker(file, byId.get('codex-master-control'), diagnostics);
  validateCodexWorker(file, byId.get('codex-code-worker'), diagnostics, {
    role: 'coder',
    slotId: 'slot-codex-code-worker',
    taskType: 'codex_code_worker',
    writeAllowed: true,
    sandbox: 'workspace-write',
    taskClasses: ['code', 'design', 'review', 'regression-analysis'],
  });
  validateCodexWorker(file, byId.get('codex-review-worker'), diagnostics, {
    role: 'reviewer',
    slotId: 'slot-codex-review-worker',
    taskType: 'codex_review_worker',
    writeAllowed: false,
    sandbox: 'read-only',
    taskClasses: ['review', 'architecture-review', 'regression-analysis'],
  });
  validateCodexWorker(file, byId.get('codex-intent-author'), diagnostics, {
    role: 'intent-author',
    slotId: 'slot-codex-intent-author',
    taskType: 'codex_intent_author',
    writeAllowed: false,
    acceptsBoardtask: false,
    sandbox: 'read-only',
    taskClasses: ['intent-authoring', 'intent-recognition'],
  });
  validateCodexWorker(file, byId.get('codex-plan-author'), diagnostics, {
    role: 'plan-author',
    slotId: 'slot-codex-plan-author',
    taskType: 'codex_plan_author',
    writeAllowed: false,
    acceptsBoardtask: false,
    sandbox: 'read-only',
    taskClasses: ['plan-authoring', 'plan-generation'],
  });
  validateAgyWorker(file, byId.get('agy-research'), diagnostics);

  const impl = root.children.find((form) => isList(form) && head(form) === 'implementation-map');
  const surface = impl?.children.find(
    (form) => isList(form) && head(form) === 'surface' && nodeText(form.children[1]) === 'workstation-pool',
  );
  if (!surface) {
    diagnostics.push({ file, message: 'implementation-map missing workstation-pool surface' });
    return;
  }
  const surfaceProps = readKeywordProps(surface);
  if (keywordPropText(surfaceProps, ':status') !== 'code-aligned') {
    diagnostics.push({ file, message: 'workstation-pool surface must be code-aligned' });
  }
  const codeRefs = nodeToStringArray(surfaceProps[':code']?.value);
  for (const required of [
    'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
    'crates/missiond-daemon/src/main.rs',
    'crates/missiond-daemon/src/slot_orchestrator/generic_cli.rs',
    'crates/missiond-pty/src/session.rs',
    'crates/missiond-pty/src/pty_recognition.rs',
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    'crates/missiond-daemon/src/handlers/compute/compute_slot.rs',
    'crates/missiond-daemon/src/handlers/compute/slot.rs',
    'crates/missiond-core/src/types/project.rs',
    'scripts/check-v3-workstation-pool-isomorphism.mjs',
  ]) {
    if (!codeRefs.includes(required)) {
      diagnostics.push({ file, message: `workstation-pool surface missing code ref: ${required}` });
    }
  }
}

function validateCodexWorker(file, worker, diagnostics, expected) {
  if (!worker) {
    diagnostics.push({ file, message: `workstation-pool missing ${expected.taskType} worker` });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'codex');
  requirePropText(diagnostics, file, props, ':role', expected.role);
  requirePropText(diagnostics, file, props, ':slot-id', expected.slotId);
  requirePropText(diagnostics, file, props, ':task-type', expected.taskType);
  requirePropText(diagnostics, file, props, ':model-profile', 'codex-master-gpt-5-5-xhigh');
  requirePropText(diagnostics, file, props, ':reasoning-effort', 'xhigh');
  requirePropText(diagnostics, file, props, ':sandbox', expected.sandbox ?? 'workspace-write');
  requirePropText(diagnostics, file, props, ':approval-policy', 'never');
  requirePropBool(diagnostics, file, props, ':search', true);
  requirePropBool(diagnostics, file, props, ':accepts-boardtask', expected.acceptsBoardtask ?? true);
  requirePropBool(diagnostics, file, props, ':write-allowed', expected.writeAllowed);
  requireListItems(diagnostics, file, props, ':task-classes', expected.taskClasses);
}

function validateAgyWorker(file, worker, diagnostics) {
  if (!worker) {
    diagnostics.push({ file, message: 'workstation-pool missing agy-research worker' });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'agy');
  requirePropText(diagnostics, file, props, ':role', 'researcher');
  requirePropText(diagnostics, file, props, ':slot-id', 'slot-agy-research');
  requirePropText(diagnostics, file, props, ':task-type', 'agy_research');
  requirePropBool(diagnostics, file, props, ':accepts-boardtask', true);
  requirePropBool(diagnostics, file, props, ':write-allowed', false);
  requireListItems(diagnostics, file, props, ':task-classes', ['research', 'review', 'context-pack']);
  requireListItems(diagnostics, file, props, ':capabilities', ['read-only', 'analysis', 'design-review']);
}

function validateClaudeWorker(file, worker, diagnostics) {
  if (!worker) {
    diagnostics.push({ file, message: 'workstation-pool missing claude-code-default worker' });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'claude-code');
  requirePropText(diagnostics, file, props, ':role', 'coder');
  requirePropText(diagnostics, file, props, ':slot-id', 'slot-claude-code-default');
  requirePropText(diagnostics, file, props, ':task-type', 'claude_code_default');
  requirePropText(diagnostics, file, props, ':model-profile', 'coding-default-opus-4-7');
  requirePropText(diagnostics, file, props, ':model', 'nil');
  requirePropBool(diagnostics, file, props, ':accepts-boardtask', true);
  requirePropBool(diagnostics, file, props, ':write-allowed', true);
  requireListItems(diagnostics, file, props, ':task-classes', ['code', 'implementation', 'ops']);
  requireListItems(diagnostics, file, props, ':capabilities', ['code-read', 'code-write', 'mcp']);
}

function validateDeployOpsWorker(file, worker, diagnostics) {
  if (!worker) {
    diagnostics.push({ file, message: 'workstation-pool missing claude-code-deploy-ops worker' });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'claude-code');
  requirePropText(diagnostics, file, props, ':role', 'deploy-ops');
  requirePropText(diagnostics, file, props, ':slot-id', 'slot-claude-code-deploy-ops');
  requirePropText(diagnostics, file, props, ':task-type', 'claude_code_deploy_ops');
  requirePropText(diagnostics, file, props, ':model-profile', 'coding-default-opus-4-7');
  requirePropText(diagnostics, file, props, ':model', 'nil');
  requirePropBool(diagnostics, file, props, ':accepts-boardtask', true);
  requirePropBool(diagnostics, file, props, ':write-allowed', false);
  requireListItems(diagnostics, file, props, ':task-classes', ['deploy-ops', 'deployment', 'ops']);
  requireListItems(diagnostics, file, props, ':capabilities', ['deploy-read', 'deploy-center-query', 'mcp']);
}

function validateGeminiWorker(file, worker, diagnostics) {
  if (!worker) {
    diagnostics.push({ file, message: 'workstation-pool missing gemini-ultra-pro worker' });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'gemini');
  requirePropText(diagnostics, file, props, ':role', 'researcher');
  requirePropText(diagnostics, file, props, ':slot-id', 'slot-gemini-ultra');
  requirePropText(diagnostics, file, props, ':task-type', 'gemini_ultra');
  requirePropText(diagnostics, file, props, ':model-profile', 'gemini-ultra-pro-preview');
  requirePropText(diagnostics, file, props, ':model', 'nil');
  requirePropText(diagnostics, file, props, ':approval-policy', 'plan');
  requirePropText(
    diagnostics,
    file,
    props,
    ':tool-policy-path',
    '.missiond/v3/policies/gemini-readonly-policy.toml',
  );
  requirePropBool(diagnostics, file, props, ':accepts-boardtask', true);
  requirePropBool(diagnostics, file, props, ':write-allowed', false);
  requireListItems(diagnostics, file, props, ':task-classes', [
    'research',
    'review',
    'context-pack',
    'lisp-compression',
  ]);
  requireListItems(diagnostics, file, props, ':capabilities', ['read-only', 'analysis', 'design-review']);
}

function validateFastPatchWorker(file, worker, diagnostics) {
  if (!worker) {
    diagnostics.push({ file, message: 'workstation-pool missing claude-code-fast-patch worker' });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'claude-code');
  requirePropText(diagnostics, file, props, ':role', 'patcher');
  requirePropText(diagnostics, file, props, ':model-profile', 'daily-sonnet');
  requirePropBool(diagnostics, file, props, ':accepts-boardtask', true);
  requirePropBool(diagnostics, file, props, ':write-allowed', true);
  requireListItems(diagnostics, file, props, ':task-classes', ['patch', 'low-risk-fast-path']);
  requireListItems(diagnostics, file, props, ':capabilities', ['narrow-patch', 'scoped-commit']);
}

function validateGeminiFastWorker(file, worker, diagnostics) {
  if (!worker) {
    diagnostics.push({ file, message: 'workstation-pool missing gemini-fast-survey worker' });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'gemini');
  requirePropText(diagnostics, file, props, ':role', 'survey');
  requirePropText(diagnostics, file, props, ':model', 'gemini-2.5-flash');
  requirePropText(diagnostics, file, props, ':approval-policy', 'plan');
  requirePropText(
    diagnostics,
    file,
    props,
    ':tool-policy-path',
    '.missiond/v3/policies/gemini-readonly-policy.toml',
  );
  requirePropBool(diagnostics, file, props, ':write-allowed', false);
  requireListItems(diagnostics, file, props, ':task-classes', ['survey', 'mechanical-scan']);
}

function validateCodexMasterWorker(file, worker, diagnostics) {
  if (!worker) {
    diagnostics.push({ file, message: 'workstation-pool missing codex-master-control worker' });
    return;
  }
  const props = readKeywordProps(worker, { start: 2 });
  requirePropText(diagnostics, file, props, ':engine', 'codex');
  requirePropText(diagnostics, file, props, ':role', 'orchestrator');
  requirePropText(diagnostics, file, props, ':slot-id', 'slot-codex-master-control');
  requirePropText(diagnostics, file, props, ':model-profile', 'codex-master-gpt-5-5-xhigh');
  requirePropText(diagnostics, file, props, ':reasoning-effort', 'xhigh');
  requirePropText(diagnostics, file, props, ':sandbox', 'danger-full-access');
  requirePropText(diagnostics, file, props, ':approval-policy', 'never');
  requirePropBool(diagnostics, file, props, ':search', true);
  requirePropBool(diagnostics, file, props, ':accepts-boardtask', false);
  requirePropBool(diagnostics, file, props, ':write-allowed', true);
  requireListItems(diagnostics, file, props, ':capabilities', ['board-write', 'kb-write', 'dispatch']);
}

function requirePropText(diagnostics, file, props, key, expected) {
  const actual = keywordPropText(props, key);
  if (actual !== expected) {
    diagnostics.push({ file, message: `${key} must be ${expected}, got ${actual ?? '<missing>'}` });
  }
}

function requirePropBool(diagnostics, file, props, key, expected) {
  const actual = keywordPropBool(props, key);
  if (actual !== expected) {
    diagnostics.push({ file, message: `${key} must be ${expected}, got ${actual ?? '<missing>'}` });
  }
}

function requireListItems(diagnostics, file, props, key, requiredItems) {
  const values = nodeToStringArray(props[key]?.value);
  for (const item of requiredItems) {
    if (!values.includes(item)) {
      diagnostics.push({ file, message: `${key} missing ${item}` });
    }
  }
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required text: ${needle}` });
    }
  }
}

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden text present: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-workstation-pool-'));
  for (const rel of Object.values(FILES)) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  }
  write(root, 'blueprint', `
(missiond-blueprint
  (workstation-pool
    :evidence ".missiond/v3/evidence/workstation-pool.lisp"
    (worker claude-code-default
      :engine claude-code
      :role coder
      :slot-id "slot-claude-code-default"
      :task-type claude_code_default
      :model-profile coding-default-opus-4-7
      :model nil
      :task-classes [code implementation review context-pack ops]
      :capabilities [code-read code-write scoped-commit mcp]
      :max-concurrency 1
      :timeout-secs 1800
      :default-use code-implementation
      :accepts-boardtask true
      :write-allowed true)
    (worker claude-code-deploy-ops
      :engine claude-code
      :role deploy-ops
      :slot-id "slot-claude-code-deploy-ops"
      :task-type claude_code_deploy_ops
      :model-profile coding-default-opus-4-7
      :model nil
      :task-classes [deploy-ops deployment ops incident-response]
      :capabilities [deploy-read deploy-observe deploy-center-query rollback-plan mcp]
      :max-concurrency 1
      :timeout-secs 2400
      :default-use deployment-operations
      :accepts-boardtask true
      :write-allowed false)
    (worker claude-code-fast-patch
      :engine claude-code
      :role patcher
      :model-profile daily-sonnet
      :task-classes [patch test chore low-risk-fast-path]
      :capabilities [code-read code-write scoped-commit narrow-patch mcp]
      :max-concurrency 1
      :timeout-secs 900
      :default-use narrow-patch
      :accepts-boardtask true
      :write-allowed true)
    (worker gemini-ultra-pro
      :engine gemini
      :role researcher
      :slot-id "slot-gemini-ultra"
      :task-type gemini_ultra
      :model-profile gemini-ultra-pro-preview
      :model nil
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [research review context-pack lisp-compression general]
      :capabilities [read-only analysis design-review]
      :max-concurrency 1
      :timeout-secs 900
      :default-use research-review
      :accepts-boardtask true
      :write-allowed false)
    (worker gemini-fast-survey
      :engine gemini
      :role survey
      :slot-id "slot-gemini-fast-survey"
      :task-type gemini_fast_survey
      :model-profile nil
      :model "gemini-2.5-flash"
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [survey summary mechanical-scan]
      :capabilities [read-only summary]
      :max-concurrency 1
      :timeout-secs 600
      :default-use low-authority-survey
      :accepts-boardtask true
      :write-allowed false)
    (worker codex-master-control
      :engine codex
      :role orchestrator
      :slot-id "slot-codex-master-control"
      :model-profile codex-master-gpt-5-5-xhigh
      :reasoning-effort xhigh
      :search true
      :sandbox danger-full-access
      :approval-policy never
      :task-classes [master-control]
      :capabilities [board-write kb-write dispatch]
      :max-concurrency 1
      :timeout-secs 7200
      :default-use resident-master-control
      :accepts-boardtask false
      :write-allowed true)
    :invariants ["Supervisor patrol (slot-supervisor) is gated on V3 workstation-pool / runtime-config registration"
                 "V3 workstation-pool (plus startup-slots) is authoritative for dispatchable slots"
                 "mission_compute_slot list status MUST derive from PTYManager"
                 "mission_slots and /api/slots MUST project activeBoardTaskId/currentTaskId and activeBoardTask"
                 "Codex master-control is a resident orchestrator lane"
                 "Read-only Gemini pool workers MUST project to Gemini CLI \`--approval-mode plan --policy .missiond/v3/policies/gemini-readonly-policy.toml\`"]
    :checker "node scripts/check-v3-workstation-pool-isomorphism.mjs")
  (implementation-map
    (surface workstation-pool
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-pty/src/session.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "scripts/check-v3-workstation-pool-isomorphism.mjs"]
      :note "n")))`);
  write(root, 'evidence', `
(workstation-pool-evidence
  :single-login-phase ((claude-code-default :runtime-rule "x") (claude-code-deploy-ops :runtime-rule "x") (claude-code-fast-patch :runtime-rule "x") (gemini-ultra-pro :write-policy read-only :approval-mode plan :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml") (gemini-fast-survey :write-policy read-only :approval-mode plan :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml") (codex-master-control :runtime-rule "x"))
  :observability ["mission_compute_slot action=list exposes workstation_pool"])`);
  write(root, 'geminiPolicy', `
toolName = ["generalist", "codebase_investigator", "invoke_agent", "invoke_subagent"]
toolName = [
"replace"
"write_file"
"run_shell_command"
"exit_plan_mode"
]
decision = "deny"
modes = ["plan"]`);
  write(root, 'runtime', `
pub(crate) struct WorkstationPoolRuntimeConfig;
pub(crate) workstation_pool: Vec<WorkstationPoolRuntimeConfig>;
pub(crate) fn workstation_pool(&self) -> &[WorkstationPoolRuntimeConfig] {}
pub(crate) fn boardtask_pool_candidates() {}
find_form(source, "workstation-pool");
"workstation-pool must include a Claude Code default BoardTask worker";
"workstation-pool must include a read-only Gemini BoardTask worker";
"workstation-pool must include a non-shard Codex master-control worker";
"claude-code-default"; "claude-code-deploy-ops"; "gemini-ultra-pro"; "codex-master-control";
reasoning_effort; search_enabled; approval_policy; tool_policy_path;
"read-only Gemini workstation-pool workers must declare :tool-policy-path";`);
  write(root, 'main', `
fn workstation_pool_model() {}
fn startup_slot_config() {}
fn workstation_pool_slot_config() {}
reasoning_effort: worker.reasoning_effort.clone();
search_enabled: Some(worker.search_enabled).filter;
fn missiond_managed_skip_permissions() {}
dangerously_skip_permissions: Some(missiond_managed_skip_permissions(engine, false));
tool_policy_path: worker.tool_policy_path.clone();
skip_permissions: missiond_managed_skip_permissions(engine, false);
async fn register_and_init_runtime_slot() { state.pty.init_slot(&pty_slot).await; state.mission.register_runtime_slot(slot_config); }
for worker in workstation_config.workstation_pool() {}
"SlotManager: workstation pool registered from V3";`);
  write(root, 'projectTypes', `
use std::path::Path;
let cwd_path = Path::new(cwd);
cwd_path.starts_with(Path::new(prefix));
fn resolve_does_not_match_sibling_by_string_prefix() {}
"/Users/rickyhq/Projects/missiond-clean";`);
  write(root, 'genericCli', `
GenericCliSlotManager;
PTYSpawnOptions;
reasoning_effort: req.reasoning_effort.clone();
search_enabled: req.search_enabled;
sandbox: req.sandbox.clone();
approval_policy: req.approval_policy.clone();
tool_policy_path: req.tool_policy_path.clone();
canonical_source_for_engine(self.engine);
TextComplete;`);
  write(root, 'controlTree', `
Agy,
Self::Agy => None;`);
  write(root, 'ptyRecognition', `
CliEngine::Agy => recognize_agy(lines);
fn recognize_agy() {}
agy_idle_screen_is_idle;
agy_auth_or_quota_error_is_blocked;`);
  write(root, 'ptySession', `
CliEngine::Gemini;
--approval-mode plan;
--policy;
--approval-mode yolo;
tool_policy_path: Option<&std::path::Path>;
Gemini CLI: read-only tool policy enabled;
Gemini CLI: plan/read-only approval mode enabled;
gemini_command_uses_plan_mode_unless_permissions_are_skipped;`);
  write(root, 'autopilot', `
fn board_task_workstation_class() {}
async fn select_workstation_pool_slot() {}
workstation_config.boardtask_pool_candidates(task_class);
task_class == "code" && !worker.write_allowed;
SessionState::Exited;
SessionState::Error;
"Autopilot: selected V3 workstation-pool slot";`);
  write(root, 'computeSlot', `
let workstation_pool = match WorkstationRuntimeConfig::load_for_current_dir() {};
"workstation_pool": workstation_pool;
"runtime_slot_present": runtime_slot_present;
"V3_BLUEPRINT_CONFIG_ERROR";
fn classify_static_slot() {}
fn derive_static_status() {}
StaticSlotClass {}
"legacy_static_slots";
"dispatchable":;
"legacy":;
"v3_authoritative";
let pty_status = state
            .pty
            .get_status(&s.config.id)
            .await;`);
  write(root, 'slotTool', `
fn projected_mission_slots() {}
WorkstationRuntimeConfig::load_for_current_dir();
list_board_tasks(Some("running"), true);
fn active_board_task_for_slot() {}
"activeBoardTaskId"; "currentTaskId"; "activeBoardTask";
get_running_slot_task(&slot.config.id);
fn is_stopped_legacy_sonnet_residual() {}
model.contains("sonnet");
v3_slot_ids.contains(&slot.config.id);`);
  write(root, 'wsServer', `
async fn handle_slot_status() {}
list_board_tasks(Some("running"), true);
fn active_board_task_for_slot_status() {}
fn board_task_status_summary_json() {}
"activeBoardTaskId"; "currentTaskId"; "activeBoardTask";`);
  write(root, 'slotManager', `
pub fn register_runtime_slot(&self, mut config: SlotConfig) {}
"Runtime slot registered";`);
  write(root, 'missionControl', `
pub fn register_runtime_slot(&self, config: SlotConfig) { self.slot_manager.register_runtime_slot(config); }`);
  write(root, 'supervisor', `
pub(crate) async fn schedule_supervisor_patrol(state: &AppState) {
    let supervisor_registered = state.mission.list_slots().into_iter().any(|s| s.config.id == SUPERVISOR_SLOT_ID);
    if !supervisor_registered { return; }
}`);
  write(root, 'aggregate', `
'workstation-pool';
scripts/check-v3-workstation-pool-isomorphism.mjs;`);
  return root;
}

function write(root, key, source) {
  fs.writeFileSync(path.join(root, FILES[key]), source.trimStart());
}

main();
