#!/usr/bin/env node

// MissionD parent hotfix planner v0.
//
// Default mode is read-only: it turns a worker draft report and parent hotfix
// facts into an explicit lifecycle event plus finalized report projection.
// --write-report is the only file mutation boundary.

import fs from 'node:fs';
import path from 'node:path';

import {
  buildParentPatch,
  finalizeReportFile,
  finalizeReportSource,
} from './task-runner-finalize-report.mjs';

const usage = `Usage:
  node scripts/task-runner-parent-hotfix.mjs --report <worker.report.lisp> \\
    --task <task-id> --agent-commit <sha> --parent-commit <sha> \\
    --kind <lint-cleanup|doc-fix|test-fix|scope-trim|hotfix-other> \\
    --reason <text> --file <repo-path>... [--write-report <path>] [--json]
  node scripts/task-runner-parent-hotfix.mjs --dry-fixture [--json]

Plans the parent/orchestrator side of a post-worker hotfix. It records the
hotfix as lineage facts and emits a finalized report; it never amends the
worker commit. Default mode is read-only. --write-report explicitly writes
the finalized report bytes.

No git mutation, no git inspection, no spawn, no network, no LLM.
`;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    report: null,
    task: null,
    agentCommit: null,
    parentCommit: null,
    kind: null,
    reason: null,
    files: [],
    acceptanceCommands: [],
    writeReport: null,
    json: false,
    dryFixture: false,
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
    } else if (arg === '--report') {
      opts.report = argv[++i] ?? fail('--report requires a value');
    } else if (arg.startsWith('--report=')) {
      opts.report = arg.slice('--report='.length);
    } else if (arg === '--task') {
      opts.task = argv[++i] ?? fail('--task requires a value');
    } else if (arg.startsWith('--task=')) {
      opts.task = arg.slice('--task='.length);
    } else if (arg === '--agent-commit') {
      opts.agentCommit = argv[++i] ?? fail('--agent-commit requires a value');
    } else if (arg.startsWith('--agent-commit=')) {
      opts.agentCommit = arg.slice('--agent-commit='.length);
    } else if (arg === '--parent-commit') {
      opts.parentCommit = argv[++i] ?? fail('--parent-commit requires a value');
    } else if (arg.startsWith('--parent-commit=')) {
      opts.parentCommit = arg.slice('--parent-commit='.length);
    } else if (arg === '--kind') {
      opts.kind = argv[++i] ?? fail('--kind requires a value');
    } else if (arg.startsWith('--kind=')) {
      opts.kind = arg.slice('--kind='.length);
    } else if (arg === '--reason') {
      opts.reason = argv[++i] ?? fail('--reason requires a value');
    } else if (arg.startsWith('--reason=')) {
      opts.reason = arg.slice('--reason='.length);
    } else if (arg === '--file') {
      opts.files.push(argv[++i] ?? fail('--file requires a value'));
    } else if (arg.startsWith('--file=')) {
      opts.files.push(arg.slice('--file='.length));
    } else if (arg === '--acceptance-command') {
      opts.acceptanceCommands.push(argv[++i] ?? fail('--acceptance-command requires a value'));
    } else if (arg.startsWith('--acceptance-command=')) {
      opts.acceptanceCommands.push(arg.slice('--acceptance-command='.length));
    } else if (arg === '--write-report') {
      opts.writeReport = argv[++i] ?? fail('--write-report requires a value');
    } else if (arg.startsWith('--write-report=')) {
      opts.writeReport = arg.slice('--write-report='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

export function buildParentHotfixEvent({
  taskId,
  agentCommit,
  parentCommit,
  kind,
  reason,
  files,
  id = null,
  seq = null,
}) {
  const patch = buildParentPatch({ commit: parentCommit, kind, reason, files });
  return {
    id: id ?? `${taskId}-parent-hotfix-${patch.commit.slice(0, 7)}`,
    seq,
    task: taskId,
    kind: 'parent_hotfix',
    agent_commit_hash: agentCommit,
    parent_commit_hash: patch.commit,
    patch_kind: patch.kind,
    reason: patch.reason,
    files: patch.files,
  };
}

export function planParentHotfixFromSource(source, opts) {
  const event = buildParentHotfixEvent({
    taskId: opts.taskId,
    agentCommit: opts.agentCommit,
    parentCommit: opts.parentCommit,
    kind: opts.kind,
    reason: opts.reason,
    files: opts.files,
  });
  const finalized = finalizeReportSource(source, {
    agentCommit: opts.agentCommit,
    finalCommit: opts.parentCommit,
    verifiedCommit: opts.verifiedCommit ?? opts.parentCommit,
    parentPatches: [
      {
        commit: opts.parentCommit,
        kind: opts.kind,
        reason: opts.reason,
        files: opts.files,
      },
    ],
    acceptanceCommands: opts.acceptanceCommands,
  });
  return {
    ok: true,
    mutation_mode: opts.writeReport ? 'write-report' : 'read-only',
    lifecycle_event: event,
    finalized_report: finalized.report,
    finalized_report_source: finalized.source,
  };
}

export function planParentHotfixFromFile(reportFile, opts) {
  const workerSource = fs.readFileSync(reportFile, 'utf8');
  return planParentHotfixFromSource(workerSource, opts);
}

function runCli() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    const result = runFixtures();
    if (opts.json) console.log(JSON.stringify(result, null, 2));
    else console.log(`task-runner-parent-hotfix fixtures OK (${result.cases} cases)`);
    return;
  }
  if (!opts.report) fail('--report is required');
  if (!opts.task) fail('--task is required');
  const plan = planParentHotfixFromFile(opts.report, {
    taskId: opts.task,
    agentCommit: opts.agentCommit,
    parentCommit: opts.parentCommit,
    kind: opts.kind,
    reason: opts.reason,
    files: opts.files,
    acceptanceCommands: opts.acceptanceCommands,
    writeReport: opts.writeReport,
  });
  if (opts.writeReport) {
    const abs = path.resolve(opts.writeReport);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, plan.finalized_report_source);
  }
  if (opts.json) {
    console.log(JSON.stringify({ ...plan, wrote: opts.writeReport ?? null }, null, 2));
  } else {
    process.stdout.write(plan.finalized_report_source);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function runFixtures() {
  const source = `(report wave29-03-runner-prep
    :schema "missiond.report-contract.v1"
    :task_id "wave29-03-runner-prep"
    :status done
    :commit_hash "d36de80"
    :files_changed ["scripts/prepare-task-runner-wave.mjs"]
    :acceptance_results
      [(:command "node scripts/prepare-task-runner-wave.mjs --dry-fixture" :exit_code 0 :ok true)])`;
  const plan = planParentHotfixFromSource(source, {
    taskId: 'wave29-03-runner-prep',
    agentCommit: 'd36de80',
    parentCommit: 'd842b1d',
    kind: 'lint-cleanup',
    reason: 'TS80007 sync await cleanup after worker commit',
    files: ['scripts/prepare-task-runner-wave.mjs'],
    acceptanceCommands: ['node scripts/prepare-task-runner-wave.mjs --dry-fixture'],
  });
  assert(plan.mutation_mode === 'read-only', 'default mode must be read-only');
  assert(plan.lifecycle_event.kind === 'parent_hotfix', 'event kind should record parent_hotfix');
  assert(plan.finalized_report.commitHash === 'd842b1d', 'final report commit should be parent commit');
  assert(plan.finalized_report.agentCommitHash === 'd36de80', 'worker commit should be retained');

  const tmp = fs.mkdtempSync(path.join(process.cwd(), '.tmp-parent-hotfix-'));
  try {
    const reportPath = path.join(tmp, 'worker.report.lisp');
    const finalPath = path.join(tmp, 'final.report.lisp');
    fs.writeFileSync(reportPath, source);
    const filePlan = planParentHotfixFromFile(reportPath, {
      taskId: 'wave29-03-runner-prep',
      agentCommit: 'd36de80',
      parentCommit: 'd842b1d',
      kind: 'lint-cleanup',
      reason: 'TS80007 sync await cleanup after worker commit',
      files: ['scripts/prepare-task-runner-wave.mjs'],
      writeReport: finalPath,
    });
    fs.writeFileSync(finalPath, filePlan.finalized_report_source);
    const reread = finalizeReportFile(finalPath, {
      agentCommit: 'd36de80',
      finalCommit: 'd842b1d',
      parentPatches: [
        {
          commit: 'd842b1d',
          kind: 'lint-cleanup',
          reason: 'idempotent second projection',
          files: ['scripts/prepare-task-runner-wave.mjs'],
        },
      ],
    });
    assert(reread.report.commitHash === 'd842b1d', 'written report should stay parseable');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }

  return { ok: true, cases: 2 };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    runCli();
  } catch (err) {
    process.stderr.write(`task-runner-parent-hotfix: ${err?.message ?? String(err)}\n`);
    process.exit(1);
  }
}
