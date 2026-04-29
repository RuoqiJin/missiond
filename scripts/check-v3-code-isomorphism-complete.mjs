#!/usr/bin/env node

// MissionD V3 complete code-isomorphism gate.
//
// Aggregate completion checker for the V3 implementation-map. Read-only,
// deterministic. Fails when:
//   - Any expected implementation-map surface is missing.
//   - Any implementation-map surface still carries :status "code-aligned-partial".
//   - Any expected surface lacks :status "code-aligned", :code, or :note.
//   - The compression-contract :checks list omits this aggregate command.
//   - Any per-surface V3 checker fails when run live.
//
// The aggregate covers exactly these implementation surfaces unless the
// blueprint explicitly changes the V3 surface set:
//   mission_request, mission_directive, mission_plan, mission_workflow,
//   task-runner-cli, workstation-config.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';

const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';
const AGGREGATE_COMMAND = 'node scripts/check-v3-code-isomorphism-complete.mjs';

export const EXPECTED_SURFACES = [
  'mission_request',
  'mission_directive',
  'mission_plan',
  'mission_workflow',
  'task-runner-cli',
  'workstation-config',
];

export const PER_SURFACE_CHECKERS = [
  'scripts/check-v3-request-lisp-isomorphism.mjs',
  'scripts/check-v3-intent-alignment-isomorphism.mjs',
  'scripts/check-v3-plan-execution-isomorphism.mjs',
  'scripts/check-v3-workflow-isomorphism.mjs',
  'scripts/check-v3-task-lifecycle-isomorphism.mjs',
  'scripts/check-v3-workstation-config-isomorphism.mjs',
  // Cross-surface request-flow smoke; aggregates the user-facing
  // request -> intent -> plan -> execute-review path declared in
  // unified-entry/review-packet/review-response. See wave42-01.
  'scripts/check-v3-request-flow-smoke.mjs',
];

const usage = `Usage:
  node scripts/check-v3-code-isomorphism-complete.mjs [--json] [--dry-fixture]
    [--blueprint <path>] [--repo <path>]

Aggregate V3 implementation-map completion gate. Without --dry-fixture it:
  1. Validates the implementation-map surface set + per-surface :status :code
     :note structure of .missiond/v3/missiond-blueprint.lisp (or --blueprint).
  2. Confirms the compression-contract :checks list includes this aggregate
     command.
  3. Runs every per-surface V3 checker live (spawnSync, no shell) and fails
     if any checker exits non-zero.
`;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
    blueprint: BLUEPRINT_PATH,
    repo: process.cwd(),
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--blueprint') {
      opts.blueprint = argv[++i] ?? fail('--blueprint requires a value');
    } else if (arg.startsWith('--blueprint=')) {
      opts.blueprint = arg.slice('--blueprint='.length);
    } else if (arg === '--repo') {
      opts.repo = argv[++i] ?? fail('--repo requires a value');
    } else if (arg.startsWith('--repo=')) {
      opts.repo = arg.slice('--repo='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

export function validateBlueprintSource(source, file = BLUEPRINT_PATH) {
  const diagnostics = [];
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    diagnostics.push({
      severity: 'error',
      file,
      line: err.line ?? 1,
      column: err.column ?? 1,
      message: err.message,
    });
    return { ok: false, diagnostics, surfaces: {} };
  }
  const root = forms.find((form) => isList(form) && head(form) === 'missiond-blueprint');
  if (!root) {
    diagnostics.push({
      severity: 'error',
      file,
      line: 1,
      column: 1,
      message: 'no (missiond-blueprint ...) root form',
    });
    return { ok: false, diagnostics, surfaces: {} };
  }

  const implementationMap = root.children.find(
    (c) => isList(c) && head(c) === 'implementation-map',
  );
  if (!implementationMap) {
    diagnostics.push({
      severity: 'error',
      file,
      line: root.loc?.line ?? 1,
      column: root.loc?.column ?? 1,
      message: 'blueprint missing (implementation-map ...) section',
    });
    return { ok: false, diagnostics, surfaces: {} };
  }

  const surfaces = {};
  const surfaceForms = implementationMap.children.filter(
    (c) => isList(c) && head(c) === 'surface',
  );
  for (const node of surfaceForms) {
    const id = nodeText(node.children[1]);
    if (!id) continue;
    const props = readKeywordProps(node, { start: 2 });
    surfaces[id] = {
      node,
      status: nodeText(props[':status']?.value),
      hasCode: !!props[':code'],
      hasNote: !!props[':note'],
    };
  }

  for (const expected of EXPECTED_SURFACES) {
    const surface = surfaces[expected];
    if (!surface) {
      diagnostics.push({
        severity: 'error',
        file,
        line: implementationMap.loc?.line ?? 1,
        column: implementationMap.loc?.column ?? 1,
        message: `implementation-map missing required surface "${expected}"`,
      });
      continue;
    }
    if (surface.status === 'code-aligned-partial') {
      diagnostics.push({
        severity: 'error',
        file,
        line: surface.node.loc?.line ?? 1,
        column: surface.node.loc?.column ?? 1,
        message: `surface "${expected}" still carries :status "code-aligned-partial"; graduate it before merging`,
      });
      continue;
    }
    if (surface.status !== 'code-aligned') {
      diagnostics.push({
        severity: 'error',
        file,
        line: surface.node.loc?.line ?? 1,
        column: surface.node.loc?.column ?? 1,
        message: `surface "${expected}" must declare :status "code-aligned"; got ${JSON.stringify(surface.status)}`,
      });
    }
    if (!surface.hasCode) {
      diagnostics.push({
        severity: 'error',
        file,
        line: surface.node.loc?.line ?? 1,
        column: surface.node.loc?.column ?? 1,
        message: `surface "${expected}" must declare :code [...]`,
      });
    }
    if (!surface.hasNote) {
      diagnostics.push({
        severity: 'error',
        file,
        line: surface.node.loc?.line ?? 1,
        column: surface.node.loc?.column ?? 1,
        message: `surface "${expected}" must declare :note "..."`,
      });
    }
  }

  // Catch ANY surface (not just expected) still labelled partial; the V3
  // gate is "no surface is partial", not just "expected surfaces aren't".
  for (const [id, surface] of Object.entries(surfaces)) {
    if (EXPECTED_SURFACES.includes(id)) continue;
    if (surface.status === 'code-aligned-partial') {
      diagnostics.push({
        severity: 'error',
        file,
        line: surface.node.loc?.line ?? 1,
        column: surface.node.loc?.column ?? 1,
        message: `surface "${id}" still carries :status "code-aligned-partial"`,
      });
    }
  }

  const compressionContract = root.children.find(
    (c) => isList(c) && head(c) === 'compression-contract',
  );
  if (!compressionContract) {
    diagnostics.push({
      severity: 'error',
      file,
      line: root.loc?.line ?? 1,
      column: root.loc?.column ?? 1,
      message: 'blueprint missing (compression-contract ...) section',
    });
  } else {
    const props = readKeywordProps(compressionContract, { start: 1 });
    const checksNode = props[':checks']?.value;
    const checkStrings = checksNode && isList(checksNode)
      ? checksNode.children.map((c) => nodeText(c)).filter((v) => v != null)
      : [];
    if (!checkStrings.includes(AGGREGATE_COMMAND)) {
      diagnostics.push({
        severity: 'error',
        file,
        line: compressionContract.loc?.line ?? 1,
        column: compressionContract.loc?.column ?? 1,
        message: `compression-contract :checks must include ${JSON.stringify(AGGREGATE_COMMAND)}`,
      });
    }
  }

  return {
    ok: !diagnostics.some((d) => d.severity === 'error'),
    diagnostics,
    surfaces,
  };
}

function runPerSurfaceCheckers(repoRoot) {
  const results = [];
  for (const script of PER_SURFACE_CHECKERS) {
    const abs = path.resolve(repoRoot, script);
    if (!fs.existsSync(abs)) {
      results.push({
        script,
        ok: false,
        exit_code: null,
        stderr_tail: '',
        stdout_tail: '',
        message: `per-surface checker not found: ${script}`,
      });
      continue;
    }
    const proc = spawnSync(process.execPath, [abs], {
      cwd: repoRoot,
      encoding: 'utf8',
      timeout: 60_000,
    });
    const ok = proc.status === 0 && proc.error == null;
    results.push({
      script,
      ok,
      exit_code: proc.status,
      stderr_tail: tail(proc.stderr ?? ''),
      stdout_tail: tail(proc.stdout ?? ''),
      message: proc.error ? proc.error.message : null,
    });
  }
  return results;
}

function tail(text, lines = 8) {
  if (!text) return '';
  return text.split('\n').slice(-lines).join('\n').trim();
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runDryFixture(opts);
    return;
  }
  const blueprintAbs = path.resolve(opts.repo, opts.blueprint);
  let source;
  try {
    source = fs.readFileSync(blueprintAbs, 'utf8');
  } catch (err) {
    process.stderr.write(`error: cannot read blueprint ${blueprintAbs}: ${err.message}\n`);
    process.exit(1);
  }
  const blueprintResult = validateBlueprintSource(source, blueprintAbs);
  const checkerResults = runPerSurfaceCheckers(opts.repo);
  const checkerOk = checkerResults.every((r) => r.ok);
  const ok = blueprintResult.ok && checkerOk;

  const result = {
    ok,
    blueprint: opts.blueprint,
    expected_surfaces: EXPECTED_SURFACES,
    surfaces: Object.fromEntries(
      Object.entries(blueprintResult.surfaces).map(([id, s]) => [
        id,
        { status: s.status, has_code: s.hasCode, has_note: s.hasNote },
      ]),
    ),
    diagnostics: blueprintResult.diagnostics,
    checks: checkerResults,
  };

  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (ok) {
    console.log(
      `v3 code-isomorphism gate OK (${EXPECTED_SURFACES.length} surfaces graduated, ${checkerResults.length} per-surface checkers passed)`,
    );
  } else {
    for (const d of blueprintResult.diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    for (const r of checkerResults) {
      if (r.ok) continue;
      console.error(`per-surface checker FAILED: ${r.script} (exit ${r.exit_code})`);
      if (r.stderr_tail) console.error(`  stderr: ${r.stderr_tail}`);
      if (r.stdout_tail) console.error(`  stdout: ${r.stdout_tail}`);
      if (r.message) console.error(`  ${r.message}`);
    }
    console.error('v3 code-isomorphism gate FAILED');
  }
  process.exit(ok ? 0 : 1);
}

function runDryFixture(opts) {
  const cases = [];

  const goodSource = `
(missiond-blueprint
  (axioms)
  (artifact-contracts)
  (unified-entry)
  (state-machines)
  (policies)
  (implementation-map
    (surface mission_request
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_directive
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_plan
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_workflow
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface task-runner-cli
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-config
      :status "code-aligned"
      :code ["a"]
      :note "n"))
  (compression-contract
    :checks ["${AGGREGATE_COMMAND}"]))`;
  cases.push({
    name: 'good fixture: all six surfaces code-aligned, aggregate command pinned',
    expectOk: true,
    source: goodSource,
  });

  const partialSource = goodSource.replace(
    '(surface task-runner-cli\n      :status "code-aligned"',
    '(surface task-runner-cli\n      :status "code-aligned-partial"',
  );
  cases.push({
    name: 'partial-status fixture: task-runner-cli still partial fails the gate',
    expectOk: false,
    expectMessage: /code-aligned-partial/i,
    source: partialSource,
  });

  const missingSurfaceSource = `
(missiond-blueprint
  (axioms)
  (artifact-contracts)
  (unified-entry)
  (state-machines)
  (policies)
  (implementation-map
    (surface mission_request
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_directive
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_plan
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_workflow
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-config
      :status "code-aligned"
      :code ["a"]
      :note "n"))
  (compression-contract
    :checks ["${AGGREGATE_COMMAND}"]))`;
  cases.push({
    name: 'missing-surface fixture: task-runner-cli absent fails the gate',
    expectOk: false,
    expectMessage: /missing required surface "task-runner-cli"/i,
    source: missingSurfaceSource,
  });

  const missingNoteSource = goodSource.replace(
    '(surface workstation-config\n      :status "code-aligned"\n      :code ["a"]\n      :note "n")',
    '(surface workstation-config\n      :status "code-aligned"\n      :code ["a"])',
  );
  cases.push({
    name: 'missing-note fixture: workstation-config without :note fails the gate',
    expectOk: false,
    expectMessage: /must declare :note/i,
    source: missingNoteSource,
  });

  const missingChecksSource = goodSource.replace(
    `:checks ["${AGGREGATE_COMMAND}"]`,
    ':checks []',
  );
  cases.push({
    name: 'missing-aggregate-command fixture: compression-contract :checks omits aggregate',
    expectOk: false,
    expectMessage: /compression-contract :checks must include/i,
    source: missingChecksSource,
  });

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-complete-'));
  let failed = 0;
  try {
    for (const c of cases) {
      const file = path.join(tmp, `${c.name.replace(/\W+/g, '-')}.lisp`);
      fs.writeFileSync(file, c.source);
      const result = validateBlueprintSource(c.source, file);
      if (result.ok !== c.expectOk) {
        failed += 1;
        console.error(`fixture FAILED: ${c.name}: expected ok=${c.expectOk}, got ok=${result.ok}`);
        for (const d of result.diagnostics) console.error(`  ${d.message}`);
        continue;
      }
      if (c.expectMessage) {
        const messages = result.diagnostics.map((d) => d.message).join(' | ');
        if (!c.expectMessage.test(messages)) {
          failed += 1;
          console.error(
            `fixture FAILED: ${c.name}: expected diagnostic matching ${c.expectMessage}, got ${JSON.stringify(messages)}`,
          );
        }
      }
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  if (failed > 0) {
    console.error(`v3 code-isomorphism gate fixtures FAILED — ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (opts.json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 code-isomorphism gate fixtures OK (${cases.length} cases)`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
