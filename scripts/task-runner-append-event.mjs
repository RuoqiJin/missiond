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
  readLifecycleEventLogs,
  renderLifecycleEvent,
  renderLifecycleEventLog,
  spliceBeforeFinalParen,
  validateLifecycleEventFiles,
} from './check-task-lifecycle-events.mjs';

const usage = `Usage:
  node scripts/task-runner-append-event.mjs --ledger <task-lifecycle-events.lisp>
    --task <task-id> --kind <event-kind> --actor-role <role>
    [--commit-role <role>] [--commit-hash <sha>] [--summary <text>]
    [--touched <path[,path...]>] [--file <path>] [--report-path <path>]
    [--receipt-path <path>] [--ref <event-id>] [--id <event-id>]
    [--at <iso8601>] [--wave <wave-id>] [--json] [--dry-fixture]

Appends one lifecycle event using a cooperative sibling .lock file.

Concurrency boundary:
  The helper serializes cooperating local writers by creating
  <ledger>.lock with O_EXCL, rereading the ledger while the lock is held,
  validating the candidate result, writing a temp file, then atomically
  renaming it over the ledger. This does not protect against manual edits,
  tools that ignore the lock, stale locks on crashed processes until timeout,
  or filesystems that do not provide atomic create/rename semantics.
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
  if (!opts.ledgerPath || !opts.task || !opts.kind || !opts.actorRole) {
    fail(usage);
  }
  const result = appendLifecycleEvent({
    ledgerPath: path.resolve(process.cwd(), opts.ledgerPath),
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
  });
  if (opts.json) {
    console.log(JSON.stringify({ ok: true, event: result.event, ledger_path: result.ledgerPath }, null, 2));
  } else {
    console.log(`task-runner-append-event OK (${result.event.id} seq ${result.event.seq})`);
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
  ledgerPath,
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
  lockTimeoutMs = 5000,
  staleLockMs = 30000,
}) {
  validateAppendInput({ task, eventKind, actorRole, commitRole, id });
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
    return { ledgerPath, event };
  });
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

function validateAppendInput({ task, eventKind, actorRole, commitRole, id }) {
  if (!ID_RE.test(task)) throw new Error(`task id "${task}" must match ${ID_RE}`);
  if (!EVENT_KINDS.has(eventKind)) throw new Error(`unknown event kind "${eventKind}"`);
  if (!ID_RE.test(actorRole)) throw new Error(`actor role "${actorRole}" must match ${ID_RE}`);
  if (!COMMIT_ROLES.has(commitRole)) throw new Error(`unknown commit role "${commitRole}"`);
  if (id && !ID_RE.test(id)) throw new Error(`event id "${id}" must match ${ID_RE}`);
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
    console.log('task-runner-append-event fixtures OK (3 cases, including concurrent child appends)');
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
