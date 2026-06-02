#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-source-hygiene-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 source-hygiene Lisp/code isomorphism contract:
  - staged source hygiene is read-only and task-contract aware.
  - NUL-byte, whitespace, and task-scope checks run before scoped commits.
  - repo-local pre-commit hook installation is explicit, inspectable, and opt-in.
  - repo-local search ignore sidecars mirror ssot-retrieval-scope cold evidence.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  hygiene: 'scripts/check-staged-source-hygiene.mjs',
  taskScopeGuard: 'scripts/task-scope-guard.mjs',
  checkHooks: 'scripts/check-missiond-hooks.mjs',
  installHooks: 'scripts/install-missiond-hooks.mjs',
  preCommit: '.githooks/pre-commit',
  rootIgnore: '.ignore',
  missiondIgnore: '.missiond/.ignore',
  missiondV3Ignore: '.missiond/v3/.ignore',
  missiondResearchIgnore: '.missiond/research/.ignore',
  missiondTasksIgnore: '.missiond/tasks/.ignore',
  batchVerifier: 'scripts/verify-task-runner-batch.mjs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES, { runRepoSearchSmoke: !dryFixture });
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 source-hygiene Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 source-hygiene Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files, options = {}) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'source-hygiene',
    '(surface source-hygiene',
    ':status "code-aligned"',
    'scripts/check-staged-source-hygiene.mjs',
    'scripts/task-scope-guard.mjs',
    'scripts/check-missiond-hooks.mjs',
    'scripts/install-missiond-hooks.mjs',
    '.githooks/pre-commit',
    '.ignore',
    '.missiond/.ignore',
    '.missiond/v3/.ignore',
    '.missiond/research/.ignore',
    '.missiond/tasks/.ignore',
    'Staged hygiene MUST be read-only',
    'MISSIOND_TASK_CONTRACT',
    'raw NUL bytes in staged blobs',
    'git diff --cached --check',
    'task-scope guard',
    'Repo text search hygiene MUST project ssot-retrieval-scope into .ignore sidecars',
    'Default repo rg MUST NOT surface .missiond/research/true-user-utterances-*.md',
    'node scripts/check-v3-source-hygiene-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.hygiene, sources.hygiene, [
    'Read-only staged source hygiene preflight',
    'raw NUL bytes in staged blobs',
    'git diff --cached --check',
    'task-scope guard readiness when --task or MISSIOND_TASK_CONTRACT is set',
    'This command never mutates git config, the index, the working tree, or hooks',
    'export function checkStagedSourceHygiene',
    'export function checkSuppliedFiles',
    'export function checkStagedNulBytes',
    'export function runGitDiffCheck',
    'export function checkTaskScopeGuardReadiness',
    "['diff', '--cached', '--name-only', '--diff-filter=ACMR', '--no-renames', '-z']",
    "['show', `:${file}`]",
    "['diff', '--cached', '--check']",
    "[TASK_SCOPE_GUARD, '--task', task, '--mode', 'staged', '--json']",
    'check-staged-source-hygiene fixtures OK (5/5)',
  ]);

  requireAll(diagnostics, files.taskScopeGuard, sources.taskScopeGuard, [
    'Task-contract aware scope guard',
    'staged   reads',
    'git diff --cached --name-only',
    'outside :write-scope or matches :must-not-touch',
    'This guard is read-only',
    'verify-task-contract.mjs',
    "['diff', '--cached', '--name-only', '--no-renames', '-z']",
    'pathMatchesAny(f, contract.mustNotTouch)',
  ]);

  requireAll(diagnostics, files.checkHooks, sources.checkHooks, [
    'Read-only default-on doctor',
    'install-missiond-hooks.mjs --check',
    'This command NEVER mutates git config, files, or the working tree',
    'flag ${arg} is rejected by the doctor alias',
    "const forwarded = ['--check']",
  ]);

  requireAll(diagnostics, files.installHooks, sources.installHooks, [
    'STAGED_SOURCE_HYGIENE_FILE',
    'EXPECTED_HOOKS_PATH',
    'EXPECTED_HOOK_FILE',
    'node scripts/install-missiond-hooks.mjs --install',
    'staged-source-hygiene-missing',
    'repo ships .githooks/pre-commit',
    'repo ships scripts/check-staged-source-hygiene.mjs',
  ]);

  requireAll(diagnostics, files.preCommit, sources.preCommit, [
    'MissionD task-contract aware pre-commit hook',
    'MISSIOND_TASK_CONTRACT',
    'missiond-work-order.mjs',
    'scripts/check-staged-source-hygiene.mjs',
    'node "${hygiene}" --task "${contract}"',
    'verify --staged',
  ]);

  requireAll(diagnostics, files.rootIgnore, sources.rootIgnore, [
    'MissionD default search boundary',
    '.missiond/v3/runtime/**',
    '.missiond/tasks/wave*/**',
    '.missiond/research/memory-review/**',
    '.missiond/research/board-cleanup/**',
    '.missiond/research/true-user-utterances-*.md',
    '.missiond/research/archived-session*/**',
    '.missiond/research/archive*/**',
    '.missiond/research/*transcript*.md',
    '.missiond/research/*transcript*.jsonl',
    '.missiond/research/*conversation*.jsonl',
  ]);

  requireAll(diagnostics, files.missiondIgnore, sources.missiondIgnore, [
    'Projection of ssot-retrieval-scope',
    'v3/runtime/**',
    'tasks/wave*/**',
    'research/memory-review/**',
    'research/board-cleanup/**',
    'research/true-user-utterances-*.md',
    'research/archived-session*/**',
    'research/archive*/**',
    'research/*transcript*.md',
    'research/*transcript*.jsonl',
    'research/*conversation*.jsonl',
  ]);

  requireAll(diagnostics, files.missiondV3Ignore, sources.missiondV3Ignore, [
    'Cold runtime artifacts',
    'runtime/**',
  ]);

  requireAll(diagnostics, files.missiondResearchIgnore, sources.missiondResearchIgnore, [
    'Historical memory/Board cleanup evidence',
    'memory-review/**',
    'memory-review-v2/**',
    'board-cleanup/**',
    'true-user-utterances-*.md',
    'archived-session*/**',
    'archive*/**',
    '*transcript*.md',
    '*transcript*.jsonl',
    '*conversation*.jsonl',
  ]);

  requireAll(diagnostics, files.missiondTasksIgnore, sources.missiondTasksIgnore, [
    'Historical task-runner waves',
    'wave*/**',
  ]);

  requireAll(diagnostics, files.batchVerifier, sources.batchVerifier, [
    "import { checkSuppliedFiles } from './check-staged-source-hygiene.mjs'",
    'validates source hygiene',
    "checkSuppliedFiles({ files: ['scripts/foo.mjs'], cwd: tmp })",
  ]);

  if (options.runRepoSearchSmoke) {
    checkDefaultRgBoundary(diagnostics, root);
  }

  return diagnostics;
}

function checkDefaultRgBoundary(diagnostics, root) {
  const rg = spawnSync('rg', ['--files', '.missiond'], {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 10 * 1024 * 1024,
  });
  if (rg.error) {
    diagnostics.push({ file: 'rg --files .missiond', message: `cannot run ripgrep: ${rg.error.message}` });
    return;
  }
  if (rg.status !== 0) {
    diagnostics.push({
      file: 'rg --files .missiond',
      message: `ripgrep search-boundary smoke failed with status ${rg.status}: ${(rg.stderr || '').trim()}`,
    });
    return;
  }
  const leaked = rg.stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .filter((line) =>
      line.includes('.missiond/v3/runtime/')
        || line.includes('.missiond/research/true-user-utterances-')
        || /(^|\/)\.missiond\/research\/archived-session/i.test(line)
        || /(^|\/)\.missiond\/research\/archive/i.test(line)
        || /(^|\/)\.missiond\/research\/.*transcript.*\.(md|jsonl)$/i.test(line)
        || /(^|\/)\.missiond\/research\/.*conversation.*\.jsonl$/i.test(line),
    );
  if (leaked.length > 0) {
    diagnostics.push({
      file: 'rg --files .missiond',
      message: `default rg leaked cold/noisy evidence paths: ${leaked.slice(0, 10).join(', ')}`,
    });
  }
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-source-hygiene-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (source-hygiene
    :invariants
      ["Staged hygiene MUST be read-only."
       "MISSIOND_TASK_CONTRACT enables task-scope guard enforcement."
       "raw NUL bytes in staged blobs are rejected."
       "git diff --cached --check runs before scoped commits."
       "task-scope guard rejects paths outside write-scope."
       "Repo text search hygiene MUST project ssot-retrieval-scope into .ignore sidecars."
       "Default repo rg MUST NOT surface .missiond/research/true-user-utterances-*.md, archived session dumps, imported transcript exports, or .missiond/v3/runtime cold compiled projections."])
  (implementation-map
    (surface source-hygiene
      :status "code-aligned"
      :code ["scripts/check-staged-source-hygiene.mjs"
             "scripts/task-scope-guard.mjs"
             "scripts/check-missiond-hooks.mjs"
             "scripts/install-missiond-hooks.mjs"
             ".githooks/pre-commit"
             ".ignore"
             ".missiond/.ignore"
             ".missiond/v3/.ignore"
             ".missiond/research/.ignore"
             ".missiond/tasks/.ignore"]
      :note "fixture"))
  (compression-contract
    :checks ["node scripts/check-v3-source-hygiene-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.hygiene, `
Read-only staged source hygiene preflight
raw NUL bytes in staged blobs
git diff --cached --check
task-scope guard readiness when --task or MISSIOND_TASK_CONTRACT is set
This command never mutates git config, the index, the working tree, or hooks
export function checkStagedSourceHygiene
export function checkSuppliedFiles
export function checkStagedNulBytes
export function runGitDiffCheck
export function checkTaskScopeGuardReadiness
['diff', '--cached', '--name-only', '--diff-filter=ACMR', '--no-renames', '-z']
['show', \`:\${file}\`]
['diff', '--cached', '--check']
[TASK_SCOPE_GUARD, '--task', task, '--mode', 'staged', '--json']
check-staged-source-hygiene fixtures OK (5/5)
`);

  writeFixture(root, DEFAULT_FILES.taskScopeGuard, `
Task-contract aware scope guard
staged   reads
git diff --cached --name-only
outside :write-scope or matches :must-not-touch
This guard is read-only
verify-task-contract.mjs
['diff', '--cached', '--name-only', '--no-renames', '-z']
pathMatchesAny(f, contract.mustNotTouch)
`);

  writeFixture(root, DEFAULT_FILES.checkHooks, `
Read-only default-on doctor
install-missiond-hooks.mjs --check
This command NEVER mutates git config, files, or the working tree
flag \${arg} is rejected by the doctor alias
const forwarded = ['--check']
`);

  writeFixture(root, DEFAULT_FILES.installHooks, `
STAGED_SOURCE_HYGIENE_FILE
EXPECTED_HOOKS_PATH
EXPECTED_HOOK_FILE
node scripts/install-missiond-hooks.mjs --install
staged-source-hygiene-missing
repo ships .githooks/pre-commit
repo ships scripts/check-staged-source-hygiene.mjs
`);

  writeFixture(root, DEFAULT_FILES.preCommit, `
MissionD task-contract aware pre-commit hook
MISSIOND_TASK_CONTRACT
missiond-work-order.mjs
scripts/check-staged-source-hygiene.mjs
node "\${hygiene}" --task "\${contract}"
verify --staged
`);

  writeFixture(root, DEFAULT_FILES.rootIgnore, `
MissionD default search boundary
.missiond/v3/runtime/**
.missiond/tasks/wave*/**
.missiond/research/memory-review/**
.missiond/research/board-cleanup/**
.missiond/research/true-user-utterances-*.md
.missiond/research/archived-session*/**
.missiond/research/archive*/**
.missiond/research/*transcript*.md
.missiond/research/*transcript*.jsonl
.missiond/research/*conversation*.jsonl
`);

  writeFixture(root, DEFAULT_FILES.missiondIgnore, `
Projection of ssot-retrieval-scope
v3/runtime/**
tasks/wave*/**
research/memory-review/**
research/board-cleanup/**
research/true-user-utterances-*.md
research/archived-session*/**
research/archive*/**
research/*transcript*.md
research/*transcript*.jsonl
research/*conversation*.jsonl
`);

  writeFixture(root, DEFAULT_FILES.missiondV3Ignore, `
Cold runtime artifacts
runtime/**
`);

  writeFixture(root, DEFAULT_FILES.missiondResearchIgnore, `
Historical memory/Board cleanup evidence
memory-review/**
memory-review-v2/**
board-cleanup/**
true-user-utterances-*.md
archived-session*/**
archive*/**
*transcript*.md
*transcript*.jsonl
*conversation*.jsonl
`);

  writeFixture(root, DEFAULT_FILES.missiondTasksIgnore, `
Historical task-runner waves
wave*/**
`);

  writeFixture(root, DEFAULT_FILES.batchVerifier, `
import { checkSuppliedFiles } from './check-staged-source-hygiene.mjs'
validates source hygiene
checkSuppliedFiles({ files: ['scripts/foo.mjs'], cwd: tmp })
`);

  return root;
}

function writeFixture(root, rel, content) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content.trimStart());
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
