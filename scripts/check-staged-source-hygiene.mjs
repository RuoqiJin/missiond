#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const SCRIPT_DIR = path.dirname(SCRIPT_PATH);
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..');
const TASK_SCOPE_GUARD = path.join(SCRIPT_DIR, 'task-scope-guard.mjs');

const usage = `Usage:
  node scripts/check-staged-source-hygiene.mjs [--task <task.lisp>] [--json]
  node scripts/check-staged-source-hygiene.mjs --files <path> [<path> ...] [--json]
  node scripts/check-staged-source-hygiene.mjs --dry-fixture [--json]

Read-only staged source hygiene preflight.

Default mode checks the staged index:
  - raw NUL bytes in staged blobs
  - \`git diff --cached --check\` whitespace errors
  - task-scope guard readiness when --task or MISSIOND_TASK_CONTRACT is set

--files checks supplied working-tree files without invoking git for content:
  - raw NUL bytes in each supplied file
  - whole-file whitespace hygiene matching git's common diff --check errors

This command never mutates git config, the index, the working tree, or hooks.
`;

function main() {
  const argv = process.argv.slice(2);
  const opts = parseArgs(argv);

  if (opts.help) {
    console.log(usage);
    process.exit(0);
  }

  if (opts.dryFixture) {
    runFixtures({ json: opts.json });
    return;
  }

  const task = opts.task ?? process.env.MISSIOND_TASK_CONTRACT ?? null;
  const result = opts.files.length > 0
    ? checkSuppliedFiles({ files: opts.files, cwd: process.cwd() })
    : checkStagedSourceHygiene({ task });

  emitResult({ json: opts.json, result });
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = {
    help: false,
    json: false,
    dryFixture: false,
    task: null,
    files: [],
  };
  let collectingFiles = false;

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (collectingFiles && !arg.startsWith('--')) {
      opts.files.push(arg);
      continue;
    }
    collectingFiles = false;

    if (arg === '-h' || arg === '--help') {
      opts.help = true;
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--task') {
      opts.task = argv[++i];
      if (!opts.task) failUsage('--task requires a value');
    } else if (arg.startsWith('--task=')) {
      opts.task = arg.slice('--task='.length);
    } else if (arg === '--files') {
      collectingFiles = true;
      while (argv[i + 1] && !argv[i + 1].startsWith('--')) {
        opts.files.push(argv[++i]);
      }
      if (opts.files.length === 0) failUsage('--files requires at least one path');
    } else if (arg.startsWith('--')) {
      failUsage(`unknown flag: ${arg}`);
    } else {
      failUsage(`unexpected positional argument: ${arg}`);
    }
  }

  if (opts.dryFixture && (opts.task || opts.files.length > 0)) {
    failUsage('--dry-fixture cannot be combined with --task or --files');
  }
  return opts;
}

function failUsage(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

export function checkStagedSourceHygiene({ task = null, git = realGit } = {}) {
  const checks = [];
  const errors = [];
  const warnings = [];

  let stagedFiles = [];
  try {
    stagedFiles = git.readStagedFiles();
    const nul = checkStagedNulBytes({ files: stagedFiles, git });
    checks.push(nul);
    errors.push(...nul.errors);
  } catch (err) {
    const check = failedCheck('staged-nul-bytes', `failed to inspect staged blobs: ${err.message ?? err}`);
    checks.push(check);
    errors.push(...check.errors);
  }

  const diffCheck = runGitDiffCheck({ files: stagedFiles, git });
  checks.push(diffCheck);
  errors.push(...diffCheck.errors);

  const scope = checkTaskScopeGuardReadiness({ task, git });
  checks.push(scope);
  errors.push(...scope.errors);
  warnings.push(...scope.warnings);

  return {
    ok: errors.length === 0,
    mode: 'staged',
    read_only: true,
    staged_files: stagedFiles,
    checks,
    errors,
    warnings,
  };
}

export function checkSuppliedFiles({ files, cwd = process.cwd() }) {
  const normalized = files.map((file) => path.resolve(cwd, file));
  const nul = checkFileNulBytes(normalized);
  const whitespace = checkWholeFileWhitespace(normalized);
  const errors = [...nul.errors, ...whitespace.errors];
  return {
    ok: errors.length === 0,
    mode: 'files',
    read_only: true,
    files: normalized,
    checks: [nul, whitespace],
    errors,
    warnings: [],
  };
}

export function checkFileNulBytes(files) {
  const hits = [];
  const errors = [];

  for (const file of files) {
    let bytes;
    try {
      bytes = fs.readFileSync(file);
    } catch (err) {
      errors.push(`${file}: failed to read file: ${err.message ?? err}`);
      continue;
    }
    if (bytes.includes(0)) hits.push(file);
  }

  if (hits.length > 0) {
    errors.push(
      `raw NUL byte(s) found:\n` +
      hits.map((file) => `  - ${file}`).join('\n'),
    );
  }

  return {
    name: 'nul-bytes',
    ok: errors.length === 0,
    files_checked: files.length,
    hits,
    errors,
  };
}

export function checkWholeFileWhitespace(files) {
  const findings = [];
  const errors = [];

  for (const file of files) {
    let text;
    try {
      const bytes = fs.readFileSync(file);
      text = bytes.toString('utf8');
    } catch (err) {
      errors.push(`${file}: failed to read file for whitespace check: ${err.message ?? err}`);
      continue;
    }

    const lines = text.split(/\n/);
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].endsWith('\r') ? lines[i].slice(0, -1) : lines[i];
      if (/[ \t]+$/.test(line)) {
        findings.push(`${file}:${i + 1}: trailing whitespace`);
      }
      if (/^ *\t/.test(line)) {
        findings.push(`${file}:${i + 1}: space before tab in indent`);
      }
    }
  }

  if (findings.length > 0) {
    errors.push(
      `whitespace error(s) found:\n` +
      findings.map((line) => `  - ${line}`).join('\n'),
    );
  }

  return {
    name: 'whole-file-whitespace',
    ok: errors.length === 0,
    files_checked: files.length,
    findings,
    errors,
  };
}

export function checkStagedNulBytes({ files, git = realGit }) {
  const hits = [];
  for (const file of files) {
    const bytes = git.readStagedBlob(file);
    if (bytes.includes(0)) hits.push(file);
  }
  const errors = hits.length > 0
    ? [
        `raw NUL byte(s) found in staged blobs:\n` +
        hits.map((file) => `  - ${file}`).join('\n'),
      ]
    : [];
  return {
    name: 'staged-nul-bytes',
    ok: errors.length === 0,
    files_checked: files.length,
    hits,
    errors,
  };
}

export function runGitDiffCheck({ files = [], git = realGit } = {}) {
  const result = git.diffCheck(files);
  const output = result.output.trim();
  const errors = result.status === 0
    ? []
    : [
        output
          ? `git diff --cached --check reported whitespace errors:\n${indent(output)}`
          : `git diff --cached --check exited ${result.status}`,
      ];
  return {
    name: 'git-diff-check',
    ok: errors.length === 0,
    status: result.status,
    output,
    errors,
  };
}

export function checkTaskScopeGuardReadiness({ task = null, git = realGit } = {}) {
  const errors = [];
  const warnings = [];
  const guardExists = fs.existsSync(TASK_SCOPE_GUARD);
  if (!guardExists) {
    errors.push(`task-scope guard missing: ${path.relative(REPO_ROOT, TASK_SCOPE_GUARD)}`);
  }

  const resolvedTask = task ? resolveTaskPath(task) : null;
  if (task && !fs.existsSync(resolvedTask)) {
    errors.push(`task contract file not found: ${task}`);
  }

  let guard = null;
  if (guardExists && resolvedTask && fs.existsSync(resolvedTask)) {
    guard = git.runTaskScopeGuard(resolvedTask);
    if (guard.status !== 0) {
      errors.push(
        guard.output.trim()
          ? `task-scope guard failed:\n${indent(guard.output.trim())}`
          : `task-scope guard exited ${guard.status}`,
      );
    }
  } else if (!task) {
    warnings.push('MISSIOND_TASK_CONTRACT is not set; task-scope guard readiness checked but staged scope was not enforced');
  }

  return {
    name: 'task-scope-guard',
    ok: errors.length === 0,
    task_contract: resolvedTask,
    guard_file: path.relative(REPO_ROOT, TASK_SCOPE_GUARD),
    guard_file_exists: guardExists,
    guard_status: guard?.status ?? null,
    errors,
    warnings,
  };
}

function resolveTaskPath(task) {
  if (path.isAbsolute(task)) return task;
  const cwdResolved = path.resolve(process.cwd(), task);
  if (fs.existsSync(cwdResolved)) return cwdResolved;
  return path.join(REPO_ROOT, task);
}

function indent(text) {
  return text
    .split('\n')
    .map((line) => `  ${line}`)
    .join('\n');
}

function failedCheck(name, error) {
  return { name, ok: false, errors: [error] };
}

const realGit = {
  readStagedFiles() {
    const raw = execFileSync(
      'git',
      ['diff', '--cached', '--name-only', '--diff-filter=ACMR', '--no-renames', '-z'],
      { encoding: 'buffer', stdio: ['ignore', 'pipe', 'pipe'] },
    );
    return raw
      .toString('utf8')
      .split('\0')
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
  },
  readStagedBlob(file) {
    return execFileSync('git', ['show', `:${file}`], {
      encoding: 'buffer',
      stdio: ['ignore', 'pipe', 'pipe'],
      maxBuffer: 50 * 1024 * 1024,
    });
  },
  diffCheck(files) {
    const args = ['diff', '--cached', '--check'];
    if (files.length > 0) args.push('--', ...files);
    const child = spawnSync('git', args, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    return {
      status: child.status ?? 1,
      output: `${child.stdout ?? ''}${child.stderr ?? ''}`,
    };
  },
  runTaskScopeGuard(task) {
    const child = spawnSync(
      'node',
      [TASK_SCOPE_GUARD, '--task', task, '--mode', 'staged', '--json'],
      { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
    );
    return {
      status: child.status ?? 1,
      output: `${child.stdout ?? ''}${child.stderr ?? ''}`,
    };
  },
};

function emitResult({ json, result }) {
  if (json) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }

  if (result.ok) {
    const count = result.mode === 'staged'
      ? `${result.staged_files.length} staged file(s)`
      : `${result.files.length} supplied file(s)`;
    console.log(`check-staged-source-hygiene OK: ${count}`);
    for (const warning of result.warnings) console.warn(`warn: ${warning}`);
    return;
  }

  console.error(`check-staged-source-hygiene FAILED (${result.mode})`);
  for (const error of result.errors) console.error(`  ${error}`);
  for (const warning of result.warnings) console.warn(`warn: ${warning}`);
}

function runFixtures({ json }) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-hygiene-'));
  const clean = path.join(tmp, 'clean.txt');
  const nul = path.join(tmp, 'nul.txt');
  const trailing = path.join(tmp, 'trailing.txt');
  const indent = path.join(tmp, 'indent.txt');
  fs.writeFileSync(clean, 'alpha\nbeta\n');
  fs.writeFileSync(nul, Buffer.from([97, 0, 98, 10]));
  fs.writeFileSync(trailing, 'alpha \n');
  fs.writeFileSync(indent, ' \talpha\n');

  const failures = [];

  const cleanResult = checkSuppliedFiles({ files: [clean], cwd: tmp });
  if (!cleanResult.ok) {
    failures.push({
      name: 'supplied clean file passes',
      expected: { ok: true },
      got: { ok: cleanResult.ok, errors: cleanResult.errors },
    });
  }

  const nulResult = checkSuppliedFiles({ files: [nul], cwd: tmp });
  if (nulResult.ok || !/raw NUL byte/.test(nulResult.errors.join('\n'))) {
    failures.push({
      name: 'supplied NUL file fails',
      expected: { ok: false, error: 'raw NUL byte' },
      got: { ok: nulResult.ok, errors: nulResult.errors },
    });
  }

  const whitespaceResult = checkSuppliedFiles({ files: [trailing, indent], cwd: tmp });
  if (
    whitespaceResult.ok ||
    !/trailing whitespace/.test(whitespaceResult.errors.join('\n')) ||
    !/space before tab/.test(whitespaceResult.errors.join('\n'))
  ) {
    failures.push({
      name: 'supplied whitespace files fail',
      expected: { ok: false, errors: ['trailing whitespace', 'space before tab'] },
      got: { ok: whitespaceResult.ok, errors: whitespaceResult.errors },
    });
  }

  const fakeGit = {
    readStagedFiles() {
      return ['scripts/check-staged-source-hygiene.mjs', 'fixtures/bad.bin'];
    },
    readStagedBlob(file) {
      return file.endsWith('bad.bin')
        ? Buffer.from([120, 0, 121])
        : Buffer.from('clean\n');
    },
    diffCheck() {
      return { status: 1, output: 'fixture.js:1: trailing whitespace.\n+bad \n' };
    },
    runTaskScopeGuard() {
      return { status: 0, output: '{"ok":true}\n' };
    },
  };
  const stagedResult = checkStagedSourceHygiene({ task: SCRIPT_PATH, git: fakeGit });
  if (
    stagedResult.ok ||
    !/raw NUL byte/.test(stagedResult.errors.join('\n')) ||
    !/git diff --cached --check/.test(stagedResult.errors.join('\n'))
  ) {
    failures.push({
      name: 'staged fake git catches NUL and diff-check failures',
      expected: { ok: false, errors: ['raw NUL byte', 'git diff --cached --check'] },
      got: { ok: stagedResult.ok, errors: stagedResult.errors },
    });
  }

  const fakeScopeFail = {
    readStagedFiles() {
      return ['scripts/check-staged-source-hygiene.mjs'];
    },
    readStagedBlob() {
      return Buffer.from('clean\n');
    },
    diffCheck() {
      return { status: 0, output: '' };
    },
    runTaskScopeGuard() {
      return { status: 1, output: 'outside :write-scope\n' };
    },
  };
  const scopeResult = checkStagedSourceHygiene({ task: SCRIPT_PATH, git: fakeScopeFail });
  if (scopeResult.ok || !/task-scope guard failed/.test(scopeResult.errors.join('\n'))) {
    failures.push({
      name: 'staged fake git propagates task-scope guard failure',
      expected: { ok: false, error: 'task-scope guard failed' },
      got: { ok: scopeResult.ok, errors: scopeResult.errors },
    });
  }

  fs.rmSync(tmp, { recursive: true, force: true });

  const ok = failures.length === 0;
  const summary = {
    mode: 'dry-fixture',
    ok,
    fixtures_run: 5,
    coverage: ['nul-bytes', 'whitespace', 'git-diff-check', 'task-scope-guard'],
    failures,
  };

  if (json) {
    console.log(JSON.stringify(summary, null, 2));
  } else if (ok) {
    console.log('check-staged-source-hygiene fixtures OK (5/5)');
  } else {
    console.error(`check-staged-source-hygiene fixtures FAILED - ${failures.length} failure(s)`);
    for (const failure of failures) console.error(JSON.stringify(failure, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
