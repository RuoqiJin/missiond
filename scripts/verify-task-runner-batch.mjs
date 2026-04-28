#!/usr/bin/env node

// MissionD task-runner batch verifier (v0).
//
// Walks every productive node in a wave28-01 task-runner manifest and joins:
//   1. task contract Lisp     (.missiond/tasks/<wave>/<task-id>.lisp)
//   2. report Lisp            (.missiond/tasks/<wave>/reports/<task-id>.report.lisp)
//   3. shared-memory ledger   (.missiond/tasks/<wave>/shared-memory.lisp;
//                              must contain (completion :task <task-id> ...))
//   4. commit hash            (from report :commit_hash; cross-checked against
//                              the matching shared-memory completion :summary
//                              when the wave28 convention surfaces a hash there)
//   5. wave23-03 verifyRun    (re-uses the per-task post-run proof from
//                              scripts/verify-task-run.mjs, so a single contract
//                              + report + ledger triple is verified by exactly
//                              the same code that the per-task CLI runs)
//
// Output schema: missiond.task-runner-batch-verification.v0 with stable, sorted
// top-level keys so the JSON is byte-identical for byte-identical inputs.
//
// Read-only by construction: never invokes a mutating git verb, never spawns a
// shell, never opens a network socket, never calls an LLM. The only git
// surface used is `git rev-parse | git log | git show` borrowed from
// scripts/verify-task-contract.mjs via verify-task-run.mjs's readCommit
// re-export — and even that is gated behind the existence of an in-tree git
// commit. Synthetic inputs (--dry-fixture and the in-process verifyManifest
// helpers) avoid git entirely.
//
// Skip behaviour for pseudo-nodes: the wave28-01 manifest checker already
// rejects archive / backfill / index / lisp-backfill nodes when
// :productive_only is true. This verifier is defence-in-depth: any node whose
// :kind ∈ FORBIDDEN_PRODUCTIVE_KINDS, or whose :task_id contains a known
// pseudo-node substring, OR whose owning manifest declares
// :productive_only false is silently SKIPPED (counted in skipped_nodes for
// telemetry). A skipped node never raises a missing-report or missing-memory
// error.

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

import {
  FORBIDDEN_PRODUCTIVE_KINDS,
  readManifestFile,
  validateManifestObject,
} from './check-task-runner-manifest.mjs';
import {
  loadContract,
  loadContractFromSource,
  readCommit,
} from './verify-task-contract.mjs';
import {
  loadLedger,
  loadLedgerFromSource,
  loadReport,
  loadReportFromSource,
  verifyRun,
} from './verify-task-run.mjs';

export const SCHEMA = 'missiond.task-runner-batch-verification.v0';

// Substrings that mark a task id as orchestrator-owned (mirrors the wave28-01
// FORBIDDEN_ID_SUBSTRINGS list — kept local so this file does not depend on a
// non-exported internal). Defence in depth only; wave28-01's checker rejects
// these at manifest-validation time when :productive_only is true.
const PSEUDO_NODE_ID_SUBSTRINGS = ['-archive-', '-backfill-', '-index', 'lisp-backfill'];

// Aggregate-status decision matrix:
//   - "all_green" ⇔ every productive node verified AND no missing/failed entries
//   - "failed"    ⇔ at least one productive node missing report OR missing
//                   memory completion OR contract verification reported failure
//   - "partial"   ⇔ no failures, but verified_nodes < total_nodes (e.g. zero
//                   productive nodes, or a node skipped by an external policy)
const STATUS_ALL_GREEN = 'all_green';
const STATUS_PARTIAL = 'partial';
const STATUS_FAILED = 'failed';

const usage = `Usage:
  node scripts/verify-task-runner-batch.mjs --manifest <manifest.lisp> [--json]
  node scripts/verify-task-runner-batch.mjs --dry-fixture [--json]

Joins a wave28-01 task-runner manifest against on-disk evidence:
  - task contract  .missiond/tasks/<wave>/<task-id>.lisp
  - report         .missiond/tasks/<wave>/reports/<task-id>.report.lisp
  - shared-memory  .missiond/tasks/<wave>/shared-memory.lisp
                   (looks for (completion :task <task-id> ...))
  - commit hash    report :commit_hash, then cross-check against the
                   matching shared-memory completion :summary when a hex
                   sha appears there

For each productive node, runs the wave23-03 single-task verifier
(scripts/verify-task-run.mjs) so the same code-path that proves a single
task run also proves the whole batch.

Pseudo-nodes (archive / backfill / index / lisp-backfill) are silently
skipped — they are orchestrator-owned and never carry a worker report.

Flags:
  --manifest <file>   wave28-01 task-runner-manifest v1 Lisp file
  --json              emit machine-readable JSON instead of text
  --dry-fixture       run self-contained fixtures (no git, no I/O)

Read-only: this script never invokes git add / commit / reset / push /
checkout / stash / merge / rebase / fetch / pull. The only git surface is
the read-only readCommit helper imported from verify-task-contract.mjs
(via verify-task-run.mjs).
`;

function failUsage(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    manifest: null,
    json: false,
    dryFixture: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--manifest') {
      opts.manifest = argv[++i] ?? failUsage('--manifest requires a value');
    } else if (arg.startsWith('--manifest=')) {
      opts.manifest = arg.slice('--manifest='.length);
    } else if (arg.startsWith('--')) {
      failUsage(`unknown flag: ${arg}`);
    } else {
      failUsage(`unexpected positional argument: ${arg}`);
    }
  }
  return opts;
}

// --- Skip predicate -------------------------------------------------------

// Return true when `node` (a projected manifest node) is a pseudo-node and
// must be excluded from worker completion accounting. Mirrors
// scripts/check-task-runner-manifest.mjs FORBIDDEN_PRODUCTIVE_KINDS plus the
// id-substring defence list — kept local rather than relying on a private
// internal export.
export function isPseudoNode(node) {
  if (!node || typeof node !== 'object') return false;
  if (typeof node.kind === 'string' && FORBIDDEN_PRODUCTIVE_KINDS.has(node.kind)) {
    return true;
  }
  if (typeof node.task_id === 'string') {
    for (const sub of PSEUDO_NODE_ID_SUBSTRINGS) {
      if (node.task_id.includes(sub)) return true;
    }
  }
  return false;
}

// --- Path joins -----------------------------------------------------------

// Build the on-disk paths a productive node MUST surface. Repo-relative so
// the JSON output is portable across checkout locations.
export function expectedPathsFor(wave, taskId) {
  return {
    task_contract: `.missiond/tasks/${wave}/${taskId}.lisp`,
    report: `.missiond/tasks/${wave}/reports/${taskId}.report.lisp`,
    shared_memory: `.missiond/tasks/${wave}/shared-memory.lisp`,
  };
}

// --- Commit hash cross-check ----------------------------------------------

// Extract the first hex sha (>= 7 hex chars, <= 40) found in `text`. Returns
// null when none. Used to harvest a commit hash from a shared-memory
// completion :summary so the batch verifier can detect divergence between
// the report's :commit_hash and what the worker recorded in the ledger.
export function extractCommitHashFromText(text) {
  if (typeof text !== 'string' || text.length === 0) return null;
  const match = text.match(/\b([0-9a-f]{7,40})\b/i);
  if (!match) return null;
  return match[1].toLowerCase();
}

function commitHashesAgreeLocal(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  const ax = a.trim().toLowerCase();
  const bx = b.trim().toLowerCase();
  if (ax.length === 0 || bx.length === 0) return false;
  if (!/^[0-9a-f]+$/.test(ax) || !/^[0-9a-f]+$/.test(bx)) return false;
  if (ax === bx) return true;
  const longer = ax.length >= bx.length ? ax : bx;
  const shorter = ax.length >= bx.length ? bx : ax;
  if (shorter.length >= 7 && longer.startsWith(shorter)) return true;
  return false;
}

// wave29-04: enumerate every commit hash carried by the report's lineage
// fields. Order: commit_hash, agent_commit_hash, final_commit_hash,
// verified_commit_hash, then each :parent_patches entry's :commit. Empty
// strings are filtered. The batch verifier uses this to accept memory
// completion summaries that mention EITHER the final commit OR the agent
// commit OR an intermediate parent-patch commit — some completions are
// written post-worker-commit but pre-hotfix and so quote the worker hash.
// Exported for fixture coverage and downstream tooling reuse.
export function collectReportLineageHashes(report) {
  const flat = [
    report.commitHash,
    report.agentCommitHash,
    report.finalCommitHash,
    report.verifiedCommitHash,
  ];
  const fromPatches = (report.parentPatches ?? []).map((p) => p.commit);
  return [...flat, ...fromPatches].filter(
    (hash) => typeof hash === 'string' && hash.trim() !== '',
  );
}

// --- Per-node verification (pure) -----------------------------------------

// Verify a single productive node against its evidence. All filesystem and
// git access is delegated to the caller via the `loaders` argument so the
// fixtures can swap in synthetic loaders (no-disk, no-git).
//
// Loaders contract:
//   loadContractAt(path)  -> contract object (throws if missing)
//   loadReportAt(path)    -> report object   (throws if missing)
//   loadLedgerAt(path)    -> ledger object   (throws if missing)
//   reportExists(path)    -> boolean         (existence check, no throw)
//   ledgerExists(path)    -> boolean
//   contractExists(path)  -> boolean
//   readCommitAt(hash)    -> { hash, message, files } (or null when in
//                            dry mode; when null the contract delegate
//                            check inside verifyRun is skipped)
//
// Returns:
//   { task_id, status: 'verified'|'missing_report'|'missing_memory_completion'|
//                      'failed_contract_verification',
//     reason?, paths, commit_hash?, run_result? }
export function verifyNode(node, manifest, loaders) {
  const wave = manifest.wave;
  const taskId = node.task_id;
  const paths = expectedPathsFor(wave, taskId);

  // Step 0: contract must exist on disk for the report/memory checks to mean
  // anything. A missing contract is treated as a contract-verification
  // failure (the manifest declared the node, but the worker never planted
  // the contract Lisp).
  if (!loaders.contractExists(paths.task_contract)) {
    return {
      task_id: taskId,
      status: 'failed_contract_verification',
      reason: `task contract not found at ${paths.task_contract}`,
      paths,
    };
  }

  // Step 1: report presence.
  if (!loaders.reportExists(paths.report)) {
    return {
      task_id: taskId,
      status: 'missing_report',
      reason: `report not found at ${paths.report}`,
      paths,
    };
  }

  // Step 2: shared-memory ledger presence.
  if (!loaders.ledgerExists(paths.shared_memory)) {
    return {
      task_id: taskId,
      status: 'missing_memory_completion',
      reason: `shared-memory ledger not found at ${paths.shared_memory}`,
      paths,
    };
  }

  // Step 3: load all three.
  let contract;
  let report;
  let ledger;
  try {
    contract = loaders.loadContractAt(paths.task_contract);
  } catch (err) {
    return {
      task_id: taskId,
      status: 'failed_contract_verification',
      reason: `failed to load task contract: ${err?.message ?? err}`,
      paths,
    };
  }
  try {
    report = loaders.loadReportAt(paths.report);
  } catch (err) {
    return {
      task_id: taskId,
      status: 'missing_report',
      reason: `failed to load report: ${err?.message ?? err}`,
      paths,
    };
  }
  try {
    ledger = loaders.loadLedgerAt(paths.shared_memory);
  } catch (err) {
    return {
      task_id: taskId,
      status: 'missing_memory_completion',
      reason: `failed to load shared-memory ledger: ${err?.message ?? err}`,
      paths,
    };
  }

  // Step 4: shared-memory must contain a (completion :task <task-id> ...).
  const completion = ledger.completions.find((c) => c.task === taskId);
  if (!completion) {
    return {
      task_id: taskId,
      status: 'missing_memory_completion',
      reason: `shared-memory ledger ${paths.shared_memory} has no (completion :task ${taskId} ...) entry`,
      paths,
    };
  }

  // Step 5: cross-check report commit lineage against the hash embedded in
  // the matching shared-memory completion :summary. wave29-04 widens the
  // accepted lineage from the four flat commit-hash fields to include every
  // (parent_patches[i].commit) entry too — some completion summaries are
  // written post-worker-commit but pre-hotfix and so quote the worker /
  // intermediate hash. The verifier still resolves to the FINAL hash for
  // the canonical result row (Step 6); this step only decides whether the
  // memory hash is consistent with SOME role in the lineage. When neither
  // side has a hash, the wave23-03 verifier catches the missing-hash case.
  // When only one side has a hash we do not fail — the report side is
  // authoritative; the memory hash is advisory.
  const memoryHash = extractCommitHashFromText(completion.summary);
  const reportLineageHashes = collectReportLineageHashes(report);
  if (
    memoryHash &&
    reportLineageHashes.length > 0 &&
    !reportLineageHashes.some((hash) => commitHashesAgreeLocal(memoryHash, hash))
  ) {
    return {
      task_id: taskId,
      status: 'failed_contract_verification',
      reason:
        `commit hash mismatch — report lineage hashes ${JSON.stringify(reportLineageHashes)} ` +
        `do not agree with shared-memory completion summary hash ${JSON.stringify(memoryHash)}`,
      paths,
      commit_hash: report.commitHash,
      memory_commit_hash: memoryHash,
    };
  }

  // Step 6: wave23-03 verifyRun against the actual git commit (when
  // available). The verification hash is always resolved to the FINAL
  // commit (verified > final > commit_hash), even when the memory summary
  // referenced an earlier hash in the lineage — the verified result row
  // points at the final/verified hash by contract. In dry-fixture mode
  // readCommitAt returns a synthetic commitInfo so the same code path runs
  // without git.
  let commitInfo;
  const verificationHash = report.verifiedCommitHash ?? report.finalCommitHash ?? report.commitHash;
  try {
    commitInfo = loaders.readCommitAt(verificationHash);
  } catch (err) {
    return {
      task_id: taskId,
      status: 'failed_contract_verification',
      reason: `failed to read git commit ${verificationHash}: ${err?.message ?? err}`,
      paths,
      commit_hash: verificationHash,
    };
  }

  const runResult = verifyRun({
    contract,
    contractFile: paths.task_contract,
    report,
    reportFile: paths.report,
    ledger,
    ledgerFile: paths.shared_memory,
    ledgerStatus: 'present',
    commitInfo,
    trace: null,
    traceFile: null,
    traceStatus: 'absent',
    traceLoadError: null,
    requireTrace: false,
  });

  if (!runResult.ok) {
    return {
      task_id: taskId,
      status: 'failed_contract_verification',
      reason: runResult.errors.join('; '),
      paths,
      commit_hash: report.commitHash ?? null,
      run_errors: runResult.errors,
    };
  }

  return {
    task_id: taskId,
    status: 'verified',
    paths,
    commit_hash: report.commitHash ?? commitInfo?.hash ?? null,
  };
}

// --- Aggregate ------------------------------------------------------------

// Build the final batch-verification report. All array fields are sorted
// (task_ids ascending) so the JSON is byte-identical for byte-identical
// inputs regardless of node ordering.
export function aggregateResults(manifestPath, manifest, perNodeResults) {
  const productive = perNodeResults.filter((r) => r.kind !== 'skipped');
  const verified = productive.filter((r) => r.status === 'verified');
  const missingReports = productive
    .filter((r) => r.status === 'missing_report')
    .map((r) => r.task_id)
    .sort();
  const missingMemory = productive
    .filter((r) => r.status === 'missing_memory_completion')
    .map((r) => r.task_id)
    .sort();
  const failedContract = productive
    .filter((r) => r.status === 'failed_contract_verification')
    .map((r) => ({ task_id: r.task_id, reason: r.reason ?? '<unknown>' }))
    .sort((a, b) => a.task_id.localeCompare(b.task_id));
  const skipped = perNodeResults
    .filter((r) => r.kind === 'skipped')
    .map((r) => r.task_id)
    .sort();

  let aggregateStatus;
  if (
    missingReports.length === 0 &&
    missingMemory.length === 0 &&
    failedContract.length === 0
  ) {
    aggregateStatus =
      verified.length === productive.length && productive.length > 0
        ? STATUS_ALL_GREEN
        : STATUS_PARTIAL;
  } else {
    aggregateStatus = STATUS_FAILED;
  }

  // Top-level keys are emitted in alphabetical order so the JSON output is
  // byte-identical for byte-identical inputs.
  return {
    aggregate_status: aggregateStatus,
    failed_contract_verifications: failedContract,
    manifest_path: manifestPath,
    missing_memory_completions: missingMemory,
    missing_reports: missingReports,
    schema: SCHEMA,
    skipped_nodes: skipped,
    total_nodes: productive.length,
    verified_nodes: verified.length,
    wave: manifest.wave ?? null,
  };
}

// --- Top-level orchestration ----------------------------------------------

// Verify a manifest given a set of loaders (real loaders for the CLI path,
// synthetic loaders for fixtures). Returns the aggregate report shape from
// aggregateResults.
export function verifyManifest({
  manifestPath,
  manifest,
  loaders,
}) {
  const perNode = [];
  for (const node of manifest.nodes ?? []) {
    if (manifest.productive_only === false || isPseudoNode(node)) {
      perNode.push({ kind: 'skipped', task_id: node.task_id });
      continue;
    }
    const result = verifyNode(node, manifest, loaders);
    perNode.push({ kind: 'productive', ...result });
  }
  return aggregateResults(manifestPath, manifest, perNode);
}

// --- Real-disk loaders ----------------------------------------------------

function realLoaders(cwd) {
  const resolve = (p) => path.resolve(cwd, p);
  return {
    contractExists: (p) => fs.existsSync(resolve(p)),
    reportExists: (p) => fs.existsSync(resolve(p)),
    ledgerExists: (p) => fs.existsSync(resolve(p)),
    loadContractAt: (p) => loadContract(resolve(p)),
    loadReportAt: (p) => loadReport(resolve(p)),
    loadLedgerAt: (p) => loadLedger(resolve(p)),
    readCommitAt: (hash) => {
      if (!hash || typeof hash !== 'string') {
        throw new Error('report :commit_hash missing — cannot resolve git commit');
      }
      return readCommit(hash);
    },
  };
}

// --- Output ---------------------------------------------------------------

function emit(payload, { json }) {
  if (json) {
    // Stable, sorted JSON so byte-identical inputs produce byte-identical
    // bytes (helpful for diffing batch reports across runs).
    console.log(stableStringify(payload));
    return;
  }
  const wave = payload.wave ?? '<unknown>';
  if (payload.aggregate_status === STATUS_ALL_GREEN) {
    console.log(
      `task-runner batch verify OK: wave=${wave} verified=${payload.verified_nodes}/${payload.total_nodes} ` +
      `(skipped=${payload.skipped_nodes.length})`,
    );
    return;
  }
  if (payload.aggregate_status === STATUS_PARTIAL) {
    console.warn(
      `task-runner batch verify PARTIAL: wave=${wave} verified=${payload.verified_nodes}/${payload.total_nodes} ` +
      `(skipped=${payload.skipped_nodes.length})`,
    );
    return;
  }
  console.error(
    `task-runner batch verify FAILED: wave=${wave} verified=${payload.verified_nodes}/${payload.total_nodes}`,
  );
  for (const id of payload.missing_reports) {
    console.error(`  missing report: ${id}`);
  }
  for (const id of payload.missing_memory_completions) {
    console.error(`  missing memory completion: ${id}`);
  }
  for (const row of payload.failed_contract_verifications) {
    console.error(`  contract verification failed: ${row.task_id} — ${row.reason}`);
  }
}

// JSON.stringify with deterministic key order. Top-level keys are already
// emitted in alphabetical order from aggregateResults; this also sorts any
// nested object keys for byte-identical reproducibility.
function stableStringify(value) {
  return JSON.stringify(sortKeys(value), null, 2);
}

function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value && typeof value === 'object') {
    const out = {};
    for (const key of Object.keys(value).sort()) {
      out[key] = sortKeys(value[key]);
    }
    return out;
  }
  return value;
}

// --- CLI ------------------------------------------------------------------

function runCli(opts) {
  if (!opts.manifest) {
    failUsage('--manifest <manifest.lisp> is required (or use --dry-fixture)');
  }
  const cwd = process.cwd();
  const manifestPath = path.resolve(cwd, opts.manifest);
  if (!fs.existsSync(manifestPath)) {
    process.stderr.write(`error: manifest not found at ${manifestPath}\n`);
    process.exit(1);
  }

  let manifests;
  try {
    manifests = readManifestFile(manifestPath);
  } catch (err) {
    process.stderr.write(`error: failed to read manifest: ${err?.message ?? err}\n`);
    process.exit(1);
  }
  if (!Array.isArray(manifests) || manifests.length === 0) {
    process.stderr.write(`error: no (task-runner-manifest ...) form found in ${manifestPath}\n`);
    process.exit(1);
  }
  if (manifests.length > 1) {
    process.stderr.write(
      `error: ${manifestPath} contains ${manifests.length} (task-runner-manifest ...) forms; ` +
      `expected exactly 1\n`,
    );
    process.exit(1);
  }
  const manifest = manifests[0];

  const schemaErrors = validateManifestObject(manifest);
  if (schemaErrors.length > 0) {
    process.stderr.write(
      `error: manifest ${manifestPath} failed wave28-01 schema validation:\n`,
    );
    for (const e of schemaErrors) process.stderr.write(`  ${e}\n`);
    process.exit(1);
  }

  const loaders = realLoaders(cwd);
  const result = verifyManifest({
    manifestPath: opts.manifest, // emit as supplied (relative when given relative)
    manifest,
    loaders,
  });

  emit(result, { json: opts.json });
  process.exit(result.aggregate_status === STATUS_FAILED ? 1 : 0);
}

// --- Fixtures -------------------------------------------------------------

// Build a synthetic loader bundle from in-memory dictionaries. Used by
// fixtures so they exercise the same verifyNode / aggregateResults code that
// the CLI runs, but without any disk or git access.
function syntheticLoaders({ contracts, reports, ledgers, commits }) {
  return {
    contractExists: (p) => Object.prototype.hasOwnProperty.call(contracts, p),
    reportExists: (p) => Object.prototype.hasOwnProperty.call(reports, p),
    ledgerExists: (p) => Object.prototype.hasOwnProperty.call(ledgers, p),
    loadContractAt: (p) => {
      if (!contracts[p]) throw new Error(`fixture: no contract at ${p}`);
      return contracts[p];
    },
    loadReportAt: (p) => {
      if (!reports[p]) throw new Error(`fixture: no report at ${p}`);
      return reports[p];
    },
    loadLedgerAt: (p) => {
      if (!ledgers[p]) throw new Error(`fixture: no ledger at ${p}`);
      return ledgers[p];
    },
    readCommitAt: (hash) => {
      if (!hash) throw new Error('fixture: report missing :commit_hash');
      const commit = commits[hash] ?? commits['*'];
      if (!commit) throw new Error(`fixture: no commit for ${hash}`);
      return commit;
    },
  };
}

// Build a (task ...) source string with the given id, contracted message,
// write-scope, and must-not-touch. Kept tiny so fixture diffs stay scoped.
function buildContractSource({
  id,
  message,
  writeScope = ['scripts/foo.mjs'],
  mustNotTouch = ['crates/**'],
}) {
  return (
    `(task ${id}\n` +
    `  :schema "missiond.task-contract.v1"\n` +
    `  :title "${id}"\n` +
    `  :kind code-alignment\n` +
    `  :status ready\n` +
    `  :owner "claudecode"\n` +
    `  :goal "fixture"\n` +
    `  :write-scope [${writeScope.map((p) => `"${p}"`).join(' ')}]\n` +
    `  :must-not-touch [${mustNotTouch.map((p) => `"${p}"`).join(' ')}]\n` +
    `  :acceptance ["true"]\n` +
    `  :commit (:required true\n` +
    `           :message "${message}"\n` +
    `           :scope-check write-scope-only))`
  );
}

function buildReportSource({
  id,
  commitHash,
  files = ['scripts/foo.mjs'],
  status = 'done',
  agentCommitHash = null,
  parentPatches = null,
}) {
  const agentLine = agentCommitHash
    ? `  :agent_commit_hash "${agentCommitHash}"\n`
    : '';
  const patchesLine = parentPatches
    ? `  :parent_patches\n    [${parentPatches
        .map(
          (p) =>
            `(:commit "${p.commit}"\n` +
            `      :kind ${p.kind}\n` +
            `      :reason "${p.reason}"\n` +
            `      :files [${p.files.map((f) => `"${f}"`).join(' ')}])`,
        )
        .join('\n     ')}]\n`
    : '';
  return (
    `(report ${id}\n` +
    `  :schema "missiond.report-contract.v1"\n` +
    `  :task_id "${id}"\n` +
    `  :status ${status}\n` +
    `  :commit_hash "${commitHash}"\n` +
    agentLine +
    patchesLine +
    `  :files_changed [${files.map((f) => `"${f}"`).join(' ')}]\n` +
    `  :acceptance_results [(:command "true" :exit_code 0 :ok true)])`
  );
}

function buildLedgerSource(wave, completions) {
  const entries = completions
    .map(
      (c, i) =>
        `  (completion\n` +
        `    :id ${wave}-completion-${String(i + 1).padStart(3, '0')}\n` +
        `    :task ${c.task}\n` +
        `    :agent claudecode\n` +
        `    :seq ${i + 1}\n` +
        `    :touched ["scripts/foo.mjs"]\n` +
        `    :summary "${c.summary}")`,
    )
    .join('\n');
  return (
    `(shared-memory ${wave}\n` +
    `  :schema "missiond.shared-memory.v1"\n` +
    `  :wave ${wave}\n` +
    `  :created-at "2026-04-28T00:00:00Z"\n` +
    `  :sequence 1\n` +
    `${entries})`
  );
}

// Common synthetic commit. The wave23-03 verifyRun delegates to the
// task-contract verifier, which checks the commit message + scope. We give
// every fixture commit a message + files set that satisfies the contract
// emitted by buildContractSource.
function syntheticCommit(hash, message = 'feat(tasks): fixture', files = ['scripts/foo.mjs']) {
  return {
    hash: hash.padEnd(40, '0'),
    message: `${message}\n\nbody\n`,
    files,
  };
}

function runFixtures({ json }) {
  const fixtures = [];

  // -------------------------- pass: all-green --------------------------
  const greenManifest = {
    id: 'm-green',
    schema: 'missiond.task-runner-manifest.v1',
    wave: 'wave99',
    brief_mode: 'thin',
    shared_preamble_path: '.missiond/claudecode/wave28-shared-preamble.md',
    productive_only: true,
    overlap_policy: 'reject',
    description: null,
    generated_at: null,
    generator: null,
    nodes: [
      {
        task_id: 'wave99-01-foo',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'A',
        estimated_minutes: 30,
        heartbeat_minutes: 10,
        write_scope: ['scripts/foo.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
      {
        task_id: 'wave99-02-bar',
        depends_on: ['wave99-01-foo'],
        verification_tier: 'local',
        dispatch_group: 'B',
        estimated_minutes: 30,
        heartbeat_minutes: 10,
        write_scope: ['scripts/foo.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
    ],
    loc: null,
  };
  const greenContracts = {
    '.missiond/tasks/wave99/wave99-01-foo.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-01-foo', message: 'feat(tasks): fixture' }),
    ),
    '.missiond/tasks/wave99/wave99-02-bar.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-02-bar', message: 'feat(tasks): fixture' }),
    ),
  };
  const greenReports = {
    '.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp': loadReportFromSource(
      buildReportSource({ id: 'wave99-01-foo', commitHash: 'abc1234' }),
      '<fx-report-foo>',
    ),
    '.missiond/tasks/wave99/reports/wave99-02-bar.report.lisp': loadReportFromSource(
      buildReportSource({ id: 'wave99-02-bar', commitHash: 'deadbee' }),
      '<fx-report-bar>',
    ),
  };
  const greenLedgers = {
    '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
      buildLedgerSource('wave99', [
        { task: 'wave99-01-foo', summary: 'done at commit abc1234' },
        { task: 'wave99-02-bar', summary: 'done at commit deadbee' },
      ]),
      '<fx-ledger-green>',
    ),
  };
  const greenCommits = {
    abc1234: syntheticCommit('abc1234'),
    deadbee: syntheticCommit('deadbee'),
  };
  fixtures.push({
    name: 'pass: all-green 2-node manifest with reports + completions + commits aligned',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: greenLedgers,
      commits: greenCommits,
    }),
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 2,
      total_nodes: 2,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: [],
    },
  });

  // ---------------------- fail: missing report ----------------------
  fixtures.push({
    name: 'fail: missing report → missing_reports populated, status=failed',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: {
        '.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp':
          greenReports['.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp'],
        // wave99-02-bar's report is missing.
      },
      ledgers: greenLedgers,
      commits: greenCommits,
    }),
    expect: {
      aggregate_status: STATUS_FAILED,
      verified_nodes: 1,
      total_nodes: 2,
      missing_reports: ['wave99-02-bar'],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: [],
    },
  });

  // -------------------- fail: missing memory completion --------------------
  fixtures.push({
    name: 'fail: missing memory completion → missing_memory_completions populated',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: {
        '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
          // Only wave99-01-foo gets a completion entry.
          buildLedgerSource('wave99', [
            { task: 'wave99-01-foo', summary: 'done at commit abc1234' },
          ]),
          '<fx-ledger-missing-memory>',
        ),
      },
      commits: greenCommits,
    }),
    expect: {
      aggregate_status: STATUS_FAILED,
      verified_nodes: 1,
      total_nodes: 2,
      missing_reports: [],
      missing_memory_completions: ['wave99-02-bar'],
      failed_contract_verifications: [],
      skipped_nodes: [],
    },
  });

  // -------------------- fail: commit hash mismatch --------------------
  fixtures.push({
    name: 'fail: commit hash mismatch (report says X, memory says Y) → failed_contract_verifications row',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: {
        '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
          buildLedgerSource('wave99', [
            // Foo's memory summary says abc1234 (matches report).
            { task: 'wave99-01-foo', summary: 'done at commit abc1234' },
            // Bar's memory summary records a different sha than its report.
            { task: 'wave99-02-bar', summary: 'done at commit cafe123' },
          ]),
          '<fx-ledger-cm>',
        ),
      },
      commits: greenCommits,
    }),
    expect: {
      aggregate_status: STATUS_FAILED,
      verified_nodes: 1,
      total_nodes: 2,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications_task_ids: ['wave99-02-bar'],
      failed_contract_verifications_reason_match: /commit hash mismatch/,
      skipped_nodes: [],
    },
  });

  // ---------------- pass: pseudo-node skipped (defence in depth) ----------------
  // Mix one productive node with one archive pseudo-node — the archive node
  // would normally be rejected by wave28-01's checker when productive_only
  // is true, but this verifier skips it as defence in depth.
  const skipManifest = {
    ...greenManifest,
    id: 'm-with-pseudo',
    nodes: [
      ...greenManifest.nodes.slice(0, 1), // wave99-01-foo only (productive)
      {
        task_id: 'wave99-99-archive-prior',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'C',
        estimated_minutes: 5,
        heartbeat_minutes: 5,
        write_scope: ['.missiond/claudecode/_archive/foo.md'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
      {
        task_id: 'wave99-50-helper',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'D',
        estimated_minutes: 10,
        heartbeat_minutes: 5,
        write_scope: ['scripts/foo.mjs'],
        notes: null,
        owner: null,
        kind: 'backfill', // pseudo via :kind
        loc: null,
      },
    ],
  };
  fixtures.push({
    name: 'pass: archive id + backfill kind pseudo-nodes silently skipped',
    manifest: skipManifest,
    manifestPath: '.missiond/tasks/wave99/manifest-pseudo.lisp',
    loaders: syntheticLoaders({
      contracts: {
        '.missiond/tasks/wave99/wave99-01-foo.lisp':
          greenContracts['.missiond/tasks/wave99/wave99-01-foo.lisp'],
      },
      reports: {
        '.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp':
          greenReports['.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp'],
      },
      ledgers: greenLedgers,
      commits: greenCommits,
    }),
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 1,
      total_nodes: 1,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: ['wave99-50-helper', 'wave99-99-archive-prior'],
    },
  });

  // ---------------- pass: productive_only=false ⇒ all nodes skipped ----------------
  fixtures.push({
    name: 'pass: productive_only=false ⇒ all nodes skipped (orchestrator-owned manifest)',
    manifest: { ...greenManifest, productive_only: false },
    manifestPath: '.missiond/tasks/wave99/manifest-non-productive.lisp',
    loaders: syntheticLoaders({
      contracts: {},
      reports: {},
      ledgers: {},
      commits: {},
    }),
    expect: {
      // Zero productive nodes ⇒ aggregate is partial, not all_green.
      aggregate_status: STATUS_PARTIAL,
      verified_nodes: 0,
      total_nodes: 0,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: ['wave99-01-foo', 'wave99-02-bar'],
    },
  });

  // ---------------- pass: deterministic byte-identical JSON ----------------
  // Verify the exact same green inputs twice and confirm the JSON output
  // is byte-identical.
  fixtures.push({
    name: 'pass: deterministic byte-identical JSON (same inputs ⇒ same bytes)',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: greenLedgers,
      commits: greenCommits,
    }),
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 2,
      total_nodes: 2,
      determinism_check: true,
    },
  });

  // ---------------- fail: contract message mismatch ⇒ verifyRun fails ----------------
  // The wave23-03 verifyRun is the source of truth for contract verification.
  // Wire up a commit with a different subject than the contract expects.
  fixtures.push({
    name: 'fail: contract message mismatch (delegated to wave23-03 verifyRun)',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: greenLedgers,
      commits: {
        abc1234: syntheticCommit('abc1234', 'chore: wrong subject'),
        deadbee: syntheticCommit('deadbee'),
      },
    }),
    expect: {
      aggregate_status: STATUS_FAILED,
      verified_nodes: 1,
      total_nodes: 2,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications_task_ids: ['wave99-01-foo'],
      failed_contract_verifications_reason_match: /commit message does not match/,
      skipped_nodes: [],
    },
  });

  // ---------------------------------------------------------------------
  // wave28-06 cross-layer smoke pins — confirm batch verifier skips the
  // same pseudo nodes the wave28-01 checker rejects + the wave28-03
  // renderer drops, AND that productive-node accounting agrees with the
  // wave28-02 plan CLI for the SAME synthetic manifest.
  // ---------------------------------------------------------------------

  // Build a wave28-06 synthetic manifest mixing 2 productive nodes plus
  // 2 pseudo nodes (archive id substring + backfill kind). The verifier
  // MUST surface verified_nodes=2 and skipped_nodes containing both pseudo
  // ids, mirroring the wave28-03 renderer's productive-only emit pass.
  const loopSmokeManifest = {
    id: 'm-wave28-06-loop-smoke-batch',
    schema: 'missiond.task-runner-manifest.v1',
    wave: 'wave99',
    brief_mode: 'thin',
    shared_preamble_path: '.missiond/claudecode/wave28-shared-preamble.md',
    productive_only: true,
    overlap_policy: 'reject',
    description: null,
    generated_at: null,
    generator: null,
    nodes: [
      {
        task_id: 'wave99-01-alpha',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'A',
        estimated_minutes: 30,
        heartbeat_minutes: 10,
        write_scope: ['scripts/alpha.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
      {
        task_id: 'wave99-02-beta',
        depends_on: ['wave99-01-alpha'],
        verification_tier: 'local',
        dispatch_group: 'B',
        estimated_minutes: 25,
        heartbeat_minutes: 10,
        write_scope: ['scripts/beta.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
      // pseudo via id substring — defence-in-depth skip
      {
        task_id: 'wave99-99-archive-prior-task-docs',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'C',
        estimated_minutes: 5,
        heartbeat_minutes: 5,
        write_scope: ['.missiond/claudecode/_archive/foo.md'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
      // pseudo via :kind — defence-in-depth skip
      {
        task_id: 'wave99-50-helper',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'D',
        estimated_minutes: 10,
        heartbeat_minutes: 5,
        write_scope: ['scripts/helper.mjs'],
        notes: null,
        owner: null,
        kind: 'backfill',
        loc: null,
      },
    ],
    loc: null,
  };
  const loopSmokeContracts = {
    '.missiond/tasks/wave99/wave99-01-alpha.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-01-alpha', message: 'feat(tasks): wave28-06 alpha' }),
    ),
    '.missiond/tasks/wave99/wave99-02-beta.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-02-beta', message: 'feat(tasks): wave28-06 beta' }),
    ),
  };
  const loopSmokeReports = {
    '.missiond/tasks/wave99/reports/wave99-01-alpha.report.lisp': loadReportFromSource(
      buildReportSource({ id: 'wave99-01-alpha', commitHash: '1111aaa' }),
      '<fx-report-loop-alpha>',
    ),
    '.missiond/tasks/wave99/reports/wave99-02-beta.report.lisp': loadReportFromSource(
      buildReportSource({ id: 'wave99-02-beta', commitHash: '2222bbb' }),
      '<fx-report-loop-beta>',
    ),
  };
  const loopSmokeLedgers = {
    '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
      buildLedgerSource('wave99', [
        { task: 'wave99-01-alpha', summary: 'done at commit 1111aaa' },
        { task: 'wave99-02-beta', summary: 'done at commit 2222bbb' },
      ]),
      '<fx-ledger-loop-smoke>',
    ),
  };
  const loopSmokeCommits = {
    '1111aaa': syntheticCommit('1111aaa', 'feat(tasks): wave28-06 alpha'),
    '2222bbb': syntheticCommit('2222bbb', 'feat(tasks): wave28-06 beta'),
  };
  fixtures.push({
    name: 'wave28-06-loop-smoke-batch-aggregate-aligns-with-plan',
    manifest: loopSmokeManifest,
    manifestPath: '.missiond/tasks/wave99/manifest-loop-smoke.lisp',
    loaders: syntheticLoaders({
      contracts: loopSmokeContracts,
      reports: loopSmokeReports,
      ledgers: loopSmokeLedgers,
      commits: loopSmokeCommits,
    }),
    expect: {
      // Productive-node accounting MUST equal 2 (alpha + beta), matching
      // what the wave28-02 plan CLI would topological-batch from the same
      // manifest. The two pseudo nodes are silently skipped.
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 2,
      total_nodes: 2,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      // skipped_nodes is sorted lexicographically by the verifier.
      skipped_nodes: ['wave99-50-helper', 'wave99-99-archive-prior-task-docs'],
    },
  });

  // ---------------------------------------------------------------------
  // wave29-04 lineage fixtures — pin parent-hotfix lineage acceptance.
  // The wave28-02 case (worker commit 954116e then parent lint-cleanup
  // commit 302330a) must verify when the memory completion summary cites
  // EITHER the worker commit OR the final commit. The verified row should
  // always point at the final commit (the verification hash resolves to
  // the FINAL commit by contract).
  // ---------------------------------------------------------------------
  const lineageManifest = {
    ...greenManifest,
    id: 'm-wave29-04-lineage',
    nodes: [
      {
        task_id: 'wave99-01-foo',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'A',
        estimated_minutes: 30,
        heartbeat_minutes: 10,
        write_scope: ['scripts/foo.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
    ],
  };
  const lineageContracts = {
    '.missiond/tasks/wave99/wave99-01-foo.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-01-foo', message: 'feat(tasks): fixture' }),
    ),
  };
  // Report mirrors the wave28-02 lineage shape: final commit is "302330a",
  // worker commit is "954116e", :parent_patches[0].commit = final.
  const lineageReports = {
    '.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp': loadReportFromSource(
      buildReportSource({
        id: 'wave99-01-foo',
        commitHash: '302330a',
        agentCommitHash: '954116e',
        parentPatches: [
          {
            commit: '302330a',
            kind: 'lint-cleanup',
            reason: 'TS6133 unused parameter cleanup',
            files: ['scripts/foo.mjs'],
          },
        ],
      }),
      '<fx-lin-final-report>',
    ),
  };
  const lineageCommits = {
    '302330a': syntheticCommit('302330a'),
  };
  // wave29-04 case A: memory completion summary mentions the FINAL commit.
  fixtures.push({
    name: 'wave29-04 lineage pass: memory summary cites final commit (302330a)',
    manifest: lineageManifest,
    manifestPath: '.missiond/tasks/wave99/manifest-lineage.lisp',
    loaders: syntheticLoaders({
      contracts: lineageContracts,
      reports: lineageReports,
      ledgers: {
        '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
          buildLedgerSource('wave99', [
            { task: 'wave99-01-foo', summary: 'done at commit 302330a (post-hotfix)' },
          ]),
          '<fx-lin-mem-final>',
        ),
      },
      commits: lineageCommits,
    }),
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 1,
      total_nodes: 1,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: [],
    },
  });
  // wave29-04 case B: memory completion summary mentions the WORKER commit
  // (some completions are written post-worker-commit but pre-hotfix). The
  // verifier MUST accept this; the verified row still points at the final
  // commit. This is the load-bearing requirement #4 from the contract.
  fixtures.push({
    name: 'wave29-04 lineage pass: memory summary cites worker commit (954116e), verified row resolves to final',
    manifest: lineageManifest,
    manifestPath: '.missiond/tasks/wave99/manifest-lineage.lisp',
    loaders: syntheticLoaders({
      contracts: lineageContracts,
      reports: lineageReports,
      ledgers: {
        '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
          buildLedgerSource('wave99', [
            { task: 'wave99-01-foo', summary: 'done at commit 954116e (pre-hotfix)' },
          ]),
          '<fx-lin-mem-agent>',
        ),
      },
      commits: lineageCommits,
    }),
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 1,
      total_nodes: 1,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: [],
    },
  });
  // wave29-04 case C: memory completion summary mentions a hash NOT in the
  // lineage. MUST fail with the structured commit-hash-mismatch error.
  fixtures.push({
    name: 'wave29-04 lineage fail: memory summary cites hash outside lineage',
    manifest: lineageManifest,
    manifestPath: '.missiond/tasks/wave99/manifest-lineage.lisp',
    loaders: syntheticLoaders({
      contracts: lineageContracts,
      reports: lineageReports,
      ledgers: {
        '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
          buildLedgerSource('wave99', [
            { task: 'wave99-01-foo', summary: 'done at commit cafe123 (typo)' },
          ]),
          '<fx-lin-mem-bad>',
        ),
      },
      commits: lineageCommits,
    }),
    expect: {
      aggregate_status: STATUS_FAILED,
      verified_nodes: 0,
      total_nodes: 1,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications_task_ids: ['wave99-01-foo'],
      failed_contract_verifications_reason_match: /commit hash mismatch/,
      skipped_nodes: [],
    },
  });

  // Run all fixtures and collect failures.
  const failures = [];
  for (const fx of fixtures) {
    const result = verifyManifest({
      manifestPath: fx.manifestPath,
      manifest: fx.manifest,
      loaders: fx.loaders,
    });
    const fxFailures = checkExpect(result, fx.expect);
    if (fxFailures.length > 0) {
      failures.push({ name: fx.name, failures: fxFailures, got: result });
    }

    // Determinism check: re-run with the same inputs and compare bytes.
    if (fx.expect.determinism_check) {
      const second = verifyManifest({
        manifestPath: fx.manifestPath,
        manifest: fx.manifest,
        loaders: fx.loaders,
      });
      const aBytes = stableStringify(result);
      const bBytes = stableStringify(second);
      if (aBytes !== bBytes) {
        failures.push({
          name: fx.name,
          failures: ['determinism: byte-identical JSON expected on re-run'],
          diff: { a: aBytes, b: bBytes },
        });
      }
    }
  }

  const ok = failures.length === 0;
  if (json) {
    console.log(
      stableStringify({
        ok,
        fixtures: fixtures.map((fx) => fx.name),
        failures,
      }),
    );
  } else if (ok) {
    console.log(
      `task-runner-batch verify fixtures OK ` +
      `(${fixtures.length} fixture${fixtures.length === 1 ? '' : 's'})`,
    );
  } else {
    console.error(
      `task-runner-batch verify fixtures FAILED — ${failures.length} of ${fixtures.length}`,
    );
    for (const f of failures) console.error(JSON.stringify(f, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

// Compare a verifyManifest result against a partial expectation. Returns an
// array of human-readable failure strings (empty = match).
function checkExpect(actual, expect) {
  const out = [];
  if (expect.aggregate_status !== undefined && actual.aggregate_status !== expect.aggregate_status) {
    out.push(
      `aggregate_status: expected ${expect.aggregate_status}, got ${actual.aggregate_status}`,
    );
  }
  if (expect.verified_nodes !== undefined && actual.verified_nodes !== expect.verified_nodes) {
    out.push(`verified_nodes: expected ${expect.verified_nodes}, got ${actual.verified_nodes}`);
  }
  if (expect.total_nodes !== undefined && actual.total_nodes !== expect.total_nodes) {
    out.push(`total_nodes: expected ${expect.total_nodes}, got ${actual.total_nodes}`);
  }
  if (expect.missing_reports !== undefined) {
    if (!arraysEqual(actual.missing_reports, expect.missing_reports)) {
      out.push(
        `missing_reports: expected ${JSON.stringify(expect.missing_reports)}, ` +
        `got ${JSON.stringify(actual.missing_reports)}`,
      );
    }
  }
  if (expect.missing_memory_completions !== undefined) {
    if (!arraysEqual(actual.missing_memory_completions, expect.missing_memory_completions)) {
      out.push(
        `missing_memory_completions: expected ${JSON.stringify(expect.missing_memory_completions)}, ` +
        `got ${JSON.stringify(actual.missing_memory_completions)}`,
      );
    }
  }
  if (expect.failed_contract_verifications !== undefined) {
    if (
      !arraysEqual(
        actual.failed_contract_verifications.map((r) => r.task_id),
        expect.failed_contract_verifications.map((r) => r.task_id),
      )
    ) {
      out.push(
        `failed_contract_verifications task_ids: expected ` +
        `${JSON.stringify(expect.failed_contract_verifications.map((r) => r.task_id))}, ` +
        `got ${JSON.stringify(actual.failed_contract_verifications.map((r) => r.task_id))}`,
      );
    }
  }
  if (expect.failed_contract_verifications_task_ids !== undefined) {
    const ids = actual.failed_contract_verifications.map((r) => r.task_id);
    if (!arraysEqual(ids, expect.failed_contract_verifications_task_ids)) {
      out.push(
        `failed_contract_verifications task_ids: expected ` +
        `${JSON.stringify(expect.failed_contract_verifications_task_ids)}, got ${JSON.stringify(ids)}`,
      );
    }
  }
  if (expect.failed_contract_verifications_reason_match !== undefined) {
    const reasons = actual.failed_contract_verifications.map((r) => r.reason);
    if (!reasons.some((r) => expect.failed_contract_verifications_reason_match.test(r))) {
      out.push(
        `failed_contract_verifications reason: expected one matching ` +
        `${expect.failed_contract_verifications_reason_match}, got ${JSON.stringify(reasons)}`,
      );
    }
  }
  if (expect.skipped_nodes !== undefined) {
    if (!arraysEqual(actual.skipped_nodes, expect.skipped_nodes)) {
      out.push(
        `skipped_nodes: expected ${JSON.stringify(expect.skipped_nodes)}, ` +
        `got ${JSON.stringify(actual.skipped_nodes)}`,
      );
    }
  }
  return out;
}

function arraysEqual(a, b) {
  if (!Array.isArray(a) || !Array.isArray(b)) return false;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

// Tiny shim around loadContractFromSource so the fixture code reads as a
// single helper call. Lifted to a named function for stack-trace clarity.
function loadContractFromSourceShim(source) {
  return loadContractFromSource(source, '<fx-contract>');
}

// --- Entrypoint ----------------------------------------------------------

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runFixtures({ json: opts.json });
  } else {
    runCli(opts);
  }
}
