#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  head,
  isList,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/check-task-lifecycle-events.mjs [--json] [--dry-fixture] <ledger.lisp...>

Checks MissionD task lifecycle event logs:
  - validates (task-lifecycle-event-log ...) header shape
  - validates each (lifecycle-event ...) entry
  - enforces unique ids, strictly increasing :seq, ISO timestamps,
    known event kinds, known commit roles, commit hash format,
    task id shape, and repo-relative touched/report/receipt paths

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.task-lifecycle-event.v1';
export const LOG_HEAD = 'task-lifecycle-event-log';
export const EVENT_HEAD = 'lifecycle-event';
export const ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
export const COMMIT_HASH_RE = /^[0-9a-fA-F]{7,64}$/;
export const ISO8601_RE =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$/;

export const EVENT_KINDS = new Set([
  'dispatch',
  'claim',
  'trace_start',
  'read',
  'worker_commit',
  'parent_hotfix',
  'finalized_report',
  'receipt',
  'completion',
  'cancelled',
  'issue',
]);

export const COMMIT_ROLES = new Set([
  'none',
  'worker',
  'parent_hotfix',
  'final',
  'receipt',
  'verified',
]);

const HEADER_REQUIRED = [':schema', ':wave', ':created-at', ':sequence'];
const REQUIRED_EVENT_FIELDS = [
  ':id',
  ':task',
  ':actor_role',
  ':event_kind',
  ':commit_role',
  ':seq',
  ':at',
  ':touched',
  ':summary',
];
const OPTIONAL_EVENT_FIELDS = [
  ':commit_hash',
  ':report_path',
  ':receipt_path',
  ':refs',
  ':legacy_memory_id',
  ':legacy_trace_id',
];
const ALLOWED_EVENT_FIELDS = new Set([...REQUIRED_EVENT_FIELDS, ...OPTIONAL_EVENT_FIELDS]);

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

  const result = validateLifecycleEventFiles(inputs.map((input) => path.resolve(process.cwd(), input)));
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(
      `task-lifecycle-events check OK (${result.ledgers} ledger${result.ledgers === 1 ? '' : 's'}, ${result.events} event${result.events === 1 ? '' : 's'})`,
    );
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `task-lifecycle-events check FAILED -- ${result.diagnostics.length} diagnostic(s), ${result.ledgers} ledger(s), ${result.events} event(s)`,
    );
  }
  process.exit(result.ok ? 0 : 1);
}

export function validateLifecycleEventFiles(files) {
  const diagnostics = [];
  let ledgers = 0;
  let events = 0;
  for (const file of [...new Set(files)]) {
    let forms;
    try {
      forms = readLispFile(file);
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
      continue;
    }
    for (const form of forms) {
      if (!isList(form) || head(form) !== LOG_HEAD) continue;
      ledgers += 1;
      events += validateLifecycleEventLog(file, form, diagnostics);
    }
  }
  return {
    ok: !diagnostics.some((d) => d.severity === 'error'),
    files: files.length,
    ledgers,
    events,
    diagnostics,
  };
}

export function parseLifecycleEventLogsFromText(source, file = '<memory>') {
  const forms = parseLisp(source, file);
  return parseLifecycleEventLogsFromForms(forms);
}

export function readLifecycleEventLogs(file) {
  return parseLifecycleEventLogsFromForms(readLispFile(file));
}

export function parseLifecycleEventLogsFromForms(forms) {
  const logs = [];
  for (const form of forms) {
    if (!isList(form) || head(form) !== LOG_HEAD) continue;
    const props = readKeywordProps(form, { start: 2 });
    const events = [];
    for (const child of form.children) {
      if (!isList(child) || head(child) !== EVENT_HEAD) continue;
      events.push(eventFromNode(child));
    }
    logs.push({
      id: nodeText(form.children[1]),
      wave: keywordPropText(props, ':wave'),
      schema: keywordPropText(props, ':schema'),
      created_at: keywordPropText(props, ':created-at'),
      sequence: parseInteger(keywordPropText(props, ':sequence')),
      events,
      form,
    });
  }
  return logs;
}

export function eventFromNode(node) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    id: keywordPropText(props, ':id'),
    task: keywordPropText(props, ':task'),
    actor_role: keywordPropText(props, ':actor_role'),
    event_kind: keywordPropText(props, ':event_kind'),
    commit_role: keywordPropText(props, ':commit_role'),
    seq: parseInteger(keywordPropText(props, ':seq')),
    at: keywordPropText(props, ':at'),
    touched: nodeToStringArray(props[':touched']?.value),
    summary: keywordPropText(props, ':summary'),
    commit_hash: keywordPropText(props, ':commit_hash'),
    report_path: keywordPropText(props, ':report_path'),
    receipt_path: keywordPropText(props, ':receipt_path'),
    refs: nodeToStringArray(props[':refs']?.value),
    legacy_memory_id: keywordPropText(props, ':legacy_memory_id'),
    legacy_trace_id: keywordPropText(props, ':legacy_trace_id'),
    loc: node.loc ?? { line: 1, column: 1 },
  };
}

export function validateLifecycleEventLog(file, log, diagnostics) {
  const ledgerId = nodeText(log.children[1]);
  if (!ledgerId) {
    addError(diagnostics, file, log.loc, `(${LOG_HEAD} ...) missing ledger id as second form`);
  } else if (!ID_RE.test(ledgerId)) {
    addError(diagnostics, file, log.children[1].loc, `ledger id "${ledgerId}" must match ${ID_RE}`);
  }

  const props = readKeywordProps(log, { start: 2 });
  for (const field of HEADER_REQUIRED) {
    if (!props[field]) {
      addError(diagnostics, file, log.loc, `(${LOG_HEAD} ${ledgerId ?? '<missing>'} ...) missing required header ${field}`);
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    addError(diagnostics, file, props[':schema']?.value?.loc ?? log.loc, `:schema must be "${SCHEMA}", got ${JSON.stringify(schema)}`);
  }

  const wave = keywordPropText(props, ':wave');
  if (wave && !ID_RE.test(wave)) {
    addError(diagnostics, file, props[':wave'].value.loc, `:wave "${wave}" must match ${ID_RE}`);
  }

  const createdAt = keywordPropText(props, ':created-at');
  if (createdAt && !ISO8601_RE.test(createdAt)) {
    addError(diagnostics, file, props[':created-at'].value.loc, `:created-at "${createdAt}" must be ISO-8601 with timezone`);
  }

  const sequenceText = keywordPropText(props, ':sequence');
  const headerSequence = parseInteger(sequenceText);
  if (sequenceText && !/^\d+$/.test(sequenceText)) {
    addError(diagnostics, file, props[':sequence'].value.loc, `:sequence must be a non-negative integer, got "${sequenceText}"`);
  }

  for (const child of log.children) {
    if (!isList(child)) continue;
    if (head(child) !== EVENT_HEAD) {
      addError(diagnostics, file, child.loc, `unknown entry head "${head(child)}"; expected ${EVENT_HEAD}`);
    }
  }

  const seenIds = new Map();
  let lastSeq = null;
  let count = 0;
  for (const child of log.children) {
    if (!isList(child) || head(child) !== EVENT_HEAD) continue;
    count += 1;
    const event = validateLifecycleEvent(file, child, diagnostics);
    if (event.id) {
      if (seenIds.has(event.id)) {
        addError(diagnostics, file, child.loc, `duplicate event :id "${event.id}" (also at line ${seenIds.get(event.id)})`);
      } else {
        seenIds.set(event.id, child.loc?.line ?? 1);
      }
    }
    if (event.seq != null) {
      if (lastSeq != null && event.seq <= lastSeq) {
        addError(diagnostics, file, child.loc, `event :seq ${event.seq} must be strictly greater than previous event :seq ${lastSeq}`);
      }
      lastSeq = event.seq;
    }
  }
  if (headerSequence != null && lastSeq != null && headerSequence < lastSeq) {
    addError(diagnostics, file, props[':sequence']?.value?.loc ?? log.loc, `header :sequence ${headerSequence} must be >= last event :seq ${lastSeq}`);
  }
  return count;
}

export function validateLifecycleEvent(file, node, diagnostics) {
  const props = readKeywordProps(node, { start: 1 });
  for (const field of REQUIRED_EVENT_FIELDS) {
    if (!props[field]) {
      addError(diagnostics, file, node.loc, `(${EVENT_HEAD} ...) missing required field ${field}`);
    }
  }
  for (const key of Object.keys(props)) {
    if (!ALLOWED_EVENT_FIELDS.has(key)) {
      addError(diagnostics, file, props[key].keyNode.loc, `unknown lifecycle event field ${key}`);
    }
  }

  const event = eventFromNode(node);
  validateIdField(file, props, ':id', event.id, 'event id', diagnostics);
  validateIdField(file, props, ':task', event.task, 'task id', diagnostics);
  validateIdField(file, props, ':actor_role', event.actor_role, 'actor_role', diagnostics);
  validateOptionalIdField(file, props, ':legacy_memory_id', event.legacy_memory_id, diagnostics);
  validateOptionalIdField(file, props, ':legacy_trace_id', event.legacy_trace_id, diagnostics);

  if (event.event_kind && !EVENT_KINDS.has(event.event_kind)) {
    addError(diagnostics, file, props[':event_kind']?.value?.loc ?? node.loc, `unknown :event_kind "${event.event_kind}"; expected one of ${[...EVENT_KINDS].join('|')}`);
  }
  if (event.commit_role && !COMMIT_ROLES.has(event.commit_role)) {
    addError(diagnostics, file, props[':commit_role']?.value?.loc ?? node.loc, `unknown :commit_role "${event.commit_role}"; expected one of ${[...COMMIT_ROLES].join('|')}`);
  }
  if (event.seq == null || !Number.isInteger(event.seq) || event.seq < 1) {
    addError(diagnostics, file, props[':seq']?.value?.loc ?? node.loc, ':seq must be a positive integer');
  }
  if (event.at && !ISO8601_RE.test(event.at)) {
    addError(diagnostics, file, props[':at']?.value?.loc ?? node.loc, `:at "${event.at}" must be ISO-8601 with timezone`);
  }
  if (event.summary == null || event.summary.trim() === '') {
    addError(diagnostics, file, props[':summary']?.value?.loc ?? node.loc, ':summary must be a non-empty string or atom');
  }
  if (!props[':touched']?.value || !isList(props[':touched'].value)) {
    addError(diagnostics, file, props[':touched']?.value?.loc ?? node.loc, ':touched must be a vector/list, even when empty');
  }
  for (const touched of event.touched) {
    validateRepoRelativePath(file, props[':touched']?.value?.loc ?? node.loc, touched, ':touched', diagnostics);
  }
  if (event.report_path) {
    validateRepoRelativePath(file, props[':report_path'].value.loc, event.report_path, ':report_path', diagnostics);
  }
  if (event.receipt_path) {
    validateRepoRelativePath(file, props[':receipt_path'].value.loc, event.receipt_path, ':receipt_path', diagnostics);
  }
  if (event.commit_hash && !COMMIT_HASH_RE.test(event.commit_hash)) {
    addError(diagnostics, file, props[':commit_hash'].value.loc, `:commit_hash "${event.commit_hash}" must match ${COMMIT_HASH_RE}`);
  }
  return event;
}

export function isRepoRelativePath(value) {
  if (typeof value !== 'string' || value.trim() === '') return false;
  if (path.isAbsolute(value) || value.startsWith('~')) return false;
  const normalized = value.replace(/\\/g, '/');
  if (normalized.startsWith('/') || normalized.includes('\0')) return false;
  return !normalized.split('/').some((segment) => segment === '..');
}

export function renderLifecycleEvent(event) {
  const touched = renderVector(event.touched ?? []);
  const lines = [
    `  (${EVENT_HEAD}`,
    `    :id ${event.id}`,
    `    :task ${event.task}`,
    `    :actor_role ${event.actor_role}`,
    `    :event_kind ${event.event_kind}`,
    `    :commit_role ${event.commit_role ?? 'none'}`,
    `    :seq ${event.seq}`,
    `    :at "${event.at}"`,
    `    :touched ${touched}`,
    `    :summary ${quoteString(event.summary ?? '')}`,
  ];
  if (event.commit_hash) lines.push(`    :commit_hash ${event.commit_hash}`);
  if (event.report_path) lines.push(`    :report_path "${event.report_path}"`);
  if (event.receipt_path) lines.push(`    :receipt_path "${event.receipt_path}"`);
  if (event.refs?.length) lines.push(`    :refs ${renderAtomVector(event.refs)}`);
  if (event.legacy_memory_id) lines.push(`    :legacy_memory_id ${event.legacy_memory_id}`);
  if (event.legacy_trace_id) lines.push(`    :legacy_trace_id ${event.legacy_trace_id}`);
  lines[lines.length - 1] += ')';
  return lines.join('\n');
}

export function renderLifecycleEventLog({ id, wave, createdAt, sequence = 0, events = [] }) {
  const body = [
    `(${LOG_HEAD} ${id}`,
    `  :schema "${SCHEMA}"`,
    `  :wave ${wave}`,
    `  :created-at "${createdAt}"`,
    `  :sequence ${sequence}`,
  ];
  if (events.length > 0) {
    body.push('');
    body.push(events.map((event) => renderLifecycleEvent(event)).join('\n\n'));
  }
  body.push(')');
  body.push('');
  return body.join('\n');
}

export function spliceBeforeFinalParen(body, block) {
  let i = body.length - 1;
  while (i >= 0 && /\s/.test(body[i])) i -= 1;
  if (i < 0 || body[i] !== ')') {
    throw new Error('lifecycle ledger is not terminated by a close paren');
  }
  return `${body.slice(0, i)}${block}${body.slice(i)}\n`.replace(/\n+$/, '\n');
}

export function nextSequence(events) {
  let max = 0;
  for (const event of events) {
    if (Number.isInteger(event.seq) && event.seq > max) max = event.seq;
  }
  return max + 1;
}

function validateIdField(file, props, key, value, label, diagnostics) {
  if (!value) return;
  if (!ID_RE.test(value)) {
    addError(diagnostics, file, props[key]?.value?.loc ?? props[key]?.keyNode?.loc, `${label} "${value}" must match ${ID_RE}`);
  }
}

function validateOptionalIdField(file, props, key, value, diagnostics) {
  if (!value) return;
  validateIdField(file, props, key, value, key.replace(':', ''), diagnostics);
}

function validateRepoRelativePath(file, loc, value, field, diagnostics) {
  if (!isRepoRelativePath(value)) {
    addError(diagnostics, file, loc, `${field} path "${value}" must be repo-relative and must not contain absolute, ~, empty, or .. traversal segments`);
  }
}

function parseInteger(text) {
  if (text == null || !/^-?\d+$/.test(text)) return null;
  return Number.parseInt(text, 10);
}

function renderVector(values) {
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

function addError(diagnostics, file, loc, message) {
  diagnostics.push({
    severity: 'error',
    file,
    line: loc?.line ?? 1,
    column: loc?.column ?? 1,
    message,
  });
}

function runFixtures(json) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-lifecycle-check-'));
  const cases = [];
  try {
    const valid = renderLifecycleEventLog({
      id: 'wave99-lifecycle',
      wave: 'wave99',
      createdAt: '2026-04-28T00:00:00Z',
      sequence: 3,
      events: [
        {
          id: 'wave99-01-dispatch-001',
          task: 'wave99-01-demo',
          actor_role: 'orchestrator',
          event_kind: 'dispatch',
          commit_role: 'none',
          seq: 1,
          at: '2026-04-28T00:00:01Z',
          touched: ['.missiond/claudecode/wave99-01-demo.md'],
          summary: 'dispatched task',
        },
        {
          id: 'wave99-01-claim-002',
          task: 'wave99-01-demo',
          actor_role: 'worker',
          event_kind: 'claim',
          commit_role: 'none',
          seq: 2,
          at: '2026-04-28T00:00:02Z',
          touched: [],
          summary: 'claimed task',
        },
        {
          id: 'wave99-01-commit-003',
          task: 'wave99-01-demo',
          actor_role: 'worker',
          event_kind: 'worker_commit',
          commit_role: 'worker',
          seq: 3,
          at: '2026-04-28T00:00:03Z',
          touched: ['scripts/demo.mjs'],
          summary: 'worker commit recorded',
          commit_hash: 'abcdef1',
        },
      ],
    });
    cases.push({ name: 'valid-ledger', source: valid, ok: true });
    cases.push({
      name: 'cancelled-kind-valid',
      source: renderLifecycleEventLog({
        id: 'wave99-cancelled-lifecycle',
        wave: 'wave99',
        createdAt: '2026-04-28T00:00:00Z',
        sequence: 1,
        events: [
          {
            id: 'wave99-01-cancelled-001',
            task: 'wave99-01-demo',
            actor_role: 'orchestrator',
            event_kind: 'cancelled',
            commit_role: 'none',
            seq: 1,
            at: '2026-04-28T00:00:01Z',
            touched: [],
            summary: 'cancelled wrong dispatch',
          },
        ],
      }),
      ok: true,
    });
    cases.push({
      name: 'duplicate-id',
      source: valid.replace('wave99-01-commit-003', 'wave99-01-claim-002'),
      ok: false,
    });
    cases.push({
      name: 'non-monotonic-seq',
      source: valid.replace(':seq 2', ':seq 1'),
      ok: false,
    });
    cases.push({
      name: 'unknown-kind',
      source: valid.replace(':event_kind worker_commit', ':event_kind surprise'),
      ok: false,
    });
    cases.push({
      name: 'bad-commit-hash',
      source: valid.replace(':commit_hash abcdef1', ':commit_hash zzzzzzz'),
      ok: false,
    });
    cases.push({
      name: 'absolute-path',
      source: valid.replace('"scripts/demo.mjs"', '"/tmp/demo.mjs"'),
      ok: false,
    });
    cases.push({
      name: 'bad-task-id',
      source: valid.replace(':task wave99-01-demo', ':task BadTask'),
      ok: false,
    });

    let failed = 0;
    for (const c of cases) {
      const file = path.join(tmp, `${c.name}.lisp`);
      fs.writeFileSync(file, c.source);
      const result = validateLifecycleEventFiles([file]);
      if (result.ok !== c.ok) {
        failed += 1;
        console.error(`fixture failed: ${c.name}: expected ok=${c.ok}, got ok=${result.ok}`);
        for (const d of result.diagnostics) console.error(`  ${d.message}`);
      }
    }
    if (failed > 0) {
      console.error(`task-lifecycle-events fixtures FAILED -- ${failed} of ${cases.length}`);
      process.exit(1);
    }
    if (json) {
      console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
    } else {
      console.log(`task-lifecycle-events fixtures OK (${cases.length} cases)`);
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
