#!/usr/bin/env node

import path from 'node:path';
import {
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  listLispFilesRecursive,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/check-task-report.mjs [--all] [--json] [--dry-fixture] <report.lisp...>

Checks MissionD report-contract v1 Lisp files:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates each (report <id> ...) form with :schema "missiond.report-contract.v1"
  - rejects: missing :task_id, invalid :status, empty :acceptance_results
    when :status = done, absolute paths in :files_changed / :scope_deviations,
    schema mismatch, and malformed acceptance entries

Use --all to scan .missiond/tasks/**/reports/*.report.lisp.
Use --dry-fixture to run self-contained fixtures.
`;

const SCHEMA = 'missiond.report-contract.v1';
const ALLOWED_STATUS = new Set(['draft', 'in-progress', 'done', 'blocked', 'rejected']);
const REQUIRED_FIELDS = [
  ':schema',
  ':task_id',
  ':status',
  ':commit_hash',
  ':files_changed',
  ':acceptance_results',
];

// wave25-02: router-recommendation enums. Must stay in lockstep with the
// wave24-04 daemon dry-run block (router_policy_dry_run module) and the
// wave24-03 Node CLI (scripts/recommend-task-backend.mjs).
const ALLOWED_ROUTER_BACKENDS = new Set([
  'claudecode',
  'missiond-llm-router',
  'deterministic-checker',
  'patch-worker',
  'verifier-worker',
]);
const ALLOWED_ROUTER_CONFIDENCE = new Set(['high', 'medium', 'low']);

// wave26-04: router-readiness enum. Mirrors the wave26-01 backend-registry
// :readiness_status enum plus the 'unknown' fallback the wave26-02/03
// pipelines emit when the backend is missing from the registry.
const ALLOWED_ROUTER_READINESS = new Set([
  'current-default',
  'advisory-only',
  'runtime-ready',
  'unavailable',
  'unknown',
]);

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
    ...(all ? collectAllReports(cwd) : []),
    ...inputs.map((input) => path.resolve(cwd, input)),
  ];

  if (files.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const diagnostics = [];
  let reportCount = 0;

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
      if (!isList(form) || head(form) !== 'report') continue;
      reportCount += 1;
      validateReport(file, form, diagnostics);
    }
  }

  const ok = !diagnostics.some((d) => d.severity === 'error');
  if (json) {
    console.log(
      JSON.stringify({ ok, files: files.length, reports: reportCount, diagnostics }, null, 2),
    );
  } else if (ok) {
    console.log(`task-report check OK (${reportCount} report${reportCount === 1 ? '' : 's'})`);
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `task-report check FAILED — ${diagnostics.length} diagnostic(s), ${reportCount} report(s)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function collectAllReports(cwd) {
  const root = path.join(cwd, '.missiond', 'tasks');
  return listLispFilesRecursive(root).filter((p) => p.endsWith('.report.lisp'));
}

function validateReport(file, report, diagnostics) {
  const id = nodeText(report.children[1]);
  if (!id) {
    addError(diagnostics, file, report.loc, '(report ...) missing id as second form');
  } else if (!/^[a-z0-9][a-z0-9._-]*$/.test(id)) {
    addError(
      diagnostics,
      file,
      report.children[1].loc,
      `report id "${id}" must be lowercase kebab/dot/underscore id`,
    );
  }

  const props = readKeywordProps(report, { start: 2 });

  for (const field of REQUIRED_FIELDS) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        report.loc,
        `(report ${id ?? '<missing>'} ...) missing required field ${field}`,
      );
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    addError(
      diagnostics,
      file,
      props[':schema']?.value?.loc ?? report.loc,
      `:schema must be "${SCHEMA}"`,
    );
  }

  const taskId = keywordPropText(props, ':task_id');
  if (!taskId || taskId.trim() === '') {
    addError(
      diagnostics,
      file,
      props[':task_id']?.value?.loc ?? report.loc,
      ':task_id must be a non-empty string',
    );
  }

  const status = keywordPropText(props, ':status');
  if (!status || !ALLOWED_STATUS.has(status)) {
    addError(
      diagnostics,
      file,
      props[':status']?.value?.loc ?? report.loc,
      `:status must be one of ${[...ALLOWED_STATUS].join('|')}`,
    );
  }

  const commitHash = keywordPropText(props, ':commit_hash');
  if (status === 'done' && (!commitHash || commitHash.trim() === '')) {
    addError(
      diagnostics,
      file,
      props[':commit_hash']?.value?.loc ?? report.loc,
      ':commit_hash must be a non-empty string when :status = done',
    );
  }

  const filesChanged = nodeToStringArray(props[':files_changed']?.value);
  if (!props[':files_changed']?.value || !isList(props[':files_changed'].value)) {
    addError(
      diagnostics,
      file,
      props[':files_changed']?.value?.loc ?? report.loc,
      ':files_changed must be a vector/list (may be empty for non-done status)',
    );
  }
  if (status === 'done' && filesChanged.length === 0) {
    addError(
      diagnostics,
      file,
      props[':files_changed']?.value?.loc ?? report.loc,
      ':files_changed must be non-empty when :status = done',
    );
  }
  for (const p of filesChanged) {
    if (path.isAbsolute(p) || p.startsWith('~')) {
      addError(
        diagnostics,
        file,
        props[':files_changed']?.value?.loc ?? report.loc,
        `:files_changed paths must be repo-relative, got "${p}"`,
      );
    }
  }

  validateAcceptance(file, report, props[':acceptance_results']?.value, status, diagnostics);
  validateScopeDeviations(file, report, props[':scope_deviations']?.value, diagnostics);

  // wave23-02: optional worker-explanation fields. Structural-only checks —
  // prose content is never treated as ground truth (facts live in
  // .missiond/tasks/<wave>/session-trace.lisp). Each field is optional; when
  // present it must be a vector. Each entry is either a plain string OR a
  // property list whose declared key is present and is a non-empty string.
  validateExplanationField(
    file,
    report,
    props[':time_sinks']?.value,
    ':time_sinks',
    ':label',
    diagnostics,
  );
  validateExplanationField(
    file,
    report,
    props[':major_decisions']?.value,
    ':major_decisions',
    ':decision',
    diagnostics,
  );
  validateExplanationField(
    file,
    report,
    props[':unexpected_work']?.value,
    ':unexpected_work',
    ':summary',
    diagnostics,
  );
  validateExplanationField(
    file,
    report,
    props[':blockers']?.value,
    ':blockers',
    ':summary',
    diagnostics,
  );
  validateTraceRefs(file, report, props[':trace_refs']?.value, diagnostics);

  // wave25-02: optional router-recommendation fields. Mirror the wave24-04
  // daemon dry-run block on the report side so completion reports can RECORD
  // what the dry-run router recommended without that recommendation ever
  // being treated as authoritative. All seven fields are optional; when
  // present, each is validated structurally:
  //   :recommended_backend   — enum (5 backend classes)
  //   :router_confidence     — enum (high|medium|low)
  //   :router_policy_path    — repo-relative string
  //   :router_dry_run_only   — literal atom true (cross-wave invariant)
  //   :router_applied        — literal atom false (cross-wave invariant)
  //   :router_reasons        — vector of non-empty strings
  //   :router_trace_index_path — repo-relative string
  validateRouterEnumField(
    file,
    report,
    props[':recommended_backend']?.value,
    ':recommended_backend',
    ALLOWED_ROUTER_BACKENDS,
    diagnostics,
  );
  validateRouterEnumField(
    file,
    report,
    props[':router_confidence']?.value,
    ':router_confidence',
    ALLOWED_ROUTER_CONFIDENCE,
    diagnostics,
  );
  validateRouterRepoRelativePath(
    file,
    report,
    props[':router_policy_path']?.value,
    ':router_policy_path',
    diagnostics,
  );
  validateRouterRepoRelativePath(
    file,
    report,
    props[':router_trace_index_path']?.value,
    ':router_trace_index_path',
    diagnostics,
  );
  validateRouterLiteralBool(
    file,
    report,
    props[':router_dry_run_only']?.value,
    ':router_dry_run_only',
    'true',
    diagnostics,
  );
  validateRouterLiteralBool(
    file,
    report,
    props[':router_applied']?.value,
    ':router_applied',
    'false',
    diagnostics,
  );
  validateRouterReasons(file, report, props[':router_reasons']?.value, diagnostics);

  // wave26-04: optional router-readiness fields. Mirror the wave26-02
  // (Node CLI) and wave26-03 (daemon plan dry_run) backend readiness
  // annotations on the report side so completion reports can RECORD what
  // the readiness pipeline observed without that record ever being treated
  // as runtime authoritative. All five fields are optional; when present,
  // each is validated structurally:
  //   :router_backend_readiness_status — enum (5 readiness classes)
  //   :router_backend_runtime_allowed  — literal atom true|false
  //   :router_apply_eligible           — literal atom true|false
  //   :router_apply_blockers           — vector of non-empty strings
  //   :router_backend_registry_path    — repo-relative string
  validateRouterEnumField(
    file,
    report,
    props[':router_backend_readiness_status']?.value,
    ':router_backend_readiness_status',
    ALLOWED_ROUTER_READINESS,
    diagnostics,
  );
  validateRouterLiteralBoolEither(
    file,
    report,
    props[':router_backend_runtime_allowed']?.value,
    ':router_backend_runtime_allowed',
    diagnostics,
  );
  validateRouterLiteralBoolEither(
    file,
    report,
    props[':router_apply_eligible']?.value,
    ':router_apply_eligible',
    diagnostics,
  );
  validateRouterApplyBlockers(
    file,
    report,
    props[':router_apply_blockers']?.value,
    diagnostics,
  );
  validateRouterRepoRelativePath(
    file,
    report,
    props[':router_backend_registry_path']?.value,
    ':router_backend_registry_path',
    diagnostics,
  );
}

// wave26-04 helper: STRICT literal-atom validator that accepts either
// `true` OR `false` (cross-wave invariant — strings are rejected). Tight
// wrapper around the wave25-02 validateRouterLiteralBool helper which only
// accepts a single fixed atom; the wave26-04 readiness fields legitimately
// carry both polarities (e.g. runtime_allowed=true for claudecode default,
// false for advisory-only backends; apply_eligible=false in seed today,
// true once a runtime adapter ships).
function validateRouterLiteralBoolEither(file, report, node, fieldName, diagnostics) {
  if (node == null) return; // optional
  if (node.type !== 'atom' || (node.value !== 'true' && node.value !== 'false')) {
    const got = node.type === 'atom' ? node.value : `<${node.type}>`;
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must be the literal atom true or false (cross-wave invariant), got ${got}`,
    );
  }
}

// wave26-04 helper: :router_apply_blockers must be a vector of non-empty
// strings — mirrors the wave25-02 validateRouterReasons pattern. Each
// entry names a concrete reason live promotion is rejected; empty strings
// are rejected because they carry no signal and almost always indicate a
// templating bug. Property-list entries are not allowed — wave26-01
// emits plain strings, wave26-02/03 splice in plain strings.
function validateRouterApplyBlockers(file, report, node, diagnostics) {
  if (node == null) return; // optional
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      ':router_apply_blockers must be a vector/list when present',
    );
    return;
  }
  for (const entry of node.children) {
    const text = nodeText(entry);
    if (text == null) {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        ':router_apply_blockers entries must be strings',
      );
      continue;
    }
    if (text.trim() === '') {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        ':router_apply_blockers entries must be non-empty strings',
      );
    }
  }
}

// wave25-02 helper: enum validator for the router fields. The value must be
// a single string/atom node whose text is one of the allowed members. Empty
// strings and non-string node types (lists, missing) are rejected.
function validateRouterEnumField(file, report, node, fieldName, allowed, diagnostics) {
  if (node == null) return; // optional
  const text = nodeText(node);
  if (text == null || text.trim() === '') {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must be a non-empty string when present`,
    );
    return;
  }
  if (!allowed.has(text)) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must be one of {${[...allowed].join(', ')}}, got "${text}"`,
    );
  }
}

// wave25-02 helper: repo-relative path validator for :router_policy_path and
// :router_trace_index_path. Mirrors the :files_changed / :trace_refs
// conventions: rejects absolute paths, leading "~", and "../" traversal.
function validateRouterRepoRelativePath(file, report, node, fieldName, diagnostics) {
  if (node == null) return; // optional
  const text = nodeText(node);
  if (text == null || text.trim() === '') {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must be a non-empty string when present`,
    );
    return;
  }
  if (path.isAbsolute(text) || text.startsWith('~')) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must be repo-relative, got "${text}"`,
    );
    return;
  }
  // Reject any "../" traversal segment to mirror the strict trace-index
  // path rule (the daemon never reads outside the repo).
  const segments = text.split(/[\\/]/);
  if (segments.some((seg) => seg === '..')) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must not contain ".." traversal, got "${text}"`,
    );
  }
}

// wave25-02 helper: STRICT literal-atom validator. Unlike keywordPropBool
// (which coerces "true"/"yes"/"on" to true), this validator demands the
// exact bareword atom — so :router_applied "false" (a string) and
// :router_dry_run_only "yes" (string coercion) are both rejected. The
// cross-wave invariant requires the exact daemon-emitted boolean shape.
function validateRouterLiteralBool(file, report, node, fieldName, expectedAtom, diagnostics) {
  if (node == null) return; // optional
  if (node.type !== 'atom' || node.value !== expectedAtom) {
    const got = node.type === 'atom' ? node.value : `<${node.type}>`;
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must be the literal atom ${expectedAtom} (cross-wave invariant), got ${got}`,
    );
  }
}

// wave25-02 helper: :router_reasons must be a vector of non-empty strings.
// Mirrors the daemon block's reasons array (matched rule ids / fallback
// notes / rejection messages). Property-list entries are not allowed —
// the daemon emits plain strings.
function validateRouterReasons(file, report, node, diagnostics) {
  if (node == null) return; // optional
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      ':router_reasons must be a vector/list when present',
    );
    return;
  }
  for (const entry of node.children) {
    const text = nodeText(entry);
    if (text == null) {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        ':router_reasons entries must be strings',
      );
      continue;
    }
    if (text.trim() === '') {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        ':router_reasons entries must be non-empty strings',
      );
    }
  }
}

// Structural validator for prose-only worker-explanation fields. Each entry
// may be a plain string OR a property list. When it is a property list, the
// caller-supplied requiredKey (e.g. :label, :decision, :summary) must be
// present and non-empty. Empty strings are rejected because they carry no
// information and almost always indicate a templating bug.
function validateExplanationField(file, report, node, fieldName, requiredKey, diagnostics) {
  if (node == null) return; // optional
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      `${fieldName} must be a vector/list when present`,
    );
    return;
  }
  for (const entry of node.children) {
    if (entry.type === 'string' || entry.type === 'atom') {
      const text = nodeText(entry);
      if (!text || text.trim() === '') {
        addError(
          diagnostics,
          file,
          entry.loc ?? node.loc,
          `${fieldName} string entries must be non-empty`,
        );
      }
      continue;
    }
    if (!isList(entry)) {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        `${fieldName} entries must be strings or property lists`,
      );
      continue;
    }
    const ep = readKeywordProps(entry, { start: 0 });
    const value = keywordPropText(ep, requiredKey);
    if (!value || value.trim() === '') {
      addError(
        diagnostics,
        file,
        ep[requiredKey]?.value?.loc ?? entry.loc,
        `${fieldName} property-list entry missing ${requiredKey}`,
      );
    }
  }
}

// :trace_refs is a vector of strings: either session-trace event ids
// (kebab-id) or repo-relative paths to trace files. Absolute paths are
// rejected to mirror :files_changed / :scope_deviations conventions.
function validateTraceRefs(file, report, node, diagnostics) {
  if (node == null) return; // optional
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      ':trace_refs must be a vector/list when present',
    );
    return;
  }
  for (const entry of node.children) {
    const text = nodeText(entry);
    if (text == null) {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        ':trace_refs entries must be strings (event id or repo-relative path)',
      );
      continue;
    }
    if (text.trim() === '') {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        ':trace_refs entries must be non-empty strings',
      );
      continue;
    }
    if (path.isAbsolute(text) || text.startsWith('~')) {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        `:trace_refs paths must be repo-relative, got "${text}"`,
      );
    }
  }
}

function validateAcceptance(file, report, node, status, diagnostics) {
  const loc = node?.loc ?? report.loc;
  if (node == null) {
    return; // already reported as missing required field
  }
  if (!isList(node)) {
    addError(diagnostics, file, loc, ':acceptance_results must be a vector/list');
    return;
  }
  const entries = node.children;
  if (status === 'done' && entries.length === 0) {
    addError(
      diagnostics,
      file,
      loc,
      ':acceptance_results must be non-empty when :status = done',
    );
    return;
  }
  for (const entry of entries) {
    if (!isList(entry)) {
      addError(
        diagnostics,
        file,
        entry.loc ?? loc,
        ':acceptance_results entries must be property lists',
      );
      continue;
    }
    const ep = readKeywordProps(entry, { start: 0 });
    const command = keywordPropText(ep, ':command');
    if (!command || command.trim() === '') {
      addError(
        diagnostics,
        file,
        ep[':command']?.value?.loc ?? entry.loc,
        'acceptance entry missing :command',
      );
    }
    const exitNode = ep[':exit_code']?.value;
    const exitText = nodeText(exitNode);
    if (exitText == null || !/^-?\d+$/.test(exitText)) {
      addError(
        diagnostics,
        file,
        exitNode?.loc ?? entry.loc,
        'acceptance entry :exit_code must be an integer',
      );
    }
    const okBool = keywordPropBool(ep, ':ok');
    if (okBool == null) {
      addError(
        diagnostics,
        file,
        ep[':ok']?.value?.loc ?? entry.loc,
        'acceptance entry :ok must be true|false',
      );
    }
  }
}

function validateScopeDeviations(file, report, node, diagnostics) {
  if (node == null) return; // optional field
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node.loc ?? report.loc,
      ':scope_deviations must be a vector/list',
    );
    return;
  }
  for (const entry of node.children) {
    if (!isList(entry)) {
      addError(
        diagnostics,
        file,
        entry.loc ?? node.loc,
        ':scope_deviations entries must be property lists',
      );
      continue;
    }
    const ep = readKeywordProps(entry, { start: 0 });
    const pth = keywordPropText(ep, ':path');
    if (!pth || pth.trim() === '') {
      addError(
        diagnostics,
        file,
        ep[':path']?.value?.loc ?? entry.loc,
        'scope_deviations entry missing :path',
      );
    } else if (path.isAbsolute(pth) || pth.startsWith('~')) {
      addError(
        diagnostics,
        file,
        ep[':path']?.value?.loc ?? entry.loc,
        `scope_deviations :path must be repo-relative, got "${pth}"`,
      );
    }
    const reason = keywordPropText(ep, ':reason');
    if (!reason || reason.trim() === '') {
      addError(
        diagnostics,
        file,
        ep[':reason']?.value?.loc ?? entry.loc,
        'scope_deviations entry missing :reason',
      );
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

function runFixtures() {
  const fixtures = [
    {
      name: 'valid done report',
      source: `(report wave19-fix-ok
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-ok"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "node scripts/x.mjs --check" :exit_code 0 :ok true)]
        :scope_deviations []
        :notes "fixture ok")`,
      ok: true,
    },
    {
      name: 'valid draft report (acceptance may be empty)',
      source: `(report wave19-fix-draft
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-draft"
        :status draft
        :commit_hash ""
        :files_changed []
        :acceptance_results [])`,
      ok: true,
    },
    {
      name: 'missing task_id',
      source: `(report wave19-fix-missing-task-id
        :schema "missiond.report-contract.v1"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)])`,
      ok: false,
      expects: /missing required field :task_id/,
    },
    {
      name: 'invalid status',
      source: `(report wave19-fix-bad-status
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-bad-status"
        :status finished
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)])`,
      ok: false,
      expects: /:status must be one of/,
    },
    {
      name: 'empty acceptance_results when done',
      source: `(report wave19-fix-empty-accept
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-empty-accept"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results [])`,
      ok: false,
      expects: /:acceptance_results must be non-empty when :status = done/,
    },
    {
      name: 'absolute path in files_changed',
      source: `(report wave19-fix-abs-path
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-abs-path"
        :status done
        :commit_hash "abc1234"
        :files_changed ["/etc/passwd"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)])`,
      ok: false,
      expects: /:files_changed paths must be repo-relative/,
    },
    {
      name: 'malformed acceptance entry (missing :ok)',
      source: `(report wave19-fix-bad-accept
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-bad-accept"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0)])`,
      ok: false,
      expects: /acceptance entry :ok must be true\|false/,
    },
    {
      name: 'schema mismatch',
      source: `(report wave19-fix-bad-schema
        :schema "missiond.task-contract.v1"
        :task_id "wave19-fix-bad-schema"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)])`,
      ok: false,
      expects: /:schema must be "missiond\.report-contract\.v1"/,
    },
    {
      name: 'done without commit_hash',
      source: `(report wave19-fix-no-commit
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-no-commit"
        :status done
        :commit_hash ""
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)])`,
      ok: false,
      expects: /:commit_hash must be a non-empty string when :status = done/,
    },
    {
      name: 'scope_deviations with absolute path',
      source: `(report wave19-fix-bad-deviation
        :schema "missiond.report-contract.v1"
        :task_id "wave19-fix-bad-deviation"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :scope_deviations
          [(:path "/tmp/leak" :reason "debug")])`,
      ok: false,
      expects: /scope_deviations :path must be repo-relative/,
    },
    {
      name: 'valid done report with all worker-explanation fields',
      source: `(report wave23-fix-explain-ok
        :schema "missiond.report-contract.v1"
        :task_id "wave23-fix-explain-ok"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "node scripts/x.mjs --check" :exit_code 0 :ok true)]
        :time_sinks
          ["debugging schema parser"
           (:label "writing fixtures" :duration_ms 900000 :notes "edge cases")]
        :major_decisions
          ["chose contract-level opt-in symbol :session-trace-writable"
           (:decision "validate explanation fields structurally only"
            :rationale "facts live in session-trace; report stays prose-only"
            :trace_ref "wave23-trace-bootstrap-001")]
        :unexpected_work
          ["had to re-render wave23-01 brief"
           (:summary "discovered renderer needed sibling-file detection" :trace_ref "wave23-trace-render-001")]
        :blockers
          ["none"
           (:summary "parallel agent claim conflict" :resolved true)]
        :trace_refs
          ["wave23-trace-bootstrap-001"
           ".missiond/tasks/wave23/session-trace.lisp"])`,
      ok: true,
    },
    {
      name: 'time_sinks not a vector',
      source: `(report wave23-fix-explain-bad-shape
        :schema "missiond.report-contract.v1"
        :task_id "wave23-fix-explain-bad-shape"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :time_sinks "should be a vector")`,
      ok: false,
      expects: /:time_sinks must be a vector\/list when present/,
    },
    {
      name: 'major_decisions entry missing :decision key',
      source: `(report wave23-fix-explain-missing-key
        :schema "missiond.report-contract.v1"
        :task_id "wave23-fix-explain-missing-key"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :major_decisions
          [(:rationale "no decision atom")])`,
      ok: false,
      expects: /:major_decisions property-list entry missing :decision/,
    },
    {
      name: 'unexpected_work property list missing :summary',
      source: `(report wave23-fix-explain-unexpected
        :schema "missiond.report-contract.v1"
        :task_id "wave23-fix-explain-unexpected"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :unexpected_work
          [(:trace_ref "wave23-trace-001")])`,
      ok: false,
      expects: /:unexpected_work property-list entry missing :summary/,
    },
    {
      name: 'blockers empty string entry rejected',
      source: `(report wave23-fix-explain-blockers
        :schema "missiond.report-contract.v1"
        :task_id "wave23-fix-explain-blockers"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :blockers [""])`,
      ok: false,
      expects: /:blockers string entries must be non-empty/,
    },
    {
      name: 'trace_refs absolute path rejected',
      source: `(report wave23-fix-trace-refs-abs
        :schema "missiond.report-contract.v1"
        :task_id "wave23-fix-trace-refs-abs"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :trace_refs ["/etc/passwd"])`,
      ok: false,
      expects: /:trace_refs paths must be repo-relative/,
    },
    // wave25-02: router-recommendation field fixtures (legacy + 5 negatives).
    {
      name: 'wave25-02 legacy report (no router fields) — backward compat',
      source: `(report wave25-fix-router-legacy
        :schema "missiond.report-contract.v1"
        :task_id "wave25-fix-router-legacy"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "node scripts/x.mjs --check" :exit_code 0 :ok true)])`,
      ok: true,
    },
    {
      name: 'wave25-02 valid router-recommendation block (all fields)',
      source: `(report wave25-fix-router-ok
        :schema "missiond.report-contract.v1"
        :task_id "wave25-fix-router-ok"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :recommended_backend "claudecode"
        :router_confidence "medium"
        :router_policy_path ".missiond/router/router-policy-v1.lisp"
        :router_dry_run_only true
        :router_applied false
        :router_reasons ["matched-rule:claudecode-default" "priority:50"]
        :router_trace_index_path ".missiond/v2/index/session-trace-index.json")`,
      ok: true,
    },
    {
      name: 'wave25-02 invalid backend rejected',
      source: `(report wave25-fix-router-bad-backend
        :schema "missiond.report-contract.v1"
        :task_id "wave25-fix-router-bad-backend"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :recommended_backend "gpt-5"
        :router_confidence "high"
        :router_policy_path ".missiond/router/router-policy-v1.lisp"
        :router_dry_run_only true
        :router_applied false)`,
      ok: false,
      expects: /:recommended_backend must be one of/,
    },
    {
      name: 'wave25-02 router_applied=true rejected (cross-wave invariant)',
      source: `(report wave25-fix-router-applied-true
        :schema "missiond.report-contract.v1"
        :task_id "wave25-fix-router-applied-true"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :recommended_backend "claudecode"
        :router_confidence "medium"
        :router_policy_path ".missiond/router/router-policy-v1.lisp"
        :router_dry_run_only true
        :router_applied true)`,
      ok: false,
      expects: /:router_applied must be the literal atom false/,
    },
    {
      name: 'wave25-02 router_dry_run_only=false rejected (cross-wave invariant)',
      source: `(report wave25-fix-router-dry-run-false
        :schema "missiond.report-contract.v1"
        :task_id "wave25-fix-router-dry-run-false"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :recommended_backend "claudecode"
        :router_confidence "low"
        :router_policy_path ".missiond/router/router-policy-v1.lisp"
        :router_dry_run_only false
        :router_applied false)`,
      ok: false,
      expects: /:router_dry_run_only must be the literal atom true/,
    },
    {
      name: 'wave25-02 absolute router_policy_path rejected',
      source: `(report wave25-fix-router-abs-policy
        :schema "missiond.report-contract.v1"
        :task_id "wave25-fix-router-abs-policy"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :recommended_backend "claudecode"
        :router_confidence "medium"
        :router_policy_path "/etc/router-policy.lisp"
        :router_dry_run_only true
        :router_applied false)`,
      ok: false,
      expects: /:router_policy_path must be repo-relative/,
    },
    // wave25-05: cross-layer measurement smoke positive fixture. Cross-pins
    // the deterministic-checker bucket the wave25-05 recommend-task-backend
    // parity fixture also drives (Layer A): a code-alignment task that
    // matches a deterministic-checker rule with a rich trace history (>=5
    // events) lands `:router_confidence high` + `:recommended_backend
    // deterministic-checker`. The :router_applied false / :router_dry_run_only
    // true literals are the cross-wave invariants the daemon's
    // applied_remains_false_with_trace_index test re-pins for every status
    // flavour. This fixture proves the report contract STILL ACCEPTS a
    // valid wave25 router-recommendation block end-to-end (the earlier
    // wave25-02 valid fixture used claudecode + medium; this one widens
    // coverage to the second backend in the wave25-05 parity grid).
    {
      name: 'wave25-05 valid router-recommendation block (deterministic-checker + high)',
      source: `(report wave25-fix-router-deterministic-high
        :schema "missiond.report-contract.v1"
        :task_id "wave25-fix-router-deterministic-high"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "node scripts/x.mjs --check" :exit_code 0 :ok true)]
        :recommended_backend "deterministic-checker"
        :router_confidence "high"
        :router_policy_path ".missiond/router/router-policy-v1.lisp"
        :router_dry_run_only true
        :router_applied false
        :router_reasons ["matched-rule:r-deterministic-checker-tasks"
                         "trace-events:7"]
        :router_trace_index_path ".missiond/v2/index/session-trace-index.json")`,
      ok: true,
    },
    // wave26-04: router-readiness field fixtures (legacy + valid + 5
    // negatives). Backward-compat is non-negotiable — the legacy fixture
    // re-pins that reports without any of the 5 new fields stay valid.
    // The valid fixture mirrors the wave26-01 seed shape: claudecode is
    // current-default with runtime_allowed=true but apply_eligible=false
    // (current-default alone is NEVER sufficient for the apply gate;
    // explicit runtime-ready opt-in is required upstream — see wave26-02
    // and wave26-03 strict gates).
    {
      name: 'wave26-04 legacy report (no readiness fields) — backward compat',
      source: `(report wave26-fix-readiness-legacy
        :schema "missiond.report-contract.v1"
        :task_id "wave26-fix-readiness-legacy"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "node scripts/x.mjs --check" :exit_code 0 :ok true)])`,
      ok: true,
    },
    {
      name: 'wave26-04 valid readiness block (all 5 fields, claudecode seed shape)',
      source: `(report wave26-fix-readiness-ok
        :schema "missiond.report-contract.v1"
        :task_id "wave26-fix-readiness-ok"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "node scripts/x.mjs --check" :exit_code 0 :ok true)]
        :recommended_backend "claudecode"
        :router_confidence "high"
        :router_policy_path ".missiond/router/router-policy-v1.lisp"
        :router_dry_run_only true
        :router_applied false
        :router_backend_readiness_status "current-default"
        :router_backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers
          ["apply gate requires runtime-ready; current-default is NOT sufficient"
           "explicit runtime-ready opt-in required upstream before live promotion"]
        :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: true,
    },
    {
      name: 'wave26-04 invalid router_backend_readiness_status rejected',
      source: `(report wave26-fix-readiness-bad-status
        :schema "missiond.report-contract.v1"
        :task_id "wave26-fix-readiness-bad-status"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :router_backend_readiness_status "unknown-status")`,
      ok: false,
      expects: /:router_backend_readiness_status must be one of/,
    },
    {
      name: 'wave26-04 router_apply_eligible="false" string rejected (cross-wave invariant)',
      source: `(report wave26-fix-readiness-eligible-string
        :schema "missiond.report-contract.v1"
        :task_id "wave26-fix-readiness-eligible-string"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :router_apply_eligible "false")`,
      ok: false,
      expects: /:router_apply_eligible must be the literal atom true or false/,
    },
    {
      name: 'wave26-04 router_backend_runtime_allowed="true" string rejected (cross-wave invariant)',
      source: `(report wave26-fix-readiness-runtime-string
        :schema "missiond.report-contract.v1"
        :task_id "wave26-fix-readiness-runtime-string"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :router_backend_runtime_allowed "true")`,
      ok: false,
      expects: /:router_backend_runtime_allowed must be the literal atom true or false/,
    },
    {
      name: 'wave26-04 absolute router_backend_registry_path rejected',
      source: `(report wave26-fix-readiness-abs-registry
        :schema "missiond.report-contract.v1"
        :task_id "wave26-fix-readiness-abs-registry"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :router_backend_registry_path "/abs/path/registry.lisp")`,
      ok: false,
      expects: /:router_backend_registry_path must be repo-relative/,
    },
    {
      name: 'wave26-04 empty string in router_apply_blockers rejected',
      source: `(report wave26-fix-readiness-empty-blocker
        :schema "missiond.report-contract.v1"
        :task_id "wave26-fix-readiness-empty-blocker"
        :status done
        :commit_hash "abc1234"
        :files_changed ["scripts/x.mjs"]
        :acceptance_results
          [(:command "echo ok" :exit_code 0 :ok true)]
        :router_apply_blockers ["valid blocker" ""])`,
      ok: false,
      expects: /:router_apply_blockers entries must be non-empty strings/,
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
    }
    if (forms) {
      for (const form of forms) {
        if (isList(form) && head(form) === 'report') validateReport(file, form, diagnostics);
      }
    }
    const ok = !diagnostics.some((d) => d.severity === 'error');
    const messages = diagnostics.map((d) => d.message).join(' | ');
    const expectMatched = fixture.expects ? fixture.expects.test(messages) : true;
    if (ok !== fixture.ok || !expectMatched) {
      failed += 1;
      console.error(`fixture FAILED: ${fixture.name}`);
      console.error(`  expected ok=${fixture.ok}, got ok=${ok}`);
      if (fixture.expects && !expectMatched) {
        console.error(`  expected diagnostic matching ${fixture.expects}`);
      }
      for (const d of diagnostics) {
        console.error(`    ${d.file}:${d.line}:${d.column}: ${d.message}`);
      }
    } else {
      console.log(`fixture OK: ${fixture.name}`);
    }
  }

  if (failed > 0) {
    console.error(`task-report fixtures FAILED — ${failed}/${fixtures.length} failed`);
    process.exit(1);
  }
  console.log(`task-report fixtures OK (${fixtures.length})`);
}

main();
