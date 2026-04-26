#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  pathMatchesAny,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

// We deliberately do NOT static-import scripts/verify-task-contract.mjs because
// that file runs main() at module load. Commit-mode delegates to it via a
// child process (true delegation, no side effects). Contract loading is shared
// by re-implementing the same v1 schema parse against scripts/lib/missiond_lisp.mjs,
// which keeps both scripts honoring identical glob + keyword-prop semantics.

const VERIFY_TASK_CONTRACT_PATH = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  'verify-task-contract.mjs',
);
const SCHEMA = 'missiond.task-contract.v1';
const ALLOWED_SCOPE_CHECK = new Set(['write-scope-only', 'none', 'not-required']);

const usage = `Usage:
  node scripts/task-scope-guard.mjs --task <task.lisp> --mode <staged|commit>
                                    [--commit <hash>] [--json]
  node scripts/task-scope-guard.mjs --dry-fixture [--json]

Task-contract aware scope guard. Two modes:

  staged   reads \`git diff --cached --name-only\` and fails if any staged path
           is outside :write-scope or matches :must-not-touch in the contract.
           Designed to run from a pre-commit hook so the commit aborts before
           the index is locked in.

  commit   resolves a git commit (default HEAD) and delegates to
           verify-task-contract semantics: scope check + forbidden overlap
           check. Commit subject (\`:commit :message\`) is enforced as well.

Use --dry-fixture to run self-contained fixtures (no git access). Combine with
--json for machine-readable output.

This guard is read-only: it never invokes git add, commit, reset, checkout,
stash, push, merge, or rebase. It only runs git diff / git rev-parse / git
log / git show with --name-only and --pretty=format formatters.
`;

const ALLOWED_MODES = new Set(['staged', 'commit']);

function main() {
  const argv = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  let mode = null;
  let taskFile = null;
  let commitArg = null;

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else if (arg === '--task') {
      taskFile = argv[++i];
      if (!taskFile) failUsage('--task requires a value');
    } else if (arg.startsWith('--task=')) {
      taskFile = arg.slice('--task='.length);
    } else if (arg === '--mode') {
      mode = argv[++i];
      if (!mode) failUsage('--mode requires a value');
    } else if (arg.startsWith('--mode=')) {
      mode = arg.slice('--mode='.length);
    } else if (arg === '--commit') {
      commitArg = argv[++i];
      if (!commitArg) failUsage('--commit requires a value');
    } else if (arg.startsWith('--commit=')) {
      commitArg = arg.slice('--commit='.length);
    } else if (arg.startsWith('--')) {
      failUsage(`unknown flag: ${arg}`);
    } else {
      failUsage(`unexpected positional argument: ${arg}`);
    }
  }

  if (dryFixture) {
    runFixtures({ json });
    return;
  }

  if (!taskFile) failUsage('--task is required (or pass --dry-fixture)');
  if (!mode) failUsage('--mode is required: staged|commit (or pass --dry-fixture)');
  if (!ALLOWED_MODES.has(mode)) failUsage(`--mode must be one of ${[...ALLOWED_MODES].join('|')}`);
  if (commitArg && mode !== 'commit') {
    failUsage('--commit is only valid when --mode commit');
  }

  const resolvedTask = path.resolve(process.cwd(), taskFile);
  let contract;
  try {
    contract = loadContract(resolvedTask);
  } catch (err) {
    emitResult({
      json,
      ok: false,
      mode,
      file: resolvedTask,
      errors: [`failed to load task contract: ${err.message ?? err}`],
    });
    process.exit(1);
  }

  if (mode === 'staged') {
    runStagedMode({ contract, json });
    return;
  }

  runCommitMode({ contract, json, commitArg });
}

function failUsage(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

// --- staged mode ----------------------------------------------------------

function runStagedMode({ contract, json }) {
  let stagedFiles;
  try {
    stagedFiles = readStagedFiles();
  } catch (err) {
    emitResult({
      json,
      ok: false,
      mode: 'staged',
      file: contract.file,
      taskId: contract.id,
      errors: [`failed to read git index: ${err.message ?? err}`],
    });
    process.exit(1);
  }

  const result = checkScope(contract, stagedFiles);
  emitResult({
    json,
    ok: result.ok,
    mode: 'staged',
    file: contract.file,
    taskId: contract.id,
    contract: {
      writeScope: contract.writeScope,
      mustNotTouch: contract.mustNotTouch,
      scopeCheck: contract.commit.scopeCheck,
    },
    stagedFiles,
    errors: result.errors,
    warnings: result.warnings,
  });
  process.exit(result.ok ? 0 : 1);
}

function readStagedFiles() {
  const raw = execFileSync(
    'git',
    ['diff', '--cached', '--name-only', '--no-renames', '-z'],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  );
  return raw
    .split('\0')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

// --- commit mode ----------------------------------------------------------

function runCommitMode({ contract, json, commitArg }) {
  // Delegate to scripts/verify-task-contract.mjs as a child process so the
  // single source of truth for commit-vs-contract semantics stays in the
  // existing verifier (subject + scope + must-not-touch). The guard only
  // forwards stdout/stderr + exit code; it never mutates anything.
  const args = [VERIFY_TASK_CONTRACT_PATH, contract.file];
  if (commitArg) args.push('--commit', commitArg);
  if (json) args.push('--json');

  const child = spawnSync('node', args, { stdio: 'inherit' });
  if (child.error) {
    emitResult({
      json,
      ok: false,
      mode: 'commit',
      file: contract.file,
      taskId: contract.id,
      errors: [`failed to invoke verify-task-contract: ${child.error.message ?? child.error}`],
    });
    process.exit(1);
  }
  process.exit(child.status ?? 1);
}

// --- contract loading (mirrors verify-task-contract loader exactly) -------

export function loadContract(file) {
  const forms = readLispFile(file);
  return contractFromForms(forms, file);
}

export function loadContractFromSource(source, file = '<memory>') {
  const forms = parseLisp(source, file);
  return contractFromForms(forms, file);
}

function contractFromForms(forms, file) {
  const taskForm = forms.find((form) => isList(form) && head(form) === 'task');
  if (!taskForm) throw new Error(`${file}: no (task ...) form found`);
  const id = nodeText(taskForm.children[1]) ?? '<missing>';
  const props = readKeywordProps(taskForm, { start: 2 });

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    throw new Error(`${file}: :schema must be "${SCHEMA}", got ${schema ?? '<missing>'}`);
  }

  const writeScope = nodeToStringArray(props[':write-scope']?.value);
  const mustNotTouch = nodeToStringArray(props[':must-not-touch']?.value);

  const commitNode = props[':commit']?.value;
  if (!isList(commitNode)) throw new Error(`${file}: :commit must be a property list`);
  const commitProps = readKeywordProps(commitNode, { start: 0 });
  const required = keywordPropBool(commitProps, ':required');
  const message = keywordPropText(commitProps, ':message');
  const scopeCheck = keywordPropText(commitProps, ':scope-check');
  if (scopeCheck && !ALLOWED_SCOPE_CHECK.has(scopeCheck)) {
    throw new Error(`${file}: :commit :scope-check must be one of ${[...ALLOWED_SCOPE_CHECK].join('|')}`);
  }

  return {
    id,
    file,
    writeScope,
    mustNotTouch,
    commit: {
      required: required === true,
      message: message ?? null,
      scopeCheck: scopeCheck ?? null,
    },
  };
}

// Local commit-style verifier used only in --dry-fixture mode so the test
// suite can exercise commit semantics without a child process. Production
// commit-mode delegates to scripts/verify-task-contract.mjs.
function verifyContractLocal(contract, commitInfo) {
  const errors = [];
  const warnings = [];
  if (!contract.commit.required) {
    warnings.push('contract has :commit :required false; guard still checks scope/forbidden overlaps');
  }
  const expected = contract.commit.message;
  if (expected) {
    const firstLine = (text) => {
      const idx = text.indexOf('\n');
      return idx === -1 ? text : text.slice(0, idx);
    };
    if (firstLine(commitInfo.message).trim() !== expected.trim()) {
      errors.push(
        `commit message does not match contract :commit :message\n` +
        `  expected first line: ${JSON.stringify(expected)}\n` +
        `  got first line:      ${JSON.stringify(firstLine(commitInfo.message))}`,
      );
    }
  }
  const forbiddenHits = commitInfo.files.filter((f) => pathMatchesAny(f, contract.mustNotTouch));
  if (forbiddenHits.length > 0) {
    errors.push(
      `commit touches files inside :must-not-touch:\n` +
      forbiddenHits.map((f) => `  - ${f}`).join('\n'),
    );
  }
  if (contract.commit.scopeCheck === 'write-scope-only') {
    const outOfScope = commitInfo.files.filter((f) => !pathMatchesAny(f, contract.writeScope));
    if (outOfScope.length > 0) {
      errors.push(
        `commit touches files outside :write-scope (scope-check=write-scope-only):\n` +
        outOfScope.map((f) => `  - ${f}`).join('\n'),
      );
    }
  }
  return { ok: errors.length === 0, errors, warnings };
}

// --- pure scope check (shared semantics for staged + tests) ---------------

export function checkScope(contract, files) {
  const errors = [];
  const warnings = [];

  const forbiddenHits = files.filter((f) => pathMatchesAny(f, contract.mustNotTouch));
  if (forbiddenHits.length > 0) {
    errors.push(
      `staged files inside :must-not-touch:\n` +
      forbiddenHits.map((f) => `  - ${f}`).join('\n'),
    );
  }

  if (contract.commit.scopeCheck === 'write-scope-only') {
    const outOfScope = files.filter((f) => !pathMatchesAny(f, contract.writeScope));
    if (outOfScope.length > 0) {
      errors.push(
        `staged files outside :write-scope (scope-check=write-scope-only):\n` +
        outOfScope.map((f) => `  - ${f}`).join('\n'),
      );
    }
  } else if (contract.commit.scopeCheck !== 'none' && contract.commit.scopeCheck !== 'not-required') {
    warnings.push(
      `contract :commit :scope-check is "${contract.commit.scopeCheck}"; staged-mode only enforces write-scope when scope-check is "write-scope-only"`,
    );
  }

  return { ok: errors.length === 0, errors, warnings };
}

// --- output ---------------------------------------------------------------

function emitResult(payload) {
  if (payload.json) {
    const { json: _drop, ...rest } = payload;
    console.log(JSON.stringify(rest, null, 2));
    return;
  }
  if (payload.ok) {
    const ctx = payload.mode === 'staged'
      ? `${payload.stagedFiles?.length ?? 0} staged file(s)`
      : `commit ${payload.commit?.hash?.slice(0, 12) ?? '<unknown>'}`;
    console.log(`task-scope-guard ${payload.mode} OK: ${payload.taskId ?? '<unknown>'} (${ctx})`);
    if (payload.warnings && payload.warnings.length) {
      for (const w of payload.warnings) console.warn(`warn: ${w}`);
    }
    return;
  }
  console.error(`task-scope-guard ${payload.mode ?? '?'} FAILED: ${payload.taskId ?? payload.file ?? '<unknown>'}`);
  for (const err of payload.errors ?? []) console.error(`  ${err}`);
  if (payload.warnings && payload.warnings.length) {
    for (const w of payload.warnings) console.warn(`warn: ${w}`);
  }
}

// --- fixtures -------------------------------------------------------------

function runFixtures({ json }) {
  const baseSource = `(task wave20-fixture
    :schema "missiond.task-contract.v1"
    :title "Fixture"
    :kind code-alignment
    :status ready
    :owner "claudecode"
    :goal "fixture"
    :write-scope ["scripts/task-scope-guard.mjs"
                  ".githooks/pre-commit"
                  ".missiond/tasks/schema/task-contract-v1.lisp"]
    :must-not-touch ["crates/**"
                     ".missiond/v2/*.lisp"
                     ".missiond/tasks/wave19/**"
                     ".missiond/claudecode/wave19-*.md"]
    :acceptance ["true"]
    :commit (:required true
             :message "feat(tasks): guard staged files by task scope"
             :scope-check write-scope-only))`;

  const baseContract = loadContractFromSource(baseSource, '<fixture-base>');

  const stagedFixtures = [
    {
      name: 'staged: all-green minimal write-scope-only',
      contract: baseContract,
      files: ['scripts/task-scope-guard.mjs'],
      expectOk: true,
    },
    {
      name: 'staged: all-green full ownership set',
      contract: baseContract,
      files: [
        'scripts/task-scope-guard.mjs',
        '.githooks/pre-commit',
        '.missiond/tasks/schema/task-contract-v1.lisp',
      ],
      expectOk: true,
    },
    {
      name: 'staged: empty index passes (nothing to violate)',
      contract: baseContract,
      files: [],
      expectOk: true,
    },
    {
      name: 'staged: file outside write-scope (the wave19 pollution case)',
      contract: baseContract,
      files: [
        'scripts/task-scope-guard.mjs',
        '.missiond/tasks/wave19/wave19-04-shared-memory-ledger-v0.lisp',
      ],
      expectOk: false,
      expectError: /must-not-touch|outside :write-scope/,
    },
    {
      name: 'staged: must-not-touch glob (crates/**) blocks staged Rust file',
      contract: baseContract,
      files: ['crates/missiond-core/src/lib.rs'],
      expectOk: false,
      expectError: /must-not-touch/,
    },
    {
      name: 'staged: must-not-touch wildcard (.missiond/v2/*.lisp) blocks v2 lisp',
      contract: baseContract,
      files: ['.missiond/v2/intent-event-bus.lisp'],
      expectOk: false,
      expectError: /must-not-touch/,
    },
    {
      name: 'staged: out-of-scope claudecode brief is rejected',
      contract: baseContract,
      files: ['.missiond/claudecode/wave20-01-task-scope-index-guard-v1.md'],
      expectOk: false,
      expectError: /outside :write-scope/,
    },
    {
      name: 'staged: scope-check=none allows out-of-scope (forbidden still enforced)',
      contract: loadContractFromSource(
        baseSource.replace('write-scope-only', 'none'),
        '<fixture-scope-none>',
      ),
      files: ['docs/anywhere.md'],
      expectOk: true,
    },
    {
      name: 'staged: scope-check=none does not bypass must-not-touch',
      contract: loadContractFromSource(
        baseSource.replace('write-scope-only', 'none'),
        '<fixture-scope-none-forbidden>',
      ),
      files: ['crates/missiond-core/src/lib.rs'],
      expectOk: false,
      expectError: /must-not-touch/,
    },
  ];

  const commitFixtures = [
    {
      name: 'commit: subject + scope match',
      contract: baseContract,
      commit: {
        hash: 'a'.repeat(40),
        message: 'feat(tasks): guard staged files by task scope\n\nbody\n',
        files: ['scripts/task-scope-guard.mjs', '.githooks/pre-commit'],
      },
      expectOk: true,
    },
    {
      name: 'commit: subject mismatch fails',
      contract: baseContract,
      commit: {
        hash: 'b'.repeat(40),
        message: 'chore: drift\n',
        files: ['scripts/task-scope-guard.mjs'],
      },
      expectOk: false,
      expectError: /commit message does not match/,
    },
    {
      name: 'commit: forbidden hit fails even with right subject',
      contract: baseContract,
      commit: {
        hash: 'c'.repeat(40),
        message: 'feat(tasks): guard staged files by task scope',
        files: ['scripts/task-scope-guard.mjs', '.missiond/v2/intent-event-bus.lisp'],
      },
      expectOk: false,
      expectError: /must-not-touch/,
    },
  ];

  const failures = [];

  for (const fx of stagedFixtures) {
    const result = checkScope(fx.contract, fx.files);
    const okMatch = result.ok === fx.expectOk;
    let errMatch = true;
    if (!fx.expectOk && fx.expectError) {
      errMatch = result.errors.some((e) => fx.expectError.test(e));
    }
    if (!okMatch || !errMatch) {
      failures.push({
        kind: 'staged',
        name: fx.name,
        expected: { ok: fx.expectOk, errorMatches: fx.expectError?.toString() },
        got: { ok: result.ok, errors: result.errors, warnings: result.warnings },
      });
    }
  }

  for (const fx of commitFixtures) {
    const result = verifyContractLocal(fx.contract, fx.commit);
    const okMatch = result.ok === fx.expectOk;
    let errMatch = true;
    if (!fx.expectOk && fx.expectError) {
      errMatch = result.errors.some((e) => fx.expectError.test(e));
    }
    if (!okMatch || !errMatch) {
      failures.push({
        kind: 'commit',
        name: fx.name,
        expected: { ok: fx.expectOk, errorMatches: fx.expectError?.toString() },
        got: { ok: result.ok, errors: result.errors, warnings: result.warnings },
      });
    }
  }

  const ok = failures.length === 0;
  const summary = {
    ok,
    stagedFixtures: stagedFixtures.map((fx) => fx.name),
    commitFixtures: commitFixtures.map((fx) => fx.name),
    failures,
  };

  if (json) {
    console.log(JSON.stringify(summary, null, 2));
  } else if (ok) {
    console.log(
      `task-scope-guard fixtures OK ` +
      `(${stagedFixtures.length} staged + ${commitFixtures.length} commit)`,
    );
  } else {
    console.error(`task-scope-guard fixtures FAILED — ${failures.length} failure(s)`);
    for (const f of failures) console.error(JSON.stringify(f, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

main();
