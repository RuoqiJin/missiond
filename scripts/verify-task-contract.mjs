#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import path from 'node:path';
import {
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  pathMatchesAny,
  pathMatchesPattern,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/verify-task-contract.mjs <task.lisp> [--commit <hash>] [--json]
  node scripts/verify-task-contract.mjs --dry-fixture [--json]

Verifies a MissionD task-contract v1 Lisp file against a completed git commit:
  - resolves the commit (default HEAD) and confirms it exists
  - checks the commit message contains the contract :commit :message
  - checks every changed file is inside :write-scope when :commit :scope-check
    is "write-scope-only"
  - checks no changed file overlaps :must-not-touch (always enforced)

This verifier is read-only: it never invokes git add, commit, reset, checkout,
stash, push, merge, rebase, or any other mutating git command.

Use --dry-fixture to run self-contained fixtures (no git access, no disk I/O
beyond loading this script). Combine with --json for machine-readable output.
`;

const SCHEMA = 'missiond.task-contract.v1';
const ALLOWED_SCOPE_CHECK = new Set(['write-scope-only', 'none', 'not-required']);

function main() {
  const argv = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  let commitArg = null;
  const positionals = [];

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else if (arg === '--commit') {
      commitArg = argv[++i];
      if (!commitArg) failUsage('--commit requires a value');
    } else if (arg.startsWith('--commit=')) {
      commitArg = arg.slice('--commit='.length);
    } else if (arg.startsWith('--')) {
      failUsage(`unknown flag: ${arg}`);
    } else {
      positionals.push(arg);
    }
  }

  if (dryFixture) {
    runFixtures({ json });
    return;
  }

  if (positionals.length !== 1) {
    failUsage('expected exactly one task.lisp path (or --dry-fixture)');
  }

  const taskFile = path.resolve(process.cwd(), positionals[0]);
  let contract;
  try {
    contract = loadContract(taskFile);
  } catch (err) {
    emitResult({ json, ok: false, file: taskFile, errors: [String(err.message ?? err)] });
    process.exit(1);
  }

  let commitInfo;
  try {
    commitInfo = readCommit(commitArg ?? 'HEAD');
  } catch (err) {
    emitResult({
      json,
      ok: false,
      file: taskFile,
      taskId: contract.id,
      errors: [`failed to read git commit ${commitArg ?? 'HEAD'}: ${err.message ?? err}`],
    });
    process.exit(1);
  }

  const result = verifyContract(contract, commitInfo);
  emitResult({
    json,
    ok: result.ok,
    file: taskFile,
    taskId: contract.id,
    commit: commitInfo,
    contract: {
      message: contract.commit.message,
      scopeCheck: contract.commit.scopeCheck,
      writeScope: contract.writeScope,
      mustNotTouch: contract.mustNotTouch,
    },
    errors: result.errors,
    warnings: result.warnings,
  });
  process.exit(result.ok ? 0 : 1);
}

function failUsage(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

// --- Contract loading -----------------------------------------------------

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
  if (!taskForm) {
    throw new Error(`${file}: no (task ...) form found`);
  }
  const id = nodeText(taskForm.children[1]) ?? '<missing>';
  const props = readKeywordProps(taskForm, { start: 2 });

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    throw new Error(`${file}: :schema must be "${SCHEMA}", got ${schema ?? '<missing>'}`);
  }

  const writeScope = nodeToStringArray(props[':write-scope']?.value);
  const mustNotTouch = nodeToStringArray(props[':must-not-touch']?.value);

  const commitNode = props[':commit']?.value;
  if (!isList(commitNode)) {
    throw new Error(`${file}: :commit must be a property list`);
  }
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

// --- Git access (read-only) ----------------------------------------------

export function readCommit(ref) {
  const hash = execFileSync('git', ['rev-parse', '--verify', `${ref}^{commit}`], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
  const message = execFileSync('git', ['log', '-1', '--format=%B', hash], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  // Use --no-renames so a rename surfaces as both old + new path; that lets
  // the scope check catch a contract that forgets the new path.
  const filesRaw = execFileSync(
    'git',
    ['show', '--no-renames', '--name-only', '--pretty=format:', hash],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  );
  const files = filesRaw
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  return { hash, message: message.replace(/\r\n/g, '\n'), files };
}

// --- Verification core (pure) --------------------------------------------

export function verifyContract(contract, commitInfo) {
  const errors = [];
  const warnings = [];

  if (!contract.commit.required) {
    warnings.push('contract has :commit :required false; verifier still checks scope/forbidden overlaps');
  }

  // Commit message check.
  const expected = contract.commit.message;
  if (expected) {
    if (!commitMessageMatches(commitInfo.message, expected)) {
      errors.push(
        `commit message does not match contract :commit :message\n` +
        `  expected first line: ${JSON.stringify(expected)}\n` +
        `  got first line:      ${JSON.stringify(firstLine(commitInfo.message))}`,
      );
    }
  } else if (contract.commit.required) {
    errors.push('contract :commit :required true but :commit :message missing');
  }

  // Forbidden overlap check (always enforced regardless of scope-check).
  const forbiddenHits = commitInfo.files.filter((f) => pathMatchesAny(f, contract.mustNotTouch));
  if (forbiddenHits.length > 0) {
    errors.push(
      `commit touches files inside :must-not-touch:\n` +
      forbiddenHits.map((f) => `  - ${f}`).join('\n'),
    );
  }

  // Write-scope check.
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

function commitMessageMatches(actual, expected) {
  // Treat the contract :message as the expected commit subject (first line).
  // Trailers like Co-Authored-By and bodies are allowed and not compared.
  return firstLine(actual).trim() === expected.trim();
}

function firstLine(text) {
  const idx = text.indexOf('\n');
  return idx === -1 ? text : text.slice(0, idx);
}

// --- Output ---------------------------------------------------------------

function emitResult(payload) {
  if (payload.json) {
    const { json: _drop, ...rest } = payload;
    console.log(JSON.stringify(rest, null, 2));
    return;
  }
  if (payload.ok) {
    const commit = payload.commit ? ` against ${payload.commit.hash.slice(0, 12)}` : '';
    console.log(`task-contract verify OK: ${payload.taskId ?? '<unknown>'}${commit}`);
    if (payload.warnings && payload.warnings.length) {
      for (const w of payload.warnings) console.warn(`warn: ${w}`);
    }
    return;
  }
  console.error(`task-contract verify FAILED: ${payload.taskId ?? payload.file ?? '<unknown>'}`);
  for (const err of payload.errors ?? []) console.error(`  ${err}`);
  if (payload.warnings && payload.warnings.length) {
    for (const w of payload.warnings) console.warn(`warn: ${w}`);
  }
}

// --- Fixtures -------------------------------------------------------------

function runFixtures({ json }) {
  const baseSource = `(task wave19-fixture
    :schema "missiond.task-contract.v1"
    :title "Fixture"
    :kind code-alignment
    :status ready
    :owner "claudecode"
    :goal "fixture"
    :write-scope ["scripts/verify-task-contract.mjs" "scripts/lib/**"]
    :must-not-touch ["crates/**" ".missiond/v2/*.lisp"]
    :acceptance ["true"]
    :commit (:required true
             :message "feat(tasks): verify task contracts against commits"
             :scope-check write-scope-only))`;

  const baseContract = loadContractFromSource(baseSource, '<fixture-base>');

  const fixtures = [
    {
      name: 'all-green: subject + scope + no forbidden',
      contract: baseContract,
      commit: {
        hash: 'a'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits\n\nbody\n',
        files: ['scripts/verify-task-contract.mjs', 'scripts/lib/missiond_lisp.mjs'],
      },
      expectOk: true,
    },
    {
      name: 'commit subject mismatch',
      contract: baseContract,
      commit: {
        hash: 'b'.repeat(40),
        message: 'chore: something else\n',
        files: ['scripts/verify-task-contract.mjs'],
      },
      expectOk: false,
      expectError: /commit message does not match/,
    },
    {
      name: 'file outside write-scope',
      contract: baseContract,
      commit: {
        hash: 'c'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: ['scripts/verify-task-contract.mjs', 'README.md'],
      },
      expectOk: false,
      expectError: /outside :write-scope/,
    },
    {
      name: 'file inside must-not-touch (crates glob)',
      contract: baseContract,
      commit: {
        hash: 'd'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: ['scripts/verify-task-contract.mjs', 'crates/missiond-core/src/lib.rs'],
      },
      expectOk: false,
      expectError: /must-not-touch/,
    },
    {
      name: 'must-not-touch wins even if also in write-scope (forbidden glob outranks scope)',
      contract: loadContractFromSource(
        baseSource.replace('"crates/**"', '"scripts/lib/forbidden.mjs"'),
        '<fixture-overlap>',
      ),
      commit: {
        hash: 'e'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: ['scripts/lib/forbidden.mjs'],
      },
      expectOk: false,
      expectError: /must-not-touch/,
    },
    {
      name: 'scope-check none allows out-of-scope file (forbidden still enforced)',
      contract: loadContractFromSource(
        baseSource.replace('write-scope-only', 'none'),
        '<fixture-scope-none>',
      ),
      commit: {
        hash: 'f'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: ['docs/anywhere.md'],
      },
      expectOk: true,
    },
    {
      name: 'scope-check none does not bypass must-not-touch',
      contract: loadContractFromSource(
        baseSource.replace('write-scope-only', 'none'),
        '<fixture-scope-none-forbidden>',
      ),
      commit: {
        hash: '0'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: ['crates/missiond-core/src/lib.rs'],
      },
      expectOk: false,
      expectError: /must-not-touch/,
    },
    {
      name: 'glob wildcard in write-scope (scripts/lib/**) matches nested files',
      contract: baseContract,
      commit: {
        hash: '1'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: ['scripts/verify-task-contract.mjs', 'scripts/lib/nested/util.mjs'],
      },
      expectOk: true,
    },
    {
      name: 'empty commit (no files) still passes when message + scope are clean',
      contract: baseContract,
      commit: {
        hash: '2'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: [],
      },
      expectOk: true,
    },
    {
      name: 'pathMatchesPattern sanity: bare pattern matches exact path',
      contract: loadContractFromSource(
        baseSource.replace('["scripts/verify-task-contract.mjs" "scripts/lib/**"]', '["docs/note.md"]'),
        '<fixture-bare>',
      ),
      commit: {
        hash: '3'.repeat(40),
        message: 'feat(tasks): verify task contracts against commits',
        files: ['docs/note.md'],
      },
      expectOk: true,
    },
  ];

  // Internal sanity check on glob helpers.
  const helperCases = [
    ['crates/foo/bar.rs', 'crates/**', true],
    ['scripts/lib/util.mjs', 'scripts/lib/**', true],
    ['scripts/check.mjs', 'scripts/lib/**', false],
    ['.missiond/v2/foo.lisp', '.missiond/v2/*.lisp', true],
    ['.missiond/v2/sub/foo.lisp', '.missiond/v2/*.lisp', false],
    ['docs/note.md', 'docs/note.md', true],
  ];

  const failures = [];
  for (const [p, pat, expected] of helperCases) {
    const got = pathMatchesPattern(p, pat);
    if (got !== expected) {
      failures.push({
        kind: 'helper',
        case: `${p} ~ ${pat}`,
        expected,
        got,
      });
    }
  }

  for (const fx of fixtures) {
    const result = verifyContract(fx.contract, fx.commit);
    const okMatch = result.ok === fx.expectOk;
    let errMatch = true;
    if (!fx.expectOk && fx.expectError) {
      errMatch = result.errors.some((e) => fx.expectError.test(e));
    }
    if (!okMatch || !errMatch) {
      failures.push({
        kind: 'fixture',
        name: fx.name,
        expected: { ok: fx.expectOk, errorMatches: fx.expectError?.toString() },
        got: { ok: result.ok, errors: result.errors },
      });
    }
  }

  const ok = failures.length === 0;
  if (json) {
    console.log(JSON.stringify({
      ok,
      fixtures: fixtures.map((fx) => fx.name),
      helperCases: helperCases.length,
      failures,
    }, null, 2));
  } else if (ok) {
    console.log(`task-contract verify fixtures OK (${fixtures.length} fixture${fixtures.length === 1 ? '' : 's'}, ${helperCases.length} helper case${helperCases.length === 1 ? '' : 's'})`);
  } else {
    console.error(`task-contract verify fixtures FAILED — ${failures.length} failure(s)`);
    for (const f of failures) {
      console.error(JSON.stringify(f, null, 2));
    }
  }
  process.exit(ok ? 0 : 1);
}

main();
