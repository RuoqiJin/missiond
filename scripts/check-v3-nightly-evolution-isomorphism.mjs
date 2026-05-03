#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workflow: '.missiond/workflows/nightly-evolution.lisp',
  runtime: 'crates/missiond-daemon/src/engine/nightly_evolution.rs',
  engineMod: 'crates/missiond-daemon/src/engine/mod.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  slotHandler: 'crates/missiond-daemon/src/handlers/compute/slot.rs',
  daemonHandlers: 'crates/missiond-daemon/src/handlers/mod.rs',
  masterControl: 'crates/missiond-daemon/src/engine/master_control.rs',
  mcpProcessTools: 'crates/missiond-mcp/src/tools/compute/process.rs',
  mcpGateway: 'crates/missiond-mcp/src/gen_gateway.rs',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
  gitignore: '.gitignore',
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
    console.error(`v3 nightly-evolution check FAILED -- ${diagnostics.length} diagnostic(s)`);
    process.exit(1);
  }
  console.log('v3 nightly-evolution check OK');
}

function check(s, diagnostics) {
  requireAll(diagnostics, FILES.workflow, s.workflow, [
    '(workflow nightly-evolution',
    ':workflow_id nightly-evolution',
    'mission_nightly_evolution',
    'observe-only',
    'safe-backfill',
    'needs-investigation',
    'architecture-proposal',
    'requires-user-decision',
    'missiond-v3-blueprint',
    'final-convergence-static-snapshot',
    'recent-v3-commits',
    'Default nightly mode does not read KB, historical conversations, provider logs, worker telemetry, or Board open tasks',
    'Check only MissionD V3 SSOT logic',
    'select only findings whose class matches the requested mode',
    '.missiond/v3/runtime/nightly-evolution/<date>.report.lisp',
    'must not hide, delete, or bulk-mutate historical tasks',
    'no KB task or memory mutation is created in default mode',
  ]);
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(nightly-evolution-loop',
    '(function nightly-evolution',
    ':surface nightly-evolution-loop',
    ':policy (:workflow ".missiond/workflows/nightly-evolution.lisp"',
    ':default-mode observe-only',
    ':risk-gate',
    'requested mode matches finding class',
    '(surface nightly-evolution-loop',
    'crates/missiond-daemon/src/engine/nightly_evolution.rs',
    'mission_nightly_evolution',
    'scripts/check-v3-nightly-evolution-isomorphism.mjs',
  ]);
  requireAll(diagnostics, FILES.runtime, s.runtime, [
    'NIGHTLY_REPORT_DIR',
    'NightlyEvolutionRuntime',
    'start_nightly_evolution_service',
    'MISSIOND_NIGHTLY_EVOLUTION_INTERVAL_SECS',
    'interval.tick().await;',
    'mission_nightly_evolution',
    'NightlyEvolutionArgs',
    'read_final_convergence_snapshot',
    'check-v3-final-convergence.mjs',
    '--static-only',
    'read_recent_commits',
    '".missiond/v3"',
    'build_findings',
    'v3-runtime-projection-review',
    'v3-surface-checker-drift',
    'v3-logic-consistency-review',
    'v3-lisp-density-review',
    'select_requested_followup',
    'create_requested_followup_if_needed',
    'followup_selection_respects_requested_mode',
    'auto_execute: Some(false)',
    'hidden: Some(false)',
    'status_snapshot',
    'nightly-evolution-report',
    'findings_focus_only_on_v3_ssot_topics',
  ]);
  requireAll(diagnostics, FILES.engineMod, s.engineMod, ['pub mod nightly_evolution;']);
  requireAll(diagnostics, FILES.main, s.main, [
    'engine::nightly_evolution::start_nightly_evolution_service',
  ]);
  requireAll(diagnostics, FILES.slotHandler, s.slotHandler, [
    '"mission_nightly_evolution"',
    'nightly_evolution::mission_nightly_evolution(state, args).await',
  ]);
  requireAll(diagnostics, FILES.daemonHandlers, s.daemonHandlers, ['"mission_nightly_evolution"']);
  requireAll(diagnostics, FILES.masterControl, s.masterControl, [
    'nightlyEvolution',
    'crate::engine::nightly_evolution::status_snapshot().await',
  ]);
  requireAll(diagnostics, FILES.mcpProcessTools, s.mcpProcessTools, [
    '"mission_nightly_evolution"',
    'nightly evolution workflow',
    'observe-only',
  ]);
  requireAll(diagnostics, FILES.mcpGateway, s.mcpGateway, ['"mission_nightly_evolution"']);
  requireAll(diagnostics, FILES.aggregate, s.aggregate, [
    "'nightly-evolution-loop'",
    'scripts/check-v3-nightly-evolution-isomorphism.mjs',
  ]);
  requireAll(diagnostics, FILES.gitignore, s.gitignore, [
    '.missiond/v3/runtime/nightly-evolution/*.report.lisp',
  ]);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(`${file}: missing required text: ${needle}`);
  }
}

main();
