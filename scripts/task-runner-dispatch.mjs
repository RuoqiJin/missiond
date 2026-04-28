#!/usr/bin/env node

// MissionD task-runner dispatch descriptor v0.
//
// Bridges Lisp task-runner state to the existing mission_task_delegate MCP
// surface. Default mode is read-only: it selects currently runnable work and
// emits delegate payloads. It never spawns workers, invokes MCP, touches git,
// calls a network, or calls an LLM.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { runNextAction } from './task-runner-next-action.mjs';

const DISPATCH_SCHEMA = 'missiond.task-runner-dispatch.v0';

const usage = `Usage:
  node scripts/task-runner-dispatch.mjs --manifest <manifest.lisp>
    [--lifecycle <task-lifecycle-events.lisp>] [--receipts <receipts.lisp>]
    [--repo <repo-root>] [--max-parallel <n|all>] [--actor-role <role>]
    [--allow-missing-briefs] [--emit-dispatch-events] [--json]
  node scripts/task-runner-dispatch.mjs --dry-fixture [--json]

Builds mission_task_delegate call descriptors from task-runner next actions.

Default mode is read-only and returns delegate_calls only. With
--emit-dispatch-events it first validates every selected dispatch has a
rendered brief, appends lifecycle dispatch events, and re-projects state.

It does not call mission_task_delegate itself. A daemon/MCP wrapper can submit
the returned delegate_calls, preserving this CLI as the deterministic Lisp
orchestration boundary.
`;

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
    maxParallel: 'all',
    actorRole: 'orchestrator',
    allowMissingBriefs: false,
    emitDispatchEvents: false,
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
    } else if (arg === '--allow-missing-briefs') {
      opts.allowMissingBriefs = true;
    } else if (arg === '--emit-dispatch-events') {
      opts.emitDispatchEvents = true;
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
    } else if (arg === '--max-parallel') {
      opts.maxParallel = argv[++i] ?? fail('--max-parallel requires a value');
    } else if (arg.startsWith('--max-parallel=')) {
      opts.maxParallel = arg.slice('--max-parallel='.length);
    } else if (arg === '--actor-role') {
      opts.actorRole = argv[++i] ?? fail('--actor-role requires a value');
    } else if (arg.startsWith('--actor-role=')) {
      opts.actorRole = arg.slice('--actor-role='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

export function runDispatch({
  manifestPath,
  repoRoot = process.cwd(),
  lifecyclePath = null,
  receiptsPath = null,
  maxParallel = 'all',
  actorRole = 'orchestrator',
  allowMissingBriefs = false,
  emitDispatchEvents = false,
  nowIso = isoNow(),
}) {
  const repo = path.resolve(repoRoot);
  const before = runNextAction({
    manifestPath,
    repoRoot: repo,
    lifecyclePath,
    receiptsPath,
    action: 'runnable',
    limit: maxParallel,
    actorRole,
    emitDispatchEvents: false,
    nowIso,
  });
  const dispatchActions = before.selected_actions.filter((a) => a.action === 'dispatch_task');
  const blockers = before.selected_actions.filter((a) => a.action !== 'dispatch_task');
  const missingBriefs = dispatchActions
    .map((action) => ({
      task_id: action.task_id,
      brief_path: action.brief_path,
      exists: fileExists(repo, action.brief_path),
    }))
    .filter((entry) => !entry.exists);

  const canBuildCalls =
    blockers.length === 0 &&
    dispatchActions.length > 0 &&
    (allowMissingBriefs || missingBriefs.length === 0);

  const delegateCalls = canBuildCalls
    ? dispatchActions.map((action) =>
        buildDelegateCall({ action, repoRoot: repo, manifestPath: before.manifest_path, actorRole }),
      )
    : [];

  const result = {
    ok: true,
    schema: DISPATCH_SCHEMA,
    mutation_mode: emitDispatchEvents ? 'emit-dispatch-events' : 'read-only',
    wave: before.wave,
    manifest_path: before.manifest_path,
    lifecycle_path: before.lifecycle_path,
    max_parallel: maxParallel,
    status: computeStatus({ blockers, dispatchActions, missingBriefs, allowMissingBriefs }),
    counts: before.counts,
    selected_actions: before.selected_actions,
    blocker_actions: blockers,
    missing_briefs: missingBriefs.map(({ task_id, brief_path }) => ({ task_id, brief_path })),
    delegate_call_count: delegateCalls.length,
    delegate_calls: delegateCalls,
    appended_events: [],
    after_counts: null,
    after_running: null,
    after_dispatchable: null,
  };

  if (!emitDispatchEvents || delegateCalls.length === 0) return result;

  const after = runNextAction({
    manifestPath,
    repoRoot: repo,
    lifecyclePath,
    receiptsPath,
    action: 'dispatch_task',
    limit: maxParallel,
    actorRole,
    emitDispatchEvents: true,
    nowIso,
  });
  result.appended_events = after.appended_events;
  result.after_counts = after.after_counts;
  result.after_running = after.after_running;
  result.after_dispatchable = after.after_dispatchable;
  return result;
}

export function buildDelegateCall({ action, repoRoot, manifestPath, actorRole = 'orchestrator' }) {
  const taskId = action.task_id;
  const briefPath = action.brief_path;
  const contractPath = path.posix.join('.missiond', 'tasks', action.wave, `${taskId}.lisp`);
  const estimated = Number.isInteger(action.estimated_minutes) ? action.estimated_minutes : 30;
  return {
    task_id: taskId,
    target_tool: 'mission_task_delegate',
    target_args: {
      objective: buildObjective({
        taskId,
        wave: action.wave,
        briefPath,
        contractPath,
        manifestPath,
        softRefs: action.soft_refs ?? [],
      }),
      intent: 'code',
      cwd: repoRoot,
      timeout_secs: timeoutForMinutes(estimated),
      priority: priorityForAction(action),
      context_hints: buildContextHints({ action, briefPath, contractPath, manifestPath }),
    },
    dispatch_event: {
      event_kind: 'dispatch',
      actor_role: actorRole,
      touched: [manifestPath, briefPath].filter(Boolean).sort(),
    },
  };
}

function buildObjective({ taskId, wave, briefPath, contractPath, manifestPath, softRefs }) {
  const lines = [
    `Execute MissionD task ${taskId} from wave ${wave}.`,
    `Read and follow the thin brief first: ${briefPath}`,
    `Task contract SSOT: ${contractPath}`,
    `Wave manifest: ${manifestPath}`,
    'Follow the shared preamble, lifecycle/report/commit protocol, write scope, and acceptance commands in the brief.',
  ];
  if (softRefs.length > 0) {
    lines.push(`Soft context refs are guidance only, not blockers: ${softRefs.join(', ')}`);
  }
  lines.push('When done, write the task report and commit only the declared write scope.');
  return lines.join('\n');
}

function buildContextHints({ action, briefPath, contractPath, manifestPath }) {
  return uniqueSorted([
    action.task_id,
    action.wave,
    briefPath,
    contractPath,
    manifestPath,
    ...(action.soft_refs ?? []),
  ]);
}

function computeStatus({ blockers, dispatchActions, missingBriefs, allowMissingBriefs }) {
  if (blockers.some((a) => a.action === 'finalize_report')) return 'blocked_by_finalization';
  if (blockers.some((a) => a.action === 'wait_for_hard_deps')) return 'blocked_by_hard_deps';
  if (dispatchActions.length === 0) return 'idle';
  if (!allowMissingBriefs && missingBriefs.length > 0) return 'blocked_missing_briefs';
  return 'ready_to_delegate';
}

function priorityForAction(action) {
  if (action.verification_tier === 'full' || action.verification_tier === 'smoke') return 'high';
  return 'medium';
}

function timeoutForMinutes(minutes) {
  const padded = (minutes + 10) * 60;
  return Math.max(900, Math.min(7200, padded));
}

function fileExists(repoRoot, repoRelativePath) {
  return Boolean(repoRelativePath) && fs.existsSync(path.resolve(repoRoot, repoRelativePath));
}

function uniqueSorted(values) {
  return [...new Set(values.filter(Boolean))].sort((a, b) => a.localeCompare(b));
}

function isoNow() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  try {
    if (opts.dryFixture) {
      const result = runFixtures();
      if (opts.json) console.log(JSON.stringify(result, null, 2));
      else console.log(`task-runner-dispatch fixtures OK (${result.cases} cases)`);
      return;
    }
    if (!opts.manifest) fail('--manifest is required');
    const result = runDispatch({
      manifestPath: opts.manifest,
      repoRoot: opts.repo,
      lifecyclePath: opts.lifecycle,
      receiptsPath: opts.receipts,
      maxParallel: opts.maxParallel,
      actorRole: opts.actorRole,
      allowMissingBriefs: opts.allowMissingBriefs,
      emitDispatchEvents: opts.emitDispatchEvents,
    });
    if (opts.json) {
      console.log(JSON.stringify(result, null, 2));
    } else {
      console.log(
        `task-runner-dispatch OK (${result.wave}): ` +
          `${result.status}, ${result.delegate_call_count} delegate call(s), ${result.mutation_mode}`,
      );
    }
  } catch (err) {
    process.stderr.write(`task-runner-dispatch: ${err?.message ?? String(err)}\n`);
    process.exit(1);
  }
}

function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-dispatch-'));
  try {
    const manifestPath = path.join(tmp, '.missiond/tasks/wave99/manifest.lisp');
    const briefDir = path.join(tmp, '.missiond/claudecode');
    fs.mkdirSync(path.dirname(manifestPath), { recursive: true });
    fs.mkdirSync(briefDir, { recursive: true });
    fs.writeFileSync(manifestPath, fixtureManifest());
    fs.writeFileSync(path.join(briefDir, 'wave99-01-alpha.md'), '# alpha\n');
    fs.writeFileSync(path.join(briefDir, 'wave99-02-beta.md'), '# beta\n');

    const readOnly = runDispatch({
      manifestPath,
      repoRoot: tmp,
      maxParallel: 1,
      nowIso: '2026-04-28T00:00:00Z',
    });
    assert(readOnly.status === 'ready_to_delegate', 'one root should be ready');
    assert(readOnly.delegate_call_count === 1, 'maxParallel=1 should emit one call');
    assert(
      readOnly.delegate_calls[0].target_tool === 'mission_task_delegate',
      'delegate target should be mission_task_delegate',
    );
    assert(
      readOnly.delegate_calls[0].target_args.objective.includes(readOnly.delegate_calls[0].task_id),
      'objective should name the selected task',
    );

    const emitted = runDispatch({
      manifestPath,
      repoRoot: tmp,
      maxParallel: 1,
      emitDispatchEvents: true,
      nowIso: '2026-04-28T00:01:00Z',
    });
    assert(emitted.appended_events.length === 1, 'emit mode should append one dispatch event');
    assert(emitted.after_running.length === 1, 'after projection should mark one task running');

    const missingManifestPath = path.join(tmp, '.missiond/tasks/wave98/manifest.lisp');
    fs.mkdirSync(path.dirname(missingManifestPath), { recursive: true });
    fs.writeFileSync(missingManifestPath, fixtureManifest('wave98'));
    const missing = runDispatch({
      manifestPath: missingManifestPath,
      repoRoot: tmp,
      maxParallel: 'all',
    });
    assert(missing.status === 'blocked_missing_briefs', 'missing briefs should block delegation');
    assert(missing.delegate_call_count === 0, 'missing briefs should emit no delegate calls');

    const finalized = runDispatch({
      manifestPath,
      repoRoot: tmp,
      maxParallel: 'all',
      nowIso: '2026-04-28T00:02:00Z',
    });
    assert(
      finalized.status === 'ready_to_delegate',
      'a still-undispatched root should remain ready after maxParallel=1 dispatch',
    );

    return { ok: true, cases: 4 };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function fixtureManifest(wave = 'wave99') {
  return `(task-runner-manifest ${wave}-dispatch
  :schema "missiond.task-runner-manifest.v2"
  :wave ${wave}
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/${wave}-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  (node :task_id ${wave}-01-alpha
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 15
        :heartbeat_minutes 5
        :write_scope ["scripts/${wave}-alpha.mjs"])
  (node :task_id ${wave}-02-beta
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 20
        :heartbeat_minutes 5
        :write_scope ["scripts/${wave}-beta.mjs"])
  (node :task_id ${wave}-03-child
        :depends_on [${wave}-01-alpha]
        :hard_deps [${wave}-01-alpha]
        :soft_refs [${wave}-02-beta]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 10
        :heartbeat_minutes 5
        :write_scope ["scripts/${wave}-child.mjs"]))\n`;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
