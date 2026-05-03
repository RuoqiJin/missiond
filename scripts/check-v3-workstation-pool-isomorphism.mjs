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
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/check-v3-workstation-pool-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 workstation-pool Lisp/code isomorphism contract:
  - V3 declares Claude Code Default, Claude fast-patch, Gemini, and Codex master workers.
  - Claude coding default remains coding-default-opus-4-7, which omits --model.
  - Gemini remains read-only until a separate write smoke promotes it.
  - Rust projects the pool into SlotManager, MissionControl runtime slots,
    Autopilot BoardTask routing, and mission_compute_slot observability.
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  evidence: '.missiond/v3/evidence/workstation-pool.lisp',
  runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
  computeSlot: 'crates/missiond-daemon/src/handlers/compute/compute_slot.rs',
  slotTool: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  slotManager: 'crates/missiond-core/src/core/slot_manager.rs',
  missionControl: 'crates/missiond-core/src/core/mission_control.rs',
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
    'claude-code-fast-patch',
    'gemini-ultra-pro',
    'gemini-fast-survey',
    'codex-master-control',
    'read-only',
    'mission_compute_slot action=list exposes workstation_pool',
  ]);

  requireAll(diagnostics, FILES.runtime, sources.runtime, [
    'pub(crate) struct WorkstationPoolRuntimeConfig',
    'workstation_pool: Vec<WorkstationPoolRuntimeConfig>',
    'pub(crate) fn workstation_pool(&self) -> &[WorkstationPoolRuntimeConfig]',
    'pub(crate) fn boardtask_pool_candidates',
    'find_form(source, "workstation-pool")',
    'workstation-pool must include a Claude Code default BoardTask worker',
    'workstation-pool must include a read-only Gemini BoardTask worker',
    'workstation-pool must include a non-shard Codex master-control worker',
    '"claude-code-default"',
    '"gemini-ultra-pro"',
    '"codex-master-control"',
    'reasoning_effort',
    'search_enabled',
  ]);

  requireAll(diagnostics, FILES.main, sources.main, [
    'fn workstation_pool_model',
    'fn startup_slot_config',
    'fn workstation_pool_slot_config',
    'reasoning_effort: worker.reasoning_effort.clone()',
    'search_enabled: Some(worker.search_enabled).filter',
    'async fn register_and_init_runtime_slot',
    'state.pty.init_slot(&pty_slot).await',
    'for worker in workstation_config.workstation_pool()',
    'state.mission.register_runtime_slot(slot_config)',
    'SlotManager: workstation pool registered from V3',
  ]);

  requireAll(diagnostics, FILES.autopilot, sources.autopilot, [
    'fn board_task_workstation_class',
    'async fn select_workstation_pool_slot',
    'workstation_config.boardtask_pool_candidates(task_class)',
    'task_class == "code" && !worker.write_allowed',
    'SessionState::Exited',
    'SessionState::Error',
    'Autopilot: selected V3 workstation-pool slot',
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
    'mission_slots MUST project activeBoardTaskId/currentTaskId and activeBoardTask',
    'Codex master-control is a resident orchestrator lane',
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
  validateFastPatchWorker(file, byId.get('claude-code-fast-patch'), diagnostics);
  validateGeminiWorker(file, byId.get('gemini-ultra-pro'), diagnostics);
  validateGeminiFastWorker(file, byId.get('gemini-fast-survey'), diagnostics);
  validateCodexMasterWorker(file, byId.get('codex-master-control'), diagnostics);

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
    'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
    'crates/missiond-daemon/src/handlers/compute/compute_slot.rs',
    'crates/missiond-daemon/src/handlers/compute/slot.rs',
    'scripts/check-v3-workstation-pool-isomorphism.mjs',
  ]) {
    if (!codeRefs.includes(required)) {
      diagnostics.push({ file, message: `workstation-pool surface missing code ref: ${required}` });
    }
  }
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
    (worker gemini-ultra
      :engine gemini
      :role researcher
      :slot-id "slot-gemini-ultra"
      :task-type gemini_ultra
      :model-profile nil
      :model nil
      :task-classes [research review context-pack lisp-compression general]
      :capabilities [read-only analysis design-review]
      :max-concurrency 1
      :timeout-secs 900
      :default-use research-review
      :accepts-boardtask true
      :write-allowed false)
    :invariants ["Supervisor patrol (slot-supervisor) is gated on V3 workstation-pool / runtime-config registration"
                 "V3 workstation-pool (plus startup-slots) is authoritative for dispatchable slots"
                 "mission_compute_slot list status MUST derive from PTYManager"]
    :checker "node scripts/check-v3-workstation-pool-isomorphism.mjs")
  (implementation-map
    (surface workstation-pool
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "scripts/check-v3-workstation-pool-isomorphism.mjs"]
      :note "n")))`);
  write(root, 'evidence', `
(workstation-pool-evidence
  :single-login-phase ((claude-code-default :runtime-rule "x") (gemini-ultra :write-policy read-only))
  :observability ["mission_compute_slot action=list exposes workstation_pool"])`);
  write(root, 'runtime', `
pub(crate) struct WorkstationPoolRuntimeConfig;
pub(crate) workstation_pool: Vec<WorkstationPoolRuntimeConfig>;
pub(crate) fn workstation_pool(&self) -> &[WorkstationPoolRuntimeConfig] {}
pub(crate) fn boardtask_pool_candidates() {}
find_form(source, "workstation-pool");
"workstation-pool must include a Claude Code default BoardTask worker";
"workstation-pool must include a read-only Gemini BoardTask worker";
"claude-code-default"; "gemini-ultra";`);
  write(root, 'main', `
fn workstation_pool_model() {}
fn startup_slot_config() {}
fn workstation_pool_slot_config() {}
async fn register_and_init_runtime_slot() { state.pty.init_slot(&pty_slot).await; state.mission.register_runtime_slot(slot_config); }
for worker in workstation_config.workstation_pool() {}
"SlotManager: workstation pool registered from V3";`);
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
fn is_stopped_legacy_sonnet_residual() {}
model.contains("sonnet");
v3_slot_ids.contains(&slot.config.id);`);
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
