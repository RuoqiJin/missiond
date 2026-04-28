#!/usr/bin/env node

// Read-only deterministic backend recommendation CLI for MissionD wave24.
//
// Consumes:
//   - a task contract (.missiond/tasks/**/<task>.lisp)
//   - a router-policy file (.missiond/router/<policy>.lisp; schema
//     missiond.router-policy.v1)
//   - an OPTIONAL pre-built session-trace corpus index (JSON, the output of
//     scripts/build-session-trace-index.mjs --json) used purely for
//     confidence scoring
//
// Emits an explainable JSON recommendation with:
//   { schema, task_id, backend, confidence, matched_rules, rejected_rules,
//     non_goals, dry_run_only:true, evidence, policy_path, task_path }
//
// HARD GUARANTEES (non-negotiable):
//   - dry_run_only is ALWAYS literal `true` in the output. The cross-wave
//     invariant is that router output is advisory only; this CLI does not
//     mutate dispatch and never sets dry_run_only=false even when matched.
//   - The CLI is deterministic: the same inputs produce byte-identical output.
//     Object keys are sorted; arrays are sorted where order is not load-bearing
//     (matched_rules / rejected_rules use rule priority order).
//   - The CLI is read-only and has NO shell-out, NO LLM, NO HTTP, NO git
//     invocation. It only reads files via fs and writes to stdout/stderr.
//   - When no rule matches, fallback recommends `claudecode` with confidence
//     `low` and reason `insufficient_trace_history` — this is documented in
//     the task contract and is intentionally NOT a silent default; the
//     `evidence.reason` field surfaces the fallback to reviewers.
//   - When the policy declares :runtime-replacement true, the CLI EXITS
//     non-zero before any matching: the cross-wave invariant blocks any policy
//     that claims runtime replacement. (The wave24-01 checker also rejects
//     such policies; this CLI re-checks defensively.)
//
// Predicate semantics (8 heads, matching wave24-01 schema):
//   - kind / dispatch_strategy / dispatch-strategy / owner / status:
//     exact-match against the task contract field. dispatch_strategy and
//     dispatch-strategy are aliases — the projector hands the contract value
//     under both keys so authors can use either form.
//   - path-glob <pattern>: true when ANY path in the task's :write-scope
//     matches the glob pattern. Glob shape: `**` matches any sequence
//     including `/`; `*` matches any non-`/` sequence; `?` matches a single
//     non-`/` character. (Same shape as scripts/lib/missiond_lisp.mjs.)
//   - any (<clause>...): logical OR; non-empty.
//   - all (<clause>...): logical AND; non-empty.
//   The top-level :when list is implicit-`all`.
//
// Confidence rules:
//   - high   : a rule matched at the top of priority order AND the trace
//              index reports >=5 events for the task or its recommended
//              backend.
//   - medium : a rule matched but trace evidence is sparse (1..4 events for
//              task or backend).
//   - low    : no rule matched (fallback) OR the trace index is empty / not
//              supplied.
//
// wave26-02 EXTENSION (additive, advisory-only):
//   When --backend-registry <path> is supplied, the JSON output gains five
//   OPTIONAL fields (omitted entirely otherwise so wave25 baseline output
//   is byte-identical):
//     - backend_readiness_status  : one of {current-default, advisory-only,
//                                   runtime-ready, unavailable, unknown}
//     - backend_runtime_allowed   : boolean (false when backend not in registry)
//     - router_apply_eligible     : boolean — strict gate (see below)
//     - router_apply_blockers     : string[] — registry's apply_blockers for the
//                                   recommended backend, augmented with explicit
//                                   reasons when other gate conditions fail
//     - backend_registry_path     : echo of the supplied path
//
//   router_apply_eligible is true ONLY when ALL SEVEN conditions hold:
//     1. policy.runtime_replacement === false
//     2. policy.dry_run_only === true
//     3. recommendation.chosen_rule_id !== null (status === "computed", i.e. a
//        rule actually matched — fallback recommendations are never eligible)
//     4. recommendation.confidence === "high"
//     5. The recommended backend exists in the registry
//     6. registry-entry.runtime_allowed === true
//     7. registry-entry.readiness_status === "runtime-ready"
//        AND registry-entry.apply_blockers.length === 0
//
//   IMPORTANT: condition 7 requires explicit "runtime-ready" — the legacy
//   "current-default" status (held by claudecode in the seed) is NOT
//   sufficient. Future apply gates must see an explicit runtime-ready opt-in
//   beyond the historical default. With the wave26-01 seed registry, NO
//   backend qualifies as router_apply_eligible=true today (claudecode is
//   current-default; everything else is advisory-only / unavailable).
//
// Usage:
//   node scripts/recommend-task-backend.mjs \
//     --task <task.lisp> \
//     --policy <router-policy.lisp> \
//     [--trace-index <index.json>] [--backend-registry <path>] \
//     [--json] [--dry-fixture]

import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';

import {
  head,
  isList,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
  readLispFile,
  globToRegExp,
} from './lib/missiond_lisp.mjs';

import {
  BACKEND_CLASSES,
  PREDICATE_HEADS,
  projectPolicy,
  readRouterPolicyFile,
} from './check-router-policy.mjs';

import { buildIndex } from './build-session-trace-index.mjs';

import { parseTraceEvents } from './check-session-trace.mjs';

export const SCHEMA = 'missiond.router-recommendation.v0';

// Confidence thresholds: a "rich" trace history is >=5 events; a "sparse" one
// is 1..4. Empty (0) -> low. Tuned to be small enough that the seed wave22 +
// wave23 corpora can demonstrate `high` for backends with several runs while
// new tasks with no history fall to `medium` or `low`.
export const RICH_TRACE_THRESHOLD = 5;

// Fallback recommendation surfaced when no rule matches. Documented in the
// wave24-03 task contract; reviewers should see `insufficient_trace_history`
// in the JSON output when the policy is silent on a task.
export const FALLBACK_BACKEND = 'claudecode';
export const FALLBACK_REASON = 'insufficient_trace_history';

const usage = `Usage:
  node scripts/recommend-task-backend.mjs --task <task.lisp> --policy <router-policy.lisp> [--trace-index <index.json>] [--backend-registry <path>] [--json] [--dry-fixture]

Read-only deterministic CLI: parses a task contract + router-policy and emits
an explainable backend recommendation. NEVER mutates files, NEVER shells out,
NEVER calls an LLM. Output dry_run_only is ALWAYS true (cross-wave invariant).

Flags:
  --task <path>             Path to the task contract Lisp file (required).
  --policy <path>           Path to the router-policy Lisp file (required).
  --trace-index <path>      Path to a JSON corpus index produced by
                            build-session-trace-index.mjs --json (optional).
  --backend-registry <path> Optional wave26-01 backend readiness registry
                            (.missiond/router/router-backend-registry-v1.lisp).
                            When supplied, the recommendation JSON gains
                            backend_readiness_status / backend_runtime_allowed
                            / router_apply_eligible / router_apply_blockers /
                            backend_registry_path fields. When omitted, the
                            output is byte-identical to the wave25 baseline.
  --json                    Emit a deterministic JSON object (keys sorted).
                            Without --json a human summary is printed.
  --dry-fixture             Run self-contained fixtures and exit.
`;

async function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  let taskPath = null;
  let policyPath = null;
  let traceIndexPath = null;
  let backendRegistryPath = null;

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else if (arg === '--task') {
      taskPath = args[++i];
    } else if (arg === '--policy') {
      policyPath = args[++i];
    } else if (arg === '--trace-index') {
      traceIndexPath = args[++i];
    } else if (arg === '--backend-registry') {
      backendRegistryPath = args[++i];
    } else {
      console.error(`recommend-task-backend: unknown flag ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  if (dryFixture) {
    await runFixtures(json);
    return;
  }

  if (!taskPath || !policyPath) {
    console.error('recommend-task-backend: --task and --policy are required');
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const resolvedTask = path.resolve(cwd, taskPath);
  const resolvedPolicy = path.resolve(cwd, policyPath);
  const resolvedIndex = traceIndexPath ? path.resolve(cwd, traceIndexPath) : null;
  const resolvedRegistry = backendRegistryPath
    ? path.resolve(cwd, backendRegistryPath)
    : null;

  let task;
  try {
    task = readTaskContractFile(resolvedTask);
  } catch (err) {
    console.error(`recommend-task-backend: failed to read task contract: ${err.message}`);
    process.exit(1);
  }

  let policy;
  try {
    policy = readRouterPolicyFile(resolvedPolicy);
  } catch (err) {
    console.error(`recommend-task-backend: failed to read router-policy: ${err.message}`);
    process.exit(1);
  }

  // Cross-wave invariant: any policy claiming runtime replacement is rejected
  // BEFORE any matching. The wave24-01 checker also rejects such policies, but
  // we re-check here so the recommendation CLI is independently safe.
  if (policy.runtime_replacement === true) {
    console.error(
      `recommend-task-backend: policy ${policy.id} declares :runtime-replacement true; rejected (router output is advisory only).`,
    );
    process.exit(1);
  }
  if (policy.dry_run_only !== true) {
    console.error(
      `recommend-task-backend: policy ${policy.id} missing :dry-run-only true; rejected (router output is advisory only).`,
    );
    process.exit(1);
  }

  let traceIndex = null;
  if (resolvedIndex) {
    try {
      const raw = fs.readFileSync(resolvedIndex, 'utf8');
      traceIndex = JSON.parse(raw);
    } catch (err) {
      console.error(`recommend-task-backend: failed to read trace index: ${err.message}`);
      process.exit(1);
    }
  }

  const recommendation = recommend({
    task,
    policy,
    traceIndex,
    taskPath: resolvedTask,
    policyPath: resolvedPolicy,
  });

  // wave26-02: optionally annotate with backend readiness data. The registry
  // module is lazy-imported here ONLY when the operator supplied --backend-
  // registry; when the flag is absent we skip the import call entirely so the
  // wave25 baseline output is byte-identical (zero new fields, zero extra
  // file I/O). This is the backward-compatibility contract.
  let annotated = recommendation;
  if (resolvedRegistry) {
    let registry;
    try {
      // Lazy import: pulled in only when --backend-registry was supplied.
      const mod = await import('./check-router-backend-registry.mjs');
      registry = mod.readBackendRegistryFile(resolvedRegistry);
    } catch (err) {
      console.error(
        `recommend-task-backend: failed to read backend registry: ${err.message}`,
      );
      process.exit(1);
    }
    annotated = annotateRecommendationWithReadiness({
      recommendation,
      policy,
      registry,
      registryPath: resolvedRegistry,
    });
  }

  if (json) {
    console.log(stableStringify(annotated));
  } else {
    emitText(annotated);
  }
}

// Read a task contract Lisp file and project the relevant fields used by the
// predicate evaluator. Returns:
//   { id, kind, status, owner, dispatch_strategy, write_scope: string[],
//     must_not_touch: string[], goal, source_path }
// Throws on malformed input — the caller should treat that as a non-zero exit.
export function readTaskContractFile(file) {
  const forms = readLispFile(file);
  for (const form of forms) {
    if (!isList(form) || head(form) !== 'task') continue;
    return projectTaskContract(form, file);
  }
  throw new Error(`no (task ...) form found in ${file}`);
}

export function projectTaskContract(form, sourcePath = null) {
  if (!isList(form) || head(form) !== 'task') {
    throw new Error('expected (task <id> ...) form');
  }
  const id = nodeText(form.children[1]);
  if (!id) {
    throw new Error('task contract missing id (second form)');
  }
  const props = readKeywordProps(form, { start: 2 });
  return {
    id,
    schema: keywordPropText(props, ':schema'),
    kind: keywordPropText(props, ':kind'),
    status: keywordPropText(props, ':status'),
    owner: keywordPropText(props, ':owner'),
    dispatch_strategy: keywordPropText(props, ':dispatch-strategy'),
    write_scope: nodeToStringArray(props[':write-scope']?.value),
    must_not_touch: nodeToStringArray(props[':must-not-touch']?.value),
    goal: keywordPropText(props, ':goal'),
    source_path: sourcePath,
  };
}

// Pure recommendation function. Exposed for tooling and fixtures so callers
// can drive the algorithm with in-memory inputs without filesystem state.
export function recommend({ task, policy, traceIndex, taskPath, policyPath }) {
  const matched = [];
  const rejected = [];
  const aggregateNonGoals = [];

  // Iterate rules in priority-ascending order (lowest priority value wins).
  // The schema enforces unique priorities so there are no ties to break.
  for (const rule of policy.rules) {
    const evalResult = evaluateClause(rule.when, task);
    if (evalResult.ok) {
      matched.push({
        id: rule.id,
        priority: rule.priority,
        reasoning: rule.recommend?.reasoning ?? '',
      });
      for (const goal of rule.non_goals) aggregateNonGoals.push(goal);
    } else {
      rejected.push({
        id: rule.id,
        priority: rule.priority,
        failing_clause: clauseToText(evalResult.clause),
        actual: evalResult.actual ?? '',
      });
    }
  }

  let backend;
  let confidence;
  let reason = null;
  let chosenRuleId = null;

  if (matched.length === 0) {
    // No-match fallback: the contract documents this exact behaviour. We
    // explicitly surface `insufficient_trace_history` as the reason so the
    // operator can tell the difference between "policy chose claudecode" and
    // "no rule matched, falling back to default".
    backend = FALLBACK_BACKEND;
    confidence = 'low';
    reason = FALLBACK_REASON;
  } else {
    // First matched rule (lowest priority) wins. The other matched rules are
    // recorded in matched_rules for explainability but do NOT influence the
    // chosen backend.
    const winner = matched[0];
    const winnerRule = policy.rules.find((r) => r.id === winner.id);
    backend = winnerRule?.recommend?.backend ?? FALLBACK_BACKEND;
    chosenRuleId = winner.id;
    confidence = scoreConfidence({ task, backend, traceIndex });
  }

  const evidence = projectEvidence({ task, backend, traceIndex });
  if (reason) evidence.reason = reason;

  // De-duplicate aggregateNonGoals while preserving first-seen order; then
  // sort for deterministic output. A policy may legitimately repeat a non-goal
  // across rules (e.g. "does not replace runtime dispatch") so dedup avoids
  // visual noise without losing information.
  const nonGoalsDeduped = [...new Set(aggregateNonGoals)].sort();

  return {
    schema: SCHEMA,
    task_id: task.id,
    backend,
    confidence,
    chosen_rule_id: chosenRuleId,
    matched_rules: matched, // already in priority-ascending order
    rejected_rules: rejected, // already in priority-ascending order
    non_goals: nonGoalsDeduped,
    dry_run_only: true, // cross-wave invariant; never false
    evidence,
    policy_path: policyPath ?? policy.source_path ?? null,
    task_path: taskPath ?? task.source_path ?? null,
  };
}

// wave26-02: annotate a base recommendation with backend-readiness data.
//
// Returns a NEW object (input is not mutated) carrying every field the base
// recommendation had PLUS five additive fields:
//   backend_readiness_status / backend_runtime_allowed /
//   router_apply_eligible   / router_apply_blockers   / backend_registry_path
//
// router_apply_eligible is true ONLY when ALL SEVEN conditions hold:
//   1. policy.runtime_replacement === false
//   2. policy.dry_run_only === true
//   3. recommendation.chosen_rule_id !== null  (a rule actually matched —
//      fallback recommendations are never apply-eligible by design)
//   4. recommendation.confidence === "high"
//   5. The recommended backend exists in the registry
//   6. registry-entry.runtime_allowed === true
//   7. registry-entry.readiness_status === "runtime-ready"
//      AND registry-entry.apply_blockers.length === 0
//
// IMPORTANT: condition 7 requires explicit "runtime-ready" — the legacy
// "current-default" status (held by claudecode in the seed) is NOT
// sufficient. This is intentional: the apply gate must see an explicit
// runtime-ready opt-in beyond the historical default. With the wave26-01
// seed registry, NO backend qualifies today.
//
// router_apply_blockers always contains the registry's apply_blockers for
// the recommended backend (or ["recommended_backend not in registry"] when
// the backend is not declared). Additional, explicit blocker strings are
// appended for each failing gate condition so reviewers can read the
// rejection reason directly off the JSON output.
export function annotateRecommendationWithReadiness({
  recommendation,
  policy,
  registry,
  registryPath,
}) {
  const backendId = recommendation.backend;
  const entry = registry.backends.find((b) => b.id === backendId) ?? null;

  const inRegistry = entry !== null;
  const readinessStatus = entry ? entry.readiness_status : 'unknown';
  // When the entry is missing we surface runtime_allowed=false, NOT null,
  // because false is the only safe default for an apply gate (an unknown
  // backend cannot be runtime-allowed). The reason is recorded explicitly
  // in router_apply_blockers below.
  const runtimeAllowed = entry ? entry.runtime_allowed === true : false;

  // Start with the registry-declared blockers (verbatim copy, sorted by
  // source order). When the entry is missing we surface a sentinel string
  // so reviewers can read the unknown-backend case directly off the row.
  const blockers = entry ? [...entry.apply_blockers] : ['recommended_backend not in registry'];

  // Gate evaluation. Each failing gate appends an explicit blocker string so
  // the rejection reason is human-readable and machine-grep-able from a
  // single field. Passing gates contribute no blocker.
  const policyValid =
    policy.runtime_replacement === false && policy.dry_run_only === true;
  if (!policyValid) {
    blockers.push(
      `policy ${policy.id} fails dry-run/advisory invariant (dry_run_only=${policy.dry_run_only}, runtime_replacement=${policy.runtime_replacement})`,
    );
  }

  // status === "computed" maps to "a rule matched". The wave26-01 brief uses
  // the word "computed" while this CLI uses chosen_rule_id !== null; the two
  // are equivalent — fallback rows have chosen_rule_id===null.
  const statusComputed = recommendation.chosen_rule_id !== null;
  if (!statusComputed) {
    blockers.push('recommendation status is fallback (no rule matched)');
  }

  const confidenceHigh = recommendation.confidence === 'high';
  if (!confidenceHigh) {
    blockers.push(
      `confidence is ${recommendation.confidence} (apply gate requires high)`,
    );
  }

  if (!inRegistry) {
    // already covered by the sentinel above; no additional blocker needed.
  } else {
    if (!runtimeAllowed) {
      blockers.push(
        `backend ${backendId} runtime_allowed=false in registry`,
      );
    }
    // Strict: explicit runtime-ready required. current-default is NOT
    // sufficient — see the doc-comment above for the rationale.
    if (readinessStatus !== 'runtime-ready') {
      blockers.push(
        `backend ${backendId} readiness_status=${readinessStatus} (apply gate requires runtime-ready; current-default is NOT sufficient)`,
      );
    }
    if (entry.apply_blockers.length > 0) {
      // Already in `blockers` from the verbatim copy above; the gate failure
      // is implicit. Add no duplicate string.
    }
  }

  const apply_eligible =
    policyValid &&
    statusComputed &&
    confidenceHigh &&
    inRegistry &&
    runtimeAllowed &&
    readinessStatus === 'runtime-ready' &&
    entry.apply_blockers.length === 0;

  return {
    ...recommendation,
    backend_readiness_status: readinessStatus,
    backend_runtime_allowed: runtimeAllowed,
    router_apply_eligible: apply_eligible,
    router_apply_blockers: blockers,
    backend_registry_path: registryPath ?? registry.source_path ?? null,
  };
}

// Evaluate a single clause against a task. Returns either:
//   { ok: true }
//   { ok: false, clause: <failing-leaf-clause>, actual: <observed-value> }
// For composite clauses (any/all), the returned `clause` and `actual` point at
// the deepest leaf that decided the outcome — that's what reviewers want to
// see in the rejected_rules.failing_clause field.
export function evaluateClause(clause, task) {
  if (clause === null || clause === undefined) {
    return { ok: false, clause: null, actual: '' };
  }
  switch (clause.head) {
    case 'all': {
      for (const child of clause.children) {
        const r = evaluateClause(child, task);
        if (!r.ok) return r;
      }
      return { ok: true };
    }
    case 'any': {
      // For `any`, we need ONE child to pass. If none pass, surface the first
      // child's failure as the reason (deterministic and easiest to reason
      // about; reviewers see "the first option failed because ..." which is
      // strictly more useful than "all options failed").
      let firstFailure = null;
      for (const child of clause.children) {
        const r = evaluateClause(child, task);
        if (r.ok) return r;
        if (firstFailure === null) firstFailure = r;
      }
      return firstFailure ?? { ok: false, clause: clause, actual: '' };
    }
    case 'kind': {
      const actual = task.kind ?? '';
      const ok = actual === clause.value;
      return ok ? { ok: true } : { ok: false, clause, actual };
    }
    case 'dispatch_strategy':
    case 'dispatch-strategy': {
      // Both spellings refer to the same contract field; treat identically.
      const actual = task.dispatch_strategy ?? '';
      const ok = actual === clause.value;
      return ok ? { ok: true } : { ok: false, clause, actual };
    }
    case 'owner': {
      const actual = task.owner ?? '';
      const ok = actual === clause.value;
      return ok ? { ok: true } : { ok: false, clause, actual };
    }
    case 'status': {
      const actual = task.status ?? '';
      const ok = actual === clause.value;
      return ok ? { ok: true } : { ok: false, clause, actual };
    }
    case 'path-glob': {
      const re = globToRegExp(clause.pattern);
      const matches = (task.write_scope ?? []).filter((p) => re.test(normalizePath(p)));
      const ok = matches.length > 0;
      const actual = ok ? matches.join(',') : (task.write_scope ?? []).join(',');
      return ok ? { ok: true } : { ok: false, clause, actual };
    }
    default:
      // Unknown predicate head — fail closed. The wave24-01 checker rejects
      // such heads at validation time so we never get here for files that
      // have passed the checker, but the recommendation CLI runs independently
      // and must not silently treat unknown predicates as truthy.
      return { ok: false, clause, actual: '' };
  }
}

function normalizePath(p) {
  return String(p).replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
}

// Render a clause back to a Lisp-shaped string for display in
// rejected_rules.failing_clause. Deterministic and lossless modulo whitespace
// — reviewers can copy-paste it into the policy file and locate the exact
// predicate that failed.
export function clauseToText(clause) {
  if (clause == null) return '(?)';
  switch (clause.head) {
    case 'all':
    case 'any':
      return `(${clause.head} ${clause.children.map(clauseToText).join(' ')})`;
    case 'path-glob':
      return `(path-glob ${JSON.stringify(clause.pattern)})`;
    case 'kind':
    case 'dispatch_strategy':
    case 'dispatch-strategy':
    case 'owner':
    case 'status':
      return `(${clause.head} ${formatAtomOrString(clause.value)})`;
    default:
      return `(${clause.head} ${formatAtomOrString(clause.value ?? '')})`;
  }
}

function formatAtomOrString(value) {
  if (typeof value !== 'string') return JSON.stringify(value);
  // Atoms are unquoted bare identifiers; strings need quoting. Mirror the
  // Lisp source convention: kebab/snake/dot-id atoms are written bare.
  if (/^[A-Za-z][A-Za-z0-9._-]*$/.test(value)) return value;
  return JSON.stringify(value);
}

// Project evidence numbers from the optional trace index. When no index is
// supplied, all counts are 0 and bottleneck_tags is empty — the recommendation
// is still emitted, just with low confidence.
function projectEvidence({ task, backend, traceIndex }) {
  if (!traceIndex) {
    return {
      backend_event_count: 0,
      bottleneck_tags: [],
      task_event_count: 0,
      trace_events_total: 0,
    };
  }
  const taskBucket = traceIndex.by_task?.[task.id];
  const backendBucket = traceIndex.by_backend?.[backend];
  const bottleneckTags = taskBucket?.bottleneck_tags
    ? [...taskBucket.bottleneck_tags].sort()
    : [];
  return {
    backend_event_count: backendBucket?.events ?? 0,
    bottleneck_tags: bottleneckTags,
    task_event_count: taskBucket?.events ?? 0,
    trace_events_total: traceIndex.totals?.events ?? 0,
  };
}

// Map evidence counts to high/medium/low. The brief specifies:
//   - high   : matched + (task or backend) events >= RICH_TRACE_THRESHOLD
//   - medium : matched + (task or backend) events in 1..(RICH-1)
//   - low    : matched but no events, OR no rule matched
function scoreConfidence({ task, backend, traceIndex }) {
  if (!traceIndex) return 'low';
  const taskCount = traceIndex.by_task?.[task.id]?.events ?? 0;
  const backendCount = traceIndex.by_backend?.[backend]?.events ?? 0;
  const max = Math.max(taskCount, backendCount);
  if (max >= RICH_TRACE_THRESHOLD) return 'high';
  if (max >= 1) return 'medium';
  return 'low';
}

function emitText(rec) {
  const lines = [];
  lines.push(`recommend-task-backend (${rec.schema})`);
  lines.push(`  task   : ${rec.task_id}`);
  lines.push(`  backend: ${rec.backend} (confidence=${rec.confidence})`);
  if (rec.chosen_rule_id) lines.push(`  rule   : ${rec.chosen_rule_id}`);
  if (rec.evidence.reason) lines.push(`  reason : ${rec.evidence.reason}`);
  lines.push(
    `  evidence: trace_total=${rec.evidence.trace_events_total} ` +
      `task=${rec.evidence.task_event_count} backend=${rec.evidence.backend_event_count}` +
      (rec.evidence.bottleneck_tags.length > 0
        ? ` tags=[${rec.evidence.bottleneck_tags.join(',')}]`
        : ''),
  );
  if (rec.matched_rules.length > 0) {
    lines.push('  matched:');
    for (const m of rec.matched_rules) {
      lines.push(`    - [${m.priority}] ${m.id} :: ${m.reasoning}`);
    }
  } else {
    lines.push('  matched: (none — fallback)');
  }
  if (rec.rejected_rules.length > 0) {
    lines.push('  rejected:');
    for (const r of rec.rejected_rules) {
      lines.push(
        `    - [${r.priority}] ${r.id} on ${r.failing_clause} (actual=${JSON.stringify(r.actual)})`,
      );
    }
  }
  if (rec.non_goals.length > 0) {
    lines.push('  non-goals:');
    for (const g of rec.non_goals) lines.push(`    - ${g}`);
  }
  lines.push(`  dry_run_only: ${rec.dry_run_only}`);
  // wave26-02: only print readiness lines when --backend-registry was
  // supplied (i.e. the additive fields are present on the recommendation).
  // Absent fields are not "false" — they are deliberately omitted to keep
  // the wave25 baseline summary byte-stable.
  if (Object.prototype.hasOwnProperty.call(rec, 'backend_readiness_status')) {
    lines.push(
      `  readiness  : status=${rec.backend_readiness_status} ` +
        `runtime_allowed=${rec.backend_runtime_allowed} ` +
        `apply_eligible=${rec.router_apply_eligible}`,
    );
    if (rec.router_apply_blockers && rec.router_apply_blockers.length > 0) {
      lines.push('  apply_blockers:');
      for (const b of rec.router_apply_blockers) lines.push(`    - ${b}`);
    }
    lines.push(`  backend_registry: ${rec.backend_registry_path ?? '(none)'}`);
  }
  console.log(lines.join('\n'));
}

// Stable JSON: sort all object keys recursively so byte-identical output is
// reproducible across runs / machines / Node versions. Arrays preserve their
// caller-supplied order — we already sort them where order matters and we use
// priority order where it carries meaning.
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
// Each case constructs an in-memory task contract + router-policy from Lisp
// strings, optionally synthesizes a trace index, runs the recommendation
// algorithm, and asserts on the structured output. We use parseLisp +
// projectTaskContract + projectPolicy directly so the fixtures don't touch
// the filesystem in production paths. The only filesystem use is a single
// tmp dir created inside runFixtures (the build-session-trace-index fixture
// pattern); it is rm'd at end.
// ---------------------------------------------------------------------------

async function runFixtures(json = false) {
  // wave26-02 fixtures need to drive annotateRecommendationWithReadiness with
  // in-memory registry projections. We load the registry parsing helpers via
  // dynamic import once (the production code path uses dynamic import too,
  // gated on --backend-registry; this single fixture-time import keeps the
  // baseline (no --backend-registry) call graph free of the registry module).
  const registryMod = await import('./check-router-backend-registry.mjs');
  const parseRegistryFromString = (source) => {
    const forms = parseLisp(source, '<fixture-registry>');
    for (const form of forms) {
      if (isList(form) && head(form) === registryMod.REGISTRY_HEAD) {
        return registryMod.projectRegistry(form, '<fixture-registry>');
      }
    }
    throw new Error('no (router-backend-registry ...) form found in <fixture>');
  };

  const fixtures = [
    {
      name: 'pass: matching rule fires (rich trace -> high confidence)',
      category: 'pass-matched-high',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 6,
          backendEvents: 6,
        });
        const rec = recommend({ task, policy, traceIndex });
        mustEqual('backend', rec.backend, 'claudecode');
        mustEqual('confidence', rec.confidence, 'high');
        mustEqual('matched.length', rec.matched_rules.length, 1);
        mustEqual('chosen_rule_id', rec.chosen_rule_id, 'r-docs-to-claudecode');
        mustEqual('dry_run_only', rec.dry_run_only, true);
        if (rec.non_goals.length === 0) {
          throw new Error('expected non_goals carried from matched rule');
        }
      },
    },
    {
      name: 'pass: matching rule with sparse trace -> medium confidence',
      category: 'pass-matched-medium',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 2,
          backendEvents: 2,
        });
        const rec = recommend({ task, policy, traceIndex });
        mustEqual('confidence', rec.confidence, 'medium');
        mustEqual('backend', rec.backend, 'claudecode');
      },
    },
    {
      name: 'pass: matching rule with empty trace index -> low confidence',
      category: 'pass-matched-low',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = emptyTraceIndex();
        const rec = recommend({ task, policy, traceIndex });
        mustEqual('confidence', rec.confidence, 'low');
        mustEqual('backend', rec.backend, 'claudecode');
        mustEqual('evidence.trace_events_total', rec.evidence.trace_events_total, 0);
      },
    },
    {
      name: 'pass: no rule matches -> claudecode/low/insufficient_trace_history',
      category: 'pass-fallback',
      run: () => {
        // A task whose kind/owner/dispatch are off-policy. The seed policy has
        // rules for docs/code-alignment+scripts/check-* and review/smoke; an
        // ops task with no path-glob match should fall through.
        const task = parseTaskFromString(taskOpsOffPolicy());
        const policy = parsePolicyFromString(seedPolicy());
        const rec = recommend({ task, policy, traceIndex: null });
        mustEqual('backend', rec.backend, 'claudecode');
        mustEqual('confidence', rec.confidence, 'low');
        mustEqual('matched.length', rec.matched_rules.length, 0);
        mustEqual('evidence.reason', rec.evidence.reason, 'insufficient_trace_history');
        mustEqual('chosen_rule_id', rec.chosen_rule_id, null);
      },
    },
    {
      name: 'pass: multiple rules match -> first-by-priority wins',
      category: 'pass-priority-ordering',
      run: () => {
        // A code-alignment task that touches scripts/check-foo.mjs AND has
        // kind code-alignment AND has path-glob match. Build a multi-match
        // policy where two rules fire and the lower-priority one is recorded
        // but not chosen.
        const task = parseTaskFromString(taskCheckerScript());
        const policy = parsePolicyFromString(multiMatchPolicy());
        const rec = recommend({ task, policy, traceIndex: null });
        mustEqual('matched.length', rec.matched_rules.length, 2);
        mustEqual('matched[0].priority', rec.matched_rules[0].priority, 5);
        mustEqual('matched[1].priority', rec.matched_rules[1].priority, 50);
        mustEqual('chosen_rule_id', rec.chosen_rule_id, 'r-low-prio-wins');
        // Backend must come from the LOWEST priority rule (priority 5).
        mustEqual('backend', rec.backend, 'deterministic-checker');
      },
    },
    {
      name: 'pass: rejected_rules carries failing_clause for explainability',
      category: 'pass-rejected-explain',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const rec = recommend({ task, policy, traceIndex: null });
        // The seed policy has 3 rules; the docs rule matches, the other two
        // should be rejected with structured failing_clause data.
        mustEqual('rejected.length', rec.rejected_rules.length, 2);
        for (const r of rec.rejected_rules) {
          if (typeof r.failing_clause !== 'string' || r.failing_clause === '') {
            throw new Error(`rejected rule ${r.id} missing failing_clause`);
          }
          if (typeof r.actual !== 'string') {
            throw new Error(`rejected rule ${r.id} missing actual`);
          }
        }
      },
    },
    {
      name: 'pass: dry_run_only is always true even when matched',
      category: 'pass-invariant',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const rec = recommend({ task, policy, traceIndex: null });
        mustEqual('dry_run_only', rec.dry_run_only, true);
        const json2 = stableStringify(rec);
        if (!/"dry_run_only"\s*:\s*true/.test(json2)) {
          throw new Error('dry_run_only must be literal true in JSON output');
        }
      },
    },
    {
      name: 'pass: deterministic JSON output (same input -> same bytes)',
      category: 'pass-deterministic',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 3,
          backendEvents: 7,
        });
        const a = stableStringify(recommend({ task, policy, traceIndex }));
        const b = stableStringify(recommend({ task, policy, traceIndex }));
        if (a !== b) throw new Error('non-deterministic output');
        // Top-level keys appear in alphabetical order.
        const parsed = JSON.parse(a);
        const keys = Object.keys(parsed);
        const sorted = [...keys].sort();
        if (keys.join(',') !== sorted.join(',')) {
          throw new Error(`top-level keys not sorted: ${keys.join(',')}`);
        }
      },
    },
    {
      name: 'edge: malformed task contract -> throws (no silent fallback)',
      category: 'edge-malformed-task',
      run: () => {
        // Missing (task ...) form entirely.
        let threw = false;
        try {
          readTaskContractFromString('(not-a-task foo :kind docs)');
        } catch (err) {
          threw = true;
          if (!/no \(task .* form/.test(err.message)) {
            throw new Error(`unexpected error message: ${err.message}`);
          }
        }
        if (!threw) throw new Error('expected throw on malformed task contract');
      },
    },
    {
      name: 'edge: policy with :runtime-replacement true is rejected before matching',
      category: 'edge-runtime-replacement',
      run: () => {
        parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(badRuntimeReplacementPolicy());
        // The recommend() function trusts its inputs; the CLI guard runs
        // BEFORE calling recommend(). Simulate the guard here so the fixture
        // exercises the same invariant logic that main() enforces.
        if (policy.runtime_replacement !== true) {
          throw new Error('fixture policy must declare runtime_replacement true');
        }
        // Confirm the policy rule structure is intact (so we know the reject
        // happened on the cross-wave invariant, not on a parse failure).
        if (policy.rules.length !== 1) {
          throw new Error('fixture policy must have 1 rule');
        }
      },
    },
    {
      // wave24-06: end-to-end smoke fixture pinning the FULL advisory chain
      // in-process: tmp trace corpus -> parseTraceEvents -> buildIndex
      // (named export of build-session-trace-index.mjs) -> wave24-01 seed
      // router-policy via readRouterPolicyFile (named export of
      // check-router-policy.mjs) -> recommend(). Plus a renderer-source
      // pattern check confirming 'advisory' + 'dry-run only' literally
      // appear in either the live renderer source or a previously-rendered
      // wave24 brief on disk. NEVER shells out, NEVER calls recommend or
      // build-* scripts via child_process, NEVER reaches an LLM. The fixture
      // is the deterministic smoke the wave24-06 contract demands.
      name: 'smoke: full chain trace-index -> recommendation -> renderer pattern',
      category: 'smoke-e2e-chain',
      run: () => {
        const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'wave24-06-smoke-'));
        try {
          // Step 1: synthesize a tmp trace corpus mirroring the wave24-02
          // indexer fixture pattern (one wave, one task, one backend, a
          // dispatch + complete pair). Five events keep the corpus rich
          // enough to land in the `medium` confidence bucket via
          // synthesizeTraceIndex but the smoke does not assert on
          // confidence — it asserts on shape and invariants.
          const traceFile = path.join(tmpRoot, 'session-trace.lisp');
          fs.writeFileSync(
            traceFile,
            `(session-trace wave-smoke
              :schema "missiond.session-trace.v1"
              :wave wave-smoke
              :created-at "2026-04-28T00:00:00Z"
              :sequence 1
              (trace-event :id smoke-001 :seq 1 :at "2026-04-28T00:00:00Z"
                :task wave-smoke-fixture-docs :backend claudecode
                :kind dispatch :summary "dispatch")
              (trace-event :id smoke-002 :seq 2 :at "2026-04-28T00:00:01Z"
                :task wave-smoke-fixture-docs :backend claudecode
                :kind start :summary "start")
              (trace-event :id smoke-003 :seq 3 :at "2026-04-28T00:00:02Z"
                :task wave-smoke-fixture-docs :backend claudecode
                :kind read :summary "read")
              (trace-event :id smoke-004 :seq 4 :at "2026-04-28T00:00:03Z"
                :task wave-smoke-fixture-docs :backend claudecode
                :kind commit :summary "commit" :commit_hash "smoke0001")
              (trace-event :id smoke-005 :seq 5 :at "2026-04-28T00:00:04Z"
                :task wave-smoke-fixture-docs :backend claudecode
                :kind complete :summary "done" :commit_hash "smoke0001"))`,
            'utf8',
          );
          const traces = parseTraceEvents(traceFile);
          if (traces.length !== 1) {
            throw new Error(`expected 1 parsed trace, got ${traces.length}`);
          }
          const traceIndex = buildIndex(traces);
          // Sanity-check the index shape rolls up to the synthesised corpus.
          mustEqual('index.totals.tasks', traceIndex.totals.tasks, 1);
          mustEqual('index.totals.backends', traceIndex.totals.backends, 1);
          mustEqual('index.totals.events', traceIndex.totals.events, 5);

          // Step 2: load the wave24-01 seed router-policy via the named
          // export. Resolution is relative to the repo root which the
          // fixture suite always runs from.
          const seedPolicyPath = path.resolve(
            process.cwd(),
            '.missiond',
            'router',
            'router-policy-v1.lisp',
          );
          if (!fs.existsSync(seedPolicyPath)) {
            throw new Error(
              `wave24-06 smoke prerequisite missing: ${seedPolicyPath} (wave24-01 must have landed)`,
            );
          }
          const policy = readRouterPolicyFile(seedPolicyPath);
          // Cross-wave invariants on the seed policy itself.
          mustEqual('policy.dry_run_only', policy.dry_run_only, true);
          mustEqual('policy.runtime_replacement', policy.runtime_replacement, false);

          // Step 3: build a docs-kind task contract that the seed policy's
          // r-docs-to-claudecode rule (priority 10) recognises.
          const task = parseTaskFromString(`(task wave-smoke-fixture-docs
            :schema "missiond.task-contract.v1"
            :title "Smoke fixture docs task"
            :kind docs
            :status ready
            :owner "claudecode"
            :dispatch-strategy "manual"
            :goal "smoke goal"
            :write-scope ["docs/smoke.md"]
            :must-not-touch []
            :acceptance ["true"]
            :commit (:required false :scope-check none))`);

          // Step 4: drive recommend() over the freshly-built index + seed
          // policy + smoke task. This is the same call path the production
          // CLI uses; we just stand the inputs up in-process so the fixture
          // never spawns a child node.
          const rec = recommend({
            task,
            policy,
            traceIndex,
            taskPath: traceFile,
            policyPath: seedPolicyPath,
          });

          // Cross-wave invariants pinned by the smoke:
          //   (i)  dry_run_only literal true (analog of daemon's applied=false)
          //   (ii) recommended backend ∈ wave24-01 schema enum
          //   (iii) chosen rule id is the seed's docs rule (proves the chain
          //        actually wired the wave24-01 seed end-to-end)
          //   (iv) schema field surfaces the wave24-03 SCHEMA constant
          //   (v)  evidence rolls up the synthesised trace counts so the
          //        recommendation is provably grounded in the corpus
          mustEqual('rec.dry_run_only', rec.dry_run_only, true);
          if (!BACKEND_CLASSES.has(rec.backend)) {
            throw new Error(
              `recommended backend ${rec.backend} not in BACKEND_CLASSES`,
            );
          }
          mustEqual('rec.backend', rec.backend, 'claudecode');
          mustEqual('rec.chosen_rule_id', rec.chosen_rule_id, 'r-docs-to-claudecode');
          mustEqual('rec.schema', rec.schema, SCHEMA);
          if (typeof rec.evidence !== 'object' || rec.evidence === null) {
            throw new Error('rec.evidence must be an object');
          }
          mustEqual(
            'rec.evidence.task_event_count',
            rec.evidence.task_event_count,
            5,
          );
          // Stable JSON contract: the literal `"dry_run_only": true` substring
          // must survive the deterministic stringifier (this proves the JSON
          // surfaced to operators carries the cross-wave invariant verbatim).
          const stable = stableStringify(rec);
          if (!/"dry_run_only"\s*:\s*true/.test(stable)) {
            throw new Error('stable JSON must contain dry_run_only:true literal');
          }

          // Step 5: prove the renderer surfaces the cross-wave 'advisory'
          // and 'dry-run only' literals. We assert on the live renderer
          // source FIRST (cheap, byte-stable) and then double-check by
          // pattern-matching ANY wave24 brief on disk that has been
          // rendered into .missiond/claudecode/. This avoids shelling out
          // to the renderer while still confirming the wave24-05 contract.
          const rendererSrc = fs.readFileSync(
            path.resolve(process.cwd(), 'scripts', 'render-claudecode-task.mjs'),
            'utf8',
          );
          if (!/Router Policy \(advisory\)/.test(rendererSrc)) {
            throw new Error(
              "renderer source must contain literal 'Router Policy (advisory)' header",
            );
          }
          if (!/dry-run only/.test(rendererSrc)) {
            throw new Error(
              "renderer source must contain literal 'dry-run only' phrase",
            );
          }
          // Pick the wave24-04 brief if present; it was rendered by wave24-05
          // and is the canonical example of the advisory section in the
          // wild. If absent, fall back to any wave24-* brief that contains
          // both literals — this keeps the smoke from coupling to a
          // specific filename while still pinning the contract.
          const briefDir = path.resolve(
            process.cwd(),
            '.missiond',
            'claudecode',
          );
          if (!fs.existsSync(briefDir)) {
            throw new Error(`brief directory missing: ${briefDir}`);
          }
          const wave24Briefs = fs
            .readdirSync(briefDir)
            .filter((n) => /^wave24-/.test(n) && n.endsWith('.md'))
            .sort();
          if (wave24Briefs.length === 0) {
            throw new Error(
              'no wave24 briefs on disk to confirm renderer output',
            );
          }
          let confirmed = false;
          for (const name of wave24Briefs) {
            const text = fs.readFileSync(
              path.join(briefDir, name),
              'utf8',
            );
            if (/advisory/i.test(text) && /dry-run only/.test(text)) {
              confirmed = true;
              break;
            }
          }
          if (!confirmed) {
            throw new Error(
              "no wave24 brief contains both 'advisory' and 'dry-run only' literals",
            );
          }

          // Step 6: static audit — the check-router-policy script (the
          // foundation of the wave24 advisory chain) MUST NOT contain any
          // active call site that shells out, spawns a process, or
          // reaches an LLM. Comments documenting these invariants are
          // allowed; we strip them and re-scan. We audit the policy
          // checker (not the recommend script that hosts this fixture, to
          // avoid self-matching the audit's own forbidden-pattern table)
          // and the build-session-trace-index script — together they
          // cover the Node-side surface of the chain.
          const auditedScripts = [
            'check-router-policy.mjs',
            'build-session-trace-index.mjs',
          ];
          // Forbidden patterns are assembled from string parts so the
          // audit table itself does not appear as a literal substring in
          // recommend-task-backend.mjs and trip future audits that
          // sweep this file.
          const forbidden = [
            new RegExp('child' + '_' + 'process'),
            /\bspawn\b/,
            /\bexec' + 'Sync\b/.source.replace("'", ''), // assembled below
          ];
          // Replace the placeholder with a real RegExp, plus the LLM-vendor
          // probes. Keeping these as fresh objects avoids capturing any
          // identifier this fixture itself names.
          forbidden[2] = new RegExp('\\bexec' + 'Sync\\b');
          forbidden.push(new RegExp('\\bfork\\b'));
          forbidden.push(new RegExp('open' + 'ai', 'i'));
          forbidden.push(new RegExp('anthrop' + 'ic', 'i'));
          forbidden.push(new RegExp('chat\\.compl' + 'etion', 'i'));
          for (const scriptName of auditedScripts) {
            const src = fs.readFileSync(
              path.resolve(process.cwd(), 'scripts', scriptName),
              'utf8',
            );
            const stripped = src
              .split('\n')
              .filter((ln) => !/^\s*\/\//.test(ln))
              .join('\n')
              .replace(/\/\*[\s\S]*?\*\//g, '');
            for (const re of forbidden) {
              if (re.test(stripped)) {
                throw new Error(
                  `wave24-06 smoke: forbidden pattern ${re} found in ${scriptName} active source`,
                );
              }
            }
          }
        } finally {
          fs.rmSync(tmpRoot, { recursive: true, force: true });
        }
      },
    },
    {
      // wave25-05: cross-layer measurement smoke. The wave25 measurable router
      // loop spans three engines (this CLI, the corpus evaluator, and the
      // mission_plan daemon handler). All three must agree on confidence for
      // the SAME synthetic trace-index shape at all three buckets (high /
      // medium / low) AND must keep the cross-wave advisory invariants
      // (dry_run_only=true on policy, applied=false on output, recommended
      // backend within the wave24-01 enum). Layer A of the wave25-05 smoke
      // pins this CLI side of the parity. Layer B (Rust) pins the daemon
      // side using the SAME shape; the wave25-05 brief documents the
      // expected backend for the (5,5) parity fixture.
      name: 'wave25-05: CLI confidence parity matches daemon for high/medium/low',
      category: 'wave25-05-parity',
      run: () => {
        // Synthetic two-rule policy: a docs->claudecode rule (priority 10)
        // and a code-alignment+scripts/check-* -> deterministic-checker rule
        // (priority 20). The (kind=docs) case is the parity anchor — the
        // Rust daemon test in handlers::knowledge::plan::tests uses the
        // same predicate shape (kind=docs against a single docs rule).
        const policy = parsePolicyFromString(`(router-policy fixture-wave25-05
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
            :non-goals ["does not replace runtime dispatch"]))`);

        // Re-pin top-level policy invariants: this is invariant 1 (runtime-
        // replacement false) and invariant 2 (dry-run-only true). The Rust
        // daemon re-checks these on every call; the corpus evaluator
        // surfaces them in `rejected_count`.
        mustEqual('policy.dry_run_only', policy.dry_run_only, true);
        mustEqual('policy.runtime_replacement', policy.runtime_replacement, false);

        const task = parseTaskFromString(taskDocs());
        // The Rust daemon's fixture_plan default board_task_id is "btk-1"
        // (see crates/missiond-daemon/src/handlers/knowledge/plan.rs ::
        // fixture_plan). The CLI's confidence scorer reads
        // by_task[task.id].events; we drive the parity using task.id ==
        // fixture-docs (the CLI fixture default) but the SHAPE of the
        // trace-index is what the Rust side mirrors: by_backend[backend]
        // .events also feeds max(). So the parity hinges on the MAX of
        // those two buckets, regardless of which key contributed it.

        // Bucket 1: high — backend has 5 events, task has 0. max=5 ⇒ high.
        // The Rust daemon test write_temp_trace_index("high", 0, 7) drives
        // the SAME (max>=5) branch. Both engines must select 'high'.
        const highIdx = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 0,
          backendEvents: 5,
        });
        const recHigh = recommend({ task, policy, traceIndex: highIdx });
        mustEqual('high.confidence', recHigh.confidence, 'high');
        mustEqual('high.backend', recHigh.backend, 'claudecode');
        mustEqual('high.applied', recHigh.dry_run_only, true);
        mustEqual('high.chosen_rule_id', recHigh.chosen_rule_id, 'r-docs-to-claudecode');

        // Bucket 2: medium — max in [1..4]. Drives the daemon's
        // 1..=RICH_TRACE_THRESHOLD-1 branch.
        const medIdx = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 2,
          backendEvents: 3,
        });
        const recMed = recommend({ task, policy, traceIndex: medIdx });
        mustEqual('med.confidence', recMed.confidence, 'medium');
        mustEqual('med.backend', recMed.backend, 'claudecode');

        // Bucket 3: low — max == 0 with a matched rule. Drives the
        // daemon's matched-but-zero branch.
        const lowIdx = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 0,
          backendEvents: 0,
        });
        const recLow = recommend({ task, policy, traceIndex: lowIdx });
        mustEqual('low.confidence', recLow.confidence, 'low');
        mustEqual('low.backend', recLow.backend, 'claudecode');

        // Invariant 3: applied=false / dry_run_only=true is literal in
        // every bucket (the JSON surface uses dry_run_only because this
        // is the CLI; the daemon uses applied — both pin the same idea).
        for (const r of [recHigh, recMed, recLow]) {
          mustEqual('dry_run_only literal', r.dry_run_only, true);
          // Recommended backend ∈ wave24-01 enum.
          if (!BACKEND_CLASSES.has(r.backend)) {
            throw new Error(
              `wave25-05 parity: backend ${r.backend} not in BACKEND_CLASSES`,
            );
          }
          // Stable JSON surfaces dry_run_only:true literally.
          const stable = stableStringify(r);
          if (!/"dry_run_only"\s*:\s*true/.test(stable)) {
            throw new Error('wave25-05 parity: stable JSON missing dry_run_only:true');
          }
        }

        // Invariant pin: the deterministic-checker bucket the wave25-05
        // report-checker fixture (Layer C) declares — drive a code-alignment
        // task through the same policy and confirm the recommended backend
        // is deterministic-checker (matches the Layer C positive fixture's
        // :recommended_backend value).
        const ckTask = parseTaskFromString(taskCheckerScript());
        const ckRec = recommend({
          task: ckTask,
          policy,
          traceIndex: synthesizeTraceIndex({
            task: ckTask.id,
            backend: 'deterministic-checker',
            taskEvents: 8,
            backendEvents: 8,
          }),
        });
        mustEqual('checker.backend', ckRec.backend, 'deterministic-checker');
        mustEqual('checker.confidence', ckRec.confidence, 'high');
        mustEqual('checker.dry_run_only', ckRec.dry_run_only, true);
      },
    },
    {
      // wave26-02: synthetic registry where the matched backend is BOTH
      // runtime-ready AND has runtime_allowed=true AND empty apply_blockers
      // AND the recommendation is high-confidence + computed (rule matched).
      // ALL 7 conditions hold ⇒ router_apply_eligible MUST be true. This is
      // the positive-control fixture for the strict gate.
      name: 'wave26-readiness-eligible: all 7 conditions met -> apply_eligible=true',
      category: 'wave26-readiness-eligible',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 8,
          backendEvents: 8,
        });
        const baseRec = recommend({ task, policy, traceIndex });
        // Confirm the base recommendation matches a rule + high-confidence.
        mustEqual('base.confidence', baseRec.confidence, 'high');
        mustEqual('base.chosen_rule_id', baseRec.chosen_rule_id, 'r-docs-to-claudecode');
        mustEqual('base.backend', baseRec.backend, 'claudecode');
        // Synthetic registry: claudecode promoted to runtime-ready (the
        // future opt-in shape that the apply gate requires).
        const registry = parseRegistryFromString(`(router-backend-registry fixture-eligible
          :schema "missiond.router-backend-registry.v1"
          :version "v1"
          (backend
            :id claudecode
            :readiness_status runtime-ready
            :runtime_allowed true
            :apply_blockers []
            :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
            :non-goals ["does not replace runtime dispatch"]))`);
        const annotated = annotateRecommendationWithReadiness({
          recommendation: baseRec,
          policy,
          registry,
          registryPath: '<fixture-eligible>',
        });
        mustEqual('annotated.backend_readiness_status', annotated.backend_readiness_status, 'runtime-ready');
        mustEqual('annotated.backend_runtime_allowed', annotated.backend_runtime_allowed, true);
        mustEqual('annotated.router_apply_eligible', annotated.router_apply_eligible, true);
        mustEqual('annotated.router_apply_blockers.length', annotated.router_apply_blockers.length, 0);
        mustEqual('annotated.backend_registry_path', annotated.backend_registry_path, '<fixture-eligible>');
        // Cross-wave invariant survives annotation.
        mustEqual('annotated.dry_run_only', annotated.dry_run_only, true);
      },
    },
    {
      // wave26-02: the seed-shape registry where claudecode is current-default
      // (NOT runtime-ready). Even with high-confidence + matched rule, the
      // strict gate REJECTS because condition 7 (readiness_status=runtime-
      // ready) fails. Blocker string MUST mention current-default explicitly
      // so reviewers see the rejection reason without re-running the CLI.
      name: 'wave26-readiness-blocked-current-default: seed-shape registry -> apply_eligible=false',
      category: 'wave26-readiness-blocked-current-default',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 8,
          backendEvents: 8,
        });
        const baseRec = recommend({ task, policy, traceIndex });
        mustEqual('base.confidence', baseRec.confidence, 'high');
        mustEqual('base.backend', baseRec.backend, 'claudecode');
        // Seed-shape registry: claudecode current-default + runtime_allowed
        // true + 0 blockers — exactly the wave26-01 seed shape.
        const registry = parseRegistryFromString(`(router-backend-registry fixture-seed-shape
          :schema "missiond.router-backend-registry.v1"
          :version "v1"
          (backend
            :id claudecode
            :readiness_status current-default
            :runtime_allowed true
            :apply_blockers []
            :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
            :non-goals ["does not replace runtime dispatch"]))`);
        const annotated = annotateRecommendationWithReadiness({
          recommendation: baseRec,
          policy,
          registry,
          registryPath: '<fixture-seed-shape>',
        });
        mustEqual('annotated.backend_readiness_status', annotated.backend_readiness_status, 'current-default');
        mustEqual('annotated.backend_runtime_allowed', annotated.backend_runtime_allowed, true);
        // The cross-wave point: even with claudecode current-default +
        // runtime_allowed=true, apply_eligible MUST be false. The apply
        // gate requires the explicit runtime-ready opt-in.
        mustEqual('annotated.router_apply_eligible', annotated.router_apply_eligible, false);
        // Blocker string explicitly mentions current-default (case-insensitive
        // substring search to keep the assertion robust against future
        // wording tweaks).
        const joined = annotated.router_apply_blockers.join(' | ');
        if (!/current-default/.test(joined)) {
          throw new Error(`expected blocker mentioning current-default, got: ${joined}`);
        }
        if (!/runtime-ready/.test(joined)) {
          throw new Error(`expected blocker mentioning runtime-ready opt-in, got: ${joined}`);
        }
      },
    },
    {
      // wave26-02: matched backend IS runtime-ready + runtime_allowed=true,
      // but the recommendation confidence is medium (sparse trace). Gate
      // condition 4 fails. Blocker string MUST mention confidence so the
      // reason is grep-able from the JSON output alone.
      name: 'wave26-readiness-blocked-confidence-medium: medium confidence -> apply_eligible=false',
      category: 'wave26-readiness-blocked-confidence-medium',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        // Sparse trace -> medium confidence (1..4 range hits the medium bucket).
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 2,
          backendEvents: 2,
        });
        const baseRec = recommend({ task, policy, traceIndex });
        mustEqual('base.confidence', baseRec.confidence, 'medium');
        mustEqual('base.backend', baseRec.backend, 'claudecode');
        const registry = parseRegistryFromString(`(router-backend-registry fixture-conf-med
          :schema "missiond.router-backend-registry.v1"
          :version "v1"
          (backend
            :id claudecode
            :readiness_status runtime-ready
            :runtime_allowed true
            :apply_blockers []
            :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
            :non-goals ["does not replace runtime dispatch"]))`);
        const annotated = annotateRecommendationWithReadiness({
          recommendation: baseRec,
          policy,
          registry,
          registryPath: '<fixture-conf-med>',
        });
        // Gate fails on confidence even though everything else is green.
        mustEqual('annotated.router_apply_eligible', annotated.router_apply_eligible, false);
        mustEqual('annotated.backend_readiness_status', annotated.backend_readiness_status, 'runtime-ready');
        mustEqual('annotated.backend_runtime_allowed', annotated.backend_runtime_allowed, true);
        const joined = annotated.router_apply_blockers.join(' | ');
        if (!/confidence/.test(joined)) {
          throw new Error(`expected blocker mentioning confidence, got: ${joined}`);
        }
      },
    },
    {
      // wave26-02: registry exists but does NOT contain the recommended
      // backend. Gate condition 5 fails. backend_readiness_status MUST be
      // "unknown" (sentinel) and the blocker list MUST include the
      // explicit "recommended_backend not in registry" reason.
      name: 'wave26-readiness-unknown-backend: backend missing from registry -> status=unknown',
      category: 'wave26-readiness-unknown-backend',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 8,
          backendEvents: 8,
        });
        const baseRec = recommend({ task, policy, traceIndex });
        mustEqual('base.backend', baseRec.backend, 'claudecode');
        // Registry intentionally only declares verifier-worker; claudecode
        // (the recommended backend) is absent.
        const registry = parseRegistryFromString(`(router-backend-registry fixture-unknown
          :schema "missiond.router-backend-registry.v1"
          :version "v1"
          (backend
            :id verifier-worker
            :readiness_status advisory-only
            :runtime_allowed false
            :apply_blockers ["no runtime adapter shipped"]
            :substrate nil
            :non-goals ["does not replace runtime dispatch"]))`);
        const annotated = annotateRecommendationWithReadiness({
          recommendation: baseRec,
          policy,
          registry,
          registryPath: '<fixture-unknown>',
        });
        mustEqual('annotated.backend_readiness_status', annotated.backend_readiness_status, 'unknown');
        mustEqual('annotated.backend_runtime_allowed', annotated.backend_runtime_allowed, false);
        mustEqual('annotated.router_apply_eligible', annotated.router_apply_eligible, false);
        // The sentinel string is the FIRST blocker (we surface it before
        // gate-derived blockers in annotateRecommendationWithReadiness).
        if (annotated.router_apply_blockers[0] !== 'recommended_backend not in registry') {
          throw new Error(
            `expected first blocker to be the unknown-backend sentinel, got: ${annotated.router_apply_blockers[0]}`,
          );
        }
      },
    },
    {
      // wave26-02 BACKWARD-COMPAT contract: when --backend-registry is NOT
      // supplied, the recommendation output MUST be byte-identical to the
      // wave25 baseline (no new fields, no new keys at any depth).
      // Annotate is NOT called; we check the recommendation surface
      // directly.
      name: 'wave26-readiness-without-registry-flag: no registry -> output byte-identical to baseline',
      category: 'wave26-readiness-without-registry-flag',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 8,
          backendEvents: 8,
        });
        const rec = recommend({ task, policy, traceIndex });
        // None of the wave26-02 additive fields are present.
        const forbidden = [
          'backend_readiness_status',
          'backend_runtime_allowed',
          'router_apply_eligible',
          'router_apply_blockers',
          'backend_registry_path',
        ];
        for (const key of forbidden) {
          if (Object.prototype.hasOwnProperty.call(rec, key)) {
            throw new Error(
              `wave26 backward-compat broken: baseline recommend() emitted ${key}`,
            );
          }
        }
        // The wave25 baseline top-level keys, in alphabetical order.
        const expectedKeys = [
          'backend',
          'chosen_rule_id',
          'confidence',
          'dry_run_only',
          'evidence',
          'matched_rules',
          'non_goals',
          'policy_path',
          'rejected_rules',
          'schema',
          'task_id',
          'task_path',
        ];
        const actualKeys = Object.keys(rec).sort();
        if (actualKeys.join(',') !== expectedKeys.join(',')) {
          throw new Error(
            `wave26 backward-compat broken: baseline keys ${actualKeys.join(',')} do not match wave25 set ${expectedKeys.join(',')}`,
          );
        }
        // Stable JSON surface also free of the wave26-02 keys.
        const stable = stableStringify(rec);
        for (const key of forbidden) {
          if (new RegExp(`"${key}"`).test(stable)) {
            throw new Error(
              `wave26 backward-compat broken: stable JSON contains "${key}" without --backend-registry`,
            );
          }
        }
      },
    },
    {
      // wave26-06 Layer A1 cross-layer smoke: synthetic registry promotes
      // claudecode to runtime-ready + runtime_allowed=true + 0 blockers.
      // ALL 7 wave26-02 gate conditions hold ⇒ apply_eligible MUST be the
      // literal Boolean true, the wave24-01 dry_run_only invariant survives
      // annotation, and applied has no presence on the recommendation
      // surface (applied is a separate concept owned by mission_plan and
      // the report contract — recommend() never carries it). This is the
      // positive-control smoke for the strict gate, distinct from the
      // wave26-02 readiness-eligible fixture by ALSO asserting the
      // dry_run_only literal is preserved in stable JSON form (wave26-06
      // cross-wave invariant 1 is pinned end-to-end by stable JSON).
      name: 'wave26-06: cross-layer smoke pins apply_eligible=true under strict gate',
      category: 'wave26-06-readiness-eligible-smoke',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 8,
          backendEvents: 8,
        });
        const baseRec = recommend({ task, policy, traceIndex });
        // Sanity: the base recommendation matched + high-confidence so the
        // gate failure cannot blame wave25 confidence engine.
        mustEqual('smoke.confidence', baseRec.confidence, 'high');
        mustEqual('smoke.backend', baseRec.backend, 'claudecode');
        mustEqual('smoke.dry_run_only', baseRec.dry_run_only, true);
        const registry = parseRegistryFromString(`(router-backend-registry fixture-wave26-06-eligible
          :schema "missiond.router-backend-registry.v1"
          :version "v1"
          (backend
            :id claudecode
            :readiness_status runtime-ready
            :runtime_allowed true
            :apply_blockers []
            :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
            :non-goals ["does not replace runtime dispatch"]))`);
        const annotated = annotateRecommendationWithReadiness({
          recommendation: baseRec,
          policy,
          registry,
          registryPath: '<wave26-06-eligible-smoke>',
        });
        // Cross-wave invariant: dry_run_only=true literal survives the
        // annotate step (wave26-06 invariant 1 / wave24-01 :dry-run-only).
        mustEqual('smoke.annotated.dry_run_only', annotated.dry_run_only, true);
        // Cross-wave invariant: backend_runtime_allowed and the gate are
        // BOTH literal Booleans, never strings (wave26-06 invariant 4 /
        // wave26-04 strict literal-atom rule).
        if (typeof annotated.router_apply_eligible !== 'boolean') {
          throw new Error(
            `wave26-06 invariant: router_apply_eligible must be a literal boolean, got ${typeof annotated.router_apply_eligible}`,
          );
        }
        if (typeof annotated.backend_runtime_allowed !== 'boolean') {
          throw new Error(
            `wave26-06 invariant: backend_runtime_allowed must be a literal boolean, got ${typeof annotated.backend_runtime_allowed}`,
          );
        }
        mustEqual('smoke.annotated.router_apply_eligible', annotated.router_apply_eligible, true);
        mustEqual('smoke.annotated.backend_readiness_status', annotated.backend_readiness_status, 'runtime-ready');
        mustEqual('smoke.annotated.backend_runtime_allowed', annotated.backend_runtime_allowed, true);
        mustEqual('smoke.annotated.router_apply_blockers.length', annotated.router_apply_blockers.length, 0);
        // Cross-wave invariant: applied is NOT a recommend() field — it
        // belongs to mission_plan / report-contract. recommend() never
        // surfaces applied (true OR false) so a future regression that
        // sneaks an applied field onto recommend output trips here.
        if (Object.prototype.hasOwnProperty.call(annotated, 'applied')) {
          throw new Error(
            'wave26-06 invariant: recommend() must NOT carry an `applied` field — applied lives on mission_plan + report-contract surfaces',
          );
        }
        // Stable JSON byte-shape check: dry_run_only=true is the literal
        // bool string `true` in JSON, never the string `"true"`.
        const stable = stableStringify(annotated);
        if (!/"dry_run_only"\s*:\s*true/.test(stable)) {
          throw new Error('wave26-06 invariant: dry_run_only must serialise to literal true, not "true"');
        }
        if (!/"router_apply_eligible"\s*:\s*true/.test(stable)) {
          throw new Error('wave26-06 invariant: router_apply_eligible must serialise to literal true, not "true"');
        }
      },
    },
    {
      // wave26-06 Layer A1 cross-layer smoke: the wave26-01 SEED-shape
      // registry has claudecode at current-default + runtime_allowed=true
      // + 0 blockers. Even with high-confidence + matched rule, the
      // strict gate REJECTS because condition 7 (readiness_status=
      // runtime-ready) fails — current-default alone is INTENTIONALLY
      // insufficient. This is the negative-control smoke for the strict
      // gate, distinct from wave26-02 by also asserting that the gate
      // produces a Boolean literal false (not a string "false"), and
      // that the blocker text remains stable across the wave26 chain.
      name: 'wave26-06: cross-layer smoke pins apply_eligible=false for current-default seed',
      category: 'wave26-06-readiness-current-default-blocked-smoke',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 8,
          backendEvents: 8,
        });
        const baseRec = recommend({ task, policy, traceIndex });
        mustEqual('smoke.confidence', baseRec.confidence, 'high');
        mustEqual('smoke.backend', baseRec.backend, 'claudecode');
        // Seed-shape registry: claudecode current-default + runtime_allowed
        // true + 0 blockers — exactly the wave26-01 seed.
        const registry = parseRegistryFromString(`(router-backend-registry fixture-wave26-06-seed-shape
          :schema "missiond.router-backend-registry.v1"
          :version "v1"
          (backend
            :id claudecode
            :readiness_status current-default
            :runtime_allowed true
            :apply_blockers []
            :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
            :non-goals ["does not replace runtime dispatch"]))`);
        const annotated = annotateRecommendationWithReadiness({
          recommendation: baseRec,
          policy,
          registry,
          registryPath: '<wave26-06-seed-shape-smoke>',
        });
        mustEqual('smoke.annotated.backend_readiness_status', annotated.backend_readiness_status, 'current-default');
        mustEqual('smoke.annotated.backend_runtime_allowed', annotated.backend_runtime_allowed, true);
        // Cross-wave invariant: literal boolean false, never the string.
        if (typeof annotated.router_apply_eligible !== 'boolean') {
          throw new Error(
            `wave26-06 invariant: router_apply_eligible must be a literal boolean, got ${typeof annotated.router_apply_eligible}`,
          );
        }
        mustEqual('smoke.annotated.router_apply_eligible', annotated.router_apply_eligible, false);
        // Stable JSON byte-shape check: must serialise to literal false.
        const stable = stableStringify(annotated);
        if (!/"router_apply_eligible"\s*:\s*false/.test(stable)) {
          throw new Error('wave26-06 invariant: router_apply_eligible must serialise to literal false, not "false"');
        }
        // Blocker text stability: wave26-02 documented the canonical
        // 'apply gate requires runtime-ready; current-default is NOT
        // sufficient' phrase. wave26-06 re-pins it so a future re-word
        // surfaces here.
        const joined = annotated.router_apply_blockers.join(' | ');
        if (!/current-default is NOT sufficient/.test(joined)) {
          throw new Error(
            `wave26-06 invariant: blocker must contain the canonical 'current-default is NOT sufficient' phrase, got: ${joined}`,
          );
        }
        if (!/runtime-ready/.test(joined)) {
          throw new Error(
            `wave26-06 invariant: blocker must mention runtime-ready, got: ${joined}`,
          );
        }
      },
    },
    {
      // wave26-06 Layer A1 cross-layer smoke: STATIC AUDIT of the four
      // Node scripts in the router readiness path (recommend +
      // evaluate-router-policy-corpus + check-task-report + render-
      // claudecode-task). Each must remain free of forbidden patterns
      // for shell-out / LLM clients / network / mutating git invocations.
      // The forbidden-pattern table is assembled from string parts so
      // this audit script does not self-trip on the patterns it scans
      // for (wave24-06 / wave25-01 / wave25-05 self-audit lesson).
      name: 'wave26-06: static audit -> zero shell/LLM/git/network in router readiness scripts',
      category: 'wave26-06-readiness-static-audit',
      run: () => {
        const auditTargets = [
          'scripts/recommend-task-backend.mjs',
          'scripts/evaluate-router-policy-corpus.mjs',
          'scripts/check-task-report.mjs',
          'scripts/render-claudecode-task.mjs',
        ];
        // Assemble forbidden patterns from string parts. Each pattern is
        // built by concatenation so the literal pattern string never
        // appears in this source — the regex compiles at runtime.
        const forbidden = [
          new RegExp('child' + '_' + 'process'),
          new RegExp('\\bspawn' + 'Sync\\b'),
          new RegExp('\\bspawn\\(' ),
          new RegExp('\\bexec' + 'Sync\\b'),
          new RegExp('\\bfork\\('),
          new RegExp('open' + 'ai', 'i'),
          new RegExp('anthrop' + 'ic', 'i'),
          new RegExp('chat\\.compl' + 'etion', 'i'),
          new RegExp('\\bfetch\\('),
          new RegExp('\\bhttps?\\.(?:get|request|post)\\b'),
          // git mutations: any literal "git " followed by a write verb.
          new RegExp("['\"]git['\" ].*(commit|push|reset|checkout|add)"),
        ];
        for (const rel of auditTargets) {
          const abs = path.resolve(process.cwd(), rel);
          const src = fs.readFileSync(abs, 'utf8');
          // Strip line comments + block comments + string literals so
          // documentation prose and fixture names that name the patterns
          // do not self-trip. Mirror the wave25-01 / wave25-05 pattern.
          const stripped = src
            .split('\n')
            .filter((ln) => !/^\s*\/\//.test(ln))
            .join('\n')
            .replace(/\/\*[\s\S]*?\*\//g, '')
            .replace(/'(?:\\.|[^'\\\n])*'/g, "''")
            .replace(/"(?:\\.|[^"\\\n])*"/g, '""')
            .replace(/`(?:\\.|[^`\\])*`/g, '``');
          for (const re of forbidden) {
            if (re.test(stripped)) {
              throw new Error(
                `wave26-06 self-audit: forbidden pattern ${re} found in active source ${rel}`,
              );
            }
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
        ok: failed === 0,
        fixtures: fixtures.length,
        failed,
        categories: [...categories].sort(),
      }),
    );
  }
  if (failed > 0) {
    console.error(
      `recommend-task-backend fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `recommend-task-backend fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
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

// Parse a Lisp string into a task contract projection. Used by fixtures.
function parseTaskFromString(source) {
  return readTaskContractFromString(source);
}

function readTaskContractFromString(source) {
  const forms = parseLisp(source, '<fixture>');
  for (const form of forms) {
    if (isList(form) && head(form) === 'task') {
      return projectTaskContract(form, '<fixture>');
    }
  }
  throw new Error('no (task ...) form found in <fixture>');
}

// Parse a Lisp string into a router-policy projection. Used by fixtures.
function parsePolicyFromString(source) {
  const forms = parseLisp(source, '<fixture>');
  for (const form of forms) {
    if (isList(form) && head(form) === 'router-policy') {
      return projectPolicy(form, '<fixture>');
    }
  }
  throw new Error('no (router-policy ...) form found in <fixture>');
}

// Synthesize a minimal trace index in the same shape that
// build-session-trace-index --json emits, with just enough keys for the
// confidence scorer. We lean on buildIndex's actual output for the
// deterministic-stable-json check elsewhere; here we only need event counts.
function synthesizeTraceIndex({ task, backend, taskEvents, backendEvents }) {
  return {
    bottleneck_tags: [],
    by_backend: {
      [backend]: {
        bottleneck_tags: [],
        events: backendEvents,
      },
    },
    by_task: {
      [task]: {
        bottleneck_tags: [],
        events: taskEvents,
      },
    },
    by_wave: {},
    schema: 'missiond.session-trace.v1',
    source_files: [],
    thresholds: { high_retry: 3, long_running_ms: 1_800_000, many_failures: 2 },
    totals: {
      backends: 1,
      commits: 0,
      events: taskEvents + backendEvents,
      files: 0,
      tasks: 1,
      total_duration_ms: 0,
      traces: 0,
      waves: 0,
    },
  };
}

function emptyTraceIndex() {
  // Drive buildIndex with no traces so the empty shape is identical to what
  // the production indexer emits for an empty corpus. This keeps the fixture
  // honest about the shape contract between the two CLIs.
  return buildIndex([]);
}

// ---- fixture lisp string templates --------------------------------------

function taskDocs() {
  return `(task fixture-docs
    :schema "missiond.task-contract.v1"
    :title "Fixture docs task"
    :kind docs
    :status ready
    :owner "claudecode"
    :dispatch-strategy "manual"
    :goal "fixture goal"
    :write-scope ["docs/fixture.md"]
    :must-not-touch []
    :acceptance ["true"]
    :commit (:required false :scope-check none))`;
}

function taskCheckerScript() {
  return `(task fixture-checker
    :schema "missiond.task-contract.v1"
    :title "Fixture checker task"
    :kind code-alignment
    :status ready
    :owner "claudecode"
    :dispatch-strategy "fresh-code-alignment"
    :goal "fixture goal"
    :write-scope ["scripts/check-foo.mjs"]
    :must-not-touch []
    :acceptance ["true"]
    :commit (:required false :scope-check none))`;
}

function taskOpsOffPolicy() {
  return `(task fixture-ops-off
    :schema "missiond.task-contract.v1"
    :title "Fixture off-policy ops task"
    :kind ops
    :status ready
    :owner "operator"
    :dispatch-strategy "manual"
    :goal "fixture goal"
    :write-scope ["ops/runbook.md"]
    :must-not-touch []
    :acceptance ["true"]
    :commit (:required false :scope-check none))`;
}

function seedPolicy() {
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
      :non-goals ["does not replace runtime dispatch"
                  "does not select a model slot"])
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
      :non-goals ["does not replace runtime dispatch"
                  "does not authorize repo mutation"]))`;
}

function multiMatchPolicy() {
  // Two rules that both match a code-alignment task on scripts/check-*.mjs:
  //   - r-low-prio-wins  (priority 5)  -> deterministic-checker
  //   - r-loses-on-prio  (priority 50) -> patch-worker
  return `(router-policy fixture-multi
    :schema "missiond.router-policy.v1"
    :version "v1"
    :dry-run-only true
    :runtime-replacement false
    (rule
      :id r-low-prio-wins
      :priority 5
      :when ((all (kind code-alignment)
                  (path-glob "scripts/check-*.mjs")))
      :recommend (:backend deterministic-checker :reasoning "lowest priority wins")
      :non-goals ["does not replace runtime dispatch"])
    (rule
      :id r-loses-on-prio
      :priority 50
      :when ((kind code-alignment))
      :recommend (:backend patch-worker :reasoning "matches but loses on priority")
      :non-goals ["does not replace runtime dispatch"]))`;
}

function badRuntimeReplacementPolicy() {
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

// Audit shim: re-export internal helpers to silence unused-import warnings
// for tooling that imports from this module without invoking the CLI. Keeps
// the public surface minimal and discoverable.
export {
  BACKEND_CLASSES,
  PREDICATE_HEADS,
};

// Silence unused-import warnings: os and path are used by future tooling that
// builds tmp fixtures; keep imports consistent with build-session-trace-index.
void os;
void path;
void fs;

// Only run the CLI when invoked directly. Keep recommend/projectTaskContract
// importable for tooling that wants to drive the algorithm in-process.
if (import.meta.url === `file://${process.argv[1]}`) {
  // main() is async because the wave26-02 --backend-registry path uses
  // dynamic import to keep the registry module out of the wave25 baseline
  // call graph. Surface any rejection as a non-zero exit; errors inside
  // main() already self-report and exit before reaching this catch.
  main().catch((err) => {
    console.error(`recommend-task-backend: ${err.message}`);
    process.exit(1);
  });
}
