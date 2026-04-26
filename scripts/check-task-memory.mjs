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
  node scripts/check-task-memory.mjs [--json] [--dry-fixture] <ledger.lisp...>

Checks MissionD shared-memory ledger v1 Lisp files:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates the (shared-memory <wave-id> ...) header
  - validates each entry head: claim | observation | blocker | completion | correction | handoff
  - enforces unique :id values, strictly increasing :seq, ISO-8601 :at,
    repo-relative :touched paths, and a kebab/dot/underscore id style

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

const SCHEMA = 'missiond.shared-memory.v1';

const ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
const ISO8601_RE =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$/;

const ENTRY_SHAPES = {
  claim: {
    required: [':id', ':task', ':agent', ':seq', ':summary'],
    optional: [':at', ':touched', ':refs'],
  },
  observation: {
    required: [':id', ':task', ':agent', ':seq', ':summary'],
    optional: [':at', ':touched', ':refs'],
  },
  blocker: {
    required: [':id', ':task', ':agent', ':seq', ':summary'],
    optional: [':at', ':touched', ':refs'],
  },
  completion: {
    required: [':id', ':task', ':agent', ':seq', ':summary', ':touched'],
    optional: [':at', ':refs'],
  },
  correction: {
    required: [':id', ':task', ':agent', ':seq', ':summary', ':refs'],
    optional: [':at', ':touched'],
  },
  handoff: {
    required: [':id', ':task', ':agent', ':seq', ':summary', ':refs'],
    optional: [':at', ':touched'],
  },
};

const ENTRY_HEADS = new Set(Object.keys(ENTRY_SHAPES));

const HEADER_REQUIRED = [':schema', ':wave', ':created-at', ':sequence'];

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
    runFixtures();
    return;
  }

  if (inputs.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const files = inputs.map((input) => path.resolve(cwd, input));

  const diagnostics = [];
  let ledgerCount = 0;
  let entryCount = 0;

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
      if (!isList(form) || head(form) !== 'shared-memory') continue;
      ledgerCount += 1;
      entryCount += validateLedger(file, form, diagnostics);
    }
  }

  const ok = !diagnostics.some((d) => d.severity === 'error');
  if (json) {
    console.log(
      JSON.stringify(
        { ok, files: files.length, ledgers: ledgerCount, entries: entryCount, diagnostics },
        null,
        2,
      ),
    );
  } else if (ok) {
    console.log(
      `shared-memory check OK (${ledgerCount} ledger${ledgerCount === 1 ? '' : 's'}, ${entryCount} entr${entryCount === 1 ? 'y' : 'ies'})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `shared-memory check FAILED — ${diagnostics.length} diagnostic(s), ${ledgerCount} ledger(s), ${entryCount} entries`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function validateLedger(file, ledger, diagnostics) {
  const waveId = nodeText(ledger.children[1]);
  if (!waveId) {
    addError(diagnostics, file, ledger.loc, '(shared-memory ...) missing wave id as second form');
  } else if (!ID_RE.test(waveId)) {
    addError(
      diagnostics,
      file,
      ledger.children[1].loc,
      `wave id "${waveId}" must match ${ID_RE}`,
    );
  }

  const props = readKeywordProps(ledger, { start: 2 });
  for (const field of HEADER_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        ledger.loc,
        `(shared-memory ${waveId ?? '<missing>'} ...) missing required header ${field}`,
      );
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    addError(
      diagnostics,
      file,
      props[':schema']?.value?.loc ?? ledger.loc,
      `:schema must be "${SCHEMA}", got ${JSON.stringify(schema)}`,
    );
  }

  const createdAt = keywordPropText(props, ':created-at');
  if (createdAt && !ISO8601_RE.test(createdAt)) {
    addError(
      diagnostics,
      file,
      props[':created-at'].value.loc,
      `:created-at "${createdAt}" must be ISO-8601 with timezone (e.g. 2026-04-26T15:30:00Z)`,
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

  // Entries are the ledger's children that are themselves lists with a known
  // entry head. Header keywords (:schema etc) are also children but not lists.
  const entries = ledger.children.filter((c) => isList(c) && ENTRY_HEADS.has(head(c)));
  // Reject any list child whose head is unknown (e.g. typo).
  for (const child of ledger.children) {
    if (!isList(child)) continue;
    const h = head(child);
    if (!ENTRY_HEADS.has(h)) {
      addError(
        diagnostics,
        file,
        child.loc,
        `unknown entry head "${h}"; expected one of ${[...ENTRY_HEADS].join('|')}`,
      );
    }
  }

  const seenIds = new Map();
  let lastSeq = null;
  for (const entry of entries) {
    const result = validateEntry(file, entry, diagnostics);
    if (result.id) {
      if (seenIds.has(result.id)) {
        addError(
          diagnostics,
          file,
          entry.loc,
          `duplicate entry :id "${result.id}" (also at line ${seenIds.get(result.id)})`,
        );
      } else {
        seenIds.set(result.id, entry.loc?.line ?? 1);
      }
    }
    if (result.seq != null) {
      if (lastSeq != null && result.seq <= lastSeq) {
        addError(
          diagnostics,
          file,
          entry.loc,
          `entry :seq ${result.seq} must be strictly greater than previous entry :seq ${lastSeq}`,
        );
      }
      lastSeq = result.seq;
    }
  }

  return entries.length;
}

function validateEntry(file, entry, diagnostics) {
  const kind = head(entry);
  const shape = ENTRY_SHAPES[kind];
  const props = readKeywordProps(entry, { start: 1 });
  for (const field of shape.required) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        entry.loc,
        `(${kind} ...) missing required field ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!shape.required.includes(key) && !shape.optional.includes(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${kind} ...) has unknown field ${key}`,
      );
    }
  }

  const id = keywordPropText(props, ':id');
  if (id != null && !ID_RE.test(id)) {
    addError(
      diagnostics,
      file,
      props[':id'].value.loc,
      `entry :id "${id}" must match ${ID_RE}`,
    );
  }

  const taskId = keywordPropText(props, ':task');
  if (taskId != null && !ID_RE.test(taskId)) {
    addError(
      diagnostics,
      file,
      props[':task'].value.loc,
      `entry :task "${taskId}" must match ${ID_RE}`,
    );
  }

  const agent = keywordPropText(props, ':agent');
  if (agent != null && agent.trim() === '') {
    addError(
      diagnostics,
      file,
      props[':agent'].value.loc,
      'entry :agent must be a non-empty atom or string',
    );
  }

  const summary = keywordPropText(props, ':summary');
  if (summary != null && summary.trim() === '') {
    addError(
      diagnostics,
      file,
      props[':summary'].value.loc,
      'entry :summary must be a non-empty string',
    );
  }
  if (summary != null && summary.includes('\n')) {
    addError(
      diagnostics,
      file,
      props[':summary'].value.loc,
      'entry :summary must be a single line',
    );
  }

  const seqText = keywordPropText(props, ':seq');
  let seq = null;
  if (seqText != null) {
    if (!/^\d+$/.test(seqText)) {
      addError(
        diagnostics,
        file,
        props[':seq'].value.loc,
        `entry :seq must be a non-negative integer, got "${seqText}"`,
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
      `entry :at "${at}" must be ISO-8601 with timezone (e.g. 2026-04-26T15:30:00Z)`,
    );
  }

  if (props[':touched']) {
    const touchedNode = props[':touched'].value;
    if (!isList(touchedNode)) {
      addError(
        diagnostics,
        file,
        touchedNode?.loc ?? entry.loc,
        'entry :touched must be a vector/list of repo-relative path strings',
      );
    } else {
      const touched = nodeToStringArray(touchedNode);
      for (const p of touched) {
        if (path.isAbsolute(p)) {
          addError(
            diagnostics,
            file,
            touchedNode.loc,
            `entry :touched path must be repo-relative, got absolute "${p}"`,
          );
        }
        if (p.split('/').some((seg) => seg === '..')) {
          addError(
            diagnostics,
            file,
            touchedNode.loc,
            `entry :touched path must not traverse upward, got "${p}"`,
          );
        }
        if (p.trim() === '') {
          addError(
            diagnostics,
            file,
            touchedNode.loc,
            'entry :touched path must not be empty',
          );
        }
      }
    }
  }

  if (props[':refs']) {
    const refsNode = props[':refs'].value;
    if (!isList(refsNode)) {
      addError(
        diagnostics,
        file,
        refsNode?.loc ?? entry.loc,
        'entry :refs must be a vector/list of prior entry :id atoms/strings',
      );
    } else {
      const refs = nodeToStringArray(refsNode);
      for (const r of refs) {
        if (!ID_RE.test(r)) {
          addError(
            diagnostics,
            file,
            refsNode.loc,
            `entry :refs id "${r}" must match ${ID_RE}`,
          );
        }
      }
    }
  }

  return { id, seq };
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

function runFixtures() {
  const fixtures = [
    {
      name: 'valid header + bootstrap observation',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (observation
          :id wave19-04-bootstrap-001
          :task wave19-04-shared-memory-ledger-v0
          :agent claudecode
          :seq 1
          :at "2026-04-26T00:00:00Z"
          :touched []
          :summary "bootstrap"))`,
      ok: true,
    },
    {
      name: 'wrong schema',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v0"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1)`,
      ok: false,
    },
    {
      name: 'missing header field',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :sequence 1)`,
      ok: false,
    },
    {
      name: 'unknown entry head',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (chitchat
          :id wave19-04-x
          :task wave19-04
          :agent claudecode
          :seq 1
          :summary "nope"))`,
      ok: false,
    },
    {
      name: 'duplicate entry id',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (observation
          :id wave19-04-dup
          :task wave19-04
          :agent claudecode
          :seq 1
          :summary "first")
        (observation
          :id wave19-04-dup
          :task wave19-04
          :agent claudecode
          :seq 2
          :summary "second"))`,
      ok: false,
    },
    {
      name: 'non-monotonic seq',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (observation
          :id wave19-04-a
          :task wave19-04
          :agent claudecode
          :seq 5
          :summary "first")
        (observation
          :id wave19-04-b
          :task wave19-04
          :agent claudecode
          :seq 5
          :summary "same"))`,
      ok: false,
    },
    {
      name: 'absolute touched path',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (completion
          :id wave19-04-done
          :task wave19-04
          :agent claudecode
          :seq 1
          :touched ["/etc/passwd"]
          :summary "abs path"))`,
      ok: false,
    },
    {
      name: 'parent-traversal touched path',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (completion
          :id wave19-04-traverse
          :task wave19-04
          :agent claudecode
          :seq 1
          :touched ["../outside.rs"]
          :summary "traverse"))`,
      ok: false,
    },
    {
      name: 'bad ISO8601 timestamp',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26 00:00:00"
        :sequence 1)`,
      ok: false,
    },
    {
      name: 'bad entry id style',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (observation
          :id BadCaseID
          :task wave19-04
          :agent claudecode
          :seq 1
          :summary "bad id"))`,
      ok: false,
    },
    {
      name: 'completion missing required :touched',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (completion
          :id wave19-04-done
          :task wave19-04
          :agent claudecode
          :seq 1
          :summary "missing touched"))`,
      ok: false,
    },
    {
      name: 'unknown entry field',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (observation
          :id wave19-04-typo
          :task wave19-04
          :agent claudecode
          :seq 1
          :summary "ok"
          :extra "nope"))`,
      ok: false,
    },
    {
      name: 'correction with refs ok',
      source: `(shared-memory wave19
        :schema "missiond.shared-memory.v1"
        :wave wave19
        :created-at "2026-04-26T00:00:00Z"
        :sequence 1
        (observation
          :id wave19-04-orig
          :task wave19-04
          :agent claudecode
          :seq 1
          :summary "orig")
        (correction
          :id wave19-04-fix
          :task wave19-04
          :agent claudecode
          :seq 2
          :refs [wave19-04-orig]
          :summary "fix"))`,
      ok: true,
    },
  ];

  let failed = 0;
  for (const fixture of fixtures) {
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
      if (isList(form) && head(form) === 'shared-memory') {
        validateLedger(file, form, diagnostics);
      }
    }
    const ok = !diagnostics.some((d) => d.severity === 'error');
    if (ok !== fixture.ok) {
      failed += 1;
      console.error(`fixture failed: ${fixture.name} (expected ok=${fixture.ok}, got ok=${ok})`);
      for (const d of diagnostics) {
        console.error(`  ${d.file}:${d.line}:${d.column}: ${d.message}`);
      }
    }
  }
  if (failed > 0) {
    console.error(`shared-memory fixtures FAILED — ${failed} of ${fixtures.length}`);
    process.exit(1);
  }
  console.log(`shared-memory fixtures OK (${fixtures.length})`);
}

main();
