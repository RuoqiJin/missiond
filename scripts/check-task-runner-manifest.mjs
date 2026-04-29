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
  node scripts/check-task-runner-manifest.mjs [--json] [--stdin] [--dry-fixture] [<manifest.lisp> ...]

Checks MissionD task-runner-manifest v1/v2 Lisp records:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates each (task-runner-manifest <id> ...) header: required
    :schema :wave :brief_mode :shared_preamble_path :productive_only;
    optional :overlap_policy :description :generated_at :generator
  - validates each (node ...) entry: required :task_id :depends_on
    :verification_tier :dispatch_group :estimated_minutes
    :heartbeat_minutes :write_scope; optional :hard_deps :soft_refs
    :notes :owner :kind :model_profile :timeout_secs :context_pack_path
  - rejects: schema mismatch; unknown / missing / unknown-extra fields;
    brief_mode / verification_tier / overlap_policy enum violations;
    non-literal-atom productive_only (strings rejected); non-positive
    estimated_minutes / heartbeat_minutes; absolute / ~ / .. paths in
    shared_preamble_path or any write_scope entry; duplicate task_id;
    depends_on referencing a non-existent node id in the SAME manifest;
    self-edge in depends_on; productive_only=true with an
    archive/backfill/index/lisp-backfill node; same-file write_scope
    overlap among nodes in the SAME dispatch_group when overlap_policy
    is reject (default); malformed wave / task_id / dispatch_group id;
    unknown entry head.

Cross-wave invariants enforced: manifests are an advisory batch
descriptor, NOT a worker-task surface and NOT a runtime backend switch.
productive_only=true MUST exclude archive/backfill/index/lisp-backfill
orchestrator tasks. Same-file write_scope overlap among nodes in the
SAME dispatch_group is REJECTED by default; the overlap_policy=warn
opt-in is reserved for explicitly serialized retry/repair manifests.

Use --stdin to read a manifest record from stdin (handy for piping the
wave28-02 plan CLI output through the checker without a temp file).

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.task-runner-manifest.v1';
export const SCHEMA_V2 = 'missiond.task-runner-manifest.v2';
export const ACCEPTED_SCHEMAS = new Set([SCHEMA, SCHEMA_V2]);

export const MANIFEST_HEAD = 'task-runner-manifest';
export const NODE_HEAD = 'node';

const WAVE_ID_RE = /^[a-z][a-z0-9-]*$/;
const TASK_ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
const GROUP_RE = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const POSITIVE_INT_RE = /^[1-9][0-9]*$/;
const MODEL_PROFILE_RE = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const MIN_TIMEOUT_SECS = 60;
const MAX_TIMEOUT_SECS = 7200;

export const BRIEF_MODES = new Set(['thin', 'full', 'preamble']);

// Verification tiers — MUST mirror task-contract v1's :verification-tier
// enum (scripts/check-task-contract.mjs ALLOWED_VERIFICATION_TIER).
// Drift is a structural error.
export const VERIFICATION_TIERS = new Set(['local', 'smoke', 'full']);

export const OVERLAP_POLICIES = new Set(['reject', 'warn']);

// Categories that the wave28 dispatch-plan policy keeps orchestrator-owned.
// productive_only=true MUST exclude any node naming one of these as :kind
// or whose :task_id contains one of these substrings.
export const FORBIDDEN_PRODUCTIVE_KINDS = new Set([
  'archive',
  'backfill',
  'index',
  'lisp-backfill',
]);
const FORBIDDEN_ID_SUBSTRINGS = ['-archive-', '-backfill-', '-index', 'lisp-backfill'];

const HEADER_REQUIRED = [
  ':schema',
  ':wave',
  ':brief_mode',
  ':shared_preamble_path',
  ':productive_only',
];

const HEADER_OPTIONAL = [
  ':overlap_policy',
  ':description',
  ':generated_at',
  ':generator',
];

const HEADER_ALLOWED = new Set([...HEADER_REQUIRED, ...HEADER_OPTIONAL]);

const NODE_REQUIRED = [
  ':task_id',
  ':depends_on',
  ':verification_tier',
  ':dispatch_group',
  ':estimated_minutes',
  ':heartbeat_minutes',
  ':write_scope',
];

const NODE_OPTIONAL = [
  ':hard_deps',
  ':soft_refs',
  ':notes',
  ':owner',
  ':kind',
  ':model_profile',
  ':timeout_secs',
  ':context_pack_path',
];

const NODE_ALLOWED = new Set([...NODE_REQUIRED, ...NODE_OPTIONAL]);

const ISO_8601_RE =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$/;

const DEFAULT_OVERLAP_POLICY = 'reject';

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
  let manifestsValidated = 0;
  let nodesValidated = 0;

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
      if (h !== MANIFEST_HEAD) {
        addError(
          diagnostics,
          src.file,
          form.loc,
          `unknown entry head "${h}"; expected "${MANIFEST_HEAD}"`,
        );
        continue;
      }
      const result = validateManifest(src.file, form, diagnostics, { allowDiskCheck: src.file !== '<stdin>' });
      manifestsValidated += 1;
      nodesValidated += result.nodesValidated;
    }
  }

  const errors = diagnostics.filter((d) => d.severity === 'error');
  const warnings = diagnostics.filter((d) => d.severity === 'warning');
  const ok = errors.length === 0;

  if (json) {
    console.log(
      JSON.stringify(
        {
          ok,
          files: sources.length,
          manifests_validated: manifestsValidated,
          nodes_validated: nodesValidated,
          errors,
          warnings,
        },
        null,
        2,
      ),
    );
  } else if (ok) {
    const wText = warnings.length > 0 ? `, ${warnings.length} warning(s)` : '';
    for (const w of warnings) {
      console.warn(`${w.file}:${w.line}:${w.column}: warning: ${w.message}`);
    }
    console.log(
      `task-runner-manifest check OK (${manifestsValidated} manifest${manifestsValidated === 1 ? '' : 's'}, ${nodesValidated} node${nodesValidated === 1 ? '' : 's'}${wText})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `task-runner-manifest check FAILED — ${errors.length} error(s), ${warnings.length} warning(s), ${manifestsValidated} manifest(s), ${nodesValidated} node(s)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function validateManifest(file, manifest, diagnostics, opts = {}) {
  const allowDiskCheck = opts.allowDiskCheck !== false;
  const manifestId = nodeText(manifest.children[1]);
  if (!manifestId) {
    addError(
      diagnostics,
      file,
      manifest.loc,
      `(${MANIFEST_HEAD} ...) missing manifest id as second form`,
    );
  } else if (!TASK_ID_RE.test(manifestId)) {
    addError(
      diagnostics,
      file,
      manifest.children[1].loc,
      `manifest id "${manifestId}" must match ${TASK_ID_RE}`,
    );
  }

  const props = readKeywordProps(manifest, { start: 2 });

  // Header required-field presence + unknown-field rejection. Header
  // properties live alongside (node ...) entries — we only enforce
  // unknown-key rejection on the keyword props, not on list children.
  for (const field of HEADER_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        manifest.loc,
        `(${MANIFEST_HEAD} ${manifestId ?? '<missing>'} ...) missing required header field ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!HEADER_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${MANIFEST_HEAD} ...) has unknown header field ${key}`,
      );
    }
  }

  // :schema literal pin
  const schema = keywordPropText(props, ':schema');
  if (props[':schema'] && !ACCEPTED_SCHEMAS.has(schema)) {
    addError(
      diagnostics,
      file,
      props[':schema'].value?.loc ?? manifest.loc,
      `:schema must be one of {${[...ACCEPTED_SCHEMAS].join('|')}}, got ${JSON.stringify(schema)}`,
    );
  }

  // :wave id shape
  const wave = keywordPropText(props, ':wave');
  if (props[':wave'] && (wave == null || !WAVE_ID_RE.test(wave))) {
    addError(
      diagnostics,
      file,
      props[':wave'].value?.loc ?? manifest.loc,
      `:wave "${wave}" must match ${WAVE_ID_RE}`,
    );
  }

  // :brief_mode enum
  const briefMode = keywordPropText(props, ':brief_mode');
  if (props[':brief_mode'] && (briefMode == null || !BRIEF_MODES.has(briefMode))) {
    addError(
      diagnostics,
      file,
      props[':brief_mode'].value?.loc ?? manifest.loc,
      `:brief_mode "${briefMode}" not in enum {${[...BRIEF_MODES].join('|')}}`,
    );
  }

  // :productive_only literal-bool
  const productiveOnlyAtom = literalBoolAtom(props[':productive_only']?.value);
  if (props[':productive_only'] && productiveOnlyAtom == null) {
    const got = describeBoolNode(props[':productive_only'].value);
    addError(
      diagnostics,
      file,
      props[':productive_only'].value?.loc ?? manifest.loc,
      `:productive_only must be the literal atom true or false (strings are rejected), got ${got}`,
    );
  }
  const productiveOnly = productiveOnlyAtom === 'true';

  // :overlap_policy enum (optional; default reject)
  const overlapPolicyText = keywordPropText(props, ':overlap_policy');
  if (props[':overlap_policy'] && (overlapPolicyText == null || !OVERLAP_POLICIES.has(overlapPolicyText))) {
    addError(
      diagnostics,
      file,
      props[':overlap_policy'].value?.loc ?? manifest.loc,
      `:overlap_policy "${overlapPolicyText}" not in enum {${[...OVERLAP_POLICIES].join('|')}}`,
    );
  }
  const overlapPolicy =
    props[':overlap_policy'] && OVERLAP_POLICIES.has(overlapPolicyText)
      ? overlapPolicyText
      : DEFAULT_OVERLAP_POLICY;

  // :shared_preamble_path repo-relative shape + on-disk warning
  const sharedPreamblePath = keywordPropText(props, ':shared_preamble_path');
  if (props[':shared_preamble_path']) {
    const node = props[':shared_preamble_path'].value;
    const text = sharedPreamblePath;
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        node?.loc ?? manifest.loc,
        ':shared_preamble_path must be a non-empty string',
      );
    } else if (path.isAbsolute(text) || text.startsWith('~')) {
      addError(
        diagnostics,
        file,
        node?.loc ?? manifest.loc,
        `:shared_preamble_path must be repo-relative (no leading '/' or '~'), got "${text}"`,
      );
    } else if (text.split(/[\\/]/).some((seg) => seg === '..')) {
      addError(
        diagnostics,
        file,
        node?.loc ?? manifest.loc,
        `:shared_preamble_path must not contain ".." traversal, got "${text}"`,
      );
    } else if (
      allowDiskCheck &&
      (briefMode === 'thin' || briefMode === 'preamble') &&
      file !== '<stdin>'
    ) {
      // Warn (not error) when the preamble file is missing on disk: the
      // schema does not own the preamble and we still want CI to pass on
      // ad hoc fixture files. The wave28-05 batch verifier owns the hard
      // cross-file join.
      const cwd = process.cwd();
      const resolved = path.isAbsolute(text) ? text : path.resolve(cwd, text);
      if (!fs.existsSync(resolved)) {
        diagnostics.push({
          severity: 'warning',
          file,
          line: node.loc?.line ?? 1,
          column: node.loc?.column ?? 1,
          message: `:shared_preamble_path "${text}" not present on disk (warning only — schema does not own the preamble file)`,
        });
      }
    }
  }

  // Optional :description / :generator non-empty when present
  for (const field of [':description', ':generator']) {
    if (!props[field]) continue;
    const text = keywordPropText(props, field);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[field].value?.loc ?? manifest.loc,
        `${field} must be a non-empty string when present (omit the field if empty)`,
      );
    }
  }

  // Optional :generated_at ISO-8601-ish
  if (props[':generated_at']) {
    const text = keywordPropText(props, ':generated_at');
    if (text == null || !ISO_8601_RE.test(text)) {
      addError(
        diagnostics,
        file,
        props[':generated_at'].value?.loc ?? manifest.loc,
        `:generated_at must be an ISO-8601 timestamp string, got ${JSON.stringify(text)}`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // Node entries.
  // -----------------------------------------------------------------------

  const nodeForms = [];
  for (const child of manifest.children.slice(2)) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== NODE_HEAD) {
      addError(
        diagnostics,
        file,
        child.loc,
        `unknown entry head "${h}" inside (${MANIFEST_HEAD} ...); expected "${NODE_HEAD}"`,
      );
      continue;
    }
    nodeForms.push(child);
  }

  // First pass: per-node validation + collect projected views.
  const nodes = [];
  const seenIds = new Map(); // task_id -> first node form for duplicate-loc reporting
  for (const nodeForm of nodeForms) {
    const node = validateNode(file, nodeForm, diagnostics, {
      productiveOnly,
    });
    if (!node) continue;
    if (node.task_id != null) {
      if (seenIds.has(node.task_id)) {
        addError(
          diagnostics,
          file,
          node.taskIdLoc ?? nodeForm.loc,
          `duplicate node :task_id "${node.task_id}" (also defined at line ${seenIds.get(node.task_id).loc?.line ?? 1})`,
        );
      } else {
        seenIds.set(node.task_id, nodeForm);
      }
    }
    nodes.push(node);
  }

  // Second pass: hard dependency + soft reference cross-reference checks.
  // :depends_on remains the v1 hard-dependency field. :hard_deps is the
  // v2/additive explicit hard-dependency field; schedulers prefer it when
  // present. :soft_refs are context-only references and never block dispatch,
  // but they must still resolve to task ids so rendered briefs do not point
  // at ghosts.
  const taskIds = new Set(nodes.map((n) => n.task_id).filter((id) => id != null));
  for (const node of nodes) {
    for (const [field, refs] of [
      [':depends_on', node.depends_on ?? []],
      [':hard_deps', node.hard_deps ?? []],
      [':soft_refs', node.soft_refs ?? []],
    ]) {
      for (const dep of refs) {
        if (dep.value === node.task_id) {
          addError(
            diagnostics,
            file,
            dep.loc,
            `node "${node.task_id}" ${field} self-reference "${dep.value}" is not allowed`,
          );
          continue;
        }
        if (!taskIds.has(dep.value)) {
          addError(
            diagnostics,
            file,
            dep.loc,
            `node "${node.task_id ?? '<missing>'}" ${field} entry "${dep.value}" does not match any node :task_id in this manifest`,
          );
        }
      }
    }
  }

  // Explicit hard deps must not contradict legacy :depends_on in v2-style
  // manifests. This catches accidental partial migrations where one field
  // says "hard" and the other silently says "none".
  for (const node of nodes) {
    if ((node.hard_deps ?? []).length === 0) continue;
    const legacy = new Set((node.depends_on ?? []).map((d) => d.value));
    for (const dep of node.hard_deps) {
      if (!legacy.has(dep.value)) {
        addError(
          diagnostics,
          file,
          dep.loc,
          `node "${node.task_id}" :hard_deps entry "${dep.value}" must also appear in :depends_on for v1 compatibility`,
        );
      }
    }
  }

  // Third pass: write_scope overlap inside the same dispatch_group.
  // Default policy is reject; warn opt-in lowers the severity.
  const groupBuckets = new Map(); // group -> [{ task_id, path, loc }]
  for (const node of nodes) {
    if (node.dispatch_group == null) continue;
    const bucket = groupBuckets.get(node.dispatch_group) ?? [];
    for (const entry of node.write_scope ?? []) {
      bucket.push({ task_id: node.task_id, path: entry.value, loc: entry.loc });
    }
    groupBuckets.set(node.dispatch_group, bucket);
  }
  for (const [group, bucket] of groupBuckets) {
    const byPath = new Map(); // path -> [{ task_id, loc }]
    for (const item of bucket) {
      const arr = byPath.get(item.path) ?? [];
      arr.push(item);
      byPath.set(item.path, arr);
    }
    for (const [pth, items] of byPath) {
      if (items.length < 2) continue;
      // Same task_id legitimately listing a path twice would already be
      // a duplicate inside its own write_scope — handled per-node. Here
      // we only flag overlap across DIFFERENT nodes.
      const distinctTasks = new Set(items.map((i) => i.task_id));
      if (distinctTasks.size < 2) continue;
      const tasksList = [...distinctTasks].join(', ');
      const message = `write_scope overlap in dispatch_group "${group}": path "${pth}" is claimed by ${distinctTasks.size} nodes [${tasksList}] (overlap_policy=${overlapPolicy})`;
      const loc = items[0].loc ?? manifest.loc;
      if (overlapPolicy === 'warn') {
        diagnostics.push({
          severity: 'warning',
          file,
          line: loc?.line ?? 1,
          column: loc?.column ?? 1,
          message,
        });
      } else {
        addError(diagnostics, file, loc, message);
      }
    }
  }

  return { nodesValidated: nodeForms.length };
}

function validateNode(file, nodeForm, diagnostics, ctx) {
  const props = readKeywordProps(nodeForm, { start: 1 });

  // Required + unknown-key rejection.
  for (const field of NODE_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        nodeForm.loc,
        `(${NODE_HEAD} ...) missing required field ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!NODE_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${NODE_HEAD} ...) has unknown field ${key}`,
      );
    }
  }

  const projected = {
    task_id: null,
    taskIdLoc: null,
    depends_on: [],
    hard_deps: [],
    hard_deps_declared: Boolean(props[':hard_deps']),
    soft_refs: [],
    soft_refs_declared: Boolean(props[':soft_refs']),
    verification_tier: null,
    dispatch_group: null,
    estimated_minutes: null,
    heartbeat_minutes: null,
    write_scope: [],
    notes: null,
    owner: null,
    kind: null,
    model_profile: null,
    timeout_secs: null,
    context_pack_path: null,
  };

  // :task_id — non-empty kebab id
  const taskIdNode = props[':task_id']?.value;
  const taskIdText = nodeText(taskIdNode);
  if (taskIdNode) {
    projected.taskIdLoc = taskIdNode.loc;
    if (taskIdText == null || taskIdText.trim() === '' || !TASK_ID_RE.test(taskIdText)) {
      addError(
        diagnostics,
        file,
        taskIdNode.loc,
        `:task_id "${taskIdText}" must match ${TASK_ID_RE}`,
      );
    } else {
      projected.task_id = taskIdText;
    }
  }

  parseNodeRefVector({
    field: ':hard_deps',
    props,
    projected,
    targetKey: 'hard_deps',
    file,
    nodeForm,
    diagnostics,
  });
  parseNodeRefVector({
    field: ':soft_refs',
    props,
    projected,
    targetKey: 'soft_refs',
    file,
    nodeForm,
    diagnostics,
  });

  // :depends_on — vector of task_id atoms/strings.
  if (props[':depends_on']) {
    const node = props[':depends_on'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? nodeForm.loc,
        ':depends_on must be a vector/list of node :task_id values (use [] for none)',
      );
    } else {
      for (const child of node.children) {
        const text = nodeText(child);
        if (text == null || text.trim() === '' || !TASK_ID_RE.test(text)) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `:depends_on entry "${text}" must match ${TASK_ID_RE}`,
          );
          continue;
        }
        projected.depends_on.push({ value: text, loc: child.loc });
      }
    }
  }

  // :verification_tier enum
  const tierText = keywordPropText(props, ':verification_tier');
  if (props[':verification_tier']) {
    if (tierText == null || !VERIFICATION_TIERS.has(tierText)) {
      addError(
        diagnostics,
        file,
        props[':verification_tier'].value?.loc ?? nodeForm.loc,
        `:verification_tier "${tierText}" not in enum {${[...VERIFICATION_TIERS].join('|')}} (must mirror task-contract v1)`,
      );
    } else {
      projected.verification_tier = tierText;
    }
  }

  // :dispatch_group — non-empty atom or string
  const groupNode = props[':dispatch_group']?.value;
  const groupText = nodeText(groupNode);
  if (groupNode) {
    if (groupText == null || groupText.trim() === '' || !GROUP_RE.test(groupText)) {
      addError(
        diagnostics,
        file,
        groupNode.loc,
        `:dispatch_group "${groupText}" must be a non-empty id matching ${GROUP_RE}`,
      );
    } else {
      projected.dispatch_group = groupText;
    }
  }

  // :estimated_minutes / :heartbeat_minutes — positive integer literal atom
  for (const field of [':estimated_minutes', ':heartbeat_minutes']) {
    if (!props[field]) continue;
    const node = props[field].value;
    if (!node || node.type !== 'atom') {
      addError(
        diagnostics,
        file,
        node?.loc ?? nodeForm.loc,
        `${field} must be a positive integer literal atom (no strings, no fractions), got ${describeNode(node)}`,
      );
      continue;
    }
    if (!POSITIVE_INT_RE.test(node.value)) {
      addError(
        diagnostics,
        file,
        node.loc,
        `${field} must be a positive integer (>= 1), got "${node.value}"`,
      );
      continue;
    }
    const intValue = Number.parseInt(node.value, 10);
    if (field === ':estimated_minutes') projected.estimated_minutes = intValue;
    else projected.heartbeat_minutes = intValue;
  }

  // :write_scope — vector of repo-relative path strings/atoms
  if (props[':write_scope']) {
    const node = props[':write_scope'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? nodeForm.loc,
        ':write_scope must be a vector/list of repo-relative path strings (use [] for none)',
      );
    } else {
      const seenPaths = new Set();
      for (const child of node.children) {
        const text = nodeText(child);
        if (text == null || text.trim() === '') {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            ':write_scope entries must be non-empty path strings',
          );
          continue;
        }
        if (path.isAbsolute(text) || text.startsWith('~')) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `:write_scope entry must be repo-relative (no leading '/' or '~'), got "${text}"`,
          );
          continue;
        }
        if (text.split(/[\\/]/).some((seg) => seg === '..')) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `:write_scope entry must not contain ".." traversal, got "${text}"`,
          );
          continue;
        }
        if (seenPaths.has(text)) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `:write_scope contains duplicate path "${text}" inside the same node`,
          );
          continue;
        }
        seenPaths.add(text);
        projected.write_scope.push({ value: text, loc: child.loc });
      }
    }
  }

  // Optional :notes / :owner non-empty when present
  for (const field of [':notes', ':owner']) {
    if (!props[field]) continue;
    const text = keywordPropText(props, field);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[field].value?.loc ?? nodeForm.loc,
        `${field} must be a non-empty string when present (omit the field if empty)`,
      );
    } else if (field === ':notes') projected.notes = text;
    else projected.owner = text;
  }

  // Optional :kind — non-empty atom
  if (props[':kind']) {
    const text = keywordPropText(props, ':kind');
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':kind'].value?.loc ?? nodeForm.loc,
        ':kind must be a non-empty atom when present',
      );
    } else {
      projected.kind = text;
    }
  }

  // Optional :model_profile — safe single token projected to mission_task_delegate.
  if (props[':model_profile']) {
    const text = keywordPropText(props, ':model_profile');
    if (text == null || text.trim() === '' || !MODEL_PROFILE_RE.test(text)) {
      addError(
        diagnostics,
        file,
        props[':model_profile'].value?.loc ?? nodeForm.loc,
        `:model_profile "${text}" must match ${MODEL_PROFILE_RE}`,
      );
    } else {
      projected.model_profile = text;
    }
  }

  // Optional :timeout_secs — explicit BoardTask timeout override.
  if (props[':timeout_secs']) {
    const node = props[':timeout_secs'].value;
    if (!node || node.type !== 'atom') {
      addError(
        diagnostics,
        file,
        node?.loc ?? nodeForm.loc,
        `:timeout_secs must be a positive integer literal atom, got ${describeNode(node)}`,
      );
    } else if (!POSITIVE_INT_RE.test(node.value)) {
      addError(
        diagnostics,
        file,
        node.loc,
        `:timeout_secs must be a positive integer, got "${node.value}"`,
      );
    } else {
      const intValue = Number.parseInt(node.value, 10);
      if (intValue < MIN_TIMEOUT_SECS || intValue > MAX_TIMEOUT_SECS) {
        addError(
          diagnostics,
          file,
          node.loc,
          `:timeout_secs must be between ${MIN_TIMEOUT_SECS} and ${MAX_TIMEOUT_SECS}, got ${intValue}`,
        );
      } else {
        projected.timeout_secs = intValue;
      }
    }
  }

  // Optional :context_pack_path — repo-relative path to the integration-plan pack.
  if (props[':context_pack_path']) {
    const text = keywordPropText(props, ':context_pack_path');
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':context_pack_path'].value?.loc ?? nodeForm.loc,
        ':context_pack_path must be a non-empty repo-relative path when present',
      );
    } else if (path.isAbsolute(text) || text.startsWith('~')) {
      addError(
        diagnostics,
        file,
        props[':context_pack_path'].value?.loc ?? nodeForm.loc,
        `:context_pack_path must be repo-relative, got "${text}"`,
      );
    } else if (text.split(/[\\/]/).some((seg) => seg === '..')) {
      addError(
        diagnostics,
        file,
        props[':context_pack_path'].value?.loc ?? nodeForm.loc,
        `:context_pack_path must not contain ".." traversal, got "${text}"`,
      );
    } else {
      projected.context_pack_path = text;
    }
  }

  // Productive-only gate: archive / backfill / index / lisp-backfill nodes
  // are forbidden when the manifest is productive-only. We inspect both
  // :kind (canonical) and :task_id substrings (defence in depth).
  if (ctx.productiveOnly && projected.task_id != null) {
    if (projected.kind && FORBIDDEN_PRODUCTIVE_KINDS.has(projected.kind)) {
      addError(
        diagnostics,
        file,
        props[':kind']?.value?.loc ?? nodeForm.loc,
        `:productive_only true rejects node "${projected.task_id}" with :kind "${projected.kind}" (archive/backfill/index/lisp-backfill stay orchestrator-owned per dispatch-plan policy)`,
      );
    }
    for (const sub of FORBIDDEN_ID_SUBSTRINGS) {
      if (projected.task_id.includes(sub)) {
        addError(
          diagnostics,
          file,
          projected.taskIdLoc ?? nodeForm.loc,
          `:productive_only true rejects node "${projected.task_id}" — id contains "${sub}" (archive/backfill/index/lisp-backfill stay orchestrator-owned per dispatch-plan policy)`,
        );
        break;
      }
    }
  }

  return projected;
}

function parseNodeRefVector({ field, props, projected, targetKey, file, nodeForm, diagnostics }) {
  if (!props[field]) return;
  const node = props[field].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? nodeForm.loc,
      `${field} must be a vector/list of node :task_id values (use [] for none)`,
    );
    return;
  }
  const seen = new Set();
  for (const child of node.children) {
    const text = nodeText(child);
    if (text == null || text.trim() === '' || !TASK_ID_RE.test(text)) {
      addError(
        diagnostics,
        file,
        child.loc ?? node.loc,
        `${field} entry "${text}" must match ${TASK_ID_RE}`,
      );
      continue;
    }
    if (seen.has(text)) {
      addError(
        diagnostics,
        file,
        child.loc ?? node.loc,
        `${field} contains duplicate task id "${text}" inside the same node`,
      );
      continue;
    }
    seen.add(text);
    projected[targetKey].push({ value: text, loc: child.loc });
  }
}

function literalBoolAtom(node) {
  if (!node || node.type !== 'atom') return null;
  if (node.value === 'true' || node.value === 'false') return node.value;
  return null;
}

function describeBoolNode(node) {
  if (node == null) return '<missing>';
  if (node.type === 'atom') return node.value;
  if (node.type === 'string') return `"${node.value}" (string — strings are rejected)`;
  return `<${node.type}>`;
}

function describeNode(node) {
  if (node == null) return '<missing>';
  if (node.type === 'atom') return node.value;
  if (node.type === 'string') return `"${node.value}" (string)`;
  return `<${node.type}>`;
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
// Named exports for downstream tooling (wave28-02 plan CLI, wave28-03 brief
// renderer, wave28-05 batch verifier). These helpers project an already-
// parsed manifest form into a structured object so callers reason over
// manifests without re-implementing the Lisp reader. The functions are
// READ-ONLY: parse, project, return — they do not mutate input nodes and
// never write files.
// ---------------------------------------------------------------------------

// Project a single (task-runner-manifest ...) form into a structured
// object. Returns null when the form is not a manifest. Returned shape:
//   { id, schema, wave, brief_mode, shared_preamble_path,
//     productive_only: boolean|null, overlap_policy, description,
//     generated_at, generator,
//     nodes: [{ task_id, depends_on: string[], hard_deps: string[],
//               soft_refs: string[], verification_tier,
//               dispatch_group, estimated_minutes: number|null,
//               heartbeat_minutes: number|null, write_scope: string[],
//               notes, owner, kind, model_profile, timeout_secs,
//               context_pack_path, loc }],
//     loc: { line, column } }
export function projectManifest(form) {
  if (!isList(form) || head(form) !== MANIFEST_HEAD) return null;
  const id = nodeText(form.children[1]);
  const props = readKeywordProps(form, { start: 2 });

  const nodes = [];
  for (const child of form.children.slice(2)) {
    if (!isList(child) || head(child) !== NODE_HEAD) continue;
    nodes.push(projectNode(child));
  }

  return {
    id,
    schema: keywordPropText(props, ':schema'),
    wave: keywordPropText(props, ':wave'),
    brief_mode: keywordPropText(props, ':brief_mode'),
    shared_preamble_path: keywordPropText(props, ':shared_preamble_path'),
    productive_only: literalBool(props[':productive_only']?.value),
    overlap_policy: keywordPropText(props, ':overlap_policy') ?? DEFAULT_OVERLAP_POLICY,
    description: keywordPropText(props, ':description') ?? null,
    generated_at: keywordPropText(props, ':generated_at') ?? null,
    generator: keywordPropText(props, ':generator') ?? null,
    nodes,
    loc: form.loc ?? null,
  };
}

function projectNode(form) {
  const props = readKeywordProps(form, { start: 1 });
  const dependsNode = props[':depends_on']?.value ?? null;
  const depends_on = projectRefVector(dependsNode);
  const hard_deps = projectRefVector(props[':hard_deps']?.value ?? null);
  const soft_refs = projectRefVector(props[':soft_refs']?.value ?? null);
  const writeScopeNode = props[':write_scope']?.value ?? null;
  const write_scope = [];
  if (isList(writeScopeNode)) {
    for (const child of writeScopeNode.children) {
      const text = nodeText(child);
      if (text != null && text !== '') write_scope.push(text);
    }
  }
  const estimated = parseIntegerAtom(props[':estimated_minutes']?.value);
  const heartbeat = parseIntegerAtom(props[':heartbeat_minutes']?.value);
  const timeoutSecs = parseIntegerAtom(props[':timeout_secs']?.value);
  return {
    task_id: keywordPropText(props, ':task_id'),
    depends_on,
    hard_deps,
    hard_deps_declared: Boolean(props[':hard_deps']),
    soft_refs,
    soft_refs_declared: Boolean(props[':soft_refs']),
    verification_tier: keywordPropText(props, ':verification_tier'),
    dispatch_group: keywordPropText(props, ':dispatch_group'),
    estimated_minutes: estimated,
    heartbeat_minutes: heartbeat,
    write_scope,
    notes: keywordPropText(props, ':notes') ?? null,
    owner: keywordPropText(props, ':owner') ?? null,
    kind: keywordPropText(props, ':kind') ?? null,
    model_profile: keywordPropText(props, ':model_profile') ?? null,
    timeout_secs: timeoutSecs,
    context_pack_path: keywordPropText(props, ':context_pack_path') ?? null,
    loc: form.loc ?? null,
  };
}

function projectRefVector(node) {
  const out = [];
  if (isList(node)) {
    for (const child of node.children) {
      const text = nodeText(child);
      if (text != null && text !== '') out.push(text);
    }
  }
  return out;
}

function literalBool(node) {
  if (!node || node.type !== 'atom') return null;
  if (node.value === 'true') return true;
  if (node.value === 'false') return false;
  return null;
}

function parseIntegerAtom(node) {
  if (!node || node.type !== 'atom') return null;
  if (!POSITIVE_INT_RE.test(node.value)) return null;
  return Number.parseInt(node.value, 10);
}

// Read a task-runner-manifest Lisp file from disk and return all
// projected manifest forms in source order. Throws (with file context)
// when the reader fails. Callers that need validation should run main() /
// validateManifestObject() separately.
export function readManifestFile(file) {
  const forms = readLispFile(file);
  const out = [];
  for (const form of forms) {
    const projected = projectManifest(form);
    if (projected) out.push(projected);
  }
  return out;
}

// Validate an already-projected manifest object against the schema
// (re-uses the same enum / overlap / productive-only rules as the on-disk
// path). Returns an array of error message strings; empty array means
// valid. wave28-02 / wave28-05 will call this on freshly-emitted CLI
// output before writing it anywhere.
export function validateManifestObject(obj) {
  const errors = [];
  if (!obj || typeof obj !== 'object') {
    errors.push('manifest must be an object');
    return errors;
  }
  if (!ACCEPTED_SCHEMAS.has(obj.schema)) {
    errors.push(`:schema must be one of {${[...ACCEPTED_SCHEMAS].join('|')}}, got ${JSON.stringify(obj.schema)}`);
  }
  if (typeof obj.wave !== 'string' || !WAVE_ID_RE.test(obj.wave)) {
    errors.push(`:wave "${obj.wave}" must match ${WAVE_ID_RE}`);
  }
  if (!BRIEF_MODES.has(obj.brief_mode)) {
    errors.push(
      `:brief_mode "${obj.brief_mode}" not in enum {${[...BRIEF_MODES].join('|')}}`,
    );
  }
  if (typeof obj.shared_preamble_path !== 'string' || obj.shared_preamble_path.trim() === '') {
    errors.push(':shared_preamble_path must be a non-empty string');
  } else {
    const text = obj.shared_preamble_path;
    if (path.isAbsolute(text) || text.startsWith('~')) {
      errors.push(`:shared_preamble_path must be repo-relative, got "${text}"`);
    }
    if (text.split(/[\\/]/).some((seg) => seg === '..')) {
      errors.push(`:shared_preamble_path must not contain ".." traversal, got "${text}"`);
    }
  }
  if (typeof obj.productive_only !== 'boolean') {
    errors.push(':productive_only must be the literal atom true or false');
  }
  if (obj.overlap_policy != null && !OVERLAP_POLICIES.has(obj.overlap_policy)) {
    errors.push(
      `:overlap_policy "${obj.overlap_policy}" not in enum {${[...OVERLAP_POLICIES].join('|')}}`,
    );
  }
  if (!Array.isArray(obj.nodes)) {
    errors.push(':nodes must be an array');
    return errors;
  }
  const seenIds = new Set();
  for (const node of obj.nodes) {
    if (!node || typeof node !== 'object') {
      errors.push('node must be an object');
      continue;
    }
    if (typeof node.task_id !== 'string' || !TASK_ID_RE.test(node.task_id)) {
      errors.push(`node :task_id "${node.task_id}" must match ${TASK_ID_RE}`);
      continue;
    }
    if (seenIds.has(node.task_id)) {
      errors.push(`duplicate node :task_id "${node.task_id}"`);
      continue;
    }
    seenIds.add(node.task_id);
    if (!VERIFICATION_TIERS.has(node.verification_tier)) {
      errors.push(
        `node "${node.task_id}" :verification_tier "${node.verification_tier}" not in enum {${[...VERIFICATION_TIERS].join('|')}}`,
      );
    }
    if (typeof node.dispatch_group !== 'string' || !GROUP_RE.test(node.dispatch_group)) {
      errors.push(
        `node "${node.task_id}" :dispatch_group "${node.dispatch_group}" must match ${GROUP_RE}`,
      );
    }
    if (!Number.isInteger(node.estimated_minutes) || node.estimated_minutes <= 0) {
      errors.push(`node "${node.task_id}" :estimated_minutes must be a positive integer`);
    }
    if (!Number.isInteger(node.heartbeat_minutes) || node.heartbeat_minutes <= 0) {
      errors.push(`node "${node.task_id}" :heartbeat_minutes must be a positive integer`);
    }
    if (node.model_profile != null) {
      if (typeof node.model_profile !== 'string' || !MODEL_PROFILE_RE.test(node.model_profile)) {
        errors.push(`node "${node.task_id}" :model_profile must match ${MODEL_PROFILE_RE}`);
      }
    }
    if (node.timeout_secs != null) {
      if (
        !Number.isInteger(node.timeout_secs) ||
        node.timeout_secs < MIN_TIMEOUT_SECS ||
        node.timeout_secs > MAX_TIMEOUT_SECS
      ) {
        errors.push(
          `node "${node.task_id}" :timeout_secs must be between ${MIN_TIMEOUT_SECS} and ${MAX_TIMEOUT_SECS}`,
        );
      }
    }
    if (node.context_pack_path != null) {
      if (typeof node.context_pack_path !== 'string' || node.context_pack_path.trim() === '') {
        errors.push(`node "${node.task_id}" :context_pack_path must be a non-empty string`);
      } else if (path.isAbsolute(node.context_pack_path) || node.context_pack_path.startsWith('~')) {
        errors.push(
          `node "${node.task_id}" :context_pack_path must be repo-relative, got "${node.context_pack_path}"`,
        );
      } else if (node.context_pack_path.split(/[\\/]/).some((seg) => seg === '..')) {
        errors.push(
          `node "${node.task_id}" :context_pack_path must not contain "..", got "${node.context_pack_path}"`,
        );
      }
    }
    if (!Array.isArray(node.depends_on)) {
      errors.push(`node "${node.task_id}" :depends_on must be an array`);
    }
    if (node.hard_deps != null && !Array.isArray(node.hard_deps)) {
      errors.push(`node "${node.task_id}" :hard_deps must be an array when present`);
    }
    if (node.soft_refs != null && !Array.isArray(node.soft_refs)) {
      errors.push(`node "${node.task_id}" :soft_refs must be an array when present`);
    }
    if (!Array.isArray(node.write_scope)) {
      errors.push(`node "${node.task_id}" :write_scope must be an array`);
    } else {
      for (const entry of node.write_scope) {
        if (typeof entry !== 'string' || entry.trim() === '') {
          errors.push(`node "${node.task_id}" :write_scope entries must be non-empty strings`);
          break;
        }
        if (path.isAbsolute(entry) || entry.startsWith('~')) {
          errors.push(`node "${node.task_id}" :write_scope entry must be repo-relative, got "${entry}"`);
          break;
        }
        if (entry.split(/[\\/]/).some((seg) => seg === '..')) {
          errors.push(`node "${node.task_id}" :write_scope entry must not contain "..", got "${entry}"`);
          break;
        }
      }
    }
    if (obj.productive_only === true) {
      if (node.kind && FORBIDDEN_PRODUCTIVE_KINDS.has(node.kind)) {
        errors.push(
          `:productive_only true rejects node "${node.task_id}" with :kind "${node.kind}"`,
        );
      }
      for (const sub of FORBIDDEN_ID_SUBSTRINGS) {
        if (node.task_id.includes(sub)) {
          errors.push(
            `:productive_only true rejects node "${node.task_id}" — id contains "${sub}"`,
          );
          break;
        }
      }
    }
  }
  // Cross-node checks.
  for (const node of obj.nodes) {
    if (!node || typeof node !== 'object') continue;
    for (const [field, refs] of [
      [':depends_on', node.depends_on],
      [':hard_deps', node.hard_deps ?? []],
      [':soft_refs', node.soft_refs ?? []],
    ]) {
      if (!Array.isArray(refs)) continue;
      for (const dep of refs) {
        if (typeof dep !== 'string' || !TASK_ID_RE.test(dep)) {
          errors.push(`node "${node.task_id}" ${field} entry "${dep}" must match ${TASK_ID_RE}`);
          continue;
        }
        if (dep === node.task_id) {
          errors.push(`node "${node.task_id}" ${field} self-reference "${dep}" not allowed`);
          continue;
        }
        if (!seenIds.has(dep)) {
          errors.push(
            `node "${node.task_id}" ${field} entry "${dep}" does not match any node :task_id`,
          );
        }
      }
    }
    if (Array.isArray(node.hard_deps) && node.hard_deps.length > 0) {
      const legacy = new Set(Array.isArray(node.depends_on) ? node.depends_on : []);
      for (const dep of node.hard_deps) {
        if (!legacy.has(dep)) {
          errors.push(
            `node "${node.task_id}" :hard_deps entry "${dep}" must also appear in :depends_on for v1 compatibility`,
          );
        }
      }
    }
  }
  // Same-dispatch_group write_scope overlap (object-side mirror of the
  // on-disk pass; respects overlap_policy).
  const policy = OVERLAP_POLICIES.has(obj.overlap_policy) ? obj.overlap_policy : DEFAULT_OVERLAP_POLICY;
  const groupBuckets = new Map();
  for (const node of obj.nodes) {
    if (!node || typeof node !== 'object' || typeof node.dispatch_group !== 'string') continue;
    if (!Array.isArray(node.write_scope)) continue;
    const bucket = groupBuckets.get(node.dispatch_group) ?? [];
    for (const entry of node.write_scope) {
      bucket.push({ task_id: node.task_id, path: entry });
    }
    groupBuckets.set(node.dispatch_group, bucket);
  }
  for (const [group, bucket] of groupBuckets) {
    const byPath = new Map();
    for (const item of bucket) {
      const arr = byPath.get(item.path) ?? [];
      arr.push(item);
      byPath.set(item.path, arr);
    }
    for (const [pth, items] of byPath) {
      if (items.length < 2) continue;
      const distinctTasks = new Set(items.map((i) => i.task_id));
      if (distinctTasks.size < 2) continue;
      const message = `write_scope overlap in dispatch_group "${group}": "${pth}" claimed by [${[...distinctTasks].join(', ')}]`;
      if (policy === 'reject') errors.push(message);
    }
  }
  return errors;
}

function runFixtures(json = false) {
  const sharedPreamble = '.missiond/claudecode/wave28-shared-preamble.md';

  const fixtures = [
    {
      name: 'pass-valid-productive-only-2-groups',
      category: 'pass',
      source: `(task-runner-manifest m-valid-2-groups
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :model_profile coding-default-opus-4-7
              :timeout_secs 1800
              :context_pack_path ".missiond/tasks/wave99/context-pack.lisp"
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
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 25
              :heartbeat_minutes 10
              :write_scope ["scripts/baz.mjs"]))`,
      ok: true,
    },
    {
      name: 'pass-overlap-policy-warn-allows-overlap',
      category: 'pass-overlap-warn',
      source: `(task-runner-manifest m-overlap-warn
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
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
      ok: true,
    },
    {
      name: 'pass-overlap-different-dispatch-groups-allowed',
      category: 'pass-overlap-cross-group',
      source: `(task-runner-manifest m-overlap-cross-group
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
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
      ok: true,
    },
    {
      name: 'wave30-04-pass-v2-hard-deps-and-soft-refs',
      category: 'wave30-04-hard-soft',
      source: `(task-runner-manifest m-wave30-04-hard-soft
        :schema "${SCHEMA_V2}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-atlas
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/atlas.mjs"])
        (node :task_id wave99-02-patterns
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 60
              :heartbeat_minutes 10
              :write_scope ["scripts/patterns.mjs"])
        (node :task_id wave99-03-runner-prep
              :depends_on [wave99-01-atlas]
              :hard_deps [wave99-01-atlas]
              :soft_refs [wave99-02-patterns]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/runner-prep.mjs"]))`,
      ok: true,
    },
    {
      name: 'wave30-04-fail-soft-ref-must-resolve',
      category: 'wave30-04-hard-soft',
      source: `(task-runner-manifest m-wave30-04-soft-missing
        :schema "${SCHEMA_V2}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-atlas
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/atlas.mjs"])
        (node :task_id wave99-03-runner-prep
              :depends_on [wave99-01-atlas]
              :hard_deps [wave99-01-atlas]
              :soft_refs [wave99-99-missing]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/runner-prep.mjs"]))`,
      ok: false,
    },
    {
      name: 'wave30-04-fail-hard-deps-must-stay-v1-compatible',
      category: 'wave30-04-hard-soft',
      source: `(task-runner-manifest m-wave30-04-hard-compat
        :schema "${SCHEMA_V2}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-atlas
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/atlas.mjs"])
        (node :task_id wave99-03-runner-prep
              :depends_on []
              :hard_deps [wave99-01-atlas]
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 10
              :heartbeat_minutes 5
              :write_scope ["scripts/runner-prep.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-duplicate-task-id',
      category: 'fail-duplicate',
      source: `(task-runner-manifest m-dup-task-id
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"])
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group B
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo2.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-missing-dependency-reference',
      category: 'fail-missing-dep',
      source: `(task-runner-manifest m-missing-dep
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-02-bar
              :depends_on [wave99-01-foo]
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/bar.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-self-edge-in-depends-on',
      category: 'fail-self-edge',
      source: `(task-runner-manifest m-self-edge
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on [wave99-01-foo]
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-invalid-verification-tier',
      category: 'fail-enum',
      source: `(task-runner-manifest m-bad-tier
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier preview
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-invalid-brief-mode',
      category: 'fail-enum',
      source: `(task-runner-manifest m-bad-brief
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode verbose
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-write-scope-overlap-same-dispatch-group',
      category: 'fail-overlap',
      source: `(task-runner-manifest m-overlap-reject
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
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
      ok: false,
    },
    {
      name: 'fail-zero-heartbeat-minutes',
      category: 'fail-positive-int',
      source: `(task-runner-manifest m-zero-heartbeat
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 0
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-negative-estimated-minutes',
      category: 'fail-positive-int',
      source: `(task-runner-manifest m-neg-estimated
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes -5
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-string-bool-productive-only',
      category: 'fail-string-bool',
      source: `(task-runner-manifest m-string-bool
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only "true"
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-invalid-dispatch-projection-fields',
      category: 'fail-dispatch-projection',
      source: `(task-runner-manifest m-invalid-dispatch-projection
        :schema "${SCHEMA_V2}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :model_profile "bad profile with spaces"
              :timeout_secs 99999
              :context_pack_path "../context-pack.lisp"
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-archive-task-rejected-when-productive-only',
      category: 'fail-productive-only',
      source: `(task-runner-manifest m-archive-rejected
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-00-archive-prior-task-docs
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope [".missiond/claudecode/_archive/foo.md"]))`,
      ok: false,
    },
    {
      name: 'fail-backfill-task-rejected-when-productive-only',
      category: 'fail-productive-only',
      source: `(task-runner-manifest m-backfill-rejected
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-09-lisp-backfill-status
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope [".missiond/v2/status.lisp"]))`,
      ok: false,
    },
    {
      name: 'fail-index-task-kind-rejected-when-productive-only',
      category: 'fail-productive-only',
      source: `(task-runner-manifest m-index-kind-rejected
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-10-parallel-dispatch
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope [".missiond/claudecode/wave99-10.md"]
              :kind index))`,
      ok: false,
    },
    {
      name: 'fail-absolute-write-scope-path',
      category: 'fail-path',
      source: `(task-runner-manifest m-abs-path
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["/etc/passwd"]))`,
      ok: false,
    },
    {
      name: 'fail-traversal-shared-preamble-path',
      category: 'fail-path',
      source: `(task-runner-manifest m-traversal-preamble
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "../../etc/passwd"
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-unknown-entry-head-at-top-level',
      category: 'fail-unknown-entry-head',
      source: `(manifest m-wrong-head
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true)`,
      ok: false,
    },
    {
      name: 'fail-unknown-header-field',
      category: 'fail-unknown-field',
      source: `(task-runner-manifest m-unknown-header
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        :extraneous "no such field"
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-missing-required-header-shared-preamble',
      category: 'fail-missing-required',
      source: `(task-runner-manifest m-missing-preamble
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :productive_only true
        (node :task_id wave99-01-foo
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope ["scripts/foo.mjs"]))`,
      ok: false,
    },
    // wave28-06 cross-layer smoke pin (Layer A). Synthetic productive-only
    // manifest matching the shape the smoke drives through plan CLI,
    // renderer, daemon dry-run surface, and batch verifier. Pins the
    // wave28-06 invariant 1 (same manifest validates clean at the schema
    // layer) at the checker boundary.
    {
      name: 'wave28-06-loop-smoke-productive-only-pinned',
      category: 'wave28-06-loop-smoke',
      source: `(task-runner-manifest m-wave28-06-loop-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
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
      ok: true,
    },
    // wave28-06 invariant 2 defence-in-depth pin: even with a real-looking
    // task id, an archive substring is rejected when productive_only=true.
    // Mirrors the wave28-03 renderer + wave28-05 batch verifier skip-paths
    // — the schema layer is the first defence, the renderer/verifier are
    // the second + third.
    {
      name: 'wave28-06-loop-smoke-archive-substring-rejected',
      category: 'wave28-06-loop-smoke',
      source: `(task-runner-manifest m-wave28-06-archive-rejected
        :schema "${SCHEMA}"
        :wave wave99
        :brief_mode thin
        :shared_preamble_path "${sharedPreamble}"
        :productive_only true
        (node :task_id wave99-00-archive-prior-task-docs
              :depends_on []
              :verification_tier local
              :dispatch_group A
              :estimated_minutes 30
              :heartbeat_minutes 10
              :write_scope [".missiond/claudecode/_archive/foo.md"]))`,
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
      if (h !== MANIFEST_HEAD) {
        addError(
          diagnostics,
          file,
          form.loc,
          `unknown entry head "${h}"; expected "${MANIFEST_HEAD}"`,
        );
        continue;
      }
      validateManifest(file, form, diagnostics, { allowDiskCheck: false });
    }
    const ok = !diagnostics.some((d) => d.severity === 'error');
    if (ok !== fixture.ok) {
      failed += 1;
      console.error(
        `fixture failed: ${fixture.name} (expected ok=${fixture.ok}, got ok=${ok})`,
      );
      for (const d of diagnostics) {
        console.error(`  ${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
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
      `task-runner-manifest fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `task-runner-manifest fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

// Only run the CLI when invoked directly. Keeps named exports clean for
// future tooling (wave28-02 plan CLI, wave28-03 brief renderer, wave28-05
// batch verifier).
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
