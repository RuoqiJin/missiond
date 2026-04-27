#!/usr/bin/env node

// Read-only analyzer for MissionD session-trace v1 ledgers.
//
// Aggregates factual telemetry recorded by `(trace-event ...)` entries:
//   - per-task and per-backend event-kind counts
//   - command/test counts, failure/retry counts
//   - total :duration_ms (when present)
//   - deduplicated set of repo-relative :files touched
//   - distinct :commit_hash values
//
// Bottleneck hint rules (strictly conservative — fired only from observed
// facts, never from inferred ClaudeCode reasoning or model-selection guesses):
//   - long-running       :total :duration_ms >= LONG_RUNNING_MS  (1.8e6 ms = 30 min)
//   - high-retry         :retry kind count    >= HIGH_RETRY        (3)
//   - many-failures      :failure kind count  >= MANY_FAILURES     (2)
//   - no-completion      task has >=1 dispatch event but 0 complete events
//
// Non-goals (explicit):
//   - does NOT execute or replay any traced command
//   - does NOT propose router policy / model replacements
//   - does NOT speculate about hidden agent reasoning
//   - does NOT mutate the trace file or any other path (stdout/stderr only)
//
// Usage:
//   node scripts/analyze-session-trace.mjs [--json] [--dry-fixture] <trace.lisp...>

import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';

import { parseLisp, isList, head } from './lib/missiond_lisp.mjs';
import {
  SCHEMA,
  KIND_VALUES,
  ENTRY_HEAD,
  parseTraceEvents,
} from './check-session-trace.mjs';

const usage = `Usage:
  node scripts/analyze-session-trace.mjs [--json] [--dry-fixture] <trace.lisp...>

Reads MissionD session-trace v1 files and emits a factual summary:
  - per-task and per-backend event counts
  - command count, test count, failure count, retry count
  - total :duration_ms when present
  - deduplicated :files set, distinct :commit_hash values
  - conservative bottleneck hints from observed facts only

Use --dry-fixture to run self-contained pass/edge fixtures.
Use --json to emit a structured object for tooling.
`;

// Bottleneck-hint thresholds — documented in module header above.
const LONG_RUNNING_MS = 1_800_000; // 30 minutes
const HIGH_RETRY = 3;
const MANY_FAILURES = 2;

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

  if (inputs.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const files = [...new Set(inputs.map((input) => path.resolve(cwd, input)))];

  const traces = [];
  for (const file of files) {
    for (const t of parseTraceEvents(file)) traces.push(t);
  }

  const summary = summarize(traces);
  emit(summary, json);
}

// Pure aggregator: takes parsed traces (output of parseTraceEvents) and
// returns the structured summary object. Exposed for tooling and fixtures.
export function summarize(traces) {
  const tasks = new Map();
  const backends = new Map();
  let totalEvents = 0;
  const filesAll = new Set();
  const commitsAll = new Set();
  const tracesMeta = traces.map((t) => ({
    file: t.file,
    wave: t.wave,
    schema: t.header.schema,
    events: t.events.length,
  }));

  for (const trace of traces) {
    for (const event of trace.events) {
      totalEvents += 1;
      // Per-task bucket
      if (event.task) bumpBucket(tasks, event.task, event);
      // Per-backend bucket
      if (event.backend) bumpBucket(backends, event.backend, event);
      // Global files set
      for (const f of event.files) filesAll.add(f);
      // Global distinct commits
      if (event.commit_hash) commitsAll.add(event.commit_hash);
    }
  }

  // Convert maps to plain objects with hint computation per task.
  const tasksOut = {};
  for (const [taskId, agg] of tasks) {
    tasksOut[taskId] = finalizeBucket(agg, /* withHints */ true);
  }
  const backendsOut = {};
  for (const [backendId, agg] of backends) {
    backendsOut[backendId] = finalizeBucket(agg, /* withHints */ false);
  }

  // Aggregate hints across tasks for top-level convenience.
  const aggregateHints = [];
  // Note: aggregate "long-running" is computed at task granularity only;
  // we expose the global total duration as a fact, not as a hint.
  const totalDuration = Object.values(tasksOut).reduce(
    (sum, t) => sum + (t.total_duration_ms ?? 0),
    0,
  );

  return {
    schema: SCHEMA,
    files_scanned: tracesMeta.length,
    traces: tracesMeta,
    totals: {
      events: totalEvents,
      tasks: tasks.size,
      backends: backends.size,
      files_touched: filesAll.size,
      commits: commitsAll.size,
      total_duration_ms: totalDuration,
    },
    files: [...filesAll].sort(),
    commits: [...commitsAll].sort(),
    by_task: tasksOut,
    by_backend: backendsOut,
    hints: aggregateHints, // reserved; per-task hints live in by_task[*].hints
    thresholds: {
      long_running_ms: LONG_RUNNING_MS,
      high_retry: HIGH_RETRY,
      many_failures: MANY_FAILURES,
    },
  };
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
  if (typeof event.duration_ms === 'number') {
    agg.total_duration_ms += event.duration_ms;
  }
  for (const f of event.files) agg._files.add(f);
  if (event.commit_hash) agg._commits.add(event.commit_hash);
}

function freshBucket() {
  const kinds = {};
  for (const k of KIND_VALUES) kinds[k] = 0;
  return {
    events: 0,
    kinds,
    command_count: 0,
    test_count: 0,
    failure_count: 0,
    retry_count: 0,
    dispatch_count: 0,
    complete_count: 0,
    total_duration_ms: 0,
    _files: new Set(),
    _commits: new Set(),
  };
}

function finalizeBucket(agg, withHints) {
  const out = {
    events: agg.events,
    kinds: agg.kinds,
    command_count: agg.command_count,
    test_count: agg.test_count,
    failure_count: agg.failure_count,
    retry_count: agg.retry_count,
    dispatch_count: agg.dispatch_count,
    complete_count: agg.complete_count,
    total_duration_ms: agg.total_duration_ms,
    files_touched: agg._files.size,
    files: [...agg._files].sort(),
    commits: [...agg._commits].sort(),
  };
  if (withHints) {
    out.hints = computeHints(out);
  }
  return out;
}

function computeHints(taskBucket) {
  const hints = [];
  if (taskBucket.total_duration_ms >= LONG_RUNNING_MS) {
    hints.push({
      rule: 'long-running',
      threshold_ms: LONG_RUNNING_MS,
      observed_ms: taskBucket.total_duration_ms,
    });
  }
  if (taskBucket.retry_count >= HIGH_RETRY) {
    hints.push({
      rule: 'high-retry',
      threshold: HIGH_RETRY,
      observed: taskBucket.retry_count,
    });
  }
  if (taskBucket.failure_count >= MANY_FAILURES) {
    hints.push({
      rule: 'many-failures',
      threshold: MANY_FAILURES,
      observed: taskBucket.failure_count,
    });
  }
  if (taskBucket.dispatch_count >= 1 && taskBucket.complete_count === 0) {
    hints.push({
      rule: 'no-completion',
      threshold: 'dispatch>=1 AND complete==0',
      observed: {
        dispatch: taskBucket.dispatch_count,
        complete: taskBucket.complete_count,
      },
    });
  }
  return hints;
}

function emit(summary, json) {
  if (json) {
    console.log(JSON.stringify(summary, null, 2));
    return;
  }
  // Human-readable rendering. Conservative: echo facts, then list any hints.
  const lines = [];
  lines.push(`session-trace summary (${summary.schema})`);
  lines.push(
    `  files_scanned=${summary.files_scanned} events=${summary.totals.events} ` +
      `tasks=${summary.totals.tasks} backends=${summary.totals.backends} ` +
      `files_touched=${summary.totals.files_touched} commits=${summary.totals.commits} ` +
      `total_duration_ms=${summary.totals.total_duration_ms}`,
  );
  lines.push('');
  lines.push('per task:');
  const taskIds = Object.keys(summary.by_task).sort();
  if (taskIds.length === 0) {
    lines.push('  (no events)');
  }
  for (const id of taskIds) {
    const b = summary.by_task[id];
    lines.push(
      `  - ${id}: events=${b.events} cmds=${b.command_count} tests=${b.test_count} ` +
        `failures=${b.failure_count} retries=${b.retry_count} ` +
        `dispatch=${b.dispatch_count} complete=${b.complete_count} ` +
        `duration_ms=${b.total_duration_ms} files=${b.files_touched} commits=${b.commits.length}`,
    );
    for (const h of b.hints) {
      const detail =
        h.rule === 'long-running'
          ? `observed=${h.observed_ms}ms threshold=${h.threshold_ms}ms`
          : h.rule === 'no-completion'
            ? `dispatch=${h.observed.dispatch} complete=${h.observed.complete}`
            : `observed=${h.observed} threshold=${h.threshold}`;
      lines.push(`      hint[${h.rule}]: ${detail}`);
    }
  }
  lines.push('');
  lines.push('per backend:');
  const backendIds = Object.keys(summary.by_backend).sort();
  if (backendIds.length === 0) {
    lines.push('  (no events)');
  }
  for (const id of backendIds) {
    const b = summary.by_backend[id];
    lines.push(
      `  - ${id}: events=${b.events} cmds=${b.command_count} tests=${b.test_count} ` +
        `failures=${b.failure_count} retries=${b.retry_count} ` +
        `duration_ms=${b.total_duration_ms} files=${b.files_touched} commits=${b.commits.length}`,
    );
  }
  lines.push('');
  lines.push(
    `thresholds: long_running_ms=${summary.thresholds.long_running_ms} ` +
      `high_retry=${summary.thresholds.high_retry} ` +
      `many_failures=${summary.thresholds.many_failures}`,
  );
  console.log(lines.join('\n'));
}

// ---------------------------------------------------------------------------
// Self-contained fixtures. Each case writes a temp .lisp file, runs the
// analyzer's pure summarize() over it, and asserts on the resulting summary.
// We do NOT shell out — fixtures live entirely inside the Node process.
// ---------------------------------------------------------------------------

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'clean trace with mixed kinds — counts, duration, files',
      sources: [
        cleanMixedTrace(),
      ],
      assert: (summary) => {
        const t = summary.by_task['wave23-fixture-clean'];
        if (!t) throw new Error('expected task bucket missing');
        mustEqual('events', t.events, 5);
        mustEqual('command_count', t.command_count, 1);
        mustEqual('test_count', t.test_count, 1);
        mustEqual('failure_count', t.failure_count, 0);
        mustEqual('retry_count', t.retry_count, 0);
        mustEqual('total_duration_ms', t.total_duration_ms, 1500 + 800);
        mustEqual('files_touched', t.files_touched, 2);
        mustEqual('commits', t.commits.length, 1);
        mustEqual('hints.length', t.hints.length, 0);
        mustEqual('totals.events', summary.totals.events, 5);
        mustEqual('totals.commits', summary.totals.commits, 1);
      },
    },
    {
      name: 'retries and failures — high-retry + many-failures hints fire',
      sources: [retriesAndFailuresTrace()],
      assert: (summary) => {
        const t = summary.by_task['wave23-fixture-flaky'];
        mustEqual('failure_count', t.failure_count, 2);
        mustEqual('retry_count', t.retry_count, 3);
        const ruleSet = new Set(t.hints.map((h) => h.rule));
        if (!ruleSet.has('high-retry')) throw new Error('expected high-retry hint');
        if (!ruleSet.has('many-failures'))
          throw new Error('expected many-failures hint');
      },
    },
    {
      name: 'dispatched but no completion — no-completion hint fires',
      sources: [dispatchOnlyTrace()],
      assert: (summary) => {
        const t = summary.by_task['wave23-fixture-stalled'];
        mustEqual('dispatch_count', t.dispatch_count, 1);
        mustEqual('complete_count', t.complete_count, 0);
        const ruleSet = new Set(t.hints.map((h) => h.rule));
        if (!ruleSet.has('no-completion'))
          throw new Error('expected no-completion hint');
      },
    },
    {
      name: 'long-running aggregate — long-running hint fires',
      sources: [longRunningTrace()],
      assert: (summary) => {
        const t = summary.by_task['wave23-fixture-slow'];
        // 4 commands at 500_000ms each = 2_000_000ms (> 1_800_000 threshold)
        mustEqual('total_duration_ms', t.total_duration_ms, 2_000_000);
        const ruleSet = new Set(t.hints.map((h) => h.rule));
        if (!ruleSet.has('long-running'))
          throw new Error('expected long-running hint');
      },
    },
    {
      name: 'multi-file aggregation correct',
      sources: [cleanMixedTrace(), retriesAndFailuresTrace()],
      assert: (summary) => {
        mustEqual('files_scanned', summary.files_scanned, 2);
        // Two distinct task buckets.
        const ids = Object.keys(summary.by_task).sort();
        const expect = ['wave23-fixture-clean', 'wave23-fixture-flaky'];
        if (ids.join(',') !== expect.join(','))
          throw new Error(
            `expected tasks ${expect.join(',')}, got ${ids.join(',')}`,
          );
        // Aggregate file set merges across files.
        if (!summary.files.includes('scripts/check-session-trace.mjs'))
          throw new Error('expected merged files set to contain shared path');
        // Backends merge across files (claudecode + codex-orchestrator).
        const backendIds = Object.keys(summary.by_backend).sort();
        if (backendIds.length < 2)
          throw new Error(
            `expected multiple backend buckets, got ${backendIds.join(',')}`,
          );
      },
    },
    {
      name: 'empty trace — graceful zero-counts output',
      sources: [emptyTrace()],
      assert: (summary) => {
        mustEqual('files_scanned', summary.files_scanned, 1);
        mustEqual('totals.events', summary.totals.events, 0);
        mustEqual('totals.tasks', summary.totals.tasks, 0);
        mustEqual('totals.backends', summary.totals.backends, 0);
        mustEqual('totals.files_touched', summary.totals.files_touched, 0);
        mustEqual('totals.commits', summary.totals.commits, 0);
        mustEqual(
          'totals.total_duration_ms',
          summary.totals.total_duration_ms,
          0,
        );
        mustEqual(
          'by_task',
          Object.keys(summary.by_task).length,
          0,
        );
      },
    },
  ];

  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'analyze-trace-'));
  let failed = 0;
  try {
    for (const fixture of fixtures) {
      const files = fixture.sources.map((src, idx) => {
        const p = path.join(tmpRoot, `${slugify(fixture.name)}-${idx}.lisp`);
        fs.writeFileSync(p, src, 'utf8');
        // Sanity: the parser inside check-session-trace.mjs must parse our
        // fixtures cleanly, otherwise the analyzer would mask bugs.
        try {
          parseLisp(src, p);
        } catch (err) {
          throw new Error(
            `fixture "${fixture.name}" source unparseable: ${err.message}`,
          );
        }
        return p;
      });

      const traces = [];
      for (const f of files) {
        for (const t of parseTraceEvents(f)) traces.push(t);
      }
      const summary = summarize(traces);
      try {
        fixture.assert(summary);
      } catch (err) {
        failed += 1;
        console.error(`fixture failed: ${fixture.name}`);
        console.error(`  ${err.message}`);
      }
    }
  } finally {
    fs.rmSync(tmpRoot, { recursive: true, force: true });
  }

  if (json) {
    console.log(
      JSON.stringify(
        { ok: failed === 0, fixtures: fixtures.length, failed },
        null,
        2,
      ),
    );
  }
  if (failed > 0) {
    console.error(
      `analyze-session-trace fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `analyze-session-trace fixtures OK (${fixtures.length} cases)`,
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

// ---- fixture trace generators (kept as plain string templates) ----------

function cleanMixedTrace() {
  return `(session-trace wave23
    :schema "missiond.session-trace.v1"
    :wave wave23
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wave23-fixture-clean-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task wave23-fixture-clean
      :backend claudecode
      :kind dispatch
      :summary "task dispatched")
    (trace-event
      :id wave23-fixture-clean-002
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task wave23-fixture-clean
      :backend claudecode
      :kind read
      :summary "read file"
      :files ["scripts/check-session-trace.mjs"])
    (trace-event
      :id wave23-fixture-clean-003
      :seq 3
      :at "2026-04-28T00:00:02Z"
      :task wave23-fixture-clean
      :backend claudecode
      :kind command
      :summary "ran command"
      :command "node scripts/check-session-trace.mjs --dry-fixture"
      :exit_code 0
      :duration_ms 1500
      :files ["scripts/check-session-trace.mjs"])
    (trace-event
      :id wave23-fixture-clean-004
      :seq 4
      :at "2026-04-28T00:00:03Z"
      :task wave23-fixture-clean
      :backend claudecode
      :kind test
      :summary "ran tests"
      :command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :duration_ms 800
      :files ["scripts/analyze-session-trace.mjs"])
    (trace-event
      :id wave23-fixture-clean-005
      :seq 5
      :at "2026-04-28T00:00:04Z"
      :task wave23-fixture-clean
      :backend claudecode
      :kind complete
      :summary "task done"
      :commit_hash "deadbeef"))`;
}

function retriesAndFailuresTrace() {
  return `(session-trace wave23
    :schema "missiond.session-trace.v1"
    :wave wave23
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wave23-fixture-flaky-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task wave23-fixture-flaky
      :backend codex-orchestrator
      :kind dispatch
      :summary "dispatched")
    (trace-event
      :id wave23-fixture-flaky-002
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task wave23-fixture-flaky
      :backend codex-orchestrator
      :kind failure
      :summary "first failure")
    (trace-event
      :id wave23-fixture-flaky-003
      :seq 3
      :at "2026-04-28T00:00:02Z"
      :task wave23-fixture-flaky
      :backend codex-orchestrator
      :kind retry
      :summary "first retry"
      :trace_refs [wave23-fixture-flaky-002])
    (trace-event
      :id wave23-fixture-flaky-004
      :seq 4
      :at "2026-04-28T00:00:03Z"
      :task wave23-fixture-flaky
      :backend codex-orchestrator
      :kind failure
      :summary "second failure")
    (trace-event
      :id wave23-fixture-flaky-005
      :seq 5
      :at "2026-04-28T00:00:04Z"
      :task wave23-fixture-flaky
      :backend codex-orchestrator
      :kind retry
      :summary "second retry"
      :trace_refs [wave23-fixture-flaky-004])
    (trace-event
      :id wave23-fixture-flaky-006
      :seq 6
      :at "2026-04-28T00:00:05Z"
      :task wave23-fixture-flaky
      :backend codex-orchestrator
      :kind retry
      :summary "third retry")
    (trace-event
      :id wave23-fixture-flaky-007
      :seq 7
      :at "2026-04-28T00:00:06Z"
      :task wave23-fixture-flaky
      :backend codex-orchestrator
      :kind complete
      :summary "eventually done"))`;
}

function dispatchOnlyTrace() {
  return `(session-trace wave23
    :schema "missiond.session-trace.v1"
    :wave wave23
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wave23-fixture-stalled-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task wave23-fixture-stalled
      :backend claudecode
      :kind dispatch
      :summary "dispatched but never finished")
    (trace-event
      :id wave23-fixture-stalled-002
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task wave23-fixture-stalled
      :backend claudecode
      :kind start
      :summary "agent acknowledged"))`;
}

function longRunningTrace() {
  // 4 command events of 500_000ms each = 2_000_000ms total (> 1.8e6 threshold)
  return `(session-trace wave23
    :schema "missiond.session-trace.v1"
    :wave wave23
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id wave23-fixture-slow-001
      :seq 1
      :at "2026-04-28T00:00:00Z"
      :task wave23-fixture-slow
      :backend claudecode
      :kind dispatch
      :summary "dispatched")
    (trace-event
      :id wave23-fixture-slow-002
      :seq 2
      :at "2026-04-28T00:00:01Z"
      :task wave23-fixture-slow
      :backend claudecode
      :kind command
      :summary "long step 1"
      :duration_ms 500000)
    (trace-event
      :id wave23-fixture-slow-003
      :seq 3
      :at "2026-04-28T00:00:02Z"
      :task wave23-fixture-slow
      :backend claudecode
      :kind command
      :summary "long step 2"
      :duration_ms 500000)
    (trace-event
      :id wave23-fixture-slow-004
      :seq 4
      :at "2026-04-28T00:00:03Z"
      :task wave23-fixture-slow
      :backend claudecode
      :kind command
      :summary "long step 3"
      :duration_ms 500000)
    (trace-event
      :id wave23-fixture-slow-005
      :seq 5
      :at "2026-04-28T00:00:04Z"
      :task wave23-fixture-slow
      :backend claudecode
      :kind command
      :summary "long step 4"
      :duration_ms 500000)
    (trace-event
      :id wave23-fixture-slow-006
      :seq 6
      :at "2026-04-28T00:00:05Z"
      :task wave23-fixture-slow
      :backend claudecode
      :kind complete
      :summary "finally done"))`;
}

function emptyTrace() {
  return `(session-trace wave23
    :schema "missiond.session-trace.v1"
    :wave wave23
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1)`;
}

// Only run the CLI when invoked directly; keep `summarize` importable.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
