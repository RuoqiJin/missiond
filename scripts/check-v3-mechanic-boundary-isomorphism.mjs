#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-mechanic-boundary-isomorphism.mjs [--json]

Checks the V3 Jarvis Mechanic collaboration boundary:
  - jarvis-mechanic is registered as a project-local SSOT devtool.
  - MissionD owns orchestration, Board, event bus, Universe, dispatch, and approval.
  - Mechanic is declared as an opt-in repair executor lane only.
  - Runtime wiring is restricted to mission_task_delegate engine_hint=mechanic,
    accepted_shard_id, context_pack_path, write_scope, and task-result-artifact.
`;

const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';

function main() {
  const args = process.argv.slice(2);
  const json = args.includes('--json');
  if (args.includes('--help') || args.includes('-h')) {
    console.log(usage);
    process.exit(0);
  }
  if (args.some((arg) => !['--json', '--help', '-h'].includes(arg))) {
    console.error(usage);
    process.exit(2);
  }

  const root = process.cwd();
  const diagnostics = [];
  const blueprint = readBlueprintWithEvidenceSidecars(root, BLUEPRINT);

  requireAll(diagnostics, BLUEPRINT, blueprint, [
    '(project :id jarvis-mechanic',
    ':root "/Users/jinchen/Projects/jarvis-mechanic"',
    ':intent ".missiond/intent.lisp"',
    'node scripts/check-mechanic-ssot.mjs',
    'bash .missiond/check.sh',
    'opt-in repair executor CLI, not a MissionD orchestrator or automatic runtime worker',
    '(mechanic-collaboration-boundary',
    ':schema "missiond.mechanic-collaboration-boundary.v1"',
    ':status mechanic-executor-lane-enabled',
    ':owner missiond',
    ':executor jarvis-mechanic',
    'MissionD owns workflows, Board, event bus, project registry, Universe, worker dispatch, checkpoints, and approval',
    ':missiond-responsibilities [ssot-universe workflow-lisp boardtask-lifecycle eventbus resident-master-control swarm-dispatch approval checkpoint verification]',
    ':mechanic-responsibilities [compiler-as-judge-repair narrow-code-transform dry-run-diagnostics patch-proposal repair-report]',
    ':runtime-policy (:enabled true',
    ':default-mode dry-run',
    ':delegate-entry mission_task_delegate',
    ':engine-hint mechanic',
    ':allowed-entrypoints [approved-repair-shard dry-run-diagnostics]',
    ':forbidden-entrypoints [resident-master-control night-scheduler nightly-evolution-loop autonomous-orchestrator generic-worker-pool]',
    ':handoff-contract (:entry [BoardTaskApproved accepted_shard_id context_pack_path write_scope acceptance mechanic_mode]',
    'Mechanic never performs broad architecture discovery',
    'MissionD creates an accepted exact repair shard',
    'mission_task_delegate routes engine_hint=mechanic only when accepted_shard_id, context_pack_path, and write_scope are present',
    'mechanic implementation requests without exact shard metadata are rejected before BoardTask creation',
    'visible non-autoExecute BoardTask',
    'Autopilot/PTY dispatch cannot reroute mechanic work to ClaudeCode',
    'Mechanic runs compiler-as-judge repair only inside the approved write_scope',
    'task-result-artifact',
    'MissionD verifies diff, checker, tests, commit/Lisp convergence',
    'Mechanic MUST NOT be called by nightly-evolution by default.',
    'Mechanic MUST NOT become a resident master or project orchestrator.',
    'standalone mission_mechanic tools remain forbidden',
    'node scripts/check-v3-mechanic-boundary-isomorphism.mjs',
  ]);

  requireAll(diagnostics, 'crates/missiond-mcp/src/tools/compute/task_delegate.rs',
    readIfExists(root, 'crates/missiond-mcp/src/tools/compute/task_delegate.rs'), [
      '"engine_hint": {"type": "string"',
      '"mechanic_mode"',
      '"mechanic_target"',
      '"mechanic_bin"',
    ]);

  requireAll(diagnostics, 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs',
    readIfExists(root, 'crates/missiond-daemon/src/handlers/compute/task_delegate.rs'), [
      'engine_hint_is_mechanic',
      'parse_mechanic_run_config',
      'spawn_mechanic_repair',
      'mechanic_mode',
      'auto_execute: Some(!mechanic_config.is_some() && !xjpcode_worker)',
      'if mechanic_config.is_none()',
      'must not enter the normal Autopilot/PTY dispatcher',
      'mechanic_implementation_requires_exact_shard_metadata_even_without_write_scope',
      'EXACT_SHARD_WRITE_SCOPE_REQUIRED',
      'task-result-artifact',
      'worker_settle',
      'MECHANIC_PROJECT_ROOT_REQUIRED',
    ]);

  forbidStandaloneMechanicTool(diagnostics, root, [
    'crates/missiond-mcp/src/tools',
    'crates/missiond-daemon/src/handlers',
  ]);

  const result = {
    ok: diagnostics.length === 0,
    diagnostics,
  };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 mechanic boundary isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 mechanic boundary isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required text: ${needle}` });
    }
  }
}

function readIfExists(root, relFile) {
  const file = path.join(root, relFile);
  if (!fs.existsSync(file)) return '';
  return fs.readFileSync(file, 'utf8');
}

function forbidStandaloneMechanicTool(diagnostics, root, relDirs) {
  for (const relDir of relDirs) {
    const dir = path.join(root, relDir);
    if (!fs.existsSync(dir)) continue;
    for (const file of walk(dir)) {
      if (!file.endsWith('.rs')) continue;
      const source = fs.readFileSync(file, 'utf8');
      if (source.includes('mission_mechanic')) {
        diagnostics.push({
          file: path.relative(root, file),
          message: 'standalone mission_mechanic tool wiring is forbidden; mechanic must route through mission_task_delegate engine_hint=mechanic',
        });
      }
    }
  }
}

function* walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      yield* walk(abs);
    } else {
      yield abs;
    }
  }
}

main();
