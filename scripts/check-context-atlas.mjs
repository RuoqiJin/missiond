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
  node scripts/check-context-atlas.mjs [--json] [--stdin] [--dry-fixture] [<atlas.lisp> ...]

Checks MissionD context-atlas v1 Lisp records:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates the (context-atlas <id> ...) header: required :schema and
    :read-order; one of :purpose | :goal required (synonyms); optional
    :wave :version :description :generated_at :generator
  - validates entry forms: top-level (global-anchors ...) container with
    (file ...) leaves only; top-level (task-focus ...) container with
    (task ...) leaves only; flat top-level (file ...), (task_focus ...),
    (grep ...), (avoid ...) entries are also accepted
  - rejects: schema mismatch (must be missiond.context-atlas.v1 OR the
    transitional missiond.context-atlas.dispatch.v0); missing required
    header; missing both :purpose and :goal; unknown header field; empty
    :read-order; absolute / ~ / .. path in :read-order, file path, or
    :first-reads; duplicate file path within an atlas; duplicate task
    id within an atlas; malformed task id; empty grep anchor; empty
    avoid string; (global-anchors ...) child whose head is not file;
    (task-focus ...) container child whose head is not task; multiple
    (context-atlas ...) forms in one file; unknown top-level entry head;
    atlas with zero entries.

Cross-wave invariants enforced: an atlas is NAVIGATION METADATA ONLY,
never a behavioral contract. The checker validates structure only — it
does NOT call git, does NOT shell out, does NOT touch the network or any
LLM, and does NOT verify that referenced file paths exist on disk.

Use --stdin to read an atlas record from stdin.
Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.context-atlas.v1';

// Schema strings the checker accepts. v1 is the durable form; dispatch.v0
// is the transitional dispatch-time variant (the wave29 dispatch atlas
// uses this string). Future waves should migrate to v1.
export const ACCEPTED_SCHEMAS = new Set([
  'missiond.context-atlas.v1',
  'missiond.context-atlas.dispatch.v0',
]);

export const ATLAS_HEAD = 'context-atlas';

// Container heads carry no own keyword props; their children must be a
// specific leaf head.
export const CONTAINER_HEADS = new Set(['global-anchors', 'task-focus']);

// Leaf entry heads valid at top level OR (where applicable) inside a
// container. (file ...) is valid both top-level and inside global-anchors;
// (task ...) is valid only inside (task-focus ...) container; (task_focus
// ...) is the top-level shorthand for one task entry; (grep ...) and
// (avoid ...) are top-level only.
export const TOP_LEAF_HEADS = new Set(['file', 'task_focus', 'grep', 'avoid']);
export const ALL_TOP_HEADS = new Set([...CONTAINER_HEADS, ...TOP_LEAF_HEADS]);

const TASK_ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
const WAVE_ID_RE = /^[a-z][a-z0-9-]*$/;

const HEADER_REQUIRED = [':schema', ':read-order'];
const HEADER_ONE_OF = [':purpose', ':goal'];
const HEADER_OPTIONAL = [
  ':purpose',
  ':goal',
  ':wave',
  ':version',
  ':description',
  ':generated_at',
  ':generator',
];
const HEADER_ALLOWED = new Set([...HEADER_REQUIRED, ...HEADER_OPTIONAL]);

const FILE_REQUIRED = [];
const FILE_OPTIONAL = [':purpose', ':grep'];
const FILE_ALLOWED = new Set([...FILE_REQUIRED, ...FILE_OPTIONAL]);

const TASK_REQUIRED = [];
const TASK_OPTIONAL = [':first-reads', ':avoid', ':purpose'];
const TASK_ALLOWED = new Set([...TASK_REQUIRED, ...TASK_OPTIONAL]);

const GREP_OPTIONAL = [':purpose'];
const GREP_ALLOWED = new Set([...GREP_OPTIONAL]);

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
  let atlasesValidated = 0;

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
    const atlasForms = forms.filter((f) => isList(f) && head(f) === ATLAS_HEAD);
    const otherForms = forms.filter((f) => isList(f) && head(f) !== ATLAS_HEAD);
    for (const other of otherForms) {
      addError(
        diagnostics,
        src.file,
        other.loc,
        `unknown top-level form head "${head(other)}"; expected "${ATLAS_HEAD}"`,
      );
    }
    if (atlasForms.length > 1) {
      // Atlases are single-document; flag every form past the first.
      for (let i = 1; i < atlasForms.length; i++) {
        addError(
          diagnostics,
          src.file,
          atlasForms[i].loc,
          `file contains multiple (${ATLAS_HEAD} ...) forms; atlases are single-document`,
        );
      }
    }
    for (const form of atlasForms) {
      validateAtlas(src.file, form, diagnostics);
      atlasesValidated += 1;
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
          atlases_validated: atlasesValidated,
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
      `context-atlas check OK (${atlasesValidated} atlas${atlasesValidated === 1 ? '' : 'es'}${wText})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `context-atlas check FAILED — ${errors.length} error(s), ${warnings.length} warning(s), ${atlasesValidated} atlas(es)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function validateAtlas(file, atlas, diagnostics) {
  const atlasId = nodeText(atlas.children[1]);
  if (!atlasId) {
    addError(
      diagnostics,
      file,
      atlas.loc,
      `(${ATLAS_HEAD} ...) missing atlas id as second form`,
    );
  } else if (!TASK_ID_RE.test(atlasId)) {
    addError(
      diagnostics,
      file,
      atlas.children[1].loc,
      `atlas id "${atlasId}" must match ${TASK_ID_RE}`,
    );
  }

  const props = readKeywordProps(atlas, { start: 2 });

  // Required header presence + unknown-key rejection.
  for (const field of HEADER_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        atlas.loc,
        `(${ATLAS_HEAD} ${atlasId ?? '<missing>'} ...) missing required header field ${field}`,
      );
    }
  }
  // One-of (:purpose | :goal) requirement.
  if (!HEADER_ONE_OF.some((k) => props[k])) {
    addError(
      diagnostics,
      file,
      atlas.loc,
      `(${ATLAS_HEAD} ${atlasId ?? '<missing>'} ...) must declare one of {${HEADER_ONE_OF.join(' | ')}} (synonyms for the same field)`,
    );
  }
  for (const key of Object.keys(props)) {
    if (!HEADER_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${ATLAS_HEAD} ...) has unknown header field ${key}`,
      );
    }
  }

  // :schema literal in accepted-schema-strings.
  const schema = keywordPropText(props, ':schema');
  if (props[':schema'] && (schema == null || !ACCEPTED_SCHEMAS.has(schema))) {
    addError(
      diagnostics,
      file,
      props[':schema'].value?.loc ?? atlas.loc,
      `:schema must be one of {${[...ACCEPTED_SCHEMAS].join(' | ')}}, got ${JSON.stringify(schema)}`,
    );
  }

  // :read-order — non-empty vector of repo-relative path strings/atoms.
  if (props[':read-order']) {
    const node = props[':read-order'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? atlas.loc,
        ':read-order must be a vector/list of repo-relative path strings',
      );
    } else if (node.children.length === 0) {
      addError(
        diagnostics,
        file,
        node.loc,
        ':read-order must be non-empty (declare at least one canonical read path)',
      );
    } else {
      for (const child of node.children) {
        const text = nodeText(child);
        validateRepoRelativePath(
          diagnostics,
          file,
          child.loc ?? node.loc,
          text,
          ':read-order entry',
        );
      }
    }
  }

  // :wave id shape (when present).
  if (props[':wave']) {
    const wave = keywordPropText(props, ':wave');
    if (wave == null || !WAVE_ID_RE.test(wave)) {
      addError(
        diagnostics,
        file,
        props[':wave'].value?.loc ?? atlas.loc,
        `:wave "${wave}" must match ${WAVE_ID_RE}`,
      );
    }
  }

  // Optional non-empty strings.
  for (const field of [':purpose', ':goal', ':description', ':version', ':generator']) {
    if (!props[field]) continue;
    const text = keywordPropText(props, field);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[field].value?.loc ?? atlas.loc,
        `${field} must be a non-empty string when present (omit the field if empty)`,
      );
    }
  }

  // Optional :generated_at ISO-8601-ish.
  if (props[':generated_at']) {
    const text = keywordPropText(props, ':generated_at');
    if (text == null || !ISO_8601_RE.test(text)) {
      addError(
        diagnostics,
        file,
        props[':generated_at'].value?.loc ?? atlas.loc,
        `:generated_at must be an ISO-8601 timestamp string, got ${JSON.stringify(text)}`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // Entry forms.
  // -----------------------------------------------------------------------

  // Children of (context-atlas <id> ...) interleave keyword props (atom
  // starting with ':' followed by its value) and entry forms (lists with a
  // recognised head). We must skip keyword values — including bracket-list
  // values like `:read-order [...]` — when collecting entries.
  const entries = collectEntries(atlas, 2);

  // Reject zero entries.
  if (entries.length === 0) {
    addError(
      diagnostics,
      file,
      atlas.loc,
      `(${ATLAS_HEAD} ${atlasId ?? '<missing>'} ...) must declare at least one entry (file / task_focus / grep / avoid / global-anchors / task-focus)`,
    );
  }

  // Track uniqueness across the whole atlas.
  const seenFilePaths = new Map(); // path -> { loc, sourceLabel }
  const seenTaskIds = new Map();   // task_id -> { loc, sourceLabel }

  for (const entry of entries) {
    const h = head(entry);
    if (!ALL_TOP_HEADS.has(h)) {
      addError(
        diagnostics,
        file,
        entry.loc,
        `unknown entry head "${h}" inside (${ATLAS_HEAD} ...); expected one of {${[...ALL_TOP_HEADS].join(' | ')}}`,
      );
      continue;
    }
    if (h === 'global-anchors') {
      validateGlobalAnchors(file, entry, diagnostics, seenFilePaths);
    } else if (h === 'task-focus') {
      validateTaskFocusContainer(file, entry, diagnostics, seenTaskIds);
    } else if (h === 'file') {
      validateFileEntry(file, entry, diagnostics, seenFilePaths);
    } else if (h === 'task_focus') {
      validateTaskEntry(file, entry, diagnostics, seenTaskIds, 'task_focus');
    } else if (h === 'grep') {
      validateGrepEntry(file, entry, diagnostics);
    } else if (h === 'avoid') {
      validateAvoidEntry(file, entry, diagnostics);
    }
  }
}

function validateGlobalAnchors(file, container, diagnostics, seenFilePaths) {
  // Containers carry no keyword props of their own; every list child must
  // be (file ...). Atom / string children are ignored (no narrative atoms
  // belong here, but we don't strictly forbid them — the leaf path
  // validation does the load-bearing work).
  for (const child of container.children.slice(1)) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== 'file') {
      addError(
        diagnostics,
        file,
        child.loc,
        `(global-anchors ...) child must have head "file"; got "${h}"`,
      );
      continue;
    }
    validateFileEntry(file, child, diagnostics, seenFilePaths);
  }
}

function validateTaskFocusContainer(file, container, diagnostics, seenTaskIds) {
  for (const child of container.children.slice(1)) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== 'task') {
      addError(
        diagnostics,
        file,
        child.loc,
        `(task-focus ...) child must have head "task"; got "${h}"`,
      );
      continue;
    }
    validateTaskEntry(file, child, diagnostics, seenTaskIds, 'task');
  }
}

function validateFileEntry(file, entry, diagnostics, seenFilePaths) {
  const pathNode = entry.children[1];
  const pathText = nodeText(pathNode);
  if (!pathNode) {
    addError(
      diagnostics,
      file,
      entry.loc,
      '(file ...) entry missing path as second form',
    );
  } else {
    validateRepoRelativePath(
      diagnostics,
      file,
      pathNode.loc ?? entry.loc,
      pathText,
      '(file ...) path',
    );
    if (pathText != null && pathText.trim() !== '') {
      const existing = seenFilePaths.get(pathText);
      if (existing) {
        addError(
          diagnostics,
          file,
          pathNode.loc ?? entry.loc,
          `duplicate file path "${pathText}" in atlas (also at line ${existing.loc?.line ?? 1})`,
        );
      } else {
        seenFilePaths.set(pathText, { loc: pathNode.loc });
      }
    }
  }

  const props = readKeywordProps(entry, { start: 2 });
  for (const key of Object.keys(props)) {
    if (!FILE_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(file ...) has unknown field ${key}`,
      );
    }
  }

  if (props[':purpose']) {
    const text = keywordPropText(props, ':purpose');
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':purpose'].value?.loc ?? entry.loc,
        '(file ...) :purpose must be a non-empty string when present',
      );
    }
  }

  if (props[':grep']) {
    const node = props[':grep'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? entry.loc,
        '(file ...) :grep must be a vector/list of non-empty anchor strings',
      );
    } else {
      for (const child of node.children) {
        const text = nodeText(child);
        if (text == null || text.trim() === '') {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            '(file ...) :grep entries must be non-empty strings (empty grep anchors are rejected)',
          );
        }
      }
    }
  }
}

function validateTaskEntry(file, entry, diagnostics, seenTaskIds, sourceLabel) {
  const taskIdNode = entry.children[1];
  const taskIdText = nodeText(taskIdNode);
  if (!taskIdNode) {
    addError(
      diagnostics,
      file,
      entry.loc,
      `(${sourceLabel} ...) entry missing task id as second form`,
    );
  } else if (taskIdText == null || taskIdText.trim() === '' || !TASK_ID_RE.test(taskIdText)) {
    addError(
      diagnostics,
      file,
      taskIdNode.loc,
      `(${sourceLabel} ...) :task_id "${taskIdText}" must match ${TASK_ID_RE}`,
    );
  } else {
    const existing = seenTaskIds.get(taskIdText);
    if (existing) {
      addError(
        diagnostics,
        file,
        taskIdNode.loc,
        `duplicate task id "${taskIdText}" in atlas (also at line ${existing.loc?.line ?? 1})`,
      );
    } else {
      seenTaskIds.set(taskIdText, { loc: taskIdNode.loc });
    }
  }

  const props = readKeywordProps(entry, { start: 2 });
  for (const key of Object.keys(props)) {
    if (!TASK_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${sourceLabel} ...) has unknown field ${key}`,
      );
    }
  }

  if (props[':first-reads']) {
    const node = props[':first-reads'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? entry.loc,
        `(${sourceLabel} ...) :first-reads must be a vector/list of repo-relative path strings`,
      );
    } else {
      for (const child of node.children) {
        const text = nodeText(child);
        validateRepoRelativePath(
          diagnostics,
          file,
          child.loc ?? node.loc,
          text,
          `(${sourceLabel} ...) :first-reads entry`,
        );
      }
    }
  }

  if (props[':avoid']) {
    const node = props[':avoid'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? entry.loc,
        `(${sourceLabel} ...) :avoid must be a vector/list of non-empty strings`,
      );
    } else {
      for (const child of node.children) {
        const text = nodeText(child);
        if (text == null || text.trim() === '') {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `(${sourceLabel} ...) :avoid entries must be non-empty strings`,
          );
        }
      }
    }
  }

  if (props[':purpose']) {
    const text = keywordPropText(props, ':purpose');
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':purpose'].value?.loc ?? entry.loc,
        `(${sourceLabel} ...) :purpose must be a non-empty string when present`,
      );
    }
  }
}

function validateGrepEntry(file, entry, diagnostics) {
  const patternNode = entry.children[1];
  const patternText = nodeText(patternNode);
  if (!patternNode) {
    addError(
      diagnostics,
      file,
      entry.loc,
      '(grep ...) entry missing pattern as second form',
    );
  } else if (patternText == null || patternText.trim() === '') {
    addError(
      diagnostics,
      file,
      patternNode.loc ?? entry.loc,
      '(grep ...) pattern must be a non-empty string (empty grep anchors are rejected)',
    );
  }

  const props = readKeywordProps(entry, { start: 2 });
  for (const key of Object.keys(props)) {
    if (!GREP_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(grep ...) has unknown field ${key}`,
      );
    }
  }
  if (props[':purpose']) {
    const text = keywordPropText(props, ':purpose');
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':purpose'].value?.loc ?? entry.loc,
        '(grep ...) :purpose must be a non-empty string when present',
      );
    }
  }
}

function validateAvoidEntry(file, entry, diagnostics) {
  const children = entry.children.slice(1);
  if (children.length === 0) {
    addError(
      diagnostics,
      file,
      entry.loc,
      '(avoid ...) must contain at least one non-empty note string',
    );
    return;
  }
  for (const child of children) {
    const text = nodeText(child);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        child.loc ?? entry.loc,
        '(avoid ...) note entries must be non-empty strings',
      );
    }
  }
}

function validateRepoRelativePath(diagnostics, file, loc, text, label) {
  if (text == null || text.trim() === '') {
    addError(diagnostics, file, loc, `${label} must be a non-empty path string`);
    return;
  }
  if (path.isAbsolute(text) || text.startsWith('~')) {
    addError(
      diagnostics,
      file,
      loc,
      `${label} must be repo-relative (no leading '/' or '~'), got "${text}"`,
    );
    return;
  }
  if (text.split(/[\\/]/).some((seg) => seg === '..')) {
    addError(
      diagnostics,
      file,
      loc,
      `${label} must not contain ".." traversal, got "${text}"`,
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

// Walk a list's children starting at `start`, treating any atom that
// begins with ':' as a keyword consuming the next form as its value, and
// returning the remaining list children as entries. This mirrors how
// readKeywordProps discovers props but yields the leftover lists for
// entry-by-entry validation.
function collectEntries(parent, start) {
  const out = [];
  const children = parent.children;
  for (let i = start; i < children.length; i++) {
    const current = children[i];
    if (current.type === 'atom' && typeof current.value === 'string' && current.value.startsWith(':')) {
      // Skip the keyword and its value (if any).
      i += 1;
      continue;
    }
    if (isList(current)) out.push(current);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Named exports for downstream tooling (wave29-03 prep CLI, wave29-06
// ready-queue planner). These helpers project a parsed atlas form into a
// structured object so callers reason over atlases without re-implementing
// the Lisp reader. The functions are READ-ONLY: parse, project, return —
// they do not mutate input nodes and never write files.
// ---------------------------------------------------------------------------

// Project a single (context-atlas ...) form into a structured object.
// Returns null when the form is not an atlas. Returned shape:
//   { id, schema, purpose, goal, wave, version, description,
//     generated_at, generator, read_order: string[],
//     files: [{ path, purpose, grep: string[], source }],
//     tasks: [{ task_id, first_reads: string[], avoid: string[],
//               purpose, source }],
//     greps: [{ pattern, purpose }],
//     avoids: [{ notes: string[] }],
//     loc: { line, column } }
// `source` records whether a file/task came from a top-level entry or a
// container child ("global-anchors", "task-focus", "file", "task_focus").
export function projectAtlas(form) {
  if (!isList(form) || head(form) !== ATLAS_HEAD) return null;
  const id = nodeText(form.children[1]);
  const props = readKeywordProps(form, { start: 2 });

  const readOrder = collectStringVector(props[':read-order']?.value);

  const files = [];
  const tasks = [];
  const greps = [];
  const avoids = [];

  for (const child of form.children.slice(2)) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h === 'global-anchors') {
      for (const inner of child.children.slice(1)) {
        if (isList(inner) && head(inner) === 'file') {
          files.push(projectFile(inner, 'global-anchors'));
        }
      }
    } else if (h === 'task-focus') {
      for (const inner of child.children.slice(1)) {
        if (isList(inner) && head(inner) === 'task') {
          tasks.push(projectTask(inner, 'task-focus'));
        }
      }
    } else if (h === 'file') {
      files.push(projectFile(child, 'file'));
    } else if (h === 'task_focus') {
      tasks.push(projectTask(child, 'task_focus'));
    } else if (h === 'grep') {
      greps.push(projectGrep(child));
    } else if (h === 'avoid') {
      avoids.push(projectAvoid(child));
    }
  }

  return {
    id,
    schema: keywordPropText(props, ':schema'),
    purpose: keywordPropText(props, ':purpose') ?? null,
    goal: keywordPropText(props, ':goal') ?? null,
    wave: keywordPropText(props, ':wave') ?? null,
    version: keywordPropText(props, ':version') ?? null,
    description: keywordPropText(props, ':description') ?? null,
    generated_at: keywordPropText(props, ':generated_at') ?? null,
    generator: keywordPropText(props, ':generator') ?? null,
    read_order: readOrder,
    files,
    tasks,
    greps,
    avoids,
    loc: form.loc ?? null,
  };
}

function projectFile(form, source) {
  const props = readKeywordProps(form, { start: 2 });
  return {
    path: nodeText(form.children[1]),
    purpose: keywordPropText(props, ':purpose') ?? null,
    grep: collectStringVector(props[':grep']?.value),
    source,
  };
}

function projectTask(form, source) {
  const props = readKeywordProps(form, { start: 2 });
  return {
    task_id: nodeText(form.children[1]),
    first_reads: collectStringVector(props[':first-reads']?.value),
    avoid: collectStringVector(props[':avoid']?.value),
    purpose: keywordPropText(props, ':purpose') ?? null,
    source,
  };
}

function projectGrep(form) {
  const props = readKeywordProps(form, { start: 2 });
  return {
    pattern: nodeText(form.children[1]),
    purpose: keywordPropText(props, ':purpose') ?? null,
  };
}

function projectAvoid(form) {
  const notes = [];
  for (const child of form.children.slice(1)) {
    const text = nodeText(child);
    if (text != null && text !== '') notes.push(text);
  }
  return { notes };
}

function collectStringVector(node) {
  if (!isList(node)) return [];
  const out = [];
  for (const child of node.children) {
    const text = nodeText(child);
    if (text != null && text !== '') out.push(text);
  }
  return out;
}

// Read a context-atlas Lisp file from disk and return all projected atlas
// forms in source order. Throws (with file context) when the reader
// fails. Callers that need validation should run main() /
// validateAtlasObject() separately.
export function readContextAtlasFile(file) {
  const forms = readLispFile(file);
  const out = [];
  for (const form of forms) {
    const projected = projectAtlas(form);
    if (projected) out.push(projected);
  }
  return out;
}

// Validate an already-projected atlas object against the schema (re-uses
// the same enum / path / uniqueness rules as the on-disk path). Returns an
// array of error message strings; empty array means valid. wave29-03 prep
// CLI / wave29-06 planner will call this on freshly-emitted CLI output
// before writing it anywhere.
export function validateAtlasObject(obj) {
  const errors = [];
  if (!obj || typeof obj !== 'object') {
    errors.push('atlas must be an object');
    return errors;
  }
  if (!ACCEPTED_SCHEMAS.has(obj.schema)) {
    errors.push(
      `:schema must be one of {${[...ACCEPTED_SCHEMAS].join(' | ')}}, got ${JSON.stringify(obj.schema)}`,
    );
  }
  if (!Array.isArray(obj.read_order) || obj.read_order.length === 0) {
    errors.push(':read-order must be a non-empty array of repo-relative paths');
  } else {
    for (const entry of obj.read_order) {
      if (typeof entry !== 'string' || entry.trim() === '') {
        errors.push(':read-order entries must be non-empty strings');
        break;
      }
      if (path.isAbsolute(entry) || entry.startsWith('~')) {
        errors.push(`:read-order entry must be repo-relative, got "${entry}"`);
        break;
      }
      if (entry.split(/[\\/]/).some((seg) => seg === '..')) {
        errors.push(`:read-order entry must not contain "..", got "${entry}"`);
        break;
      }
    }
  }
  if ((obj.purpose == null || obj.purpose === '') && (obj.goal == null || obj.goal === '')) {
    errors.push('atlas must declare one of :purpose | :goal (synonyms)');
  }
  if (obj.wave != null && obj.wave !== '' && !WAVE_ID_RE.test(obj.wave)) {
    errors.push(`:wave "${obj.wave}" must match ${WAVE_ID_RE}`);
  }

  const seenFilePaths = new Set();
  for (const f of obj.files ?? []) {
    if (!f || typeof f !== 'object') continue;
    if (typeof f.path !== 'string' || f.path.trim() === '') {
      errors.push('(file ...) entries must declare a non-empty path string');
      continue;
    }
    if (path.isAbsolute(f.path) || f.path.startsWith('~')) {
      errors.push(`(file ...) path must be repo-relative, got "${f.path}"`);
      continue;
    }
    if (f.path.split(/[\\/]/).some((seg) => seg === '..')) {
      errors.push(`(file ...) path must not contain "..", got "${f.path}"`);
      continue;
    }
    if (seenFilePaths.has(f.path)) {
      errors.push(`duplicate file path "${f.path}" in atlas`);
      continue;
    }
    seenFilePaths.add(f.path);
    if (Array.isArray(f.grep)) {
      for (const anchor of f.grep) {
        if (typeof anchor !== 'string' || anchor.trim() === '') {
          errors.push(`(file ...) "${f.path}" :grep entries must be non-empty strings`);
          break;
        }
      }
    }
  }
  const seenTaskIds = new Set();
  for (const t of obj.tasks ?? []) {
    if (!t || typeof t !== 'object') continue;
    if (typeof t.task_id !== 'string' || !TASK_ID_RE.test(t.task_id)) {
      errors.push(`(task ...) :task_id "${t.task_id}" must match ${TASK_ID_RE}`);
      continue;
    }
    if (seenTaskIds.has(t.task_id)) {
      errors.push(`duplicate task id "${t.task_id}" in atlas`);
      continue;
    }
    seenTaskIds.add(t.task_id);
    if (Array.isArray(t.first_reads)) {
      for (const entry of t.first_reads) {
        if (typeof entry !== 'string' || entry.trim() === '') {
          errors.push(`task "${t.task_id}" :first-reads entries must be non-empty strings`);
          break;
        }
        if (path.isAbsolute(entry) || entry.startsWith('~')) {
          errors.push(`task "${t.task_id}" :first-reads must be repo-relative, got "${entry}"`);
          break;
        }
        if (entry.split(/[\\/]/).some((seg) => seg === '..')) {
          errors.push(`task "${t.task_id}" :first-reads must not contain "..", got "${entry}"`);
          break;
        }
      }
    }
    if (Array.isArray(t.avoid)) {
      for (const entry of t.avoid) {
        if (typeof entry !== 'string' || entry.trim() === '') {
          errors.push(`task "${t.task_id}" :avoid entries must be non-empty strings`);
          break;
        }
      }
    }
  }
  for (const g of obj.greps ?? []) {
    if (!g || typeof g.pattern !== 'string' || g.pattern.trim() === '') {
      errors.push('(grep ...) entries must declare a non-empty pattern string');
    }
  }
  for (const a of obj.avoids ?? []) {
    if (!a || !Array.isArray(a.notes) || a.notes.length === 0) {
      errors.push('(avoid ...) entries must declare at least one non-empty note string');
      continue;
    }
    for (const note of a.notes) {
      if (typeof note !== 'string' || note.trim() === '') {
        errors.push('(avoid ...) note entries must be non-empty strings');
        break;
      }
    }
  }
  // At least one entry overall.
  const totalEntries =
    (obj.files?.length ?? 0) +
    (obj.tasks?.length ?? 0) +
    (obj.greps?.length ?? 0) +
    (obj.avoids?.length ?? 0);
  if (totalEntries === 0) {
    errors.push('atlas must declare at least one entry (file / task / grep / avoid)');
  }
  return errors;
}

// ---------------------------------------------------------------------------
// Self-contained pass/fail fixtures.
// ---------------------------------------------------------------------------

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'pass-minimal-valid-atlas',
      category: 'pass',
      source: `(context-atlas atlas-min
        :schema "${SCHEMA}"
        :purpose "Smallest legal atlas: header + one file entry."
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "scripts/check-context-atlas.mjs"
          :purpose "checker entry point"
          :grep ["export function projectAtlas"]))`,
      ok: true,
    },
    {
      name: 'pass-mixed-containers-and-flat-entries',
      category: 'pass',
      source: `(context-atlas atlas-mixed
        :schema "${SCHEMA}"
        :purpose "Mix global-anchors / task-focus containers with flat top-level entries."
        :read-order ["scripts/check-context-atlas.mjs"
                     ".missiond/tasks/schema/context-atlas-v1.lisp"]
        (global-anchors
          (file "scripts/check-context-atlas.mjs"
            :purpose "checker"
            :grep ["validateAtlas" "projectAtlas"])
          (file "scripts/lib/missiond_lisp.mjs"
            :purpose "shared parser"))
        (task-focus
          (task wave29-03-runner-wave-prep-v0
            :first-reads ["scripts/render-wave-briefs.mjs"]
            :avoid ["Do not change task contract schema."]))
        (grep "validateAtlasObject"
          :purpose "named export consumed by wave29-03 / wave29-06")
        (avoid "Do not promote atlas to a behavioral contract."))`,
      ok: true,
    },
    {
      name: 'pass-dispatch-v0-schema-string-accepted',
      category: 'pass',
      source: `(context-atlas atlas-dispatch
        :schema "missiond.context-atlas.dispatch.v0"
        :goal "Transitional dispatch-time atlas accepted by v1 checker."
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "scripts/check-context-atlas.mjs"
          :purpose "checker"))`,
      ok: true,
    },
    {
      name: 'fail-schema-mismatch',
      category: 'fail-schema',
      source: `(context-atlas atlas-bad-schema
        :schema "missiond.context-atlas.v0"
        :purpose "Wrong schema string."
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-missing-read-order',
      category: 'fail-missing-required',
      source: `(context-atlas atlas-no-read-order
        :schema "${SCHEMA}"
        :purpose "Missing :read-order header."
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-missing-purpose-and-goal',
      category: 'fail-missing-required',
      source: `(context-atlas atlas-no-purpose
        :schema "${SCHEMA}"
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-empty-read-order',
      category: 'fail-empty-read-order',
      source: `(context-atlas atlas-empty-read-order
        :schema "${SCHEMA}"
        :purpose "Empty read order vector."
        :read-order []
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-absolute-path-in-read-order',
      category: 'fail-path',
      source: `(context-atlas atlas-abs-read-order
        :schema "${SCHEMA}"
        :purpose "Absolute path in read order."
        :read-order ["/etc/passwd"]
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-traversal-in-file-path',
      category: 'fail-path',
      source: `(context-atlas atlas-traversal-file
        :schema "${SCHEMA}"
        :purpose "Path traversal in file entry."
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "../../etc/passwd"
          :purpose "evil"))`,
      ok: false,
    },
    {
      name: 'fail-tilde-in-first-reads',
      category: 'fail-path',
      source: `(context-atlas atlas-tilde-first-reads
        :schema "${SCHEMA}"
        :purpose "Tilde-prefixed path in :first-reads."
        :read-order ["scripts/check-context-atlas.mjs"]
        (task_focus wave29-99-foo
          :first-reads ["~/some/path"]))`,
      ok: false,
    },
    {
      name: 'fail-duplicate-file-path-within-atlas',
      category: 'fail-duplicate',
      source: `(context-atlas atlas-dup-file
        :schema "${SCHEMA}"
        :purpose "Duplicate file path across nested + flat shapes."
        :read-order ["scripts/check-context-atlas.mjs"]
        (global-anchors
          (file "scripts/check-context-atlas.mjs"
            :purpose "first"))
        (file "scripts/check-context-atlas.mjs"
          :purpose "duplicate"))`,
      ok: false,
    },
    {
      name: 'fail-duplicate-task-focus-id',
      category: 'fail-duplicate',
      source: `(context-atlas atlas-dup-task
        :schema "${SCHEMA}"
        :purpose "Duplicate task id across container + flat shapes."
        :read-order ["scripts/check-context-atlas.mjs"]
        (task-focus
          (task wave29-99-foo
            :first-reads ["scripts/check-context-atlas.mjs"]))
        (task_focus wave29-99-foo
          :first-reads ["scripts/check-context-atlas.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-empty-grep-anchor',
      category: 'fail-empty-grep',
      source: `(context-atlas atlas-empty-grep
        :schema "${SCHEMA}"
        :purpose "Empty grep anchor on file entry."
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "scripts/check-context-atlas.mjs"
          :grep [""]))`,
      ok: false,
    },
    {
      name: 'fail-empty-grep-pattern-top-level',
      category: 'fail-empty-grep',
      source: `(context-atlas atlas-empty-grep-pattern
        :schema "${SCHEMA}"
        :purpose "Empty pattern on top-level grep entry."
        :read-order ["scripts/check-context-atlas.mjs"]
        (grep ""))`,
      ok: false,
    },
    {
      name: 'fail-malformed-read-order-not-list',
      category: 'fail-malformed-read-order',
      source: `(context-atlas atlas-bad-read-order
        :schema "${SCHEMA}"
        :purpose "read-order is a string, not a vector."
        :read-order "scripts/check-context-atlas.mjs"
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-multiple-atlas-forms-in-one-file',
      category: 'fail-multiple-atlases',
      source: `(context-atlas atlas-one
        :schema "${SCHEMA}"
        :purpose "First atlas."
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "scripts/check-context-atlas.mjs"))
       (context-atlas atlas-two
        :schema "${SCHEMA}"
        :purpose "Second atlas in same file is rejected."
        :read-order ["scripts/check-context-atlas.mjs"]
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-unknown-top-level-entry-head',
      category: 'fail-unknown-entry-head',
      source: `(context-atlas atlas-bad-entry
        :schema "${SCHEMA}"
        :purpose "Unknown top-level entry head."
        :read-order ["scripts/check-context-atlas.mjs"]
        (waypoint "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    {
      name: 'fail-global-anchors-non-file-child',
      category: 'fail-container-child',
      source: `(context-atlas atlas-bad-anchors-child
        :schema "${SCHEMA}"
        :purpose "global-anchors with a non-file child."
        :read-order ["scripts/check-context-atlas.mjs"]
        (global-anchors
          (task wave29-99-foo
            :first-reads ["scripts/check-context-atlas.mjs"])))`,
      ok: false,
    },
    {
      name: 'fail-task-focus-non-task-child',
      category: 'fail-container-child',
      source: `(context-atlas atlas-bad-tf-child
        :schema "${SCHEMA}"
        :purpose "task-focus container with a non-task child."
        :read-order ["scripts/check-context-atlas.mjs"]
        (task-focus
          (file "scripts/check-context-atlas.mjs")))`,
      ok: false,
    },
    {
      name: 'fail-zero-entries',
      category: 'fail-empty-atlas',
      source: `(context-atlas atlas-empty
        :schema "${SCHEMA}"
        :purpose "Atlas with no entries."
        :read-order ["scripts/check-context-atlas.mjs"])`,
      ok: false,
    },
    {
      name: 'fail-unknown-header-field',
      category: 'fail-unknown-field',
      source: `(context-atlas atlas-bad-header
        :schema "${SCHEMA}"
        :purpose "Unknown header field."
        :read-order ["scripts/check-context-atlas.mjs"]
        :extraneous "no such field"
        (file "scripts/check-context-atlas.mjs"))`,
      ok: false,
    },
    // wave29-07 cross-layer smoke: the real wave29 dispatch atlas on disk
    // MUST validate clean through the in-process projectAtlas +
    // validateAtlasObject pair the prep CLI / planner depend on. This is
    // the layer-A pin for the wave29-07 cross-layer smoke — a regression
    // where the durable schema drifts away from the live dispatch shape
    // surfaces here BEFORE downstream tooling notices.
    {
      name: 'wave29-07-loop-smoke-real-wave29-atlas-validates',
      category: 'wave29-07-loop-smoke',
      source: null,
      ok: true,
      run: () => {
        const realPath = path.resolve(
          process.cwd(),
          '.missiond/tasks/wave29/context-atlas.lisp',
        );
        if (!fs.existsSync(realPath)) {
          throw new Error(`real wave29 context atlas missing at ${realPath}`);
        }
        const projected = readContextAtlasFile(realPath);
        if (projected.length !== 1) {
          throw new Error(
            `expected exactly 1 (context-atlas ...) form on disk, got ${projected.length}`,
          );
        }
        const errors = validateAtlasObject(projected[0]);
        if (errors.length !== 0) {
          throw new Error(
            `real wave29 atlas failed validation:\n  ${errors.join('\n  ')}`,
          );
        }
      },
    },
  ];

  let failed = 0;
  const categories = new Set();
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    const file = `<fixture:${fixture.name}>`;
    // wave29-07 hook: a fixture with `run` runs an out-of-band check
    // (e.g. against a real on-disk atlas via the named exports). It
    // throws on failure; otherwise it counts as ok.
    if (typeof fixture.run === 'function') {
      let runOk = true;
      let runErr = null;
      try {
        fixture.run();
      } catch (err) {
        runOk = false;
        runErr = err;
      }
      if (runOk !== fixture.ok) {
        failed += 1;
        console.error(
          `fixture failed: ${fixture.name} (expected ok=${fixture.ok}, got ok=${runOk})`,
        );
        if (runErr) console.error(`  ${runErr.message}`);
      }
      continue;
    }
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
    const atlasForms = forms.filter((f) => isList(f) && head(f) === ATLAS_HEAD);
    const otherForms = forms.filter((f) => isList(f) && head(f) !== ATLAS_HEAD);
    for (const other of otherForms) {
      addError(
        diagnostics,
        file,
        other.loc,
        `unknown top-level form head "${head(other)}"; expected "${ATLAS_HEAD}"`,
      );
    }
    if (atlasForms.length > 1) {
      for (let i = 1; i < atlasForms.length; i++) {
        addError(
          diagnostics,
          file,
          atlasForms[i].loc,
          `file contains multiple (${ATLAS_HEAD} ...) forms; atlases are single-document`,
        );
      }
    }
    for (const form of atlasForms) {
      validateAtlas(file, form, diagnostics);
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
      `context-atlas fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `context-atlas fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

// Only run the CLI when invoked directly. Keeps named exports clean for
// future tooling (wave29-03 prep CLI, wave29-06 ready-queue planner).
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
