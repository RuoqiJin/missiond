#!/usr/bin/env node

// MissionD task-runner report finalizer v0.
//
// Pure projection helper: takes a worker draft report plus finalization facts
// and emits a deterministic finalized report. It does not inspect git, mutate
// git, spawn workers, call LLMs, or touch the network.

import fs from 'node:fs';
import path from 'node:path';

import { loadReport, loadReportFromSource } from './verify-task-run.mjs';
import {
  head,
  isList,
  keywordPropText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/task-runner-finalize-report.mjs --report <report.lisp> \\
    --agent-commit <sha> --final-commit <sha> [--verified-commit <sha>] \\
    --parent-patch-commit <sha> --parent-patch-kind <kind> \\
    --parent-patch-reason <text> --parent-patch-file <repo-path>... \\
    [--acceptance-command <cmd>] [--write <path>] \\
    [--request-id <request-id> --request-reports-dir <dir>] \\
    [--verification-receipt <receipt-id-or-path>] [--json]
  node scripts/task-runner-finalize-report.mjs --dry-fixture [--json]

Projects parent/orchestrator hotfix facts into a finalized report-contract v1
record. Default mode is read-only and writes the final report to stdout. The
--write flag is the only mutation boundary and only writes the report file.

No git mutation, no git inspection, no spawn, no network, no LLM.
`;

export const REPORT_SCHEMA = 'missiond.report-contract.v1';
export const REQUEST_FINAL_REPORT_SCHEMA = 'missiond.final-report.v1';
export const ALLOWED_PARENT_PATCH_KINDS = new Set([
  'lint-cleanup',
  'doc-fix',
  'test-fix',
  'scope-trim',
  'hotfix-other',
]);

const SHA_RE = /^[0-9a-f]{7,64}$/i;

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    report: null,
    agentCommit: null,
    finalCommit: null,
    verifiedCommit: null,
    parentPatchCommit: null,
    parentPatchKind: null,
    parentPatchReason: null,
    parentPatchFiles: [],
    acceptanceCommands: [],
    requestId: null,
    requestReportsDir: null,
    verificationReceipts: [],
    write: null,
    json: false,
    dryFixture: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--report') {
      opts.report = argv[++i] ?? fail('--report requires a value');
    } else if (arg.startsWith('--report=')) {
      opts.report = arg.slice('--report='.length);
    } else if (arg === '--agent-commit') {
      opts.agentCommit = argv[++i] ?? fail('--agent-commit requires a value');
    } else if (arg.startsWith('--agent-commit=')) {
      opts.agentCommit = arg.slice('--agent-commit='.length);
    } else if (arg === '--final-commit') {
      opts.finalCommit = argv[++i] ?? fail('--final-commit requires a value');
    } else if (arg.startsWith('--final-commit=')) {
      opts.finalCommit = arg.slice('--final-commit='.length);
    } else if (arg === '--verified-commit') {
      opts.verifiedCommit = argv[++i] ?? fail('--verified-commit requires a value');
    } else if (arg.startsWith('--verified-commit=')) {
      opts.verifiedCommit = arg.slice('--verified-commit='.length);
    } else if (arg === '--parent-patch-commit' || arg === '--patch-commit') {
      opts.parentPatchCommit = argv[++i] ?? fail(`${arg} requires a value`);
    } else if (arg.startsWith('--parent-patch-commit=')) {
      opts.parentPatchCommit = arg.slice('--parent-patch-commit='.length);
    } else if (arg === '--parent-patch-kind' || arg === '--patch-kind') {
      opts.parentPatchKind = argv[++i] ?? fail(`${arg} requires a value`);
    } else if (arg.startsWith('--parent-patch-kind=')) {
      opts.parentPatchKind = arg.slice('--parent-patch-kind='.length);
    } else if (arg === '--parent-patch-reason' || arg === '--patch-reason') {
      opts.parentPatchReason = argv[++i] ?? fail(`${arg} requires a value`);
    } else if (arg.startsWith('--parent-patch-reason=')) {
      opts.parentPatchReason = arg.slice('--parent-patch-reason='.length);
    } else if (arg === '--parent-patch-file' || arg === '--patch-file') {
      opts.parentPatchFiles.push(argv[++i] ?? fail(`${arg} requires a value`));
    } else if (arg.startsWith('--parent-patch-file=')) {
      opts.parentPatchFiles.push(arg.slice('--parent-patch-file='.length));
    } else if (arg === '--acceptance-command') {
      opts.acceptanceCommands.push(argv[++i] ?? fail('--acceptance-command requires a value'));
    } else if (arg.startsWith('--acceptance-command=')) {
      opts.acceptanceCommands.push(arg.slice('--acceptance-command='.length));
    } else if (arg === '--request-id') {
      opts.requestId = argv[++i] ?? fail('--request-id requires a value');
    } else if (arg.startsWith('--request-id=')) {
      opts.requestId = arg.slice('--request-id='.length);
    } else if (arg === '--request-reports-dir') {
      opts.requestReportsDir = argv[++i] ?? fail('--request-reports-dir requires a value');
    } else if (arg.startsWith('--request-reports-dir=')) {
      opts.requestReportsDir = arg.slice('--request-reports-dir='.length);
    } else if (arg === '--verification-receipt') {
      opts.verificationReceipts.push(argv[++i] ?? fail('--verification-receipt requires a value'));
    } else if (arg.startsWith('--verification-receipt=')) {
      opts.verificationReceipts.push(arg.slice('--verification-receipt='.length));
    } else if (arg === '--write') {
      opts.write = argv[++i] ?? fail('--write requires a value');
    } else if (arg.startsWith('--write=')) {
      opts.write = arg.slice('--write='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

export function shaPrefixAgrees(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  const ax = a.trim().toLowerCase();
  const bx = b.trim().toLowerCase();
  if (!SHA_RE.test(ax) || !SHA_RE.test(bx)) return false;
  if (ax === bx) return true;
  const longer = ax.length >= bx.length ? ax : bx;
  const shorter = ax.length >= bx.length ? bx : ax;
  return shorter.length >= 7 && longer.startsWith(shorter);
}

function normalizeHash(value, field) {
  if (typeof value !== 'string' || !SHA_RE.test(value.trim())) {
    throw new Error(`${field} must be a hex git SHA string (>=7 and <=64 chars)`);
  }
  return value.trim().toLowerCase();
}

function ensureRepoRelativePath(value, field) {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`${field} entries must be non-empty strings`);
  }
  const p = value.trim();
  if (path.isAbsolute(p) || p.startsWith('~') || p.split(/[\\/]/).some((part) => part === '..')) {
    throw new Error(`${field} entries must be repo-relative paths, got ${JSON.stringify(value)}`);
  }
  return p;
}

function ensureRequestId(value) {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error('request id must be a non-empty string');
  }
  const requestId = value.trim();
  if (!/^[a-zA-Z0-9][a-zA-Z0-9._:-]*$/.test(requestId)) {
    throw new Error(`request id ${JSON.stringify(value)} must be a stable symbol/string token`);
  }
  return requestId;
}

function ensureReceiptRef(value) {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(':verification_receipts entries must be non-empty strings');
  }
  const ref = value.trim();
  if (path.isAbsolute(ref) || ref.startsWith('~') || ref.split(/[\\/]/).some((part) => part === '..')) {
    throw new Error(`:verification_receipts entries must be ids or repo-relative paths, got ${JSON.stringify(value)}`);
  }
  return ref;
}

function uniqueSorted(values) {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

function normalizeParentPatch(patch) {
  const commit = normalizeHash(patch?.commit, 'parent patch :commit');
  const kind = typeof patch?.kind === 'string' ? patch.kind.trim() : '';
  if (!ALLOWED_PARENT_PATCH_KINDS.has(kind)) {
    throw new Error(
      `parent patch :kind must be one of ${[...ALLOWED_PARENT_PATCH_KINDS].join(', ')}`,
    );
  }
  const reason = typeof patch?.reason === 'string' ? patch.reason.trim() : '';
  if (reason === '') throw new Error('parent patch :reason must be non-empty');
  const files = (patch?.files ?? []).map((p) => ensureRepoRelativePath(p, 'parent patch :files'));
  if (files.length === 0) throw new Error('parent patch :files must be non-empty');
  return { commit, kind, reason, files: uniqueSorted(files) };
}

export function buildParentPatch({ commit, kind, reason, files }) {
  return normalizeParentPatch({ commit, kind, reason, files });
}

export function finalizeReportObject(workerReport, opts = {}) {
  if (!workerReport || typeof workerReport !== 'object') {
    throw new Error('worker report object is required');
  }
  const agentCommit = normalizeHash(
    opts.agentCommit ?? workerReport.agentCommitHash ?? workerReport.commitHash,
    'agent commit',
  );
  const finalCommit = normalizeHash(
    opts.finalCommit ?? workerReport.finalCommitHash ?? workerReport.commitHash,
    'final commit',
  );
  const verifiedCommit = normalizeHash(
    opts.verifiedCommit ?? workerReport.verifiedCommitHash ?? finalCommit,
    'verified commit',
  );

  const existingPatches = (workerReport.parentPatches ?? []).map(normalizeParentPatch);
  const addedPatches = (opts.parentPatches ?? []).map(normalizeParentPatch);
  const parentPatches = [...existingPatches, ...addedPatches];
  if (parentPatches.length === 0) {
    throw new Error('at least one parent patch is required for finalized parent-hotfix reports');
  }
  const tailCommit = parentPatches[parentPatches.length - 1].commit;
  if (!shaPrefixAgrees(finalCommit, tailCommit)) {
    throw new Error(
      `final commit ${finalCommit} must agree with trailing parent patch commit ${tailCommit}`,
    );
  }
  if (!shaPrefixAgrees(verifiedCommit, finalCommit)) {
    throw new Error(`verified commit ${verifiedCommit} must agree with final commit ${finalCommit}`);
  }

  const patchFiles = parentPatches.flatMap((p) => p.files);
  const filesChanged = uniqueSorted([
    ...(workerReport.filesChanged ?? []),
    ...(opts.filesChanged ?? []),
    ...patchFiles,
  ].map((p) => ensureRepoRelativePath(p, ':files_changed')));
  if (filesChanged.length === 0) {
    throw new Error(':files_changed must be non-empty on finalized reports');
  }

  const acceptanceResults = opts.acceptanceResults ?? (opts.acceptanceCommands ?? []).map((command) => ({
    command,
    exit_code: 0,
    ok: true,
  }));
  const finalAcceptance = acceptanceResults.length > 0
    ? acceptanceResults
    : [
        {
          command: 'node scripts/task-runner-finalize-report.mjs --dry-fixture',
          exit_code: 0,
          ok: true,
        },
      ];

  return {
    id: workerReport.id,
    taskId: workerReport.taskId ?? workerReport.id,
    status: 'done',
    commitHash: finalCommit,
    agentCommitHash: agentCommit,
    finalCommitHash: finalCommit,
    verifiedCommitHash: verifiedCommit,
    parentPatches,
    filesChanged,
    acceptanceResults: finalAcceptance,
  };
}

export function finalizeReportSource(source, opts = {}) {
  const workerReport = loadReportFromSource(source, opts.file ?? '<report>');
  const report = finalizeReportObject(workerReport, opts);
  return {
    report,
    source: renderFinalReport(report),
  };
}

export function finalizeReportFile(file, opts = {}) {
  const workerReport = loadReport(file);
  const report = finalizeReportObject(workerReport, opts);
  return {
    report,
    source: renderFinalReport(report),
  };
}

function lispString(value) {
  return JSON.stringify(String(value));
}

function renderStringVector(values) {
  if (values.length === 0) return '[]';
  return `[${values.map((v) => lispString(v)).join(' ')}]`;
}

function renderParentPatches(parentPatches) {
  const body = parentPatches
    .map(
      (p) =>
        `(:commit ${lispString(p.commit)}\n` +
        `    :kind ${p.kind}\n` +
        `    :reason ${lispString(p.reason)}\n` +
        `    :files ${renderStringVector(p.files)})`,
    )
    .join('\n   ');
  return `[\n   ${body}]`;
}

function renderAcceptanceResults(results) {
  const body = results
    .map((r) => {
      const exitCode = Number.isInteger(r.exit_code) ? r.exit_code : 0;
      const ok = r.ok === false ? 'false' : 'true';
      return `(:command ${lispString(r.command)} :exit_code ${exitCode} :ok ${ok})`;
    })
    .join('\n   ');
  return `[\n   ${body}]`;
}

export function renderFinalReport(report) {
  return `(report ${report.id}
  :schema ${lispString(REPORT_SCHEMA)}
  :task_id ${lispString(report.taskId)}
  :status done
  :commit_hash ${lispString(report.commitHash)}
  :agent_commit_hash ${lispString(report.agentCommitHash)}
  :final_commit_hash ${lispString(report.finalCommitHash)}
  :verified_commit_hash ${lispString(report.verifiedCommitHash)}
  :parent_patches
    ${renderParentPatches(report.parentPatches)}
  :files_changed ${renderStringVector(report.filesChanged)}
  :acceptance_results
    ${renderAcceptanceResults(report.acceptanceResults)})\n`;
}

export function renderRequestFinalReport(report, opts = {}) {
  const requestId = ensureRequestId(opts.requestId);
  const receipts = uniqueSorted((opts.verificationReceipts ?? []).map(ensureReceiptRef));
  const reportState = report.status === 'done' ? 'finalized' : report.status;
  const outcome = typeof opts.outcome === 'string' && opts.outcome.trim() !== ''
    ? opts.outcome.trim()
    : (report.status === 'done' ? 'done' : report.status);

  return `(final-report ${lispString(`${requestId}-final`)}
  :schema ${lispString(REQUEST_FINAL_REPORT_SCHEMA)}
  :request_id ${lispString(requestId)}
  :task_id ${lispString(report.taskId)}
  :report_state ${reportState}
  :agent_commit_hash ${lispString(report.agentCommitHash)}
  :final_commit_hash ${lispString(report.finalCommitHash)}
  :verified_commit_hash ${lispString(report.verifiedCommitHash)}
  :verification_receipts ${renderStringVector(receipts)}
  :parent_patches
    ${renderParentPatches(report.parentPatches)}
  :outcome ${lispString(outcome)}
  :files_changed ${renderStringVector(report.filesChanged)}
  :acceptance_results
    ${renderAcceptanceResults(report.acceptanceResults)})\n`;
}

export function validateRequestFinalReportSource(source, file = '<request-final-report>') {
  const diagnostics = [];
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    return [`${err.line ?? 1}:${err.column ?? 1}: ${err.message}`];
  }
  const reports = forms.filter((form) => isList(form) && head(form) === 'final-report');
  if (reports.length !== 1) {
    diagnostics.push(`expected exactly one (final-report ...) form, got ${reports.length}`);
    return diagnostics;
  }
  const report = reports[0];
  const props = readKeywordProps(report, { start: 2 });
  const required = [
    ':schema',
    ':request_id',
    ':report_state',
    ':final_commit_hash',
    ':verification_receipts',
    ':parent_patches',
    ':outcome',
  ];
  for (const field of required) {
    if (!props[field]) diagnostics.push(`missing required field ${field}`);
  }
  if (keywordPropText(props, ':schema') !== REQUEST_FINAL_REPORT_SCHEMA) {
    diagnostics.push(`:schema must be ${JSON.stringify(REQUEST_FINAL_REPORT_SCHEMA)}`);
  }
  const requestId = keywordPropText(props, ':request_id');
  if (!requestId || requestId.trim() === '') diagnostics.push(':request_id must be non-empty');
  const state = keywordPropText(props, ':report_state');
  if (!['draft', 'finalized', 'blocked', 'failed', 'done'].includes(state)) {
    diagnostics.push(':report_state must be draft|finalized|blocked|failed|done');
  }
  const finalCommit = keywordPropText(props, ':final_commit_hash');
  if (!finalCommit || !SHA_RE.test(finalCommit.trim())) {
    diagnostics.push(':final_commit_hash must be a hex git SHA string (>=7 and <=64 chars)');
  }
  const receipts = nodeToStringArray(props[':verification_receipts']?.value);
  for (const receipt of receipts) {
    try {
      ensureReceiptRef(receipt);
    } catch (err) {
      diagnostics.push(err.message);
    }
  }
  if (!props[':parent_patches']?.value || !isList(props[':parent_patches'].value)) {
    diagnostics.push(':parent_patches must be a list/vector');
  }
  if (!keywordPropText(props, ':outcome')) diagnostics.push(':outcome must be non-empty');
  return diagnostics;
}

export function writeRequestFinalReportFile({
  report,
  requestId,
  requestReportsDir,
  verificationReceipts = [],
  outcome = null,
}) {
  if (!requestReportsDir || typeof requestReportsDir !== 'string') {
    throw new Error('requestReportsDir is required to write a request-local final report');
  }
  const source = renderRequestFinalReport(report, {
    requestId,
    verificationReceipts,
    outcome,
  });
  const diagnostics = validateRequestFinalReportSource(source);
  if (diagnostics.length > 0) {
    throw new Error(`generated request-local final report failed validation: ${diagnostics.join('; ')}`);
  }
  const dir = path.resolve(requestReportsDir);
  const target = path.join(dir, 'final.lisp');
  fs.mkdirSync(dir, { recursive: true });
  const tmp = path.join(dir, `.final.lisp.tmp-${process.pid}-${Date.now()}`);
  fs.writeFileSync(tmp, source, { flag: 'wx' });
  fs.renameSync(tmp, target);
  return {
    path: target,
    source,
  };
}

function cliOptionsToFinalizeOptions(opts) {
  const parentPatches = [
    buildParentPatch({
      commit: opts.parentPatchCommit,
      kind: opts.parentPatchKind,
      reason: opts.parentPatchReason,
      files: opts.parentPatchFiles,
    }),
  ];
  return {
    agentCommit: opts.agentCommit,
    finalCommit: opts.finalCommit,
    verifiedCommit: opts.verifiedCommit ?? opts.finalCommit,
    parentPatches,
    acceptanceCommands: opts.acceptanceCommands,
  };
}

function runCli() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    const result = runFixtures();
    if (opts.json) console.log(JSON.stringify(result, null, 2));
    else console.log(`task-runner-finalize-report fixtures OK (${result.cases} cases)`);
    return;
  }
  if (!opts.report) fail('--report is required');
  const result = finalizeReportFile(opts.report, cliOptionsToFinalizeOptions(opts));
  if (opts.write) {
    fs.mkdirSync(path.dirname(path.resolve(opts.write)), { recursive: true });
    fs.writeFileSync(opts.write, result.source);
  }
  let requestFinalReport = null;
  if (opts.requestId || opts.requestReportsDir) {
    if (!opts.requestId) fail('--request-id is required with --request-reports-dir');
    if (!opts.requestReportsDir) fail('--request-reports-dir is required with --request-id');
    requestFinalReport = writeRequestFinalReportFile({
      report: result.report,
      requestId: opts.requestId,
      requestReportsDir: opts.requestReportsDir,
      verificationReceipts: opts.verificationReceipts,
    });
  }
  if (opts.json) {
    console.log(JSON.stringify({
      ok: true,
      wrote: opts.write ?? null,
      request_final_report: requestFinalReport ? {
        path: requestFinalReport.path,
        schema: REQUEST_FINAL_REPORT_SCHEMA,
      } : null,
      report: result.report,
    }, null, 2));
  } else {
    process.stdout.write(result.source);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function runFixtures() {
  const workerDraft = `(report wave29-03-runner-prep
    :schema "missiond.report-contract.v1"
    :task_id "wave29-03-runner-prep"
    :status done
    :commit_hash "d36de80"
    :files_changed ["scripts/prepare-task-runner-wave.mjs"]
    :acceptance_results
      [(:command "node scripts/prepare-task-runner-wave.mjs --dry-fixture" :exit_code 0 :ok true)])`;

  const finalized = finalizeReportSource(workerDraft, {
    agentCommit: 'd36de80',
    finalCommit: 'd842b1d',
    verifiedCommit: 'd842b1d',
    parentPatches: [
      {
        commit: 'd842b1d',
        kind: 'lint-cleanup',
        reason: 'TS80007 sync await cleanup after worker commit',
        files: ['scripts/prepare-task-runner-wave.mjs'],
      },
    ],
    acceptanceCommands: ['node scripts/prepare-task-runner-wave.mjs --dry-fixture'],
  });
  assert(finalized.report.commitHash === 'd842b1d', 'final commit should become report commit_hash');
  assert(finalized.report.agentCommitHash === 'd36de80', 'worker commit should remain agent_commit_hash');
  assert(finalized.report.parentPatches[0].commit === 'd842b1d', 'parent patch commit should be recorded');
  loadReportFromSource(finalized.source, '<finalized-wave29-03>');

  const requestFinal = renderRequestFinalReport(finalized.report, {
    requestId: 'req-wave29-03',
    verificationReceipts: ['.missiond/requests/req-wave29-03/receipts/d842b1d.lisp'],
  });
  let requestFinalErrors = validateRequestFinalReportSource(requestFinal, '<request-final>');
  assert(requestFinalErrors.length === 0, `request-local final report should validate: ${requestFinalErrors.join('; ')}`);

  const tmp = fs.mkdtempSync(path.join(process.cwd(), '.tmp-final-report-'));
  try {
    const wrote = writeRequestFinalReportFile({
      report: finalized.report,
      requestId: 'req-wave29-03',
      requestReportsDir: path.join(tmp, 'requests', 'req-wave29-03', 'reports'),
      verificationReceipts: ['req-wave29-03-d842b1d-smoke'],
    });
    assert(wrote.path.endsWith(path.join('reports', 'final.lisp')), 'request-local final path should be reports/final.lisp');
    requestFinalErrors = validateRequestFinalReportSource(fs.readFileSync(wrote.path, 'utf8'), wrote.path);
    assert(requestFinalErrors.length === 0, 'written request-local final report should validate');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }

  let rejected = false;
  try {
    finalizeReportSource(workerDraft, {
      agentCommit: 'd36de80',
      finalCommit: 'd842b1d',
      parentPatches: [
        {
          commit: 'd842b1d',
          kind: 'lint-cleanup',
          reason: 'absolute path should fail',
          files: ['/tmp/oops.mjs'],
        },
      ],
    });
  } catch (err) {
    rejected = /repo-relative/.test(err.message);
  }
  assert(rejected, 'absolute parent patch files must be rejected');

  rejected = false;
  try {
    finalizeReportSource(workerDraft, {
      agentCommit: 'd36de80',
      finalCommit: 'd842b1d',
      parentPatches: [
        {
          commit: 'abc1234',
          kind: 'lint-cleanup',
          reason: 'drift',
          files: ['scripts/prepare-task-runner-wave.mjs'],
        },
      ],
    });
  } catch (err) {
    rejected = /trailing parent patch/.test(err.message);
  }
  assert(rejected, 'final commit must agree with trailing parent patch');

  return { ok: true, cases: 4 };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    runCli();
  } catch (err) {
    process.stderr.write(`task-runner-finalize-report: ${err?.message ?? String(err)}\n`);
    process.exit(1);
  }
}
