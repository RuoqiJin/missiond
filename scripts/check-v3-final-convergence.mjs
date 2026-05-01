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

import { EXPECTED_SURFACES } from './check-v3-code-isomorphism-complete.mjs';
import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';

const CHECK_COMMAND = 'node scripts/check-v3-final-convergence.mjs';
const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';

const LIVE_CHECKS = [
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
];

const BLUEPRINT_NEEDLES = [
  ['v2-convergence-map', '(v2-convergence-map'],
  ['public-surface-map', '(public-surface-map'],
  ['pillar-flow-map', '(pillar-flow-map'],
  ['implementation-map', '(implementation-map'],
  ['workstation-config', '(workstation-config'],
  ['context-pack-run-wave', 'context-pack-run-wave'],
  ['context-pack dispatch policy', 'dispatch-policy context-pack-run-wave'],
  ['runtime v3 paths', '.missiond/v3/runtime/'],
  ['V2 historical status', ':v2 "Kept as historical'],
  ['entry/core/egress function shape', ':entry ['],
  ['ordered core function steps', ':core ('],
  ['function egress shape', ':egress ['],
];

const FACADE_BUDGETS = [
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

const REQUIRED_SPLIT_FILES = [
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

const REQUIRED_RUNTIME_FILES = [
  {
    file: 'scripts/lib/v3_workstation_runtime.mjs',
    needles: [
      'DEFAULT_MODEL_PROFILE',
      'coding-default-opus-4-7',
      'contextPackDispatchPolicy',
      'V3_BLUEPRINT_CONFIG_ERROR',
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
    [--repo <path>] [--blueprint <path>]

Final V3 convergence closure gate. It composes existing V3/V2/task-contract
checks and asserts the hard completion invariants that make V3 the engineering
SSOT: V2/public coverage, surface graduation, physical facade split, runtime
projection, context-pack runner, and task contract health.
`;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
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
  const result = runFinalConvergenceCheck(repoRoot, opts.blueprint);
  if (opts.json) {
    fs.writeSync(1, `${JSON.stringify(result, null, 2)}\n`);
  } else if (result.ok) {
    console.log(
      `v3 final convergence OK (${result.summary.surfaces} surfaces, ${result.summary.checkers} V3 checkers, ${result.summary.v2_items} V2 items, ${result.summary.public_tools} public tools, ${result.summary.facades} facades under budget)`,
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

export function runFinalConvergenceCheck(repoRoot, blueprintRel = BLUEPRINT_PATH) {
  const diagnostics = [];
  const blueprintPath = path.resolve(repoRoot, blueprintRel);
  let blueprintSource = '';
  try {
    blueprintSource = fs.readFileSync(blueprintPath, 'utf8');
  } catch (err) {
    diagnostics.push({
      file: blueprintRel,
      message: `cannot read V3 blueprint: ${err.message}`,
    });
  }

  if (blueprintSource) {
    diagnostics.push(...checkBlueprintClosure(blueprintSource, blueprintRel));
  }
  const facades = checkFacadeBudgets(repoRoot);
  diagnostics.push(...facades.diagnostics);
  const splitFiles = checkRequiredFiles(repoRoot, REQUIRED_SPLIT_FILES, 'required split module missing');
  diagnostics.push(...splitFiles.diagnostics);
  const runtimeFiles = checkRuntimeProjectionFiles(repoRoot);
  diagnostics.push(...runtimeFiles.diagnostics);

  const subchecks = LIVE_CHECKS.map((check) => runCheck(repoRoot, check));
  for (const check of subchecks) {
    if (!check.ok) {
      diagnostics.push({
        file: check.command,
        message: `subcheck ${check.id} failed`,
      });
    }
  }

  const aggregate = subchecks.find((c) => c.id === 'v3-code-isomorphism-complete')?.json_data;
  const coverage = subchecks.find((c) => c.id === 'v2-public-coverage')?.json_data;
  diagnostics.push(...checkAggregateSummary(aggregate));
  diagnostics.push(...checkCoverageSummary(coverage));

  const summary = {
    surfaces: Array.isArray(aggregate?.expected_surfaces)
      ? aggregate.expected_surfaces.length
      : EXPECTED_SURFACES.length,
    checkers: Array.isArray(aggregate?.checks) ? aggregate.checks.length : 0,
    v2_items: Number.isInteger(coverage?.v2_items) ? coverage.v2_items : 0,
    public_tools: Number.isInteger(coverage?.public_tools) ? coverage.public_tools : 0,
    facades: facades.files.length,
    split_files: splitFiles.files.length,
    runtime_files: runtimeFiles.files.length,
    external_final_checks: [
      'cargo test --workspace',
      'scripts/cargo-fmt-touched.sh --check',
      'git diff --check',
    ],
  };

  return {
    ok: diagnostics.length === 0,
    summary,
    diagnostics,
    facades: facades.files,
    split_files: splitFiles.files,
    runtime_files: runtimeFiles.files,
    subchecks,
  };
}

export function checkBlueprintClosure(source, file = BLUEPRINT_PATH) {
  const diagnostics = [];
  for (const [id, needle] of BLUEPRINT_NEEDLES) {
    if (!source.includes(needle)) {
      diagnostics.push({
        file,
        message: `blueprint missing final convergence needle ${id}: ${needle}`,
      });
    }
  }

  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    diagnostics.push({
      file,
      message: `blueprint parse failed: ${err.message}`,
    });
    return diagnostics;
  }
  const root = forms.find((form) => isList(form) && head(form) === 'missiond-blueprint');
  if (!root) {
    diagnostics.push({ file, message: 'missing (missiond-blueprint ...) root' });
    return diagnostics;
  }
  const compression = root.children.find(
    (child) => isList(child) && head(child) === 'compression-contract',
  );
  if (!compression) {
    diagnostics.push({ file, message: 'missing (compression-contract ...) section' });
    return diagnostics;
  }
  const props = readKeywordProps(compression, { start: 1 });
  const checks = props[':checks']?.value;
  const checkStrings = checks && isList(checks)
    ? checks.children.map((child) => nodeText(child)).filter((value) => value != null)
    : [];
  if (!checkStrings.includes(CHECK_COMMAND)) {
    diagnostics.push({
      file,
      message: `compression-contract :checks must include "${CHECK_COMMAND}"`,
    });
  }
  return diagnostics;
}

export function checkFacadeBudgets(repoRoot, budgets = FACADE_BUDGETS) {
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

function checkRuntimeProjectionFiles(repoRoot) {
  const diagnostics = [];
  const files = [];
  for (const item of REQUIRED_RUNTIME_FILES) {
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
  if (!Array.isArray(surfaces) || surfaces.length !== EXPECTED_SURFACES.length) {
    diagnostics.push({
      file: 'scripts/check-v3-code-isomorphism-complete.mjs',
      message: `expected ${EXPECTED_SURFACES.length} V3 surfaces, got ${surfaces.length}`,
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

function checkCoverageSummary(coverage) {
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
  if (coverage.code_aligned_surfaces < EXPECTED_SURFACES.length) {
    diagnostics.push({
      file: 'scripts/check-v3-v2-coverage.mjs',
      message: `expected ${EXPECTED_SURFACES.length} code-aligned surfaces, got ${coverage.code_aligned_surfaces}`,
    });
  }
  return diagnostics;
}

function runCheck(repoRoot, check) {
  const proc = spawnSync(process.execPath, check.argv, {
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
    command: `node ${check.argv.join(' ')}`,
    ok: ok && (!check.json || jsonData != null),
    exit_code: proc.status,
    json_data: jsonData,
    stdout_tail: tail(proc.stdout ?? ''),
    stderr_tail: tail(proc.stderr ?? ''),
    error,
  };
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
    const goodDiagnostics = checkBlueprintClosure(goodBlueprint, '<fixture-good>');
    cases.push(assertCase(
      'blueprint closure accepts all required needles and final check command',
      goodDiagnostics.length === 0,
      goodDiagnostics,
    ));

    const missingFinalDiagnostics = checkBlueprintClosure(
      fixtureBlueprint({ includeFinalCheck: false }),
      '<fixture-missing-final>',
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
    const okFacades = checkFacadeBudgets(facadeRoot);
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
    const badFacades = checkFacadeBudgets(badRoot);
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
