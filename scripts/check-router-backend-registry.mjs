#!/usr/bin/env node

import path from 'node:path';
import {
  head,
  isList,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/check-router-backend-registry.mjs [--json] [--dry-fixture] <registry.lisp...>

Checks MissionD router-backend-registry v1 Lisp files:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates the (router-backend-registry <id> ...) header: :schema, :version
  - validates each (backend ...) entry: required :id :readiness_status
    :runtime_allowed :apply_blockers :substrate :non-goals; optional
    :notes :owner :adapter_path
  - rejects: schema mismatch, unknown backend :id (not in the 5-element
    enum), duplicate :id within a file, :readiness_status not in the
    4-element enum, :runtime_allowed not literal true/false,
    :runtime_allowed true while :readiness_status is neither current-default
    nor runtime-ready, :readiness_status runtime-ready / current-default
    without a non-nil :substrate, :readiness_status advisory-only / unavailable
    with an empty :apply_blockers vector, missing :non-goals, unknown
    entry head, missing required field, unknown backend field

Cross-wave invariant enforced: only backends with :readiness_status
current-default or runtime-ready may declare :runtime_allowed true. Any
other combination is rejected. The checker validates structure only — it
does not consume the registry at runtime, does not promote any
recommendation, and does not verify the declared :substrate Rust module
actually exists.

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.router-backend-registry.v1';

const ID_RE = /^[a-z][a-z0-9-]*$/;

export const REGISTRY_HEAD = 'router-backend-registry';
export const BACKEND_HEAD = 'backend';

// Backend ids — MUST stay aligned with router-policy v1's BACKEND_CLASSES
// (scripts/check-router-policy.mjs). Drift between the two enums is a
// structural error; cross-checking is the recommendation CLI's job
// (wave26-02 / wave26-03), not this checker's.
export const BACKEND_IDS = new Set([
  'claudecode',
  'missiond-llm-router',
  'deterministic-checker',
  'patch-worker',
  'verifier-worker',
]);

export const READINESS_STATUSES = new Set([
  'current-default',
  'advisory-only',
  'runtime-ready',
  'unavailable',
]);

// Statuses for which :runtime_allowed true is permitted. Any backend
// declaring :runtime_allowed true outside this set is a structural error —
// this is the cross-wave invariant the checker exists to enforce.
export const RUNTIME_ALLOWED_STATUSES = new Set(['current-default', 'runtime-ready']);

const HEADER_REQUIRED = [':schema', ':version'];
const HEADER_OPTIONAL = new Set([':description']);
const HEADER_ALLOWED = new Set([...HEADER_REQUIRED, ...HEADER_OPTIONAL]);

const BACKEND_REQUIRED = [
  ':id',
  ':readiness_status',
  ':runtime_allowed',
  ':apply_blockers',
  ':substrate',
  ':non-goals',
];
const BACKEND_OPTIONAL = [':notes', ':owner', ':adapter_path'];
const BACKEND_ALLOWED = new Set([...BACKEND_REQUIRED, ...BACKEND_OPTIONAL]);

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  const inputs = [];

  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      inputs.push(arg);
    }
  }

  if (dryFixture) {
    runFixtures(json);
    return;
  }

  if (inputs.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const files = inputs.map((input) => path.resolve(cwd, input));

  const diagnostics = [];
  let registryCount = 0;
  let backendsValidated = 0;

  for (const file of [...new Set(files)]) {
    let forms;
    try {
      forms = readLispFile(file);
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
      continue;
    }
    for (const form of forms) {
      if (!isList(form) || head(form) !== REGISTRY_HEAD) continue;
      registryCount += 1;
      backendsValidated += validateRegistry(file, form, diagnostics);
    }
  }

  const errors = diagnostics.filter((d) => d.severity === 'error');
  const ok = errors.length === 0;

  if (json) {
    console.log(
      JSON.stringify(
        {
          ok,
          files: files.length,
          registries: registryCount,
          backends_validated: backendsValidated,
          errors,
        },
        null,
        2,
      ),
    );
  } else if (ok) {
    console.log(
      `router-backend-registry check OK (${registryCount} registr${registryCount === 1 ? 'y' : 'ies'}, ${backendsValidated} backend${backendsValidated === 1 ? '' : 's'})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `router-backend-registry check FAILED — ${errors.length} error(s), ${registryCount} registr${registryCount === 1 ? 'y' : 'ies'}, ${backendsValidated} backend(s)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function validateRegistry(file, registry, diagnostics) {
  const registryId = nodeText(registry.children[1]);
  if (!registryId) {
    addError(
      diagnostics,
      file,
      registry.loc,
      '(router-backend-registry ...) missing registry id as second form',
    );
  } else if (!ID_RE.test(registryId)) {
    addError(
      diagnostics,
      file,
      registry.children[1].loc,
      `registry id "${registryId}" must match ${ID_RE}`,
    );
  }

  const props = readKeywordProps(registry, { start: 2 });
  for (const field of HEADER_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        registry.loc,
        `(${REGISTRY_HEAD} ${registryId ?? '<missing>'} ...) missing required header ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!HEADER_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${REGISTRY_HEAD} ...) has unknown header ${key}`,
      );
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    addError(
      diagnostics,
      file,
      props[':schema']?.value?.loc ?? registry.loc,
      `:schema must be "${SCHEMA}", got ${JSON.stringify(schema)}`,
    );
  }

  // Reject any list child whose head is not `backend` (only `backend`
  // entries allowed at the top level). Header keywords are not lists so
  // they pass through.
  for (const child of registry.children) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== BACKEND_HEAD) {
      addError(
        diagnostics,
        file,
        child.loc,
        `unknown entry head "${h}"; expected "${BACKEND_HEAD}"`,
      );
    }
  }

  const backends = registry.children.filter((c) => isList(c) && head(c) === BACKEND_HEAD);
  const seenIds = new Map();
  for (const backend of backends) {
    const result = validateBackend(file, backend, diagnostics);
    if (result.id) {
      if (seenIds.has(result.id)) {
        addError(
          diagnostics,
          file,
          backend.loc,
          `duplicate backend :id "${result.id}" (also at line ${seenIds.get(result.id)})`,
        );
      } else {
        seenIds.set(result.id, backend.loc?.line ?? 1);
      }
    }
  }

  return backends.length;
}

function validateBackend(file, backend, diagnostics) {
  const props = readKeywordProps(backend, { start: 1 });

  for (const field of BACKEND_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        backend.loc,
        `(${BACKEND_HEAD} ...) missing required field ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!BACKEND_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${BACKEND_HEAD} ...) has unknown field ${key}`,
      );
    }
  }

  const id = keywordPropText(props, ':id');
  if (id != null && !BACKEND_IDS.has(id)) {
    addError(
      diagnostics,
      file,
      props[':id'].value.loc,
      `backend :id "${id}" not in enum {${[...BACKEND_IDS].join('|')}}`,
    );
  }

  const status = keywordPropText(props, ':readiness_status');
  if (status != null && !READINESS_STATUSES.has(status)) {
    addError(
      diagnostics,
      file,
      props[':readiness_status'].value.loc,
      `:readiness_status "${status}" not in enum {${[...READINESS_STATUSES].join('|')}}`,
    );
  }

  // :runtime_allowed must be the literal atom true or false. We require the
  // exact atoms (not yes/on) so the data contract is unambiguous and the
  // checker is the single source of truth for what the invariant looks
  // like on disk.
  let runtimeAllowed = null;
  if (props[':runtime_allowed']) {
    const text = nodeText(props[':runtime_allowed'].value);
    if (text !== 'true' && text !== 'false') {
      addError(
        diagnostics,
        file,
        props[':runtime_allowed'].value.loc,
        `:runtime_allowed must be literal true or false; got ${JSON.stringify(text)}`,
      );
    } else {
      runtimeAllowed = text === 'true';
    }
  }

  // Cross-wave invariant: :runtime_allowed true REQUIRES
  // :readiness_status in (current-default | runtime-ready). Any other
  // combination is rejected.
  if (
    runtimeAllowed === true &&
    status != null &&
    READINESS_STATUSES.has(status) &&
    !RUNTIME_ALLOWED_STATUSES.has(status)
  ) {
    addError(
      diagnostics,
      file,
      props[':runtime_allowed'].value.loc,
      `:runtime_allowed true is only permitted when :readiness_status is in {${[...RUNTIME_ALLOWED_STATUSES].join('|')}}; got status "${status}"`,
    );
  }

  validateSubstrate(file, props, diagnostics, backend, status);
  validateApplyBlockers(file, props, diagnostics, backend, status);
  validateNonGoals(file, props, diagnostics, backend);

  if (props[':notes']) {
    const notes = keywordPropText(props, ':notes');
    if (notes != null && notes.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':notes'].value.loc,
        'backend :notes must be a non-empty string when present (omit the field if empty)',
      );
    }
  }
  if (props[':owner']) {
    const owner = keywordPropText(props, ':owner');
    if (owner != null && owner.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':owner'].value.loc,
        'backend :owner must be a non-empty string when present (omit the field if empty)',
      );
    }
  }
  if (props[':adapter_path']) {
    const adapter = keywordPropText(props, ':adapter_path');
    if (adapter != null && adapter.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':adapter_path'].value.loc,
        'backend :adapter_path must be a non-empty string when present (omit the field if empty)',
      );
    }
  }

  return { id };
}

function validateSubstrate(file, props, diagnostics, backend, status) {
  if (!props[':substrate']) return; // missing-required already reported
  const node = props[':substrate'].value;
  const text = nodeText(node);
  // The substrate must be a string OR the literal atom `nil`. Accept both
  // shapes so authors can declare absent adapters explicitly.
  const isNil = node?.type === 'atom' && text === 'nil';
  const isString = node?.type === 'string' && typeof text === 'string';
  if (!isNil && !isString) {
    addError(
      diagnostics,
      file,
      node?.loc ?? backend.loc,
      `:substrate must be a non-empty string OR the literal atom nil; got ${JSON.stringify(text)}`,
    );
    return;
  }
  if (isString && text.trim() === '') {
    addError(
      diagnostics,
      file,
      node.loc,
      ':substrate string must be non-empty (use the literal atom nil to declare no adapter)',
    );
    return;
  }
  // Cross-wave invariant: a backend that claims runtime-ready or
  // current-default MUST name a concrete substrate so the apply gate can
  // route to it.
  if (status != null && RUNTIME_ALLOWED_STATUSES.has(status) && isNil) {
    addError(
      diagnostics,
      file,
      node.loc,
      `:readiness_status "${status}" requires a non-nil :substrate (the runtime adapter / surface to invoke)`,
    );
  }
}

function validateApplyBlockers(file, props, diagnostics, backend, status) {
  if (!props[':apply_blockers']) return;
  const node = props[':apply_blockers'].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? backend.loc,
      ':apply_blockers must be a vector/list of strings (use [] for none)',
    );
    return;
  }
  const items = nodeToStringArray(node);
  // Validate that every present entry is a non-empty string (catches stray
  // atoms like nil mistakenly placed in the blockers vector).
  for (const child of node.children) {
    const childText = nodeText(child);
    if (child.type !== 'string' || typeof childText !== 'string' || childText.trim() === '') {
      addError(
        diagnostics,
        file,
        child.loc ?? node.loc,
        ':apply_blockers entries must be non-empty strings',
      );
    }
  }
  // Cross-wave invariant: advisory-only / unavailable backends MUST list
  // at least one concrete blocker so reviewers can see WHY the backend is
  // not runtime-ready.
  if (
    status != null &&
    READINESS_STATUSES.has(status) &&
    !RUNTIME_ALLOWED_STATUSES.has(status) &&
    items.length === 0
  ) {
    addError(
      diagnostics,
      file,
      node.loc,
      `:readiness_status "${status}" requires a non-empty :apply_blockers vector (state at least one concrete reason promotion is rejected)`,
    );
  }
}

function validateNonGoals(file, props, diagnostics, backend) {
  if (!props[':non-goals']) return;
  const node = props[':non-goals'].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? backend.loc,
      'backend :non-goals must be a vector/list of strings (every backend must list at least one explicit non-goal)',
    );
    return;
  }
  const items = nodeToStringArray(node);
  if (items.length === 0) {
    addError(
      diagnostics,
      file,
      node.loc,
      'backend :non-goals must contain at least one entry — explicit non-goals are required so reviewers can audit drift toward live dispatch',
    );
    return;
  }
  for (const child of node.children) {
    const childText = nodeText(child);
    if (child.type !== 'string' || typeof childText !== 'string' || childText.trim() === '') {
      addError(
        diagnostics,
        file,
        child.loc ?? node.loc,
        'backend :non-goals entries must be non-empty strings',
      );
    }
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
// Named exports for downstream tooling (wave26-02 recommendation join,
// wave26-03 report-side surface). These helpers expose the already-validated
// structure of a backend-registry file as plain JavaScript objects so callers
// can reason over backends without re-implementing the Lisp reader. The
// functions are READ-ONLY: they parse, project, and return — they do not
// mutate input nodes and they never write files.
// ---------------------------------------------------------------------------

// Project a single (backend ...) form into a structured object suitable for
// the recommendation join CLI. Returned shape:
//   { id, readiness_status, runtime_allowed: boolean|null,
//     apply_blockers: string[], substrate: string|null,
//     non_goals: string[], notes: string|null, owner: string|null,
//     adapter_path: string|null, loc: { line, column } }
// `runtime_allowed` is null when the file does not declare it (the
// validator already rejects this upstream). `substrate` is null when the
// file declares the literal atom nil; otherwise a string.
export function projectBackend(backend) {
  if (!isList(backend) || head(backend) !== BACKEND_HEAD) return null;
  const props = readKeywordProps(backend, { start: 1 });
  const id = keywordPropText(props, ':id');
  const status = keywordPropText(props, ':readiness_status');
  const runtimeAllowedText = nodeText(props[':runtime_allowed']?.value);
  let runtimeAllowed = null;
  if (runtimeAllowedText === 'true') runtimeAllowed = true;
  else if (runtimeAllowedText === 'false') runtimeAllowed = false;

  const blockersNode = props[':apply_blockers']?.value ?? null;
  const blockers = blockersNode ? nodeToStringArray(blockersNode) : [];

  const substrateNode = props[':substrate']?.value ?? null;
  let substrate = null;
  if (substrateNode) {
    const txt = nodeText(substrateNode);
    if (substrateNode.type === 'string') substrate = txt;
    else if (substrateNode.type === 'atom' && txt !== 'nil') substrate = txt;
  }

  const nonGoalsNode = props[':non-goals']?.value ?? null;
  const nonGoals = nonGoalsNode ? nodeToStringArray(nonGoalsNode) : [];

  return {
    id,
    readiness_status: status,
    runtime_allowed: runtimeAllowed,
    apply_blockers: blockers,
    substrate,
    non_goals: nonGoals,
    notes: keywordPropText(props, ':notes') ?? null,
    owner: keywordPropText(props, ':owner') ?? null,
    adapter_path: keywordPropText(props, ':adapter_path') ?? null,
    loc: backend.loc ?? null,
  };
}

// Project an entire router-backend-registry form into a structured object:
//   { id, schema, version, description, backends: Backend[], source_path }
// Backends are returned in source order.
export function projectRegistry(form, sourcePath = null) {
  if (!isList(form) || head(form) !== REGISTRY_HEAD) return null;
  const id = nodeText(form.children[1]);
  const props = readKeywordProps(form, { start: 2 });
  const schema = keywordPropText(props, ':schema');
  const version = keywordPropText(props, ':version');
  const description = keywordPropText(props, ':description');
  const backends = form.children
    .filter((c) => isList(c) && head(c) === BACKEND_HEAD)
    .map((b) => projectBackend(b))
    .filter((b) => b !== null);
  return {
    id,
    schema,
    version,
    description: description ?? null,
    backends,
    source_path: sourcePath,
  };
}

// Read a router-backend-registry Lisp file from disk and return the FIRST
// projected registry form. Throws (with file context) when the reader
// fails or no router-backend-registry form is present. This helper is the
// simplest entry point for the recommendation join CLI: it doesn't
// validate, only projects — callers that need validation should run
// main() / runFixtures() separately.
export function readBackendRegistryFile(file) {
  const forms = readLispFile(file);
  for (const form of forms) {
    if (isList(form) && head(form) === REGISTRY_HEAD) {
      return projectRegistry(form, file);
    }
  }
  throw new Error(
    `router-backend-registry: no (router-backend-registry ...) form found in ${file}`,
  );
}

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'valid minimal registry (claudecode current-default only)',
      category: 'pass',
      source: `(router-backend-registry seed-min
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: true,
    },
    {
      name: 'all five backends with mixed statuses',
      category: 'pass',
      source: buildAllBackendsRegistry(),
      ok: true,
    },
    {
      name: 'runtime-ready backend with non-nil substrate',
      category: 'pass',
      source: `(router-backend-registry rr-ok
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id verifier-worker
          :readiness_status runtime-ready
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::verifier_adapter"
          :non-goals ["does not mutate the index"]))`,
      ok: true,
    },
    {
      name: 'advisory-only with optional :owner / :notes / :adapter_path',
      category: 'pass',
      source: `(router-backend-registry opt-fields
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id deterministic-checker
          :readiness_status advisory-only
          :runtime_allowed false
          :apply_blockers ["no runtime adapter shipped"]
          :substrate nil
          :owner "claudecode"
          :adapter_path "scripts/check-router-policy.mjs"
          :notes "future deterministic-checker worker would import the schema"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: true,
    },
    {
      name: 'wrong schema',
      category: 'fail-schema',
      source: `(router-backend-registry bad-schema
        :schema "missiond.router-backend-registry.v0"
        :version "v0"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'unknown backend :id (not in 5-enum)',
      category: 'fail-unknown-id',
      source: `(router-backend-registry bad-id
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id gpt-5
          :readiness_status advisory-only
          :runtime_allowed false
          :apply_blockers ["not in enum"]
          :substrate nil
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'duplicate backend :id within file',
      category: 'fail-duplicate-id',
      source: `(router-backend-registry dup-id
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"])
        (backend
          :id claudecode
          :readiness_status advisory-only
          :runtime_allowed false
          :apply_blockers ["duplicate"]
          :substrate nil
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'unknown :readiness_status',
      category: 'fail-status-enum',
      source: `(router-backend-registry bad-status
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status preview
          :runtime_allowed false
          :apply_blockers ["unknown status"]
          :substrate nil
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: ':runtime_allowed not a literal boolean atom',
      category: 'fail-runtime-allowed-literal',
      source: `(router-backend-registry bad-bool
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed yes
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: ':runtime_allowed true with advisory-only status REJECTED',
      category: 'fail-runtime-allowed-status',
      source: `(router-backend-registry runtime-allowed-mismatch
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id missiond-llm-router
          :readiness_status advisory-only
          :runtime_allowed true
          :apply_blockers ["should not be runtime allowed"]
          :substrate "missiond-daemon::router::llm"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: ':runtime_allowed true with unavailable status REJECTED',
      category: 'fail-runtime-allowed-status',
      source: `(router-backend-registry runtime-allowed-unavailable
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id patch-worker
          :readiness_status unavailable
          :runtime_allowed true
          :apply_blockers ["adapter does not exist"]
          :substrate "missiond-daemon::patch::worker"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'runtime-ready WITHOUT :substrate (nil)',
      category: 'fail-runtime-ready-substrate',
      source: `(router-backend-registry rr-no-substrate
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id verifier-worker
          :readiness_status runtime-ready
          :runtime_allowed true
          :apply_blockers []
          :substrate nil
          :non-goals ["does not mutate"]))`,
      ok: false,
    },
    {
      name: 'current-default WITHOUT :substrate (nil)',
      category: 'fail-runtime-ready-substrate',
      source: `(router-backend-registry cd-no-substrate
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate nil
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'advisory-only with EMPTY :apply_blockers',
      category: 'fail-empty-blockers',
      source: `(router-backend-registry advisory-no-blockers
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id missiond-llm-router
          :readiness_status advisory-only
          :runtime_allowed false
          :apply_blockers []
          :substrate nil
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'unavailable with EMPTY :apply_blockers',
      category: 'fail-empty-blockers',
      source: `(router-backend-registry unavailable-no-blockers
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id patch-worker
          :readiness_status unavailable
          :runtime_allowed false
          :apply_blockers []
          :substrate nil
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'empty :non-goals',
      category: 'fail-non-goals',
      source: `(router-backend-registry empty-non-goals
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals []))`,
      ok: false,
    },
    {
      name: 'unknown top-level entry head',
      category: 'fail-unknown-entry-head',
      source: `(router-backend-registry bad-entry
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (provider
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'missing required field (:apply_blockers)',
      category: 'fail-missing-required',
      source: `(router-backend-registry missing-blockers
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'unknown backend field',
      category: 'fail-unknown-field',
      source: `(router-backend-registry unknown-field
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"]
          :unknown "nope"))`,
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
      if (isList(form) && head(form) === REGISTRY_HEAD) {
        validateRegistry(file, form, diagnostics);
      }
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
      `router-backend-registry fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `router-backend-registry fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

function buildAllBackendsRegistry() {
  // Exercise every enum slot at least once: claudecode current-default,
  // missiond-llm-router advisory-only, deterministic-checker advisory-only,
  // patch-worker unavailable, verifier-worker advisory-only.
  return `(router-backend-registry all-backends
        :schema "missiond.router-backend-registry.v1"
        :version "v1"
        (backend
          :id claudecode
          :readiness_status current-default
          :runtime_allowed true
          :apply_blockers []
          :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
          :non-goals ["does not replace runtime dispatch"])
        (backend
          :id missiond-llm-router
          :readiness_status advisory-only
          :runtime_allowed false
          :apply_blockers ["no runtime adapter shipped"]
          :substrate nil
          :non-goals ["does not replace runtime dispatch"])
        (backend
          :id deterministic-checker
          :readiness_status advisory-only
          :runtime_allowed false
          :apply_blockers ["no runtime adapter shipped"]
          :substrate nil
          :non-goals ["does not replace runtime dispatch"])
        (backend
          :id patch-worker
          :readiness_status unavailable
          :runtime_allowed false
          :apply_blockers ["no runtime adapter shipped"]
          :substrate nil
          :non-goals ["does not replace runtime dispatch"])
        (backend
          :id verifier-worker
          :readiness_status advisory-only
          :runtime_allowed false
          :apply_blockers ["no runtime adapter shipped"]
          :substrate nil
          :non-goals ["does not replace runtime dispatch"]))`;
}

// Only run the CLI when invoked directly. Keeps named exports clean for
// future tooling that wants to reuse SCHEMA / BACKEND_IDS /
// READINESS_STATUSES / RUNTIME_ALLOWED_STATUSES and the projectBackend /
// projectRegistry / readBackendRegistryFile helpers (wave26-02 / 03).
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
