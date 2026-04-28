#!/usr/bin/env node

import path from 'node:path';
import {
  formatLoc,
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  listLispFilesRecursive,
  nodeText,
  nodeToStringArray,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/check-task-contract.mjs [--all] [--json] [--dry-fixture] <task.lisp...>

Checks MissionD task-contract v1 Lisp files:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates each (task <id> ...) form with :schema "missiond.task-contract.v1"
  - requires machine-owned boundaries: :write-scope, :must-not-touch,
    :acceptance, and :commit
  - validates commit policy and basic scope overlaps

Use --all to scan .missiond/tasks/**/*.lisp.
Use --dry-fixture to run self-contained fixtures.
`;

const REQUIRED_TASK_FIELDS = [
  ':schema',
  ':title',
  ':kind',
  ':status',
  ':owner',
  ':goal',
  ':write-scope',
  ':must-not-touch',
  ':acceptance',
  ':commit',
];

const ALLOWED_STATUS = new Set(['draft', 'ready', 'running', 'blocked', 'done', 'archived']);
const ALLOWED_SCOPE_CHECK = new Set(['write-scope-only', 'none', 'not-required']);
const ALLOWED_VERIFICATION_TIER = new Set(['local', 'smoke', 'full']);
const SCHEMA = 'missiond.task-contract.v1';

function main() {
  const args = process.argv.slice(2);
  let all = false;
  let json = false;
  let dryFixture = false;
  const inputs = [];

  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--all') {
      all = true;
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

  const cwd = process.cwd();
  const files = [
    ...(all ? listLispFilesRecursive(path.join(cwd, '.missiond', 'tasks')) : []),
    ...inputs.map((input) => path.resolve(cwd, input)),
  ];

  if (files.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const diagnostics = [];
  let taskCount = 0;

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
      if (!isList(form) || head(form) !== 'task') continue;
      taskCount += 1;
      validateTask(file, form, diagnostics);
    }
  }

  const ok = !diagnostics.some((d) => d.severity === 'error');
  if (json) {
    console.log(JSON.stringify({ ok, files: files.length, tasks: taskCount, diagnostics }, null, 2));
  } else if (ok) {
    console.log(`task-contract check OK (${taskCount} task${taskCount === 1 ? '' : 's'})`);
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(`task-contract check FAILED — ${diagnostics.length} diagnostic(s), ${taskCount} task(s)`);
  }
  process.exit(ok ? 0 : 1);
}

function validateTask(file, task, diagnostics) {
  const id = nodeText(task.children[1]);
  if (!id) {
    addError(diagnostics, file, task.loc, '(task ...) missing id as second form');
  } else if (!/^[a-z0-9][a-z0-9._-]*$/.test(id)) {
    addError(diagnostics, file, task.children[1].loc, `task id "${id}" must be lowercase kebab/dot/underscore id`);
  }

  const props = readKeywordProps(task, { start: 2 });
  for (const field of REQUIRED_TASK_FIELDS) {
    if (!props[field]) {
      addError(diagnostics, file, task.loc, `(task ${id ?? '<missing>'} ...) missing required field ${field}`);
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    addError(diagnostics, file, props[':schema']?.value?.loc ?? task.loc, `:schema must be "${SCHEMA}"`);
  }

  const status = keywordPropText(props, ':status');
  if (status && !ALLOWED_STATUS.has(status)) {
    addError(diagnostics, file, props[':status'].value.loc, `:status "${status}" must be one of ${[...ALLOWED_STATUS].join('|')}`);
  }

  for (const key of [':write-scope', ':acceptance']) {
    const values = nodeToStringArray(props[key]?.value);
    if (values.length === 0) {
      addError(diagnostics, file, props[key]?.value?.loc ?? task.loc, `${key} must be a non-empty string vector/list`);
    }
  }

  const mustNotTouch = nodeToStringArray(props[':must-not-touch']?.value);
  if (!props[':must-not-touch']?.value || !isList(props[':must-not-touch'].value)) {
    addError(diagnostics, file, props[':must-not-touch']?.value?.loc ?? task.loc, ':must-not-touch must be present as a vector/list (may be empty)');
  }

  const writeScope = nodeToStringArray(props[':write-scope']?.value);
  for (const p of [...writeScope, ...mustNotTouch]) {
    if (path.isAbsolute(p)) {
      addError(diagnostics, file, task.loc, `paths must be repo-relative, got absolute path "${p}"`);
    }
  }
  for (const p of writeScope) {
    if (mustNotTouch.includes(p)) {
      addError(diagnostics, file, task.loc, `path "${p}" appears in both :write-scope and :must-not-touch`);
    }
  }

  validateDispatchMetadata(file, task, props, diagnostics);

  validateCommit(file, task, props[':commit']?.value, diagnostics);
}

function validateDispatchMetadata(file, task, props, diagnostics) {
  const verificationTier = keywordPropText(props, ':verification-tier');
  if (verificationTier && !ALLOWED_VERIFICATION_TIER.has(verificationTier)) {
    addError(
      diagnostics,
      file,
      props[':verification-tier'].value.loc,
      `:verification-tier "${verificationTier}" must be one of ${[...ALLOWED_VERIFICATION_TIER].join('|')}`,
    );
  }

  const dispatchGroup = keywordPropText(props, ':dispatch-group');
  if (dispatchGroup && !/^[A-Za-z0-9._-]+$/.test(dispatchGroup)) {
    addError(diagnostics, file, props[':dispatch-group'].value.loc, ':dispatch-group must be a compact id (letters, numbers, dot, underscore, dash)');
  }

  for (const key of [':estimated-minutes', ':heartbeat-minutes']) {
    const value = keywordPropText(props, key);
    if (value != null && !/^[1-9][0-9]*$/.test(value)) {
      addError(diagnostics, file, props[key].value.loc, `${key} must be a positive integer atom`);
    }
  }
}

function validateCommit(file, task, node, diagnostics) {
  if (!isList(node)) {
    addError(diagnostics, file, node?.loc ?? task.loc, ':commit must be a property list');
    return;
  }
  const props = readKeywordProps(node, { start: 0 });
  const required = keywordPropBool(props, ':required');
  if (required == null) {
    addError(diagnostics, file, props[':required']?.value?.loc ?? node.loc, ':commit requires boolean :required');
  }
  const message = keywordPropText(props, ':message');
  if (required === true && (!message || message.trim() === '')) {
    addError(diagnostics, file, props[':message']?.value?.loc ?? node.loc, ':commit :message is required when :required true');
  }
  const scopeCheck = keywordPropText(props, ':scope-check');
  if (!scopeCheck || !ALLOWED_SCOPE_CHECK.has(scopeCheck)) {
    addError(diagnostics, file, props[':scope-check']?.value?.loc ?? node.loc, `:commit :scope-check must be one of ${[...ALLOWED_SCOPE_CHECK].join('|')}`);
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

function runFixtures() {
  const fixtures = [
    {
      name: 'valid minimal task',
      source: `(task wave19-00
        :schema "missiond.task-contract.v1"
        :title "Pilot"
        :kind code-alignment
        :status ready
        :owner "claudecode"
        :goal "prove task contract"
        :write-scope ["scripts/x.mjs"]
        :must-not-touch [".missiond/v2/*.lisp"]
        :acceptance ["git diff --check"]
        :commit (:required true :message "test: pilot" :scope-check write-scope-only))`,
      ok: true,
    },
    {
      name: 'missing acceptance',
      source: `(task wave19-01
        :schema "missiond.task-contract.v1"
        :title "Bad"
        :kind code-alignment
        :status ready
        :owner "claudecode"
        :goal "bad"
        :write-scope ["scripts/x.mjs"]
        :must-not-touch []
        :commit (:required true :message "test: bad" :scope-check write-scope-only))`,
      ok: false,
    },
    {
      name: 'scope overlap',
      source: `(task wave19-02
        :schema "missiond.task-contract.v1"
        :title "Bad"
        :kind code-alignment
        :status ready
        :owner "claudecode"
        :goal "bad"
        :write-scope ["a.rs"]
        :must-not-touch ["a.rs"]
        :acceptance ["git diff --check"]
        :commit (:required true :message "test: bad" :scope-check write-scope-only))`,
      ok: false,
    },
    {
      name: 'valid dispatch metadata',
      source: `(task wave28-01
        :schema "missiond.task-contract.v1"
        :title "Dispatch metadata"
        :kind code-alignment
        :status ready
        :owner "claudecode"
        :dispatch-strategy "fresh-code-alignment"
        :verification-tier smoke
        :dispatch-group "A"
        :estimated-minutes 25
        :heartbeat-minutes 10
        :goal "metadata"
        :write-scope ["scripts/x.mjs"]
        :must-not-touch []
        :acceptance ["git diff --check"]
        :commit (:required true :message "test: metadata" :scope-check write-scope-only))`,
      ok: true,
    },
    {
      name: 'invalid verification tier',
      source: `(task wave28-02
        :schema "missiond.task-contract.v1"
        :title "Bad metadata"
        :kind code-alignment
        :status ready
        :owner "claudecode"
        :verification-tier huge
        :goal "bad"
        :write-scope ["scripts/x.mjs"]
        :must-not-touch []
        :acceptance ["git diff --check"]
        :commit (:required true :message "test: bad" :scope-check write-scope-only))`,
      ok: false,
    },
    {
      name: 'invalid heartbeat',
      source: `(task wave28-03
        :schema "missiond.task-contract.v1"
        :title "Bad heartbeat"
        :kind code-alignment
        :status ready
        :owner "claudecode"
        :heartbeat-minutes 0
        :goal "bad"
        :write-scope ["scripts/x.mjs"]
        :must-not-touch []
        :acceptance ["git diff --check"]
        :commit (:required true :message "test: bad" :scope-check write-scope-only))`,
      ok: false,
    },
  ];

  for (const fixture of fixtures) {
    const file = `<fixture:${fixture.name}>`;
    const diagnostics = [];
    const forms = (awaitImportParseLisp(fixture.source, file));
    for (const form of forms) {
      if (isList(form) && head(form) === 'task') validateTask(file, form, diagnostics);
    }
    const ok = !diagnostics.some((d) => d.severity === 'error');
    if (ok !== fixture.ok) {
      console.error(`fixture failed: ${fixture.name}`);
      for (const d of diagnostics) console.error(`${formatLoc(d.file, d)}: ${d.message}`);
      process.exit(1);
    }
  }
  console.log(`task-contract fixtures OK (${fixtures.length})`);
}

function awaitImportParseLisp(source, file) {
  // Kept as a helper so fixture parsing goes through the same module-level
  // parser without adding async main plumbing.
  const parserModule = { parse: null };
  return globalThis.__missiondParseLisp
    ? globalThis.__missiondParseLisp(source, file)
    : fallbackParse(source, file, parserModule);
}

function fallbackParse(source, file) {
  // Dynamic imports cannot be synchronous, so fixtures use the already-loaded
  // parser via this tiny duplicated call path.
  return readSourceFromString(source, file);
}

function readSourceFromString(source, file) {
  // `readLispFile` reads from disk; this local wrapper mirrors its parser use
  // for fixture strings by relying on the public parse import through a lazy
  // function assignment below.
  return parseFixture(source, file);
}

import { parseLisp as parseFixture } from './lib/missiond_lisp.mjs';

main();
