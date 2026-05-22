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
//   mission_request, unified-entry-runtime, file-artifacts,
//   mission_directive, mission_plan, evidence-collector,
//   mission_execution-log, mission_execution-claim-lease,
//   mission_execution-completion-audit, mission_workflow, review-gate,
//   task-runner-cli, source-hygiene, context-pack, workstation-config,
//   workstation-pool, resident-master-control, commit-lisp-convergence-loop,
//   nightly-evolution-loop, autopilot-runtime, workstation-dispatch, mission_board, memory-kb, project-registry,
//   board-frontend,
//   conversation-ingestion, skill-runtime, cascade-governance,
//   router-policy, incident-governance, capability-governance,
//   compute-primitives, sysinfra-control, ops-infra, eventbridge,
//   memory-provider-boundary, eventhub-service-boundary, typed-lisp-compiler,
//   genome-runtime, mission-shared-memory, evidence-governance-view.

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
import {
  compiledSurfaceIds,
  loadCompiledV3Contract,
} from './lib/v3_compiled_contract.mjs';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';
const AGGREGATE_COMMAND = 'node scripts/check-v3-code-isomorphism-complete.mjs';

// Bootstrap/dry-fixture surface list. Live validation derives the active
// surface set from missiond-lispc emit-v3 + emit-semantic-ir so checker
// correctness follows typed Lisp structure instead of this compatibility list.
export const EXPECTED_SURFACES = [
  'mission_request',
  'unified-entry-runtime',
  'file-artifacts',
  'mission_directive',
  'mission_plan',
  'evidence-collector',
  'mission_execution-log',
  'mission_execution-claim-lease',
  'mission_execution-completion-audit',
  'mission_workflow',
  'work-order-lifecycle',
  'review-gate',
  'task-runner-cli',
  'source-hygiene',
  'lisp-code-drift-policy',
  'commit-lisp-convergence-loop',
  'lisp-code-sync-loop',
  'nightly-evolution-loop',
  'context-pack',
  'workstation-config',
  'workstation-pool',
  'resident-master-control',
  'autopilot-runtime',
  'workstation-dispatch',
  'mission_board',
  'memory-kb',
  'project-registry',
  'board-frontend',
  'conversation-ingestion',
  'skill-runtime',
  'cascade-governance',
  'router-policy',
  'incident-governance',
  'capability-governance',
  'compute-primitives',
  'sysinfra-control',
  'ops-infra',
  'eventbridge',
  'data-residency-universe',
  'memory-provider-boundary',
  'eventhub-service-boundary',
  'typed-lisp-compiler',
  'genome-runtime',
  'mission-shared-memory',
  'evidence-governance-view',
];

export const PER_SURFACE_CHECKERS = [
  // Cross-surface IR check: each pillar owns functions, each function has
  // entry -> ordered core steps -> egress and maps back to one surface.
  'scripts/check-v3-pillar-flow-schema.mjs',
  // Cross-surface historical/public coverage check: V2 effective design and
  // every public MCP tool must have an explicit V3 destination.
  'scripts/check-v3-v2-coverage.mjs',
  // Cross-surface runtime artifact path hygiene: public/runtime-facing docs
  // must cite V3 runtime sidecars first and keep V2 only as legacy fallback.
  'scripts/check-v3-runtime-path-hygiene.mjs',
  'scripts/check-v3-request-lisp-isomorphism.mjs',
  'scripts/check-v3-unified-entry-isomorphism.mjs',
  'scripts/check-v3-file-artifacts-isomorphism.mjs',
  'scripts/check-v3-intent-alignment-isomorphism.mjs',
  'scripts/check-v3-plan-execution-isomorphism.mjs',
  'scripts/check-v3-evidence-collector-isomorphism.mjs',
  'scripts/check-v3-mission-execution-isomorphism.mjs',
  'scripts/check-v3-workflow-isomorphism.mjs',
  'scripts/check-v3-work-order-lifecycle-isomorphism.mjs',
  'scripts/check-v3-review-gate-isomorphism.mjs',
  'scripts/check-v3-task-lifecycle-isomorphism.mjs',
  'scripts/check-v3-codex-boot-context-isomorphism.mjs',
  'scripts/check-v3-memory-kb-isomorphism.mjs',
  'scripts/check-v3-project-registry-isomorphism.mjs',
  'scripts/check-project-ssot-universe.mjs',
  'scripts/check-v3-data-residency-universe-isomorphism.mjs',
  'scripts/check-v3-conversation-ingestion-isomorphism.mjs',
  'scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs',
  'scripts/check-v3-skill-runtime-isomorphism.mjs',
  'scripts/check-v3-cascade-governance-isomorphism.mjs',
  'scripts/check-v3-router-policy-isomorphism.mjs',
  'scripts/check-v3-incident-governance-isomorphism.mjs',
  'scripts/check-v3-capability-governance-isomorphism.mjs',
  'scripts/check-v3-mechanic-boundary-isomorphism.mjs',
  'scripts/check-v3-compute-primitives-isomorphism.mjs',
  'scripts/check-v3-pty-recognition-isomorphism.mjs',
  'scripts/check-v3-sysinfra-control-isomorphism.mjs',
  'scripts/check-missiond-blue-green-deploy.mjs',
  'scripts/check-v3-source-hygiene-isomorphism.mjs',
  'scripts/check-v3-direct-code-drift-policy.mjs',
  'scripts/check-v3-context-pack-isomorphism.mjs',
  'scripts/check-v3-workstation-config-isomorphism.mjs',
  'scripts/check-v3-workstation-pool-isomorphism.mjs',
  'scripts/check-v3-control-plane-m6-split.mjs',
  'scripts/check-v3-master-control-isomorphism.mjs',
  'scripts/check-v3-autopilot-runtime-isomorphism.mjs',
  'scripts/check-v3-commit-convergence-loop.mjs',
  'scripts/check-v3-lisp-code-sync-isomorphism.mjs',
  'scripts/check-v3-nightly-evolution-isomorphism.mjs',
  'scripts/check-v3-workstation-dispatch-isomorphism.mjs',
  'scripts/check-v3-board-isomorphism.mjs',
  'scripts/check-frontend-board-lisp-schema.mjs',
  'scripts/check-frontend-board-code-isomorphism.mjs',
  'scripts/check-frontend-board-runtime-projection.mjs',
  'scripts/check-v3-ops-infra-isomorphism.mjs',
  'scripts/check-v3-eventbridge-isomorphism.mjs',
  'scripts/check-v3-service-extraction-isomorphism.mjs',
  'scripts/check-typed-lisp-compiler.mjs',
  'scripts/check-v3-genome-runtime-isomorphism.mjs',
  'scripts/check-v3-autopilot-genome-isomorphism.mjs',
  'scripts/check-v3-shared-memory-isomorphism.mjs',
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

export function validateBlueprintSource(source, file = BLUEPRINT_PATH, {
  expectedSurfaces = EXPECTED_SURFACES,
} = {}) {
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

  for (const expected of expectedSurfaces) {
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
    if (expectedSurfaces.includes(id)) continue;
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
    expectedSurfaces,
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
  let source;
  try {
    source = readBlueprintWithEvidenceSidecars(path.resolve(opts.repo), opts.blueprint);
  } catch (err) {
    process.stderr.write(`error: cannot read blueprint ${opts.blueprint}: ${err.message}\n`);
    process.exit(1);
  }
  const compiled = loadCompiledV3Contract({
    repoRoot: opts.repo,
    blueprint: opts.blueprint,
    semanticIr: true,
  });
  const expectedSurfaces = compiledSurfaceIds(compiled);
  const expectedSurfaceSet = expectedSurfaces.length > 0 ? expectedSurfaces : EXPECTED_SURFACES;
  const blueprintResult = validateBlueprintSource(source, opts.blueprint, {
    expectedSurfaces: expectedSurfaceSet,
  });
  const checkerResults = runPerSurfaceCheckers(opts.repo);
  const checkerOk = checkerResults.every((r) => r.ok);
  const compiledOk = compiled.ok === true && expectedSurfaces.length > 0;
  const ok = blueprintResult.ok && checkerOk && compiledOk;

  const result = {
    ok,
    blueprint: opts.blueprint,
    expected_surfaces: expectedSurfaceSet,
    surface_source: expectedSurfaces.length > 0
      ? 'missiond-lispc emit-semantic-ir'
      : 'bootstrap-fallback',
    typed_surface_count: expectedSurfaces.length,
    typed_source_hash: compiled.sourceHash,
    typed_semantic_source_hash: compiled.semanticSourceHash,
    surfaces: Object.fromEntries(
      Object.entries(blueprintResult.surfaces).map(([id, s]) => [
        id,
        { status: s.status, has_code: s.hasCode, has_note: s.hasNote },
      ]),
    ),
    diagnostics: [...compiled.diagnostics, ...blueprintResult.diagnostics],
    checks: checkerResults,
  };

  if (opts.json) {
    fs.writeSync(1, `${JSON.stringify(result, null, 2)}\n`);
  } else if (ok) {
    console.log(
      `v3 code-isomorphism gate OK (${expectedSurfaceSet.length} typed surfaces graduated, ${checkerResults.length} per-surface checkers passed)`,
    );
  } else {
    for (const d of compiled.diagnostics) {
      console.error(`${d.file}:${d.line ?? 1}:${d.column ?? 1}: error: ${d.message}`);
    }
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
    (surface unified-entry-runtime
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface file-artifacts
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
    (surface evidence-collector
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_execution-log
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_execution-claim-lease
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_execution-completion-audit
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_workflow
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface work-order-lifecycle
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface review-gate
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface task-runner-cli
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface source-hygiene
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface lisp-code-drift-policy
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface commit-lisp-convergence-loop
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface lisp-code-sync-loop
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface nightly-evolution-loop
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface context-pack
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-config
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-pool
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface resident-master-control
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface autopilot-runtime
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-dispatch
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_board
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface memory-kb
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface project-registry
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface board-frontend
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface conversation-ingestion
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface skill-runtime
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface cascade-governance
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface router-policy
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface incident-governance
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface capability-governance
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface compute-primitives
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface sysinfra-control
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface ops-infra
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface eventbridge
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface data-residency-universe
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface memory-provider-boundary
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface eventhub-service-boundary
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface typed-lisp-compiler
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface genome-runtime
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission-shared-memory
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface evidence-governance-view
      :status "code-aligned"
      :code ["a"]
      :note "n"))
  (compression-contract
    :checks ["${AGGREGATE_COMMAND}"]))`;
  cases.push({
    name: 'good fixture: all thirty surfaces code-aligned, aggregate command pinned',
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
    (surface unified-entry-runtime
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface file-artifacts
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
    (surface evidence-collector
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_execution-log
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_execution-claim-lease
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_execution-completion-audit
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_workflow
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface review-gate
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface task-runner-cli
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface context-pack
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-config
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-pool
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface autopilot-runtime
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface workstation-dispatch
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface mission_board
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface memory-kb
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface project-registry
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface board-frontend
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface conversation-ingestion
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface skill-runtime
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface cascade-governance
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface router-policy
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface incident-governance
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface capability-governance
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface compute-primitives
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface sysinfra-control
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface ops-infra
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface eventbridge
      :status "code-aligned"
      :code ["a"]
      :note "n")
    (surface typed-lisp-compiler
      :status "code-aligned"
      :code ["a"]
      :note "n"))
  (compression-contract
    :checks ["${AGGREGATE_COMMAND}"]))`;
  cases.push({
    name: 'missing-surface fixture: source-hygiene absent fails the gate',
    expectOk: false,
    expectMessage: /missing required surface "source-hygiene"/i,
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
    fs.writeSync(1, `${JSON.stringify({ ok: true, fixtures: cases.length }, null, 2)}\n`);
  } else {
    console.log(`v3 code-isomorphism gate fixtures OK (${cases.length} cases)`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
