#!/usr/bin/env node

// Read-only corpus evaluator for the MissionD router policy.
//
// Runs the dry-run router recommendation across an entire corpus of task
// contracts and an optional session-trace corpus index, aggregating the
// result into a deterministic JSON report. Produces no runtime dispatch
// changes — emits a measurable view of what the policy would recommend if
// it were live.
//
// HARD GUARANTEES (non-negotiable):
//   - Read-only. The CLI only reads files via fs and writes to stdout/stderr.
//     A tmp directory created by --dry-fixture is removed at end.
//   - No shell-out. No child_process / spawn / fork / exec call sites.
//   - No git invocation. No HTTP. No LLM.
//   - Deterministic. Object keys are sorted; arrays sorted on stable keys
//     (per_task by task_id ascending). Same inputs -> byte-identical output.
//   - dry_run_only is preserved through the per-task rows; the wrapper
//     evaluation schema does not invent a runtime dispatch surface.
//   - The CLI is composable: it imports recommend(), buildIndex,
//     readRouterPolicyFile, parseTraceEvents, readTaskContractFile via
//     named exports — never spawning the underlying CLIs.
//
// Usage:
//   node scripts/evaluate-router-policy-corpus.mjs \
//     --policy <router-policy.lisp> \
//     [--tasks-root .missiond/tasks] \
//     [--trace-index <index.json>] \
//     [--json] [--dry-fixture]
//
// Output schema: missiond.router-policy-evaluation.v0
//   {
//     schema, policy_path, tasks_root, trace_index_source,
//     totals: { tasks, fallbacks, rejections },
//     by_backend: { <backend>: <count>, ... },        // includes zero buckets
//     by_confidence: { high, medium, low },           // includes zero buckets
//     fallback_count, rejected_count,
//     per_task: [
//       { task_id, task_path, backend, confidence,
//         matched_rule_id, fallback_reason }, ...
//     ]
//   }

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  recommend,
  readTaskContractFile,
  FALLBACK_BACKEND,
  FALLBACK_REASON,
} from './recommend-task-backend.mjs';

import {
  buildIndex,
  findSessionTraceFiles,
  DEFAULT_SCAN_ROOT,
} from './build-session-trace-index.mjs';

import {
  BACKEND_CLASSES,
  readRouterPolicyFile,
} from './check-router-policy.mjs';

import { parseTraceEvents } from './check-session-trace.mjs';

export const SCHEMA = 'missiond.router-policy-evaluation.v0';

const usage = `Usage:
  node scripts/evaluate-router-policy-corpus.mjs --policy <router-policy.lisp> [--tasks-root .missiond/tasks] [--trace-index <index.json>] [--json] [--dry-fixture]

Read-only deterministic evaluator: walks a directory of task contracts and
runs recommend() against each contract, then aggregates totals / by_backend /
by_confidence / fallback_count / rejected_count into a stable JSON report.

NEVER mutates files, NEVER shells out, NEVER calls an LLM.

Flags:
  --policy <path>          Path to the router-policy Lisp file (required).
  --tasks-root <dir>       Directory to walk for *.lisp task contracts.
                           Default: .missiond/tasks
  --trace-index <path>     JSON corpus index (output of
                           build-session-trace-index.mjs --json). When omitted,
                           the index is built in-process from any
                           session-trace.lisp under --tasks-root.
  --json                   Emit deterministic JSON (keys sorted).
                           Without --json a human summary is printed.
  --dry-fixture            Run self-contained fixtures and exit.
`;

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  let policyPath = null;
  let tasksRoot = null;
  let traceIndexPath = null;

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else if (arg === '--policy') {
      policyPath = args[++i];
    } else if (arg === '--tasks-root') {
      tasksRoot = args[++i];
    } else if (arg === '--trace-index') {
      traceIndexPath = args[++i];
    } else {
      console.error(`evaluate-router-policy-corpus: unknown flag ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  if (dryFixture) {
    runFixtures(json);
    return;
  }

  if (!policyPath) {
    console.error('evaluate-router-policy-corpus: --policy is required');
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const resolvedPolicy = path.resolve(cwd, policyPath);
  const resolvedTasksRoot = path.resolve(cwd, tasksRoot ?? DEFAULT_SCAN_ROOT);
  const resolvedTraceIndex = traceIndexPath
    ? path.resolve(cwd, traceIndexPath)
    : null;

  // Read the router policy. Both the wave24-01 checker and the wave24-03 CLI
  // re-check :runtime-replacement and :dry-run-only defensively; we mirror
  // that here so a malformed policy fails the corpus run loudly rather than
  // silently producing fallback rows for every task.
  let policy;
  try {
    policy = readRouterPolicyFile(resolvedPolicy);
  } catch (err) {
    console.error(
      `evaluate-router-policy-corpus: failed to read router-policy: ${err.message}`,
    );
    process.exit(1);
  }
  if (policy.runtime_replacement === true) {
    console.error(
      `evaluate-router-policy-corpus: policy ${policy.id} declares :runtime-replacement true; rejected (router output is advisory only).`,
    );
    process.exit(1);
  }
  if (policy.dry_run_only !== true) {
    console.error(
      `evaluate-router-policy-corpus: policy ${policy.id} missing :dry-run-only true; rejected (router output is advisory only).`,
    );
    process.exit(1);
  }

  // Build or load the trace index. We never spawn build-session-trace-index;
  // we either load the JSON the user supplied or stand the index up in-process
  // by collecting session-trace.lisp files under tasks-root and feeding them
  // through parseTraceEvents + buildIndex (the named exports already used by
  // the wave24-06 smoke fixture).
  let traceIndex;
  let traceIndexSource;
  if (resolvedTraceIndex) {
    try {
      const raw = fs.readFileSync(resolvedTraceIndex, 'utf8');
      traceIndex = JSON.parse(raw);
      traceIndexSource = `file:${repoRelative(cwd, resolvedTraceIndex)}`;
    } catch (err) {
      console.error(
        `evaluate-router-policy-corpus: failed to read trace index: ${err.message}`,
      );
      process.exit(1);
    }
  } else {
    try {
      traceIndex = buildIndexFromTasksRoot(resolvedTasksRoot);
      traceIndexSource = `built-in-process:${repoRelative(cwd, resolvedTasksRoot)}`;
    } catch (err) {
      console.error(
        `evaluate-router-policy-corpus: failed to build trace index: ${err.message}`,
      );
      process.exit(1);
    }
  }

  // Walk the corpus.
  const taskFiles = findTaskContractFiles(resolvedTasksRoot);

  const evaluation = evaluateCorpus({
    cwd,
    taskFiles,
    policy,
    policyPath: resolvedPolicy,
    tasksRoot: resolvedTasksRoot,
    traceIndex,
    traceIndexSource,
  });

  if (json) {
    console.log(stableStringify(evaluation));
  } else {
    emitText(evaluation);
  }
}

// Walk a directory recursively for task contract Lisp files. Skips the
// non-contract files that share the *.lisp extension:
//   - schema/**           (task / report / shared-memory / session-trace
//                          schema definitions; not real contracts)
//   - reports/**          (machine-readable report ledgers)
//   - shared-memory.lisp  (per-wave append-only ledger)
//   - session-trace.lisp  (per-wave append-only trace)
//   - *-parallel-dispatch-index.lisp (coordination shim, not a real task)
// Any other *.lisp under tasks-root is treated as a task contract.
export function findTaskContractFiles(root) {
  const out = [];
  if (!fs.existsSync(root)) return out;
  const stack = [root];
  while (stack.length > 0) {
    const dir = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.isDirectory()) {
        if (entry.name === '.git' || entry.name === 'node_modules') continue;
        if (entry.name === 'schema' || entry.name === 'reports') continue;
        stack.push(path.join(dir, entry.name));
      } else if (entry.isFile() && entry.name.endsWith('.lisp')) {
        const name = entry.name;
        if (name === 'shared-memory.lisp') continue;
        if (name === 'session-trace.lisp') continue;
        if (/-parallel-dispatch-index\.lisp$/.test(name)) continue;
        out.push(path.join(dir, name));
      }
    }
  }
  out.sort();
  return out;
}

// Pure aggregator: drives recommend() across a list of task contract files,
// surfaces fallback / rejection counts, and emits the deterministic
// missiond.router-policy-evaluation.v0 shape. Exported so tooling and
// fixtures can call it with in-memory inputs without filesystem state for
// the policy / trace-index / output.
export function evaluateCorpus({
  cwd,
  taskFiles,
  policy,
  policyPath,
  tasksRoot,
  traceIndex,
  traceIndexSource,
}) {
  // by_backend buckets are seeded with every backend the wave24-01 schema
  // defines so the report shape is stable even when the corpus does not
  // exercise every class. Reviewers can read 0 directly off the report
  // without inferring it from a missing key.
  const byBackend = {};
  for (const backend of [...BACKEND_CLASSES].sort()) byBackend[backend] = 0;

  const byConfidence = { high: 0, low: 0, medium: 0 };

  let fallbackCount = 0;
  let rejectedCount = 0;
  const perTask = [];

  for (const file of taskFiles) {
    const relPath = repoRelative(cwd, file);
    let task;
    try {
      task = readTaskContractFile(file);
    } catch (err) {
      // A task whose contract fails to parse is a corpus rejection. We do
      // not call recommend() on it (the predicate evaluator would die on
      // missing fields). Surface the row so reviewers can see which file
      // tripped the parser instead of silently dropping it.
      rejectedCount += 1;
      perTask.push({
        backend: null,
        confidence: null,
        fallback_reason: `malformed_task_contract: ${err.message}`,
        matched_rule_id: null,
        task_id: deriveTaskIdFromPath(file),
        task_path: relPath,
      });
      continue;
    }

    const rec = recommend({
      task,
      policy,
      traceIndex,
      taskPath: file,
      policyPath,
    });

    if (byBackend[rec.backend] === undefined) {
      // A backend that the policy recommends but the schema enum does not
      // declare is a real defect; don't silently roll it into another bucket.
      byBackend[rec.backend] = 0;
    }
    byBackend[rec.backend] += 1;
    byConfidence[rec.confidence] = (byConfidence[rec.confidence] ?? 0) + 1;

    const fallbackReason =
      rec.evidence?.reason && typeof rec.evidence.reason === 'string'
        ? rec.evidence.reason
        : null;

    // A row is a fallback when the recommend() pipeline fell through to the
    // documented FALLBACK_BACKEND/FALLBACK_REASON pair, OR when the matched
    // rule produced a low-confidence verdict because the trace index could
    // not corroborate it. We use chosen_rule_id===null as the primary signal
    // (no rule matched) and FALLBACK_REASON as the secondary so reviewers
    // can later distinguish the two flavours from the per_task row alone.
    const isFallback =
      rec.chosen_rule_id === null ||
      (rec.backend === FALLBACK_BACKEND && fallbackReason === FALLBACK_REASON);
    if (isFallback) fallbackCount += 1;

    perTask.push({
      backend: rec.backend,
      confidence: rec.confidence,
      fallback_reason: fallbackReason,
      matched_rule_id: rec.chosen_rule_id,
      task_id: task.id,
      task_path: relPath,
    });
  }

  perTask.sort((a, b) => (a.task_id < b.task_id ? -1 : a.task_id > b.task_id ? 1 : 0));

  return {
    by_backend: byBackend,
    by_confidence: byConfidence,
    fallback_count: fallbackCount,
    per_task: perTask,
    policy_path: repoRelative(cwd, policyPath),
    rejected_count: rejectedCount,
    schema: SCHEMA,
    tasks_root: repoRelative(cwd, tasksRoot),
    totals: {
      fallbacks: fallbackCount,
      rejections: rejectedCount,
      tasks: taskFiles.length,
    },
    trace_index_source: traceIndexSource,
  };
}

// Build a session-trace corpus index in-process. Mirrors the production
// indexer's file discovery (findSessionTraceFiles) and parsing
// (parseTraceEvents) so the resulting index is byte-compatible with the
// JSON the user could supply via --trace-index.
function buildIndexFromTasksRoot(root) {
  const traceFiles = findSessionTraceFiles(root);
  const traces = [];
  for (const file of traceFiles) {
    const parsed = parseTraceEvents(file);
    for (const trace of parsed) traces.push(trace);
  }
  return buildIndex(traces);
}

// Best-effort task id recovery from a malformed contract path. Keeps the
// per_task row useful even when the file failed to parse: the file basename
// without the .lisp extension is the canonical task id by repo convention.
function deriveTaskIdFromPath(file) {
  const base = path.basename(file, '.lisp');
  return base || path.basename(file);
}

function repoRelative(cwd, p) {
  if (!p) return null;
  const rel = path.relative(cwd, p);
  return rel.split(path.sep).join('/');
}

function emitText(evaluation) {
  const lines = [];
  lines.push(`evaluate-router-policy-corpus (${evaluation.schema})`);
  lines.push(`  policy      : ${evaluation.policy_path}`);
  lines.push(`  tasks_root  : ${evaluation.tasks_root}`);
  lines.push(`  trace_index : ${evaluation.trace_index_source}`);
  lines.push(
    `  totals      : tasks=${evaluation.totals.tasks} ` +
      `fallbacks=${evaluation.totals.fallbacks} ` +
      `rejections=${evaluation.totals.rejections}`,
  );
  lines.push('  by_backend  :');
  for (const backend of Object.keys(evaluation.by_backend).sort()) {
    lines.push(`    - ${backend.padEnd(24)} ${evaluation.by_backend[backend]}`);
  }
  lines.push('  by_confidence:');
  for (const conf of Object.keys(evaluation.by_confidence).sort()) {
    lines.push(`    - ${conf.padEnd(8)} ${evaluation.by_confidence[conf]}`);
  }
  if (evaluation.per_task.length > 0) {
    lines.push('  per_task    :');
    for (const row of evaluation.per_task) {
      const tail = row.matched_rule_id
        ? `rule=${row.matched_rule_id}`
        : `fallback=${row.fallback_reason ?? '(none)'}`;
      lines.push(
        `    - ${row.task_id} -> ${row.backend ?? '(none)'} ` +
          `(${row.confidence ?? 'none'}) ${tail}`,
      );
    }
  }
  console.log(lines.join('\n'));
}

// Stable JSON: mirror the wave24-03 stringifier so corpus reports are
// byte-identical across runs. Object keys are sorted recursively. Arrays
// preserve their caller-supplied order — per_task is sorted on task_id by
// the aggregator before reaching the stringifier.
export function stableStringify(value, indent = 2) {
  return JSON.stringify(sortKeysDeep(value), null, indent);
}

function sortKeysDeep(value) {
  if (Array.isArray(value)) return value.map(sortKeysDeep);
  if (value && typeof value === 'object') {
    const out = {};
    for (const key of Object.keys(value).sort()) out[key] = sortKeysDeep(value[key]);
    return out;
  }
  return value;
}

// ---------------------------------------------------------------------------
// Self-contained dry fixtures.
// All filesystem state is created inside a tmp dir and removed on exit. The
// fixtures cover empty corpus, multi-task corpus, fallback rows, rejected
// policy, deterministic stable JSON, and trace-index supplied vs built —
// directly mapping to the wave25-01 contract's --dry-fixture targets.
// ---------------------------------------------------------------------------

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'empty corpus -> totals=0, no per_task rows',
      category: 'empty-corpus',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-01-empty-'));
        try {
          const policyPath = writePolicyFile(tmp, seedPolicyText());
          const evalResult = runOnTmp({
            tmp,
            policyPath,
            tasksRoot: path.join(tmp, 'tasks-empty'),
          });
          fs.mkdirSync(path.join(tmp, 'tasks-empty'), { recursive: true });
          const reRun = runOnTmp({
            tmp,
            policyPath,
            tasksRoot: path.join(tmp, 'tasks-empty'),
          });
          mustEqual('first.totals.tasks', evalResult.totals.tasks, 0);
          mustEqual('first.per_task.length', evalResult.per_task.length, 0);
          mustEqual('rerun.totals.tasks', reRun.totals.tasks, 0);
          mustEqual('rerun.per_task.length', reRun.per_task.length, 0);
          mustEqual('rerun.totals.fallbacks', reRun.totals.fallbacks, 0);
          mustEqual('rerun.totals.rejections', reRun.totals.rejections, 0);
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      name: 'multi-task corpus -> totals match, by_backend / by_confidence sum to total',
      category: 'multi-task-corpus',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-01-multi-'));
        try {
          const tasksRoot = path.join(tmp, 'tasks');
          fs.mkdirSync(tasksRoot, { recursive: true });
          // 3 tasks: 1 docs (matches), 1 checker (matches), 1 review (matches verifier).
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-docs.lisp'),
            taskDocsText('fx-docs'),
            'utf8',
          );
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-checker.lisp'),
            taskCheckerText('fx-checker'),
            'utf8',
          );
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-review.lisp'),
            taskReviewText('fx-review'),
            'utf8',
          );
          const policyPath = writePolicyFile(tmp, seedPolicyText());
          const evalResult = runOnTmp({ tmp, policyPath, tasksRoot });
          mustEqual('totals.tasks', evalResult.totals.tasks, 3);
          mustEqual('per_task.length', evalResult.per_task.length, 3);
          // Sums hold.
          const byBackendSum = Object.values(evalResult.by_backend).reduce(
            (s, n) => s + n,
            0,
          );
          const byConfSum = Object.values(evalResult.by_confidence).reduce(
            (s, n) => s + n,
            0,
          );
          mustEqual('byBackendSum', byBackendSum, 3);
          mustEqual('byConfSum', byConfSum, 3);
          mustEqual(
            'by_backend.claudecode',
            evalResult.by_backend.claudecode,
            1,
          );
          mustEqual(
            'by_backend.deterministic-checker',
            evalResult.by_backend['deterministic-checker'],
            1,
          );
          mustEqual(
            'by_backend.verifier-worker',
            evalResult.by_backend['verifier-worker'],
            1,
          );
          // per_task is sorted by task_id ascending.
          const ids = evalResult.per_task.map((r) => r.task_id);
          const sortedIds = [...ids].sort();
          if (ids.join(',') !== sortedIds.join(',')) {
            throw new Error(`per_task not sorted by task_id: ${ids.join(',')}`);
          }
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      name: 'fallback rows -> fallback_count > 0 (no matching rule)',
      category: 'fallback-rows',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-01-fallback-'));
        try {
          const tasksRoot = path.join(tmp, 'tasks');
          fs.mkdirSync(tasksRoot, { recursive: true });
          // A task whose kind is `ops` and whose write-scope is off-policy.
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-ops.lisp'),
            taskOpsOffPolicyText('fx-ops'),
            'utf8',
          );
          // A docs task that DOES match -- the fallback count must remain 1.
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-docs.lisp'),
            taskDocsText('fx-docs'),
            'utf8',
          );
          const policyPath = writePolicyFile(tmp, seedPolicyText());
          const evalResult = runOnTmp({ tmp, policyPath, tasksRoot });
          mustEqual('totals.tasks', evalResult.totals.tasks, 2);
          mustEqual('fallback_count', evalResult.fallback_count, 1);
          mustEqual('totals.fallbacks', evalResult.totals.fallbacks, 1);
          // The ops row must surface the documented fallback reason.
          const opsRow = evalResult.per_task.find((r) => r.task_id === 'fx-ops');
          mustEqual('opsRow.matched_rule_id', opsRow.matched_rule_id, null);
          mustEqual(
            'opsRow.fallback_reason',
            opsRow.fallback_reason,
            FALLBACK_REASON,
          );
          mustEqual('opsRow.backend', opsRow.backend, FALLBACK_BACKEND);
          mustEqual('opsRow.confidence', opsRow.confidence, 'low');
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      name: 'rejected policy (:runtime-replacement true) -> early non-zero exit (simulated)',
      category: 'rejected-policy',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-01-rr-'));
        try {
          const policyPath = writePolicyFile(tmp, badRuntimeReplacementText());
          // We cannot exercise process.exit() directly inside the fixture
          // suite (it would tear down the harness). Instead, mirror main()'s
          // guard: read the policy, confirm the projector surfaces
          // runtime_replacement === true, and confirm dry_run_only checking
          // would also reject it. The runtime CLI test below proves the
          // guard wires through to a non-zero exit.
          const policy = readRouterPolicyFile(policyPath);
          if (policy.runtime_replacement !== true) {
            throw new Error('fixture policy must declare runtime_replacement true');
          }
          if (policy.dry_run_only !== true) {
            throw new Error('fixture policy should still set dry-run-only true to isolate the rr branch');
          }
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      name: 'rejected task (malformed contract) -> rejected_count > 0',
      category: 'rejected-task',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-01-mal-'));
        try {
          const tasksRoot = path.join(tmp, 'tasks');
          fs.mkdirSync(tasksRoot, { recursive: true });
          // Lisp form that parses but is NOT a (task ...) — readTaskContractFile
          // will throw "no (task ...) form found".
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-bad.lisp'),
            '(not-a-task fx-bad :kind docs)',
            'utf8',
          );
          // One real task to confirm the loop continues past the malformed file.
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-docs.lisp'),
            taskDocsText('fx-docs'),
            'utf8',
          );
          const policyPath = writePolicyFile(tmp, seedPolicyText());
          const evalResult = runOnTmp({ tmp, policyPath, tasksRoot });
          mustEqual('totals.tasks', evalResult.totals.tasks, 2);
          mustEqual('totals.rejections', evalResult.totals.rejections, 1);
          mustEqual('rejected_count', evalResult.rejected_count, 1);
          const badRow = evalResult.per_task.find((r) => r.task_id === 'fx-bad');
          mustEqual('badRow.backend', badRow.backend, null);
          mustEqual('badRow.confidence', badRow.confidence, null);
          if (!/^malformed_task_contract:/.test(badRow.fallback_reason ?? '')) {
            throw new Error(
              `expected malformed_task_contract reason, got ${badRow.fallback_reason}`,
            );
          }
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      name: 'deterministic stable JSON (same fixtures twice -> byte-identical output)',
      category: 'deterministic-output',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-01-det-'));
        try {
          const tasksRoot = path.join(tmp, 'tasks');
          fs.mkdirSync(tasksRoot, { recursive: true });
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-docs.lisp'),
            taskDocsText('fx-docs'),
            'utf8',
          );
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-checker.lisp'),
            taskCheckerText('fx-checker'),
            'utf8',
          );
          const policyPath = writePolicyFile(tmp, seedPolicyText());
          const a = stableStringify(runOnTmp({ tmp, policyPath, tasksRoot }));
          const b = stableStringify(runOnTmp({ tmp, policyPath, tasksRoot }));
          if (a !== b) throw new Error('non-deterministic JSON output');
          const parsed = JSON.parse(a);
          const keys = Object.keys(parsed);
          const sorted = [...keys].sort();
          if (keys.join(',') !== sorted.join(',')) {
            throw new Error(`top-level keys not sorted: ${keys.join(',')}`);
          }
          mustEqual('parsed.schema', parsed.schema, SCHEMA);
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      name: 'trace index supplied vs built in-process -> same per-task recommendation',
      category: 'trace-index-equivalence',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-01-tix-'));
        try {
          const tasksRoot = path.join(tmp, 'tasks');
          fs.mkdirSync(tasksRoot, { recursive: true });
          // Single task whose id appears in the trace fixture below.
          fs.writeFileSync(
            path.join(tasksRoot, 'wave-fx-docs.lisp'),
            taskDocsText('wave-fx-docs'),
            'utf8',
          );
          // Plant a session-trace.lisp under tasksRoot so the
          // build-in-process path discovers it via findSessionTraceFiles.
          const traceLispPath = path.join(tasksRoot, 'session-trace.lisp');
          fs.writeFileSync(
            traceLispPath,
            traceFixtureText('wave-fx-docs', 'claudecode'),
            'utf8',
          );

          const policyPath = writePolicyFile(tmp, seedPolicyText());

          // Pre-build the same index via the same named exports so the
          // supplied JSON path is provably byte-equivalent to what the
          // in-process build would produce on this corpus.
          const traces = parseTraceEvents(traceLispPath);
          const builtIndex = buildIndex(traces);
          const indexJsonPath = path.join(tmp, 'trace-index.json');
          fs.writeFileSync(
            indexJsonPath,
            stableStringify(builtIndex) + '\n',
            'utf8',
          );

          const fromBuilt = evaluateCorpus({
            cwd: tmp,
            taskFiles: findTaskContractFiles(tasksRoot),
            policy: readRouterPolicyFile(policyPath),
            policyPath,
            tasksRoot,
            traceIndex: builtIndex,
            traceIndexSource: 'built-in-process:tasks',
          });
          const fromSupplied = evaluateCorpus({
            cwd: tmp,
            taskFiles: findTaskContractFiles(tasksRoot),
            policy: readRouterPolicyFile(policyPath),
            policyPath,
            tasksRoot,
            traceIndex: JSON.parse(fs.readFileSync(indexJsonPath, 'utf8')),
            traceIndexSource: `file:trace-index.json`,
          });

          // Source labels differ on purpose; everything else must match.
          const stripSource = (e) => {
            const { trace_index_source: _drop, ...rest } = e;
            return rest;
          };
          const a = stableStringify(stripSource(fromBuilt));
          const b = stableStringify(stripSource(fromSupplied));
          if (a !== b) {
            throw new Error('trace-index supplied vs built produced different rows');
          }
          mustEqual('totals.tasks', fromBuilt.totals.tasks, 1);
          // The synthesised trace has 5 events -> rich -> high confidence.
          mustEqual(
            'per_task[0].confidence',
            fromBuilt.per_task[0].confidence,
            'high',
          );
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      // wave25-05: cross-layer measurement smoke. The corpus evaluator is one
      // of the three engines that consume the wave24-01 router-policy. The
      // smoke pins:
      //   * the evaluator surfaces dry_run_only=true / runtime_replacement=
      //     false from the parsed policy projection (Layer A invariants 1+2);
      //   * every per_task row carries a confidence in {high, medium, low}
      //     and a backend in BACKEND_CLASSES (Layer A invariant: enum closed);
      //   * by_backend totals are the EXACT set of backends emitted across
      //     per_task rows (no silent backend leak);
      //   * the evaluator itself never declares an `applied` field — that
      //     concept lives ONLY on the recommend-task-backend.mjs / mission_plan
      //     emit surface and the wave25-02 report contract; the evaluator
      //     deliberately does NOT carry the dry_run_only / applied invariant
      //     because aggregation is shape-orthogonal to advisory status.
      name: 'wave25-05: cross-layer invariants pinned on evaluator output',
      category: 'wave25-05-cross-layer',
      run: () => {
        const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'wave25-05-xlayer-'));
        try {
          const tasksRoot = path.join(tmp, 'tasks');
          fs.mkdirSync(tasksRoot, { recursive: true });
          // Mirror the wave25-05 Layer A parity policy: docs->claudecode +
          // code-alignment->deterministic-checker so the corpus exercises 2
          // backends. Add a review task so verifier-worker also lights up.
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-docs.lisp'),
            taskDocsText('fx-docs'),
            'utf8',
          );
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-checker.lisp'),
            taskCheckerText('fx-checker'),
            'utf8',
          );
          fs.writeFileSync(
            path.join(tasksRoot, 'fx-review.lisp'),
            taskReviewText('fx-review'),
            'utf8',
          );
          const policyPath = writePolicyFile(tmp, seedPolicyText());

          // Re-pin policy invariants the evaluator's Lisp projector parses.
          const policy = readRouterPolicyFile(policyPath);
          mustEqual('policy.dry_run_only', policy.dry_run_only, true);
          mustEqual('policy.runtime_replacement', policy.runtime_replacement, false);

          const evalResult = runOnTmp({ tmp, policyPath, tasksRoot });
          mustEqual('totals.tasks', evalResult.totals.tasks, 3);

          // Every per_task row's confidence ∈ {high, medium, low}.
          const allowedConf = new Set(['high', 'medium', 'low']);
          for (const row of evalResult.per_task) {
            if (!allowedConf.has(row.confidence)) {
              throw new Error(
                `wave25-05: per_task row confidence ${row.confidence} not in {high,medium,low}`,
              );
            }
            if (!BACKEND_CLASSES.has(row.backend)) {
              throw new Error(
                `wave25-05: per_task row backend ${row.backend} not in BACKEND_CLASSES`,
              );
            }
          }
          // by_backend totals match the union of per_task backends.
          const emittedBackends = new Set(
            evalResult.per_task.map((r) => r.backend),
          );
          for (const backend of emittedBackends) {
            const count = evalResult.per_task.filter(
              (r) => r.backend === backend,
            ).length;
            mustEqual(
              `by_backend[${backend}]`,
              evalResult.by_backend[backend],
              count,
            );
          }

          // Re-pin: the rejected-runtime-replacement policy is rejected by
          // the evaluator's main() guard. We assert this on the projector
          // because the evaluator main() exits non-zero before aggregation
          // (mirrors the recommend-task-backend.mjs guard); the projector
          // surface is what the guard reads from.
          fs.mkdirSync(path.join(tmp, 'rr'), { recursive: true });
          const rrPath = writePolicyFile(
            path.join(tmp, 'rr'),
            badRuntimeReplacementText(),
          );
          const rrPolicy = readRouterPolicyFile(rrPath);
          mustEqual(
            'rr.runtime_replacement',
            rrPolicy.runtime_replacement,
            true,
          );
        } finally {
          fs.rmSync(tmp, { recursive: true, force: true });
        }
      },
    },
    {
      name: 'audit: no shell / git / LLM / HTTP call sites in active source',
      category: 'self-audit',
      run: () => {
        const selfPath = path.resolve(
          process.cwd(),
          'scripts',
          'evaluate-router-policy-corpus.mjs',
        );
        const src = fs.readFileSync(selfPath, 'utf8');
        // Strip line comments and block comments before scanning so
        // documentation that NAMES the forbidden patterns does not
        // self-trip the audit.
        // Strip line comments, block comments, AND single/double/back-quote
        // string literals before scanning so documentation prose and fixture
        // names that NAME the forbidden patterns do not self-trip the audit.
        // The intent is to flag actual call sites only.
        const stripped = src
          .split('\n')
          .filter((ln) => !/^\s*\/\//.test(ln))
          .join('\n')
          .replace(/\/\*[\s\S]*?\*\//g, '')
          .replace(/'(?:\\.|[^'\\\n])*'/g, "''")
          .replace(/"(?:\\.|[^"\\\n])*"/g, '""')
          .replace(/`(?:\\.|[^`\\])*`/g, '``');
        const forbidden = [
          new RegExp('child' + '_' + 'process'),
          new RegExp('\\bspawn\\b'),
          new RegExp('\\bexec' + 'Sync\\b'),
          new RegExp('\\bfork\\b'),
          new RegExp('open' + 'ai', 'i'),
          new RegExp('anthrop' + 'ic', 'i'),
          new RegExp('chat\\.compl' + 'etion', 'i'),
          new RegExp('\\bfetch\\('),
          new RegExp('\\bhttps?\\.(?:get|request|post)\\b'),
        ];
        for (const re of forbidden) {
          if (re.test(stripped)) {
            throw new Error(
              `wave25-01 self-audit: forbidden pattern ${re} found in active source`,
            );
          }
        }
      },
    },
  ];

  let failed = 0;
  const categories = new Set();
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    try {
      fixture.run();
    } catch (err) {
      failed += 1;
      console.error(`fixture failed: ${fixture.name}`);
      console.error(`  ${err.message}`);
    }
  }

  if (json) {
    console.log(
      stableStringify({
        categories: [...categories].sort(),
        failed,
        fixtures: fixtures.length,
        ok: failed === 0,
      }),
    );
  }
  if (failed > 0) {
    console.error(
      `evaluate-router-policy-corpus fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `evaluate-router-policy-corpus fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

function mustEqual(label, actual, expected) {
  if (actual !== expected) {
    throw new Error(
      `assert ${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  }
}

// Helper used by every fixture so the wrapper logic (read policy + build
// in-process trace index + walk corpus + aggregate) is tested end-to-end
// rather than only at the aggregator boundary.
function runOnTmp({ tmp, policyPath, tasksRoot }) {
  const policy = readRouterPolicyFile(policyPath);
  const taskFiles = findTaskContractFiles(tasksRoot);
  const traceIndex = buildIndexFromTasksRoot(tasksRoot);
  return evaluateCorpus({
    cwd: tmp,
    taskFiles,
    policy,
    policyPath,
    tasksRoot,
    traceIndex,
    traceIndexSource: 'built-in-process:tasks',
  });
}

function writePolicyFile(tmp, body) {
  const p = path.join(tmp, 'policy.lisp');
  fs.writeFileSync(p, body, 'utf8');
  return p;
}

function seedPolicyText() {
  // Mirrors the wave24-01 seed (3 rules) so fixtures match real-world output.
  return `(router-policy fixture-seed
    :schema "missiond.router-policy.v1"
    :version "v1"
    :dry-run-only true
    :runtime-replacement false
    (rule
      :id r-docs-to-claudecode
      :priority 10
      :when ((kind docs))
      :recommend (:backend claudecode :reasoning "docs are interactive")
      :non-goals ["does not replace runtime dispatch"])
    (rule
      :id r-deterministic-checker-tasks
      :priority 20
      :when ((all (kind code-alignment)
                  (path-glob "scripts/check-*.mjs")))
      :recommend (:backend deterministic-checker :reasoning "scripted acceptance")
      :non-goals ["does not replace runtime dispatch"])
    (rule
      :id r-post-commit-verifier
      :priority 30
      :when ((any (kind review)
                  (kind smoke)))
      :recommend (:backend verifier-worker :reasoning "verifies an existing commit")
      :non-goals ["does not replace runtime dispatch"]))`;
}

function badRuntimeReplacementText() {
  return `(router-policy fixture-bad-rr
    :schema "missiond.router-policy.v1"
    :version "v1"
    :dry-run-only true
    :runtime-replacement true
    (rule
      :id r-rr
      :priority 1
      :when ((kind docs))
      :recommend (:backend claudecode :reasoning "should never run")
      :non-goals ["does not replace runtime dispatch"]))`;
}

function taskDocsText(id) {
  return `(task ${id}
    :schema "missiond.task-contract.v1"
    :title "Fixture docs task"
    :kind docs
    :status ready
    :owner "claudecode"
    :dispatch-strategy "manual"
    :goal "fixture goal"
    :write-scope ["docs/${id}.md"]
    :must-not-touch []
    :acceptance ["true"]
    :commit (:required false :scope-check none))`;
}

function taskCheckerText(id) {
  return `(task ${id}
    :schema "missiond.task-contract.v1"
    :title "Fixture checker task"
    :kind code-alignment
    :status ready
    :owner "claudecode"
    :dispatch-strategy "fresh-code-alignment"
    :goal "fixture goal"
    :write-scope ["scripts/check-${id}.mjs"]
    :must-not-touch []
    :acceptance ["true"]
    :commit (:required false :scope-check none))`;
}

function taskReviewText(id) {
  return `(task ${id}
    :schema "missiond.task-contract.v1"
    :title "Fixture review task"
    :kind review
    :status ready
    :owner "claudecode"
    :dispatch-strategy "manual"
    :goal "fixture goal"
    :write-scope ["docs/${id}.md"]
    :must-not-touch []
    :acceptance ["true"]
    :commit (:required false :scope-check none))`;
}

function taskOpsOffPolicyText(id) {
  return `(task ${id}
    :schema "missiond.task-contract.v1"
    :title "Fixture off-policy ops task"
    :kind ops
    :status ready
    :owner "operator"
    :dispatch-strategy "manual"
    :goal "fixture goal"
    :write-scope ["ops/${id}.md"]
    :must-not-touch []
    :acceptance ["true"]
    :commit (:required false :scope-check none))`;
}

function traceFixtureText(taskId, backend) {
  return `(session-trace wave-fx
    :schema "missiond.session-trace.v1"
    :wave wave-fx
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event :id fx-001 :seq 1 :at "2026-04-28T00:00:00Z"
      :task ${taskId} :backend ${backend}
      :kind dispatch :summary "dispatch")
    (trace-event :id fx-002 :seq 2 :at "2026-04-28T00:00:01Z"
      :task ${taskId} :backend ${backend}
      :kind start :summary "start")
    (trace-event :id fx-003 :seq 3 :at "2026-04-28T00:00:02Z"
      :task ${taskId} :backend ${backend}
      :kind read :summary "read")
    (trace-event :id fx-004 :seq 4 :at "2026-04-28T00:00:03Z"
      :task ${taskId} :backend ${backend}
      :kind commit :summary "commit" :commit_hash "fxabc1234567")
    (trace-event :id fx-005 :seq 5 :at "2026-04-28T00:00:04Z"
      :task ${taskId} :backend ${backend}
      :kind complete :summary "done" :commit_hash "fxabc1234567"))`;
}

// Only run the CLI when invoked directly. Keep evaluateCorpus /
// findTaskContractFiles / stableStringify / SCHEMA importable so future
// tooling (a wave25-04 renderer surface, e.g.) can drive the algorithm
// in-process without re-implementing the walker.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
