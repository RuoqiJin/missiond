#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-mechanic-boundary-isomorphism.mjs [--json]

Checks the V3 Jarvis Mechanic collaboration boundary:
  - jarvis-mechanic is registered as a project-local SSOT devtool.
  - MissionD owns orchestration, Board, event bus, Universe, dispatch, and approval.
  - Mechanic is declared as an opt-in repair executor only.
  - Runtime wiring stays disabled until a future explicit Lisp/checker change.
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
    ':status declared-not-runtime-enabled',
    ':owner missiond',
    ':executor jarvis-mechanic',
    'MissionD owns workflows, Board, event bus, project registry, Universe, worker dispatch, checkpoints, and approval',
    ':missiond-responsibilities [ssot-universe workflow-lisp boardtask-lifecycle eventbus resident-master-control swarm-dispatch approval checkpoint verification]',
    ':mechanic-responsibilities [compiler-as-judge-repair narrow-code-transform dry-run-diagnostics patch-proposal repair-report]',
    ':runtime-policy (:enabled false',
    ':default-mode disabled',
    ':allowed-entrypoints [approved-repair-shard dry-run-diagnostics]',
    ':forbidden-entrypoints [resident-master-control night-scheduler nightly-evolution-loop autonomous-orchestrator generic-worker-pool]',
    ':handoff-contract (:entry [BoardTaskApproved repair-shard write_scope acceptance]',
    'MissionD creates an exact repair shard',
    'Mechanic runs compiler-as-judge repair only inside the approved write_scope',
    'MissionD verifies diff, checker, tests, and commit/Lisp convergence',
    'Mechanic MUST NOT be called by nightly-evolution by default.',
    'Mechanic MUST NOT become a resident master or project orchestrator.',
    'node scripts/check-v3-mechanic-boundary-isomorphism.mjs',
  ]);

  forbidRuntimeTool(diagnostics, root, [
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

function forbidRuntimeTool(diagnostics, root, relDirs) {
  for (const relDir of relDirs) {
    const dir = path.join(root, relDir);
    if (!fs.existsSync(dir)) continue;
    for (const file of walk(dir)) {
      if (!file.endsWith('.rs')) continue;
      const source = fs.readFileSync(file, 'utf8');
      if (source.includes('mission_mechanic') || source.includes('mechanic_')) {
        diagnostics.push({
          file: path.relative(root, file),
          message: 'mechanic runtime tool wiring is forbidden while mechanic-collaboration-boundary :runtime-policy :enabled is false',
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
