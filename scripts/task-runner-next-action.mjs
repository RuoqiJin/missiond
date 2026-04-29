#!/usr/bin/env node

// MissionD task-runner next-action controller v0.
//
// Read-only by default: projects wave state and selects the next actionable
// priority class. With --emit-dispatch-events it records dispatch decisions
// into the lifecycle ledger; it still does not spawn workers, touch git, call
// a network, or invoke an LLM.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { appendLifecycleEvent } from './task-runner-append-event.mjs';
import { projectWaveStateFromFiles } from './task-runner-wave-state.mjs';

const NEXT_ACTION_SCHEMA = 'missiond.task-runner-next-action.v0';

const usage = `Usage:
  node scripts/task-runner-next-action.mjs --manifest <manifest.lisp>
    [--lifecycle <task-lifecycle-events.lisp>] [--events-dir <task-events-dir>]
    [--receipts <receipts.lisp>]
    [--repo <repo-root>] [--action runnable|all|dispatch_task|finalize_report|wait_for_hard_deps]
    [--limit <n|all>] [--actor-role <role>] [--emit-dispatch-events] [--json]
    [--request-id <request-id> --request-events-dir <dir>]
  node scripts/task-runner-next-action.mjs --dry-fixture [--json]

Selects MissionD's next task-runner action from Lisp artifacts. Default action
policy is "runnable": finalize_report actions win; otherwise all currently
dispatchable tasks are selected; otherwise blocked wait actions are surfaced.

Mutation boundary:
  Default mode is read-only. --emit-dispatch-events may only be used when the
  selected actions are dispatch_task. It appends lifecycle event_kind=dispatch
  records and then re-projects state, so repeated callers do not dispatch the
  same task again.
`;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    manifest: null,
    lifecycle: null,
    eventsDir: null,
    receipts: null,
    repo: process.cwd(),
    action: 'runnable',
    limit: 'all',
    actorRole: 'orchestrator',
    requestId: null,
    requestEventsDir: null,
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
    } else if (arg === '--events-dir') {
      opts.eventsDir = argv[++i] ?? fail('--events-dir requires a value');
    } else if (arg.startsWith('--events-dir=')) {
      opts.eventsDir = arg.slice('--events-dir='.length);
    } else if (arg === '--receipts') {
      opts.receipts = argv[++i] ?? fail('--receipts requires a value');
    } else if (arg.startsWith('--receipts=')) {
      opts.receipts = arg.slice('--receipts='.length);
    } else if (arg === '--repo') {
      opts.repo = argv[++i] ?? fail('--repo requires a value');
    } else if (arg.startsWith('--repo=')) {
      opts.repo = arg.slice('--repo='.length);
    } else if (arg === '--action') {
      opts.action = argv[++i] ?? fail('--action requires a value');
    } else if (arg.startsWith('--action=')) {
      opts.action = arg.slice('--action='.length);
    } else if (arg === '--limit') {
      opts.limit = argv[++i] ?? fail('--limit requires a value');
    } else if (arg.startsWith('--limit=')) {
      opts.limit = arg.slice('--limit='.length);
    } else if (arg === '--actor-role') {
      opts.actorRole = argv[++i] ?? fail('--actor-role requires a value');
    } else if (arg.startsWith('--actor-role=')) {
      opts.actorRole = arg.slice('--actor-role='.length);
    } else if (arg === '--request-id') {
      opts.requestId = argv[++i] ?? fail('--request-id requires a value');
    } else if (arg.startsWith('--request-id=')) {
      opts.requestId = arg.slice('--request-id='.length);
    } else if (arg === '--request-events-dir') {
      opts.requestEventsDir = argv[++i] ?? fail('--request-events-dir requires a value');
    } else if (arg.startsWith('--request-events-dir=')) {
      opts.requestEventsDir = arg.slice('--request-events-dir='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

export function runNextAction({
  manifestPath,
  repoRoot = process.cwd(),
  lifecyclePath = null,
  eventsDirPath = null,
  receiptsPath = null,
  action = 'runnable',
  limit = 'all',
  actorRole = 'orchestrator',
  requestId = null,
  requestEventsDir = null,
  emitDispatchEvents = false,
  nowIso = isoNow(),
}) {
  const repo = path.resolve(repoRoot);
  validateRequestProjectionArgs(requestId, requestEventsDir);
  const requestEventsTarget = requestEventsDir ? path.resolve(repo, requestEventsDir) : null;
  const before = projectWaveStateFromFiles({
    manifestPath,
    repoRoot: repo,
    lifecyclePath,
    eventsDirPath,
    receiptsPath,
  });
  const selectedActions = selectNextActions(before, { action, limit });
  const selectedKinds = [...new Set(selectedActions.map((a) => a.action))].sort();
  const lifecycleTarget = before.lifecycle_path ?? defaultLifecyclePath(before.wave);
  const eventsDirTarget = eventsDirPath ?? before.events_dir_path ?? defaultEventsDirPath(before.wave);
  const result = {
    ok: true,
    schema: NEXT_ACTION_SCHEMA,
    mutation_mode: emitDispatchEvents ? 'emit-dispatch-events' : 'read-only',
    selection_policy: action,
    limit,
    wave: before.wave,
    manifest_path: before.manifest_path,
    lifecycle_path: lifecycleTarget,
    events_dir_path: eventsDirTarget,
    counts: before.counts,
    selected_count: selectedActions.length,
    selected_actions: selectedActions,
    appended_events: [],
    after_counts: null,
  };
  if (requestId || requestEventsTarget) {
    result.request_id = requestId;
    result.request_events_dir = requestEventsTarget
      ? toRepoRelative(requestEventsTarget, repo)
      : null;
  }

  if (!emitDispatchEvents) return result;

  if (selectedActions.length === 0) {
    result.after_counts = before.counts;
    return result;
  }
  if (selectedKinds.length !== 1 || selectedKinds[0] !== 'dispatch_task') {
    throw new Error(
      `--emit-dispatch-events only supports dispatch_task selections; got ${selectedKinds.join(', ')}`,
    );
  }
  result.appended_events = emitDispatchEventsForActions({
    actions: selectedActions,
    repoRoot: repo,
    lifecyclePath: lifecycleTarget,
    eventsDirPath: eventsDirTarget,
    manifestPath: before.manifest_path,
    actorRole,
    requestId,
    requestEventsDir: requestEventsTarget,
    nowIso,
    wave: before.wave,
  });
  const after = projectWaveStateFromFiles({
    manifestPath,
    repoRoot: repo,
    lifecyclePath: lifecycleTarget,
    eventsDirPath: eventsDirTarget,
    receiptsPath,
  });
  result.after_counts = after.counts;
  result.after_running = after.running;
  result.after_dispatchable = after.dispatchable;
  return result;
}

export function selectNextActions(state, { action = 'runnable', limit = 'all' } = {}) {
  const all = Array.isArray(state?.next_actions) ? state.next_actions : [];
  let selected;
  if (action === 'all') {
    selected = all;
  } else if (action === 'runnable') {
    const finalizers = all.filter((a) => a.action === 'finalize_report');
    if (finalizers.length > 0) selected = finalizers;
    else {
      const dispatches = all.filter((a) => a.action === 'dispatch_task');
      selected = dispatches.length > 0
        ? dispatches
        : all.filter((a) => a.action === 'wait_for_hard_deps');
    }
  } else if (['dispatch_task', 'finalize_report', 'wait_for_hard_deps'].includes(action)) {
    selected = all.filter((a) => a.action === action);
  } else {
    throw new Error(
      '--action must be one of runnable|all|dispatch_task|finalize_report|wait_for_hard_deps',
    );
  }
  return selected.slice(0, parseLimit(limit, selected.length));
}

export function emitDispatchEventsForActions({
  actions,
  repoRoot,
  lifecyclePath,
  eventsDirPath = null,
  manifestPath,
  actorRole,
  requestId = null,
  requestEventsDir = null,
  nowIso,
  wave,
}) {
  const ledgerPath = lifecyclePath ? path.resolve(repoRoot, lifecyclePath) : null;
  const eventsDir = eventsDirPath ? path.resolve(repoRoot, eventsDirPath) : null;
  const appended = [];
  for (const action of actions) {
    const touched = uniqueSorted([manifestPath, action.brief_path].filter(Boolean));
    const appendResult = appendLifecycleEvent({
      ledgerPath,
      eventsDir,
      task: action.task_id,
      eventKind: 'dispatch',
      actorRole,
      commitRole: 'none',
      touched,
      summary: `Dispatch ${action.task_id}: hard dependencies satisfied.`,
      at: nowIso,
      wave,
      requestId,
      requestEventsDir,
    });
    const event = { ...appendResult.event };
    if (appendResult.eventFile) {
      event.event_file = toRepoRelative(appendResult.eventFile, repoRoot);
    }
    if (appendResult.requestEventPath) {
      event.request_event_path = toRepoRelative(appendResult.requestEventPath, repoRoot);
    }
    appended.push(event);
  }
  return appended;
}

function parseLimit(value, fallback) {
  if (value == null || value === '' || value === 'all') return fallback;
  const n = Number.parseInt(String(value), 10);
  if (!Number.isInteger(n) || n < 0) {
    throw new Error(`--limit must be a non-negative integer or "all"; got ${JSON.stringify(value)}`);
  }
  return n;
}

function defaultLifecyclePath(wave) {
  return path.posix.join('.missiond', 'tasks', wave, 'task-lifecycle-events.lisp');
}

function defaultEventsDirPath(wave) {
  return path.posix.join('.missiond', 'tasks', wave, 'events');
}

function validateRequestProjectionArgs(requestId, requestEventsDir) {
  if ((requestId && !requestEventsDir) || (!requestId && requestEventsDir)) {
    throw new Error('--request-id and --request-events-dir must be supplied together');
  }
}

function toRepoRelative(filePath, repoRoot) {
  const rel = path.relative(path.resolve(repoRoot), path.resolve(filePath));
  if (rel.startsWith('..') || path.isAbsolute(rel)) return filePath;
  return rel.split(path.sep).join('/');
}

function uniqueSorted(values) {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
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
      else console.log(`task-runner-next-action fixtures OK (${result.cases} cases)`);
      return;
    }
    if (!opts.manifest) fail('--manifest is required');
    const result = runNextAction({
      manifestPath: opts.manifest,
      repoRoot: opts.repo,
      lifecyclePath: opts.lifecycle,
      eventsDirPath: opts.eventsDir,
      receiptsPath: opts.receipts,
      action: opts.action,
      limit: opts.limit,
      actorRole: opts.actorRole,
      requestId: opts.requestId,
      requestEventsDir: opts.requestEventsDir,
      emitDispatchEvents: opts.emitDispatchEvents,
    });
    if (opts.json) {
      console.log(JSON.stringify(result, null, 2));
    } else {
      console.log(
        `task-runner-next-action OK (${result.wave}): ` +
          `${result.selected_count} selected, ${result.mutation_mode}`,
      );
    }
  } catch (err) {
    process.stderr.write(`task-runner-next-action: ${err?.message ?? String(err)}\n`);
    process.exit(1);
  }
}

function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-next-action-'));
  try {
    const manifestPath = path.join(tmp, '.missiond/tasks/wave99/manifest.lisp');
    fs.mkdirSync(path.dirname(manifestPath), { recursive: true });
    fs.writeFileSync(manifestPath, fixtureManifest());

    const readOnly = runNextAction({
      manifestPath,
      repoRoot: tmp,
      action: 'runnable',
      nowIso: '2026-04-28T00:00:00Z',
    });
    assert(readOnly.mutation_mode === 'read-only', 'default mode should be read-only');
    assert(readOnly.selected_count === 2, 'two roots should be selected for dispatch');
    assert(
      readOnly.selected_actions.every((a) => a.action === 'dispatch_task'),
      'read-only selection should pick dispatchable roots',
    );

    const emitted = runNextAction({
      manifestPath,
      repoRoot: tmp,
      action: 'dispatch_task',
      emitDispatchEvents: true,
      requestId: 'req-wave99-next',
      requestEventsDir: '.missiond/requests/req-wave99-next/events',
      nowIso: '2026-04-28T00:00:00Z',
    });
    assert(emitted.appended_events.length === 2, 'two dispatch events should append');
    assert(
      emitted.appended_events.every((event) => event.request_event_path?.includes('/events/')),
      'dispatch events should project request-local event files when request args are supplied',
    );
    assert(
      emitted.appended_events.every((event) => event.event_file?.endsWith('.event.lisp')),
      'dispatch events should also write task-scoped one-event files via the auto-detected events-dir',
    );
    assert(
      emitted.events_dir_path === '.missiond/tasks/wave99/events',
      'next-action should expose the resolved task-scoped events_dir_path',
    );
    assert(emitted.after_counts.running === 2, 'dispatch events should move selected tasks to running');
    assert(emitted.after_counts.dispatchable === 0, 'emitted tasks should not remain dispatchable');

    const idempotent = runNextAction({
      manifestPath,
      repoRoot: tmp,
      action: 'dispatch_task',
      nowIso: '2026-04-28T00:00:00Z',
    });
    assert(idempotent.selected_count === 0, 'already-dispatched tasks should not be selected again');

    const finalizerRepo = path.join(tmp, 'finalizer');
    const finalizerManifestPath = path.join(finalizerRepo, '.missiond/tasks/wave98/manifest.lisp');
    const finalizerLifecyclePath = path.join(finalizerRepo, '.missiond/tasks/wave98/task-lifecycle-events.lisp');
    fs.mkdirSync(path.dirname(finalizerManifestPath), { recursive: true });
    fs.writeFileSync(finalizerManifestPath, fixtureFinalizerManifest());
    fs.writeFileSync(finalizerLifecyclePath, fixtureFinalizerLifecycle());
    const finalizer = runNextAction({
      manifestPath: finalizerManifestPath,
      repoRoot: finalizerRepo,
      action: 'runnable',
      nowIso: '2026-04-28T00:00:00Z',
    });
    assert(finalizer.selected_count === 1, 'finalization should have priority over dispatch');
    assert(finalizer.selected_actions[0].action === 'finalize_report', 'first runnable action should finalize');

    return { ok: true, cases: 4 };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function fixtureManifest() {
  return `(task-runner-manifest wave99-next-action
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
        :write_scope ["scripts/root.mjs"])
  (node :task_id wave99-02-peer
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 8
        :heartbeat_minutes 5
        :write_scope ["scripts/peer.mjs"])
  (node :task_id wave99-03-child
        :depends_on [wave99-01-root]
        :hard_deps [wave99-01-root]
        :soft_refs [wave99-02-peer]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 5
        :heartbeat_minutes 5
        :write_scope ["scripts/child.mjs"]))
`;
}

function fixtureFinalizerManifest() {
  return `(task-runner-manifest wave98-finalizer
  :schema "missiond.task-runner-manifest.v2"
  :wave wave98
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave98-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  (node :task_id wave98-01-needs-final
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 10
        :heartbeat_minutes 5
        :write_scope ["scripts/final.mjs"])
  (node :task_id wave98-02-ready
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 10
        :heartbeat_minutes 5
        :write_scope ["scripts/ready.mjs"]))
`;
}

function fixtureFinalizerLifecycle() {
  return `(task-lifecycle-event-log wave98-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave98
  :created-at "2026-04-28T00:00:00Z"
  :sequence 1

  (lifecycle-event
    :id wave98-01-worker-commit-001
    :task wave98-01-needs-final
    :actor_role worker
    :event_kind worker_commit
    :commit_role worker
    :seq 1
    :at "2026-04-28T00:00:00Z"
    :touched ["scripts/final.mjs"]
    :summary "worker committed but report is not finalized"
    :commit_hash abc1001))
`;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
