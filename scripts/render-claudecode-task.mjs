#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import {
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  nodeToStringArray,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/render-claudecode-task.mjs [--stdout] [--force] [--out <path>] <task.lisp>

Renders a MissionD task-contract v1 Lisp file into the current ClaudeCode
Markdown task-brief format. By default writes:
  .missiond/claudecode/<task-id>.md
`;

function main() {
  const args = process.argv.slice(2);
  let stdout = false;
  let force = false;
  let outPath = null;
  const inputs = [];

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--stdout') {
      stdout = true;
    } else if (arg === '--force') {
      force = true;
    } else if (arg === '--out') {
      outPath = args[++i];
      if (!outPath) fail('--out requires a path');
    } else {
      inputs.push(arg);
    }
  }

  if (inputs.length !== 1) fail(usage);

  const sourcePath = path.resolve(process.cwd(), inputs[0]);
  const task = loadSingleTask(sourcePath);
  const markdown = renderTask(task, sourcePath);

  if (stdout) {
    process.stdout.write(markdown);
    return;
  }

  const outputPath = path.resolve(
    process.cwd(),
    outPath ?? path.join('.missiond', 'claudecode', `${task.id}.md`),
  );
  if (fs.existsSync(outputPath) && !force) {
    fail(`${outputPath} already exists; pass --force to overwrite`);
  }
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, markdown);
  console.log(`rendered ${path.relative(process.cwd(), outputPath)} from ${path.relative(process.cwd(), sourcePath)}`);
}

function loadSingleTask(file) {
  const forms = readLispFile(file);
  const tasks = forms.filter((form) => isList(form) && head(form) === 'task');
  if (tasks.length !== 1) fail(`${file} must contain exactly one (task ...) form; got ${tasks.length}`);
  const node = tasks[0];
  const id = node.children[1]?.value;
  const props = readKeywordProps(node, { start: 2 });
  const commitNode = props[':commit']?.value;
  const commitProps = isList(commitNode) ? readKeywordProps(commitNode, { start: 0 }) : {};
  return {
    id,
    title: keywordPropText(props, ':title') ?? id,
    kind: keywordPropText(props, ':kind') ?? 'task',
    status: keywordPropText(props, ':status') ?? 'ready',
    owner: keywordPropText(props, ':owner') ?? 'claudecode',
    goal: keywordPropText(props, ':goal') ?? '',
    dependsOn: nodeToStringArray(props[':depends-on']?.value),
    dispatchStrategy: keywordPropText(props, ':dispatch-strategy'),
    writeScope: nodeToStringArray(props[':write-scope']?.value),
    mustNotTouch: nodeToStringArray(props[':must-not-touch']?.value),
    requirements: nodeToStringArray(props[':requirements']?.value),
    acceptance: nodeToStringArray(props[':acceptance']?.value),
    report: nodeToStringArray(props[':report']?.value),
    commit: {
      required: keywordPropBool(commitProps, ':required'),
      message: keywordPropText(commitProps, ':message'),
      scopeCheck: keywordPropText(commitProps, ':scope-check'),
    },
    sourcePath: file,
  };
}

// Derive the wave-id prefix (e.g. "wave19") from a task id like
// "wave19-05-renderer-dispatch-brief-v1". Returns null when the id does not
// follow the wave prefix convention so callers can skip wave-scoped rendering.
function deriveWaveId(taskId) {
  if (typeof taskId !== 'string') return null;
  const match = taskId.match(/^(wave\d+)/);
  return match ? match[1] : null;
}

// Resolve the shared-memory ledger path for a task by its wave id, returning
// null when the ledger file is not present on disk. The path itself is fixed
// by the shared-memory-v1 contract (.missiond/tasks/<wave>/shared-memory.lisp).
function resolveSharedMemoryPath(taskId) {
  const wave = deriveWaveId(taskId);
  if (!wave) return null;
  const rel = path.join('.missiond', 'tasks', wave, 'shared-memory.lisp');
  const abs = path.resolve(process.cwd(), rel);
  return fs.existsSync(abs) ? rel : null;
}

// Resolve the expected report-contract output path for a task. The report file
// itself usually does not exist yet at dispatch time, so this function returns
// the convention-derived path unconditionally when a wave id can be derived.
function resolveReportContractPath(taskId) {
  const wave = deriveWaveId(taskId);
  if (!wave) return null;
  return path.join('.missiond', 'tasks', wave, 'reports', `${taskId}.report.lisp`);
}

function renderTask(task, sourcePath) {
  const relSource = path.relative(process.cwd(), sourcePath);
  const sharedMemoryPath = resolveSharedMemoryPath(task.id);
  const reportContractPath = resolveReportContractPath(task.id);
  const lines = [];
  lines.push(`# ${task.id} — ${task.title}`);
  lines.push('');
  lines.push('> Generated from MissionD task-contract v1.');
  lines.push(`> Source: \`${relSource}\``);
  lines.push('');
  lines.push('## Machine Contract');
  lines.push('');
  lines.push(`- kind: \`${task.kind}\``);
  lines.push(`- status: \`${task.status}\``);
  lines.push(`- owner: \`${task.owner}\``);
  if (task.dispatchStrategy) lines.push(`- dispatch_strategy: \`${task.dispatchStrategy}\``);
  if (task.dependsOn.length > 0) lines.push(`- depends_on: ${task.dependsOn.map(code).join(', ')}`);
  if (sharedMemoryPath) lines.push(`- shared_memory: \`${sharedMemoryPath}\``);
  if (reportContractPath) lines.push(`- report_contract: \`${reportContractPath}\``);
  lines.push('');
  if (task.dispatchStrategy === 'agent-team') {
    lines.push('## Dispatch Note');
    lines.push('');
    lines.push('使用 agent-team提高效率');
    lines.push('');
  }
  lines.push('## Goal');
  lines.push('');
  lines.push(task.goal || '(no goal supplied)');
  lines.push('');
  renderList(lines, 'Ownership', task.writeScope, 'Expected files');
  renderList(lines, 'Must Not Touch', task.mustNotTouch, 'Forbidden files');
  renderNumbered(lines, 'Requirements', task.requirements);
  renderCommands(lines, 'Acceptance Commands', task.acceptance);
  if (sharedMemoryPath) renderSharedMemory(lines, task, sharedMemoryPath);
  if (reportContractPath) renderReportContract(lines, task, reportContractPath);
  lines.push('## Commit');
  lines.push('');
  if (task.commit.required) {
    lines.push('After acceptance, commit only files inside the declared write scope.');
    lines.push('');
    lines.push('Stage just the declared scope, run the pre-commit scoped-index guard, then commit:');
    lines.push('');
    lines.push('```bash');
    lines.push(renderGitAdd(task.writeScope));
    lines.push(`node scripts/task-scope-guard.mjs --task ${relSource} --mode staged`);
    lines.push(`MISSIOND_TASK_CONTRACT=${relSource} \\`);
    lines.push(`  git commit -m ${JSON.stringify(task.commit.message ?? '')}`);
    lines.push('```');
    lines.push('');
    lines.push(`Scope check: \`${task.commit.scopeCheck ?? 'write-scope-only'}\`.`);
    lines.push('');
    lines.push(
      'The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).',
    );
    lines.push('');
    renderVerifyTaskContract(lines, task, relSource);
  } else {
    lines.push('No commit required by contract.');
    lines.push('');
  }
  renderList(lines, 'Report', task.report.length ? task.report : [
    'Commit hash or no-commit reason.',
    'Files changed.',
    'Acceptance command results.',
  ], 'Return');
  return `${lines.join('\n')}\n`;
}

function renderSharedMemory(lines, task, sharedMemoryPath) {
  lines.push('## Shared Memory');
  lines.push('');
  lines.push(`Coordination ledger: \`${sharedMemoryPath}\` (schema \`missiond.shared-memory.v1\`).`);
  lines.push('');
  lines.push('- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.');
  lines.push('- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.');
  lines.push('- `:touched` paths in your entries must stay inside this task `:write-scope`.');
  lines.push('');
  lines.push('Validate with:');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/check-task-memory.mjs ${sharedMemoryPath}`);
  lines.push('```');
  lines.push('');
}

function renderReportContract(lines, task, reportContractPath) {
  lines.push('## Report Contract');
  lines.push('');
  lines.push(`Expected machine-readable report: \`${reportContractPath}\` (schema \`missiond.report-contract.v1\`).`);
  lines.push('');
  lines.push('- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.');
  lines.push('- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.');
  lines.push('- Free-form prose belongs in `:notes`; structural fields drive automated verification.');
  lines.push('');
  lines.push('Validate with:');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/check-task-report.mjs ${reportContractPath}`);
  lines.push('```');
  lines.push('');
}

function renderVerifyTaskContract(lines, task, relSource) {
  lines.push('Verify the commit against this contract (read-only, post-commit):');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/verify-task-contract.mjs ${relSource}`);
  lines.push('```');
  lines.push('');
}

function renderList(lines, title, items, label) {
  lines.push(`## ${title}`);
  lines.push('');
  if (!items.length) {
    lines.push(`- (${label.toLowerCase()} empty)`);
  } else {
    for (const item of items) lines.push(`- \`${item}\``);
  }
  lines.push('');
}

function renderNumbered(lines, title, items) {
  lines.push(`## ${title}`);
  lines.push('');
  if (!items.length) {
    lines.push('No additional requirements.');
  } else {
    for (let i = 0; i < items.length; i++) lines.push(`${i + 1}. ${items[i]}`);
  }
  lines.push('');
}

function renderCommands(lines, title, commands) {
  lines.push(`## ${title}`);
  lines.push('');
  lines.push('```bash');
  for (const command of commands) lines.push(command);
  lines.push('```');
  lines.push('');
}

function renderGitAdd(paths) {
  if (paths.length === 0) return '# no write scope declared';
  return `git add ${paths.map((p) => JSON.stringify(p)).join(' \\\n        ')}`;
}

function code(value) {
  return `\`${value}\``;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

main();
