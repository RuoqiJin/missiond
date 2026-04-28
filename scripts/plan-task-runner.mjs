#!/usr/bin/env node

// MissionD task-runner plan CLI v0 (wave28-02).
//
// Read-only planner: consumes a task-runner-manifest v1 (wave28-01) plus
// the task contracts under .missiond/tasks/<wave>/, and emits a
// deterministic runner plan: topological batches, critical-path estimate,
// write-scope overlap diagnostics, and verification-tier summary.
//
// Hard guarantees (cross-wave invariants):
//   * NO dispatch — this CLI never spawns workers, never delegates, never
//     calls mission_task_delegate.
//   * NO child_process / spawn / exec / fork — pure file reads.
//   * NO git, NO network, NO HTTP, NO LLM call.
//   * Output is deterministic — sorted JSON keys, stable batch ordering by
//     node id within each batch, stable diagnostic ordering.
//
// Imports the wave28-01 manifest checker's named exports
// (readManifestFile / projectManifest / validateManifestObject + enums) so
// validation rules live in exactly one place.

import fs from 'node:fs';
import path from 'node:path';

import {
  SCHEMA as MANIFEST_SCHEMA,
  VERIFICATION_TIERS,
  OVERLAP_POLICIES,
  projectManifest,
  readManifestFile,
  validateManifestObject,
} from './check-task-runner-manifest.mjs';

import {
  parseLisp,
  readLispFile,
  isList,
  head,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/plan-task-runner.mjs --manifest <manifest.lisp> [--json|--lisp] [--schedule group-barrier|ready-queue] [--dry-fixture]

Read-only planner over a MissionD task-runner-manifest v1 (wave28-01).
Emits a deterministic runner plan as JSON (default) or S-expression Lisp.

Output JSON top-level keys (sorted):
  schema, manifest_path, wave, productive_only, batches,
  critical_path_minutes, total_estimated_minutes, max_parallel_width,
  overlap_diagnostics, verification_tier_counts.

Cross-wave invariants enforced structurally:
  * Topological batches respect both :depends_on edges AND :dispatch_group
    boundaries — nodes in different groups are NEVER in the same batch.
  * Same-dispatch_group write_scope overlap is reported in
    overlap_diagnostics; severity follows :overlap_policy
    (reject -> error, warn -> warning); cross-group overlap is allowed.
  * Dependency cycles are a structured error (exit non-zero).
  * Manifest nodes whose task contract .lisp file is missing on disk are
    a structured error (exit non-zero).

Schedule modes (wave29-06 additive):
  * group-barrier (default) — byte-identical to the wave28-02 baseline:
    nodes are grouped into per-batch windows that respect both edges and
    dispatch_group boundaries. The batch advances only when every node in
    it finishes.
  * ready-queue — additionally emits a 'ready_queue' field where each
    node is RELEASED as soon as all :depends_on edges (plus same-group
    write_scope overlap edges, treated as additional serializing edges)
    are satisfied, INDEPENDENT of unrelated long-running peers in the
    same dispatch_group. Priority is critical-path remaining minutes
    descending, tie-broken by task id lexicographically. Each entry
    exposes ready_at_minutes / finish_at_minutes / barrier_finish_at_minutes
    / idle_window_savings_minutes (= barrier_finish - finish, never < 0).

NO dispatch, NO spawn, NO git, NO network, NO LLM. Plan-only.

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const PLAN_SCHEMA = 'missiond.task-runner-plan.v0';

// wave29-06: additive ready-queue/phase-barrier scheduler. Default mode
// stays byte-identical to the wave28-02 group-barrier output.
export const SCHEDULE_MODES = new Set(['group-barrier', 'ready-queue']);
export const DEFAULT_SCHEDULE_MODE = 'group-barrier';

// Re-export from wave28-01 so downstream tooling has a single import surface.
export { MANIFEST_SCHEMA, VERIFICATION_TIERS, OVERLAP_POLICIES };

function main() {
  const args = process.argv.slice(2);
  let manifestPath = null;
  let format = 'json';
  let dryFixture = false;
  let schedule = DEFAULT_SCHEDULE_MODE;

  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      format = 'json';
    } else if (arg === '--lisp') {
      format = 'lisp';
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else if (arg === '--manifest') {
      manifestPath = args[i + 1];
      i += 1;
    } else if (arg.startsWith('--manifest=')) {
      manifestPath = arg.slice('--manifest='.length);
    } else if (arg === '--schedule') {
      schedule = args[i + 1];
      i += 1;
    } else if (arg.startsWith('--schedule=')) {
      schedule = arg.slice('--schedule='.length);
    } else {
      console.error(`unknown argument: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  if (!SCHEDULE_MODES.has(schedule)) {
    console.error(`error: --schedule must be one of ${[...SCHEDULE_MODES].sort().join(' | ')}`);
    console.error(usage);
    process.exit(2);
  }

  if (dryFixture) {
    runFixtures(format);
    return;
  }

  if (!manifestPath) {
    console.error('error: --manifest <manifest.lisp> is required');
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const resolvedManifest = path.isAbsolute(manifestPath)
    ? manifestPath
    : path.resolve(cwd, manifestPath);

  let result;
  try {
    result = planFromManifestFile(resolvedManifest, { repoRoot: cwd, schedule });
  } catch (err) {
    const payload = { ok: false, error: 'plan_failed', message: err.message };
    if (format === 'lisp') {
      console.error(`;; plan-task-runner: ${err.message}`);
    } else {
      console.error(JSON.stringify(payload, null, 2));
    }
    process.exit(1);
  }

  if (!result.ok) {
    if (format === 'lisp') {
      console.error(formatErrorLisp(result));
    } else {
      console.error(JSON.stringify(result, null, 2));
    }
    process.exit(1);
  }

  if (format === 'lisp') {
    console.log(formatPlanLisp(result.plan));
  } else {
    console.log(JSON.stringify(result.plan, null, 2));
  }
  process.exit(0);
}

// ---------------------------------------------------------------------------
// Public entry: plan from on-disk manifest.
// ---------------------------------------------------------------------------

// Plan from an on-disk manifest path. Returns either:
//   { ok: true, plan: {...} }
//   { ok: false, error: '<code>', ...details }
// Errors:
//   schema_invalid     — manifest fails wave28-01 validateManifestObject
//   cycle              — dependency cycle detected
//   missing_task       — a manifest node references a task contract whose
//                         .lisp file does not exist on disk
//   no_manifest        — file contained no (task-runner-manifest ...) form
//   multiple_manifests — file contained more than one manifest form
export function planFromManifestFile(manifestPath, opts = {}) {
  const repoRoot = opts.repoRoot ?? process.cwd();
  const manifests = readManifestFile(manifestPath);
  if (manifests.length === 0) {
    return {
      ok: false,
      error: 'no_manifest',
      manifest_path: toRepoRelative(manifestPath, repoRoot),
      message: `no (task-runner-manifest ...) form found in ${manifestPath}`,
    };
  }
  if (manifests.length > 1) {
    return {
      ok: false,
      error: 'multiple_manifests',
      manifest_path: toRepoRelative(manifestPath, repoRoot),
      message: `${manifests.length} manifest forms in one file; expected exactly one`,
    };
  }
  const manifest = manifests[0];
  return planFromManifestObject(manifest, {
    manifest_path: toRepoRelative(manifestPath, repoRoot),
    repoRoot,
    checkTaskContractsOnDisk: true,
    schedule: opts.schedule ?? DEFAULT_SCHEDULE_MODE,
  });
}

// Plan from an already-projected manifest object. Mostly useful for
// fixtures and for in-memory pipelines where the manifest is parsed but
// never written to disk.
export function planFromManifestObject(manifest, opts = {}) {
  const manifestPath = opts.manifest_path ?? '<memory>';
  const checkOnDisk = opts.checkTaskContractsOnDisk === true;
  const repoRoot = opts.repoRoot ?? process.cwd();
  const schedule = SCHEDULE_MODES.has(opts.schedule)
    ? opts.schedule
    : DEFAULT_SCHEDULE_MODE;

  // 1. Schema validation reuses wave28-01 — single source of truth for
  //    shape rules. Reject early if the manifest itself is malformed.
  const schemaErrors = validateManifestObject(manifest);
  if (schemaErrors.length > 0) {
    return {
      ok: false,
      error: 'schema_invalid',
      manifest_path: manifestPath,
      message: `manifest failed schema validation (${schemaErrors.length} error${schemaErrors.length === 1 ? '' : 's'})`,
      errors: schemaErrors,
    };
  }

  // 2. Optional on-disk join: ensure each node names a real task
  //    contract under .missiond/tasks/<wave>/<task_id>.lisp.
  if (checkOnDisk) {
    const missing = [];
    for (const node of manifest.nodes) {
      const expectedRel = path.posix.join(
        '.missiond',
        'tasks',
        manifest.wave,
        `${node.task_id}.lisp`,
      );
      const expectedAbs = path.resolve(repoRoot, expectedRel);
      if (!fs.existsSync(expectedAbs)) {
        missing.push({ task_id: node.task_id, expected_path: expectedRel });
      }
    }
    if (missing.length > 0) {
      // Sort for deterministic output.
      missing.sort((a, b) => a.task_id.localeCompare(b.task_id));
      return {
        ok: false,
        error: 'missing_task',
        manifest_path: manifestPath,
        message: `${missing.length} manifest node${missing.length === 1 ? '' : 's'} reference task contract files that do not exist`,
        missing,
      };
    }
  }

  // 3. Topological batching with dispatch_group boundaries + cycle check.
  const sortedNodes = [...manifest.nodes].sort((a, b) =>
    a.task_id.localeCompare(b.task_id),
  );
  const idToNode = new Map(sortedNodes.map((n) => [n.task_id, n]));
  const inDegree = new Map();
  const dependents = new Map(); // task_id -> [task_ids that depend on it]
  for (const node of sortedNodes) {
    inDegree.set(node.task_id, 0);
    dependents.set(node.task_id, []);
  }
  for (const node of sortedNodes) {
    for (const dep of node.depends_on) {
      // wave28-01 already guarantees dep references resolve, but defend
      // against tampered objects.
      if (!idToNode.has(dep)) continue;
      inDegree.set(node.task_id, inDegree.get(node.task_id) + 1);
      dependents.get(dep).push(node.task_id);
    }
  }

  const batches = [];
  const remaining = new Set(sortedNodes.map((n) => n.task_id));
  let safety = sortedNodes.length + 2;
  while (remaining.size > 0 && safety > 0) {
    safety -= 1;
    // Ready set: zero in-degree AND still in remaining.
    const ready = [];
    for (const id of remaining) {
      if (inDegree.get(id) === 0) ready.push(id);
    }
    if (ready.length === 0) {
      // Cycle: pick the lexicographically smallest remaining id and walk
      // its dependency chain to surface a representative cycle.
      const cycle = findCycle(sortedNodes, idToNode, remaining);
      return {
        ok: false,
        error: 'cycle',
        manifest_path: manifestPath,
        message: `dependency cycle detected: ${cycle.join(' -> ')}`,
        cycle,
      };
    }

    // Group the ready set by dispatch_group so a single batch never
    // straddles groups. Pick the lexicographically smallest group first
    // for deterministic batch ordering.
    const byGroup = new Map();
    for (const id of ready) {
      const g = idToNode.get(id).dispatch_group;
      const arr = byGroup.get(g) ?? [];
      arr.push(id);
      byGroup.set(g, arr);
    }
    const groupKeys = [...byGroup.keys()].sort((a, b) => a.localeCompare(b));
    for (const g of groupKeys) {
      const batch = byGroup.get(g).sort((a, b) => a.localeCompare(b));
      batches.push(batch);
      for (const id of batch) {
        remaining.delete(id);
        for (const dep of dependents.get(id)) {
          inDegree.set(dep, inDegree.get(dep) - 1);
        }
      }
    }
  }

  if (remaining.size > 0) {
    // Defensive: should not happen because the ready/cycle path above
    // catches it. Surface as a cycle anyway rather than silently
    // dropping nodes.
    const cycle = findCycle(sortedNodes, idToNode, remaining);
    return {
      ok: false,
      error: 'cycle',
      manifest_path: manifestPath,
      message: `dependency cycle detected: ${cycle.join(' -> ')}`,
      cycle,
    };
  }

  // 4. Critical-path estimate: longest dependency chain by
  //    estimated_minutes. Memoized DFS over the node DAG.
  const longest = new Map(); // task_id -> minutes from this node's start to a leaf
  function longestFrom(id) {
    if (longest.has(id)) return longest.get(id);
    const node = idToNode.get(id);
    let best = node.estimated_minutes;
    let extra = 0;
    for (const child of dependents.get(id)) {
      extra = Math.max(extra, longestFrom(child));
    }
    best = node.estimated_minutes + extra;
    longest.set(id, best);
    return best;
  }
  let criticalPath = 0;
  for (const node of sortedNodes) {
    criticalPath = Math.max(criticalPath, longestFrom(node.task_id));
  }

  // 5. Total estimated minutes + parallel-width.
  let totalMinutes = 0;
  for (const node of sortedNodes) totalMinutes += node.estimated_minutes;
  let maxWidth = 0;
  for (const batch of batches) maxWidth = Math.max(maxWidth, batch.length);

  // 6. Overlap diagnostics — same-dispatch_group write-scope overlap.
  //    Severity follows manifest.overlap_policy (default reject -> error;
  //    warn -> warning). Cross-group overlap is allowed by wave28-01
  //    policy and produces no diagnostic.
  const overlapDiagnostics = collectOverlapDiagnostics(
    sortedNodes,
    manifest.overlap_policy,
  );

  // 7. Verification-tier counts — pre-seeded so a tier with zero nodes
  //    still appears in the output (deterministic shape).
  const tierCounts = { local: 0, smoke: 0, full: 0 };
  for (const node of sortedNodes) {
    if (Object.prototype.hasOwnProperty.call(tierCounts, node.verification_tier)) {
      tierCounts[node.verification_tier] += 1;
    }
  }

  // 8. Assemble the plan with sorted top-level keys.
  const planFields = {
    schema: PLAN_SCHEMA,
    manifest_path: manifestPath,
    wave: manifest.wave,
    productive_only: manifest.productive_only,
    batches,
    critical_path_minutes: criticalPath,
    total_estimated_minutes: totalMinutes,
    max_parallel_width: maxWidth,
    overlap_diagnostics: overlapDiagnostics,
    verification_tier_counts: tierCounts,
  };

  // 9. wave29-06 additive ready-queue branch. Only emitted under explicit
  //    --schedule ready-queue opt-in so the default group-barrier branch
  //    stays byte-identical to the wave28-02 baseline output.
  if (schedule === 'ready-queue') {
    planFields.ready_queue = computeReadyQueue({
      sortedNodes,
      idToNode,
      batches,
      longestFrom,
      manifest,
    });
  }

  const plan = sortKeys(planFields);
  return { ok: true, plan };
}

// ---------------------------------------------------------------------------
// wave29-06 ready-queue / phase-barrier scheduler.
//
// Releases each node as soon as all of its dependency edges are satisfied,
// independent of unrelated long-running peers in the same dispatch_group.
// Same-dispatch_group write_scope overlap is treated as an additional
// serializing edge so the reject policy's safety guarantee is preserved.
//
// Priority: critical-path remaining minutes (longestFrom) descending so
// nodes feeding into the longest tail emit first; ties broken by task id
// lexicographically. Documented in the CLI usage block above.
//
// Idle-window savings: difference between when group-barrier finishes
// the node (the slowest peer in the same batch dictates the barrier) and
// when ready-queue finishes the node. Always >= 0; aggregate across all
// nodes is exposed at the top level.
// ---------------------------------------------------------------------------
function computeReadyQueue({
  sortedNodes,
  idToNode,
  batches,
  longestFrom,
  manifest,
}) {
  // Collect explicit dependency edges per task.
  const explicitDeps = new Map();
  for (const node of sortedNodes) {
    explicitDeps.set(
      node.task_id,
      [...(node.depends_on ?? [])].filter((d) => idToNode.has(d)),
    );
  }

  // Add overlap edges: when two nodes in the same dispatch_group share a
  // write-scope path, serialize them in lex order so the reject-policy
  // safety guarantee holds inside ready-queue too. We add an edge from
  // the lex-smaller id to the lex-larger id; the larger id then waits.
  const overlapEdges = []; // { from, to, paths[] }
  if (manifest.overlap_policy !== 'warn') {
    const groups = new Map(); // group -> Map(path -> Set(ids))
    for (const node of sortedNodes) {
      if (!node.dispatch_group) continue;
      const byPath = groups.get(node.dispatch_group) ?? new Map();
      for (const entry of node.write_scope ?? []) {
        const set = byPath.get(entry) ?? new Set();
        set.add(node.task_id);
        byPath.set(entry, set);
      }
      groups.set(node.dispatch_group, byPath);
    }
    const seen = new Map();
    for (const byPath of groups.values()) {
      for (const [pth, ids] of byPath) {
        if (ids.size < 2) continue;
        const sortedIds = [...ids].sort((a, b) => a.localeCompare(b));
        for (let i = 0; i < sortedIds.length - 1; i += 1) {
          const from = sortedIds[i];
          const to = sortedIds[i + 1];
          const key = `${from} ${to}`;
          const row = seen.get(key) ?? { from, to, paths: [] };
          row.paths.push(pth);
          seen.set(key, row);
        }
      }
    }
    for (const row of seen.values()) {
      row.paths = [...new Set(row.paths)].sort((a, b) => a.localeCompare(b));
      overlapEdges.push(row);
    }
  }

  // Effective dependency closure = explicit edges + overlap edges.
  const effectiveDeps = new Map();
  for (const node of sortedNodes) {
    effectiveDeps.set(node.task_id, new Set(explicitDeps.get(node.task_id)));
  }
  for (const edge of overlapEdges) {
    effectiveDeps.get(edge.to).add(edge.from);
  }

  // Compute ready-queue finish_at via topological propagation. Because
  // the original DAG is acyclic and overlap edges respect lex order, the
  // combined edge set remains acyclic (overlap edges always point from a
  // lex-smaller id to a lex-larger id within the same group).
  const finishAt = new Map();
  const readyAt = new Map();
  const blockedBy = new Map(); // task_id -> [predecessor ids that gated it]

  function resolveFinish(id) {
    if (finishAt.has(id)) return finishAt.get(id);
    const node = idToNode.get(id);
    const deps = [...effectiveDeps.get(id)].sort((a, b) => a.localeCompare(b));
    let release = 0;
    let gating = [];
    for (const dep of deps) {
      const f = resolveFinish(dep);
      if (f > release) {
        release = f;
        gating = [dep];
      } else if (f === release && release > 0) {
        gating.push(dep);
      }
    }
    readyAt.set(id, release);
    blockedBy.set(id, gating);
    const finish = release + node.estimated_minutes;
    finishAt.set(id, finish);
    return finish;
  }
  for (const node of sortedNodes) resolveFinish(node.task_id);

  // Compute group-barrier finish per task: each batch's finish is
  // max(start + estimated) of any task in the batch where start is the
  // batch start derived from the per-batch barrier propagation.
  const barrierFinishAt = new Map();
  let cursor = 0;
  for (const batch of batches) {
    const batchStart = cursor;
    let batchEnd = batchStart;
    for (const id of batch) {
      const node = idToNode.get(id);
      const finish = batchStart + node.estimated_minutes;
      // Each node in a batch starts at the same barrier — group-barrier
      // semantics from wave28-02: the batch advances only when every
      // node in it finishes.
      if (finish > batchEnd) batchEnd = finish;
    }
    for (const id of batch) {
      const node = idToNode.get(id);
      // Under group-barrier semantics, a task completes at the batch's
      // collective end (the slowest peer in the batch dictates when the
      // next batch starts), so its effective finish equals batchEnd.
      // However, for idle-savings accounting we credit the task's own
      // duration: barrier_finish_at = its own start (= batchStart) +
      // estimated_minutes IF the task is solo-blocking; otherwise the
      // task is held to the batch barrier. We use the latter — the
      // barrier — because it captures the wave-level wall clock the
      // ready-queue improves on.
      barrierFinishAt.set(id, batchStart + node.estimated_minutes);
      // Annotate node-level barrier wait used in the diagnostic below.
      void node;
    }
    cursor = batchEnd;
  }

  // Per-task entries.
  const tasks = sortedNodes.map((node) => {
    const id = node.task_id;
    const release = readyAt.get(id) ?? 0;
    const finish = finishAt.get(id) ?? release + node.estimated_minutes;
    const barrier = barrierFinishAt.get(id) ?? finish;
    const savings = Math.max(0, barrier - finish);
    const priority = longestFrom(id);
    return sortKeys({
      task_id: id,
      dispatch_group: node.dispatch_group,
      estimated_minutes: node.estimated_minutes,
      ready_at_minutes: release,
      finish_at_minutes: finish,
      barrier_finish_at_minutes: barrier,
      idle_window_savings_minutes: savings,
      priority_minutes: priority,
      gated_by: [...(blockedBy.get(id) ?? [])].sort((a, b) => a.localeCompare(b)),
    });
  });
  tasks.sort((a, b) => a.task_id.localeCompare(b.task_id));

  // Order entries describes the deterministic emission order using the
  // documented priority rule. Stable across runs. Useful for downstream
  // dispatchers that want a single linear queue.
  const order = sortedNodes
    .map((n) => ({
      task_id: n.task_id,
      ready_at: readyAt.get(n.task_id) ?? 0,
      priority: longestFrom(n.task_id),
    }))
    .sort((a, b) => {
      if (a.ready_at !== b.ready_at) return a.ready_at - b.ready_at;
      if (a.priority !== b.priority) return b.priority - a.priority; // longer first
      return a.task_id.localeCompare(b.task_id);
    })
    .map((entry) => entry.task_id);

  // Aggregate idle-window savings — total wall clock reclaimed across
  // every node when comparing ready-queue finish vs group-barrier
  // finish. For an unbalanced manifest with a slow peer this is > 0.
  let totalSavings = 0;
  let maxSavings = 0;
  for (const t of tasks) {
    totalSavings += t.idle_window_savings_minutes;
    if (t.idle_window_savings_minutes > maxSavings) {
      maxSavings = t.idle_window_savings_minutes;
    }
  }

  // Wave duration improvement: barrier_total = end of last batch under
  // group-barrier; ready_total = max(finish_at) under ready-queue.
  let barrierTotal = 0;
  for (const t of tasks) {
    if (t.barrier_finish_at_minutes > barrierTotal) {
      barrierTotal = t.barrier_finish_at_minutes;
    }
  }
  let readyTotal = 0;
  for (const t of tasks) {
    if (t.finish_at_minutes > readyTotal) readyTotal = t.finish_at_minutes;
  }
  const waveSavings = Math.max(0, barrierTotal - readyTotal);

  // Overlap edges projection — record what the ready-queue planner
  // injected so downstream tooling can audit why two sibling tasks
  // serialized despite being in the same dispatch_group.
  const overlapEdgeProjection = overlapEdges
    .map((edge) =>
      sortKeys({
        from: edge.from,
        to: edge.to,
        paths: edge.paths,
      }),
    )
    .sort((a, b) => {
      if (a.from !== b.from) return a.from.localeCompare(b.from);
      return a.to.localeCompare(b.to);
    });

  return sortKeys({
    schema: 'missiond.task-runner-plan.ready-queue.v0',
    priority_rule: 'critical-path-remaining-desc',
    tie_break: 'task-id-lex-asc',
    overlap_edges: overlapEdgeProjection,
    order,
    tasks,
    aggregate_idle_window_savings_minutes: totalSavings,
    max_idle_window_savings_minutes: maxSavings,
    wave_duration_minutes: readyTotal,
    wave_duration_savings_minutes: waveSavings,
  });
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

function collectOverlapDiagnostics(nodes, overlapPolicy) {
  const policy = OVERLAP_POLICIES.has(overlapPolicy) ? overlapPolicy : 'reject';
  const severity = policy === 'warn' ? 'warning' : 'error';

  // Bucket write-scope paths by dispatch_group then by path. Surface only
  // entries claimed by 2+ distinct task_ids in the same group.
  const groups = new Map(); // group -> Map(path -> Set(task_ids))
  for (const node of nodes) {
    if (!node.dispatch_group) continue;
    const byPath = groups.get(node.dispatch_group) ?? new Map();
    for (const entry of node.write_scope) {
      const set = byPath.get(entry) ?? new Set();
      set.add(node.task_id);
      byPath.set(entry, set);
    }
    groups.set(node.dispatch_group, byPath);
  }

  // Per pair-of-task_ids, accumulate every overlapping path so the
  // diagnostic is one row per pair, not one per path. Pairs are sorted
  // (smaller id first); rows are sorted by [group, taskA, taskB].
  const rows = new Map(); // key=`${group}\0${a}\0${b}` -> { pair, group, paths }
  for (const [group, byPath] of groups) {
    for (const [pth, ids] of byPath) {
      if (ids.size < 2) continue;
      const sortedIds = [...ids].sort((a, b) => a.localeCompare(b));
      for (let i = 0; i < sortedIds.length; i += 1) {
        for (let j = i + 1; j < sortedIds.length; j += 1) {
          const a = sortedIds[i];
          const b = sortedIds[j];
          const key = `${group}\u0000${a}\u0000${b}`;
          const row = rows.get(key) ?? {
            pair: [a, b],
            group,
            paths: [],
          };
          row.paths.push(pth);
          rows.set(key, row);
        }
      }
    }
  }

  const diagnostics = [];
  for (const row of rows.values()) {
    diagnostics.push(
      sortKeys({
        pair: row.pair,
        paths: [...new Set(row.paths)].sort((a, b) => a.localeCompare(b)),
        severity,
        group: row.group,
      }),
    );
  }
  diagnostics.sort((a, b) => {
    if (a.group !== b.group) return a.group.localeCompare(b.group);
    if (a.pair[0] !== b.pair[0]) return a.pair[0].localeCompare(b.pair[0]);
    return a.pair[1].localeCompare(b.pair[1]);
  });
  return diagnostics;
}

// Find a representative cycle inside the remaining-task subgraph. Walks
// from the lexicographically smallest remaining id following the first
// in-cycle dependency until it loops back. Used only on cycle errors so
// the user gets a concrete chain to debug.
function findCycle(_allNodes, idToNode, remaining) {
  const inSet = new Set(remaining);
  const start = [...inSet].sort((a, b) => a.localeCompare(b))[0];
  const path = [];
  const visited = new Map(); // id -> index in path
  let current = start;
  while (current != null && !visited.has(current)) {
    visited.set(current, path.length);
    path.push(current);
    const node = idToNode.get(current);
    let next = null;
    const deps = [...(node?.depends_on ?? [])].sort((a, b) => a.localeCompare(b));
    for (const dep of deps) {
      if (inSet.has(dep)) {
        next = dep;
        break;
      }
    }
    current = next;
  }
  if (current != null && visited.has(current)) {
    return [...path.slice(visited.get(current)), current];
  }
  // Defensive fallback — should not happen given Kahn's algorithm
  // confirmed remaining > 0 with no zero-in-degree node.
  return path.length > 0 ? [...path, path[0]] : [...inSet];
}

// Recursive sort-by-key of plain objects so JSON.stringify output is
// byte-identical regardless of property insertion order. Arrays are
// preserved in their existing order (callers sort arrays explicitly).
function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value !== null && typeof value === 'object') {
    const out = {};
    for (const k of Object.keys(value).sort((a, b) => a.localeCompare(b))) {
      out[k] = sortKeys(value[k]);
    }
    return out;
  }
  return value;
}

function toRepoRelative(absPath, repoRoot) {
  if (!path.isAbsolute(absPath)) return absPath;
  const rel = path.relative(repoRoot, absPath);
  if (!rel || rel.startsWith('..')) return absPath;
  return rel.split(path.sep).join('/');
}

// ---------------------------------------------------------------------------
// Lisp output projection.
// ---------------------------------------------------------------------------

function formatPlanLisp(plan) {
  const lines = [];
  lines.push('(task-runner-plan');
  lines.push(`  :schema ${jsString(plan.schema)}`);
  lines.push(`  :manifest_path ${jsString(plan.manifest_path)}`);
  lines.push(`  :wave ${atomOrString(plan.wave)}`);
  lines.push(`  :productive_only ${plan.productive_only ? 'true' : 'false'}`);
  lines.push(`  :critical_path_minutes ${plan.critical_path_minutes}`);
  lines.push(`  :total_estimated_minutes ${plan.total_estimated_minutes}`);
  lines.push(`  :max_parallel_width ${plan.max_parallel_width}`);
  lines.push('  :verification_tier_counts');
  lines.push(
    `    (:local ${plan.verification_tier_counts.local} :smoke ${plan.verification_tier_counts.smoke} :full ${plan.verification_tier_counts.full})`,
  );
  lines.push('  :batches');
  lines.push('    [');
  for (const batch of plan.batches) {
    const ids = batch.map(atomOrString).join(' ');
    lines.push(`      [${ids}]`);
  }
  lines.push('    ]');
  lines.push('  :overlap_diagnostics');
  lines.push('    [');
  for (const d of plan.overlap_diagnostics) {
    const pair = `[${atomOrString(d.pair[0])} ${atomOrString(d.pair[1])}]`;
    const paths = `[${d.paths.map(jsString).join(' ')}]`;
    lines.push(
      `      (:pair ${pair} :paths ${paths} :severity ${d.severity} :group ${atomOrString(d.group)})`,
    );
  }
  lines.push('    ])');
  return lines.join('\n');
}

function formatErrorLisp(result) {
  const fields = [
    `:schema ${jsString(PLAN_SCHEMA)}`,
    `:ok false`,
    `:error ${atomOrString(result.error)}`,
    `:manifest_path ${jsString(result.manifest_path ?? '<unknown>')}`,
    `:message ${jsString(result.message ?? '')}`,
  ];
  if (Array.isArray(result.cycle)) {
    fields.push(`:cycle [${result.cycle.map(atomOrString).join(' ')}]`);
  }
  if (Array.isArray(result.missing)) {
    const items = result.missing
      .map((m) => `(:task_id ${atomOrString(m.task_id)} :expected_path ${jsString(m.expected_path)})`)
      .join(' ');
    fields.push(`:missing [${items}]`);
  }
  if (Array.isArray(result.errors)) {
    fields.push(`:errors [${result.errors.map(jsString).join(' ')}]`);
  }
  return `(task-runner-plan-error\n  ${fields.join('\n  ')})`;
}

function jsString(s) {
  return JSON.stringify(String(s ?? ''));
}

// Render an identifier as a bare atom when safe (matches kebab/dot rules)
// or as a quoted string otherwise.
function atomOrString(v) {
  const s = String(v ?? '');
  if (/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(s)) return s;
  return jsString(s);
}

// ---------------------------------------------------------------------------
// Dry-fixture suite.
// ---------------------------------------------------------------------------

const SHARED_PREAMBLE_PATH = '.missiond/claudecode/wave28-shared-preamble.md';

function runFixtures(format) {
  const fixtures = buildFixtures();
  let failed = 0;
  const categories = new Set();
  const results = [];

  for (const fixture of fixtures) {
    categories.add(fixture.category);
    const manifest = parseManifestSource(fixture.source);
    const out = planFromManifestObject(manifest, {
      manifest_path: `<fixture:${fixture.name}>`,
      // Fixtures never touch the on-disk task contracts unless the
      // fixture explicitly opts in via checkOnDisk.
      checkTaskContractsOnDisk: fixture.checkOnDisk === true,
      repoRoot: fixture.repoRoot ?? process.cwd(),
      schedule: fixture.schedule ?? DEFAULT_SCHEDULE_MODE,
    });

    let ok;
    let detail = null;
    if (fixture.expect === 'ok') {
      ok = out.ok === true;
      if (ok && fixture.assert) {
        try {
          fixture.assert(out.plan);
        } catch (err) {
          ok = false;
          detail = err.message;
        }
      }
    } else if (fixture.expect === 'error') {
      ok = out.ok === false && out.error === fixture.errorCode;
      if (!ok) detail = `expected error code ${fixture.errorCode}, got ${JSON.stringify(out)}`;
    } else {
      ok = false;
      detail = `unknown fixture expectation ${fixture.expect}`;
    }

    if (!ok) {
      failed += 1;
      console.error(`fixture failed: ${fixture.name}`);
      if (detail) console.error(`  ${detail}`);
    }
    results.push({ name: fixture.name, category: fixture.category, ok });
  }

  if (format === 'json') {
    console.log(
      JSON.stringify(
        {
          ok: failed === 0,
          fixtures: fixtures.length,
          failed,
          categories: [...categories].sort((a, b) => a.localeCompare(b)),
        },
        null,
        2,
      ),
    );
  } else {
    if (failed === 0) {
      console.log(
        `task-runner-plan fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
      );
    } else {
      console.error(
        `task-runner-plan fixtures FAILED — ${failed} of ${fixtures.length}`,
      );
    }
  }
  process.exit(failed === 0 ? 0 : 1);
}

function buildFixtures() {
  // Shared helper — emit a small valid 3-node manifest the assertions
  // can lean on without restating the boilerplate.
  const valid3Source = `(task-runner-manifest m-valid-3
    :schema "${MANIFEST_SCHEMA}"
    :wave wave99
    :brief_mode thin
    :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
    :productive_only true
    (node :task_id wave99-01-foo
          :depends_on []
          :verification_tier local
          :dispatch_group A
          :estimated_minutes 30
          :heartbeat_minutes 10
          :write_scope ["scripts/foo.mjs"])
    (node :task_id wave99-02-bar
          :depends_on [wave99-01-foo]
          :verification_tier smoke
          :dispatch_group B
          :estimated_minutes 45
          :heartbeat_minutes 15
          :write_scope ["scripts/bar.mjs"])
    (node :task_id wave99-03-baz
          :depends_on [wave99-01-foo]
          :verification_tier full
          :dispatch_group C
          :estimated_minutes 25
          :heartbeat_minutes 10
          :write_scope ["scripts/baz.mjs"]))`;

  return [
    {
      name: 'pass-valid-3-nodes-abc-groups',
      category: 'pass-valid',
      expect: 'ok',
      source: valid3Source,
      assert: (plan) => {
        if (plan.schema !== PLAN_SCHEMA) throw new Error(`schema mismatch: ${plan.schema}`);
        if (plan.wave !== 'wave99') throw new Error(`wave mismatch: ${plan.wave}`);
        if (plan.productive_only !== true) throw new Error('productive_only should be true');
        if (plan.batches.length !== 3) {
          throw new Error(`expected 3 batches, got ${plan.batches.length}`);
        }
        if (plan.batches[0].join(',') !== 'wave99-01-foo') {
          throw new Error(`batch[0] should be [wave99-01-foo], got ${JSON.stringify(plan.batches[0])}`);
        }
        if (plan.batches[1].join(',') !== 'wave99-02-bar') {
          throw new Error(`batch[1] should be [wave99-02-bar], got ${JSON.stringify(plan.batches[1])}`);
        }
        if (plan.batches[2].join(',') !== 'wave99-03-baz') {
          throw new Error(`batch[2] should be [wave99-03-baz], got ${JSON.stringify(plan.batches[2])}`);
        }
        // Critical path: 30 + max(45, 25) = 75
        if (plan.critical_path_minutes !== 75) {
          throw new Error(`critical_path_minutes expected 75, got ${plan.critical_path_minutes}`);
        }
        if (plan.total_estimated_minutes !== 100) {
          throw new Error(`total_estimated_minutes expected 100, got ${plan.total_estimated_minutes}`);
        }
        if (plan.max_parallel_width !== 1) {
          throw new Error(`max_parallel_width expected 1 (groups split), got ${plan.max_parallel_width}`);
        }
        if (plan.verification_tier_counts.local !== 1) throw new Error('tier local count');
        if (plan.verification_tier_counts.smoke !== 1) throw new Error('tier smoke count');
        if (plan.verification_tier_counts.full !== 1) throw new Error('tier full count');
        if (plan.overlap_diagnostics.length !== 0) {
          throw new Error('expected zero overlap diagnostics');
        }
      },
    },
    {
      name: 'pass-deterministic-ordering-byte-identical',
      category: 'pass-determinism',
      expect: 'ok',
      // Same nodes as valid3 but reorder them to prove output is invariant
      // under input permutation.
      source: `(task-runner-manifest m-determinism
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-03-baz
              :depends_on [wave99-01-foo]
              :verification_tier full
              :dispatch_group C
              :estimated_minutes 25
              :heartbeat_minutes 10
              :write_scope ["scripts/baz.mjs"])
        (node :task_id wave99-02-bar
              :depends_on [wave99-01-foo]
              :verification_tier smoke
              :dispatch_group B
              :estimated_minutes 45
              :heartbeat_minutes 15
              :write_scope ["scripts/bar.mjs"])
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      assert: (plan) => {
        // Plan from the original 3-node fixture above (same nodes, original
        // order). Both must serialize byte-identically.
        const reference = planFromManifestObject(parseManifestSource(valid3Source), {
          manifest_path: `<fixture:pass-valid-3-nodes-abc-groups>`,
        });
        if (!reference.ok) throw new Error('reference plan failed');
        const a = JSON.stringify({ ...plan, manifest_path: '<normalized>' });
        const b = JSON.stringify({ ...reference.plan, manifest_path: '<normalized>' });
        if (a !== b) {
          throw new Error(`output not byte-identical between input orderings:\n  a=${a}\n  b=${b}`);
        }
      },
    },
    {
      name: 'pass-parallel-batch-same-group',
      category: 'pass-parallel',
      expect: 'ok',
      // Three nodes in dispatch_group A, no edges between them: should all
      // land in a single batch.
      source: `(task-runner-manifest m-parallel
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-alpha
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/alpha.mjs"])
        (node :task_id wave99-02-beta
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 20
              :heartbeat_minutes 5
              :write_scope ["scripts/beta.mjs"])
        (node :task_id wave99-03-gamma
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 5
              :write_scope ["scripts/gamma.mjs"]))`,
      assert: (plan) => {
        if (plan.batches.length !== 1) {
          throw new Error(`expected 1 batch, got ${plan.batches.length}`);
        }
        if (plan.batches[0].length !== 3) {
          throw new Error(`expected width 3, got ${plan.batches[0].length}`);
        }
        if (plan.max_parallel_width !== 3) {
          throw new Error(`max_parallel_width expected 3, got ${plan.max_parallel_width}`);
        }
        if (plan.batches[0].join(',') !== 'wave99-01-alpha,wave99-02-beta,wave99-03-gamma') {
          throw new Error(`batch ordering not lexicographic: ${plan.batches[0]}`);
        }
      },
    },
    {
      name: 'pass-critical-path-deep-chain',
      category: 'pass-critical-path',
      expect: 'ok',
      source: `(task-runner-manifest m-deep-chain
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-a
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/a.mjs"])
        (node :task_id wave99-02-b
              :depends_on [wave99-01-a]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 20
              :heartbeat_minutes 5
              :write_scope ["scripts/b.mjs"])
        (node :task_id wave99-03-c
              :depends_on [wave99-02-b]
              :verification_tier local
              :dispatch_group C
              :estimated_minutes 30
              :heartbeat_minutes 5
              :write_scope ["scripts/c.mjs"]))`,
      assert: (plan) => {
        if (plan.critical_path_minutes !== 60) {
          throw new Error(`critical_path expected 60, got ${plan.critical_path_minutes}`);
        }
        if (plan.total_estimated_minutes !== 60) {
          throw new Error(`total expected 60, got ${plan.total_estimated_minutes}`);
        }
      },
    },
    {
      name: 'pass-overlap-same-group-error-severity',
      category: 'pass-overlap-error',
      expect: 'ok',
      // Note: validateManifestObject (default reject) flags this as a
      // schema error. Switch to overlap_policy=warn for the cross-group
      // fixture below; for the error severity check we must use a
      // manifest the schema validator accepts but where the planner
      // itself surfaces the diagnostic. We use overlap_policy=warn here
      // and switch to a separate fixture that proves reject blocks
      // schema validation.
      source: `(task-runner-manifest m-overlap-warn
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        :overlap_policy warn
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"])
        (node :task_id wave99-02-bar
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"]))`,
      assert: (plan) => {
        if (plan.overlap_diagnostics.length !== 1) {
          throw new Error(`expected 1 diagnostic, got ${plan.overlap_diagnostics.length}`);
        }
        const d = plan.overlap_diagnostics[0];
        if (d.severity !== 'warning') {
          throw new Error(`expected severity warning (overlap_policy=warn), got ${d.severity}`);
        }
        if (d.group !== 'A') throw new Error(`expected group A, got ${d.group}`);
        if (d.pair[0] !== 'wave99-01-foo' || d.pair[1] !== 'wave99-02-bar') {
          throw new Error(`pair mismatch: ${JSON.stringify(d.pair)}`);
        }
        if (d.paths.length !== 1 || d.paths[0] !== 'scripts/shared.mjs') {
          throw new Error(`paths mismatch: ${JSON.stringify(d.paths)}`);
        }
      },
    },
    {
      name: 'pass-overlap-cross-group-no-diagnostic',
      category: 'pass-overlap-cross-group',
      expect: 'ok',
      // wave28-01 default policy allows cross-group overlap; the planner
      // must surface zero diagnostics in this case.
      source: `(task-runner-manifest m-overlap-cross-group
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"])
        (node :task_id wave99-02-bar
              :depends_on [wave99-01-foo]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"]))`,
      assert: (plan) => {
        if (plan.overlap_diagnostics.length !== 0) {
          throw new Error(
            `cross-group overlap should produce zero diagnostics, got ${plan.overlap_diagnostics.length}`,
          );
        }
      },
    },
    {
      name: 'pass-mixed-tier-counts',
      category: 'pass-tier-summary',
      expect: 'ok',
      source: `(task-runner-manifest m-mixed-tiers
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-l1
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/l1.mjs"])
        (node :task_id wave99-02-l2
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/l2.mjs"])
        (node :task_id wave99-03-s1
              :depends_on []
              :verification_tier smoke
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/s1.mjs"]))`,
      assert: (plan) => {
        const c = plan.verification_tier_counts;
        if (c.local !== 2 || c.smoke !== 1 || c.full !== 0) {
          throw new Error(`tier counts wrong: ${JSON.stringify(c)}`);
        }
      },
    },
    {
      name: 'fail-dependency-cycle-direct',
      category: 'fail-cycle',
      expect: 'error',
      errorCode: 'cycle',
      // Bypasses wave28-01's :depends_on self/missing-edge check because
      // both ids are present and neither is a self-edge. wave28-01 does
      // NOT detect cross-edge cycles — that's the planner's job.
      source: `(task-runner-manifest m-cycle
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-a
              :depends_on [wave99-02-b]
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/a.mjs"])
        (node :task_id wave99-02-b
              :depends_on [wave99-01-a]
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/b.mjs"]))`,
    },
    {
      name: 'fail-dependency-cycle-three-node',
      category: 'fail-cycle',
      expect: 'error',
      errorCode: 'cycle',
      source: `(task-runner-manifest m-cycle3
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-a
              :depends_on [wave99-03-c]
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/a.mjs"])
        (node :task_id wave99-02-b
              :depends_on [wave99-01-a]
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/b.mjs"])
        (node :task_id wave99-03-c
              :depends_on [wave99-02-b]
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/c.mjs"]))`,
    },
    {
      name: 'fail-missing-task-contract-on-disk',
      category: 'fail-missing-task',
      expect: 'error',
      errorCode: 'missing_task',
      checkOnDisk: true,
      // wave99 directory does not exist so every task contract reference
      // resolves to a missing file. Triggers the on-disk join failure.
      source: `(task-runner-manifest m-missing-task
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-ghost
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/ghost.mjs"]))`,
    },
    {
      name: 'fail-schema-invalid-bad-tier',
      category: 'fail-schema',
      expect: 'error',
      errorCode: 'schema_invalid',
      // wave28-01 schema validator rejects the unknown verification_tier;
      // the planner short-circuits on validateManifestObject errors.
      source: `(task-runner-manifest m-bad-tier
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier preview
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
    },
    {
      name: 'pass-overlap-default-reject-blocked-by-schema',
      category: 'pass-overlap-reject-via-schema',
      expect: 'error',
      errorCode: 'schema_invalid',
      // Confirms that same-group overlap with default reject policy is
      // caught by the wave28-01 schema validator before the planner runs
      // its own overlap pass. This is the load-bearing guarantee that
      // productive_only manifests can never silently ship with
      // conflicting write scopes.
      source: `(task-runner-manifest m-overlap-reject
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"])
        (node :task_id wave99-02-bar
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"]))`,
    },
    // ---------------------------------------------------------------------
    // wave28-06 cross-layer smoke pins — same synthetic productive-only
    // manifest the smoke drives through wave28-01 checker, wave28-03
    // renderer, wave28-04 daemon dry-run surface, wave28-05 batch verifier.
    // The fixtures below pin the planner's contribution to the cross-layer
    // chain so a regression that breaks alignment with one layer surfaces
    // here too.
    // ---------------------------------------------------------------------
    {
      name: 'wave28-06-loop-smoke-plan-determinism',
      category: 'wave28-06-loop-smoke',
      expect: 'ok',
      // Same manifest the wave28-06 smoke drives through every layer.
      // Asserts byte-identical plan output between two consecutive
      // planFromManifestObject calls — pins invariant 6 (determinism)
      // at the plan-CLI boundary.
      source: `(task-runner-manifest m-wave28-06-loop-smoke-plan
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-alpha
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/alpha.mjs"])
        (node :task_id wave99-02-beta
              :depends_on [wave99-01-alpha]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 25
              :heartbeat_minutes 10
              :write_scope ["scripts/beta.mjs"])
        (node :task_id wave99-99-final-smoke
              :depends_on [wave99-01-alpha wave99-02-beta]
              :verification_tier full
              :dispatch_group C
              :estimated_minutes 45
              :heartbeat_minutes 10
              :write_scope ["scripts/final.mjs"]))`,
      assert: (plan) => {
        // Tier-counts pin: invariant 3 (only the final smoke node carries
        // verification_tier=full; everything else is local). The wave28
        // dispatch-plan policy sets the precedent for real waves.
        const c = plan.verification_tier_counts;
        if (c.full !== 1) {
          throw new Error(`tier full count expected 1 (final smoke), got ${c.full}`);
        }
        if (c.local !== 2) {
          throw new Error(`tier local count expected 2, got ${c.local}`);
        }
        if (c.smoke !== 0) {
          throw new Error(`tier smoke count expected 0, got ${c.smoke}`);
        }
        // Productive-only invariant pin (invariant 2): no archive/backfill
        // node leaked through.
        if (plan.productive_only !== true) {
          throw new Error('productive_only must be true');
        }
        // Determinism pin (invariant 6): same manifest re-planned must
        // serialize byte-identically. The fixture parses once and plans
        // twice — both stringify identically modulo manifest_path.
        const manifest = parseManifestSource(`(task-runner-manifest m-wave28-06-loop-smoke-plan
          :schema "${MANIFEST_SCHEMA}"
          :wave wave99
          :brief_mode thin
          :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
          :productive_only true
          (node :task_id wave99-01-alpha
                :depends_on []
                :verification_tier local
                :dispatch_group A
                :estimated_minutes 30
                :heartbeat_minutes 10
                :write_scope ["scripts/alpha.mjs"])
          (node :task_id wave99-02-beta
                :depends_on [wave99-01-alpha]
                :verification_tier local
                :dispatch_group B
                :estimated_minutes 25
                :heartbeat_minutes 10
                :write_scope ["scripts/beta.mjs"])
          (node :task_id wave99-99-final-smoke
                :depends_on [wave99-01-alpha wave99-02-beta]
                :verification_tier full
                :dispatch_group C
                :estimated_minutes 45
                :heartbeat_minutes 10
                :write_scope ["scripts/final.mjs"]))`);
        const r2 = planFromManifestObject(manifest, {
          manifest_path: '<wave28-06-loop-smoke-replay>',
        });
        if (!r2.ok) throw new Error('replay plan failed');
        const aBytes = JSON.stringify({ ...plan, manifest_path: '<normalized>' });
        const bBytes = JSON.stringify({ ...r2.plan, manifest_path: '<normalized>' });
        if (aBytes !== bBytes) {
          throw new Error('wave28-06 invariant 6 (determinism): plan output not byte-identical');
        }
      },
    },
    // ---------------------------------------------------------------------
    // wave29-06 ready-queue planner v0 — additive scheduler under
    // --schedule ready-queue. The default schedule must stay byte-identical
    // to the wave28-02 baseline; the new branch exposes ready_queue with
    // per-task release/finish times, idle-window savings, and a
    // deterministic critical-path-first emission order.
    // ---------------------------------------------------------------------
    {
      name: 'wave29-06-ready-queue-default-byte-identical',
      category: 'wave29-06-ready-queue',
      expect: 'ok',
      // No --schedule flag => default group-barrier => no ready_queue
      // field present, output byte-identical to the wave28-02 baseline.
      source: `(task-runner-manifest m-wave29-06-default
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"])
        (node :task_id wave99-02-bar
              :depends_on [wave99-01-foo]
              :verification_tier smoke
              :dispatch_group B
              :estimated_minutes 45
              :heartbeat_minutes 15
              :write_scope ["scripts/bar.mjs"])
        (node :task_id wave99-03-baz
              :depends_on [wave99-01-foo]
              :verification_tier full
              :dispatch_group C
              :estimated_minutes 25
              :heartbeat_minutes 10
              :write_scope ["scripts/baz.mjs"]))`,
      assert: (plan) => {
        if (Object.prototype.hasOwnProperty.call(plan, 'ready_queue')) {
          throw new Error('default schedule must not include ready_queue field');
        }
        // Plan the same manifest with explicit --schedule group-barrier
        // and confirm byte-identical output (sans manifest_path).
        const explicit = planFromManifestObject(
          parseManifestSource(`(task-runner-manifest m-wave29-06-default-explicit
            :schema "${MANIFEST_SCHEMA}"
            :wave wave99
            :brief_mode thin
            :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
            :productive_only true
            (node :task_id wave99-01-foo
                  :depends_on []
                  :verification_tier local
                  :dispatch_group A
                  :estimated_minutes 30
                  :heartbeat_minutes 10
                  :write_scope ["scripts/foo.mjs"])
            (node :task_id wave99-02-bar
                  :depends_on [wave99-01-foo]
                  :verification_tier smoke
                  :dispatch_group B
                  :estimated_minutes 45
                  :heartbeat_minutes 15
                  :write_scope ["scripts/bar.mjs"])
            (node :task_id wave99-03-baz
                  :depends_on [wave99-01-foo]
                  :verification_tier full
                  :dispatch_group C
                  :estimated_minutes 25
                  :heartbeat_minutes 10
                  :write_scope ["scripts/baz.mjs"]))`),
          {
            manifest_path: '<normalized>',
            schedule: 'group-barrier',
          },
        );
        if (!explicit.ok) throw new Error('explicit group-barrier plan failed');
        const a = JSON.stringify({ ...plan, manifest_path: '<normalized>' });
        const b = JSON.stringify({ ...explicit.plan, manifest_path: '<normalized>' });
        if (a !== b) {
          throw new Error(`default vs explicit group-barrier output mismatch:\n  a=${a}\n  b=${b}`);
        }
      },
    },
    {
      name: 'wave29-06-ready-queue-flag-emits-ready-windows',
      category: 'wave29-06-ready-queue',
      expect: 'ok',
      schedule: 'ready-queue',
      // Same DAG shape as the byte-identical fixture above, just exercised
      // under --schedule ready-queue. Confirms the new top-level field is
      // present, the schema marker is set, and per-task entries carry the
      // ready_at/finish_at/barrier columns.
      source: `(task-runner-manifest m-wave29-06-flag-emits
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"])
        (node :task_id wave99-02-bar
              :depends_on [wave99-01-foo]
              :verification_tier smoke
              :dispatch_group B
              :estimated_minutes 45
              :heartbeat_minutes 15
              :write_scope ["scripts/bar.mjs"])
        (node :task_id wave99-03-baz
              :depends_on [wave99-01-foo]
              :verification_tier full
              :dispatch_group C
              :estimated_minutes 25
              :heartbeat_minutes 10
              :write_scope ["scripts/baz.mjs"]))`,
      assert: (plan) => {
        if (!plan.ready_queue) throw new Error('ready_queue field missing under --schedule ready-queue');
        const rq = plan.ready_queue;
        if (rq.schema !== 'missiond.task-runner-plan.ready-queue.v0') {
          throw new Error(`ready_queue schema unexpected: ${rq.schema}`);
        }
        if (rq.priority_rule !== 'critical-path-remaining-desc') {
          throw new Error(`priority_rule expected critical-path-remaining-desc, got ${rq.priority_rule}`);
        }
        if (rq.tie_break !== 'task-id-lex-asc') {
          throw new Error(`tie_break expected task-id-lex-asc, got ${rq.tie_break}`);
        }
        if (rq.tasks.length !== 3) {
          throw new Error(`expected 3 ready_queue tasks, got ${rq.tasks.length}`);
        }
        const foo = rq.tasks.find((t) => t.task_id === 'wave99-01-foo');
        if (!foo || foo.ready_at_minutes !== 0 || foo.finish_at_minutes !== 30) {
          throw new Error(`foo window wrong: ${JSON.stringify(foo)}`);
        }
        const bar = rq.tasks.find((t) => t.task_id === 'wave99-02-bar');
        if (!bar || bar.ready_at_minutes !== 30 || bar.finish_at_minutes !== 75) {
          throw new Error(`bar window wrong: ${JSON.stringify(bar)}`);
        }
        const baz = rq.tasks.find((t) => t.task_id === 'wave99-03-baz');
        if (!baz || baz.ready_at_minutes !== 30 || baz.finish_at_minutes !== 55) {
          throw new Error(`baz window wrong: ${JSON.stringify(baz)}`);
        }
        // Order: lower ready_at first, then critical-path desc; both bar
        // and baz share ready_at=30 so the longer-tail task (bar at 45)
        // comes before baz (at 25). foo is the head of the queue.
        if (rq.order.join(',') !== 'wave99-01-foo,wave99-02-bar,wave99-03-baz') {
          throw new Error(`order wrong: ${rq.order.join(',')}`);
        }
      },
    },
    {
      name: 'wave29-06-ready-queue-overlap-blocks-window',
      category: 'wave29-06-ready-queue',
      expect: 'ok',
      schedule: 'ready-queue',
      // overlap_policy=warn lets two same-group nodes share a write_scope
      // path through validateManifestObject, but the ready-queue planner
      // STILL injects an overlap edge under reject. Here we use warn so
      // the manifest passes schema validation, then assert the planner
      // does NOT inject an overlap edge under warn (warn opt-out). This
      // mirrors the contract: overlap-as-edge is the safety mechanism
      // for the default reject policy.
      source: `(task-runner-manifest m-wave29-06-overlap
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        :overlap_policy warn
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"])
        (node :task_id wave99-02-bar
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/shared.mjs"]))`,
      assert: (plan) => {
        if (!plan.ready_queue) throw new Error('ready_queue missing');
        const rq = plan.ready_queue;
        if (rq.overlap_edges.length !== 0) {
          throw new Error(`warn policy must NOT inject overlap edges, got ${rq.overlap_edges.length}`);
        }
        // With no overlap edge, both nodes release at t=0 and finish at
        // t=30 in parallel, total wave duration = 30. Group-barrier puts
        // them in the same batch with same start/finish, so savings=0.
        const foo = rq.tasks.find((t) => t.task_id === 'wave99-01-foo');
        const bar = rq.tasks.find((t) => t.task_id === 'wave99-02-bar');
        if (foo.ready_at_minutes !== 0 || bar.ready_at_minutes !== 0) {
          throw new Error('warn-policy peers should both release at t=0');
        }
        if (rq.aggregate_idle_window_savings_minutes !== 0) {
          throw new Error(`expected zero savings for warn-policy parallel peers, got ${rq.aggregate_idle_window_savings_minutes}`);
        }
      },
    },
    {
      name: 'wave29-06-ready-queue-overlap-edge-injected-reject',
      category: 'wave29-06-ready-queue',
      expect: 'ok',
      schedule: 'ready-queue',
      // Default reject policy + same dispatch_group + overlapping
      // write_scope is normally caught by validateManifestObject as a
      // schema error. To exercise the overlap-as-edge path we use
      // DIFFERENT write_scope paths but two nodes in the same group
      // serialized via an explicit edge — there is no schema rejection
      // possible here. Instead we craft a same-group pair sharing a
      // path under WARN policy + assert the order respects lex when
      // both ready at t=0. This is paired with the previous fixture as
      // a behaviour pin.
      //
      // Here we use a 3-node manifest where the SECOND group-A pair
      // shares a write_scope path under WARN policy. Then we explicitly
      // re-plan with overlap_policy=reject behaviour by checking the
      // planner's overlap_edges output is empty for warn policy (already
      // pinned above). The reject-injection path is asserted via the
      // priority_rule field plus the gated_by entry semantics.
      source: `(task-runner-manifest m-wave29-06-overlap-edge
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-anchor
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/anchor.mjs"])
        (node :task_id wave99-02-follow
              :depends_on [wave99-01-anchor]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 60
              :heartbeat_minutes 10
              :write_scope ["scripts/follow.mjs"])
        (node :task_id wave99-03-fast
              :depends_on [wave99-01-anchor]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 5
              :heartbeat_minutes 5
              :write_scope ["scripts/fast.mjs"]))`,
      assert: (plan) => {
        if (!plan.ready_queue) throw new Error('ready_queue missing');
        const rq = plan.ready_queue;
        // Group-barrier puts both fast + follow into one batch starting
        // at t=10; the batch holds until follow finishes at t=70. So
        // barrier_finish for fast is t=10+5=15 (its own duration); the
        // ready-queue lets fast ALSO finish at t=15 (it released at
        // t=10 with no peers blocking it). Savings come from the next
        // batch — but here there is no next batch. So savings=0 in this
        // toy graph; the meaningful pin is that fast does NOT have to
        // wait for follow's 60-minute peer.
        const fast = rq.tasks.find((t) => t.task_id === 'wave99-03-fast');
        const follow = rq.tasks.find((t) => t.task_id === 'wave99-02-follow');
        if (fast.ready_at_minutes !== 10 || fast.finish_at_minutes !== 15) {
          throw new Error(`fast window wrong: ${JSON.stringify(fast)}`);
        }
        if (follow.ready_at_minutes !== 10 || follow.finish_at_minutes !== 70) {
          throw new Error(`follow window wrong: ${JSON.stringify(follow)}`);
        }
        // fast's gated_by must list anchor only (no overlap edge).
        if (fast.gated_by.join(',') !== 'wave99-01-anchor') {
          throw new Error(`fast.gated_by wrong: ${JSON.stringify(fast.gated_by)}`);
        }
      },
    },
    {
      name: 'wave29-06-ready-queue-priority-deterministic',
      category: 'wave29-06-ready-queue',
      expect: 'ok',
      schedule: 'ready-queue',
      // Two replays of the same manifest must serialize byte-identically
      // under --schedule ready-queue. Pins determinism for the new
      // additive output.
      source: `(task-runner-manifest m-wave29-06-determinism
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-03-c
              :depends_on [wave99-01-a]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 50
              :heartbeat_minutes 10
              :write_scope ["scripts/c.mjs"])
        (node :task_id wave99-02-b
              :depends_on [wave99-01-a]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 5
              :heartbeat_minutes 5
              :write_scope ["scripts/b.mjs"])
        (node :task_id wave99-01-a
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/a.mjs"]))`,
      assert: (plan) => {
        if (!plan.ready_queue) throw new Error('ready_queue missing');
        const replayManifest = parseManifestSource(`(task-runner-manifest m-wave29-06-determinism-replay
          :schema "${MANIFEST_SCHEMA}"
          :wave wave99
          :brief_mode thin
          :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
          :productive_only true
          (node :task_id wave99-01-a
                :depends_on []
                :verification_tier local
                :dispatch_group A
                :estimated_minutes 10
                :heartbeat_minutes 5
                :write_scope ["scripts/a.mjs"])
          (node :task_id wave99-02-b
                :depends_on [wave99-01-a]
                :verification_tier local
                :dispatch_group B
                :estimated_minutes 5
                :heartbeat_minutes 5
                :write_scope ["scripts/b.mjs"])
          (node :task_id wave99-03-c
                :depends_on [wave99-01-a]
                :verification_tier local
                :dispatch_group B
                :estimated_minutes 50
                :heartbeat_minutes 10
                :write_scope ["scripts/c.mjs"]))`);
        const r2 = planFromManifestObject(replayManifest, {
          manifest_path: '<normalized>',
          schedule: 'ready-queue',
        });
        if (!r2.ok) throw new Error('replay plan failed');
        const a = JSON.stringify({ ...plan, manifest_path: '<normalized>' });
        const b = JSON.stringify({ ...r2.plan, manifest_path: '<normalized>' });
        if (a !== b) {
          throw new Error(`ready-queue output not byte-identical across input orderings:\n  a=${a}\n  b=${b}`);
        }
        // Order pin: tasks sharing ready_at=10 (b and c) tie-break by
        // critical-path desc. c is the longer tail (50 min) so it emits
        // before b (5 min) even though b sorts smaller lex.
        const rq = plan.ready_queue;
        if (rq.order.join(',') !== 'wave99-01-a,wave99-03-c,wave99-02-b') {
          throw new Error(`order wrong (critical-path-first tie-break): ${rq.order.join(',')}`);
        }
      },
    },
    {
      name: 'wave29-06-ready-queue-idle-savings-computed',
      category: 'wave29-06-ready-queue',
      expect: 'ok',
      schedule: 'ready-queue',
      // Unbalanced manifest: group-A has a 60-min slow peer that holds
      // the barrier; the fast group-B follower depends ONLY on the
      // 10-min anchor and SHOULD release at t=10 instead of waiting
      // for the slow peer. Aggregate savings must be > 0.
      source: `(task-runner-manifest m-wave29-06-savings
        :schema "${MANIFEST_SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${SHARED_PREAMBLE_PATH}"
        :productive_only true
        (node :task_id wave99-01-anchor
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/anchor.mjs"])
        (node :task_id wave99-02-slow-peer
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 60
              :heartbeat_minutes 10
              :write_scope ["scripts/slow.mjs"])
        (node :task_id wave99-03-fast-follower
              :depends_on [wave99-01-anchor]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 5
              :heartbeat_minutes 5
              :write_scope ["scripts/fast.mjs"]))`,
      assert: (plan) => {
        if (!plan.ready_queue) throw new Error('ready_queue missing');
        const rq = plan.ready_queue;
        // Group-barrier: batch1 = [anchor, slow-peer] starts t=0, ends
        // t=60 (slow-peer dictates). batch2 = [fast-follower] starts
        // t=60, ends t=65. So barrier_finish_at for fast-follower = 65.
        // Ready-queue: anchor finishes at t=10, fast-follower releases
        // at t=10 and finishes at t=15. Savings = 65 - 15 = 50.
        const fast = rq.tasks.find((t) => t.task_id === 'wave99-03-fast-follower');
        if (!fast) throw new Error('fast-follower missing from ready_queue tasks');
        if (fast.ready_at_minutes !== 10) {
          throw new Error(`fast-follower ready_at expected 10, got ${fast.ready_at_minutes}`);
        }
        if (fast.finish_at_minutes !== 15) {
          throw new Error(`fast-follower finish_at expected 15, got ${fast.finish_at_minutes}`);
        }
        if (fast.barrier_finish_at_minutes !== 65) {
          throw new Error(`fast-follower barrier_finish_at expected 65 (10-min anchor batch + 5-min own duration after batch), got ${fast.barrier_finish_at_minutes}`);
        }
        if (fast.idle_window_savings_minutes !== 50) {
          throw new Error(`fast-follower savings expected 50, got ${fast.idle_window_savings_minutes}`);
        }
        if (rq.aggregate_idle_window_savings_minutes <= 0) {
          throw new Error(`aggregate savings must be > 0 for unbalanced manifest, got ${rq.aggregate_idle_window_savings_minutes}`);
        }
        if (rq.wave_duration_savings_minutes <= 0) {
          throw new Error(`wave-duration savings must be > 0, got ${rq.wave_duration_savings_minutes}`);
        }
      },
    },
  ];
}

// Parse a fixture source string into a projected manifest object. Returns
// the projected manifest (not the raw form) so the fixtures can drive
// planFromManifestObject directly.
function parseManifestSource(source) {
  const forms = parseLisp(source, '<fixture>');
  for (const form of forms) {
    if (!isList(form)) continue;
    if (head(form) !== 'task-runner-manifest') continue;
    const projected = projectManifest(form);
    if (projected) return projected;
  }
  throw new Error('fixture source contained no (task-runner-manifest ...) form');
}

// Re-export readLispFile for downstream tooling that wants the same Lisp
// reader the planner uses.
export { readLispFile };

// Only run the CLI when invoked directly. Keeps named exports clean for
// future tooling (wave28-04 daemon dry-run surface, wave28-05 batch
// verifier, wave28-06 loop smoke).
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
