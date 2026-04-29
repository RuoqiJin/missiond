#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-task-lifecycle-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 task-runner lifecycle Lisp/code isomorphism contract:
  - V3 blueprint names the real task-runner lifecycle/finalizer scripts.
  - lifecycle events are validated before append/projection.
  - append-event owns cooperative sequence allocation and atomic ledger writes.
  - append-event can project request-local one-event files when request ids are supplied.
  - finalizer can project request-local final.lisp reports when request ids are supplied.
  - dispatch/submit paths pass request-local lifecycle projection args into append-event.
  - parent-hotfix and final-report projection preserve lineage fields.
  - verification-receipt checker exposes a request-local single-receipt
    writer/projection that validates rendered Lisp bytes through the existing
    structural checker before atomic create-only rename, while legacy task-
    scoped (verification-receipt-set ...) Lisp files remain compatibility
    inputs.
  - scheduler and batch verifier consume lifecycle/finalization projections.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  lifecycleSchema: '.missiond/tasks/schema/task-lifecycle-event-v1.lisp',
  reportSchema: '.missiond/tasks/schema/report-contract-v1.lisp',
  lifecycleChecker: 'scripts/check-task-lifecycle-events.mjs',
  appendEvent: 'scripts/task-runner-append-event.mjs',
  finalizer: 'scripts/task-runner-finalize-report.mjs',
  parentHotfix: 'scripts/task-runner-parent-hotfix.mjs',
  projector: 'scripts/project-task-lifecycle-ledger.mjs',
  nextAction: 'scripts/task-runner-next-action.mjs',
  dispatch: 'scripts/task-runner-dispatch.mjs',
  submitDispatch: 'scripts/task-runner-submit-dispatch.mjs',
  waveState: 'scripts/task-runner-wave-state.mjs',
  receiptChecker: 'scripts/check-verification-receipt.mjs',
  batchVerifier: 'scripts/verify-task-runner-batch.mjs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 task lifecycle Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 task lifecycle Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'orchestrator-final-truth',
    'event-sourced-runtime',
    'parent-hotfix-finalization',
    'surface task-runner-cli',
    ':status "code-aligned"',
    'scripts/check-task-lifecycle-events.mjs',
    'scripts/task-runner-append-event.mjs',
    'scripts/task-runner-finalize-report.mjs',
    'scripts/task-runner-parent-hotfix.mjs',
    'scripts/project-task-lifecycle-ledger.mjs',
    'scripts/task-runner-dispatch.mjs',
    'scripts/task-runner-submit-dispatch.mjs',
    'scripts/check-verification-receipt.mjs',
    'scripts/verify-task-runner-batch.mjs',
    'Task-scoped lifecycle events are first-class one-event files',
    '.missiond/tasks/<wave>/events/<seq>.event.lisp',
    'task-lifecycle-events.lisp ledger is now a compatibility projection/input',
    '.missiond/requests/<request_id>/events/<seq>.event.lisp',
    '.missiond/requests/<request_id>/reports/final.lisp',
    '.missiond/requests/<request_id>/receipts/<receipt_id>.lisp',
    'request-local one-event files',
    'request-local final-report',
    'request-local verification-receipt artifact',
    'renderRequestVerificationReceipt',
    'validateRequestVerificationReceiptSource',
    'writeRequestVerificationReceiptFile',
    'legacy task-scoped (verification-receipt-set ...) Lisp files remain compatibility inputs',
    'task-runner-dispatch and task-runner-submit-dispatch pass both task-scoped events-dir and request-local lifecycle projection args',
    'task-runner-append-event is the only cooperative mutation helper for task-lifecycle-event-log and task-scoped event files',
    'parent-hotfix finalization is a sparse Lisp projection over the worker report',
    ':acceptance_results is preserved by default',
    'node scripts/check-v3-task-lifecycle-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.lifecycleChecker, sources.lifecycleChecker, [
    'missiond.task-lifecycle-event.v1',
    'missiond.lifecycle-event.v1',
    'validateRequestLifecycleEventFile',
    'validateTaskScopedLifecycleEventFile',
    'renderTaskScopedLifecycleEventFile',
    '--events-dir',
    'parent_hotfix',
    'finalized_report',
    'receipt',
    'completion',
    'COMMIT_HASH_RE',
    'isRepoRelativePath',
    'strictly greater',
    '--dry-fixture',
  ]);

  requireAll(diagnostics, files.appendEvent, sources.appendEvent, [
    "from './check-task-lifecycle-events.mjs'",
    'withLedgerLock',
    "fs.openSync(lockPath, 'wx')",
    'nextSeq(log.events)',
    'validateLifecycleEventFiles([tmp])',
    'fs.renameSync(tmp, ledgerPath)',
    'renderLifecycleEventLog',
    'renderTaskScopedLifecycleEventFile',
    'appendLifecycleEventToEventsDir',
    'scanTaskScopedEventDirMaxSeq',
    'appendLedgerCompatProjection',
    '--events-dir',
    "fs.openSync(eventFile, 'wx')",
    '--request-id',
    '--request-events-dir',
    'writeRequestLifecycleEventFile',
    'renderRequestLifecycleEventFile',
    'request-local projection',
    'concurrent child appends',
    'task-scoped events-dir',
  ]);

  requireAll(diagnostics, files.finalizer, sources.finalizer, [
    'export function finalizeReportObject',
    'at least one parent patch is required',
    'trailing parent patch commit',
    ':agent_commit_hash',
    ':final_commit_hash',
    ':verified_commit_hash',
    ':parent_patches',
    'renderFinalReport',
    'REQUEST_FINAL_REPORT_SCHEMA',
    '--request-id',
    '--request-reports-dir',
    '--replace-acceptance',
    'projectFinalReportSource',
    'preservedKeys',
    'LINEAGE_KEYS',
    'sparse Lisp projection',
    'replaceAcceptance',
    'renderRequestFinalReport',
    'validateRequestFinalReportSource',
    'writeRequestFinalReportFile',
  ]);

  requireAll(diagnostics, files.parentHotfix, sources.parentHotfix, [
    "from './task-runner-finalize-report.mjs'",
    "kind: 'parent_hotfix'",
    "mutation_mode: opts.writeReport ? 'write-report' : 'read-only'",
    'finalized_report_source',
    'request_final_report',
    '--request-reports-dir',
    '--write-report',
    'preservedKeys',
    'preservation through planner',
  ]);

  requireAll(diagnostics, files.projector, sources.projector, [
    "event.event_kind === 'parent_hotfix'",
    "event.event_kind === 'finalized_report'",
    "event.event_kind === 'receipt'",
    "event.event_kind === 'completion'",
    'appendProjectedEntries',
    'existingIds.has(entry.id)',
  ]);

  requireAll(diagnostics, files.nextAction, sources.nextAction, [
    "from './task-runner-append-event.mjs'",
    "a.action === 'finalize_report'",
    'finalizers.length > 0',
    'emitDispatchEventsForActions',
    'appendLifecycleEvent({',
    '--events-dir',
    '--request-events-dir',
    'eventsDirPath',
    'requestEventsDir',
    'request_event_path',
    'event_file',
  ]);

  requireAll(diagnostics, files.dispatch, sources.dispatch, [
    "from './task-runner-next-action.mjs'",
    'runDispatch',
    '--events-dir',
    '--request-id',
    '--request-events-dir',
    'eventsDirPath',
    'requestEventsDir',
    'request_event_path',
    'events_dir_path',
  ]);

  requireAll(diagnostics, files.submitDispatch, sources.submitDispatch, [
    "from './task-runner-dispatch.mjs'",
    "from './task-runner-next-action.mjs'",
    'submitDispatch',
    '--events-dir',
    '--request-id',
    '--request-events-dir',
    'eventsDirPath',
    'requestEventsDir',
    'request_event_path',
    'events_dir_path',
  ]);

  requireAll(diagnostics, files.receiptChecker, sources.receiptChecker, [
    "export const SCHEMA = 'missiond.verification-receipt.v1'",
    'export const REQUEST_RECEIPT_SCHEMA',
    'export const REQUEST_ID_RE',
    'export function renderRequestVerificationReceipt',
    'export function validateRequestVerificationReceiptSource',
    'export function writeRequestVerificationReceiptFile',
    "fs.openSync(target, 'wx')",
    "validateRequestVerificationReceiptSource(onDisk",
    'validateReceiptObject(receipt)',
    "mode = 'create-only'",
    'request-local verification-receipt projection',
    "expected exactly one (${SINGLE_HEAD} ...) form for a request-local projection",
    'wave37-01-request-projection-writer',
  ]);

  requireAll(diagnostics, files.waveState, sources.waveState, [
    "from './check-task-lifecycle-events.mjs'",
    'defaultEventsDirPath',
    'readOptionalTaskScopedEventFiles',
    'mergeLifecycleEvents',
    '--events-dir',
    'events_dir_path',
    'lifecycle_event_count',
  ]);

  requireAll(diagnostics, files.lifecycleSchema, sources.lifecycleSchema, [
    'missiond.task-lifecycle-event.v1',
    'standalone-task-event-file',
    'legacy-task-lifecycle-event-log',
    '.missiond/tasks/<wave>/events/<seq>.event.lisp',
    '.missiond/tasks/<wave>/task-lifecycle-events.lisp',
    'compatibility projection/input',
  ]);

  requireAll(diagnostics, files.reportSchema, sources.reportSchema, [
    'missiond.report-contract.v1',
    'Wave40-01 sparse-projection invariant',
    'sparse Lisp projection',
    'patching only the lineage fields',
    ':acceptance_results is preserved by default',
    'append rather than replace',
  ]);

  requireAll(diagnostics, files.batchVerifier, sources.batchVerifier, [
    "from './check-task-lifecycle-events.mjs'",
    "from './task-runner-append-event.mjs'",
    "from './task-runner-parent-hotfix.mjs'",
    "from './check-verification-receipt.mjs'",
    'validateLifecycleEventFiles',
    'appendLifecycleEvent',
    'planParentHotfixFromSource',
    'writeRequestVerificationReceiptFile',
    'lifecycle receipt finalized-report smoke',
    'task-scoped events-dir',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-task-lifecycle-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (axioms
    (orchestrator-final-truth)
    (event-sourced-runtime))
  (policies
    (policy parent-hotfix-finalization))
  (implementation-map
    (surface task-runner-cli
      :status "code-aligned"
      :code ["scripts/check-task-lifecycle-events.mjs"
             "scripts/task-runner-append-event.mjs"
             "scripts/task-runner-finalize-report.mjs"
             "scripts/task-runner-parent-hotfix.mjs"
             "scripts/project-task-lifecycle-ledger.mjs"
             "scripts/task-runner-dispatch.mjs"
             "scripts/task-runner-submit-dispatch.mjs"
             "scripts/check-verification-receipt.mjs"
             "scripts/verify-task-runner-batch.mjs"]
      :note "Task-scoped lifecycle events are first-class one-event files at .missiond/tasks/<wave>/events/<seq>.event.lisp; the task-lifecycle-events.lisp ledger is now a compatibility projection/input. request-local one-event files at .missiond/requests/<request_id>/events/<seq>.event.lisp; request-local final-report at .missiond/requests/<request_id>/reports/final.lisp; request-local verification-receipt artifact at .missiond/requests/<request_id>/receipts/<receipt_id>.lisp via renderRequestVerificationReceipt + validateRequestVerificationReceiptSource + writeRequestVerificationReceiptFile, while legacy task-scoped (verification-receipt-set ...) Lisp files remain compatibility inputs; task-runner-dispatch and task-runner-submit-dispatch pass both task-scoped events-dir and request-local lifecycle projection args; task-runner-append-event is the only cooperative mutation helper for task-lifecycle-event-log and task-scoped event files. parent-hotfix finalization is a sparse Lisp projection over the worker report; :acceptance_results is preserved by default."))
  (compression-contract
    :checks ["node scripts/check-v3-task-lifecycle-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.lifecycleSchema, `
(task-lifecycle-event-schema missiond.task-lifecycle-event.v1
  :file ".missiond/tasks/<wave>/events/<seq>.event.lisp"
  :legacy ".missiond/tasks/<wave>/task-lifecycle-events.lisp"
  :note "compatibility projection/input"
  :shape-primary standalone-task-event-file
  :shape-legacy legacy-task-lifecycle-event-log)`);
  writeFixture(root, DEFAULT_FILES.lifecycleChecker, `
const SCHEMA = 'missiond.task-lifecycle-event.v1';
const REQUEST_EVENT_SCHEMA = 'missiond.lifecycle-event.v1';
function validateRequestLifecycleEventFile() {}
function validateTaskScopedLifecycleEventFile() {}
function renderTaskScopedLifecycleEventFile() {}
const args = '--events-dir';
const EVENT_KINDS = ['parent_hotfix', 'finalized_report', 'receipt', 'completion'];
const COMMIT_HASH_RE = /x/;
function isRepoRelativePath() {}
const msg = 'strictly greater';
const usage = '--dry-fixture';`);
  writeFixture(root, DEFAULT_FILES.appendEvent, `
import {} from './check-task-lifecycle-events.mjs';
function withLedgerLock() {}
fs.openSync(lockPath, 'wx');
fs.openSync(eventFile, 'wx');
nextSeq(log.events);
validateLifecycleEventFiles([tmp]);
fs.renameSync(tmp, ledgerPath);
renderLifecycleEventLog({});
function renderTaskScopedLifecycleEventFile() {}
function appendLifecycleEventToEventsDir() {}
function scanTaskScopedEventDirMaxSeq() {}
function appendLedgerCompatProjection() {}
const args = '--events-dir --request-id --request-events-dir';
function writeRequestLifecycleEventFile() {}
function renderRequestLifecycleEventFile() {}
const msg = 'request-local projection concurrent child appends task-scoped events-dir';`);
  writeFixture(root, DEFAULT_FILES.finalizer, `
export function finalizeReportObject() {}
const a = 'at least one parent patch is required';
const b = 'trailing parent patch commit';
const c = ':agent_commit_hash :final_commit_hash :verified_commit_hash :parent_patches';
function renderFinalReport() {}
const s = 'REQUEST_FINAL_REPORT_SCHEMA --request-id --request-reports-dir --replace-acceptance';
const proj = 'projectFinalReportSource preservedKeys LINEAGE_KEYS sparse Lisp projection replaceAcceptance';
function renderRequestFinalReport() {}
function validateRequestFinalReportSource() {}
function writeRequestFinalReportFile() {}`);
  writeFixture(root, DEFAULT_FILES.parentHotfix, `
import {} from './task-runner-finalize-report.mjs';
const event = { kind: 'parent_hotfix' };
const mode = "mutation_mode: opts.writeReport ? 'write-report' : 'read-only'";
const a = 'finalized_report_source request_final_report --request-reports-dir --write-report';
const p = 'preservedKeys preservation through planner';`);
  writeFixture(root, DEFAULT_FILES.reportSchema, `
(report-contract-schema missiond.report-contract.v1
  :note "Wave40-01 sparse-projection invariant: sparse Lisp projection patching only the lineage fields. :acceptance_results is preserved by default; --acceptance-command appends instead of append rather than replace.")`);
  writeFixture(root, DEFAULT_FILES.projector, `
if (event.event_kind === 'parent_hotfix') {}
if (event.event_kind === 'finalized_report') {}
if (event.event_kind === 'receipt') {}
if (event.event_kind === 'completion') {}
function appendProjectedEntries() {}
existingIds.has(entry.id);`);
  writeFixture(root, DEFAULT_FILES.nextAction, `
import {} from './task-runner-append-event.mjs';
if (a.action === 'finalize_report') {}
if (finalizers.length > 0) {}
function emitDispatchEventsForActions() { appendLifecycleEvent({}); }
const args = '--events-dir --request-events-dir';
const a = 'eventsDirPath requestEventsDir request_event_path event_file';`);
  writeFixture(root, DEFAULT_FILES.dispatch, `
import {} from './task-runner-next-action.mjs';
function runDispatch() {}
const args = '--events-dir --request-id --request-events-dir';
const a = 'eventsDirPath requestEventsDir request_event_path events_dir_path';`);
  writeFixture(root, DEFAULT_FILES.submitDispatch, `
import {} from './task-runner-dispatch.mjs';
import {} from './task-runner-next-action.mjs';
function submitDispatch() {}
const args = '--events-dir --request-id --request-events-dir';
const a = 'eventsDirPath requestEventsDir request_event_path events_dir_path';`);
  writeFixture(root, DEFAULT_FILES.waveState, `
import {} from './check-task-lifecycle-events.mjs';
function defaultEventsDirPath() {}
function readOptionalTaskScopedEventFiles() {}
function mergeLifecycleEvents() {}
const args = '--events-dir';
const a = 'events_dir_path lifecycle_event_count';`);
  writeFixture(root, DEFAULT_FILES.receiptChecker, `
export const SCHEMA = 'missiond.verification-receipt.v1';
export const REQUEST_RECEIPT_SCHEMA = SCHEMA;
export const REQUEST_ID_RE = /^[a-z0-9]/;
export function renderRequestVerificationReceipt() {}
export function validateRequestVerificationReceiptSource() {}
export function writeRequestVerificationReceiptFile() {
  fs.openSync(target, 'wx');
  validateRequestVerificationReceiptSource(onDisk);
  validateReceiptObject(receipt);
  const mode = 'create-only';
  const m = 'request-local verification-receipt projection';
  const e = 'expected exactly one (\${SINGLE_HEAD} ...) form for a request-local projection';
}
const cat = 'wave37-01-request-projection-writer';`);
  writeFixture(root, DEFAULT_FILES.batchVerifier, `
import {} from './check-task-lifecycle-events.mjs';
import {} from './task-runner-append-event.mjs';
import {} from './task-runner-parent-hotfix.mjs';
import {} from './check-verification-receipt.mjs';
validateLifecycleEventFiles([]);
appendLifecycleEvent({});
planParentHotfixFromSource('');
writeRequestVerificationReceiptFile({});
const smoke = 'lifecycle receipt finalized-report smoke task-scoped events-dir';`);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
