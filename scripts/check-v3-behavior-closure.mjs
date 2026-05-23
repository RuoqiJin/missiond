#!/usr/bin/env node

import { validateBehaviorClosure } from './lib/behavior_universe.mjs';
import { runBehaviorUniverseFixtures } from './lib/behavior_universe_fixtures.mjs';

const usage = `Usage:
  node scripts/check-v3-behavior-closure.mjs [--json] [--repo <path>] [--skip-fixtures]

Hard gate for MissionD program-level SSOT closure. It reverse-discovers active
runtime behavior from code and fails unless every observed behavior is claimed
by a V3 (behavior-universe ...) form or explicitly tombstoned.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const fixtures = opts.skipFixtures ? { ok: true, cases: [], diagnostics: [] } : runBehaviorUniverseFixtures();
  const result = validateBehaviorClosure(opts.repo, {
    projectId: 'missiond',
    missiondV3: true,
  });
  const diagnostics = [...fixtures.diagnostics, ...result.diagnostics];
  const ok = fixtures.ok && result.ok;
  const output = publicResult(result, { ok, diagnostics, fixtures });

  if (opts.json) {
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
  } else if (ok) {
    console.log(`V3 behavior closure OK (${result.observed.length} observed behaviors, ${fixtures.cases.length} fixtures)`);
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line ?? 1}:${d.column ?? 1}: ${d.code}: ${d.message}`);
    }
    console.error(`V3 behavior closure FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, repo: process.cwd(), skipFixtures: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--skip-fixtures') opts.skipFixtures = true;
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function fail(message) {
  console.error(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function publicResult(result, { ok = result.ok, diagnostics = result.diagnostics, fixtures = null } = {}) {
  return {
    ok,
    projectId: result.projectId,
    observed_count: result.observed.length,
    behavior_count: result.declared.behaviors.length,
    effect_count: result.declared.effects.length,
    tombstone_count: result.declared.tombstones.length,
    fixture_count: fixtures?.cases?.length ?? 0,
    diagnostics,
  };
}

main();
