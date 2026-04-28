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
  - parent-hotfix and final-report projection preserve lineage fields.
  - scheduler and batch verifier consume lifecycle/finalization projections.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  lifecycleChecker: 'scripts/check-task-lifecycle-events.mjs',
  appendEvent: 'scripts/task-runner-append-event.mjs',
  finalizer: 'scripts/task-runner-finalize-report.mjs',
  parentHotfix: 'scripts/task-runner-parent-hotfix.mjs',
  projector: 'scripts/project-task-lifecycle-ledger.mjs',
  nextAction: 'scripts/task-runner-next-action.mjs',
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
    'code-aligned-partial',
    'scripts/check-task-lifecycle-events.mjs',
    'scripts/task-runner-append-event.mjs',
    'scripts/task-runner-finalize-report.mjs',
    'scripts/task-runner-parent-hotfix.mjs',
    'scripts/project-task-lifecycle-ledger.mjs',
    'scripts/verify-task-runner-batch.mjs',
    'task-scoped compatibility lifecycle ledger',
    'task-runner-append-event is the only cooperative mutation helper',
    'node scripts/check-v3-task-lifecycle-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.lifecycleChecker, sources.lifecycleChecker, [
    'missiond.task-lifecycle-event.v1',
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
    'concurrent child appends',
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
  ]);

  requireAll(diagnostics, files.parentHotfix, sources.parentHotfix, [
    "from './task-runner-finalize-report.mjs'",
    "kind: 'parent_hotfix'",
    "mutation_mode: opts.writeReport ? 'write-report' : 'read-only'",
    'finalized_report_source',
    '--write-report',
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
  ]);

  requireAll(diagnostics, files.batchVerifier, sources.batchVerifier, [
    "from './check-task-lifecycle-events.mjs'",
    "from './task-runner-append-event.mjs'",
    "from './task-runner-parent-hotfix.mjs'",
    'validateLifecycleEventFiles',
    'appendLifecycleEvent',
    'planParentHotfixFromSource',
    'lifecycle receipt finalized-report smoke',
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
      :status "code-aligned-partial"
      :code ["scripts/check-task-lifecycle-events.mjs"
             "scripts/task-runner-append-event.mjs"
             "scripts/task-runner-finalize-report.mjs"
             "scripts/task-runner-parent-hotfix.mjs"
             "scripts/project-task-lifecycle-ledger.mjs"
             "scripts/verify-task-runner-batch.mjs"]
      :note "task-scoped compatibility lifecycle ledger; task-runner-append-event is the only cooperative mutation helper"))
  (compression-contract
    :checks ["node scripts/check-v3-task-lifecycle-isomorphism.mjs"]))`);
  writeFixture(root, DEFAULT_FILES.lifecycleChecker, `
const SCHEMA = 'missiond.task-lifecycle-event.v1';
const EVENT_KINDS = ['parent_hotfix', 'finalized_report', 'receipt', 'completion'];
const COMMIT_HASH_RE = /x/;
function isRepoRelativePath() {}
const msg = 'strictly greater';
const usage = '--dry-fixture';`);
  writeFixture(root, DEFAULT_FILES.appendEvent, `
import {} from './check-task-lifecycle-events.mjs';
function withLedgerLock() {}
fs.openSync(lockPath, 'wx');
nextSeq(log.events);
validateLifecycleEventFiles([tmp]);
fs.renameSync(tmp, ledgerPath);
renderLifecycleEventLog({});
const msg = 'concurrent child appends';`);
  writeFixture(root, DEFAULT_FILES.finalizer, `
export function finalizeReportObject() {}
const a = 'at least one parent patch is required';
const b = 'trailing parent patch commit';
const c = ':agent_commit_hash :final_commit_hash :verified_commit_hash :parent_patches';
function renderFinalReport() {}`);
  writeFixture(root, DEFAULT_FILES.parentHotfix, `
import {} from './task-runner-finalize-report.mjs';
const event = { kind: 'parent_hotfix' };
const mode = "mutation_mode: opts.writeReport ? 'write-report' : 'read-only'";
const a = 'finalized_report_source --write-report';`);
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
function emitDispatchEventsForActions() { appendLifecycleEvent({}); }`);
  writeFixture(root, DEFAULT_FILES.batchVerifier, `
import {} from './check-task-lifecycle-events.mjs';
import {} from './task-runner-append-event.mjs';
import {} from './task-runner-parent-hotfix.mjs';
validateLifecycleEventFiles([]);
appendLifecycleEvent({});
planParentHotfixFromSource('');
const smoke = 'lifecycle receipt finalized-report smoke';`);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
