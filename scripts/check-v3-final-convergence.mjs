#!/usr/bin/env node

// MissionD V3 final convergence gate.
//
// This checker is intentionally a thin closure gate. It does not replace the
// per-surface V3 checkers; it composes their public results with a few hard
// completion invariants that answer "is the Lisp convergence done enough to
// treat V3 as the engineering SSOT?"

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

import {
  compiledSurfaceIds,
  loadCompiledV3Contract,
  loadResolvedV3Contract,
} from './lib/v3_compiled_contract.mjs';

const CHECK_COMMAND = 'node scripts/check-v3-final-convergence.mjs';
const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';

const DRY_FIXTURE_LIVE_CHECKS = [
  {
    id: 'lisp-blueprint-compression',
    argv: ['scripts/check-lisp-blueprint-compression.mjs'],
    timeoutMs: 60_000,
  },
  {
    id: 'architecture-lisp',
    argv: [
      'scripts/check-architecture-lisp.mjs',
      '--no-structure',
      BLUEPRINT_PATH,
    ],
    timeoutMs: 60_000,
  },
  {
    id: 'v3-code-isomorphism-complete',
    argv: ['scripts/check-v3-code-isomorphism-complete.mjs', '--json'],
    json: true,
    timeoutMs: 120_000,
  },
  {
    id: 'v2-public-coverage',
    argv: ['scripts/check-v3-v2-coverage.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'task-contract-all',
    argv: ['scripts/check-task-contract.mjs', '--all'],
    timeoutMs: 120_000,
  },
  {
    id: 'missiond-blue-green-deploy',
    argv: ['scripts/check-missiond-blue-green-deploy.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'missiond-rustfmt-convergence',
    command: 'bash',
    argv: ['scripts/rustfmt-missiond.sh', '--check'],
    timeoutMs: 120_000,
  },
  {
    id: 'project-ssot-universe',
    argv: ['scripts/check-project-ssot-universe.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'infrastructure-universe',
    argv: ['scripts/check-v3-infrastructure-universe-isomorphism.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'data-residency-universe',
    argv: ['scripts/check-v3-data-residency-universe-isomorphism.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'project-maturity',
    argv: ['scripts/check-project-maturity.mjs', '--min-level', 'M5'],
    timeoutMs: 60_000,
  },
  {
    id: 'auth-m6-depth',
    argv: ['scripts/check-project-maturity.mjs', '--min-level', 'M6', '--project', 'auth', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'typed-lisp-runtime-compile',
    argv: ['scripts/compile-v3-runtime.mjs', '--check', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'production-runtime-boundary',
    argv: ['scripts/check-v3-production-runtime-boundary.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'agent-cli-regression',
    argv: ['scripts/check-v3-agent-cli-regression.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'semantic-checker-coverage',
    argv: ['scripts/check-v3-semantic-checker-coverage.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'runtime-artifact-catalog',
    argv: ['scripts/check-v3-runtime-artifact-catalog.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
  {
    id: 'typed-sidecar-compression',
    argv: ['scripts/check-v3-typed-sidecar-compression.mjs', '--json'],
    json: true,
    timeoutMs: 60_000,
  },
];

const DRY_FIXTURE_RUNTIME_CHECKS = [
  {
    id: 'cargo-test-workspace',
    command: 'cargo',
    argv: ['test', '--workspace'],
    timeoutMs: 900_000,
  },
  {
    id: 'board-build',
    command: 'pnpm',
    argv: ['--dir', 'packages/board', 'build'],
    timeoutMs: 300_000,
  },
  {
    id: 'git-diff-check',
    command: 'git',
    argv: ['diff', '--check'],
    timeoutMs: 60_000,
  },
];

const DRY_FIXTURE_BLUEPRINT_NEEDLES = [
  ['v2-convergence-map', '(v2-convergence-map'],
  ['public-surface-map', '(public-surface-map'],
  ['pillar-flow-map', '(pillar-flow-map'],
  ['implementation-map', '(implementation-map'],
  ['workstation-config', '(workstation-config'],
  ['agent-cli-regression-policy', 'check-v3-agent-cli-regression.mjs'],
  ['control-plane-m6-split', '(control-plane-m6-split'],
  ['control-plane split checker', 'check-v3-control-plane-m6-split.mjs'],
  ['data-residency-universe', '(data-residency-universe'],
  ['data-residency checker', 'check-v3-data-residency-universe-isomorphism.mjs'],
  ['lisp-code-sync-loop', '(lisp-code-sync-loop'],
  ['lisp-code-sync checker', 'check-v3-lisp-code-sync-isomorphism.mjs'],
  ['context-pack-run-wave', 'context-pack-run-wave'],
  ['context-pack dispatch policy', 'dispatch-policy context-pack-run-wave'],
  ['swarm-run resolves project_root', 'mission_swarm_run MUST resolve project_id to a registered project_root'],
  ['autopilot overrides cwd to BoardTask project_root', 'Autopilot ensure_pty MUST override pty_slot.cwd'],
  ['durable conversation taskId attribution', 'bind conversations.task_id to the active BoardTask via a bounded retry helper'],
  ['taskId query survives slot reuse', 'message-anchored BoardTask id fallback'],
  ['runtime v3 paths', '.missiond/v3/runtime/'],
  ['V2 historical status', ':v2 "Kept as historical'],
  ['entry/core/egress function shape', ':entry ['],
  ['ordered core function steps', ':core ('],
  ['function egress shape', ':egress ['],
];

const DRY_FIXTURE_FACADE_BUDGETS = [
  {
    id: 'mission_plan facade',
    file: 'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
    maxLines: 800,
  },
  {
    id: 'mission_execution facade',
    file: 'crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs',
    maxLines: 350,
  },
  {
    id: 'workstation_dispatch facade',
    file: 'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs',
    maxLines: 400,
  },
];

const DRY_FIXTURE_REQUIRED_SPLIT_FILES = [
  'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs',
  'crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/descriptor.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/brief.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/decision.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/runner.rs',
];

const DRY_FIXTURE_REQUIRED_RUNTIME_FILES = [
  {
    file: 'scripts/lib/v3_workstation_runtime.mjs',
    needles: [
      'DEFAULT_MODEL_PROFILE',
      'coding-default-opus-4-7',
      'contextPackDispatchPolicy',
      'V3_BLUEPRINT_CONFIG_ERROR',
      'source_units',
      'MISSIOND_V3_ALLOW_SOURCE_FALLBACK',
      'compiled runtime config is required',
    ],
  },
  {
    file: 'scripts/context-pack-run-wave.mjs',
    needles: [
      'loadWorkstationRuntimeConfigForRepo',
      'contextPackMaxParallel',
      'ensureWaveLedgers',
      '--apply',
    ],
  },
  {
    file: 'scripts/context-pack-materialize-wave.mjs',
    needles: [
      'context-pack-run-wave.mjs',
      'loadWorkstationRuntimeConfigForRepo',
      'model_profile',
      'timeout_secs',
    ],
  },
  {
    file: 'scripts/task-runner-dispatch.mjs',
    needles: [
      'loadWorkstationRuntimeConfigForRepo',
      'model_profile',
      'timeout_secs',
    ],
  },
  {
    file: 'scripts/task-runner-submit-dispatch.mjs',
    needles: [
      'runDispatch',
      'runtime_projection',
      'model_profile',
      'timeout_secs',
    ],
  },
];

const usage = `Usage:
  node scripts/check-v3-final-convergence.mjs [--json] [--dry-fixture]
    [--repo <path>] [--blueprint <path>] [--static-only]

Final V3 convergence closure gate. It composes existing V3/V2/task-contract
checks, runtime/build hygiene, and the hard completion invariants that make V3
the engineering SSOT. Use --static-only for fast daemon/MCP status snapshots.
`;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
    staticOnly: false,
    repo: process.cwd(),
    blueprint: BLUEPRINT_PATH,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--static-only') {
      opts.staticOnly = true;
    } else if (arg === '--repo') {
      opts.repo = argv[++i] ?? fail('--repo requires a value');
    } else if (arg.startsWith('--repo=')) {
      opts.repo = arg.slice('--repo='.length);
    } else if (arg === '--blueprint') {
      opts.blueprint = argv[++i] ?? fail('--blueprint requires a value');
    } else if (arg.startsWith('--blueprint=')) {
      opts.blueprint = arg.slice('--blueprint='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runDryFixture(opts);
    return;
  }

  const repoRoot = path.resolve(opts.repo);
  const result = runFinalConvergenceCheck(repoRoot, opts.blueprint, {
    staticOnly: opts.staticOnly,
  });
  if (opts.json) {
    fs.writeSync(1, `${JSON.stringify(result, null, 2)}\n`);
  } else if (result.ok) {
    console.log(
      `v3 final convergence OK (${result.summary.surfaces} surfaces, ${result.summary.checkers} V3 checkers, ${result.summary.v2_items} V2 items, ${result.summary.public_tools} public tools, ${result.summary.facades} facades under budget, runtime=${result.runtime_status.mode})`,
    );
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file ?? '<repo>'}: ${d.message}`);
    }
    for (const check of result.subchecks) {
      if (check.ok) continue;
      console.error(`subcheck FAILED: ${check.id} (${check.command})`);
      if (check.stderr_tail) console.error(`  stderr: ${check.stderr_tail}`);
      if (check.stdout_tail) console.error(`  stdout: ${check.stdout_tail}`);
      if (check.error) console.error(`  error: ${check.error}`);
    }
    console.error(`v3 final convergence FAILED — ${result.diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

export function runFinalConvergenceCheck(repoRoot, blueprintRel = BLUEPRINT_PATH, options = {}) {
  const diagnostics = [];
  const contract = loadCompiledV3Contract({
    repoRoot,
    blueprint: blueprintRel,
    semanticIr: true,
  });
  const compiledSurfaceCount = compiledSurfaceIds(contract).length;
  diagnostics.push(...compiledDiagnostics(contract, blueprintRel));
  const gate = contract.finalConvergenceGate;
  if (!gate) {
    diagnostics.push({
      file: blueprintRel,
      message: 'compiled semantic IR missing final_convergence_gate fact',
    });
  }

  let blueprintSource = '';
  const resolved = loadResolvedV3Contract({
    repoRoot,
    blueprint: blueprintRel,
    includeEvidenceSidecar: true,
  });
  if (resolved.ok && resolved.resolvedSource) {
    blueprintSource = resolved.resolvedSource;
  } else {
    diagnostics.push(...compiledDiagnostics(resolved, blueprintRel));
  }

  if (blueprintSource && gate) {
    diagnostics.push(...checkBlueprintClosure(blueprintSource, blueprintRel, gate, contract.checkerRegistry));
  }
  const facades = checkFacadeBudgets(repoRoot, gate?.facadeBudgets ?? []);
  diagnostics.push(...facades.diagnostics);
  const splitFiles = checkRequiredFiles(repoRoot, gate?.requiredSplitFiles ?? [], 'required split module missing');
  diagnostics.push(...splitFiles.diagnostics);
  const runtimeFiles = checkRuntimeProjectionFiles(repoRoot, gate?.requiredRuntimeFiles ?? []);
  diagnostics.push(...runtimeFiles.diagnostics);

  const subchecks = (gate?.liveChecks ?? []).map((check) => runCheck(repoRoot, check));
  const runtimeChecks = options.staticOnly
    ? (gate?.runtimeChecks ?? []).map((check) => ({
        id: check.id,
        command: formatCheckCommand(check),
        ok: true,
        skipped: true,
        exit_code: null,
        json_data: null,
        stdout_tail: '',
        stderr_tail: '',
        error: null,
      }))
    : (gate?.runtimeChecks ?? []).map((check) => runCheck(repoRoot, check));
  for (const check of subchecks) {
    if (!check.ok) {
      diagnostics.push({
        file: check.command,
        message: `subcheck ${check.id} failed`,
      });
    }
  }
  for (const check of runtimeChecks) {
    if (check.ok) continue;
    diagnostics.push({
      file: check.command,
      message: `runtime check ${check.id} failed`,
    });
  }

  const aggregate = subchecks.find((c) => c.id === 'v3-code-isomorphism-complete')?.json_data;
  const coverage = subchecks.find((c) => c.id === 'v2-public-coverage')?.json_data;
  diagnostics.push(...checkAggregateSummary(aggregate));
  const expectedSurfaceCount = Array.isArray(aggregate?.expected_surfaces)
    ? aggregate.expected_surfaces.length
    : compiledSurfaceCount;
  diagnostics.push(...checkCoverageSummary(coverage, expectedSurfaceCount));

  const summary = {
    surfaces: Array.isArray(aggregate?.expected_surfaces)
      ? aggregate.expected_surfaces.length
      : compiledSurfaceCount,
    checkers: Array.isArray(aggregate?.checks) ? aggregate.checks.length : 0,
    v2_items: Number.isInteger(coverage?.v2_items) ? coverage.v2_items : 0,
    public_tools: Number.isInteger(coverage?.public_tools) ? coverage.public_tools : 0,
    facades: facades.files.length,
    split_files: splitFiles.files.length,
    runtime_files: runtimeFiles.files.length,
    external_final_checks: runtimeChecks.map((check) => ({
      id: check.id,
      command: check.command,
      ok: check.ok,
      skipped: check.skipped === true,
    })),
  };
  const runtimeStatus = readRuntimeStatus(repoRoot, {
    staticOnly: options.staticOnly === true,
    runtimeChecks,
  });
  const failedStage = inferFailedStage(diagnostics, subchecks, runtimeChecks);
  const blockingItems = buildBlockingItems(diagnostics, subchecks, runtimeChecks);

  return {
    ok: diagnostics.length === 0,
    failed_stage: failedStage,
    blocking_items: blockingItems,
    next_action: nextActionForStage(failedStage),
    runtime_status: runtimeStatus,
    smoke_task_ids: readSmokeTaskIds(repoRoot),
    summary,
    diagnostics,
    facades: facades.files,
    split_files: splitFiles.files,
    runtime_files: runtimeFiles.files,
    subchecks,
    runtime_checks: runtimeChecks,
  };
}

function compiledDiagnostics(contract, file) {
  return (contract?.diagnostics ?? []).map((diagnostic) => ({
    file: diagnostic.file ?? file,
    message: diagnostic.message ?? JSON.stringify(diagnostic),
  }));
}

export function checkBlueprintClosure(source, file = BLUEPRINT_PATH, gate = null, checkerRegistry = []) {
  const diagnostics = [];
  for (const { id, needle } of gate?.blueprintNeedles ?? []) {
    if (!source.includes(needle)) {
      diagnostics.push({
        file,
        message: `blueprint missing final convergence needle ${id}: ${needle}`,
      });
    }
  }

  const checkStrings = (checkerRegistry ?? []).flatMap((entry) => entry.checks ?? []);
  if (!checkStrings.includes(CHECK_COMMAND)) {
    diagnostics.push({
      file,
      message: `compiled checker registry must include "${CHECK_COMMAND}"`,
    });
  }
  return diagnostics;
}

export function checkFacadeBudgets(repoRoot, budgets = []) {
  const diagnostics = [];
  const files = [];
  for (const budget of budgets) {
    const abs = path.join(repoRoot, budget.file);
    let source = '';
    try {
      source = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({
        file: budget.file,
        message: `cannot read facade file: ${err.message}`,
      });
      continue;
    }
    const lines = countLines(source);
    files.push({ ...budget, lines });
    if (lines > budget.maxLines) {
      diagnostics.push({
        file: budget.file,
        message: `${budget.id} has ${lines} lines, above final facade budget ${budget.maxLines}`,
      });
    }
  }
  return { diagnostics, files };
}

function checkRequiredFiles(repoRoot, files, message) {
  const diagnostics = [];
  const found = [];
  for (const file of files) {
    if (fs.existsSync(path.join(repoRoot, file))) {
      found.push(file);
    } else {
      diagnostics.push({ file, message });
    }
  }
  return { diagnostics, files: found };
}

function checkRuntimeProjectionFiles(repoRoot, requiredRuntimeFiles = []) {
  const diagnostics = [];
  const files = [];
  for (const item of requiredRuntimeFiles) {
    const abs = path.join(repoRoot, item.file);
    let source = '';
    try {
      source = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({
        file: item.file,
        message: `cannot read runtime projection file: ${err.message}`,
      });
      continue;
    }
    files.push(item.file);
    for (const needle of item.needles) {
      if (!source.includes(needle)) {
        diagnostics.push({
          file: item.file,
          message: `runtime projection missing ${needle}`,
        });
      }
    }
  }
  return { diagnostics, files };
}

function checkAggregateSummary(aggregate) {
  const diagnostics = [];
  if (!aggregate) {
    diagnostics.push({
      file: 'scripts/check-v3-code-isomorphism-complete.mjs',
      message: 'missing aggregate JSON summary',
    });
    return diagnostics;
  }
  if (aggregate.ok !== true) {
    diagnostics.push({
      file: 'scripts/check-v3-code-isomorphism-complete.mjs',
      message: 'aggregate V3 isomorphism check did not return ok=true',
    });
  }
  const surfaces = aggregate.expected_surfaces ?? [];
  if (!Array.isArray(surfaces) || surfaces.length < 40) {
    diagnostics.push({
      file: 'scripts/check-v3-code-isomorphism-complete.mjs',
      message: `expected at least 40 V3 surfaces from typed projection, got ${surfaces.length}`,
    });
  }
  const acceptedSurfaceSources = new Set([
    'missiond-lispc emit-contract-abi',
    'missiond-lispc emit-semantic-ir',
  ]);
  if (!acceptedSurfaceSources.has(aggregate.surface_source)) {
    diagnostics.push({
      file: 'scripts/check-v3-code-isomorphism-complete.mjs',
      message: `aggregate must derive surfaces from missiond-lispc compiled facts; got ${aggregate.surface_source ?? '<missing>'}`,
    });
  }
  if (Number.isInteger(aggregate.typed_surface_count)
    && Array.isArray(surfaces)
    && aggregate.typed_surface_count !== surfaces.length) {
    diagnostics.push({
      file: 'scripts/check-v3-code-isomorphism-complete.mjs',
      message: `typed surface count mismatch: expected_surfaces=${surfaces.length}, typed_surface_count=${aggregate.typed_surface_count}`,
    });
  }
  const checks = aggregate.checks ?? [];
  if (!Array.isArray(checks) || checks.length < 30) {
    diagnostics.push({
      file: 'scripts/check-v3-code-isomorphism-complete.mjs',
      message: `expected at least 30 V3 checkers, got ${checks.length}`,
    });
  }
  const requiredChecker = 'scripts/check-v3-runtime-path-hygiene.mjs';
  if (!checks.some((check) => check.script === requiredChecker && check.ok === true)) {
    diagnostics.push({
      file: requiredChecker,
      message: 'aggregate must include passing V3 runtime path hygiene checker',
    });
  }
  return diagnostics;
}

function checkCoverageSummary(coverage, expectedSurfaceCount = 0) {
  const diagnostics = [];
  if (!coverage) {
    diagnostics.push({
      file: 'scripts/check-v3-v2-coverage.mjs',
      message: 'missing V2/public coverage JSON summary',
    });
    return diagnostics;
  }
  if (coverage.ok !== true) {
    diagnostics.push({
      file: 'scripts/check-v3-v2-coverage.mjs',
      message: 'V2/public coverage check did not return ok=true',
    });
  }
  if (coverage.v2_items < 28) {
    diagnostics.push({
      file: 'scripts/check-v3-v2-coverage.mjs',
      message: `expected at least 28 V2 convergence items, got ${coverage.v2_items}`,
    });
  }
  if (coverage.public_tools < 84) {
    diagnostics.push({
      file: 'scripts/check-v3-v2-coverage.mjs',
      message: `expected at least 84 public tools, got ${coverage.public_tools}`,
    });
  }
  if (coverage.code_aligned_surfaces < expectedSurfaceCount) {
    diagnostics.push({
      file: 'scripts/check-v3-v2-coverage.mjs',
      message: `expected ${expectedSurfaceCount} code-aligned surfaces, got ${coverage.code_aligned_surfaces}`,
    });
  }
  return diagnostics;
}

function runCheck(repoRoot, check) {
  const command = check.command ?? process.execPath;
  const argv = check.command ? check.argv : check.argv;
  const proc = spawnSync(command, argv, {
    cwd: repoRoot,
    encoding: 'utf8',
    timeout: check.timeoutMs,
    maxBuffer: 8 * 1024 * 1024,
  });
  const ok = proc.status === 0 && proc.error == null;
  let jsonData = null;
  let error = proc.error ? proc.error.message : null;
  if (ok && check.json) {
    try {
      jsonData = JSON.parse(proc.stdout);
    } catch (err) {
      error = `failed to parse JSON stdout: ${err.message}`;
    }
  }
  return {
    id: check.id,
    command: formatCheckCommand(check),
    ok: ok && (!check.json || jsonData != null),
    exit_code: proc.status,
    json_data: jsonData,
    stdout_tail: tail(proc.stdout ?? ''),
    stderr_tail: tail(proc.stderr ?? ''),
    error,
  };
}

function formatCheckCommand(check) {
  return check.command ? `${check.command} ${check.argv.join(' ')}` : `node ${check.argv.join(' ')}`;
}

function inferFailedStage(diagnostics, subchecks, runtimeChecks) {
  const failedSubcheck = subchecks.find((check) => !check.ok);
  if (failedSubcheck) return failedSubcheck.id;
  const failedRuntime = runtimeChecks.find((check) => !check.ok);
  if (failedRuntime) return failedRuntime.id;
  const firstDiagnostic = diagnostics[0];
  if (firstDiagnostic) return firstDiagnostic.file ?? 'static-closure';
  return null;
}

function buildBlockingItems(diagnostics, subchecks, runtimeChecks) {
  const items = [];
  for (const diagnostic of diagnostics) {
    items.push({
      kind: 'diagnostic',
      stage: diagnostic.file ?? 'static-closure',
      message: diagnostic.message,
    });
  }
  for (const check of [...subchecks, ...runtimeChecks]) {
    if (check.ok) continue;
    items.push({
      kind: check.skipped ? 'skipped-check' : 'failed-check',
      stage: check.id,
      command: check.command,
      exit_code: check.exit_code,
      stdout_tail: check.stdout_tail,
      stderr_tail: check.stderr_tail,
      error: check.error,
    });
  }
  return items;
}

function nextActionForStage(stage) {
  if (!stage) return 'none';
  if (stage.includes('cargo')) return 'fix Rust test/build failure, then rerun final convergence gate';
  if (stage.includes('board-build')) return 'fix Board frontend build failure, then rerun final convergence gate';
  if (stage.includes('git-diff')) return 'fix whitespace/diff hygiene, then rerun git diff --check';
  if (stage.includes('v2') || stage.includes('coverage')) return 'map missing public/V2 behavior into V3 and rerun coverage';
  if (stage.includes('frontend')) return 'repair Board frontend Lisp/code/runtime projection';
  if (stage.includes('lisp') || stage.includes('blueprint')) return 'repair compact Lisp SSOT or move long evidence to sidecar';
  return 'inspect blocking_items[0], repair the named stage, and rerun final convergence gate';
}

function readRuntimeStatus(repoRoot, { staticOnly, runtimeChecks }) {
  const checkpoint = readCheckpointStatus(repoRoot);
  const git = readGitStatus(repoRoot);
  return {
    schema: 'missiond.final-convergence.runtime-status.v1',
    mode: staticOnly ? 'static-only' : 'full',
    checkpoint,
    git,
    runtime_checks: runtimeChecks.map((check) => ({
      id: check.id,
      ok: check.ok,
      skipped: check.skipped === true,
      exit_code: check.exit_code,
    })),
  };
}

function readCheckpointStatus(repoRoot) {
  const rel = '.missiond/v3/runtime/master-control-checkpoint.lisp';
  const abs = path.join(repoRoot, rel);
  try {
    const stat = fs.statSync(abs);
    const text = fs.readFileSync(abs, 'utf8');
    return {
      path: rel,
      exists: true,
      size: stat.size,
      updated_at_epoch_ms: stat.mtimeMs,
      worker: matchLispKeyword(text, ':worker'),
      tick_id: matchLispKeyword(text, ':tick-id'),
      phase: matchLispKeyword(text, ':phase'),
      active_objective_id: matchLispKeyword(text, ':active-objective-id'),
      blocked_reason: matchLispKeyword(text, ':blocked-reason'),
    };
  } catch {
    return { path: rel, exists: false };
  }
}

function readGitStatus(repoRoot) {
  const proc = spawnSync('git', ['status', '--short'], {
    cwd: repoRoot,
    encoding: 'utf8',
    timeout: 30_000,
    maxBuffer: 1024 * 1024,
  });
  if (proc.status !== 0 || proc.error) {
    return {
      ok: false,
      error: proc.error?.message ?? proc.stderr?.trim() ?? 'git status failed',
    };
  }
  const lines = proc.stdout.split('\n').filter((line) => line.trim() !== '');
  return {
    ok: true,
    dirty: lines.length > 0,
    changed_count: lines.length,
    preview: lines.slice(0, 20),
  };
}

function readSmokeTaskIds(repoRoot) {
  const checkpoint = path.join(repoRoot, '.missiond/v3/runtime/final-convergence-smoke.json');
  try {
    const parsed = JSON.parse(fs.readFileSync(checkpoint, 'utf8'));
    if (Array.isArray(parsed.smoke_task_ids)) return parsed.smoke_task_ids;
  } catch {}
  return [];
}

function matchLispKeyword(source, keyword) {
  const re = new RegExp(`${escapeRegExp(keyword)}\\s+("[^"]*"|[^\\s)]+)`);
  const match = source.match(re);
  if (!match) return null;
  const value = match[1];
  if (value === 'nil') return null;
  if (value.startsWith('"') && value.endsWith('"')) return value.slice(1, -1);
  return value;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function tail(text, lines = 8) {
  if (!text) return '';
  return text.split('\n').slice(-lines).join('\n').trim();
}

function countLines(source) {
  if (source.length === 0) return 0;
  return source.endsWith('\n') ? source.split('\n').length - 1 : source.split('\n').length;
}

function runDryFixture(opts) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-final-'));
  const cases = [];
  try {
    const goodBlueprint = fixtureBlueprint({ includeFinalCheck: true });
    const goodDiagnostics = checkBlueprintClosure(
      goodBlueprint,
      '<fixture-good>',
      fixtureGate(),
      [{ id: 'v3-compression-contract', checks: [CHECK_COMMAND] }],
    );
    cases.push(assertCase(
      'blueprint closure accepts all required needles and final check command',
      goodDiagnostics.length === 0,
      goodDiagnostics,
    ));

    const missingFinalDiagnostics = checkBlueprintClosure(
      fixtureBlueprint({ includeFinalCheck: false }),
      '<fixture-missing-final>',
      fixtureGate(),
      [{ id: 'v3-compression-contract', checks: ['node scripts/check-v3-code-isomorphism-complete.mjs'] }],
    );
    cases.push(assertCase(
      'blueprint closure rejects compression-contract without final check command',
      missingFinalDiagnostics.some((d) => d.message.includes(CHECK_COMMAND)),
      missingFinalDiagnostics,
    ));

    const facadeRoot = path.join(tmp, 'facade-ok');
    writeFile(
      facadeRoot,
      'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
      'mod plan;\n'.repeat(20),
    );
    writeFile(
      facadeRoot,
      'crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs',
      'mod agent_execution;\n'.repeat(10),
    );
    writeFile(
      facadeRoot,
      'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs',
      'mod workstation_dispatch;\n'.repeat(10),
    );
    const okFacades = checkFacadeBudgets(facadeRoot, DRY_FIXTURE_FACADE_BUDGETS);
    cases.push(assertCase(
      'facade budget accepts thin facade files',
      okFacades.diagnostics.length === 0,
      okFacades.diagnostics,
    ));

    const badRoot = path.join(tmp, 'facade-bad');
    writeFile(
      badRoot,
      'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
      'fn x() {}\n'.repeat(805),
    );
    writeFile(
      badRoot,
      'crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs',
      'mod agent_execution;\n'.repeat(10),
    );
    writeFile(
      badRoot,
      'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs',
      'mod workstation_dispatch;\n'.repeat(10),
    );
    const badFacades = checkFacadeBudgets(badRoot, DRY_FIXTURE_FACADE_BUDGETS);
    cases.push(assertCase(
      'facade budget rejects oversized plan.rs',
      badFacades.diagnostics.some((d) => d.message.includes('above final facade budget')),
      badFacades.diagnostics,
    ));
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }

  const failed = cases.filter((c) => !c.ok);
  if (opts.json) {
    console.log(JSON.stringify({ ok: failed.length === 0, fixtures: cases }, null, 2));
  } else if (failed.length === 0) {
    console.log(`v3 final convergence fixtures OK (${cases.length} cases)`);
  } else {
    for (const c of failed) {
      console.error(`fixture FAILED: ${c.name}`);
      for (const d of c.diagnostics) console.error(`  ${d.file}: ${d.message}`);
    }
    console.error(`v3 final convergence fixtures FAILED — ${failed.length}/${cases.length}`);
  }
  process.exit(failed.length === 0 ? 0 : 1);
}

function fixtureGate() {
  return {
    blueprintNeedles: DRY_FIXTURE_BLUEPRINT_NEEDLES.map(([id, needle]) => ({ id, needle })),
    facadeBudgets: DRY_FIXTURE_FACADE_BUDGETS,
    requiredSplitFiles: DRY_FIXTURE_REQUIRED_SPLIT_FILES,
    requiredRuntimeFiles: DRY_FIXTURE_REQUIRED_RUNTIME_FILES,
  };
}

function assertCase(name, ok, diagnostics) {
  return { name, ok, diagnostics };
}

function writeFile(root, rel, source) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, source);
}

function fixtureBlueprint({ includeFinalCheck }) {
  const checks = includeFinalCheck
    ? `"${CHECK_COMMAND}"`
    : '"node scripts/check-v3-code-isomorphism-complete.mjs"';
  return `
(missiond-blueprint
  (axioms)
  (artifact-contracts
    (artifact final-report :path ".missiond/v3/runtime/reports/final.lisp"))
  (workstation-config
    (dispatch-policy context-pack-run-wave :default_max_parallel 4))
  (pillar-flow-map
    (pillar request
      (function mission_request
        :entry [mission_request]
        :core ((step s1 :logic "ordered core"))
        :egress [intent-alignment.lisp])))
  (implementation-map
    (surface mission_request :status "code-aligned" :code ["x"] :note "n")
    (surface context-pack :status "code-aligned" :code ["context-pack-run-wave"] :note "n"))
  (v2-convergence-map
    :v2 "Kept as historical source index"
    (public-surface-map))
  (compression-contract
    :v2 "Kept as historical source index, implementation status, and wave evidence."
    :checks [${checks}]))`;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
