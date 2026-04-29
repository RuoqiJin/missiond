#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  EVENT_KINDS,
  COMMIT_ROLES,
  ID_RE,
  REQUEST_EVENT_SCHEMA,
  readLifecycleEventLogs,
  renderLifecycleEvent,
  renderLifecycleEventLog,
  renderTaskScopedLifecycleEventFile,
  spliceBeforeFinalParen,
  validateLifecycleEventFiles,
} from './check-task-lifecycle-events.mjs';

const usage = `Usage:
  node scripts/task-runner-append-event.mjs
    [--ledger <task-lifecycle-events.lisp>] [--events-dir <task-events-dir>]
    --task <task-id> --kind <event-kind> --actor-role <role>
    [--commit-role <role>] [--commit-hash <sha>] [--summary <text>]
    [--touched <path[,path...]>] [--file <path>] [--report-path <path>]
    [--receipt-path <path>] [--ref <event-id>] [--id <event-id>]
    [--request-id <request-id> --request-events-dir <dir>]
    [--at <iso8601>] [--wave <wave-id>] [--json] [--dry-fixture]

Appends one lifecycle event using a cooperative sibling .lock file.

Modes:
  --ledger only          Legacy task-lifecycle-events.lisp ledger append.
  --events-dir only      Task-scoped one-event-per-file write into
                         .missiond/tasks/<wave>/events/<seq>.event.lisp.
  --ledger + --events-dir
                         Task-scoped event file is primary; the ledger is
                         updated as a compatibility projection with the
                         same allocated :seq.

Concurrency boundary:
  Ledger-only mode locks <ledger>.lock with O_EXCL, validates the candidate,
  writes a temp file, then atomically renames it over the ledger. Events-dir
  mode locks <events-dir>/.dir.lock, scans the directory for the next numeric
  sequence (also taking the legacy ledger into account when both flags are
  supplied), validates the standalone event bytes, then creates the final
  file via fs.openSync(file, 'wx') so two cooperating writers cannot
  overwrite the same numeric file. None of these locks protect against
  manual edits, tools that ignore the lock, stale locks on crashed
  processes until timeout, or filesystems that do not provide atomic
  create/rename semantics.
`;

function main() {
  const args = process.argv.slice(2);
  const opts = parseArgs(args);
  if (opts.help) {
    console.log(usage);
    process.exit(0);
  }
  if (opts.dryFixture) {
    runFixtures().then(
      () => {},
      (err) => {
        console.error(err.stack || err.message);
        process.exit(1);
      },
    );
    return;
  }
  if ((!opts.ledgerPath && !opts.eventsDir) || !opts.task || !opts.kind || !opts.actorRole) {
    fail(usage);
  }
  const result = appendLifecycleEvent({
    ledgerPath: opts.ledgerPath ? path.resolve(process.cwd(), opts.ledgerPath) : null,
    eventsDir: opts.eventsDir ? path.resolve(process.cwd(), opts.eventsDir) : null,
    task: opts.task,
    eventKind: opts.kind,
    actorRole: opts.actorRole,
    commitRole: opts.commitRole ?? 'none',
    commitHash: opts.commitHash,
    touched: opts.touched,
    summary: opts.summary ?? `${opts.kind} lifecycle event`,
    reportPath: opts.reportPath,
    receiptPath: opts.receiptPath,
    refs: opts.refs,
    id: opts.id,
    at: opts.at ?? isoNow(),
    wave: opts.wave,
    requestId: opts.requestId,
    requestEventsDir: opts.requestEventsDir
      ? path.resolve(process.cwd(), opts.requestEventsDir)
      : null,
  });
  if (opts.json) {
    console.log(JSON.stringify({
      ok: true,
      event: result.event,
      ledger_path: result.ledgerPath,
      events_dir: result.eventsDir ?? null,
      event_file: result.eventFile ?? null,
      request_event_path: result.requestEventPath ?? null,
    }, null, 2));
  } else {
    const parts = [];
    if (result.eventFile) parts.push(`event file ${result.eventFile}`);
    if (result.requestEventPath) parts.push(`request event ${result.requestEventPath}`);
    const suffix = parts.length > 0 ? `; ${parts.join('; ')}` : '';
    console.log(`task-runner-append-event OK (${result.event.id} seq ${result.event.seq}${suffix})`);
  }
}

function parseArgs(args) {
  const opts = { touched: [], refs: [] };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') opts.help = true;
    else if (arg === '--dry-fixture') opts.dryFixture = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--ledger') opts.ledgerPath = needValue(args, ++i, arg);
    else if (arg === '--events-dir') opts.eventsDir = needValue(args, ++i, arg);
    else if (arg === '--task') opts.task = needValue(args, ++i, arg);
    else if (arg === '--kind' || arg === '--event-kind') opts.kind = needValue(args, ++i, arg);
    else if (arg === '--actor-role') opts.actorRole = needValue(args, ++i, arg);
    else if (arg === '--commit-role') opts.commitRole = needValue(args, ++i, arg);
    else if (arg === '--commit-hash') opts.commitHash = needValue(args, ++i, arg);
    else if (arg === '--summary') opts.summary = needValue(args, ++i, arg);
    else if (arg === '--touched') opts.touched.push(...splitPaths(needValue(args, ++i, arg)));
    else if (arg === '--file') opts.touched.push(needValue(args, ++i, arg));
    else if (arg === '--report-path') opts.reportPath = needValue(args, ++i, arg);
    else if (arg === '--receipt-path') opts.receiptPath = needValue(args, ++i, arg);
    else if (arg === '--ref') opts.refs.push(needValue(args, ++i, arg));
    else if (arg === '--id') opts.id = needValue(args, ++i, arg);
    else if (arg === '--request-id') opts.requestId = needValue(args, ++i, arg);
    else if (arg === '--request-events-dir') opts.requestEventsDir = needValue(args, ++i, arg);
    else if (arg === '--at') opts.at = needValue(args, ++i, arg);
    else if (arg === '--wave') opts.wave = needValue(args, ++i, arg);
    else fail(`unknown argument: ${arg}\n\n${usage}`);
  }
  return opts;
}

function needValue(args, index, flag) {
  const value = args[index];
  if (!value) fail(`${flag} requires a value`);
  return value;
}

function splitPaths(value) {
  return value
    .split(',')
    .map((part) => part.trim())
    .filter(Boolean);
}

export function appendLifecycleEvent({
  ledgerPath = null,
  eventsDir = null,
  task,
  eventKind,
  actorRole,
  commitRole = 'none',
  commitHash = null,
  touched = [],
  summary,
  reportPath = null,
  receiptPath = null,
  refs = [],
  id = null,
  at = isoNow(),
  wave = null,
  legacyMemoryId = null,
  legacyTraceId = null,
  requestId = null,
  requestEventsDir = null,
  lockTimeoutMs = 5000,
  staleLockMs = 30000,
}) {
  validateAppendInput({ task, eventKind, actorRole, commitRole, id, requestId, requestEventsDir, ledgerPath, eventsDir });
  if (eventsDir) {
    return appendLifecycleEventToEventsDir({
      eventsDir,
      ledgerPath,
      task,
      eventKind,
      actorRole,
      commitRole,
      commitHash,
      touched,
      summary,
      reportPath,
      receiptPath,
      refs,
      id,
      at,
      wave,
      legacyMemoryId,
      legacyTraceId,
      requestId,
      requestEventsDir,
      lockTimeoutMs,
      staleLockMs,
    });
  }
  fs.mkdirSync(path.dirname(ledgerPath), { recursive: true });
  const lockPath = `${ledgerPath}.lock`;
  return withLedgerLock(lockPath, { timeoutMs: lockTimeoutMs, staleMs: staleLockMs }, () => {
    const resolvedWave = wave ?? inferWave(task);
    if (!fs.existsSync(ledgerPath)) {
      const createdAt = at;
      fs.writeFileSync(
        ledgerPath,
        renderLifecycleEventLog({
          id: `${resolvedWave}-lifecycle-events`,
          wave: resolvedWave,
          createdAt,
          sequence: 0,
          events: [],
        }),
      );
    }

    const logs = readLifecycleEventLogs(ledgerPath);
    if (logs.length !== 1) {
      throw new Error(`expected exactly one task lifecycle event log in ${ledgerPath}, found ${logs.length}`);
    }
    const log = logs[0];
    const seq = nextSeq(log.events);
    const event = buildLifecycleEvent({
      id, task, actorRole, eventKind, commitRole, seq, at, touched, summary, refs,
      commitHash, reportPath, receiptPath, legacyMemoryId, legacyTraceId,
    });

    const original = fs.readFileSync(ledgerPath, 'utf8');
    const withSequence = original.replace(/(:sequence\s+)\d+\b/, `$1${seq}`);
    const next = spliceBeforeFinalParen(withSequence, `\n\n${renderLifecycleEvent(event)}`);
    const tmp = path.join(path.dirname(ledgerPath), `.${path.basename(ledgerPath)}.${process.pid}.${Date.now()}.tmp`);
    fs.writeFileSync(tmp, next);
    const check = validateLifecycleEventFiles([tmp]);
    if (!check.ok) {
      fs.rmSync(tmp, { force: true });
      throw new Error(`candidate lifecycle ledger failed validation:\n${check.diagnostics.map((d) => `  - ${d.message}`).join('\n')}`);
    }
    fs.renameSync(tmp, ledgerPath);
    let requestEventPath = null;
    if (requestId && requestEventsDir) {
      requestEventPath = writeRequestLifecycleEventFile({
        eventsDir: requestEventsDir,
        requestId,
        event,
      });
    }
    return { ledgerPath, eventsDir: null, eventFile: null, event, requestEventPath };
  });
}

function buildLifecycleEvent({
  id, task, actorRole, eventKind, commitRole, seq, at, touched, summary, refs,
  commitHash, reportPath, receiptPath, legacyMemoryId, legacyTraceId,
}) {
  const event = {
    id: id ?? `${task}-${eventKind.replace(/_/g, '-')}-${String(seq).padStart(3, '0')}`,
    task,
    actor_role: actorRole,
    event_kind: eventKind,
    commit_role: commitRole,
    seq,
    at,
    touched,
    summary,
    refs,
  };
  if (commitHash) event.commit_hash = commitHash;
  if (reportPath) event.report_path = reportPath;
  if (receiptPath) event.receipt_path = receiptPath;
  if (legacyMemoryId) event.legacy_memory_id = legacyMemoryId;
  if (legacyTraceId) event.legacy_trace_id = legacyTraceId;
  return event;
}

export function appendLifecycleEventToEventsDir({
  eventsDir,
  ledgerPath = null,
  task,
  eventKind,
  actorRole,
  commitRole = 'none',
  commitHash = null,
  touched = [],
  summary,
  reportPath = null,
  receiptPath = null,
  refs = [],
  id = null,
  at = isoNow(),
  wave = null,
  legacyMemoryId = null,
  legacyTraceId = null,
  requestId = null,
  requestEventsDir = null,
  lockTimeoutMs = 5000,
  staleLockMs = 30000,
}) {
  fs.mkdirSync(eventsDir, { recursive: true });
  const dirLockPath = path.join(eventsDir, '.dir.lock');
  return withLedgerLock(dirLockPath, { timeoutMs: lockTimeoutMs, staleMs: staleLockMs }, () => {
    const resolvedWave = wave ?? inferWave(task);
    const dirMaxSeq = scanTaskScopedEventDirMaxSeq(eventsDir);
    let ledgerEvents = null;
    let ledgerMaxSeq = 0;
    if (ledgerPath && fs.existsSync(ledgerPath)) {
      const logs = readLifecycleEventLogs(ledgerPath);
      if (logs.length === 1) {
        ledgerEvents = logs[0].events;
        for (const e of ledgerEvents) {
          if (Number.isInteger(e.seq) && e.seq > ledgerMaxSeq) ledgerMaxSeq = e.seq;
        }
      }
    }
    const seq = Math.max(dirMaxSeq, ledgerMaxSeq) + 1;
    const event = buildLifecycleEvent({
      id, task, actorRole, eventKind, commitRole, seq, at, touched, summary, refs,
      commitHash, reportPath, receiptPath, legacyMemoryId, legacyTraceId,
    });

    const eventFile = path.join(eventsDir, `${String(seq).padStart(6, '0')}.event.lisp`);
    const standaloneSource = renderTaskScopedLifecycleEventFile({ wave: resolvedWave, event });
    const tmp = path.join(eventsDir, `.${path.basename(eventFile)}.${process.pid}.${Date.now()}.tmp`);
    fs.writeFileSync(tmp, standaloneSource);
    const check = validateLifecycleEventFiles([tmp]);
    if (!check.ok) {
      fs.rmSync(tmp, { force: true });
      throw new Error(`candidate task-scoped event file failed validation:\n${check.diagnostics.map((d) => `  - ${d.message}`).join('\n')}`);
    }
    fs.rmSync(tmp, { force: true });
    try {
      const fd = fs.openSync(eventFile, 'wx');
      try {
        fs.writeFileSync(fd, standaloneSource);
      } finally {
        fs.closeSync(fd);
      }
    } catch (err) {
      throw new Error(`failed to create task-scoped event file ${eventFile}: ${err.message}`);
    }

    let writtenLedger = null;
    if (ledgerPath) {
      writtenLedger = appendLedgerCompatProjection({
        ledgerPath,
        wave: resolvedWave,
        event,
        lockTimeoutMs,
        staleLockMs,
      });
    }

    let requestEventPath = null;
    if (requestId && requestEventsDir) {
      requestEventPath = writeRequestLifecycleEventFile({
        eventsDir: requestEventsDir,
        requestId,
        event,
      });
    }

    return {
      ledgerPath: writtenLedger,
      eventsDir,
      eventFile,
      event,
      requestEventPath,
    };
  });
}

function appendLedgerCompatProjection({ ledgerPath, wave, event, lockTimeoutMs, staleLockMs }) {
  fs.mkdirSync(path.dirname(ledgerPath), { recursive: true });
  const lockPath = `${ledgerPath}.lock`;
  return withLedgerLock(lockPath, { timeoutMs: lockTimeoutMs, staleMs: staleLockMs }, () => {
    if (!fs.existsSync(ledgerPath)) {
      fs.writeFileSync(
        ledgerPath,
        renderLifecycleEventLog({
          id: `${wave}-lifecycle-events`,
          wave,
          createdAt: event.at,
          sequence: 0,
          events: [],
        }),
      );
    }
    const original = fs.readFileSync(ledgerPath, 'utf8');
    const withSequence = original.replace(/(:sequence\s+)\d+\b/, `$1${event.seq}`);
    const next = spliceBeforeFinalParen(withSequence, `\n\n${renderLifecycleEvent(event)}`);
    const tmp = path.join(
      path.dirname(ledgerPath),
      `.${path.basename(ledgerPath)}.${process.pid}.${Date.now()}.tmp`,
    );
    fs.writeFileSync(tmp, next);
    const check = validateLifecycleEventFiles([tmp]);
    if (!check.ok) {
      fs.rmSync(tmp, { force: true });
      throw new Error(`candidate lifecycle ledger compat projection failed validation:\n${check.diagnostics.map((d) => `  - ${d.message}`).join('\n')}`);
    }
    fs.renameSync(tmp, ledgerPath);
    return ledgerPath;
  });
}

function scanTaskScopedEventDirMaxSeq(eventsDir) {
  if (!fs.existsSync(eventsDir)) return 0;
  let max = 0;
  for (const name of fs.readdirSync(eventsDir)) {
    const match = name.match(/^(\d+)\.event\.lisp$/);
    if (!match) continue;
    const value = Number.parseInt(match[1], 10);
    if (Number.isInteger(value) && value > max) max = value;
  }
  return max;
}

export function writeRequestLifecycleEventFile({ eventsDir, requestId, event }) {
  fs.mkdirSync(eventsDir, { recursive: true });
  for (let attempt = 0; attempt < 1000; attempt += 1) {
    const seq = nextRequestEventSeq(eventsDir);
    const file = path.join(eventsDir, `${String(seq).padStart(6, '0')}.event.lisp`);
    const tmp = path.join(eventsDir, `.${path.basename(file)}.${process.pid}.${Date.now()}.tmp`);
    const source = renderRequestLifecycleEventFile({ requestId, event });
    fs.writeFileSync(tmp, source);
    const check = validateLifecycleEventFiles([tmp]);
    if (!check.ok) {
      fs.rmSync(tmp, { force: true });
      throw new Error(`candidate request lifecycle event failed validation:\n${check.diagnostics.map((d) => `  - ${d.message}`).join('\n')}`);
    }
    try {
      const fd = fs.openSync(file, 'wx');
      try {
        fs.writeFileSync(fd, source);
      } finally {
        fs.closeSync(fd);
      }
      fs.rmSync(tmp, { force: true });
      return file;
    } catch (err) {
      fs.rmSync(tmp, { force: true });
      if (err.code === 'EEXIST') continue;
      throw err;
    }
  }
  throw new Error(`could not allocate request lifecycle event sequence under ${eventsDir}`);
}

export function renderRequestLifecycleEventFile({ requestId, event }) {
  const payload = [
    `:task ${event.task}`,
    `:task_seq ${event.seq}`,
    `:commit_role ${event.commit_role ?? 'none'}`,
    `:touched ${renderStringVector(event.touched ?? [])}`,
    `:summary ${quoteString(event.summary ?? '')}`,
  ];
  if (event.commit_hash) payload.push(`:commit_hash ${event.commit_hash}`);
  if (event.report_path) payload.push(`:report_path ${quoteString(event.report_path)}`);
  if (event.receipt_path) payload.push(`:receipt_path ${quoteString(event.receipt_path)}`);
  if (event.refs?.length) payload.push(`:refs ${renderAtomVector(event.refs)}`);
  if (event.legacy_memory_id) payload.push(`:legacy_memory_id ${event.legacy_memory_id}`);
  if (event.legacy_trace_id) payload.push(`:legacy_trace_id ${event.legacy_trace_id}`);
  return [
    '(lifecycle-event',
    `  :schema ${quoteString(REQUEST_EVENT_SCHEMA)}`,
    `  :event_id ${event.id}`,
    `  :request_id ${requestId}`,
    `  :kind ${event.event_kind}`,
    `  :actor ${event.actor_role}`,
    `  :time ${quoteString(event.at)}`,
    `  :payload (${payload.join(' ')})`,
    `  :idempotency_key ${quoteString(`${requestId}:${event.id}`)})`,
    '',
  ].join('\n');
}

export function buildLifecycleEventRecord({
  id,
  task,
  actorRole,
  eventKind,
  commitRole = 'none',
  seq,
  at,
  touched = [],
  summary,
  commitHash = null,
  reportPath = null,
  receiptPath = null,
  refs = [],
  legacyMemoryId = null,
  legacyTraceId = null,
  legacyMemorySummary = null,
  legacyTraceSummary = null,
  legacyTraceFiles = null,
}) {
  return {
    id,
    task,
    actor_role: actorRole,
    event_kind: eventKind,
    commit_role: commitRole,
    seq,
    at,
    touched,
    summary,
    commit_hash: commitHash,
    report_path: reportPath,
    receipt_path: receiptPath,
    refs,
    legacy_memory_id: legacyMemoryId,
    legacy_trace_id: legacyTraceId,
    legacy_memory_summary: legacyMemorySummary,
    legacy_trace_summary: legacyTraceSummary,
    legacy_trace_files: legacyTraceFiles,
  };
}

export function withLedgerLock(lockPath, { timeoutMs = 5000, staleMs = 30000 } = {}, fn) {
  const started = Date.now();
  let fd = null;
  while (fd == null) {
    try {
      fd = fs.openSync(lockPath, 'wx');
      fs.writeFileSync(fd, JSON.stringify({ pid: process.pid, created_at: isoNow() }));
    } catch (err) {
      if (err.code !== 'EEXIST') throw err;
      maybeRemoveStaleLock(lockPath, staleMs);
      if (Date.now() - started > timeoutMs) {
        throw new Error(`timed out waiting for lifecycle ledger lock: ${lockPath}`);
      }
      sleepSync(25);
    }
  }
  try {
    return fn();
  } finally {
    try {
      fs.closeSync(fd);
    } finally {
      fs.rmSync(lockPath, { force: true });
    }
  }
}

function maybeRemoveStaleLock(lockPath, staleMs) {
  try {
    const stat = fs.statSync(lockPath);
    if (Date.now() - stat.mtimeMs > staleMs) fs.rmSync(lockPath, { force: true });
  } catch (err) {
    if (err.code !== 'ENOENT') throw err;
  }
}

function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function validateAppendInput({ task, eventKind, actorRole, commitRole, id, requestId, requestEventsDir, ledgerPath, eventsDir }) {
  if (!ledgerPath && !eventsDir) {
    throw new Error('appendLifecycleEvent requires --ledger or --events-dir (or both)');
  }
  if (!ID_RE.test(task)) throw new Error(`task id "${task}" must match ${ID_RE}`);
  if (!EVENT_KINDS.has(eventKind)) throw new Error(`unknown event kind "${eventKind}"`);
  if (!ID_RE.test(actorRole)) throw new Error(`actor role "${actorRole}" must match ${ID_RE}`);
  if (!COMMIT_ROLES.has(commitRole)) throw new Error(`unknown commit role "${commitRole}"`);
  if (id && !ID_RE.test(id)) throw new Error(`event id "${id}" must match ${ID_RE}`);
  if ((requestId && !requestEventsDir) || (!requestId && requestEventsDir)) {
    throw new Error('--request-id and --request-events-dir must be supplied together');
  }
  if (requestId && !ID_RE.test(requestId)) throw new Error(`request id "${requestId}" must match ${ID_RE}`);
}

function inferWave(task) {
  const match = task.match(/^(wave[0-9]+)-/);
  if (match) return match[1];
  throw new Error(`cannot infer wave from task id "${task}"; pass --wave`);
}

function nextSeq(events) {
  let max = 0;
  for (const event of events) {
    if (Number.isInteger(event.seq) && event.seq > max) max = event.seq;
  }
  return max + 1;
}

function nextRequestEventSeq(eventsDir) {
  let max = 0;
  if (!fs.existsSync(eventsDir)) return 1;
  for (const name of fs.readdirSync(eventsDir)) {
    const match = name.match(/^(\d+)\.event\.lisp$/);
    if (!match) continue;
    max = Math.max(max, Number.parseInt(match[1], 10));
  }
  return max + 1;
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
  return JSON.stringify(String(value));
}

function isoNow() {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

async function runFixtures() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-lifecycle-append-'));
  try {
    const ledger = path.join(tmp, 'task-lifecycle-events.lisp');
    const first = appendLifecycleEvent({
      ledgerPath: ledger,
      task: 'wave99-01-demo',
      eventKind: 'claim',
      actorRole: 'worker',
      summary: 'claimed task',
      at: '2026-04-28T00:00:01Z',
      wave: 'wave99',
    });
    if (first.event.seq !== 1) throw new Error(`first seq should be 1, got ${first.event.seq}`);
    const second = appendLifecycleEvent({
      ledgerPath: ledger,
      task: 'wave99-01-demo',
      eventKind: 'read',
      actorRole: 'worker',
      summary: 'read files',
      touched: ['scripts/demo.mjs'],
      at: '2026-04-28T00:00:02Z',
      wave: 'wave99',
    });
    if (second.event.seq !== 2) throw new Error(`second seq should be 2, got ${second.event.seq}`);

    const requestEventsDir = path.join(tmp, '.missiond/requests/req-wave99-01/events');
    const projected = appendLifecycleEvent({
      ledgerPath: ledger,
      task: 'wave99-01-demo',
      eventKind: 'receipt',
      actorRole: 'verifier',
      commitRole: 'receipt',
      summary: 'receipt recorded',
      receiptPath: '.missiond/tasks/wave99/receipts/wave99-01.receipt.lisp',
      at: '2026-04-28T00:00:03Z',
      wave: 'wave99',
      requestId: 'req-wave99-01',
      requestEventsDir,
    });
    if (!projected.requestEventPath?.endsWith('000001.event.lisp')) {
      throw new Error(`request-local projection should allocate 000001.event.lisp, got ${projected.requestEventPath}`);
    }
    const projectedCheck = validateLifecycleEventFiles([projected.requestEventPath]);
    if (!projectedCheck.ok || projectedCheck.request_events !== 1) {
      throw new Error(`projected request event failed validation: ${projectedCheck.diagnostics.map((d) => d.message).join('; ')}`);
    }

    const childLedger = path.join(tmp, 'concurrent-lifecycle-events.lisp');
    fs.writeFileSync(
      childLedger,
      renderLifecycleEventLog({
        id: 'wave99-lifecycle-events',
        wave: 'wave99',
        createdAt: '2026-04-28T00:00:00Z',
        sequence: 0,
        events: [],
      }),
    );
    const scriptPath = fileURLToPath(import.meta.url);
    await Promise.all(
      [1, 2, 3, 4].map((n) =>
        runChild(process.execPath, [
          scriptPath,
          '--ledger',
          childLedger,
          '--task',
          'wave99-02-demo',
          '--kind',
          'read',
          '--actor-role',
          'worker',
          '--summary',
          `concurrent append ${n}`,
          '--touched',
          `scripts/demo-${n}.mjs`,
          '--at',
          `2026-04-28T00:00:0${n}Z`,
        ]),
      ),
    );
    const logs = readLifecycleEventLogs(childLedger);
    const seqs = logs[0].events.map((event) => event.seq);
    const ids = new Set(logs[0].events.map((event) => event.id));
    if (seqs.join(',') !== '1,2,3,4') {
      throw new Error(`concurrent child appends should allocate seq 1..4, got ${seqs.join(',')}`);
    }
    if (ids.size !== 4) throw new Error('concurrent child appends produced duplicate ids');
    const check = validateLifecycleEventFiles([childLedger]);
    if (!check.ok) {
      throw new Error(`concurrent ledger failed validation: ${check.diagnostics.map((d) => d.message).join('; ')}`);
    }

    // Task-scoped event-file mode: events-dir only.
    const taskEventsDir = path.join(tmp, '.missiond/tasks/wave99/events');
    const taskOnlyA = appendLifecycleEvent({
      eventsDir: taskEventsDir,
      task: 'wave99-03-events',
      eventKind: 'dispatch',
      actorRole: 'orchestrator',
      summary: 'task-scoped events-dir only #1',
      at: '2026-04-28T00:00:11Z',
      wave: 'wave99',
    });
    if (taskOnlyA.event.seq !== 1) throw new Error(`events-dir-only first seq should be 1, got ${taskOnlyA.event.seq}`);
    if (!taskOnlyA.eventFile?.endsWith('000001.event.lisp')) {
      throw new Error(`events-dir-only first file should end with 000001.event.lisp, got ${taskOnlyA.eventFile}`);
    }
    if (taskOnlyA.ledgerPath != null) throw new Error('events-dir-only should not write a ledger');
    const taskOnlyB = appendLifecycleEvent({
      eventsDir: taskEventsDir,
      task: 'wave99-03-events',
      eventKind: 'claim',
      actorRole: 'worker',
      summary: 'task-scoped events-dir only #2',
      at: '2026-04-28T00:00:12Z',
      wave: 'wave99',
    });
    if (taskOnlyB.event.seq !== 2) throw new Error(`events-dir-only second seq should be 2, got ${taskOnlyB.event.seq}`);
    if (!taskOnlyB.eventFile?.endsWith('000002.event.lisp')) {
      throw new Error(`events-dir-only second file should end with 000002.event.lisp, got ${taskOnlyB.eventFile}`);
    }
    const eventsDirCheck = validateLifecycleEventFiles([taskOnlyA.eventFile, taskOnlyB.eventFile]);
    if (!eventsDirCheck.ok || eventsDirCheck.task_event_files !== 2) {
      throw new Error(`events-dir files failed validation: ${eventsDirCheck.diagnostics.map((d) => d.message).join('; ')}`);
    }

    // Hybrid mode: events-dir primary, legacy ledger updated as compat projection.
    const hybridLedger = path.join(tmp, '.missiond/tasks/wave98/task-lifecycle-events.lisp');
    const hybridDir = path.join(tmp, '.missiond/tasks/wave98/events');
    const hybridA = appendLifecycleEvent({
      eventsDir: hybridDir,
      ledgerPath: hybridLedger,
      task: 'wave98-04-hybrid',
      eventKind: 'dispatch',
      actorRole: 'orchestrator',
      summary: 'hybrid events-dir + ledger #1',
      at: '2026-04-28T00:00:21Z',
      wave: 'wave98',
    });
    if (hybridA.event.seq !== 1) throw new Error(`hybrid first seq should be 1, got ${hybridA.event.seq}`);
    if (!hybridA.eventFile?.endsWith('000001.event.lisp')) {
      throw new Error(`hybrid first file should end with 000001.event.lisp, got ${hybridA.eventFile}`);
    }
    if (hybridA.ledgerPath !== hybridLedger) throw new Error('hybrid mode should write the ledger compat projection');
    const hybridLogs = readLifecycleEventLogs(hybridLedger);
    if (hybridLogs[0]?.events?.[0]?.seq !== 1) {
      throw new Error('hybrid mode ledger should mirror the same seq as the standalone event file');
    }

    // Backward-compat: existing ledger-only callers should still work.
    const legacyLedger = path.join(tmp, '.missiond/tasks/wave97/task-lifecycle-events.lisp');
    const legacyOnly = appendLifecycleEvent({
      ledgerPath: legacyLedger,
      task: 'wave97-05-legacy',
      eventKind: 'claim',
      actorRole: 'worker',
      summary: 'ledger-only legacy compat',
      at: '2026-04-28T00:00:31Z',
      wave: 'wave97',
    });
    if (legacyOnly.event.seq !== 1) throw new Error(`legacy ledger-only seq should be 1, got ${legacyOnly.event.seq}`);
    if (legacyOnly.eventFile != null) throw new Error('legacy ledger-only should not write event file');

    console.log('task-runner-append-event fixtures OK (7 cases, including request-local projection, concurrent child appends, task-scoped events-dir, hybrid events-dir+ledger, and legacy ledger-only compat)');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function runChild(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'] });
    let stderr = '';
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    child.on('error', reject);
    child.on('close', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`child exited ${code}: ${stderr}`));
    });
  });
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main();
  } catch (err) {
    console.error(err.stack || err.message);
    process.exit(1);
  }
}
