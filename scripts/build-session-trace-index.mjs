#!/usr/bin/env node

// Read-only corpus indexer for MissionD session-trace v1 ledgers.
//
// Scans .missiond/tasks/**/session-trace.lisp (or explicit file arguments)
// and emits a stable JSON corpus index suitable for downstream router-policy
// experiments (wave24-03+). This indexer NEVER writes files and NEVER makes
// router recommendations — it only aggregates observed facts.
//
// Top-level JSON keys (sorted, deterministic):
//   - bottleneck_tags : corpus-level array of tag strings observed across tasks
//   - by_backend      : map backend -> aggregate bucket
//   - by_task         : map task_id -> aggregate bucket (with bottleneck_tags)
//   - by_wave         : map wave_id -> aggregate bucket (with bottleneck_tags)
//   - schema          : "missiond.session-trace.v1"
//   - source_files    : sorted array of { path, events, traces, wave }
//   - thresholds      : threshold values used for bottleneck classification
//   - totals          : { events, tasks, backends, waves, files, commits,
//                         total_duration_ms }
//
// Bottleneck tag rules (match wave23-06 analyze-session-trace.mjs thresholds):
//   - long-running    total_duration_ms >= 1_800_000    (30 minutes)
//   - high-retry      retry_count       >= 3
//   - many-failures   failure_count     >= 2
//   - no-completion   dispatch_count    >= 1 AND complete_count == 0
//
// Non-goals (explicit):
//   - does NOT write any file (no fs.writeFile / fs.appendFile / fs.mkdtemp
//     in production paths — fixtures use a temp dir which is rm'd at end)
//   - does NOT execute traced commands
//   - does NOT recommend router policies, model swaps, or backend changes
//   - does NOT mutate any ledger; it only reads and projects
//
// Usage:
//   node scripts/build-session-trace-index.mjs [--json] [--dry-fixture] \
//     [<trace.lisp> ...]
//
// Default scan path when no files passed: .missiond/tasks/**/session-trace.lisp
// (resolved via Node's fs.readdirSync recursion; no shell glob is required.)

import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';

import {
  SCHEMA,
  KIND_VALUES,
  parseTraceEvents,
} from './check-session-trace.mjs';

const usage = `Usage:
  node scripts/build-session-trace-index.mjs [--json] [--dry-fixture] [<trace.lisp> ...]

Scans .missiond/tasks/**/session-trace.lisp (default) or the supplied trace
files and emits a stable corpus index aggregating per-task, per-backend, and
per-wave event facts. The output is read-only: stdout/stderr only, no file
writes.

Use --json to emit a deterministic JSON object (keys sorted) suitable for
downstream tooling. Use --dry-fixture to run self-contained pass cases that
prove aggregation correctness.
`;

// Bottleneck thresholds — keep in lock-step with analyze-session-trace.mjs
// (wave23-06). If those thresholds move, update both files together so the
// corpus and per-trace analyses remain comparable.
export const LONG_RUNNING_MS = 1_800_000; // 30 minutes
export const HIGH_RETRY = 3;
export const MANY_FAILURES = 2;

// Default scan root for the corpus. Anchored at process.cwd() at call-time so
// the script can be invoked from any directory and still pick up the repo's
// task ledgers when run from the repo root.
export const DEFAULT_SCAN_ROOT = path.join('.missiond', 'tasks');
export const TRACE_FILENAME = 'session-trace.lisp';

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  const inputs = [];

  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      inputs.push(arg);
    }
  }

  if (dryFixture) {
    runFixtures(json);
    return;
  }

  const cwd = process.cwd();
  let files;
  if (inputs.length === 0) {
    files = findSessionTraceFiles(path.resolve(cwd, DEFAULT_SCAN_ROOT));
  } else {
    files = [...new Set(inputs.map((input) => path.resolve(cwd, input)))];
  }

  const traces = [];
  for (const file of files) {
    if (!fs.existsSync(file)) {
      console.error(`build-session-trace-index: file not found: ${file}`);
      process.exit(2);
    }
    for (const t of parseTraceEvents(file)) traces.push(t);
  }

  const index = buildIndex(traces);
  emit(index, json);
}

// Recursive directory walk that returns absolute paths to every file named
// `session-trace.lisp` under `root`. We deliberately do NOT depend on shell
// `**` expansion — Node's fs.readdirSync is portable and predictable. The
// returned list is sorted for stable output regardless of filesystem order.
export function findSessionTraceFiles(root) {
  const out = [];
  if (!fs.existsSync(root)) return out;
  const stack = [root];
  while (stack.length > 0) {
    const dir = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      // Skip hidden dirs other than the .missiond root we may already be in.
      if (entry.isDirectory()) {
        if (entry.name === '.git' || entry.name === 'node_modules') continue;
        stack.push(path.join(dir, entry.name));
      } else if (entry.isFile() && entry.name === TRACE_FILENAME) {
        out.push(path.join(dir, entry.name));
      }
    }
  }
  out.sort();
  return out;
}

// Pure aggregator: takes parsed traces (output of parseTraceEvents) and
// returns the structured corpus index. Exposed for tooling and fixtures.
export function buildIndex(traces) {
  const tasks = new Map();
  const backends = new Map();
  const waves = new Map();
  let totalEvents = 0;
  const filesAll = new Set();
  const commitsAll = new Set();

  for (const trace of traces) {
    for (const event of trace.events) {
      totalEvents += 1;
      if (event.task) bumpBucket(tasks, event.task, event);
      if (event.backend) bumpBucket(backends, event.backend, event);
      const waveId = waveOfTask(event.task) ?? trace.wave ?? null;
      if (waveId) bumpBucket(waves, waveId, event);
      for (const f of event.files) filesAll.add(f);
      if (event.commit_hash) commitsAll.add(event.commit_hash);
    }
  }

  // Finalize buckets, attaching bottleneck_tags to task/wave granularity.
  // Backends do not get tagged at the indexer level: a backend's failures may
  // straddle many task buckets and tagging it would imply a recommendation,
  // which this script explicitly does not make.
  const tasksOut = {};
  const taskBottleneckTags = new Set();
  for (const [taskId, agg] of [...tasks].sort(byKey)) {
    const bucket = finalizeBucket(agg, /* withTags */ true);
    tasksOut[taskId] = bucket;
    for (const tag of bucket.bottleneck_tags) taskBottleneckTags.add(tag);
  }

  const backendsOut = {};
  for (const [backendId, agg] of [...backends].sort(byKey)) {
    backendsOut[backendId] = finalizeBucket(agg, /* withTags */ false);
  }

  const wavesOut = {};
  for (const [waveId, agg] of [...waves].sort(byKey)) {
    wavesOut[waveId] = finalizeBucket(agg, /* withTags */ true);
  }

  const sourceFiles = traces
    .map((t) => ({
      path: t.file,
      wave: t.wave ?? null,
      schema: t.header.schema ?? null,
      traces: 1,
      events: t.events.length,
    }))
    .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));

  const totalDuration = Object.values(tasksOut).reduce(
    (sum, t) => sum + (t.total_duration_ms ?? 0),
    0,
  );

  return {
    bottleneck_tags: [...taskBottleneckTags].sort(),
    by_backend: backendsOut,
    by_task: tasksOut,
    by_wave: wavesOut,
    schema: SCHEMA,
    source_files: sourceFiles,
    thresholds: {
      high_retry: HIGH_RETRY,
      long_running_ms: LONG_RUNNING_MS,
      many_failures: MANY_FAILURES,
    },
    totals: {
      backends: backends.size,
      commits: commitsAll.size,
      events: totalEvents,
      files: filesAll.size,
      tasks: tasks.size,
      total_duration_ms: totalDuration,
      traces: traces.length,
      waves: waves.size,
    },
  };
}

// Extract a wave id from a task id. Task ids are conventionally formed as
// `wave<N>-<seq>-<slug>` (e.g. `wave23-06-trace-summary-analyzer-v0`). When the
// pattern does not match — for example, an off-wave task id — we return null
// and let the caller fall back to the trace header's wave id.
export function waveOfTask(taskId) {
  if (typeof taskId !== 'string' || taskId === '') return null;
  const m = /^(wave\d+)\b/.exec(taskId);
  return m ? m[1] : null;
}

function byKey(a, b) {
  return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0;
}

function bumpBucket(map, key, event) {
  let agg = map.get(key);
  if (!agg) {
    agg = freshBucket();
    map.set(key, agg);
  }
  agg.events += 1;
  agg.kinds[event.kind] = (agg.kinds[event.kind] ?? 0) + 1;
  if (event.kind === 'command') agg.command_count += 1;
  if (event.kind === 'test') agg.test_count += 1;
  if (event.kind === 'failure') agg.failure_count += 1;
  if (event.kind === 'retry') agg.retry_count += 1;
  if (event.kind === 'dispatch') agg.dispatch_count += 1;
  if (event.kind === 'complete') agg.complete_count += 1;
  if (event.kind === 'commit') agg.commit_event_count += 1;
  if (typeof event.duration_ms === 'number') {
    agg.total_duration_ms += event.duration_ms;
  }
  for (const f of event.files) agg._files.add(f);
  if (event.commit_hash) agg._commits.add(event.commit_hash);
}

function freshBucket() {
  const kinds = {};
  for (const k of [...KIND_VALUES].sort()) kinds[k] = 0;
  return {
    events: 0,
    kinds,
    command_count: 0,
    test_count: 0,
    failure_count: 0,
    retry_count: 0,
    dispatch_count: 0,
    complete_count: 0,
    commit_event_count: 0,
    total_duration_ms: 0,
    _files: new Set(),
    _commits: new Set(),
  };
}

function finalizeBucket(agg, withTags) {
  const sortedKinds = {};
  for (const key of Object.keys(agg.kinds).sort()) sortedKinds[key] = agg.kinds[key];
  const out = {
    bottleneck_tags: [],
    command_count: agg.command_count,
    commit_event_count: agg.commit_event_count,
    commits: [...agg._commits].sort(),
    complete_count: agg.complete_count,
    dispatch_count: agg.dispatch_count,
    events: agg.events,
    failure_count: agg.failure_count,
    files: [...agg._files].sort(),
    files_touched: agg._files.size,
    kinds: sortedKinds,
    retry_count: agg.retry_count,
    test_count: agg.test_count,
    total_duration_ms: agg.total_duration_ms,
  };
  if (withTags) {
    out.bottleneck_tags = computeBottleneckTags(out);
  }
  return out;
}

// Pure tag computation; thresholds match wave23-06 analyze-session-trace.mjs
// (long-running, high-retry, many-failures, no-completion). Exposed for
// tooling and fixtures so other scripts can label without re-implementing.
export function computeBottleneckTags(bucket) {
  const tags = [];
  if (bucket.total_duration_ms >= LONG_RUNNING_MS) tags.push('long-running');
  if (bucket.retry_count >= HIGH_RETRY) tags.push('high-retry');
  if (bucket.failure_count >= MANY_FAILURES) tags.push('many-failures');
  if (bucket.dispatch_count >= 1 && bucket.complete_count === 0) {
    tags.push('no-completion');
  }
  return tags;
}

// Stable JSON: sort all object keys recursively so byte-identical output is
// reproducible across runs / machines / Node versions. Arrays preserve their
// caller-supplied order (we already sort them where order matters).
export function stableStringify(value, indent = 2) {
  return JSON.stringify(sortKeysDeep(value), null, indent);
}

function sortKeysDeep(value) {
  if (Array.isArray(value)) return value.map(sortKeysDeep);
  if (value && typeof value === 'object') {
    const out = {};
    for (const key of Object.keys(value).sort()) out[key] = sortKeysDeep(value[key]);
    return out;
  }
  return value;
}

function emit(index, json) {
  if (json) {
    console.log(stableStringify(index));
    return;
  }
  const lines = [];
  lines.push(`session-trace corpus index (${index.schema})`);
  lines.push(
    `  files=${index.source_files.length} traces=${index.totals.traces} ` +
      `events=${index.totals.events} tasks=${index.totals.tasks} ` +
      `backends=${index.totals.backends} waves=${index.totals.waves} ` +
      `files_touched=${index.totals.files} commits=${index.totals.commits} ` +
      `total_duration_ms=${index.totals.total_duration_ms}`,
  );
  if (index.bottleneck_tags.length > 0) {
    lines.push(`  corpus tags: ${index.bottleneck_tags.join(', ')}`);
  }
  lines.push('');
  lines.push('per wave:');
  const waveIds = Object.keys(index.by_wave);
  if (waveIds.length === 0) lines.push('  (no events)');
  for (const id of waveIds) {
    const w = index.by_wave[id];
    const tagStr = w.bottleneck_tags.length > 0 ? ` tags=[${w.bottleneck_tags.join(',')}]` : '';
    lines.push(
      `  - ${id}: events=${w.events} tasks_implied=${w.dispatch_count} ` +
        `cmds=${w.command_count} tests=${w.test_count} failures=${w.failure_count} ` +
        `retries=${w.retry_count} duration_ms=${w.total_duration_ms}${tagStr}`,
    );
  }
  lines.push('');
  lines.push('per task:');
  const taskIds = Object.keys(index.by_task);
  if (taskIds.length === 0) lines.push('  (no events)');
  for (const id of taskIds) {
    const t = index.by_task[id];
    const tagStr = t.bottleneck_tags.length > 0 ? ` tags=[${t.bottleneck_tags.join(',')}]` : '';
    lines.push(
      `  - ${id}: events=${t.events} cmds=${t.command_count} tests=${t.test_count} ` +
        `failures=${t.failure_count} retries=${t.retry_count} ` +
        `dispatch=${t.dispatch_count} complete=${t.complete_count} ` +
        `duration_ms=${t.total_duration_ms} files=${t.files_touched} ` +
        `commits=${t.commits.length}${tagStr}`,
    );
  }
  lines.push('');
  lines.push('per backend:');
  const backendIds = Object.keys(index.by_backend);
  if (backendIds.length === 0) lines.push('  (no events)');
  for (const id of backendIds) {
    const b = index.by_backend[id];
    lines.push(
      `  - ${id}: events=${b.events} cmds=${b.command_count} tests=${b.test_count} ` +
        `failures=${b.failure_count} retries=${b.retry_count} ` +
        `duration_ms=${b.total_duration_ms} files=${b.files_touched} ` +
        `commits=${b.commits.length}`,
    );
  }
  lines.push('');
  lines.push(
    `thresholds: long_running_ms=${index.thresholds.long_running_ms} ` +
      `high_retry=${index.thresholds.high_retry} ` +
      `many_failures=${index.thresholds.many_failures}`,
  );
  console.log(lines.join('\n'));
}

// ---------------------------------------------------------------------------
// Self-contained dry fixtures. Each case writes temporary trace files into a
// short-lived tmp dir, runs parseTraceEvents+buildIndex over them, asserts on
// the structured index, then rm's the tmp dir. The production code path
// (build-session-trace-index.mjs invoked by the user) never writes files —
// the fixture writes are confined to tmp and visible only inside this
// function.
// ---------------------------------------------------------------------------

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'pass: clean single-task trace',
      sources: { 'wave-clean.lisp': cleanTrace() },
      assert: (index) => {
        mustEqual('totals.events', index.totals.events, 5);
        mustEqual('totals.tasks', index.totals.tasks, 1);
        mustEqual('totals.backends', index.totals.backends, 1);
        mustEqual('totals.waves', index.totals.waves, 1);
        mustEqual('totals.commits', index.totals.commits, 1);
        const t = index.by_task['waveTest-fixture-clean'];
        if (!t) throw new Error('expected task waveTest-fixture-clean');
        mustEqual('task.bottleneck_tags.length', t.bottleneck_tags.length, 0);
        mustEqual(
          'index.bottleneck_tags.length',
          index.bottleneck_tags.length,
          0,
        );
      },
    },
    {
      name: 'pass: multi-wave aggregation groups by wave prefix',
      sources: {
        'wave22.lisp': wave22Trace(),
        'wave23.lisp': wave23Trace(),
      },
      assert: (index) => {
        mustEqual('totals.waves', index.totals.waves, 2);
        const waves = Object.keys(index.by_wave).sort();
        if (waves.join(',') !== 'wave22,wave23') {
          throw new Error(`expected wave22,wave23 got ${waves.join(',')}`);
        }
        // Per-wave counts roll up per-task counts inside the wave.
        const w22 = index.by_wave.wave22;
        const w23 = index.by_wave.wave23;
        if (w22.events < 1) throw new Error('wave22 events should be >=1');
        if (w23.events < 1) throw new Error('wave23 events should be >=1');
      },
    },
    {
      name: 'pass: bottleneck-tagged trace surfaces all four tag rules',
      sources: { 'wave-tagged.lisp': bottleneckTaggedTrace() },
      assert: (index) => {
        const t = index.by_task['waveTest-fixture-flaky'];
        const tags = new Set(t.bottleneck_tags);
        for (const required of ['long-running', 'high-retry', 'many-failures']) {
          if (!tags.has(required)) {
            throw new Error(`expected task tag ${required}, got ${[...tags].join(',')}`);
          }
        }
        // Corpus-level tag set unions across tasks.
        const corpus = new Set(index.bottleneck_tags);
        for (const required of ['long-running', 'high-retry', 'many-failures']) {
          if (!corpus.has(required)) {
            throw new Error(
              `expected corpus tag ${required}, got ${[...corpus].join(',')}`,
            );
          }
        }
      },
    },
    {
      name: 'pass: dispatched-but-no-completion fires no-completion tag',
      sources: { 'wave-stalled.lisp': stalledTrace() },
      assert: (index) => {
        const t = index.by_task['waveTest-fixture-stalled'];
        const tags = new Set(t.bottleneck_tags);
        if (!tags.has('no-completion')) {
          throw new Error('expected no-completion tag on stalled task');
        }
        if (!new Set(index.bottleneck_tags).has('no-completion')) {
          throw new Error('expected no-completion in corpus tags');
        }
      },
    },
    {
      name: 'pass: multi-backend aggregation tracks each backend separately',
      sources: { 'wave-multi-backend.lisp': multiBackendTrace() },
      assert: (index) => {
        mustEqual('totals.backends', index.totals.backends, 3);
        const backends = Object.keys(index.by_backend).sort();
        if (backends.join(',') !== 'claudecode,codex-orchestrator,patch-worker') {
          throw new Error(
            `expected three backends in sorted order, got ${backends.join(',')}`,
          );
        }
        // Each backend should report at least one event in this fixture.
        for (const b of backends) {
          if (index.by_backend[b].events < 1) {
            throw new Error(`backend ${b} expected events>=1`);
          }
        }
        // Backend buckets must NOT carry bottleneck_tags (we never tag backends).
        for (const b of backends) {
          if (index.by_backend[b].bottleneck_tags.length !== 0) {
            throw new Error(
              `backend ${b} unexpectedly tagged: ${index.by_backend[b].bottleneck_tags.join(',')}`,
            );
          }
        }
      },
    },
    {
      name: 'pass: empty corpus emits zero counts and stable shape',
      sources: {},
      assert: (index) => {
        mustEqual('totals.events', index.totals.events, 0);
        mustEqual('totals.tasks', index.totals.tasks, 0);
        mustEqual('totals.backends', index.totals.backends, 0);
        mustEqual('totals.waves', index.totals.waves, 0);
        mustEqual('totals.traces', index.totals.traces, 0);
        mustEqual('source_files.length', index.source_files.length, 0);
        mustEqual('bottleneck_tags.length', index.bottleneck_tags.length, 0);
        // The skeleton keys must always be present even when empty.
        for (const key of [
          'bottleneck_tags',
          'by_backend',
          'by_task',
          'by_wave',
          'schema',
          'source_files',
          'thresholds',
          'totals',
        ]) {
          if (!Object.prototype.hasOwnProperty.call(index, key)) {
            throw new Error(`missing top-level key ${key} in empty corpus`);
          }
        }
      },
    },
    {
      name: 'pass: stable JSON ordering is deterministic across runs',
      sources: {
        'wave-a.lisp': cleanTrace(),
        'wave-b.lisp': bottleneckTaggedTrace(),
      },
      assert: (index) => {
        const a = stableStringify(index);
        const b = stableStringify(index);
        if (a !== b) throw new Error('stableStringify not idempotent');
        // Re-build from the same traces and compare.
        const reBuilt = buildIndex(JSON.parse(JSON.stringify([])));
        if (typeof reBuilt !== 'object') {
          throw new Error('buildIndex must return an object even on empty input');
        }
        // Top-level keys appear in alphabetical order in stable output.
        const parsed = JSON.parse(a);
        const keys = Object.keys(parsed);
        const sorted = [...keys].sort();
        if (keys.join(',') !== sorted.join(',')) {
          throw new Error(`top-level keys not sorted: ${keys.join(',')}`);
        }
      },
    },
  ];

  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'trace-corpus-'));
  let failed = 0;
  try {
    for (const fixture of fixtures) {
      const files = [];
      for (const [name, src] of Object.entries(fixture.sources)) {
        const p = path.join(tmpRoot, `${slugify(fixture.name)}-${name}`);
        fs.writeFileSync(p, src, 'utf8');
        files.push(p);
      }
      const traces = [];
      for (const f of files) {
        for (const t of parseTraceEvents(f)) traces.push(t);
      }
      const index = buildIndex(traces);
      try {
        fixture.assert(index);
      } catch (err) {
        failed += 1;
        console.error(`fixture failed: ${fixture.name}`);
        console.error(`  ${err.message}`);
      }
    }
  } finally {
    fs.rmSync(tmpRoot, { recursive: true, force: true });
  }

  const categories = [
    'pass-clean',
    'pass-multi-wave',
    'pass-bottleneck-tags',
    'pass-no-completion',
    'pass-multi-backend',
    'pass-empty',
    'pass-deterministic',
  ];
  if (json) {
    console.log(
      stableStringify({
        ok: failed === 0,
        fixtures: fixtures.length,
        failed,
        categories,
      }),
    );
  }
  if (failed > 0) {
    console.error(
      `build-session-trace-index fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `build-session-trace-index fixtures OK (${fixtures.length} cases, ${categories.length} categories)`,
    );
  }
}

function mustEqual(label, actual, expected) {
  if (actual !== expected) {
    throw new Error(
      `assert ${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  }
}

function slugify(name) {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60);
}

// ---- fixture trace generators (pure string templates) -------------------

function cleanTrace() {
  return `(session-trace waveTest
    :schema "missiond.session-trace.v1"
    :wave waveTest
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wavetest-clean-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task waveTest-fixture-clean
      :backend claudecode
      :kind dispatch
      :summary "dispatch")
    (trace-event
      :id wavetest-clean-002
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task waveTest-fixture-clean
      :backend claudecode
      :kind read
      :summary "read"
      :files ["scripts/check-session-trace.mjs"])
    (trace-event
      :id wavetest-clean-003
      :seq 3
      :at "2026-04-28T00:00:02Z"
      :task waveTest-fixture-clean
      :backend claudecode
      :kind command
      :summary "cmd"
      :duration_ms 1500)
    (trace-event
      :id wavetest-clean-004
      :seq 4
      :at "2026-04-28T00:00:03Z"
      :task waveTest-fixture-clean
      :backend claudecode
      :kind test
      :summary "test"
      :duration_ms 800)
    (trace-event
      :id wavetest-clean-005
      :seq 5
      :at "2026-04-28T00:00:04Z"
      :task waveTest-fixture-clean
      :backend claudecode
      :kind complete
      :summary "done"
      :commit_hash "deadbeef01"))`;
}

function wave22Trace() {
  return `(session-trace wave22
    :schema "missiond.session-trace.v1"
    :wave wave22
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wave22-trace-aaa
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task wave22-01-example
      :backend claudecode
      :kind start
      :summary "begin")
    (trace-event
      :id wave22-trace-bbb
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task wave22-01-example
      :backend claudecode
      :kind complete
      :summary "end"))`;
}

function wave23Trace() {
  return `(session-trace wave23
    :schema "missiond.session-trace.v1"
    :wave wave23
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wave23-trace-aaa
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task wave23-02-example
      :backend codex-orchestrator
      :kind dispatch
      :summary "dispatch")
    (trace-event
      :id wave23-trace-bbb
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task wave23-02-example
      :backend codex-orchestrator
      :kind complete
      :summary "done"))`;
}

function bottleneckTaggedTrace() {
  // 4x500_000ms = 2_000_000ms (>=1.8e6 long-running),
  // 3 retries (>=3 high-retry), 2 failures (>=2 many-failures)
  return `(session-trace waveTest
    :schema "missiond.session-trace.v1"
    :wave waveTest
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wavetest-flaky-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind command
      :summary "long 1"
      :duration_ms 500000)
    (trace-event
      :id wavetest-flaky-002
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind command
      :summary "long 2"
      :duration_ms 500000)
    (trace-event
      :id wavetest-flaky-003
      :seq 3
      :at "2026-04-28T00:00:02Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind command
      :summary "long 3"
      :duration_ms 500000)
    (trace-event
      :id wavetest-flaky-004
      :seq 4
      :at "2026-04-28T00:00:03Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind command
      :summary "long 4"
      :duration_ms 500000)
    (trace-event
      :id wavetest-flaky-005
      :seq 5
      :at "2026-04-28T00:00:04Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind failure
      :summary "fail 1")
    (trace-event
      :id wavetest-flaky-006
      :seq 6
      :at "2026-04-28T00:00:05Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind failure
      :summary "fail 2")
    (trace-event
      :id wavetest-flaky-007
      :seq 7
      :at "2026-04-28T00:00:06Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind retry
      :summary "retry 1")
    (trace-event
      :id wavetest-flaky-008
      :seq 8
      :at "2026-04-28T00:00:07Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind retry
      :summary "retry 2")
    (trace-event
      :id wavetest-flaky-009
      :seq 9
      :at "2026-04-28T00:00:08Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind retry
      :summary "retry 3")
    (trace-event
      :id wavetest-flaky-010
      :seq 10
      :at "2026-04-28T00:00:09Z"
      :task waveTest-fixture-flaky
      :backend codex-orchestrator
      :kind complete
      :summary "eventually done"))`;
}

function stalledTrace() {
  return `(session-trace waveTest
    :schema "missiond.session-trace.v1"
    :wave waveTest
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wavetest-stalled-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task waveTest-fixture-stalled
      :backend claudecode
      :kind dispatch
      :summary "dispatched but never completed"))`;
}

function multiBackendTrace() {
  return `(session-trace waveTest
    :schema "missiond.session-trace.v1"
    :wave waveTest
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wavetest-mb-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task waveTest-fixture-mb
      :backend claudecode
      :kind start
      :summary "claudecode start")
    (trace-event
      :id wavetest-mb-002
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task waveTest-fixture-mb
      :backend codex-orchestrator
      :kind dispatch
      :summary "codex dispatch")
    (trace-event
      :id wavetest-mb-003
      :seq 3
      :at "2026-04-28T00:00:02Z"
      :task waveTest-fixture-mb
      :backend patch-worker
      :kind command
      :summary "patch-worker cmd")
    (trace-event
      :id wavetest-mb-004
      :seq 4
      :at "2026-04-28T00:00:03Z"
      :task waveTest-fixture-mb
      :backend claudecode
      :kind complete
      :summary "done"))`;
}

// Only run the CLI when invoked directly; keep helpers importable for tooling.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
