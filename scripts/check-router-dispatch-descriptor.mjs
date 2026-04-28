#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import {
  head,
  isList,
  keywordPropText,
  nodeText,
  parseLisp,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/check-router-dispatch-descriptor.mjs [--json] [--stdin] [--dry-fixture] [<descriptor.lisp> ...]

Checks MissionD router-dispatch-descriptor v1 Lisp records:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates each (router-dispatch-descriptor <id> ...) entry: required
    :schema :task_id :recommended_backend :router_confidence
    :backend_readiness_status :backend_runtime_allowed
    :router_apply_eligible :router_apply_blockers :dry_run_only
    :runtime_replacement :no_execution :source_recommendation_schema
    :source_policy_path :source_backend_registry_path; optional
    :source_trace_index_path :notes :generated_at :generator
  - rejects: schema mismatch; unknown / missing / unknown-extra fields;
    backend / readiness / confidence enum violations; non-literal-atom
    booleans (strings "true"/"false" rejected); locked-invariant
    violations (:dry_run_only must be literal true; :runtime_replacement
    must be literal false; :no_execution must be literal true);
    eligibility-gate violations (router_apply_eligible true requires
    backend_readiness_status=runtime-ready, backend_runtime_allowed=true
    literal, router_confidence=high, AND empty router_apply_blockers);
    router_apply_eligible=false with empty router_apply_blockers;
    absolute / ~ / .. paths in any source path field; empty strings in
    router_apply_blockers; unknown entry head.

Cross-wave invariants enforced: descriptors are a HANDOFF record, not a
runtime backend switch. The three locked invariants (:dry_run_only true,
:runtime_replacement false, :no_execution true) cannot be relaxed in v1.
The eligibility-gate intentionally REJECTS current-default → eligible:
explicit runtime-ready opt-in is required upstream.

Use --stdin to read a descriptor record from stdin (handy for piping the
wave27-02 CLI output through the checker without a temp file).

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.router-dispatch-descriptor.v1';

const ID_RE = /^[a-z0-9][a-z0-9._-]*$/;

export const DESCRIPTOR_HEAD = 'router-dispatch-descriptor';

// Backend ids — MUST stay aligned with router-policy v1 BACKEND_CLASSES
// (scripts/check-router-policy.mjs) AND router-backend-registry v1
// BACKEND_IDS (scripts/check-router-backend-registry.mjs). Drift between
// any of those enums is a structural error.
export const BACKEND_IDS = new Set([
  'claudecode',
  'missiond-llm-router',
  'deterministic-checker',
  'patch-worker',
  'verifier-worker',
]);

export const CONFIDENCE_LEVELS = new Set(['high', 'medium', 'low']);

// Readiness statuses — the 4 from router-backend-registry v1 plus the
// synthetic `unknown` fallback wave26-02/03 emit when the backend lookup
// against the registry fails. `unknown` is never apply-eligible.
export const READINESS_STATUSES = new Set([
  'current-default',
  'advisory-only',
  'runtime-ready',
  'unavailable',
  'unknown',
]);

// Only `runtime-ready` qualifies a descriptor for :router_apply_eligible
// true. current-default is REJECTED — explicit runtime-ready opt-in is
// required upstream so a descriptor cannot be silently promoted to live
// dispatch by a backend whose adapter is the live default but whose
// readiness has not been opt-in confirmed.
export const APPLY_ELIGIBLE_STATUSES = new Set(['runtime-ready']);

const DESCRIPTOR_REQUIRED = [
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
];

const DESCRIPTOR_OPTIONAL = [
  ':source_trace_index_path',
  ':notes',
  ':generated_at',
  ':generator',
];

const DESCRIPTOR_ALLOWED = new Set([...DESCRIPTOR_REQUIRED, ...DESCRIPTOR_OPTIONAL]);

const PATH_FIELDS = [
  ':source_policy_path',
  ':source_backend_registry_path',
  ':source_trace_index_path',
];

// Locked invariants: each MUST be the listed literal atom value. The
// checker rejects anything else (including string forms like "true").
const LOCKED_INVARIANTS = [
  { field: ':dry_run_only', expected: 'true' },
  { field: ':runtime_replacement', expected: 'false' },
  { field: ':no_execution', expected: 'true' },
];

const FREE_BOOL_FIELDS = [':backend_runtime_allowed', ':router_apply_eligible'];

const ISO_8601_RE =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$/;

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  let useStdin = false;
  const inputs = [];

  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else if (arg === '--stdin') {
      useStdin = true;
    } else {
      inputs.push(arg);
    }
  }

  if (dryFixture) {
    runFixtures(json);
    return;
  }

  if (!useStdin && inputs.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const sources = [];

  if (useStdin) {
    const stdinText = fs.readFileSync(0, 'utf8');
    sources.push({ file: '<stdin>', text: stdinText });
  }

  for (const input of inputs) {
    const resolved = path.resolve(cwd, input);
    sources.push({ file: resolved, text: null });
  }

  const seen = new Set();
  const diagnostics = [];
  let descriptorsValidated = 0;

  for (const src of sources) {
    if (src.file !== '<stdin>') {
      if (seen.has(src.file)) continue;
      seen.add(src.file);
    }
    let forms;
    try {
      forms = src.text != null ? parseLisp(src.text, src.file) : readLispFile(src.file);
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file: src.file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
      continue;
    }
    for (const form of forms) {
      if (!isList(form)) continue;
      const h = head(form);
      if (h !== DESCRIPTOR_HEAD) {
        addError(
          diagnostics,
          src.file,
          form.loc,
          `unknown entry head "${h}"; expected "${DESCRIPTOR_HEAD}"`,
        );
        continue;
      }
      validateDescriptor(src.file, form, diagnostics);
      descriptorsValidated += 1;
    }
  }

  const errors = diagnostics.filter((d) => d.severity === 'error');
  const ok = errors.length === 0;

  if (json) {
    console.log(
      JSON.stringify(
        {
          ok,
          files: sources.length,
          descriptors_validated: descriptorsValidated,
          errors,
        },
        null,
        2,
      ),
    );
  } else if (ok) {
    console.log(
      `router-dispatch-descriptor check OK (${descriptorsValidated} descriptor${descriptorsValidated === 1 ? '' : 's'})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `router-dispatch-descriptor check FAILED — ${errors.length} error(s), ${descriptorsValidated} descriptor(s)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function validateDescriptor(file, descriptor, diagnostics) {
  const descriptorId = nodeText(descriptor.children[1]);
  if (!descriptorId) {
    addError(
      diagnostics,
      file,
      descriptor.loc,
      '(router-dispatch-descriptor ...) missing descriptor id as second form',
    );
  } else if (!ID_RE.test(descriptorId)) {
    addError(
      diagnostics,
      file,
      descriptor.children[1].loc,
      `descriptor id "${descriptorId}" must match ${ID_RE}`,
    );
  }

  const props = readKeywordProps(descriptor, { start: 2 });

  for (const field of DESCRIPTOR_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        descriptor.loc,
        `(${DESCRIPTOR_HEAD} ${descriptorId ?? '<missing>'} ...) missing required field ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!DESCRIPTOR_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${DESCRIPTOR_HEAD} ...) has unknown field ${key}`,
      );
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (props[':schema'] && schema !== SCHEMA) {
    addError(
      diagnostics,
      file,
      props[':schema'].value?.loc ?? descriptor.loc,
      `:schema must be "${SCHEMA}", got ${JSON.stringify(schema)}`,
    );
  }

  validateNonEmptyString(file, props, ':task_id', descriptor, diagnostics);
  validateNonEmptyString(file, props, ':source_recommendation_schema', descriptor, diagnostics);

  const recommendedBackend = keywordPropText(props, ':recommended_backend');
  if (props[':recommended_backend'] && (recommendedBackend == null || !BACKEND_IDS.has(recommendedBackend))) {
    addError(
      diagnostics,
      file,
      props[':recommended_backend'].value?.loc ?? descriptor.loc,
      `:recommended_backend "${recommendedBackend}" not in enum {${[...BACKEND_IDS].join('|')}}`,
    );
  }

  const confidence = keywordPropText(props, ':router_confidence');
  if (props[':router_confidence'] && (confidence == null || !CONFIDENCE_LEVELS.has(confidence))) {
    addError(
      diagnostics,
      file,
      props[':router_confidence'].value?.loc ?? descriptor.loc,
      `:router_confidence "${confidence}" not in enum {${[...CONFIDENCE_LEVELS].join('|')}}`,
    );
  }

  const readiness = keywordPropText(props, ':backend_readiness_status');
  if (props[':backend_readiness_status'] && (readiness == null || !READINESS_STATUSES.has(readiness))) {
    addError(
      diagnostics,
      file,
      props[':backend_readiness_status'].value?.loc ?? descriptor.loc,
      `:backend_readiness_status "${readiness}" not in enum {${[...READINESS_STATUSES].join('|')}}`,
    );
  }

  // Locked invariants — strict literal-atom checks. Mirrors the wave25-02
  // validateRouterLiteralBool pattern; rejects "true"/"false" string forms
  // and any other atom value than the single legal one.
  for (const { field, expected } of LOCKED_INVARIANTS) {
    if (!props[field]) continue;
    const node = props[field].value;
    if (!node || node.type !== 'atom' || node.value !== expected) {
      const got = describeBoolNode(node);
      addError(
        diagnostics,
        file,
        node?.loc ?? descriptor.loc,
        `${field} must be the literal atom ${expected} (locked cross-wave invariant), got ${got}`,
      );
    }
  }

  // Free-polarity literal booleans — accept either `true` or `false` atom,
  // reject string forms.
  for (const field of FREE_BOOL_FIELDS) {
    if (!props[field]) continue;
    const node = props[field].value;
    if (!node || node.type !== 'atom' || (node.value !== 'true' && node.value !== 'false')) {
      const got = describeBoolNode(node);
      addError(
        diagnostics,
        file,
        node?.loc ?? descriptor.loc,
        `${field} must be the literal atom true or false (cross-wave invariant — strings are rejected), got ${got}`,
      );
    }
  }

  // :router_apply_blockers — vector of non-empty strings. Reject any
  // entry that is not a non-empty string node.
  let blockersOk = false;
  let blockersCount = null;
  if (props[':router_apply_blockers']) {
    const node = props[':router_apply_blockers'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? descriptor.loc,
        ':router_apply_blockers must be a vector/list of non-empty strings (use [] for none)',
      );
    } else {
      let bad = false;
      for (const child of node.children) {
        const childText = nodeText(child);
        if (child.type !== 'string' || typeof childText !== 'string' || childText.trim() === '') {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            ':router_apply_blockers entries must be non-empty strings',
          );
          bad = true;
        }
      }
      if (!bad) blockersOk = true;
      blockersCount = node.children.length;
    }
  }

  // Path fields — repo-relative shape only (no '/', '~' prefix, no '..').
  for (const field of PATH_FIELDS) {
    if (!props[field]) continue;
    const node = props[field].value;
    const text = nodeText(node);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        node?.loc ?? descriptor.loc,
        `${field} must be a non-empty string when present`,
      );
      continue;
    }
    if (path.isAbsolute(text) || text.startsWith('~')) {
      addError(
        diagnostics,
        file,
        node?.loc ?? descriptor.loc,
        `${field} must be repo-relative (no leading '/' or '~'), got "${text}"`,
      );
      continue;
    }
    const segments = text.split(/[\\/]/);
    if (segments.some((seg) => seg === '..')) {
      addError(
        diagnostics,
        file,
        node?.loc ?? descriptor.loc,
        `${field} must not contain ".." traversal, got "${text}"`,
      );
    }
  }

  // Optional :notes / :generator non-empty when present.
  for (const field of [':notes', ':generator']) {
    if (!props[field]) continue;
    const text = keywordPropText(props, field);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[field].value?.loc ?? descriptor.loc,
        `${field} must be a non-empty string when present (omit the field if empty)`,
      );
    }
  }

  // Optional :generated_at — ISO-8601-ish.
  if (props[':generated_at']) {
    const text = keywordPropText(props, ':generated_at');
    if (text == null || !ISO_8601_RE.test(text)) {
      addError(
        diagnostics,
        file,
        props[':generated_at'].value?.loc ?? descriptor.loc,
        `:generated_at must be an ISO-8601 timestamp string, got ${JSON.stringify(text)}`,
      );
    }
  }

  // Eligibility gate cross-check. Only run when :router_apply_eligible
  // parsed cleanly (literal atom true/false) — schema-level errors above
  // already explain malformed cases.
  const eligibleNode = props[':router_apply_eligible']?.value;
  const runtimeAllowedNode = props[':backend_runtime_allowed']?.value;
  const eligibleAtom = eligibleNode?.type === 'atom' ? eligibleNode.value : null;
  const runtimeAllowedAtom = runtimeAllowedNode?.type === 'atom' ? runtimeAllowedNode.value : null;

  if (eligibleAtom === 'true') {
    // gate 1: readiness MUST be runtime-ready
    if (readiness != null && READINESS_STATUSES.has(readiness) && !APPLY_ELIGIBLE_STATUSES.has(readiness)) {
      addError(
        diagnostics,
        file,
        eligibleNode.loc,
        `:router_apply_eligible true requires :backend_readiness_status in {${[...APPLY_ELIGIBLE_STATUSES].join('|')}}; got "${readiness}" (current-default is REJECTED — explicit runtime-ready opt-in required)`,
      );
    }
    // gate 2: runtime_allowed MUST be literal true
    if (runtimeAllowedAtom != null && runtimeAllowedAtom !== 'true') {
      addError(
        diagnostics,
        file,
        eligibleNode.loc,
        `:router_apply_eligible true requires :backend_runtime_allowed to be literal true; got ${runtimeAllowedAtom}`,
      );
    }
    // gate 3: confidence MUST be high
    if (confidence != null && CONFIDENCE_LEVELS.has(confidence) && confidence !== 'high') {
      addError(
        diagnostics,
        file,
        eligibleNode.loc,
        `:router_apply_eligible true requires :router_confidence high; got "${confidence}"`,
      );
    }
    // gate 4: blockers MUST be empty
    if (blockersOk && blockersCount != null && blockersCount > 0) {
      addError(
        diagnostics,
        file,
        eligibleNode.loc,
        `:router_apply_eligible true requires empty :router_apply_blockers; got ${blockersCount} entr${blockersCount === 1 ? 'y' : 'ies'}`,
      );
    }
  } else if (eligibleAtom === 'false') {
    // When apply is rejected the descriptor MUST explain at least one
    // concrete reason. An empty blockers vector with eligible=false is a
    // templating bug — there is no signal in it.
    if (blockersOk && blockersCount != null && blockersCount === 0) {
      addError(
        diagnostics,
        file,
        props[':router_apply_blockers'].value.loc,
        ':router_apply_eligible false requires :router_apply_blockers to list at least one concrete reason promotion is rejected',
      );
    }
  }
}

function describeBoolNode(node) {
  if (node == null) return '<missing>';
  if (node.type === 'atom') return node.value;
  if (node.type === 'string') return `"${node.value}" (string — strings are rejected)`;
  return `<${node.type}>`;
}

function validateNonEmptyString(file, props, key, descriptor, diagnostics) {
  if (!props[key]) return;
  const node = props[key].value;
  const text = nodeText(node);
  if (text == null || text.trim() === '') {
    addError(
      diagnostics,
      file,
      node?.loc ?? descriptor.loc,
      `${key} must be a non-empty string`,
    );
  }
}

function addError(diagnostics, file, loc, message) {
  diagnostics.push({
    severity: 'error',
    file,
    line: loc?.line ?? 1,
    column: loc?.column ?? 1,
    message,
  });
}

// ---------------------------------------------------------------------------
// Named exports for downstream tooling (wave27-02 dispatch-descriptor CLI,
// wave27-03 plan surface, wave27-04 report-side surface, wave27-05
// renderer context). These helpers project an already-parsed descriptor
// form into a structured object so callers reason over descriptors without
// re-implementing the Lisp reader. The functions are READ-ONLY: parse,
// project, return — they do not mutate input nodes and never write files.
// ---------------------------------------------------------------------------

// Project a single (router-dispatch-descriptor ...) form into a structured
// object. Returns null when the form is not a descriptor. Returned shape:
//   { id, schema, task_id, recommended_backend, router_confidence,
//     backend_readiness_status, backend_runtime_allowed: boolean|null,
//     router_apply_eligible: boolean|null, router_apply_blockers: string[],
//     dry_run_only: boolean|null, runtime_replacement: boolean|null,
//     no_execution: boolean|null, source_recommendation_schema,
//     source_policy_path, source_backend_registry_path,
//     source_trace_index_path, notes, generated_at, generator,
//     loc: { line, column } }
// Boolean fields are null when the file does not declare a literal atom
// (the validator reports this upstream). String fields are null when
// missing.
export function projectDescriptor(form) {
  if (!isList(form) || head(form) !== DESCRIPTOR_HEAD) return null;
  const id = nodeText(form.children[1]);
  const props = readKeywordProps(form, { start: 2 });

  const blockersNode = props[':router_apply_blockers']?.value ?? null;
  const blockers = [];
  if (isList(blockersNode)) {
    for (const child of blockersNode.children) {
      const text = nodeText(child);
      if (text != null && text !== '') blockers.push(text);
    }
  }

  return {
    id,
    schema: keywordPropText(props, ':schema'),
    task_id: keywordPropText(props, ':task_id'),
    recommended_backend: keywordPropText(props, ':recommended_backend'),
    router_confidence: keywordPropText(props, ':router_confidence'),
    backend_readiness_status: keywordPropText(props, ':backend_readiness_status'),
    backend_runtime_allowed: literalBool(props[':backend_runtime_allowed']?.value),
    router_apply_eligible: literalBool(props[':router_apply_eligible']?.value),
    router_apply_blockers: blockers,
    dry_run_only: literalBool(props[':dry_run_only']?.value),
    runtime_replacement: literalBool(props[':runtime_replacement']?.value),
    no_execution: literalBool(props[':no_execution']?.value),
    source_recommendation_schema: keywordPropText(props, ':source_recommendation_schema'),
    source_policy_path: keywordPropText(props, ':source_policy_path'),
    source_backend_registry_path: keywordPropText(props, ':source_backend_registry_path'),
    source_trace_index_path: keywordPropText(props, ':source_trace_index_path') ?? null,
    notes: keywordPropText(props, ':notes') ?? null,
    generated_at: keywordPropText(props, ':generated_at') ?? null,
    generator: keywordPropText(props, ':generator') ?? null,
    loc: form.loc ?? null,
  };
}

function literalBool(node) {
  if (!node || node.type !== 'atom') return null;
  if (node.value === 'true') return true;
  if (node.value === 'false') return false;
  return null;
}

// Read a router-dispatch-descriptor Lisp file from disk and return all
// projected descriptor forms in source order. Throws (with file context)
// when the reader fails. Callers that need validation should run main() /
// validateDescriptorObject() separately.
export function readDispatchDescriptorFile(file) {
  const forms = readLispFile(file);
  const out = [];
  for (const form of forms) {
    const projected = projectDescriptor(form);
    if (projected) out.push(projected);
  }
  return out;
}

// Validate an already-projected descriptor object against the schema
// (re-uses the same enum / locked-invariant / eligibility-gate rules as
// the on-disk path). Returns an array of error message strings; empty
// array means valid. wave27-02 will call this on freshly-emitted CLI
// output before writing it anywhere.
export function validateDescriptorObject(obj) {
  const errors = [];
  if (!obj || typeof obj !== 'object') {
    errors.push('descriptor must be an object');
    return errors;
  }
  if (obj.schema !== SCHEMA) {
    errors.push(`:schema must be "${SCHEMA}", got ${JSON.stringify(obj.schema)}`);
  }
  if (!obj.task_id || String(obj.task_id).trim() === '') {
    errors.push(':task_id missing or empty');
  }
  if (!obj.source_recommendation_schema || String(obj.source_recommendation_schema).trim() === '') {
    errors.push(':source_recommendation_schema missing or empty');
  }
  if (!BACKEND_IDS.has(obj.recommended_backend)) {
    errors.push(
      `:recommended_backend "${obj.recommended_backend}" not in enum {${[...BACKEND_IDS].join('|')}}`,
    );
  }
  if (!CONFIDENCE_LEVELS.has(obj.router_confidence)) {
    errors.push(
      `:router_confidence "${obj.router_confidence}" not in enum {${[...CONFIDENCE_LEVELS].join('|')}}`,
    );
  }
  if (!READINESS_STATUSES.has(obj.backend_readiness_status)) {
    errors.push(
      `:backend_readiness_status "${obj.backend_readiness_status}" not in enum {${[...READINESS_STATUSES].join('|')}}`,
    );
  }
  if (typeof obj.backend_runtime_allowed !== 'boolean') {
    errors.push(':backend_runtime_allowed must be the literal atom true or false');
  }
  if (typeof obj.router_apply_eligible !== 'boolean') {
    errors.push(':router_apply_eligible must be the literal atom true or false');
  }
  if (obj.dry_run_only !== true) {
    errors.push(':dry_run_only must be the literal atom true (locked invariant)');
  }
  if (obj.runtime_replacement !== false) {
    errors.push(':runtime_replacement must be the literal atom false (locked invariant)');
  }
  if (obj.no_execution !== true) {
    errors.push(':no_execution must be the literal atom true (locked invariant)');
  }
  if (!Array.isArray(obj.router_apply_blockers)) {
    errors.push(':router_apply_blockers must be an array of non-empty strings');
  } else {
    for (const entry of obj.router_apply_blockers) {
      if (typeof entry !== 'string' || entry.trim() === '') {
        errors.push(':router_apply_blockers entries must be non-empty strings');
        break;
      }
    }
  }
  for (const field of ['source_policy_path', 'source_backend_registry_path']) {
    const text = obj[field];
    if (typeof text !== 'string' || text.trim() === '') {
      errors.push(`:${field} must be a non-empty string`);
      continue;
    }
    if (path.isAbsolute(text) || text.startsWith('~')) {
      errors.push(`:${field} must be repo-relative (no leading '/' or '~'), got "${text}"`);
    }
    if (text.split(/[\\/]/).some((seg) => seg === '..')) {
      errors.push(`:${field} must not contain ".." traversal, got "${text}"`);
    }
  }
  if (obj.source_trace_index_path != null) {
    const text = obj.source_trace_index_path;
    if (typeof text === 'string' && text.trim() !== '') {
      if (path.isAbsolute(text) || text.startsWith('~')) {
        errors.push(`:source_trace_index_path must be repo-relative, got "${text}"`);
      }
      if (text.split(/[\\/]/).some((seg) => seg === '..')) {
        errors.push(`:source_trace_index_path must not contain ".." traversal, got "${text}"`);
      }
    }
  }
  // Eligibility-gate cross-check.
  if (obj.router_apply_eligible === true) {
    if (!APPLY_ELIGIBLE_STATUSES.has(obj.backend_readiness_status)) {
      errors.push(
        `:router_apply_eligible true requires :backend_readiness_status in {${[...APPLY_ELIGIBLE_STATUSES].join('|')}}; got "${obj.backend_readiness_status}"`,
      );
    }
    if (obj.backend_runtime_allowed !== true) {
      errors.push(
        ':router_apply_eligible true requires :backend_runtime_allowed literal true',
      );
    }
    if (obj.router_confidence !== 'high') {
      errors.push(
        `:router_apply_eligible true requires :router_confidence high; got "${obj.router_confidence}"`,
      );
    }
    if (Array.isArray(obj.router_apply_blockers) && obj.router_apply_blockers.length > 0) {
      errors.push(
        ':router_apply_eligible true requires empty :router_apply_blockers',
      );
    }
  } else if (obj.router_apply_eligible === false) {
    if (Array.isArray(obj.router_apply_blockers) && obj.router_apply_blockers.length === 0) {
      errors.push(
        ':router_apply_eligible false requires :router_apply_blockers to list at least one concrete reason',
      );
    }
  }
  return errors;
}

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'pass-eligible-runtime-ready',
      category: 'pass',
      source: `(router-dispatch-descriptor d-eligible-rr
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend verifier-worker
        :router_confidence high
        :backend_readiness_status runtime-ready
        :backend_runtime_allowed true
        :router_apply_eligible true
        :router_apply_blockers []
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: true,
    },
    {
      name: 'pass-current-default-blocked',
      category: 'pass',
      source: `(router-dispatch-descriptor d-cd-blocked
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["current-default is the live runtime today; explicit runtime-ready opt-in required upstream"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: true,
    },
    {
      name: 'pass-advisory-only-blocked-with-optional-fields',
      category: 'pass',
      source: `(router-dispatch-descriptor d-advisory-blocked
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend missiond-llm-router
        :router_confidence medium
        :backend_readiness_status advisory-only
        :backend_runtime_allowed false
        :router_apply_eligible false
        :router_apply_blockers ["no runtime adapter shipped" "confidence below high"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"
        :source_trace_index_path ".missiond/router/trace-index-v1.lisp"
        :generated_at "2026-04-28T11:23:05+08:00"
        :generator "scripts/recommend-task-backend.mjs"
        :notes "advisory-only registry entry; future apply gate would refuse promotion")`,
      ok: true,
    },
    {
      name: 'fail-string-bool-eligible',
      category: 'fail-string-bool',
      source: `(router-dispatch-descriptor d-string-bool-elig
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible "false"
        :router_apply_blockers ["string bool not allowed"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-string-bool-runtime-allowed',
      category: 'fail-string-bool',
      source: `(router-dispatch-descriptor d-string-bool-rta
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed "true"
        :router_apply_eligible false
        :router_apply_blockers ["string bool not allowed"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-missing-no-execution',
      category: 'fail-missing-required',
      source: `(router-dispatch-descriptor d-missing-noexec
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["no_execution missing"]
        :dry_run_only true
        :runtime_replacement false
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-runtime-replacement-true',
      category: 'fail-locked-invariant',
      source: `(router-dispatch-descriptor d-replacement-true
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["locked invariant violation"]
        :dry_run_only true
        :runtime_replacement true
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-dry-run-only-false',
      category: 'fail-locked-invariant',
      source: `(router-dispatch-descriptor d-dryrun-false
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["locked invariant violation"]
        :dry_run_only false
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-current-default-eligible',
      category: 'fail-eligibility-gate',
      source: `(router-dispatch-descriptor d-cd-eligible
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible true
        :router_apply_blockers []
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-medium-confidence-eligible',
      category: 'fail-eligibility-gate',
      source: `(router-dispatch-descriptor d-medium-eligible
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend verifier-worker
        :router_confidence medium
        :backend_readiness_status runtime-ready
        :backend_runtime_allowed true
        :router_apply_eligible true
        :router_apply_blockers []
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-non-empty-blockers-eligible',
      category: 'fail-eligibility-gate',
      source: `(router-dispatch-descriptor d-blockers-eligible
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend verifier-worker
        :router_confidence high
        :backend_readiness_status runtime-ready
        :backend_runtime_allowed true
        :router_apply_eligible true
        :router_apply_blockers ["leftover blocker should reject eligible"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-runtime-allowed-false-eligible',
      category: 'fail-eligibility-gate',
      source: `(router-dispatch-descriptor d-rta-false-eligible
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend verifier-worker
        :router_confidence high
        :backend_readiness_status runtime-ready
        :backend_runtime_allowed false
        :router_apply_eligible true
        :router_apply_blockers []
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-eligible-false-empty-blockers',
      category: 'fail-eligibility-gate',
      source: `(router-dispatch-descriptor d-elig-false-empty
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend missiond-llm-router
        :router_confidence medium
        :backend_readiness_status advisory-only
        :backend_runtime_allowed false
        :router_apply_eligible false
        :router_apply_blockers []
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-unknown-backend',
      category: 'fail-enum',
      source: `(router-dispatch-descriptor d-bad-backend
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend gpt-5
        :router_confidence high
        :backend_readiness_status advisory-only
        :backend_runtime_allowed false
        :router_apply_eligible false
        :router_apply_blockers ["unknown backend"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-unknown-readiness',
      category: 'fail-enum',
      source: `(router-dispatch-descriptor d-bad-readiness
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status preview
        :backend_runtime_allowed false
        :router_apply_eligible false
        :router_apply_blockers ["unknown readiness"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-unknown-confidence',
      category: 'fail-enum',
      source: `(router-dispatch-descriptor d-bad-confidence
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence very-high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["unknown confidence"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-absolute-policy-path',
      category: 'fail-path',
      source: `(router-dispatch-descriptor d-abs-policy
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["absolute path rejected"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path "/etc/missiond/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-tilde-registry-path',
      category: 'fail-path',
      source: `(router-dispatch-descriptor d-tilde-registry
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["tilde path rejected"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path "~/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-traversal-trace-index-path',
      category: 'fail-path',
      source: `(router-dispatch-descriptor d-traversal-trace
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["traversal path rejected"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"
        :source_trace_index_path "../../etc/passwd")`,
      ok: false,
    },
    {
      name: 'fail-malformed-blocker-vector-empty-string',
      category: 'fail-blockers-shape',
      source: `(router-dispatch-descriptor d-empty-blocker
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend missiond-llm-router
        :router_confidence medium
        :backend_readiness_status advisory-only
        :backend_runtime_allowed false
        :router_apply_eligible false
        :router_apply_blockers ["valid reason" ""]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-missing-required-field-task-id',
      category: 'fail-missing-required',
      source: `(router-dispatch-descriptor d-missing-task-id
        :schema "${SCHEMA}"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["task_id missing"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
    {
      name: 'fail-unknown-entry-head',
      category: 'fail-unknown-entry-head',
      source: `(dispatch-descriptor d-wrong-head
        :schema "${SCHEMA}"
        :task_id "wave99-99-demo"
        :recommended_backend claudecode
        :router_confidence high
        :backend_readiness_status current-default
        :backend_runtime_allowed true
        :router_apply_eligible false
        :router_apply_blockers ["wrong head"]
        :dry_run_only true
        :runtime_replacement false
        :no_execution true
        :source_recommendation_schema "missiond.router-recommendation.v0"
        :source_policy_path ".missiond/router/router-policy-v1.lisp"
        :source_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp")`,
      ok: false,
    },
  ];

  let failed = 0;
  const categories = new Set();
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    const file = `<fixture:${fixture.name}>`;
    const diagnostics = [];
    let forms;
    try {
      forms = parseLisp(fixture.source, file);
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
      forms = [];
    }
    for (const form of forms) {
      if (!isList(form)) continue;
      const h = head(form);
      if (h !== DESCRIPTOR_HEAD) {
        addError(
          diagnostics,
          file,
          form.loc,
          `unknown entry head "${h}"; expected "${DESCRIPTOR_HEAD}"`,
        );
        continue;
      }
      validateDescriptor(file, form, diagnostics);
    }
    const ok = !diagnostics.some((d) => d.severity === 'error');
    if (ok !== fixture.ok) {
      failed += 1;
      console.error(
        `fixture failed: ${fixture.name} (expected ok=${fixture.ok}, got ok=${ok})`,
      );
      for (const d of diagnostics) {
        console.error(`  ${d.file}:${d.line}:${d.column}: ${d.message}`);
      }
    }
  }
  if (json) {
    console.log(
      JSON.stringify(
        {
          ok: failed === 0,
          fixtures: fixtures.length,
          failed,
          categories: [...categories],
        },
        null,
        2,
      ),
    );
  }
  if (failed > 0) {
    console.error(
      `router-dispatch-descriptor fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `router-dispatch-descriptor fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

// Only run the CLI when invoked directly. Keeps named exports clean for
// future tooling (wave27-02 emit CLI, wave27-03 plan surface, wave27-04
// report surface, wave27-05 renderer context, wave27-06 smoke harness).
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
