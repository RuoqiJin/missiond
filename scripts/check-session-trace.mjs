#!/usr/bin/env node

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
  node scripts/check-session-trace.mjs [--json] [--dry-fixture] <trace.lisp...>

Checks MissionD session-trace v1 Lisp files:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates the (session-trace <wave-id> ...) header
  - validates each (trace-event ...) entry: required :id :seq :at :task
    :backend :kind :summary, optional :agent :files :command :exit_code
    :duration_ms :commit_hash :report_path :memory_refs :trace_refs
  - rejects: duplicate :id, non-increasing :seq, invalid timestamps,
    absolute paths in :files / :report_path / :trace_refs / :command,
    missing :task / :backend / :kind, malformed :duration_ms or :exit_code,
    unknown :kind values, unknown entry heads

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.session-trace.v1';

const ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
const ISO8601_RE =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$/;

export const ENTRY_HEAD = 'trace-event';

export const KIND_VALUES = new Set([
  'dispatch',
  'start',
  'read',
  'edit',
  'command',
  'test',
  'commit',
  'complete',
  'failure',
  'retry',
  'observation',
]);

const REQUIRED_FIELDS = [':id', ':seq', ':at', ':task', ':backend', ':kind', ':summary'];

const OPTIONAL_FIELDS = [
  ':agent',
  ':files',
  ':command',
  ':exit_code',
  ':duration_ms',
  ':commit_hash',
  ':report_path',
  ':memory_refs',
  ':trace_refs',
];

const ALLOWED_FIELDS = new Set([...REQUIRED_FIELDS, ...OPTIONAL_FIELDS]);

const HEADER_REQUIRED = [':schema', ':wave', ':created-at', ':sequence'];

// Read-only helper used by analyzers and other tooling. Parses a trace-file
// path and yields the structured events it contains, without applying the
// strict checker rules (the caller is responsible for tolerating partial data
// when the file is mid-validation). Returns an array of one entry per
// (session-trace ...) form found, each with `wave`, `header`, and `events`.
//
// `events` is a flat array of plain objects mirroring trace-event fields:
//   { id, seq, at, task, backend, kind, summary,
//     agent?, files: string[], command?, exit_code?: number,
//     duration_ms?: number, commit_hash?, report_path?,
//     memory_refs: string[], trace_refs: string[],
//     loc: { line, column } }
// Unknown / missing scalar fields become null; vector fields default to [].
export function parseTraceEvents(file) {
  const forms = readLispFile(file);
  const traces = [];
  for (const form of forms) {
    if (!isList(form) || head(form) !== 'session-trace') continue;
    const wave = nodeText(form.children[1]);
    const headerProps = readKeywordProps(form, { start: 2 });
    const header = {
      schema: keywordPropText(headerProps, ':schema'),
      wave: keywordPropText(headerProps, ':wave'),
      createdAt: keywordPropText(headerProps, ':created-at'),
      sequence: parseIntOrNull(keywordPropText(headerProps, ':sequence')),
    };
    const events = [];
    for (const child of form.children) {
      if (!isList(child) || head(child) !== ENTRY_HEAD) continue;
      const props = readKeywordProps(child, { start: 1 });
      events.push({
        id: keywordPropText(props, ':id'),
        seq: parseIntOrNull(keywordPropText(props, ':seq')),
        at: keywordPropText(props, ':at'),
        task: keywordPropText(props, ':task'),
        backend: keywordPropText(props, ':backend'),
        kind: keywordPropText(props, ':kind'),
        summary: keywordPropText(props, ':summary'),
        agent: keywordPropText(props, ':agent'),
        files: props[':files'] ? nodeToStringArray(props[':files'].value) : [],
        command: keywordPropText(props, ':command'),
        exit_code: parseIntOrNull(keywordPropText(props, ':exit_code')),
        duration_ms: parseIntOrNull(keywordPropText(props, ':duration_ms')),
        commit_hash: keywordPropText(props, ':commit_hash'),
        report_path: keywordPropText(props, ':report_path'),
        memory_refs: props[':memory_refs']
          ? nodeToStringArray(props[':memory_refs'].value)
          : [],
        trace_refs: props[':trace_refs']
          ? nodeToStringArray(props[':trace_refs'].value)
          : [],
        loc: child.loc ?? { line: 1, column: 1 },
      });
    }
    traces.push({ file, wave, header, events });
  }
  return traces;
}

function parseIntOrNull(text) {
  if (text == null) return null;
  if (!/^-?\d+$/.test(text)) return null;
  return Number.parseInt(text, 10);
}

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
  const files = inputs.map((input) => path.resolve(cwd, input));

  const diagnostics = [];
  let traceCount = 0;
  let eventCount = 0;

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
      if (!isList(form) || head(form) !== 'session-trace') continue;
      traceCount += 1;
      eventCount += validateTrace(file, form, diagnostics);
    }
  }

  const errors = diagnostics.filter((d) => d.severity === 'error');
  const ok = errors.length === 0;

  if (json) {
    console.log(
      JSON.stringify(
        {
          ok,
          files: files.length,
          traces: traceCount,
          events: eventCount,
          errors,
        },
        null,
        2,
      ),
    );
  } else if (ok) {
    console.log(
      `session-trace check OK (${traceCount} trace${traceCount === 1 ? '' : 's'}, ${eventCount} event${eventCount === 1 ? '' : 's'})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `session-trace check FAILED — ${errors.length} error(s), ${traceCount} trace(s), ${eventCount} events`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function validateTrace(file, trace, diagnostics) {
  const waveId = nodeText(trace.children[1]);
  if (!waveId) {
    addError(diagnostics, file, trace.loc, '(session-trace ...) missing wave id as second form');
  } else if (!ID_RE.test(waveId)) {
    addError(
      diagnostics,
      file,
      trace.children[1].loc,
      `wave id "${waveId}" must match ${ID_RE}`,
    );
  }

  const props = readKeywordProps(trace, { start: 2 });
  for (const field of HEADER_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        trace.loc,
        `(session-trace ${waveId ?? '<missing>'} ...) missing required header ${field}`,
      );
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    addError(
      diagnostics,
      file,
      props[':schema']?.value?.loc ?? trace.loc,
      `:schema must be "${SCHEMA}", got ${JSON.stringify(schema)}`,
    );
  }

  const createdAt = keywordPropText(props, ':created-at');
  if (createdAt && !ISO8601_RE.test(createdAt)) {
    addError(
      diagnostics,
      file,
      props[':created-at'].value.loc,
      `:created-at "${createdAt}" must be ISO-8601 with timezone (e.g. 2026-04-28T15:30:00Z)`,
    );
  }

  const sequenceText = keywordPropText(props, ':sequence');
  if (sequenceText && !/^\d+$/.test(sequenceText)) {
    addError(
      diagnostics,
      file,
      props[':sequence'].value.loc,
      `:sequence must be a non-negative integer, got "${sequenceText}"`,
    );
  }

  // Reject any list child whose head is unknown (only trace-event allowed).
  for (const child of trace.children) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== ENTRY_HEAD) {
      addError(
        diagnostics,
        file,
        child.loc,
        `unknown entry head "${h}"; expected "${ENTRY_HEAD}"`,
      );
    }
  }

  const events = trace.children.filter((c) => isList(c) && head(c) === ENTRY_HEAD);
  const seenIds = new Map();
  let lastSeq = null;
  for (const event of events) {
    const result = validateEvent(file, event, diagnostics);
    if (result.id) {
      if (seenIds.has(result.id)) {
        addError(
          diagnostics,
          file,
          event.loc,
          `duplicate event :id "${result.id}" (also at line ${seenIds.get(result.id)})`,
        );
      } else {
        seenIds.set(result.id, event.loc?.line ?? 1);
      }
    }
    if (result.seq != null) {
      if (lastSeq != null && result.seq <= lastSeq) {
        addError(
          diagnostics,
          file,
          event.loc,
          `event :seq ${result.seq} must be strictly greater than previous event :seq ${lastSeq}`,
        );
      }
      lastSeq = result.seq;
    }
  }

  return events.length;
}

function validateEvent(file, event, diagnostics) {
  const props = readKeywordProps(event, { start: 1 });

  for (const field of REQUIRED_FIELDS) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        event.loc,
        `(${ENTRY_HEAD} ...) missing required field ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!ALLOWED_FIELDS.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${ENTRY_HEAD} ...) has unknown field ${key}`,
      );
    }
  }

  const id = keywordPropText(props, ':id');
  if (id != null && !ID_RE.test(id)) {
    addError(
      diagnostics,
      file,
      props[':id'].value.loc,
      `event :id "${id}" must match ${ID_RE}`,
    );
  }

  const taskId = keywordPropText(props, ':task');
  if (taskId != null && !ID_RE.test(taskId)) {
    addError(
      diagnostics,
      file,
      props[':task'].value.loc,
      `event :task "${taskId}" must match ${ID_RE}`,
    );
  }

  const backend = keywordPropText(props, ':backend');
  if (backend != null && backend.trim() === '') {
    addError(
      diagnostics,
      file,
      props[':backend'].value.loc,
      'event :backend must be a non-empty atom or string',
    );
  }

  const kind = keywordPropText(props, ':kind');
  if (kind != null && !KIND_VALUES.has(kind)) {
    addError(
      diagnostics,
      file,
      props[':kind'].value.loc,
      `event :kind "${kind}" must be one of ${[...KIND_VALUES].join('|')}`,
    );
  }

  const summary = keywordPropText(props, ':summary');
  if (summary != null) {
    if (summary.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':summary'].value.loc,
        'event :summary must be a non-empty string',
      );
    } else if (summary.includes('\n')) {
      addError(
        diagnostics,
        file,
        props[':summary'].value.loc,
        'event :summary must be a single line',
      );
    }
  }

  const seqText = keywordPropText(props, ':seq');
  let seq = null;
  if (seqText != null) {
    if (!/^\d+$/.test(seqText)) {
      addError(
        diagnostics,
        file,
        props[':seq'].value.loc,
        `event :seq must be a non-negative integer, got "${seqText}"`,
      );
    } else {
      seq = Number.parseInt(seqText, 10);
    }
  }

  const at = keywordPropText(props, ':at');
  if (at != null && !ISO8601_RE.test(at)) {
    addError(
      diagnostics,
      file,
      props[':at'].value.loc,
      `event :at "${at}" must be ISO-8601 with timezone (e.g. 2026-04-28T15:30:00Z)`,
    );
  }

  const agent = keywordPropText(props, ':agent');
  if (agent != null && agent.trim() === '') {
    addError(
      diagnostics,
      file,
      props[':agent'].value.loc,
      'event :agent must be a non-empty atom or string',
    );
  }

  validatePathList(file, props, ':files', diagnostics, event);

  if (props[':report_path']) {
    const reportPath = keywordPropText(props, ':report_path');
    if (reportPath != null) {
      checkRepoRelativePath(
        file,
        diagnostics,
        props[':report_path'].value.loc,
        ':report_path',
        reportPath,
      );
    }
  }

  if (props[':command']) {
    const cmdNode = props[':command'].value;
    const cmd = nodeText(cmdNode);
    if (cmd == null || cmd.trim() === '') {
      addError(
        diagnostics,
        file,
        cmdNode?.loc ?? event.loc,
        'event :command must be a non-empty atom or string',
      );
    } else {
      checkCommandPaths(file, diagnostics, cmdNode.loc, cmd);
    }
  }

  if (props[':exit_code']) {
    const exitText = keywordPropText(props, ':exit_code');
    if (exitText == null || !/^-?\d+$/.test(exitText)) {
      addError(
        diagnostics,
        file,
        props[':exit_code'].value.loc,
        `event :exit_code must be an integer, got "${exitText}"`,
      );
    }
  }

  if (props[':duration_ms']) {
    const durText = keywordPropText(props, ':duration_ms');
    if (durText == null || !/^\d+$/.test(durText)) {
      addError(
        diagnostics,
        file,
        props[':duration_ms'].value.loc,
        `event :duration_ms must be a non-negative integer, got "${durText}"`,
      );
    }
  }

  if (props[':commit_hash']) {
    const hashText = keywordPropText(props, ':commit_hash');
    if (hashText == null || !/^[0-9a-f]{4,64}$/i.test(hashText)) {
      addError(
        diagnostics,
        file,
        props[':commit_hash'].value.loc,
        `event :commit_hash must be a 4..64 hex SHA, got "${hashText}"`,
      );
    }
  }

  validateIdRefList(file, props, ':memory_refs', diagnostics, event);
  validateTraceRefs(file, props, ':trace_refs', diagnostics, event);

  return { id, seq };
}

function validatePathList(file, props, key, diagnostics, event) {
  if (!props[key]) return;
  const node = props[key].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? event.loc,
      `event ${key} must be a vector/list of repo-relative path strings`,
    );
    return;
  }
  const paths = nodeToStringArray(node);
  for (const p of paths) {
    checkRepoRelativePath(file, diagnostics, node.loc, key, p);
  }
}

function validateIdRefList(file, props, key, diagnostics, event) {
  if (!props[key]) return;
  const node = props[key].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? event.loc,
      `event ${key} must be a vector/list of id atoms/strings`,
    );
    return;
  }
  const refs = nodeToStringArray(node);
  for (const r of refs) {
    if (!ID_RE.test(r)) {
      addError(
        diagnostics,
        file,
        node.loc,
        `event ${key} id "${r}" must match ${ID_RE}`,
      );
    }
  }
}

function validateTraceRefs(file, props, key, diagnostics, event) {
  // :trace_refs are id references to prior trace-event :id values, not paths.
  // We re-use the id-ref shape: the contract specifically lists trace_refs as
  // "absolute paths rejected", but trace_refs hold ids — paths are rejected by
  // construction since ids cannot start with `/`.
  validateIdRefList(file, props, key, diagnostics, event);
}

function checkRepoRelativePath(file, diagnostics, loc, key, p) {
  if (typeof p !== 'string' || p === '') {
    addError(diagnostics, file, loc, `event ${key} path must be a non-empty string`);
    return;
  }
  if (path.isAbsolute(p) || p.startsWith('/') || p.startsWith('~')) {
    addError(
      diagnostics,
      file,
      loc,
      `event ${key} path must be repo-relative, got absolute "${p}"`,
    );
    return;
  }
  if (p.split('/').some((seg) => seg === '..')) {
    addError(
      diagnostics,
      file,
      loc,
      `event ${key} path must not traverse upward, got "${p}"`,
    );
  }
}

function checkCommandPaths(file, diagnostics, loc, cmd) {
  // Reject absolute-looking tokens in the command argv. We split on whitespace
  // (commands here are shell-line strings, not POSIX argv) and inspect each
  // token for a leading `/` or `~`. This keeps trace facts portable across
  // workstations and prevents leaking developer-machine paths.
  const tokens = cmd.split(/\s+/).filter(Boolean);
  for (const tok of tokens) {
    // Strip a leading shell variable assignment like FOO=/abs/path
    const eqIdx = tok.indexOf('=');
    const candidate = eqIdx > 0 && /^[A-Za-z_][A-Za-z0-9_]*=/.test(tok)
      ? tok.slice(eqIdx + 1)
      : tok;
    if (candidate.startsWith('/') || candidate.startsWith('~/') || candidate === '~') {
      addError(
        diagnostics,
        file,
        loc,
        `event :command must not contain absolute paths, got token "${tok}"`,
      );
      return;
    }
  }
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

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'valid header + bootstrap observation',
      category: 'pass',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00+08:00"
        :sequence 1
        (trace-event
          :id wave23-trace-001
          :seq 1
          :at "2026-04-28T00:00:00+08:00"
          :task wave23-01-session-trace-schema-v0
          :backend claudecode
          :kind observation
          :summary "bootstrap"))`,
      ok: true,
    },
    {
      name: 'all kinds enumerated',
      category: 'pass',
      source: buildAllKindsTrace(),
      ok: true,
    },
    {
      name: 'optional fields populated',
      category: 'pass',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-cmd
          :seq 1
          :at "2026-04-28T00:00:01Z"
          :task wave23-01-session-trace-schema-v0
          :backend claudecode
          :kind command
          :summary "ran cargo build"
          :agent claudecode-session-1
          :files ["scripts/check-session-trace.mjs"]
          :command "cargo build -p missiond-core"
          :exit_code 0
          :duration_ms 12345
          :commit_hash "abc1234"
          :report_path ".missiond/tasks/wave23/reports/wave23-01-session-trace-schema-v0.report.lisp"
          :memory_refs [wave23-01-claim-001]
          :trace_refs [wave23-trace-000]))`,
      ok: true,
    },
    {
      name: 'wrong schema',
      category: 'fail-schema',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v0"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1)`,
      ok: false,
    },
    {
      name: 'missing header field',
      category: 'fail-header',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :sequence 1)`,
      ok: false,
    },
    {
      name: 'unknown entry head',
      category: 'fail-entry-head',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (random-event
          :id wave23-trace-x
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind observation
          :summary "nope"))`,
      ok: false,
    },
    {
      name: 'duplicate event id',
      category: 'fail-duplicate-id',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-dup
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind start
          :summary "first")
        (trace-event
          :id wave23-trace-dup
          :seq 2
          :at "2026-04-28T00:00:01Z"
          :task wave23-01
          :backend claudecode
          :kind complete
          :summary "second"))`,
      ok: false,
    },
    {
      name: 'non-increasing seq',
      category: 'fail-seq',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-a
          :seq 5
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind start
          :summary "first")
        (trace-event
          :id wave23-trace-b
          :seq 5
          :at "2026-04-28T00:00:01Z"
          :task wave23-01
          :backend claudecode
          :kind complete
          :summary "same seq"))`,
      ok: false,
    },
    {
      name: 'invalid timestamp',
      category: 'fail-timestamp',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-bad-at
          :seq 1
          :at "2026-04-28 00:00:00"
          :task wave23-01
          :backend claudecode
          :kind observation
          :summary "no timezone"))`,
      ok: false,
    },
    {
      name: 'absolute path in :files',
      category: 'fail-abs-files',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-abs-file
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind read
          :summary "abs file"
          :files ["/etc/passwd"]))`,
      ok: false,
    },
    {
      name: 'absolute path in :report_path',
      category: 'fail-abs-report-path',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-abs-report
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind complete
          :summary "abs report"
          :report_path "/var/missiond/report.lisp"))`,
      ok: false,
    },
    {
      name: 'absolute path in :command',
      category: 'fail-abs-command',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-abs-cmd
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind command
          :summary "abs argv"
          :command "/usr/local/bin/cargo build"))`,
      ok: false,
    },
    {
      name: 'parent-traversal in :trace_refs treated as id error',
      category: 'fail-trace-refs',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-bad-ref
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind retry
          :summary "bad ref id"
          :trace_refs ["../wave22-trace-001"]))`,
      ok: false,
    },
    {
      name: 'missing :task',
      category: 'fail-missing-task',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-no-task
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :backend claudecode
          :kind observation
          :summary "no task"))`,
      ok: false,
    },
    {
      name: 'missing :backend',
      category: 'fail-missing-backend',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-no-backend
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :kind observation
          :summary "no backend"))`,
      ok: false,
    },
    {
      name: 'missing :kind',
      category: 'fail-missing-kind',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-no-kind
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :summary "no kind"))`,
      ok: false,
    },
    {
      name: 'unknown :kind value',
      category: 'fail-unknown-kind',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-bad-kind
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind chitchat
          :summary "no such kind"))`,
      ok: false,
    },
    {
      name: 'malformed :duration_ms (negative)',
      category: 'fail-duration',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-bad-dur
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind command
          :summary "negative duration"
          :duration_ms -5))`,
      ok: false,
    },
    {
      name: 'malformed :duration_ms (non-integer)',
      category: 'fail-duration',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-float-dur
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind command
          :summary "float duration"
          :duration_ms 1.5))`,
      ok: false,
    },
    {
      name: 'malformed :exit_code (non-integer)',
      category: 'fail-exit-code',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-bad-exit
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind command
          :summary "exit not int"
          :exit_code "ok"))`,
      ok: false,
    },
    {
      name: ':exit_code negative is allowed (signal)',
      category: 'pass',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-neg-exit
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind command
          :summary "killed by signal"
          :exit_code -9))`,
      ok: true,
    },
    {
      name: 'unknown event field',
      category: 'fail-unknown-field',
      source: `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        (trace-event
          :id wave23-trace-typo
          :seq 1
          :at "2026-04-28T00:00:00Z"
          :task wave23-01
          :backend claudecode
          :kind observation
          :summary "ok"
          :extra "nope"))`,
      ok: false,
    },
  ];

  let failed = 0;
  const categories = new Set();
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    const file = `<fixture:${fixture.name}>`;
    const diagnostics = [];
    let forms;
    try {
      forms = parseLisp(fixture.source, file);
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
      forms = [];
    }
    for (const form of forms) {
      if (isList(form) && head(form) === 'session-trace') {
        validateTrace(file, form, diagnostics);
      }
    }
    const ok = !diagnostics.some((d) => d.severity === 'error');
    if (ok !== fixture.ok) {
      failed += 1;
      console.error(
        `fixture failed: ${fixture.name} (expected ok=${fixture.ok}, got ok=${ok})`,
      );
      for (const d of diagnostics) {
        console.error(`  ${d.file}:${d.line}:${d.column}: ${d.message}`);
      }
    }
  }
  if (json) {
    console.log(
      JSON.stringify(
        {
          ok: failed === 0,
          fixtures: fixtures.length,
          failed,
          categories: [...categories],
        },
        null,
        2,
      ),
    );
  }
  if (failed > 0) {
    console.error(`session-trace fixtures FAILED — ${failed} of ${fixtures.length}`);
    process.exit(1);
  }
  if (!json) {
    console.log(
      `session-trace fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

function buildAllKindsTrace() {
  const kinds = [
    'dispatch',
    'start',
    'read',
    'edit',
    'command',
    'test',
    'commit',
    'complete',
    'failure',
    'retry',
    'observation',
  ];
  const events = kinds
    .map((kind, idx) => {
      const seq = idx + 1;
      const at = `2026-04-28T00:00:${String(idx).padStart(2, '0')}Z`;
      return `(trace-event
        :id wave23-trace-kind-${kind}
        :seq ${seq}
        :at "${at}"
        :task wave23-01-session-trace-schema-v0
        :backend claudecode
        :kind ${kind}
        :summary "kind ${kind}")`;
    })
    .join('\n        ');
  return `(session-trace wave23
        :schema "missiond.session-trace.v1"
        :wave wave23
        :created-at "2026-04-28T00:00:00Z"
        :sequence 1
        ${events})`;
}

// Only run the CLI when invoked directly (preserves the existing
// `node scripts/check-session-trace.mjs ...` behavior); skips main() when
// the module is imported by analyzer/tooling so named exports stay pure.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
