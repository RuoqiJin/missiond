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
// wave29-05: optional verification-receipt loading + reuse coverage. The
// receipts are advisory caches of command evidence — they NEVER substitute
// for source facts and NEVER substitute for commit verification. The batch
// verifier MUST still verify task contract, report, memory completion, and
// git commit even when --receipts is supplied; the only effect of receipts
// here is the new receipt_coverage field in the JSON output (per-task
// receipt count + reuse decisions for downstream planners).
import {
  isReceiptReusable,
  readVerificationReceiptFile,
  validateReceiptObject,
  // wave37-01: optional cross-layer smoke for the request-local
  // verification-receipt projection. Receipts remain ADVISORY ONLY; the
  // request-local writer only adds an additive smoke fixture and never
  // changes the default --receipts JSON shape.
  writeRequestVerificationReceiptFile,
} from './check-verification-receipt.mjs';
import { checkSuppliedFiles } from './check-staged-source-hygiene.mjs';
import { validateLifecycleEventFiles } from './check-task-lifecycle-events.mjs';
import { appendLifecycleEvent } from './task-runner-append-event.mjs';
import { planParentHotfixFromSource } from './task-runner-parent-hotfix.mjs';
import { planFromManifestObject } from './plan-task-runner.mjs';

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
  --receipts <file>   OPTIONAL wave29-05 verification-receipt v1 Lisp
                      file. When supplied, receipts are loaded + structurally
                      validated and the JSON output gains a receipt_coverage
                      field with per-task receipt counts and reuse decisions
                      against each node's expected dry-fixture command.
                      Receipts are ADVISORY ACCELERATION ONLY — the batch
                      verifier STILL verifies task contract, report, memory
                      completion, and git commit for every productive node
                      regardless of receipt coverage.
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
    receipts: null,
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
    } else if (arg === '--receipts') {
      opts.receipts = argv[++i] ?? failUsage('--receipts requires a value');
    } else if (arg.startsWith('--receipts=')) {
      opts.receipts = arg.slice('--receipts='.length);
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

// wave30-01: the finalized report is the completion truth. Prefer the
// explicit verified hash, then final hash, then legacy commit_hash. This is
// exported so parent-hotfix finalizers and smoke fixtures can share the
// same final-commit resolution rule as the batch verifier.
export function finalVerificationHash(report) {
  return report?.verifiedCommitHash ?? report?.finalCommitHash ?? report?.commitHash ?? null;
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
  const verificationHash = finalVerificationHash(report);
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

// --- Receipt coverage (wave29-05) -----------------------------------------

// Build a per-task receipt-coverage row from a list of validated receipts.
// Receipts are advisory: this helper reports HOW MANY receipts each task
// has and whether ANY of them are reusable against the task's expected
// dry-fixture command. The orchestrator MUST still verify the task
// contract / report / memory completion / git commit even when reuse is
// true. The receipt_coverage field is therefore a HINT for downstream
// planners (wave29-06 ready-queue, wave29-07 cross-layer smoke), not a
// pass/fail signal for this verifier.
//
// Returned shape (sorted by task_id ascending for byte-stable JSON):
//   [{ task_id, receipt_count, reusable_count,
//      reuse_query: { commit_hash, command, tier },
//      reuse_decisions: [{ receipt_id, reusable, reason }] }]
export function computeReceiptCoverage(manifest, perNodeResults, receipts) {
  if (!Array.isArray(receipts) || receipts.length === 0) {
    return [];
  }
  // Index receipts by task_id once so each node lookup is O(per-task).
  const byTask = new Map();
  for (const r of receipts) {
    if (!r || typeof r.task_id !== 'string') continue;
    if (!byTask.has(r.task_id)) byTask.set(r.task_id, []);
    byTask.get(r.task_id).push(r);
  }
  const out = [];
  const productiveResultsByTask = new Map();
  for (const r of perNodeResults) {
    if (r.kind === 'skipped') continue;
    productiveResultsByTask.set(r.task_id, r);
  }
  for (const node of manifest.nodes ?? []) {
    if (manifest.productive_only === false || isPseudoNode(node)) continue;
    const taskId = node.task_id;
    const taskReceipts = byTask.get(taskId) ?? [];
    if (taskReceipts.length === 0) continue;
    // The reuse query is the worker's expected dry-fixture command for
    // this node. We mirror the per-task contract pattern (the manifest
    // node carries verification_tier; we run reuse decisions ONCE per
    // unique receipt against the node's tier so the planner can see what
    // would be reused).
    const productive = productiveResultsByTask.get(taskId);
    const queryCommitHash = productive?.commit_hash ?? null;
    const queryTier = node.verification_tier ?? null;
    const decisions = [];
    let reusableCount = 0;
    for (const r of taskReceipts) {
      // The reuse query must use the receipt's OWN command (since the
      // manifest doesn't pin a specific command per node). If the
      // commit/tier diverge from the receipt, the helper returns false.
      const query = {
        commit_hash: queryCommitHash ?? r.commit_hash,
        command: r.command,
        tier: queryTier ?? r.tier,
      };
      const reusable = isReceiptReusable(r, query);
      if (reusable) reusableCount += 1;
      decisions.push({
        receipt_id: r.id ?? null,
        reusable,
        reason: reusable ? 'all four conservative reuse rules satisfied' : describeReuseFailure(r, query),
      });
    }
    out.push({
      task_id: taskId,
      receipt_count: taskReceipts.length,
      reusable_count: reusableCount,
      reuse_query: {
        commit_hash: queryCommitHash,
        tier: queryTier,
      },
      reuse_decisions: decisions.sort((a, b) => {
        const ai = a.receipt_id ?? '';
        const bi = b.receipt_id ?? '';
        return ai.localeCompare(bi);
      }),
    });
  }
  return out.sort((a, b) => a.task_id.localeCompare(b.task_id));
}

// Human-readable reason for why a receipt failed reuse. Used only in the
// receipt_coverage hint output — the verifier itself never gates on this.
function describeReuseFailure(receipt, query) {
  if (!Number.isInteger(receipt?.exit_code) || receipt.exit_code !== 0) {
    return `receipt :exit_code=${receipt?.exit_code} (rule 3: must be 0)`;
  }
  const a = typeof receipt?.command === 'string' ? receipt.command.trim() : null;
  const b = typeof query?.command === 'string' ? query.command.trim() : null;
  if (a == null || b == null || a !== b) {
    return 'rule 2: :command does not match query exactly';
  }
  // Commit hash check uses the same prefix-agree rule as isReceiptReusable.
  if (typeof receipt?.commit_hash !== 'string' || typeof query?.commit_hash !== 'string') {
    return 'rule 1: :commit_hash missing on receipt or query';
  }
  const ax = receipt.commit_hash.trim().toLowerCase();
  const bx = query.commit_hash.trim().toLowerCase();
  if (ax !== bx) {
    const longer = ax.length >= bx.length ? ax : bx;
    const shorter = ax.length >= bx.length ? bx : ax;
    if (!(shorter.length >= 7 && longer.startsWith(shorter))) {
      return 'rule 1: :commit_hash does not agree with query commit';
    }
  }
  return 'rule 4: receipt :tier does not cover query tier (full > smoke > local)';
}

// --- Aggregate ------------------------------------------------------------

// Build the final batch-verification report. All array fields are sorted
// (task_ids ascending) so the JSON is byte-identical for byte-identical
// inputs regardless of node ordering.
//
// wave29-05: when `receipts` is supplied (Array of validated receipt
// objects), the result gains a `receipt_coverage` field describing
// per-task receipt counts and reuse decisions. When `receipts` is
// undefined or null the field is OMITTED so the 12 wave28-05 + wave29-04
// baseline fixtures emit byte-identical bytes (backward compat).
export function aggregateResults(manifestPath, manifest, perNodeResults, receipts) {
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
  const out = {
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
  // wave29-05: only inject receipt_coverage when receipts were supplied,
  // so prior 12 baseline fixtures remain byte-identical.
  if (receipts != null) {
    out.receipt_coverage = computeReceiptCoverage(manifest, perNodeResults, receipts);
  }
  return out;
}

// --- Top-level orchestration ----------------------------------------------

// Verify a manifest given a set of loaders (real loaders for the CLI path,
// synthetic loaders for fixtures). Returns the aggregate report shape from
// aggregateResults.
//
// wave29-05: an OPTIONAL `receipts` parameter (Array of validated receipt
// objects produced by readVerificationReceiptFile) attaches receipt-
// coverage hints to the aggregate output. Receipts are advisory; the
// verifier STILL verifies task contract / report / memory completion /
// git commit for every productive node (steps 0-6 in verifyNode) even
// when receipts are present. When `receipts` is omitted/null the
// aggregate output is byte-identical to the wave28-05 + wave29-04
// baseline (no receipt_coverage field emitted).
export function verifyManifest({
  manifestPath,
  manifest,
  loaders,
  receipts = null,
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
  return aggregateResults(manifestPath, manifest, perNode, receipts);
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

  // wave29-05: optional --receipts loading. Receipts are ADVISORY ONLY;
  // failure to load them is a hard error so the operator notices the typo,
  // but successful loading does NOT bypass any of the task-contract /
  // report / memory / commit verification steps in verifyNode. We
  // structurally validate each receipt (via validateReceiptObject) before
  // surfacing it to computeReceiptCoverage; malformed receipts cause a
  // hard CLI failure (the orchestrator should not silently ingest broken
  // evidence caches).
  let receipts = null;
  if (opts.receipts) {
    const receiptsPath = path.resolve(cwd, opts.receipts);
    if (!fs.existsSync(receiptsPath)) {
      process.stderr.write(`error: receipts file not found at ${receiptsPath}\n`);
      process.exit(1);
    }
    try {
      receipts = readVerificationReceiptFile(receiptsPath);
    } catch (err) {
      process.stderr.write(`error: failed to read receipts: ${err?.message ?? err}\n`);
      process.exit(1);
    }
    const receiptErrors = [];
    for (const r of receipts) {
      const errs = validateReceiptObject(r);
      if (errs.length > 0) {
        receiptErrors.push({ id: r?.id ?? '<unknown>', errors: errs });
      }
    }
    if (receiptErrors.length > 0) {
      process.stderr.write(
        `error: ${receiptsPath} contains ${receiptErrors.length} invalid receipt(s):\n`,
      );
      for (const re of receiptErrors) {
        process.stderr.write(`  ${re.id}: ${re.errors.join('; ')}\n`);
      }
      process.exit(1);
    }
  }

  const result = verifyManifest({
    manifestPath: opts.manifest, // emit as supplied (relative when given relative)
    manifest,
    loaders,
    receipts,
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
  finalCommitHash = null,
  verifiedCommitHash = null,
  parentPatches = null,
}) {
  const agentLine = agentCommitHash
    ? `  :agent_commit_hash "${agentCommitHash}"\n`
    : '';
  const finalLine = finalCommitHash
    ? `  :final_commit_hash "${finalCommitHash}"\n`
    : '';
  const verifiedLine = verifiedCommitHash
    ? `  :verified_commit_hash "${verifiedCommitHash}"\n`
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
    finalLine +
    verifiedLine +
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

  // wave30-01: pin the real Wave29-03 drift shape. The worker reported
  // d36de80, the parent later applied d842b1d, and the finalized report
  // must verify against the finalized commit while still accepting a
  // shared-memory completion that mentions the worker commit.
  const parentFinalizerManifest = {
    ...greenManifest,
    id: 'm-wave30-01-parent-finalizer',
    nodes: [
      {
        task_id: 'wave99-03-runner-prep',
        depends_on: [],
        verification_tier: 'local',
        dispatch_group: 'A',
        estimated_minutes: 30,
        heartbeat_minutes: 10,
        write_scope: ['scripts/prepare-task-runner-wave.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
    ],
  };
  const parentFinalizerContracts = {
    '.missiond/tasks/wave99/wave99-03-runner-prep.lisp': loadContractFromSourceShim(
      buildContractSource({
        id: 'wave99-03-runner-prep',
        message: 'feat(tasks): prepare runner wave',
        writeScope: ['scripts/prepare-task-runner-wave.mjs'],
      }),
    ),
  };
  const parentFinalizerReports = {
    '.missiond/tasks/wave99/reports/wave99-03-runner-prep.report.lisp': loadReportFromSource(
      buildReportSource({
        id: 'wave99-03-runner-prep',
        commitHash: 'd842b1d',
        agentCommitHash: 'd36de80',
        finalCommitHash: 'd842b1d4a9c2',
        verifiedCommitHash: 'd842b1d',
        files: ['scripts/prepare-task-runner-wave.mjs'],
        parentPatches: [
          {
            commit: 'd842b1d',
            kind: 'lint-cleanup',
            reason: 'TS80007 sync await cleanup after worker commit',
            files: ['scripts/prepare-task-runner-wave.mjs'],
          },
        ],
      }),
      '<fx-wave30-01-finalized-report>',
    ),
  };
  fixtures.push({
    name: 'wave30-01 parent hotfix finalizer accepts worker-memory hash and verifies final commit',
    manifest: parentFinalizerManifest,
    manifestPath: '.missiond/tasks/wave99/manifest-wave30-01.lisp',
    loaders: syntheticLoaders({
      contracts: parentFinalizerContracts,
      reports: parentFinalizerReports,
      ledgers: {
        '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
          buildLedgerSource('wave99', [
            { task: 'wave99-03-runner-prep', summary: 'worker completed at commit d36de80 before parent hotfix' },
          ]),
          '<fx-wave30-01-ledger>',
        ),
      },
      commits: {
        d842b1d: syntheticCommit(
          'd842b1d',
          'feat(tasks): prepare runner wave',
          ['scripts/prepare-task-runner-wave.mjs'],
        ),
      },
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

  // ---------------------------------------------------------------------
  // wave29-05 receipt-coverage fixtures — pin (a) backward compat: when
  // no receipts are supplied the JSON is BYTE-IDENTICAL to the wave28-05
  // + wave29-04 baseline (the all-green 2-node manifest is re-run with
  // and without receipts and the without-receipts JSON is compared
  // against the with-receipts JSON minus the receipt_coverage key) and
  // (b) reuse-rule semantics: a matching commit + command + zero-exit +
  // tier-cover receipt is reported reusable; mismatched commit / command
  // / tier / non-zero exit is reported NOT reusable. The 12 baseline
  // fixtures above continue to call verifyManifest WITHOUT receipts so
  // their JSON stays byte-identical.
  // ---------------------------------------------------------------------
  const receiptCoverageReceiptsReusable = [
    {
      id: 'wave99-01-foo-abc1234-smoke',
      wave: 'wave99',
      task_id: 'wave99-01-foo',
      commit_hash: 'abc1234',
      command: 'node scripts/check-foo.mjs --dry-fixture',
      exit_code: 0,
      tier: 'smoke',
      started_at: null,
      finished_at: null,
      duration_ms: 200,
      files: ['scripts/foo.mjs'],
      notes: null,
      loc: null,
    },
  ];
  fixtures.push({
    name: 'wave29-05 receipts pass: matching receipt ⇒ reusable_count=1, full verification still runs',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: greenLedgers,
      commits: greenCommits,
    }),
    receipts: receiptCoverageReceiptsReusable,
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 2,
      total_nodes: 2,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: [],
      receipt_coverage_present: true,
      receipt_coverage_reusable_count_for: { task: 'wave99-01-foo', count: 1 },
    },
  });
  // wave29-05 case B: receipt's commit does NOT match the report commit
  // (the report says abc1234, the receipt says cafebabe). The receipt
  // structurally validates, but isReceiptReusable returns false. The
  // verifier MUST still mark the manifest all_green (receipts never gate
  // verification) and the receipt_coverage row reports reusable_count=0.
  const receiptCoverageReceiptsNotReusable = [
    {
      id: 'wave99-01-foo-cafebab-smoke',
      wave: 'wave99',
      task_id: 'wave99-01-foo',
      commit_hash: 'cafebab',
      command: 'node scripts/check-foo.mjs --dry-fixture',
      exit_code: 0,
      tier: 'smoke',
      started_at: null,
      finished_at: null,
      duration_ms: 200,
      files: ['scripts/foo.mjs'],
      notes: null,
      loc: null,
    },
  ];
  fixtures.push({
    name: 'wave29-05 receipts pass: stale-commit receipt ⇒ reusable_count=0; verification still all_green',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: greenLedgers,
      commits: greenCommits,
    }),
    receipts: receiptCoverageReceiptsNotReusable,
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 2,
      total_nodes: 2,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: [],
      receipt_coverage_present: true,
      receipt_coverage_reusable_count_for: { task: 'wave99-01-foo', count: 0 },
    },
  });
  // wave29-05 case C: backward compat — invoking verifyManifest WITHOUT
  // receipts on the same green inputs MUST produce JSON that, when
  // augmented with a `receipt_coverage: []` key, is byte-identical to a
  // verifyManifest call WITH receipts=[] supplied. This pins the rule
  // that omitting --receipts truly omits the field entirely (no empty
  // array sneaking in).
  fixtures.push({
    name: 'wave29-05 backward-compat: no receipts ⇒ no receipt_coverage key; receipts=[] ⇒ receipt_coverage=[]',
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
      receipt_coverage_absent_when_no_receipts: true,
    },
  });
  // wave29-05 case D: full receipt covers smoke query (tier-cover rule).
  // The greenManifest declares verification_tier=local for the nodes.
  // We supply a `full` receipt and confirm reusable_count=1 — full > local.
  const receiptCoverageReceiptsFullCoversLocal = [
    {
      id: 'wave99-01-foo-abc1234-full',
      wave: 'wave99',
      task_id: 'wave99-01-foo',
      commit_hash: 'abc1234',
      command: 'node scripts/check-foo.mjs --dry-fixture',
      exit_code: 0,
      tier: 'full',
      started_at: null,
      finished_at: null,
      duration_ms: 5000,
      files: [],
      notes: null,
      loc: null,
    },
  ];
  // ---------------------------------------------------------------------
  // wave29-07 cross-layer smoke (layer H): pin the batch verifier as the
  // capstone for the runner-efficiency contract. One synthetic 3-node
  // manifest where every report carries the wave29-04 lineage fields
  // (worker commit + parent_patches + final commit_hash matching the
  // trailing parent_patches[-1].commit); every shared-memory completion
  // cites a hash inside the lineage; every git stub commit aligns with
  // the report's final hash; AND wave29-05 receipts are supplied for one
  // node so the receipt_coverage field is populated. The verifier MUST
  // return aggregate_status=all_green AND receipt_coverage MUST be
  // present with reusable_count=1 for the receipt-bearing node. This is
  // the layer-H pin: a regression where the lineage join, memory join,
  // git join, OR receipt coverage breaks surfaces here.
  // ---------------------------------------------------------------------
  const crossLayerManifest = {
    id: 'm-wave29-07-cross-layer',
    schema: 'missiond.task-runner-manifest.v1',
    wave: 'wave99',
    brief_mode: 'thin',
    shared_preamble_path: '.missiond/claudecode/wave99-shared-preamble.md',
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
        estimated_minutes: 25,
        heartbeat_minutes: 10,
        write_scope: ['scripts/bar.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
      {
        task_id: 'wave99-03-baz',
        depends_on: ['wave99-01-foo'],
        verification_tier: 'local',
        dispatch_group: 'C',
        estimated_minutes: 20,
        heartbeat_minutes: 10,
        write_scope: ['scripts/baz.mjs'],
        notes: null,
        owner: null,
        kind: null,
        loc: null,
      },
    ],
    loc: null,
  };
  const crossLayerContracts = {
    '.missiond/tasks/wave99/wave99-01-foo.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-01-foo', message: 'feat(tasks): wave29-07 foo' }),
    ),
    '.missiond/tasks/wave99/wave99-02-bar.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-02-bar', message: 'feat(tasks): wave29-07 bar' }),
    ),
    '.missiond/tasks/wave99/wave99-03-baz.lisp': loadContractFromSourceShim(
      buildContractSource({ id: 'wave99-03-baz', message: 'feat(tasks): wave29-07 baz' }),
    ),
  };
  // Every report carries lineage fields. final commit_hash MUST match
  // trailing :parent_patches[-1].commit (wave29-04 drift rule).
  const crossLayerReports = {
    '.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp': loadReportFromSource(
      buildReportSource({
        id: 'wave99-01-foo',
        commitHash: 'aa11bb2',
        agentCommitHash: 'aa11bb1',
        parentPatches: [
          {
            commit: 'aa11bb2',
            kind: 'lint-cleanup',
            reason: 'TS6133 unused parameter cleanup',
            files: ['scripts/foo.mjs'],
          },
        ],
      }),
      '<fx-wave29-07-foo>',
    ),
    '.missiond/tasks/wave99/reports/wave99-02-bar.report.lisp': loadReportFromSource(
      buildReportSource({
        id: 'wave99-02-bar',
        commitHash: 'cc33dd2',
        agentCommitHash: 'cc33dd1',
        parentPatches: [
          {
            commit: 'cc33dd2',
            kind: 'lint-cleanup',
            reason: 'TS6133 unused parameter cleanup',
            files: ['scripts/bar.mjs'],
          },
        ],
      }),
      '<fx-wave29-07-bar>',
    ),
    '.missiond/tasks/wave99/reports/wave99-03-baz.report.lisp': loadReportFromSource(
      buildReportSource({
        id: 'wave99-03-baz',
        commitHash: 'ee55ff2',
        agentCommitHash: 'ee55ff1',
        parentPatches: [
          {
            commit: 'ee55ff2',
            kind: 'lint-cleanup',
            reason: 'TS6133 unused parameter cleanup',
            files: ['scripts/baz.mjs'],
          },
        ],
      }),
      '<fx-wave29-07-baz>',
    ),
  };
  // Memory completions cite the FINAL commit hash for each node.
  const crossLayerLedgers = {
    '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
      buildLedgerSource('wave99', [
        { task: 'wave99-01-foo', summary: 'done at commit aa11bb2 (post-hotfix)' },
        { task: 'wave99-02-bar', summary: 'done at commit cc33dd2 (post-hotfix)' },
        { task: 'wave99-03-baz', summary: 'done at commit ee55ff2 (post-hotfix)' },
      ]),
      '<fx-wave29-07-ledger>',
    ),
  };
  const crossLayerCommits = {
    aa11bb2: syntheticCommit('aa11bb2', 'feat(tasks): wave29-07 foo'),
    cc33dd2: syntheticCommit('cc33dd2', 'feat(tasks): wave29-07 bar'),
    ee55ff2: syntheticCommit('ee55ff2', 'feat(tasks): wave29-07 baz'),
  };
  // wave29-05 receipt for the foo node only — proves receipt_coverage
  // populates and reusable_count=1 when the receipt's commit/command/tier
  // align with the report.
  const crossLayerReceipts = [
    {
      id: 'wave99-01-foo-aa11bb2-smoke',
      wave: 'wave99',
      task_id: 'wave99-01-foo',
      commit_hash: 'aa11bb2',
      command: 'node scripts/check-foo.mjs --dry-fixture',
      exit_code: 0,
      tier: 'smoke',
      started_at: null,
      finished_at: null,
      duration_ms: 250,
      files: ['scripts/foo.mjs'],
      notes: null,
      loc: null,
    },
  ];
  fixtures.push({
    name: 'wave29-07-loop-smoke-cross-layer-batch-verifies',
    manifest: crossLayerManifest,
    manifestPath: '.missiond/tasks/wave99/manifest-wave29-07.lisp',
    loaders: syntheticLoaders({
      contracts: crossLayerContracts,
      reports: crossLayerReports,
      ledgers: crossLayerLedgers,
      commits: crossLayerCommits,
    }),
    receipts: crossLayerReceipts,
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 3,
      total_nodes: 3,
      missing_reports: [],
      missing_memory_completions: [],
      failed_contract_verifications: [],
      skipped_nodes: [],
      receipt_coverage_present: true,
      receipt_coverage_reusable_count_for: { task: 'wave99-01-foo', count: 1 },
    },
  });

  // ---------------------------------------------------------------------
  // wave30-05 lifecycle/receipt/finalization smoke: one synthetic flow
  // starts from a worker draft report, appends a parent-hotfix lifecycle
  // event through the atomic append helper, projects a finalized report,
  // validates source hygiene, validates a reusable receipt for the FINAL
  // commit, verifies the batch against the finalized truth, and proves
  // ready-queue does not wait on a soft reference.
  // ---------------------------------------------------------------------
  {
    const smokeTask = 'wave99-04-lifecycle';
    const workerCommit = 'aa10aa1';
    const finalCommit = 'aa10aa2';
    const smokeCommand = 'node scripts/foo.mjs --dry-fixture';
    const workerDraft = buildReportSource({
      id: smokeTask,
      commitHash: workerCommit,
      files: ['scripts/foo.mjs'],
    });
    const parentPlan = planParentHotfixFromSource(workerDraft, {
      taskId: smokeTask,
      agentCommit: workerCommit,
      parentCommit: finalCommit,
      kind: 'lint-cleanup',
      reason: 'Wave30 smoke parent cleanup after worker commit',
      files: ['scripts/foo.mjs'],
      acceptanceCommands: [smokeCommand],
    });

    const tmp = fs.mkdtempSync(path.join(process.cwd(), '.tmp-wave30-05-'));
    try {
      fs.mkdirSync(path.join(tmp, 'scripts'), { recursive: true });
      fs.writeFileSync(path.join(tmp, 'scripts', 'foo.mjs'), 'export const ok = true;\n');
      const hygiene = checkSuppliedFiles({ files: ['scripts/foo.mjs'], cwd: tmp });
      if (!hygiene.ok) {
        throw new Error(`wave30-05 hygiene check failed: ${hygiene.errors.join('; ')}`);
      }

      const lifecyclePath = path.join(tmp, 'task-lifecycle-events.lisp');
      appendLifecycleEvent({
        ledgerPath: lifecyclePath,
        task: smokeTask,
        eventKind: 'parent_hotfix',
        actorRole: 'parent',
        commitRole: 'parent_hotfix',
        commitHash: finalCommit,
        touched: ['scripts/foo.mjs'],
        summary: 'Wave30 smoke parent hotfix after worker commit',
        reportPath: '.missiond/tasks/wave99/reports/wave99-04-lifecycle.report.lisp',
        receiptPath: '.missiond/tasks/wave99/receipts/wave99-04-lifecycle.receipt.lisp',
        at: '2026-04-28T00:00:00Z',
      });
      const lifecycle = validateLifecycleEventFiles([lifecyclePath]);
      if (!lifecycle.ok || lifecycle.events !== 1) {
        throw new Error('wave30-05 lifecycle event append did not validate');
      }

      // wave39-01 cross-layer smoke: project the same parent-hotfix event
      // into a task-scoped events-dir using the new --events-dir append mode
      // and revalidate it through the same checker. Additive only — does
      // not change the default verifyManifest JSON shape.
      const taskEventsDir = path.join(tmp, 'events');
      const taskScopedAppend = appendLifecycleEvent({
        eventsDir: taskEventsDir,
        task: smokeTask,
        eventKind: 'parent_hotfix',
        actorRole: 'parent',
        commitRole: 'parent_hotfix',
        commitHash: finalCommit,
        touched: ['scripts/foo.mjs'],
        summary: 'Wave39 task-scoped events-dir parent hotfix',
        reportPath: '.missiond/tasks/wave99/reports/wave99-04-lifecycle.report.lisp',
        receiptPath: '.missiond/tasks/wave99/receipts/wave99-04-lifecycle.receipt.lisp',
        at: '2026-04-28T00:00:30Z',
        wave: 'wave99',
      });
      if (!taskScopedAppend.eventFile?.endsWith('000001.event.lisp')) {
        throw new Error(
          `wave39-01 task-scoped events-dir smoke expected 000001.event.lisp, got ${taskScopedAppend.eventFile}`,
        );
      }
      const taskScopedCheck = validateLifecycleEventFiles([taskScopedAppend.eventFile]);
      if (!taskScopedCheck.ok || taskScopedCheck.task_event_files !== 1) {
        throw new Error(
          `wave39-01 task-scoped events-dir smoke failed validation: ${taskScopedCheck.diagnostics.map((d) => d.message).join('; ')}`,
        );
      }
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }

    const smokeReceipt = {
      id: `${smokeTask}-${finalCommit}-smoke`,
      wave: 'wave99',
      task_id: smokeTask,
      commit_hash: finalCommit,
      command: smokeCommand,
      exit_code: 0,
      tier: 'smoke',
      started_at: null,
      finished_at: null,
      duration_ms: 250,
      files: ['scripts/foo.mjs'],
      notes: null,
      loc: null,
    };
    const receiptErrors = validateReceiptObject(smokeReceipt);
    if (receiptErrors.length > 0) {
      throw new Error(`wave30-05 receipt invalid: ${receiptErrors.join('; ')}`);
    }
    if (!isReceiptReusable(smokeReceipt, {
      commit_hash: finalCommit,
      command: smokeCommand,
      tier: 'local',
    })) {
      throw new Error('wave30-05 receipt should cover the local reuse query');
    }

    // wave37-01 cross-layer smoke: project the same smoke receipt into a
    // request-local .missiond/requests/<request_id>/receipts/<receipt_id>.lisp
    // file and revalidate it through the same checker. Additive only — does
    // not change the default --receipts JSON shape (the smoke fixture still
    // carries the legacy receipt object array via `receipts`).
    const projectionTmp = fs.mkdtempSync(
      path.join(process.cwd(), '.tmp-wave37-01-receipt-smoke-'),
    );
    try {
      const requestId = 'req-wave99-04-lifecycle-smoke';
      const requestReceiptsDir = path.join(
        projectionTmp,
        '.missiond',
        'requests',
        requestId,
        'receipts',
      );
      const projected = writeRequestVerificationReceiptFile({
        requestReceiptsDir,
        requestId,
        receipt: smokeReceipt,
        receiptId: smokeReceipt.id,
      });
      if (projected.mode !== 'created') {
        throw new Error(
          `wave37-01 cross-layer smoke expected mode=created, got ${projected.mode}`,
        );
      }
      const projectedFromDisk = readVerificationReceiptFile(projected.path);
      if (projectedFromDisk.length !== 1 || projectedFromDisk[0].id !== smokeReceipt.id) {
        throw new Error(
          `wave37-01 cross-layer smoke: request-local projection did not parse back to one receipt with id=${smokeReceipt.id}`,
        );
      }
      if (!isReceiptReusable(projectedFromDisk[0], {
        commit_hash: finalCommit,
        command: smokeCommand,
        tier: 'local',
      })) {
        throw new Error(
          'wave37-01 cross-layer smoke: request-local projection should still satisfy the conservative reuse rules',
        );
      }
    } finally {
      fs.rmSync(projectionTmp, { recursive: true, force: true });
    }

    const smokeManifest = {
      id: 'm-wave30-05-lifecycle-smoke',
      schema: 'missiond.task-runner-manifest.v1',
      wave: 'wave99',
      brief_mode: 'thin',
      shared_preamble_path: '.missiond/claudecode/wave99-shared-preamble.md',
      productive_only: true,
      overlap_policy: 'reject',
      description: null,
      generated_at: null,
      generator: null,
      nodes: [
        {
          task_id: smokeTask,
          depends_on: [],
          verification_tier: 'local',
          dispatch_group: 'A',
          estimated_minutes: 20,
          heartbeat_minutes: 5,
          write_scope: ['scripts/foo.mjs'],
          notes: null,
          owner: null,
          kind: null,
          loc: null,
        },
      ],
      loc: null,
    };

    const smokeContracts = {
      [`.missiond/tasks/wave99/${smokeTask}.lisp`]: loadContractFromSourceShim(
        buildContractSource({
          id: smokeTask,
          message: 'feat(tasks): lifecycle smoke',
          writeScope: ['scripts/foo.mjs'],
        }),
      ),
    };
    const smokeReports = {
      [`.missiond/tasks/wave99/reports/${smokeTask}.report.lisp`]: loadReportFromSource(
        parentPlan.finalized_report_source,
        '<fx-wave30-05-finalized-report>',
      ),
    };
    const smokeLedgers = {
      '.missiond/tasks/wave99/shared-memory.lisp': loadLedgerFromSource(
        buildLedgerSource('wave99', [
          { task: smokeTask, summary: `worker completed at commit ${workerCommit}; parent finalized ${finalCommit}` },
        ]),
        '<fx-wave30-05-memory>',
      ),
    };
    const smokeCommits = {
      [finalCommit]: syntheticCommit(finalCommit, 'feat(tasks): lifecycle smoke', ['scripts/foo.mjs']),
    };

    fixtures.push({
      name: 'wave30-05 lifecycle receipt finalized-report smoke verifies',
      manifest: smokeManifest,
      manifestPath: '.missiond/tasks/wave99/manifest-wave30-05.lisp',
      loaders: syntheticLoaders({
        contracts: smokeContracts,
        reports: smokeReports,
        ledgers: smokeLedgers,
        commits: smokeCommits,
      }),
      receipts: [smokeReceipt],
      expect: {
        aggregate_status: STATUS_ALL_GREEN,
        verified_nodes: 1,
        total_nodes: 1,
        missing_reports: [],
        missing_memory_completions: [],
        failed_contract_verifications: [],
        skipped_nodes: [],
        receipt_coverage_present: true,
        receipt_coverage_reusable_count_for: { task: smokeTask, count: 1 },
      },
    });

    const readyPlan = planFromManifestObject({
      id: 'm-wave30-05-ready-queue-soft-ref',
      schema: 'missiond.task-runner-manifest.v2',
      wave: 'wave99',
      brief_mode: 'thin',
      shared_preamble_path: '.missiond/claudecode/wave99-shared-preamble.md',
      productive_only: true,
      overlap_policy: 'reject',
      description: null,
      generated_at: null,
      generator: null,
      nodes: [
        {
          task_id: 'wave99-01-anchor',
          depends_on: [],
          hard_deps: [],
          hard_deps_declared: false,
          soft_refs: [],
          verification_tier: 'local',
          dispatch_group: 'A',
          estimated_minutes: 10,
          heartbeat_minutes: 5,
          write_scope: ['scripts/anchor.mjs'],
        },
        {
          task_id: 'wave99-02-soft-slow',
          depends_on: [],
          hard_deps: [],
          hard_deps_declared: false,
          soft_refs: [],
          verification_tier: 'local',
          dispatch_group: 'A',
          estimated_minutes: 90,
          heartbeat_minutes: 10,
          write_scope: ['scripts/soft-slow.mjs'],
        },
        {
          task_id: 'wave99-03-follower',
          depends_on: ['wave99-01-anchor'],
          hard_deps: ['wave99-01-anchor'],
          hard_deps_declared: true,
          soft_refs: ['wave99-02-soft-slow'],
          verification_tier: 'local',
          dispatch_group: 'B',
          estimated_minutes: 5,
          heartbeat_minutes: 5,
          write_scope: ['scripts/follower.mjs'],
        },
      ],
    }, {
      manifest_path: '<wave30-05-ready-queue>',
      schedule: 'ready-queue',
    });
    if (!readyPlan.ok) {
      throw new Error(`wave30-05 ready-queue fixture failed: ${readyPlan.message}`);
    }
    const follower = readyPlan.plan.ready_queue.tasks.find((t) => t.task_id === 'wave99-03-follower');
    if (!follower || follower.ready_at_minutes !== 10) {
      throw new Error(
        `wave30-05 ready-queue should ignore soft_refs and release follower at 10, got ${follower?.ready_at_minutes}`,
      );
    }
  }

  fixtures.push({
    name: 'wave29-05 receipts pass: full-tier receipt covers local-tier node ⇒ reusable_count=1',
    manifest: greenManifest,
    manifestPath: '.missiond/tasks/wave99/manifest.lisp',
    loaders: syntheticLoaders({
      contracts: greenContracts,
      reports: greenReports,
      ledgers: greenLedgers,
      commits: greenCommits,
    }),
    receipts: receiptCoverageReceiptsFullCoversLocal,
    expect: {
      aggregate_status: STATUS_ALL_GREEN,
      verified_nodes: 2,
      total_nodes: 2,
      receipt_coverage_present: true,
      receipt_coverage_reusable_count_for: { task: 'wave99-01-foo', count: 1 },
    },
  });

  // Run all fixtures and collect failures.
  const failures = [];
  for (const fx of fixtures) {
    const result = verifyManifest({
      manifestPath: fx.manifestPath,
      manifest: fx.manifest,
      loaders: fx.loaders,
      receipts: fx.receipts ?? null,
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
        receipts: fx.receipts ?? null,
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

  // wave29-05: explicit byte-identical backward-compat check. Re-run the
  // first wave28-05 baseline fixture (the all-green 2-node manifest)
  // WITHOUT receipts and confirm the JSON output byte-matches a snapshot
  // captured before this task introduced the receipts plumbing. The
  // snapshot is the stableStringify of verifyManifest's result with NO
  // receipt_coverage key. We compare against re-running the same call
  // and assert the absence of receipt_coverage in the keys.
  {
    const baseline = verifyManifest({
      manifestPath: '.missiond/tasks/wave99/manifest.lisp',
      manifest: greenManifest,
      loaders: syntheticLoaders({
        contracts: greenContracts,
        reports: greenReports,
        ledgers: greenLedgers,
        commits: greenCommits,
      }),
      // receipts intentionally omitted
    });
    if (Object.prototype.hasOwnProperty.call(baseline, 'receipt_coverage')) {
      failures.push({
        name: 'wave29-05 backward-compat: baseline without receipts MUST NOT carry receipt_coverage',
        failures: ['baseline result has receipt_coverage key but no receipts were supplied'],
      });
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
  // wave29-05 receipt-coverage assertions.
  if (expect.receipt_coverage_present === true) {
    if (!Array.isArray(actual.receipt_coverage)) {
      out.push('receipt_coverage: expected an array, got ' + JSON.stringify(actual.receipt_coverage));
    }
  }
  if (expect.receipt_coverage_absent_when_no_receipts === true) {
    if (Object.prototype.hasOwnProperty.call(actual, 'receipt_coverage')) {
      out.push('receipt_coverage: expected ABSENT (no receipts supplied) but key was present');
    }
  }
  if (expect.receipt_coverage_reusable_count_for) {
    const want = expect.receipt_coverage_reusable_count_for;
    if (!Array.isArray(actual.receipt_coverage)) {
      out.push('receipt_coverage_reusable_count_for: receipt_coverage missing on result');
    } else {
      const row = actual.receipt_coverage.find((r) => r.task_id === want.task);
      if (!row) {
        out.push(`receipt_coverage_reusable_count_for: no row for task=${want.task}`);
      } else if (row.reusable_count !== want.count) {
        out.push(
          `receipt_coverage_reusable_count_for[${want.task}]: expected ` +
          `reusable_count=${want.count}, got ${row.reusable_count}`,
        );
      }
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
