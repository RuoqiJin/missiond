#!/usr/bin/env node

// MissionD task-runner wave-state projector v0.
//
// Read-only orchestration state over Lisp artifacts. This is the small
// machine interface between "we have contracts/manifests/reports/events" and
// "MissionD can decide which task to dispatch/finalize/verify next".

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  readManifestFile,
  validateManifestObject,
} from './check-task-runner-manifest.mjs';
import {
  hardDepsForNode,
  planFromManifestObject,
} from './plan-task-runner.mjs';
import { loadReport } from './verify-task-run.mjs';
import { readLifecycleEventLogs } from './check-task-lifecycle-events.mjs';
import {
  isReceiptReusable,
  readVerificationReceiptFile,
  validateReceiptObject,
} from './check-verification-receipt.mjs';

const usage = `Usage:
  node scripts/task-runner-wave-state.mjs --manifest <manifest.lisp>
    [--lifecycle <task-lifecycle-events.lisp>] [--receipts <receipts.lisp>]
    [--repo <repo-root>] [--json]
  node scripts/task-runner-wave-state.mjs --dry-fixture [--json]

Projects manifest + finalized reports + optional lifecycle events + optional
verification receipts into a deterministic orchestration state:
  complete | dispatchable | blocked | running | needs_finalization

Read-only by construction: no git, no spawn, no network, no LLM, no writes.

When --lifecycle is omitted, the projector reads the conventional
.missiond/tasks/<wave>/task-lifecycle-events.lisp path if it exists.
`;

export const WAVE_STATE_SCHEMA = 'missiond.task-runner-wave-state.v0';

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    manifest: null,
    lifecycle: null,
    receipts: null,
    repo: process.cwd(),
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
    } else if (arg === '--manifest') {
      opts.manifest = argv[++i] ?? fail('--manifest requires a value');
    } else if (arg.startsWith('--manifest=')) {
      opts.manifest = arg.slice('--manifest='.length);
    } else if (arg === '--lifecycle') {
      opts.lifecycle = argv[++i] ?? fail('--lifecycle requires a value');
    } else if (arg.startsWith('--lifecycle=')) {
      opts.lifecycle = arg.slice('--lifecycle='.length);
    } else if (arg === '--receipts') {
      opts.receipts = argv[++i] ?? fail('--receipts requires a value');
    } else if (arg.startsWith('--receipts=')) {
      opts.receipts = arg.slice('--receipts='.length);
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

export function projectWaveStateFromFiles({
  manifestPath,
  repoRoot = process.cwd(),
  lifecyclePath = null,
  receiptsPath = null,
}) {
  const repo = path.resolve(repoRoot);
  const manifestAbs = path.resolve(repo, manifestPath);
  const manifests = readManifestFile(manifestAbs);
  if (manifests.length !== 1) {
    throw new Error(`expected exactly one manifest in ${manifestPath}, found ${manifests.length}`);
  }
  const manifest = manifests[0];
  const errors = validateManifestObject(manifest);
  if (errors.length > 0) {
    throw new Error(`manifest failed validation: ${errors.join('; ')}`);
  }

  const effectiveLifecyclePath = lifecyclePath ?? defaultLifecyclePath(manifest.wave);
  const lifecycleAbs = path.resolve(repo, effectiveLifecyclePath);
  const lifecycleUsedPath = fs.existsSync(lifecycleAbs) ? toRepoRelative(lifecycleAbs, repo) : null;
  const receiptsAbs = receiptsPath ? path.resolve(repo, receiptsPath) : null;
  const receiptsUsedPath = receiptsAbs && fs.existsSync(receiptsAbs) ? toRepoRelative(receiptsAbs, repo) : null;
  const events = readOptionalLifecycleEvents(effectiveLifecyclePath, repo);
  const receipts = readOptionalReceipts(receiptsPath, repo);

  return projectWaveState({
    manifest,
    manifestPath: toRepoRelative(manifestAbs, repo),
    lifecyclePath: lifecycleUsedPath,
    receiptsPath: receiptsUsedPath,
    repoRoot: repo,
    events,
    receipts,
  });
}

export function projectWaveState({
  manifest,
  manifestPath = '<memory>',
  lifecyclePath = null,
  receiptsPath = null,
  repoRoot = process.cwd(),
  events = [],
  receipts = [],
  reportLoader = null,
}) {
  const planResult = planFromManifestObject(manifest, {
    manifest_path: manifestPath,
    repoRoot,
    checkTaskContractsOnDisk: false,
    schedule: 'ready-queue',
  });
  if (!planResult.ok) {
    throw new Error(`ready-queue projection failed: ${planResult.message ?? planResult.error}`);
  }

  const eventsByTask = groupBy(events, (event) => event.task);
  const receiptsByTask = groupBy(receipts, (receipt) => receipt.task_id);
  const reportByTask = new Map();

  const tasks = [];
  const completeIds = new Set();

  for (const node of sortedNodes(manifest.nodes)) {
    const reportPath = reportPathFor(manifest.wave, node.task_id);
    const report = loadReportForTask({
      taskId: node.task_id,
      reportPath,
      repoRoot,
      reportLoader,
    });
    if (report) reportByTask.set(node.task_id, report);

    const taskEvents = eventsByTask.get(node.task_id) ?? [];
    const latestParentHotfix = latestEvent(taskEvents, 'parent_hotfix');
    const hasWorkerCommit = taskEvents.some((e) => e.event_kind === 'worker_commit');
    const hasClaimOrRun = taskEvents.some((e) =>
      ['claim', 'trace_start', 'read'].includes(e.event_kind),
    );
    const finalHash = finalReportHash(report);
    const lineageHashes = collectLineageHashes(report);
    const latestParentHash = latestParentHotfix?.commit_hash ?? null;
    const parentHotfixUnfinalized =
      latestParentHash != null &&
      !lineageHashes.some((hash) => shaPrefixAgrees(hash, latestParentHash));
    const isDoneReport = report?.status === 'done' && finalHash != null && finalHash !== '';
    const state = parentHotfixUnfinalized || (hasWorkerCommit && !isDoneReport)
      ? 'needs_finalization'
      : isDoneReport
        ? 'complete'
        : hasClaimOrRun
          ? 'running'
          : 'planned';

    if (state === 'complete') completeIds.add(node.task_id);

    const taskReceipts = receiptsByTask.get(node.task_id) ?? [];
    const reusableReceiptCount = taskReceipts.filter((receipt) =>
      isReceiptReusable(receipt, {
        commit_hash: finalHash ?? receipt.commit_hash,
        command: receipt.command,
        tier: node.verification_tier,
      }),
    ).length;

    tasks.push({
      task_id: node.task_id,
      state,
      hard_deps: [...hardDepsForNode(node)].sort((a, b) => a.localeCompare(b)),
      soft_refs: [...(node.soft_refs ?? [])].sort((a, b) => a.localeCompare(b)),
      pending_hard_deps: [],
      dispatch_group: node.dispatch_group,
      verification_tier: node.verification_tier,
      estimated_minutes: node.estimated_minutes,
      report_path: reportPath,
      final_commit_hash: finalHash,
      agent_commit_hash: report?.agentCommitHash ?? null,
      latest_parent_hotfix_hash: latestParentHash,
      lifecycle_event_count: taskEvents.length,
      receipt_count: taskReceipts.length,
      reusable_receipt_count: reusableReceiptCount,
    });
  }

  // Recompute dispatchable/blocked after all complete ids are known.
  const byTaskId = new Map(tasks.map((task) => [task.task_id, task]));
  for (const task of tasks) {
    const pending = task.hard_deps.filter((dep) => !completeIds.has(dep));
    task.pending_hard_deps = pending;
    if (task.state === 'planned') {
      task.state = pending.length === 0 ? 'dispatchable' : 'blocked';
    }
  }

  const readyOrder = planResult.plan.ready_queue?.order ?? tasks.map((t) => t.task_id);
  const readyRank = new Map(readyOrder.map((id, index) => [id, index]));
  tasks.sort((a, b) => {
    const ar = readyRank.get(a.task_id) ?? Number.MAX_SAFE_INTEGER;
    const br = readyRank.get(b.task_id) ?? Number.MAX_SAFE_INTEGER;
    if (ar !== br) return ar - br;
    return a.task_id.localeCompare(b.task_id);
  });

  const counts = countByState(tasks);
  return {
    ok: true,
    schema: WAVE_STATE_SCHEMA,
    manifest_path: manifestPath,
    lifecycle_path: lifecyclePath,
    receipts_path: receiptsPath,
    wave: manifest.wave,
    counts,
    next_actions: buildNextActions(tasks, manifest.wave),
    dispatchable: idsWithState(tasks, 'dispatchable'),
    needs_finalization: idsWithState(tasks, 'needs_finalization'),
    running: idsWithState(tasks, 'running'),
    blocked: idsWithState(tasks, 'blocked'),
    complete: idsWithState(tasks, 'complete'),
    ready_queue_order: readyOrder,
    tasks,
  };
}

function buildNextActions(tasks, wave) {
  const actions = [];
  for (const task of tasks) {
    if (task.state === 'needs_finalization') {
      actions.push({
        action: 'finalize_report',
        task_id: task.task_id,
        reason: task.latest_parent_hotfix_hash
          ? 'parent_hotfix_not_reflected_in_final_report'
          : 'worker_commit_without_finalized_report',
        report_path: task.report_path,
        latest_parent_hotfix_hash: task.latest_parent_hotfix_hash,
      });
    }
  }
  for (const task of tasks) {
    if (task.state === 'dispatchable') {
      actions.push({
        action: 'dispatch_task',
        task_id: task.task_id,
        reason: 'all_hard_deps_complete',
        brief_path: path.posix.join('.missiond', 'claudecode', `${task.task_id}.md`),
        hard_deps: task.hard_deps,
        soft_refs: task.soft_refs,
        wave,
      });
    }
  }
  for (const task of tasks) {
    if (task.state === 'blocked') {
      actions.push({
        action: 'wait_for_hard_deps',
        task_id: task.task_id,
        pending_hard_deps: task.pending_hard_deps,
      });
    }
  }
  return actions;
}

function readOptionalLifecycleEvents(lifecyclePath, repoRoot) {
  if (!lifecyclePath) return [];
  const abs = path.resolve(repoRoot, lifecyclePath);
  if (!fs.existsSync(abs)) return [];
  return readLifecycleEventLogs(abs).flatMap((log) => log.events);
}

function readOptionalReceipts(receiptsPath, repoRoot) {
  if (!receiptsPath) return [];
  const abs = path.resolve(repoRoot, receiptsPath);
  if (!fs.existsSync(abs)) return [];
  const receipts = readVerificationReceiptFile(abs);
  const errors = [];
  for (const receipt of receipts) {
    const errs = validateReceiptObject(receipt);
    if (errs.length > 0) {
      errors.push(`${receipt.id ?? '<unknown>'}: ${errs.join('; ')}`);
    }
  }
  if (errors.length > 0) {
    throw new Error(`invalid receipts: ${errors.join(' | ')}`);
  }
  return receipts;
}

function loadReportForTask({ taskId, reportPath, repoRoot, reportLoader }) {
  if (reportLoader) return reportLoader(taskId, reportPath) ?? null;
  const abs = path.resolve(repoRoot, reportPath);
  if (!fs.existsSync(abs)) return null;
  try {
    return loadReport(abs);
  } catch {
    return null;
  }
}

function reportPathFor(wave, taskId) {
  return path.posix.join('.missiond', 'tasks', wave, 'reports', `${taskId}.report.lisp`);
}

function defaultLifecyclePath(wave) {
  return path.posix.join('.missiond', 'tasks', wave, 'task-lifecycle-events.lisp');
}

function finalReportHash(report) {
  return report?.verifiedCommitHash ?? report?.finalCommitHash ?? report?.commitHash ?? null;
}

function collectLineageHashes(report) {
  if (!report) return [];
  return [
    report.commitHash,
    report.agentCommitHash,
    report.finalCommitHash,
    report.verifiedCommitHash,
    ...(report.parentPatches ?? []).map((patch) => patch.commit),
  ].filter((hash) => typeof hash === 'string' && hash.trim() !== '');
}

function latestEvent(events, kind) {
  return events
    .filter((event) => event.event_kind === kind)
    .sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0))[0] ?? null;
}

function shaPrefixAgrees(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  const ax = a.trim().toLowerCase();
  const bx = b.trim().toLowerCase();
  if (!/^[0-9a-f]+$/.test(ax) || !/^[0-9a-f]+$/.test(bx)) return false;
  if (ax === bx) return true;
  const longer = ax.length >= bx.length ? ax : bx;
  const shorter = ax.length >= bx.length ? bx : ax;
  return shorter.length >= 7 && longer.startsWith(shorter);
}

function sortedNodes(nodes) {
  return [...(nodes ?? [])].sort((a, b) => a.task_id.localeCompare(b.task_id));
}

function groupBy(items, keyFn) {
  const out = new Map();
  for (const item of items ?? []) {
    const key = keyFn(item);
    if (!key) continue;
    const arr = out.get(key) ?? [];
    arr.push(item);
    out.set(key, arr);
  }
  return out;
}

function countByState(tasks) {
  const out = {
    total: tasks.length,
    complete: 0,
    dispatchable: 0,
    blocked: 0,
    running: 0,
    needs_finalization: 0,
  };
  for (const task of tasks) {
    if (Object.prototype.hasOwnProperty.call(out, task.state)) out[task.state] += 1;
  }
  return out;
}

function idsWithState(tasks, state) {
  return tasks.filter((task) => task.state === state).map((task) => task.task_id);
}

function toRepoRelative(abs, repoRoot) {
  return path.relative(repoRoot, abs).replace(/\\/g, '/');
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  try {
    if (opts.dryFixture) {
      const result = runFixtures();
      if (opts.json) console.log(JSON.stringify(result, null, 2));
      else console.log(`task-runner-wave-state fixtures OK (${result.cases} cases)`);
      return;
    }
    if (!opts.manifest) fail('--manifest is required');
    const state = projectWaveStateFromFiles({
      manifestPath: opts.manifest,
      repoRoot: opts.repo,
      lifecyclePath: opts.lifecycle,
      receiptsPath: opts.receipts,
    });
    if (opts.json) {
      console.log(JSON.stringify(state, null, 2));
    } else {
      console.log(
        `task-runner-wave-state OK (${state.wave}): ` +
          `${state.counts.complete} complete, ` +
          `${state.counts.dispatchable} dispatchable, ` +
          `${state.counts.needs_finalization} need finalization, ` +
          `${state.counts.blocked} blocked`,
      );
    }
  } catch (err) {
    process.stderr.write(`task-runner-wave-state: ${err?.message ?? String(err)}\n`);
    process.exit(1);
  }
}

function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-wave-state-'));
  try {
    const manifest = {
      id: 'm-wave99-state',
      schema: 'missiond.task-runner-manifest.v2',
      wave: 'wave99',
      brief_mode: 'thin',
      shared_preamble_path: '.missiond/claudecode/wave99-shared-preamble.md',
      productive_only: true,
      overlap_policy: 'reject',
      description: null,
      generated_at: null,
      generator: null,
      nodes: [
        node('wave99-01-atlas', { minutes: 10 }),
        node('wave99-02-soft-slow', { minutes: 90 }),
        node('wave99-03-follower', {
          depends: ['wave99-01-atlas'],
          hard: ['wave99-01-atlas'],
          soft: ['wave99-02-soft-slow'],
          group: 'B',
          minutes: 5,
        }),
        node('wave99-04-patched', { group: 'C', minutes: 5 }),
      ],
      loc: null,
    };
    const reports = new Map([
      ['wave99-01-atlas', fakeReport('wave99-01-atlas', 'abc1001')],
      ['wave99-04-patched', fakeReport('wave99-04-patched', 'abc2001', {
        agentCommitHash: 'abc2001',
      })],
    ]);
    const events = [
      event('wave99-04-patched', 'worker_commit', 1, 'abc2001'),
      event('wave99-04-patched', 'parent_hotfix', 2, 'abc2002'),
    ];
    const receipts = [
      {
        id: 'wave99-01-atlas-abc1001-local',
        wave: 'wave99',
        task_id: 'wave99-01-atlas',
        commit_hash: 'abc1001',
        command: 'node scripts/atlas.mjs --dry-fixture',
        exit_code: 0,
        tier: 'local',
        duration_ms: 10,
        files: ['scripts/atlas.mjs'],
      },
    ];
    const state = projectWaveState({
      manifest,
      manifestPath: '<fixture>',
      repoRoot: tmp,
      events,
      receipts,
      reportLoader: (taskId) => reports.get(taskId) ?? null,
    });
    assert(state.complete.includes('wave99-01-atlas'), 'atlas should be complete');
    assert(state.dispatchable.includes('wave99-02-soft-slow'), 'soft slow root should be dispatchable');
    assert(state.dispatchable.includes('wave99-03-follower'), 'follower should ignore soft ref and be dispatchable');
    assert(
      state.needs_finalization.includes('wave99-04-patched'),
      'parent hotfix after worker report should require finalization',
    );
    const atlas = state.tasks.find((t) => t.task_id === 'wave99-01-atlas');
    assert(atlas.reusable_receipt_count === 1, 'atlas receipt should be reusable');
    assert(
      state.next_actions.some((a) => a.action === 'finalize_report' && a.task_id === 'wave99-04-patched'),
      'next_actions should ask to finalize patched task before dispatch',
    );
    assert(
      state.next_actions.some((a) => a.action === 'dispatch_task' && a.task_id === 'wave99-03-follower'),
      'next_actions should include dispatchable follower',
    );

    const blockedManifest = {
      ...manifest,
      nodes: [
        node('wave99-01-root', { minutes: 10 }),
        node('wave99-02-child', {
          depends: ['wave99-01-root'],
          hard: ['wave99-01-root'],
          group: 'B',
        }),
      ],
    };
    const blocked = projectWaveState({
      manifest: blockedManifest,
      manifestPath: '<blocked>',
      repoRoot: tmp,
      events: [],
      receipts: [],
      reportLoader: () => null,
    });
    assert(blocked.dispatchable.includes('wave99-01-root'), 'root should be dispatchable');
    assert(blocked.blocked.includes('wave99-02-child'), 'child should be blocked');
    const child = blocked.tasks.find((t) => t.task_id === 'wave99-02-child');
    assert(child.pending_hard_deps[0] === 'wave99-01-root', 'child should explain pending hard dep');
    assert(
      blocked.next_actions.some((a) => a.action === 'wait_for_hard_deps' && a.task_id === 'wave99-02-child'),
      'next_actions should explain blocked child',
    );

    const manifestFile = path.join(tmp, '.missiond/tasks/wave99/manifest.lisp');
    const lifecycleFile = path.join(tmp, '.missiond/tasks/wave99/task-lifecycle-events.lisp');
    fs.mkdirSync(path.dirname(manifestFile), { recursive: true });
    fs.writeFileSync(manifestFile, `(task-runner-manifest wave99-state
  :schema "missiond.task-runner-manifest.v2"
  :wave wave99
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  (node :task_id wave99-01-root
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 10
        :heartbeat_minutes 5
        :write_scope ["scripts/root.mjs"]))
`);
    fs.writeFileSync(lifecycleFile, `(task-lifecycle-event-log wave99-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave99
  :created-at "2026-04-28T00:00:00Z"
  :sequence 1

  (lifecycle-event
    :id wave99-01-root-worker-commit-001
    :task wave99-01-root
    :actor_role worker
    :event_kind worker_commit
    :commit_role worker
    :seq 1
    :at "2026-04-28T00:00:00Z"
    :touched ["scripts/root.mjs"]
    :summary "worker committed without finalized report"
    :commit_hash abc1234))
`);
    const autoLifecycle = projectWaveStateFromFiles({
      manifestPath: manifestFile,
      repoRoot: tmp,
    });
    assert(
      autoLifecycle.lifecycle_path === '.missiond/tasks/wave99/task-lifecycle-events.lisp',
      'default lifecycle path should be auto-detected',
    );
    assert(
      autoLifecycle.needs_finalization.includes('wave99-01-root'),
      'auto-detected worker_commit should require finalization',
    );

    return { ok: true, cases: 3 };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function node(taskId, opts = {}) {
  return {
    task_id: taskId,
    depends_on: opts.depends ?? [],
    hard_deps: opts.hard ?? [],
    hard_deps_declared: Array.isArray(opts.hard),
    soft_refs: opts.soft ?? [],
    verification_tier: opts.tier ?? 'local',
    dispatch_group: opts.group ?? 'A',
    estimated_minutes: opts.minutes ?? 20,
    heartbeat_minutes: 5,
    write_scope: [`scripts/${taskId}.mjs`],
    notes: null,
    owner: null,
    kind: null,
    loc: null,
  };
}

function fakeReport(taskId, commitHash, opts = {}) {
  return {
    id: taskId,
    taskId,
    status: 'done',
    commitHash,
    agentCommitHash: opts.agentCommitHash ?? null,
    finalCommitHash: opts.finalCommitHash ?? null,
    verifiedCommitHash: opts.verifiedCommitHash ?? null,
    parentPatches: opts.parentPatches ?? [],
    filesChanged: [`scripts/${taskId}.mjs`],
  };
}

function event(task, eventKind, seq, commitHash = null) {
  return {
    id: `${task}-${eventKind}-${seq}`,
    task,
    actor_role: eventKind === 'parent_hotfix' ? 'parent' : 'worker',
    event_kind: eventKind,
    commit_role: eventKind === 'parent_hotfix' ? 'parent_hotfix' : 'worker',
    seq,
    at: '2026-04-28T00:00:00Z',
    touched: [`scripts/${task}.mjs`],
    summary: `${eventKind} fixture`,
    commit_hash: commitHash,
  };
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
