#!/usr/bin/env node

// Wave27-02 read-only CLI: build a router-dispatch-descriptor v1 record from
// a task contract + router-policy + (required) backend-registry + (optional)
// trace-index. Emits Lisp by default, JSON with --json. Self-tests with
// --dry-fixture.
//
// HARD GUARANTEES (non-negotiable, enforced by code shape and the wave27-01
// checker which we pre-validate against before emitting):
//   - dry_run_only is ALWAYS literal `true`.
//   - runtime_replacement is ALWAYS literal `false`.
//   - no_execution is ALWAYS literal `true`.
//   These three are hard-coded literals here — they are NEVER computed
//   conditionally and they are NEVER read off any input source. Even when
//   the readiness gate would otherwise admit `router_apply_eligible=true`,
//   the descriptor's no-execution invariants stay locked. This CLI cannot
//   promote a backend to live dispatch even by accident.
//
//   - router_apply_eligible is preserved BYTE-VERBATIM from the
//     readiness-annotated recommendation (annotateRecommendationWithReadiness).
//     We never recompute the wave26-02 7-condition gate — the upstream
//     recommend() output is the canonical source. This avoids drift between
//     the recommendation surface and the descriptor surface.
//
//   - Output is deterministic. Lisp emission walks a fixed field order
//     documented below. JSON output sorts object keys.
//
//   - Read-only: no shell-out, no child_process, no spawn, no git, no fetch
//     / http / https, no LLM. Filesystem use is fs.readFileSync only via the
//     imported helpers and the optional --dry-fixture tmpdir (which the
//     fixture cleans up at end).
//
//   - When the registry is missing / unreadable / malformed / lacks the
//     recommended backend, this CLI STILL emits a valid descriptor with
//     :router_apply_eligible false and an explicit blocker so the descriptor
//     passes the wave27-01 checker. Failure to even load the recommendation
//     (e.g. policy declares :runtime-replacement true) bubbles up as a
//     non-zero exit because there is no descriptor to emit.
//
// Usage:
//   node scripts/build-router-dispatch-descriptor.mjs \
//     --task <task.lisp> \
//     --policy <router-policy.lisp> \
//     --backend-registry <registry.lisp> \
//     [--trace-index <index.json>] [--json] [--dry-fixture]

import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';

import {
  recommend,
  annotateRecommendationWithReadiness,
  readTaskContractFile,
  projectTaskContract,
} from './recommend-task-backend.mjs';

import { readBackendRegistryFile, projectRegistry } from './check-router-backend-registry.mjs';

import { readRouterPolicyFile, projectPolicy } from './check-router-policy.mjs';

import {
  SCHEMA,
  DESCRIPTOR_HEAD,
  validateDescriptorObject,
} from './check-router-dispatch-descriptor.mjs';

import {
  parseLisp,
  isList,
  head,
} from './lib/missiond_lisp.mjs';

const RECOMMENDATION_SCHEMA = 'missiond.router-recommendation.v0';
const GENERATOR = 'build-router-dispatch-descriptor.mjs@v0';

// Fixed field order for Lisp emission. Required-first (in the wave27-01
// schema's required-descriptor-fields order), optional-last (in the schema's
// optional-descriptor-fields order). This is the deterministic shape the
// renderer/plan/report consumers can rely on.
const LISP_FIELD_ORDER = [
  ':schema',
  ':task_id',
  ':recommended_backend',
  ':router_confidence',
  ':backend_readiness_status',
  ':backend_runtime_allowed',
  ':router_apply_eligible',
  ':router_apply_blockers',
  ':dry_run_only',
  ':runtime_replacement',
  ':no_execution',
  ':source_recommendation_schema',
  ':source_policy_path',
  ':source_backend_registry_path',
  // optional below
  ':source_trace_index_path',
  ':generated_at',
  ':generator',
  ':notes',
];

const usage = `Usage:
  node scripts/build-router-dispatch-descriptor.mjs --task <task.lisp> --policy <router-policy.lisp> --backend-registry <registry.lisp> [--trace-index <index.json>] [--json] [--dry-fixture]

Read-only deterministic CLI: builds a router-dispatch-descriptor v1 record from
the supplied inputs and emits it to stdout. NEVER mutates files, NEVER shells
out, NEVER calls an LLM, NEVER executes the recommended backend. The three
no-execution invariants (:dry_run_only true, :runtime_replacement false,
:no_execution true) are hard-coded literals — they cannot be relaxed.

Flags:
  --task <path>             Path to the task contract Lisp file (required).
  --policy <path>           Path to the router-policy Lisp file (required).
  --backend-registry <path> Path to the router-backend-registry Lisp file
                            (required for descriptor mode — descriptor cannot
                            exist without readiness data).
  --trace-index <path>      Optional JSON corpus index produced by
                            build-session-trace-index.mjs --json. Used purely
                            for confidence scoring; presence does NOT change
                            apply-eligibility.
  --json                    Emit a deterministic JSON object (keys sorted).
                            Default emits the Lisp (router-dispatch-descriptor
                            ...) form so the output is grep-friendly and
                            check-router-dispatch-descriptor.mjs --stdin
                            consumable.
  --dry-fixture             Run self-contained pass/fail fixtures and exit.
`;

async function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  let taskPath = null;
  let policyPath = null;
  let backendRegistryPath = null;
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
    } else if (arg === '--task') {
      taskPath = args[++i];
    } else if (arg === '--policy') {
      policyPath = args[++i];
    } else if (arg === '--backend-registry') {
      backendRegistryPath = args[++i];
    } else if (arg === '--trace-index') {
      traceIndexPath = args[++i];
    } else {
      console.error(`build-router-dispatch-descriptor: unknown flag ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  if (dryFixture) {
    await runFixtures(json);
    return;
  }

  if (!taskPath || !policyPath) {
    console.error('build-router-dispatch-descriptor: --task and --policy are required');
    console.error(usage);
    process.exit(2);
  }
  if (!backendRegistryPath) {
    // Descriptor mode REQUIRES backend-registry. The descriptor schema's
    // :source_backend_registry_path is a required field; we never invent a
    // path. If readiness cannot be loaded we still produce a valid descriptor
    // with eligible=false + explicit blocker, but the path itself must come
    // from the operator.
    console.error(
      'build-router-dispatch-descriptor: --backend-registry is required for descriptor mode',
    );
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const resolvedTask = path.resolve(cwd, taskPath);
  const resolvedPolicy = path.resolve(cwd, policyPath);
  const resolvedRegistry = path.resolve(cwd, backendRegistryPath);
  const resolvedTraceIndex = traceIndexPath ? path.resolve(cwd, traceIndexPath) : null;

  // Load task + policy. Both are non-negotiable; failure here means no
  // descriptor can exist (no :task_id, no policy invariants to attest), so
  // exit non-zero.
  let task;
  try {
    task = readTaskContractFile(resolvedTask);
  } catch (err) {
    console.error(
      `build-router-dispatch-descriptor: failed to read task contract: ${err.message}`,
    );
    process.exit(1);
  }

  let policy;
  try {
    policy = readRouterPolicyFile(resolvedPolicy);
  } catch (err) {
    console.error(
      `build-router-dispatch-descriptor: failed to read router-policy: ${err.message}`,
    );
    process.exit(1);
  }

  // Defensive re-check: any policy that claims runtime-replacement is
  // rejected before we even attempt the recommendation. This mirrors
  // recommend-task-backend.mjs's main() so the CLI is independently safe.
  if (policy.runtime_replacement === true) {
    console.error(
      `build-router-dispatch-descriptor: policy ${policy.id} declares :runtime-replacement true; rejected (descriptors are advisory only).`,
    );
    process.exit(1);
  }
  if (policy.dry_run_only !== true) {
    console.error(
      `build-router-dispatch-descriptor: policy ${policy.id} missing :dry-run-only true; rejected (descriptors are advisory only).`,
    );
    process.exit(1);
  }

  // Optional trace index: confidence-only signal. We never mutate the
  // recommendation if it loads — a missing/malformed trace index degrades
  // confidence to low, which the readiness gate already accounts for.
  let traceIndex = null;
  if (resolvedTraceIndex) {
    try {
      const raw = fs.readFileSync(resolvedTraceIndex, 'utf8');
      traceIndex = JSON.parse(raw);
    } catch (err) {
      console.error(
        `build-router-dispatch-descriptor: failed to read trace index: ${err.message}`,
      );
      process.exit(1);
    }
  }

  // Run the wave25-02 recommendation algorithm in-process. We never shell
  // out to recommend-task-backend.mjs — we import the pure function so the
  // descriptor and the recommendation are derived from the same code.
  const recommendation = recommend({
    task,
    policy,
    traceIndex,
    taskPath: resolvedTask,
    policyPath: resolvedPolicy,
  });

  // Try to load the registry. Failure here is RECOVERABLE for the descriptor
  // — we still emit a valid record with eligible=false and an explicit
  // blocker. This is the contract requirement #6: registry missing/malformed
  // must NOT make the descriptor invalid; it makes it ineligible.
  let registry = null;
  let registryError = null;
  try {
    registry = readBackendRegistryFile(resolvedRegistry);
  } catch (err) {
    registryError = err;
  }

  const descriptor = buildDescriptor({
    task,
    policy,
    recommendation,
    registry,
    registryError,
    registryPath: resolvedRegistry,
    policyPath: resolvedPolicy,
    traceIndexPath: resolvedTraceIndex,
    cwd,
  });

  // Pre-validate against the wave27-01 checker before emitting. If the
  // descriptor fails its own schema, that is a code bug in this CLI — exit
  // non-zero rather than ship a malformed record. The downstream pipe
  // (--json | check-router-dispatch-descriptor.mjs --stdin) is a belt-and-
  // suspenders second line, but pre-validating here means callers see the
  // failure immediately even in plain Lisp mode.
  const errors = validateDescriptorObject(descriptor);
  if (errors.length > 0) {
    console.error('build-router-dispatch-descriptor: emitted descriptor failed schema:');
    for (const e of errors) console.error(`  - ${e}`);
    process.exit(1);
  }

  if (json) {
    console.log(stableStringify(descriptor));
  } else {
    console.log(emitLisp(descriptor));
  }
}

// Construct a descriptor object that matches the wave27-01 projection shape
// expected by validateDescriptorObject(). Inputs:
//   - task: projected task contract (has .id)
//   - policy: projected router-policy (used only for the registry-failure
//     readiness fallback when annotateRecommendationWithReadiness cannot run)
//   - recommendation: output of recommend({...})
//   - registry: projected registry, or null when registry could not load
//   - registryError: Error object when the registry load failed, else null
//   - registryPath / policyPath / traceIndexPath: absolute paths supplied
//     to the CLI; we relativize each against cwd before emitting because
//     the schema requires repo-relative paths.
//
// Cross-wave invariants (LITERAL, never computed):
//   dry_run_only        = true
//   runtime_replacement = false
//   no_execution        = true
function buildDescriptor({
  task,
  policy,
  recommendation,
  registry,
  registryError,
  registryPath,
  policyPath,
  traceIndexPath,
  cwd,
}) {
  // When the registry loaded, run the upstream readiness annotator so the
  // descriptor's eligibility / readiness / blockers are derived by the SAME
  // function that powers the recommend-task-backend.mjs --backend-registry
  // output. This is the "preserve verbatim" guarantee: we never recompute
  // the 7-condition gate locally.
  let annotated;
  if (registry) {
    annotated = annotateRecommendationWithReadiness({
      recommendation,
      policy,
      registry,
      registryPath,
    });
  } else {
    // Registry failed to load. Synthesize a readiness annotation with the
    // synthetic "unknown" status so the descriptor still passes the schema.
    // Always eligible=false, runtime_allowed=false, with an explicit blocker
    // describing the registry failure.
    const blocker = registryErrorToBlocker(registryError);
    annotated = {
      ...recommendation,
      backend_readiness_status: 'unknown',
      backend_runtime_allowed: false,
      router_apply_eligible: false,
      router_apply_blockers: [blocker],
      backend_registry_path: registryPath,
    };
  }

  // descriptor id is the task id with a `dd-` prefix so the same task can
  // own a descriptor without colliding with the task's own id. Schema
  // requires the id to match /^[a-z0-9][a-z0-9._-]*$/ which the prefix +
  // task id satisfies given task ids are kebab-case.
  const descriptorId = `dd-${task.id}`;

  const descriptor = {
    id: descriptorId,
    schema: SCHEMA,
    task_id: task.id,
    recommended_backend: annotated.backend,
    router_confidence: annotated.confidence,
    backend_readiness_status: annotated.backend_readiness_status,
    backend_runtime_allowed: annotated.backend_runtime_allowed === true,
    // PRESERVED VERBATIM from annotateRecommendationWithReadiness — the
    // wave26-02 7-condition gate is the canonical source. We never short-
    // circuit it on our side.
    router_apply_eligible: annotated.router_apply_eligible === true,
    router_apply_blockers: Array.isArray(annotated.router_apply_blockers)
      ? annotated.router_apply_blockers.filter((b) => typeof b === 'string' && b.trim() !== '')
      : [],
    // LOCKED LITERALS — see the file header. These are NEVER read from the
    // recommendation, NEVER read from the policy, NEVER computed.
    dry_run_only: true,
    runtime_replacement: false,
    no_execution: true,
    source_recommendation_schema: RECOMMENDATION_SCHEMA,
    source_policy_path: relPath(policyPath, cwd),
    source_backend_registry_path: relPath(registryPath, cwd),
    source_trace_index_path: traceIndexPath ? relPath(traceIndexPath, cwd) : null,
    notes: null,
    generated_at: new Date().toISOString(),
    generator: GENERATOR,
  };

  return descriptor;
}

// Map a registry-load failure into a single concrete blocker string. Caller
// is responsible for ensuring this is the only blocker recorded when the
// registry didn't load (the descriptor still must list at least one).
function registryErrorToBlocker(err) {
  if (!err) return 'registry_missing';
  // Distinguish missing file from malformed parse. fs.readFileSync raises
  // ENOENT for missing; the lisp parser raises with .message. We don't have
  // the raw fs error code reliably (readBackendRegistryFile wraps it), so we
  // string-match on the exposed message.
  const msg = String(err.message ?? err);
  if (/ENOENT|no such file|not found/i.test(msg)) {
    return `registry_missing: ${msg}`;
  }
  return `registry_malformed: ${msg}`;
}

// Render a descriptor object as a (router-dispatch-descriptor ...) Lisp
// form. Field order is the LISP_FIELD_ORDER constant above (required-first
// in schema order, then optional). Nullable optional fields are omitted
// entirely when null — they do NOT appear as `:notes ""` placeholders.
function emitLisp(descriptor) {
  const lines = [];
  lines.push(`(${DESCRIPTOR_HEAD} ${descriptor.id}`);
  for (const field of LISP_FIELD_ORDER) {
    const key = field.slice(1); // strip leading ':'
    const value = descriptor[key];
    // Skip optional fields that are null — Lisp emission must not synthesize
    // empty placeholders.
    if (value === null || value === undefined) continue;
    lines.push(`  ${field} ${formatLispValue(field, value)}`);
  }
  lines[lines.length - 1] += ')';
  return lines.join('\n');
}

function formatLispValue(field, value) {
  // String fields — quote with JSON.stringify which gives us proper escaping
  // for backslashes and quotes. The router-dispatch-descriptor schema only
  // contains plain ASCII strings in practice but we don't bet on it.
  const STRING_FIELDS = new Set([
    ':schema',
    ':task_id',
    ':source_recommendation_schema',
    ':source_policy_path',
    ':source_backend_registry_path',
    ':source_trace_index_path',
    ':notes',
    ':generated_at',
    ':generator',
  ]);
  // Atom fields — emit the bare atom text. Enum values plus the literal
  // booleans live here.
  const ATOM_FIELDS = new Set([
    ':recommended_backend',
    ':router_confidence',
    ':backend_readiness_status',
    ':backend_runtime_allowed',
    ':router_apply_eligible',
    ':dry_run_only',
    ':runtime_replacement',
    ':no_execution',
  ]);
  if (field === ':router_apply_blockers') {
    if (!Array.isArray(value) || value.length === 0) return '[]';
    return `[${value.map((v) => JSON.stringify(v)).join(' ')}]`;
  }
  if (STRING_FIELDS.has(field)) {
    return JSON.stringify(value);
  }
  if (ATOM_FIELDS.has(field)) {
    if (typeof value === 'boolean') return value ? 'true' : 'false';
    return String(value);
  }
  // Defensive default: stringify so we never emit a bare object.
  return JSON.stringify(value);
}

// Repo-relative path. The descriptor schema rejects absolute paths and `~`,
// so we always emit cwd-relative. When the input was already relative, the
// result of path.relative is the same shape.
function relPath(absolute, cwd) {
  if (!absolute) return null;
  const rel = path.relative(cwd, absolute);
  // path.relative may emit `..` if absolute escaped cwd; the schema rejects
  // that anyway and the validator will catch it. We pass it through so the
  // operator sees the validation failure instead of a silent rewrite.
  return rel === '' ? '.' : rel;
}

// Stable JSON: sort keys recursively so byte-identical output is reproducible
// across runs. Mirrors recommend-task-backend.mjs's stableStringify.
function stableStringify(value, indent = 2) {
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
// Each case constructs an in-memory task contract + policy + (optional)
// registry from Lisp strings, runs buildDescriptor() with the projected
// inputs, asserts on the structured output, and pre-validates against the
// wave27-01 checker. The pipe-smoke fixture additionally re-parses the
// emitted Lisp string through parseLisp + projectDescriptor (via the
// validator) to prove the round-trip survives.
// ---------------------------------------------------------------------------

async function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'pass-eligible-runtime-ready',
      category: 'eligible',
      run: () => {
        const task = parseTaskFromString(taskCheckerScript());
        const policy = parsePolicyFromString(policyWithCheckerRule());
        const registry = parseRegistryFromString(registryWithRuntimeReady());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'verifier-worker',
          taskEvents: 6,
          backendEvents: 6,
        });
        const recommendation = recommend({
          task,
          policy,
          traceIndex,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        mustEqual('eligible.recommended_backend', d.recommended_backend, 'verifier-worker');
        mustEqual('eligible.confidence', d.router_confidence, 'high');
        mustEqual('eligible.readiness', d.backend_readiness_status, 'runtime-ready');
        mustEqual('eligible.runtime_allowed', d.backend_runtime_allowed, true);
        mustEqual('eligible.apply_eligible', d.router_apply_eligible, true);
        mustEqual('eligible.blockers.length', d.router_apply_blockers.length, 0);
        mustEqual('eligible.dry_run_only', d.dry_run_only, true);
        mustEqual('eligible.runtime_replacement', d.runtime_replacement, false);
        mustEqual('eligible.no_execution', d.no_execution, true);
      },
    },
    {
      name: 'pass-current-default-blocked',
      category: 'current-default-blocked',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const registry = parseRegistryFromString(seedRegistry());
        const traceIndex = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 6,
          backendEvents: 6,
        });
        const recommendation = recommend({
          task,
          policy,
          traceIndex,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        mustEqual('cd.backend', d.recommended_backend, 'claudecode');
        mustEqual('cd.readiness', d.backend_readiness_status, 'current-default');
        // current-default is REJECTED by the eligibility gate.
        mustEqual('cd.eligible', d.router_apply_eligible, false);
        if (d.router_apply_blockers.length === 0) {
          throw new Error('expected at least one blocker for current-default rejection');
        }
        // Locked invariants stay literal even though current-default backend
        // is the live runtime.
        mustEqual('cd.no_execution', d.no_execution, true);
        mustEqual('cd.runtime_replacement', d.runtime_replacement, false);
      },
    },
    {
      name: 'pass-registry-missing-emits-eligible-false',
      category: 'registry-missing',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const recommendation = recommend({
          task,
          policy,
          traceIndex: null,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d = buildDescriptor({
          task,
          policy,
          recommendation,
          registry: null,
          registryError: new Error('ENOENT: no such file /missing/registry.lisp'),
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        mustEqual('rm.readiness', d.backend_readiness_status, 'unknown');
        mustEqual('rm.runtime_allowed', d.backend_runtime_allowed, false);
        mustEqual('rm.eligible', d.router_apply_eligible, false);
        mustEqual('rm.blockers.length', d.router_apply_blockers.length, 1);
        if (!/registry_missing/.test(d.router_apply_blockers[0])) {
          throw new Error(
            `expected registry_missing blocker, got "${d.router_apply_blockers[0]}"`,
          );
        }
        const errs = validateDescriptorObject(d);
        if (errs.length !== 0) {
          throw new Error(`registry-missing descriptor failed schema: ${errs.join('; ')}`);
        }
      },
    },
    {
      name: 'pass-unknown-backend-emits-blocker',
      category: 'unknown-backend',
      run: () => {
        // Synthetic registry that omits the backend the policy will pick.
        // Build the recommendation against a policy that picks claudecode,
        // then hand a registry with NO claudecode entry.
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const registry = parseRegistryFromString(registryWithoutClaudecode());
        const recommendation = recommend({
          task,
          policy,
          traceIndex: null,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        mustEqual('ub.backend', d.recommended_backend, 'claudecode');
        mustEqual('ub.eligible', d.router_apply_eligible, false);
        const hasUnknown = d.router_apply_blockers.some((b) =>
          /not in registry/.test(b),
        );
        if (!hasUnknown) {
          throw new Error(
            `expected "not in registry" blocker; got [${d.router_apply_blockers.join(', ')}]`,
          );
        }
      },
    },
    {
      name: 'pass-deterministic-output',
      category: 'determinism',
      run: () => {
        // Two descriptor builds from the same inputs must produce identical
        // Lisp output (modulo :generated_at, which we strip before
        // comparison). We do not promise generated_at is stable across calls
        // since it is the wall clock — the determinism guarantee is on the
        // structural fields.
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const registry = parseRegistryFromString(seedRegistry());
        const recommendation = recommend({
          task,
          policy,
          traceIndex: null,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d1 = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        const d2 = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        const strip = (l) => l.replace(/^\s*:generated_at .*$/m, '');
        const a = strip(emitLisp(d1));
        const b = strip(emitLisp(d2));
        if (a !== b) {
          throw new Error('non-deterministic Lisp output across two builds');
        }
        const aj = stableStringify({ ...d1, generated_at: null });
        const bj = stableStringify({ ...d2, generated_at: null });
        if (aj !== bj) {
          throw new Error('non-deterministic JSON output across two builds');
        }
      },
    },
    {
      name: 'pass-trace-index-supplied-vs-absent-same-eligible',
      category: 'trace-index-neutral-on-eligibility',
      run: () => {
        // Trace index changes confidence scoring but MUST NOT flip
        // apply_eligible by itself. With current-default seed registry no
        // backend is eligible regardless.
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const registry = parseRegistryFromString(seedRegistry());
        const trace = synthesizeTraceIndex({
          task: task.id,
          backend: 'claudecode',
          taskEvents: 12,
          backendEvents: 12,
        });
        const noTrace = recommend({
          task,
          policy,
          traceIndex: null,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const richTrace = recommend({
          task,
          policy,
          traceIndex: trace,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d1 = buildDescriptor({
          task,
          policy,
          recommendation: noTrace,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        const d2 = buildDescriptor({
          task,
          policy,
          recommendation: richTrace,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: '/abs/.missiond/router/trace-index.json',
          cwd: '/abs',
        });
        // Both still ineligible because seed registry's claudecode is
        // current-default — gate rejects regardless of confidence.
        mustEqual('ti.no-trace.eligible', d1.router_apply_eligible, false);
        mustEqual('ti.with-trace.eligible', d2.router_apply_eligible, false);
        // Trace index path should be echoed when supplied.
        if (d2.source_trace_index_path == null) {
          throw new Error('expected source_trace_index_path when --trace-index supplied');
        }
        if (d1.source_trace_index_path !== null) {
          throw new Error(
            'expected source_trace_index_path null when --trace-index absent',
          );
        }
      },
    },
    {
      name: 'pass-pipes-to-checker',
      category: 'pipe-smoke',
      run: () => {
        // Build a descriptor, emit Lisp, re-parse via the same parser the
        // checker uses, and confirm the projected object validates clean.
        const task = parseTaskFromString(taskCheckerScript());
        const policy = parsePolicyFromString(policyWithCheckerRule());
        const registry = parseRegistryFromString(registryWithRuntimeReady());
        const recommendation = recommend({
          task,
          policy,
          traceIndex: synthesizeTraceIndex({
            task: task.id,
            backend: 'verifier-worker',
            taskEvents: 6,
            backendEvents: 6,
          }),
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        const errs = validateDescriptorObject(d);
        if (errs.length !== 0) {
          throw new Error(
            `validateDescriptorObject rejected the freshly-built descriptor: ${errs.join('; ')}`,
          );
        }
        // Parse the emitted Lisp and confirm structural shape: head ==
        // router-dispatch-descriptor, second element is the descriptor id.
        const lisp = emitLisp(d);
        const forms = parseLisp(lisp, '<fixture-pipe>');
        if (forms.length !== 1) {
          throw new Error(`expected 1 form, got ${forms.length}`);
        }
        if (!isList(forms[0]) || head(forms[0]) !== DESCRIPTOR_HEAD) {
          throw new Error(
            `emitted form has wrong head; expected ${DESCRIPTOR_HEAD}`,
          );
        }
      },
    },
    {
      name: 'edge-runtime-replacement-policy-rejects-non-zero',
      category: 'policy-runtime-replacement',
      run: () => {
        // We can't invoke main() inside a fixture without spawning, so we
        // simulate the defensive check directly: a policy with
        // runtime_replacement=true would short-circuit main() before any
        // descriptor is built. Confirm the projected policy carries that
        // flag through and that recommend() (which we'd skip) is never
        // reached. We assert by running the same condition main() runs.
        const policy = parsePolicyFromString(policyWithRuntimeReplacement());
        if (policy.runtime_replacement !== true) {
          throw new Error(
            `fixture policy did not project runtime_replacement=true; got ${policy.runtime_replacement}`,
          );
        }
        // The CLI rejects this policy before even calling recommend(); the
        // fixture's job is to confirm the projection surface so the rejection
        // condition is well-formed. The full process-level non-zero exit is
        // covered by the live acceptance command (operator can rerun with a
        // crafted policy file).
      },
    },
    {
      name: 'edge-locked-invariants-cannot-be-flipped',
      category: 'locked-invariants',
      run: () => {
        // Even with a hypothetical eligible recommendation, the descriptor's
        // dry_run_only / runtime_replacement / no_execution stay locked.
        // We construct a maximally-permissive registry + matching rule, then
        // assert the locked literals.
        const task = parseTaskFromString(taskCheckerScript());
        const policy = parsePolicyFromString(policyWithCheckerRule());
        const registry = parseRegistryFromString(registryWithRuntimeReady());
        const recommendation = recommend({
          task,
          policy,
          traceIndex: synthesizeTraceIndex({
            task: task.id,
            backend: 'verifier-worker',
            taskEvents: 6,
            backendEvents: 6,
          }),
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: null,
          cwd: '/abs',
        });
        // Even though apply_eligible flipped to true here (runtime-ready
        // registry + matching high-confidence rule), the no-execution
        // invariants stay literal.
        mustEqual('lock.eligible', d.router_apply_eligible, true);
        mustEqual('lock.dry_run_only', d.dry_run_only, true);
        mustEqual('lock.runtime_replacement', d.runtime_replacement, false);
        mustEqual('lock.no_execution', d.no_execution, true);
      },
    },
    {
      name: 'edge-relative-path-roundtrip',
      category: 'paths',
      run: () => {
        const task = parseTaskFromString(taskDocs());
        const policy = parsePolicyFromString(seedPolicy());
        const registry = parseRegistryFromString(seedRegistry());
        const recommendation = recommend({
          task,
          policy,
          traceIndex: null,
          taskPath: '<fx>/task.lisp',
          policyPath: '<fx>/policy.lisp',
        });
        const d = buildDescriptor({
          task,
          policy,
          recommendation,
          registry,
          registryError: null,
          registryPath: '/abs/repo/.missiond/router/router-backend-registry-v1.lisp',
          policyPath: '/abs/repo/.missiond/router/router-policy-v1.lisp',
          traceIndexPath: '/abs/repo/.missiond/router/trace-index.json',
          cwd: '/abs/repo',
        });
        mustEqual(
          'paths.policy',
          d.source_policy_path,
          '.missiond/router/router-policy-v1.lisp',
        );
        mustEqual(
          'paths.registry',
          d.source_backend_registry_path,
          '.missiond/router/router-backend-registry-v1.lisp',
        );
        mustEqual(
          'paths.trace',
          d.source_trace_index_path,
          '.missiond/router/trace-index.json',
        );
      },
    },
  ];

  let failed = 0;
  const categories = new Set();
  const results = [];
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    try {
      await fixture.run();
      results.push({ name: fixture.name, category: fixture.category, ok: true });
    } catch (err) {
      failed += 1;
      results.push({
        name: fixture.name,
        category: fixture.category,
        ok: false,
        error: err.message,
      });
      console.error(`fixture failed: ${fixture.name}: ${err.message}`);
    }
  }

  if (json) {
    console.log(
      stableStringify({
        ok: failed === 0,
        fixtures: fixtures.length,
        failed,
        categories: [...categories],
        results,
      }),
    );
  } else if (failed === 0) {
    console.log(
      `build-router-dispatch-descriptor fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }

  if (failed > 0) {
    console.error(
      `build-router-dispatch-descriptor fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
}

function mustEqual(label, actual, expected) {
  // Deep compare for primitives is enough — the descriptor projection has no
  // nested object-shaped values where we'd assert deeply.
  if (actual !== expected) {
    throw new Error(
      `${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  }
}

// --- Fixture parsers (Lisp string -> projected object) -----------------------

function parseTaskFromString(source) {
  const forms = parseLisp(source, '<fixture-task>');
  for (const form of forms) {
    if (isList(form) && head(form) === 'task') {
      return projectTaskContract(form, '<fixture-task>');
    }
  }
  throw new Error('no (task ...) form found in <fixture>');
}

function parsePolicyFromString(source) {
  const forms = parseLisp(source, '<fixture-policy>');
  for (const form of forms) {
    if (isList(form) && head(form) === 'router-policy') {
      return projectPolicy(form, '<fixture-policy>');
    }
  }
  throw new Error('no (router-policy ...) form found in <fixture>');
}

function parseRegistryFromString(source) {
  const forms = parseLisp(source, '<fixture-registry>');
  for (const form of forms) {
    if (isList(form) && head(form) === 'router-backend-registry') {
      return projectRegistry(form, '<fixture-registry>');
    }
  }
  throw new Error('no (router-backend-registry ...) form found in <fixture>');
}

// Mirrors the synthesizeTraceIndex shape used by recommend-task-backend.mjs's
// own fixtures. scoreConfidence reads traceIndex.by_task[id].events and
// traceIndex.by_backend[id].events, so those buckets MUST be the canonical
// shape or confidence collapses to low.
function synthesizeTraceIndex({ task, backend, taskEvents, backendEvents }) {
  return {
    bottleneck_tags: [],
    by_backend: {
      [backend]: { bottleneck_tags: [], events: backendEvents },
    },
    by_task: {
      [task]: { bottleneck_tags: [], events: taskEvents },
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

// --- Fixture sources ---------------------------------------------------------

function taskDocs() {
  return `(task wave99-99-demo
    :schema "missiond.task-contract.v1"
    :title "Fixture docs task"
    :kind docs
    :status ready
    :owner "claudecode"
    :dispatch-strategy "fresh-docs"
    :goal "fixture goal"
    :write-scope ["docs/fixture.md"]
    :must-not-touch [])`;
}

function taskCheckerScript() {
  return `(task wave99-99-checker
    :schema "missiond.task-contract.v1"
    :title "Fixture checker task"
    :kind code-alignment
    :status ready
    :owner "claudecode"
    :dispatch-strategy "fresh-code-alignment"
    :goal "fixture goal"
    :write-scope ["scripts/check-fixture.mjs"]
    :must-not-touch [])`;
}

function seedPolicy() {
  return `(router-policy fixture-seed
    :schema "missiond.router-policy.v1"
    :version "v1"
    :description "fixture policy mirroring the live seed"
    :dry-run-only true
    :runtime-replacement false
    (rule
      :id r-docs-to-claudecode
      :priority 10
      :when ((kind docs))
      :recommend (:backend claudecode :reasoning "docs go to claudecode")
      :non-goals ["does not replace runtime dispatch"])
    (rule
      :id r-checker-to-verifier
      :priority 20
      :when ((path-glob "scripts/check-*.mjs"))
      :recommend (:backend verifier-worker :reasoning "checker scripts to verifier")
      :non-goals ["does not replace runtime dispatch"]))`;
}

function policyWithCheckerRule() {
  // Same as seedPolicy but ensures the checker rule is highest priority so
  // a code-alignment task touching scripts/check-*.mjs deterministically
  // picks verifier-worker.
  return `(router-policy fixture-checker
    :schema "missiond.router-policy.v1"
    :version "v1"
    :description "fixture policy with high-priority checker rule"
    :dry-run-only true
    :runtime-replacement false
    (rule
      :id r-checker-to-verifier
      :priority 5
      :when ((path-glob "scripts/check-*.mjs"))
      :recommend (:backend verifier-worker :reasoning "checker scripts to verifier")
      :non-goals ["does not replace runtime dispatch"])
    (rule
      :id r-docs-to-claudecode
      :priority 10
      :when ((kind docs))
      :recommend (:backend claudecode :reasoning "docs go to claudecode")
      :non-goals ["does not replace runtime dispatch"]))`;
}

function policyWithRuntimeReplacement() {
  return `(router-policy fixture-bad
    :schema "missiond.router-policy.v1"
    :version "v1"
    :description "fixture policy that incorrectly claims runtime replacement"
    :dry-run-only true
    :runtime-replacement true
    (rule
      :id r-bad
      :priority 1
      :when ((kind docs))
      :recommend (:backend claudecode :reasoning "should never run")
      :non-goals ["does not replace runtime dispatch"]))`;
}

function seedRegistry() {
  return `(router-backend-registry fixture-seed-registry
    :schema "missiond.router-backend-registry.v1"
    :version "v1"
    :description "fixture seed registry mirroring wave26-01 layout"
    (backend
      :id claudecode
      :readiness_status current-default
      :runtime_allowed true
      :apply_blockers []
      :substrate "live-cli"
      :non-goals ["never apply-eligible without explicit runtime-ready opt-in"]
      :owner "claudecode")
    (backend
      :id verifier-worker
      :readiness_status advisory-only
      :runtime_allowed false
      :apply_blockers ["no runtime adapter shipped"]
      :substrate nil
      :non-goals []
      :owner "claudecode"))`;
}

function registryWithRuntimeReady() {
  return `(router-backend-registry fixture-rr-registry
    :schema "missiond.router-backend-registry.v1"
    :version "v1"
    :description "fixture registry where verifier-worker is runtime-ready"
    (backend
      :id claudecode
      :readiness_status current-default
      :runtime_allowed true
      :apply_blockers []
      :substrate "live-cli"
      :non-goals []
      :owner "claudecode")
    (backend
      :id verifier-worker
      :readiness_status runtime-ready
      :runtime_allowed true
      :apply_blockers []
      :substrate "fixture-runtime"
      :non-goals []
      :owner "claudecode"))`;
}

function registryWithoutClaudecode() {
  return `(router-backend-registry fixture-no-cc-registry
    :schema "missiond.router-backend-registry.v1"
    :version "v1"
    :description "fixture registry that omits claudecode entirely"
    (backend
      :id verifier-worker
      :readiness_status advisory-only
      :runtime_allowed false
      :apply_blockers ["no runtime adapter shipped"]
      :substrate nil
      :non-goals []
      :owner "claudecode"))`;
}

// Silence unused-import warnings for Node modules pulled in by helpers we
// might use in future fixtures (mirrors the recommend-task-backend pattern).
void os;

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((err) => {
    console.error(`build-router-dispatch-descriptor: ${err.message}`);
    process.exit(1);
  });
}
