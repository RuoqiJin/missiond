#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
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
  - validates known Lisp artifacts (session-trace, shared-memory,
    task-lifecycle-events ledger and standalone events/*.event.lisp files,
    reports/*.report.lisp) using the resolved commit's bytes through the
    existing artifact checker scripts; this catches worker-commit defects
    even after later parent hotfixes have repaired the working tree.

Artifact validation runs in the CLI path only. Importers that pull
verifyContract(contract, commitInfo) keep getting the existing pure result;
no disk or child-process side effects leak into them.

This verifier is read-only: it never invokes git add, commit, reset, checkout,
stash, push, merge, rebase, or any other mutating git command.

Use --dry-fixture to run self-contained fixtures (no git access, no spawned
child checkers). Combine with --json for machine-readable output.
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
  const errors = [...result.errors];
  const warnings = [...result.warnings];

  // Artifact validation runs only on the CLI path so verifyContract stays
  // pure for importers (verify-task-run, verify-task-runner-batch).
  let artifactReport = null;
  try {
    artifactReport = validateCommitArtifacts(contract, commitInfo);
    for (const err of artifactReport.errors) errors.push(err);
    for (const warn of artifactReport.warnings) warnings.push(warn);
  } catch (err) {
    errors.push(`failed to validate commit artifacts: ${err.message ?? err}`);
  }

  const ok = errors.length === 0;
  emitResult({
    json,
    ok,
    file: taskFile,
    taskId: contract.id,
    commit: commitInfo,
    contract: {
      message: contract.commit.message,
      scopeCheck: contract.commit.scopeCheck,
      writeScope: contract.writeScope,
      mustNotTouch: contract.mustNotTouch,
    },
    artifacts: artifactReport
      ? {
          plan: artifactReport.plan,
          checked: artifactReport.checked,
          skipped: artifactReport.skipped,
        }
      : null,
    errors,
    warnings,
  });
  process.exit(ok ? 0 : 1);
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

// --- Commit-byte artifact validation (CLI-side I/O) ----------------------

// Known artifact path patterns and the existing checker that owns each
// schema. Patterns match repo-relative paths under .missiond/tasks/<wave>/.
// Adding a new schema here is intentional — the rule list is the entire
// surface that ties verify-task-contract to the per-artifact checkers and
// keeps schema validation out of this file.
export const ARTIFACT_RULES = [
  {
    name: 'session-trace',
    checker: 'scripts/check-session-trace.mjs',
    match: (p) => /^\.missiond\/tasks\/[^/]+\/session-trace\.lisp$/.test(p),
  },
  {
    name: 'shared-memory',
    checker: 'scripts/check-task-memory.mjs',
    match: (p) => /^\.missiond\/tasks\/[^/]+\/shared-memory\.lisp$/.test(p),
  },
  {
    name: 'task-lifecycle-events-ledger',
    checker: 'scripts/check-task-lifecycle-events.mjs',
    match: (p) => /^\.missiond\/tasks\/[^/]+\/task-lifecycle-events\.lisp$/.test(p),
  },
  {
    name: 'task-lifecycle-event-file',
    checker: 'scripts/check-task-lifecycle-events.mjs',
    match: (p) => /^\.missiond\/tasks\/[^/]+\/events\/[^/]+\.event\.lisp$/.test(p),
  },
  {
    name: 'task-report',
    checker: 'scripts/check-task-report.mjs',
    match: (p) => /^\.missiond\/tasks\/[^/]+\/reports\/[^/]+\.report\.lisp$/.test(p),
  },
];

// Plan which artifacts to validate. Pure: takes a contract + commitInfo and
// returns a list of {path, rule, checker} entries. Candidates come from the
// contract's :write-scope (so worker-touched artifacts that the commit's
// diff did not modify are still checked at the worker tree) plus the
// commit's modified files (so commits that lift artifacts not declared in
// :write-scope still validate them). Each path is matched against ARTIFACT_RULES
// once (first hit wins) to keep the plan deterministic.
export function planArtifactValidation(contract, commitInfo) {
  const seen = new Set();
  const candidates = [];
  const push = (p) => {
    if (typeof p !== 'string' || p.length === 0) return;
    if (seen.has(p)) return;
    seen.add(p);
    candidates.push(p);
  };
  for (const p of contract.writeScope ?? []) push(p);
  for (const p of commitInfo.files ?? []) push(p);

  const plan = [];
  for (const candidate of candidates) {
    if (containsGlob(candidate)) continue;
    for (const rule of ARTIFACT_RULES) {
      if (rule.match(candidate)) {
        plan.push({ path: candidate, rule: rule.name, checker: rule.checker });
        break;
      }
    }
  }
  return plan;
}

function containsGlob(p) {
  return p.includes('*') || p.includes('?') || p.includes('[');
}

// CLI-side: materialize commit bytes for each planned artifact via
// `git show <commit>:<path>`, write into a temp tree that preserves the
// repo-relative path so checkers that infer wave/file naming (e.g.
// events/<seq>.event.lisp) keep working, then spawn the checker.
export function validateCommitArtifacts(contract, commitInfo, options = {}) {
  const repoRoot = options.repoRoot ?? process.cwd();
  const plan = planArtifactValidation(contract, commitInfo);
  const checked = [];
  const skipped = [];
  const errors = [];
  const warnings = [];
  if (plan.length === 0) {
    return { plan, checked, skipped, errors, warnings };
  }

  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-verify-task-contract-'));
  try {
    for (const item of plan) {
      const bytes = readCommitBytes(commitInfo.hash, item.path, repoRoot);
      if (bytes == null) {
        skipped.push({ ...item, reason: 'missing-in-commit' });
        continue;
      }
      const targetPath = path.join(tmpRoot, item.path);
      fs.mkdirSync(path.dirname(targetPath), { recursive: true });
      fs.writeFileSync(targetPath, bytes);
      const checkerPath = path.join(repoRoot, item.checker);
      const result = spawnSync(process.execPath, [checkerPath, targetPath], {
        cwd: repoRoot,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      const status = result.status;
      const checkerOutput = ((result.stdout ?? '') + (result.stderr ?? '')).trim();
      const entry = {
        ...item,
        commit_hash: commitInfo.hash,
        exit_code: status,
        ok: status === 0,
      };
      checked.push(entry);
      if (status !== 0) {
        const head = `artifact ${item.path} (${item.rule}) failed validation at commit ${commitInfo.hash.slice(0, 12)} via ${item.checker}`;
        const tail = checkerOutput.length > 0 ? `\n${indent(checkerOutput, '  ')}` : '';
        errors.push(`${head}${tail}`);
      } else if (checkerOutput.length > 0 && options.surfaceCheckerStdout === true) {
        warnings.push(`${item.checker} ${item.path}: ${checkerOutput}`);
      }
    }
  } finally {
    rmrf(tmpRoot);
  }

  return { plan, checked, skipped, errors, warnings };
}

function readCommitBytes(commitHash, repoRelPath, repoRoot) {
  const result = spawnSync('git', ['show', `${commitHash}:${repoRelPath}`], {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (result.status === 0) {
    return result.stdout;
  }
  // git show exits non-zero when the path is missing in that commit's tree.
  return null;
}

function indent(text, prefix) {
  return text
    .split('\n')
    .map((line) => `${prefix}${line}`)
    .join('\n');
}

function rmrf(target) {
  try {
    fs.rmSync(target, { recursive: true, force: true });
  } catch {
    // best-effort temp cleanup; nothing to do if removal fails
  }
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

  // Artifact validation plan fixtures (pure: planArtifactValidation only).
  const artifactPlanContractSource = `(task wave52-fixture
    :schema "missiond.task-contract.v1"
    :title "Fixture"
    :kind code-alignment
    :status ready
    :owner "claudecode"
    :goal "fixture"
    :write-scope [".missiond/tasks/wave99/session-trace.lisp"
                  ".missiond/tasks/wave99/shared-memory.lisp"
                  ".missiond/tasks/wave99/task-lifecycle-events.lisp"
                  ".missiond/tasks/wave99/events/000001.event.lisp"
                  ".missiond/tasks/wave99/reports/wave99-01-fixture.report.lisp"
                  "scripts/verify-task-contract.mjs"]
    :must-not-touch ["packages/**"]
    :acceptance ["true"]
    :commit (:required true
             :message "fix(tasks): validate lisp artifacts during contract verify"
             :scope-check write-scope-only))`;
  const artifactPlanContract = loadContractFromSource(
    artifactPlanContractSource,
    '<fixture-artifact-plan>',
  );
  const planCases = [
    {
      name: 'plan from write-scope: every known artifact rule fires once',
      contract: artifactPlanContract,
      commit: { hash: 'a'.repeat(40), message: 'msg', files: [] },
      expect: [
        { path: '.missiond/tasks/wave99/session-trace.lisp', rule: 'session-trace' },
        { path: '.missiond/tasks/wave99/shared-memory.lisp', rule: 'shared-memory' },
        {
          path: '.missiond/tasks/wave99/task-lifecycle-events.lisp',
          rule: 'task-lifecycle-events-ledger',
        },
        {
          path: '.missiond/tasks/wave99/events/000001.event.lisp',
          rule: 'task-lifecycle-event-file',
        },
        {
          path: '.missiond/tasks/wave99/reports/wave99-01-fixture.report.lisp',
          rule: 'task-report',
        },
      ],
    },
    {
      name: 'plan picks up commit-only artifacts that are not declared in :write-scope',
      contract: loadContractFromSource(
        artifactPlanContractSource.replace(
          '".missiond/tasks/wave99/session-trace.lisp"\n                  ',
          '',
        ),
        '<fixture-artifact-plan-trim>',
      ),
      commit: {
        hash: 'b'.repeat(40),
        message: 'msg',
        files: ['.missiond/tasks/wave99/session-trace.lisp'],
      },
      expect: [
        { path: '.missiond/tasks/wave99/shared-memory.lisp', rule: 'shared-memory' },
        {
          path: '.missiond/tasks/wave99/task-lifecycle-events.lisp',
          rule: 'task-lifecycle-events-ledger',
        },
        {
          path: '.missiond/tasks/wave99/events/000001.event.lisp',
          rule: 'task-lifecycle-event-file',
        },
        {
          path: '.missiond/tasks/wave99/reports/wave99-01-fixture.report.lisp',
          rule: 'task-report',
        },
        { path: '.missiond/tasks/wave99/session-trace.lisp', rule: 'session-trace' },
      ],
    },
    {
      name: 'plan ignores glob entries (write-scope **) and unrelated paths',
      contract: loadContractFromSource(
        `(task wave52-fixture
          :schema "missiond.task-contract.v1"
          :title "Fixture"
          :kind code-alignment
          :status ready
          :owner "claudecode"
          :goal "fixture"
          :write-scope [".missiond/tasks/wave99/events/**"
                        "scripts/verify-task-contract.mjs"
                        "README.md"]
          :must-not-touch ["packages/**"]
          :acceptance ["true"]
          :commit (:required true
                   :message "msg"
                   :scope-check write-scope-only))`,
        '<fixture-artifact-plan-glob>',
      ),
      commit: { hash: 'c'.repeat(40), message: 'msg', files: ['scripts/verify-task-contract.mjs'] },
      expect: [],
    },
  ];

  for (const fx of planCases) {
    const got = planArtifactValidation(fx.contract, fx.commit).map((entry) => ({
      path: entry.path,
      rule: entry.rule,
    }));
    const same =
      got.length === fx.expect.length &&
      got.every(
        (entry, i) => entry.path === fx.expect[i].path && entry.rule === fx.expect[i].rule,
      );
    if (!same) {
      failures.push({
        kind: 'artifact-plan',
        name: fx.name,
        expected: fx.expect,
        got,
      });
    }
  }

  // Negative artifact regression: an invalid session-trace bytes string must
  // be rejected by the bound checker. We materialize the bytes into a temp
  // file (no git) and spawn check-session-trace.mjs directly.
  const invalidTrace = `(session-trace wave99
    :schema "missiond.session-trace.v1"
    :wave wave99
    :created-at "2026-04-29T00:00:00Z"
    :sequence 1
    (trace-event
      :id wave99-trace-bad-001
      :seq 1
      :at "2026-04-29T00:00:00Z"
      :task wave99-01
      :backend claudecode
      :kind acceptance
      :summary "invalid kind"))`;
  const tracePlan = [
    {
      path: '.missiond/tasks/wave99/session-trace.lisp',
      rule: 'session-trace',
      checker: 'scripts/check-session-trace.mjs',
    },
  ];
  const traceCheck = runArtifactPlanWithBytes(tracePlan, {
    '.missiond/tasks/wave99/session-trace.lisp': invalidTrace,
  });
  if (traceCheck.errors.length === 0) {
    failures.push({
      kind: 'artifact-checker',
      name: 'invalid session-trace :kind acceptance bytes must fail check-session-trace',
      got: traceCheck,
    });
  } else if (
    !traceCheck.errors.some((err) => /session-trace|acceptance/.test(err))
  ) {
    failures.push({
      kind: 'artifact-checker',
      name: 'invalid session-trace error must surface session-trace/acceptance text',
      got: traceCheck,
    });
  }

  const ok = failures.length === 0;
  if (json) {
    console.log(JSON.stringify({
      ok,
      fixtures: fixtures.map((fx) => fx.name),
      helperCases: helperCases.length,
      planCases: planCases.length,
      artifactCheckerCases: 1,
      failures,
    }, null, 2));
  } else if (ok) {
    console.log(
      `task-contract verify fixtures OK (${fixtures.length} fixture${fixtures.length === 1 ? '' : 's'}, ${helperCases.length} helper case${helperCases.length === 1 ? '' : 's'}, ${planCases.length} artifact-plan case${planCases.length === 1 ? '' : 's'}, 1 artifact-checker case)`,
    );
  } else {
    console.error(`task-contract verify fixtures FAILED — ${failures.length} failure(s)`);
    for (const f of failures) {
      console.error(JSON.stringify(f, null, 2));
    }
  }
  process.exit(ok ? 0 : 1);
}

// Self-contained artifact-checker harness used by --dry-fixture: writes the
// supplied bytes for each plan entry into a temp tree and runs the bound
// checker against the materialized file. This avoids any dependency on git
// while still exercising the real spawn-checker code path used in
// validateCommitArtifacts.
function runArtifactPlanWithBytes(plan, byPath, options = {}) {
  const repoRoot = options.repoRoot ?? process.cwd();
  const checked = [];
  const errors = [];
  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-verify-task-contract-fx-'));
  try {
    for (const item of plan) {
      const bytes = byPath[item.path];
      if (typeof bytes !== 'string') continue;
      const targetPath = path.join(tmpRoot, item.path);
      fs.mkdirSync(path.dirname(targetPath), { recursive: true });
      fs.writeFileSync(targetPath, bytes);
      const result = spawnSync(process.execPath, [path.join(repoRoot, item.checker), targetPath], {
        cwd: repoRoot,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      const out = ((result.stdout ?? '') + (result.stderr ?? '')).trim();
      checked.push({ ...item, exit_code: result.status, ok: result.status === 0 });
      if (result.status !== 0) {
        errors.push(`artifact ${item.path} (${item.rule}) failed via ${item.checker}\n${indent(out, '  ')}`);
      }
    }
  } finally {
    rmrf(tmpRoot);
  }
  return { plan, checked, errors };
}

// Run as CLI only when invoked directly. When imported (e.g. by
// scripts/verify-task-run.mjs) the module-level main() must NOT execute, or
// it would re-parse the importer's argv and crash on unknown flags.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
