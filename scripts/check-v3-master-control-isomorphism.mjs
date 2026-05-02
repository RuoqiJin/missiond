#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  frontendBlueprint: '.missiond/frontend/board-blueprint.lisp',
  runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  slotHandler: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  mcpProcessTools: 'crates/missiond-mcp/src/tools/compute/process.rs',
  mcpToolsMod: 'crates/missiond-mcp/src/tools/mod.rs',
  slotTypes: 'crates/missiond-core/src/types/slot.rs',
  ptyManager: 'crates/missiond-pty/src/manager.rs',
  ptySession: 'crates/missiond-pty/src/session.rs',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
};

function main() {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = fs.readFileSync(path.join(process.cwd(), rel), 'utf8');
    } catch (err) {
      diagnostics.push(`${rel}: cannot read: ${err.message}`);
    }
  }
  if (diagnostics.length === 0) check(sources, diagnostics);
  if (diagnostics.length > 0) {
    diagnostics.forEach((d) => console.error(d));
    console.error(`v3 master-control isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
    process.exit(1);
  }
  console.log('v3 master-control isomorphism check OK');
}

function check(s, diagnostics) {
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(resident-master-control',
    ':worker codex-master-control',
    ':slot-id "slot-codex-master-control"',
    ':model-profile codex-master-gpt-5-5-xhigh',
    ':model "gpt-5.5"',
    ':reasoning-effort xhigh',
    ':checkpoint',
    ':event-subscriptions',
    ':evidence-authority',
    ':settle-policy',
    'mission_master_status',
    'node scripts/check-v3-master-control-isomorphism.mjs',
    '(surface resident-master-control',
  ]);
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(worker codex-master-control',
    ':engine codex',
    ':role orchestrator',
    ':search true',
    ':sandbox read-only',
    ':approval-policy never',
    ':accepts-boardtask false',
    ':write-allowed false',
  ]);
  requireAll(diagnostics, FILES.runtime, s.runtime, [
    'DEFAULT_CODEX_MASTER_PROFILE',
    '"codex-master-control"',
    '"slot-codex-master-control"',
    'reasoning_effort: Some("xhigh".to_string())',
    'search_enabled: true',
    'workstation-pool must include a non-shard Codex master-control worker',
  ]);
  requireAll(diagnostics, FILES.main, s.main, [
    'reasoning_effort: worker.reasoning_effort.clone()',
    'search_enabled: Some(worker.search_enabled).filter',
    'approval_policy: worker.approval_policy.clone()',
    'maybe_write_master_control_startup_checkpoint',
    'master-control-checkpoint.lisp',
  ]);
  requireAll(diagnostics, FILES.slotHandler, s.slotHandler, [
    '"mission_master_status"',
    'mission_master_status(state).await',
    '"missiond.master-status.v1"',
    '"codex-master-control"',
    '"slot-codex-master-control"',
    'pty_recognition_snapshot',
    'master-control-checkpoint.lisp',
    'checkpoint_root',
    'slot.config.project_root.clone().or(slot.config.cwd.clone())',
  ]);
  requireAll(diagnostics, FILES.mcpProcessTools, s.mcpProcessTools, [
    '"mission_master_status"',
    '常驻 Codex master-control 状态',
  ]);
  requireAll(diagnostics, FILES.mcpToolsMod, s.mcpToolsMod, [
    'test_master_status_surface_registered',
    'mission_master_status not registered',
  ]);
  requireAll(diagnostics, FILES.slotTypes, s.slotTypes, [
    'pub model_profile: Option<String>',
    'pub reasoning_effort: Option<String>',
    'pub search_enabled: Option<bool>',
    'pub sandbox: Option<String>',
    'pub approval_policy: Option<String>',
  ]);
  requireAll(diagnostics, FILES.ptyManager, s.ptyManager, [
    'pub reasoning_effort: Option<String>',
    'pub search_enabled: bool',
    'pub approval_policy: Option<String>',
  ]);
  requireAll(diagnostics, FILES.ptySession, s.ptySession, [
    'model_reasoning_effort',
    '--search',
    '--sandbox',
    '--ask-for-approval',
    'codex_command_projects_model_reasoning_search_sandbox_and_approval',
  ]);
  requireAll(diagnostics, FILES.frontendBlueprint, s.frontendBlueprint, [
    'codex-master-control',
    'Codex master and Gemini PTYs must be selectable exactly like ClaudeCode PTYs',
  ]);
  requireAll(diagnostics, FILES.aggregate, s.aggregate, [
    'scripts/check-v3-master-control-isomorphism.mjs',
  ]);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(`${file}: missing required text: ${needle}`);
  }
}

main();
