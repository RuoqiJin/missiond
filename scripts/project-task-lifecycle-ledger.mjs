#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  readLifecycleEventLogs,
  validateLifecycleEventFiles,
} from './check-task-lifecycle-events.mjs';

const usage = `Usage:
  node scripts/project-task-lifecycle-ledger.mjs --ledger <task-lifecycle-events.lisp>
    [--shared-memory <shared-memory.lisp>] [--session-trace <session-trace.lisp>]
    [--dry-run] [--json] [--dry-fixture]

Projects lifecycle events into migration-compatible shared-memory and
session-trace entries. With output paths, new projected ids are appended
unless already present. Without output paths, projections are printed as JSON
when --json is supplied.
`;

function main() {
  const args = process.argv.slice(2);
  const opts = parseArgs(args);
  if (opts.help) {
    console.log(usage);
    process.exit(0);
  }
  if (opts.dryFixture) {
    runFixtures();
    return;
  }
  if (!opts.ledgerPath) fail(usage);
  const result = projectLifecycleLedgerFile({
    ledgerPath: path.resolve(process.cwd(), opts.ledgerPath),
    sharedMemoryPath: opts.sharedMemoryPath ? path.resolve(process.cwd(), opts.sharedMemoryPath) : null,
    sessionTracePath: opts.sessionTracePath ? path.resolve(process.cwd(), opts.sessionTracePath) : null,
    dryRun: opts.dryRun,
  });
  if (opts.json) {
    console.log(JSON.stringify({ ok: true, ...result }, null, 2));
  } else {
    console.log(
      `project-task-lifecycle-ledger OK (${result.events} events, shared ${result.shared_memory_appended}, trace ${result.session_trace_appended}${opts.dryRun ? ', dry-run' : ''})`,
    );
  }
}

function parseArgs(args) {
  const opts = { dryRun: false };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') opts.help = true;
    else if (arg === '--dry-fixture') opts.dryFixture = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--dry-run') opts.dryRun = true;
    else if (arg === '--ledger') opts.ledgerPath = needValue(args, ++i, arg);
    else if (arg === '--shared-memory') opts.sharedMemoryPath = needValue(args, ++i, arg);
    else if (arg === '--session-trace') opts.sessionTracePath = needValue(args, ++i, arg);
    else fail(`unknown argument: ${arg}\n\n${usage}`);
  }
  return opts;
}

function needValue(args, index, flag) {
  const value = args[index];
  if (!value) fail(`${flag} requires a value`);
  return value;
}

export function projectLifecycleLedgerFile({
  ledgerPath,
  sharedMemoryPath = null,
  sessionTracePath = null,
  dryRun = false,
}) {
  const check = validateLifecycleEventFiles([ledgerPath]);
  if (!check.ok) {
    throw new Error(`lifecycle ledger failed validation:\n${check.diagnostics.map((d) => `  - ${d.message}`).join('\n')}`);
  }
  const logs = readLifecycleEventLogs(ledgerPath);
  if (logs.length !== 1) throw new Error(`expected exactly one lifecycle ledger in ${ledgerPath}, found ${logs.length}`);
  const log = logs[0];
  const projections = projectLifecycleEvents(log.events, { wave: log.wave });
  let sharedAppended = 0;
  let traceAppended = 0;
  if (sharedMemoryPath) {
    sharedAppended = appendProjectedEntries({
      filePath: sharedMemoryPath,
      entries: projections.sharedMemoryEntries,
      render: renderSharedMemoryEntry,
      idPrefix: ':id',
      dryRun,
    });
  }
  if (sessionTracePath) {
    traceAppended = appendProjectedEntries({
      filePath: sessionTracePath,
      entries: projections.sessionTraceEvents,
      render: renderSessionTraceEvent,
      idPrefix: ':id',
      dryRun,
    });
  }
  return {
    wave: log.wave,
    events: log.events.length,
    shared_memory_projected: projections.sharedMemoryEntries.length,
    session_trace_projected: projections.sessionTraceEvents.length,
    shared_memory_appended: sharedAppended,
    session_trace_appended: traceAppended,
    shared_memory_entries: projections.sharedMemoryEntries,
    session_trace_events: projections.sessionTraceEvents,
  };
}

export function projectLifecycleEvents(events, { wave }) {
  const sharedMemoryEntries = [];
  const sessionTraceEvents = [];
  for (const event of events) {
    if (event.event_kind === 'dispatch') {
      sharedMemoryEntries.push(memoryEntry(event, 'observation'));
      sessionTraceEvents.push(traceEvent(event, 'dispatch'));
    } else if (event.event_kind === 'claim') {
      sharedMemoryEntries.push(memoryEntry(event, 'claim'));
      sessionTraceEvents.push(traceEvent(event, 'start'));
    } else if (event.event_kind === 'trace_start') {
      sharedMemoryEntries.push(memoryEntry(event, 'observation'));
      sessionTraceEvents.push(traceEvent(event, 'start'));
    } else if (event.event_kind === 'read') {
      sessionTraceEvents.push(traceEvent(event, 'read'));
    } else if (event.event_kind === 'worker_commit') {
      sessionTraceEvents.push(traceEvent(event, 'commit'));
    } else if (event.event_kind === 'parent_hotfix') {
      sharedMemoryEntries.push(memoryEntry(event, 'observation'));
      sessionTraceEvents.push(traceEvent(event, event.commit_hash ? 'commit' : 'observation'));
    } else if (event.event_kind === 'finalized_report') {
      sharedMemoryEntries.push(memoryEntry(event, 'observation'));
      sessionTraceEvents.push(traceEvent(event, 'observation'));
    } else if (event.event_kind === 'receipt') {
      sharedMemoryEntries.push(memoryEntry(event, 'observation'));
      sessionTraceEvents.push(traceEvent(event, 'test'));
    } else if (event.event_kind === 'completion') {
      sharedMemoryEntries.push(memoryEntry(event, 'completion'));
      sessionTraceEvents.push(traceEvent(event, 'complete'));
    } else if (event.event_kind === 'cancelled') {
      sharedMemoryEntries.push(memoryEntry(event, 'observation'));
      sessionTraceEvents.push(traceEvent(event, 'failure'));
    } else if (event.event_kind === 'issue') {
      sharedMemoryEntries.push(memoryEntry(event, 'blocker'));
      sessionTraceEvents.push(traceEvent(event, 'failure'));
    }
  }
  return { wave, sharedMemoryEntries, sessionTraceEvents };
}

function memoryEntry(event, head) {
  return {
    head,
    id: event.legacy_memory_id ?? `${event.id}-memory`,
    task: event.task,
    agent: event.actor_role,
    at: event.at,
    touched: event.touched ?? [],
    summary: event.legacy_memory_summary ?? event.summary,
    refs: event.refs ?? [],
  };
}

function traceEvent(event, kind) {
  const out = {
    id: event.legacy_trace_id ?? `${event.id}-trace`,
    task: event.task,
    backend: event.actor_role,
    kind,
    at: event.at,
    files: event.legacy_trace_files ?? event.touched ?? [],
    summary: event.legacy_trace_summary ?? event.summary,
    commit_hash: event.commit_hash,
    report_path: event.report_path,
    trace_refs: event.refs ?? [],
  };
  return out;
}

export function renderSharedMemoryEntry(entry, seq) {
  const lines = [
    `  (${entry.head}`,
    `    :id ${entry.id}`,
    `    :task ${entry.task}`,
    `    :agent ${entry.agent}`,
    `    :seq ${seq}`,
    `    :at "${entry.at}"`,
    `    :touched ${renderStringVector(entry.touched)}`,
    `    :summary ${quoteString(entry.summary)}`,
  ];
  if (entry.refs?.length) lines.push(`    :refs ${renderAtomVector(entry.refs)}`);
  lines[lines.length - 1] += ')';
  return lines.join('\n');
}

export function renderSessionTraceEvent(entry, seq) {
  const lines = [
    `  (trace-event`,
    `    :id ${entry.id}`,
    `    :seq ${seq}`,
    `    :at "${entry.at}"`,
    `    :task ${entry.task}`,
    `    :backend ${entry.backend}`,
    `    :kind ${entry.kind}`,
  ];
  if (entry.files?.length) lines.push(`    :files ${renderStringVector(entry.files)}`);
  if (entry.commit_hash) lines.push(`    :commit_hash ${entry.commit_hash}`);
  if (entry.report_path) lines.push(`    :report_path "${entry.report_path}"`);
  if (entry.trace_refs?.length) lines.push(`    :trace_refs ${renderAtomVector(entry.trace_refs)}`);
  lines.push(`    :summary ${quoteString(entry.summary)}`);
  lines[lines.length - 1] += ')';
  return lines.join('\n');
}

function appendProjectedEntries({ filePath, entries, render, dryRun }) {
  if (!fs.existsSync(filePath)) throw new Error(`projection target does not exist: ${filePath}`);
  const body = fs.readFileSync(filePath, 'utf8');
  const existingIds = scanIds(body);
  let seq = scanMaxSeq(body);
  const blocks = [];
  for (const entry of entries) {
    if (existingIds.has(entry.id)) continue;
    seq += 1;
    blocks.push(render(entry, seq));
  }
  if (!dryRun && blocks.length > 0) {
    fs.writeFileSync(filePath, spliceBeforeFinalParen(body, `\n\n${blocks.join('\n\n')}`));
  }
  return blocks.length;
}

function scanIds(body) {
  const ids = new Set();
  const re = /:id\s+([a-z0-9][a-z0-9._-]*)\b/g;
  let match;
  while ((match = re.exec(body))) ids.add(match[1]);
  return ids;
}

function scanMaxSeq(body) {
  const re = /:seq\s+(-?\d+)\b/g;
  let max = 0;
  let match;
  while ((match = re.exec(body))) {
    const seq = Number.parseInt(match[1], 10);
    if (Number.isInteger(seq) && seq > max) max = seq;
  }
  return max;
}

function spliceBeforeFinalParen(body, block) {
  let i = body.length - 1;
  while (i >= 0 && /\s/.test(body[i])) i -= 1;
  if (i < 0 || body[i] !== ')') throw new Error('projection target is not terminated by a close paren');
  return `${body.slice(0, i)}${block}${body.slice(i)}\n`.replace(/\n+$/, '\n');
}

function renderStringVector(values) {
  if (!values || values.length === 0) return '[]';
  return `[${values.map((value) => quoteString(value)).join(' ')}]`;
}

function renderAtomVector(values) {
  if (!values || values.length === 0) return '[]';
  return `[${values.join(' ')}]`;
}

function quoteString(value) {
  return JSON.stringify(String(value ?? ''));
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-lifecycle-project-'));
  try {
    const ledgerPath = path.join(tmp, 'task-lifecycle-events.lisp');
    const sharedPath = path.join(tmp, 'shared-memory.lisp');
    const tracePath = path.join(tmp, 'session-trace.lisp');
    fs.writeFileSync(
      ledgerPath,
      `(task-lifecycle-event-log wave99-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave99
  :created-at "2026-04-28T00:00:00Z"
  :sequence 5

  (lifecycle-event
    :id wave99-01-dispatch-001
    :task wave99-01-demo
    :actor_role orchestrator
    :event_kind dispatch
    :commit_role none
    :seq 1
    :at "2026-04-28T00:00:01Z"
    :touched [".missiond/claudecode/wave99-01-demo.md"]
    :summary "dispatched task")

  (lifecycle-event
    :id wave99-01-claim-002
    :task wave99-01-demo
    :actor_role worker
    :event_kind claim
    :commit_role none
    :seq 2
    :at "2026-04-28T00:00:02Z"
    :touched []
    :summary "claimed task")

  (lifecycle-event
    :id wave99-01-read-002
    :task wave99-01-demo
    :actor_role worker
    :event_kind read
    :commit_role none
    :seq 3
    :at "2026-04-28T00:00:03Z"
    :touched ["scripts/demo.mjs"]
    :summary "read files")

  (lifecycle-event
    :id wave99-01-commit-003
    :task wave99-01-demo
    :actor_role worker
    :event_kind worker_commit
    :commit_role worker
    :seq 4
    :at "2026-04-28T00:00:04Z"
    :touched ["scripts/demo.mjs"]
    :summary "worker commit"
    :commit_hash abcdef1)

  (lifecycle-event
    :id wave99-01-completion-004
    :task wave99-01-demo
    :actor_role worker
    :event_kind completion
    :commit_role none
    :seq 5
    :at "2026-04-28T00:00:05Z"
    :touched ["scripts/demo.mjs"]
    :summary "completed task"))
`,
    );
    fs.writeFileSync(
      sharedPath,
      `(shared-memory wave99
  :schema "missiond.shared-memory.v1"
  :wave wave99
  :created-at "2026-04-28T00:00:00Z"
  :sequence 0)
`,
    );
    fs.writeFileSync(
      tracePath,
      `(session-trace wave99
  :schema "missiond.session-trace.v1"
  :wave wave99
  :created-at "2026-04-28T00:00:00Z"
  :sequence 0)
`,
    );
    const result = projectLifecycleLedgerFile({
      ledgerPath,
      sharedMemoryPath: sharedPath,
      sessionTracePath: tracePath,
      dryRun: false,
    });
    if (result.shared_memory_appended !== 3) throw new Error(`expected 3 shared-memory appends, got ${result.shared_memory_appended}`);
    if (result.session_trace_appended !== 5) throw new Error(`expected 5 trace appends, got ${result.session_trace_appended}`);
    const shared = fs.readFileSync(sharedPath, 'utf8');
    const trace = fs.readFileSync(tracePath, 'utf8');
    if (!shared.includes('(claim') || !shared.includes('(completion')) {
      throw new Error('shared-memory projection missing claim/completion');
    }
    if (!trace.includes(':kind dispatch') || !trace.includes(':kind start') || !trace.includes(':kind commit') || !trace.includes(':kind complete')) {
      throw new Error('session-trace projection missing dispatch/start/commit/complete');
    }
    const second = projectLifecycleLedgerFile({
      ledgerPath,
      sharedMemoryPath: sharedPath,
      sessionTracePath: tracePath,
      dryRun: false,
    });
    if (second.shared_memory_appended !== 0 || second.session_trace_appended !== 0) {
      throw new Error('projection should skip ids already present on rerun');
    }
    console.log('project-task-lifecycle-ledger fixtures OK (3 cases)');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main();
  } catch (err) {
    console.error(err.stack || err.message);
    process.exit(1);
  }
}
