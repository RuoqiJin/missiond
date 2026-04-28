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
  node scripts/plan-task-runner.mjs --manifest <manifest.lisp> [--json|--lisp] [--dry-fixture]

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

NO dispatch, NO spawn, NO git, NO network, NO LLM. Plan-only.

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const PLAN_SCHEMA = 'missiond.task-runner-plan.v0';

// Re-export from wave28-01 so downstream tooling has a single import surface.
export { MANIFEST_SCHEMA, VERIFICATION_TIERS, OVERLAP_POLICIES };

function main() {
  const args = process.argv.slice(2);
  let manifestPath = null;
  let format = 'json';
  let dryFixture = false;

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
    } else {
      console.error(`unknown argument: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
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
    result = planFromManifestFile(resolvedManifest, { repoRoot: cwd });
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
  });
}

// Plan from an already-projected manifest object. Mostly useful for
// fixtures and for in-memory pipelines where the manifest is parsed but
// never written to disk.
export function planFromManifestObject(manifest, opts = {}) {
  const manifestPath = opts.manifest_path ?? '<memory>';
  const checkOnDisk = opts.checkTaskContractsOnDisk === true;
  const repoRoot = opts.repoRoot ?? process.cwd();

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
  const plan = sortKeys({
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
  });

  return { ok: true, plan };
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
          const key = `${group} ${a} ${b}`;
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
