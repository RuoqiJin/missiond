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
  node scripts/check-pattern-card.mjs [--json] [--stdin] [--dry-fixture] [<file.lisp> ...]

Checks MissionD pattern-card v1 Lisp records (read-only):
  - validates reader syntax through the shared MissionD Lisp parser
  - accepts two equivalent top-level shapes:
      (pattern-cards <set-id> :schema "missiond.pattern-card.v1"
                              [:version v1] [:wave <wave>]
                              (card <card-id> ...) ...)
      (pattern-card  <card-id> :schema "missiond.pattern-card.v1" ...)
  - accepts the LEGACY :schema value "missiond.pattern-cards.dispatch.v0"
    on the (pattern-cards ...) container so the existing wave29
    dispatch-time pattern-cards.lisp validates UNMODIFIED.
  - rejects: schema mismatch; missing card :id / :use-for / :recipe;
    duplicate card :id within a single file; empty :recipe vector;
    :recipe entry that is empty / non-string; malformed :id; :use-for
    entry not matching the task-id pattern; absolute / ~ / .. paths in
    :known-good; second-form id and :id keyword disagreement; unknown
    card field; unknown entry head.
  - warns: missing :known-good keyword entirely (acceptable for cards
    seeded before any exemplar exists); empty :known-good vector;
    :known-good path not present on disk.

Aliases normalized (kebab canonical, underscore accepted):
  :use_for       -> :use-for
  :known_good    -> :known-good
  :anti_pattern  -> :anti-pattern
  :non_goals     -> :non-goals

Use --stdin to read a pattern-card record from stdin.
Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.pattern-card.v1';
export const LEGACY_SCHEMA_DISPATCH = 'missiond.pattern-cards.dispatch.v0';
export const ACCEPTED_SCHEMAS = new Set([SCHEMA, LEGACY_SCHEMA_DISPATCH]);

export const SET_HEAD = 'pattern-cards';
export const SINGLE_HEAD = 'pattern-card';
export const CARD_HEAD = 'card';

const CARD_ID_RE = /^[a-z][a-z0-9-]*$/;
const TASK_ID_PATTERN_RE = /^[a-z0-9][a-z0-9._-]*$/;
const WAVE_ID_RE = /^[a-z][a-z0-9-]*$/;

const ALIAS_TABLE = {
  ':use_for': ':use-for',
  ':known_good': ':known-good',
  ':anti_pattern': ':anti-pattern',
  ':non_goals': ':non-goals',
};

// :known-good is RECOMMENDED but not strictly required: the wave29
// dispatch-time pattern-cards.lisp ships two cards (cross-layer-smoke,
// large-file-navigation) that intentionally omit :known-good because
// no exemplar file exists for those patterns yet. We surface the absence
// as a warning so newly-seeded cards land cleanly while drift toward
// permanent omission stays visible. The contract's :known-good rule is
// enforced at the seed-card layer (.missiond/patterns/*.pattern.lisp),
// not at the schema layer.
//
// Field sets are EXPORTED so downstream tooling (wave29-03 brief renderer,
// wave29-06 ready-queue planner, wave29-07 cross-layer smoke) can
// introspect the schema surface without re-implementing the field list.
export const REQUIRED_CARD_FIELDS = [':use-for', ':recipe'];
export const RECOMMENDED_CARD_FIELDS = [':known-good'];
export const OPTIONAL_CARD_FIELDS = [
  ':id',
  ':purpose',
  ':summary',
  ':known-good',
  ':anti-pattern',
  ':non-goals',
  ':notes',
];
const ALLOWED_CARD_FIELDS = new Set([...REQUIRED_CARD_FIELDS, ...OPTIONAL_CARD_FIELDS]);

export const HEADER_OPTIONAL_FIELDS = [':schema', ':version', ':wave', ':purpose', ':summary', ':notes'];
const HEADER_ALLOWED_SET = new Set([...HEADER_OPTIONAL_FIELDS, ...REQUIRED_CARD_FIELDS, ...OPTIONAL_CARD_FIELDS]);

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
  let cardsValidated = 0;

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
    cardsValidated += validateForms(src.file, forms, diagnostics, {
      allowDiskCheck: src.file !== '<stdin>',
    });
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
          cards_validated: cardsValidated,
          errors,
          warnings,
        },
        null,
        2,
      ),
    );
  } else if (ok) {
    for (const w of warnings) {
      console.warn(`${w.file}:${w.line}:${w.column}: warning: ${w.message}`);
    }
    const wText = warnings.length > 0 ? `, ${warnings.length} warning(s)` : '';
    console.log(
      `pattern-card check OK (${cardsValidated} card${cardsValidated === 1 ? '' : 's'}${wText})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `pattern-card check FAILED — ${errors.length} error(s), ${warnings.length} warning(s), ${cardsValidated} card(s)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

// Validate every top-level form in a file. Returns the number of cards
// validated. Diagnostics are appended to the supplied array.
function validateForms(file, forms, diagnostics, opts = {}) {
  const seenIdsAcrossFile = new Map(); // id -> first form loc
  let cardsValidated = 0;
  for (const form of forms) {
    if (!isList(form)) continue;
    const h = head(form);
    if (h === SET_HEAD) {
      cardsValidated += validateCardsContainer(file, form, diagnostics, seenIdsAcrossFile, opts);
    } else if (h === SINGLE_HEAD) {
      // Treat the (pattern-card ...) form itself as a card.
      const ok = validateCardForm(file, form, diagnostics, seenIdsAcrossFile, {
        idSource: 'second-form',
        allowDiskCheck: opts.allowDiskCheck,
        allowHeaderFields: true,
      });
      if (ok) cardsValidated += 1;
    } else {
      addError(
        diagnostics,
        file,
        form.loc,
        `unknown entry head "${h}"; expected "${SET_HEAD}" or "${SINGLE_HEAD}"`,
      );
    }
  }
  return cardsValidated;
}

function validateCardsContainer(file, form, diagnostics, seenIdsAcrossFile, opts) {
  const setId = nodeText(form.children[1]);
  if (!setId) {
    addError(
      diagnostics,
      file,
      form.loc,
      `(${SET_HEAD} ...) missing set id as second form`,
    );
  } else if (!CARD_ID_RE.test(setId)) {
    addError(
      diagnostics,
      file,
      form.children[1].loc,
      `(${SET_HEAD} ...) set id "${setId}" must match ${CARD_ID_RE}`,
    );
  }

  const props = normalizeKeywordProps(readKeywordProps(form, { start: 2 }));

  // Header :schema must be an accepted value (canonical or legacy dispatch).
  if (!props[':schema']) {
    addError(
      diagnostics,
      file,
      form.loc,
      `(${SET_HEAD} ${setId ?? '<missing>'} ...) missing required header field :schema`,
    );
  } else {
    const schemaText = keywordPropText(props, ':schema');
    if (!ACCEPTED_SCHEMAS.has(schemaText)) {
      addError(
        diagnostics,
        file,
        props[':schema'].value?.loc ?? form.loc,
        `:schema must be "${SCHEMA}" (or legacy "${LEGACY_SCHEMA_DISPATCH}"), got ${JSON.stringify(schemaText)}`,
      );
    }
  }

  // Optional :wave shape.
  const wave = keywordPropText(props, ':wave');
  if (props[':wave'] && (wave == null || !WAVE_ID_RE.test(wave))) {
    addError(
      diagnostics,
      file,
      props[':wave'].value?.loc ?? form.loc,
      `:wave "${wave}" must match ${WAVE_ID_RE}`,
    );
  }

  // Optional non-empty header strings.
  for (const field of [':version', ':purpose', ':summary', ':notes']) {
    if (!props[field]) continue;
    const text = keywordPropText(props, field);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[field].value?.loc ?? form.loc,
        `${field} must be a non-empty string when present (omit the field if empty)`,
      );
    }
  }

  // Iterate (card ...) children — anything else under (pattern-cards ...)
  // is an unknown entry head.
  let cardsValidated = 0;
  for (const child of form.children.slice(2)) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== CARD_HEAD) {
      addError(
        diagnostics,
        file,
        child.loc,
        `unknown entry head "${h}" inside (${SET_HEAD} ...); expected "${CARD_HEAD}"`,
      );
      continue;
    }
    const ok = validateCardForm(file, child, diagnostics, seenIdsAcrossFile, {
      idSource: 'second-form',
      allowDiskCheck: opts.allowDiskCheck,
      allowHeaderFields: false,
    });
    if (ok) cardsValidated += 1;
  }
  return cardsValidated;
}

// Validate a single (card <id> ...) or (pattern-card <id> ...) form.
// Returns true on success (so the caller can count cards). Diagnostics
// are appended to the supplied array.
function validateCardForm(file, form, diagnostics, seenIdsAcrossFile, opts) {
  const headValue = head(form);
  const isSingle = headValue === SINGLE_HEAD;
  const secondFormText = nodeText(form.children[1]);

  // Normalize keyword props (alias underscore variants -> kebab canonical).
  const rawProps = readKeywordProps(form, { start: 2 });
  const props = normalizeKeywordProps(rawProps);

  // Single-card form ALSO carries header fields (:schema / :version);
  // multi-container cards do not (header lives on the container).
  if (isSingle) {
    if (!props[':schema']) {
      addError(
        diagnostics,
        file,
        form.loc,
        `(${SINGLE_HEAD} ${secondFormText ?? '<missing>'} ...) missing required header field :schema`,
      );
    } else {
      const schemaText = keywordPropText(props, ':schema');
      if (schemaText !== SCHEMA) {
        addError(
          diagnostics,
          file,
          props[':schema'].value?.loc ?? form.loc,
          `:schema must be "${SCHEMA}" on single-card form, got ${JSON.stringify(schemaText)}`,
        );
      }
    }
    for (const field of [':version', ':purpose', ':summary', ':notes']) {
      if (!props[field]) continue;
      const text = keywordPropText(props, field);
      if (text == null || text.trim() === '') {
        addError(
          diagnostics,
          file,
          props[field].value?.loc ?? form.loc,
          `${field} must be a non-empty string when present (omit the field if empty)`,
        );
      }
    }
  }

  // ---------------------------------------------------------------------
  // :id resolution. The id comes from the second form (canonical) and may
  // also be supplied via the :id keyword. If both are present they MUST
  // agree.
  // ---------------------------------------------------------------------
  let cardId = null;
  let idLoc = null;
  if (secondFormText && form.children[1].type === 'atom') {
    cardId = secondFormText;
    idLoc = form.children[1].loc;
  } else if (secondFormText && form.children[1].type === 'string') {
    cardId = secondFormText;
    idLoc = form.children[1].loc;
  }
  if (props[':id']) {
    const keyId = keywordPropText(props, ':id');
    if (cardId == null) {
      cardId = keyId;
      idLoc = props[':id'].value?.loc ?? form.loc;
    } else if (keyId != null && keyId !== cardId) {
      addError(
        diagnostics,
        file,
        props[':id'].value?.loc ?? form.loc,
        `card :id keyword "${keyId}" disagrees with second-form id "${cardId}"`,
      );
    }
  }
  if (!cardId) {
    addError(
      diagnostics,
      file,
      form.loc,
      `(${headValue} ...) missing card id (provide id as the second form OR via :id)`,
    );
    return false;
  }
  if (!CARD_ID_RE.test(cardId)) {
    addError(
      diagnostics,
      file,
      idLoc ?? form.loc,
      `card id "${cardId}" must match ${CARD_ID_RE}`,
    );
    return false;
  }
  if (seenIdsAcrossFile.has(cardId)) {
    addError(
      diagnostics,
      file,
      idLoc ?? form.loc,
      `duplicate card :id "${cardId}" (also defined at line ${seenIdsAcrossFile.get(cardId).line ?? 1})`,
    );
    return false;
  }
  seenIdsAcrossFile.set(cardId, idLoc ?? form.loc);

  // ---------------------------------------------------------------------
  // Required field presence + unknown-field rejection.
  // ---------------------------------------------------------------------
  for (const field of REQUIRED_CARD_FIELDS) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        form.loc,
        `card "${cardId}" missing required field ${field}`,
      );
    }
  }
  // Unknown-field rejection. On the single-card form, header keywords are
  // ALSO allowed; on the multi-container card form, only the card field
  // surface is allowed.
  const allowedKeys = isSingle ? HEADER_ALLOWED_SET : ALLOWED_CARD_FIELDS;
  for (const key of Object.keys(props)) {
    if (allowedKeys.has(key)) continue;
    addError(
      diagnostics,
      file,
      props[key].keyNode.loc,
      `card "${cardId}" has unknown field ${key}`,
    );
  }

  // ---------------------------------------------------------------------
  // :use-for vector — each entry must match TASK_ID_PATTERN_RE.
  // ---------------------------------------------------------------------
  if (props[':use-for']) {
    const node = props[':use-for'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? form.loc,
        `card "${cardId}" :use-for must be a vector/list (use [] for none)`,
      );
    } else {
      for (const child of node.children) {
        const text = nodeText(child);
        if (text == null || text.trim() === '') {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `card "${cardId}" :use-for entries must be non-empty strings/atoms`,
          );
          continue;
        }
        if (!TASK_ID_PATTERN_RE.test(text)) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `card "${cardId}" :use-for entry "${text}" must match ${TASK_ID_PATTERN_RE}`,
          );
        }
      }
    }
  }

  // ---------------------------------------------------------------------
  // :recipe vector — at least one non-empty step string.
  // ---------------------------------------------------------------------
  if (props[':recipe']) {
    const node = props[':recipe'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? form.loc,
        `card "${cardId}" :recipe must be a vector/list of step strings`,
      );
    } else if (node.children.length === 0) {
      addError(
        diagnostics,
        file,
        node.loc ?? form.loc,
        `card "${cardId}" :recipe must contain at least one step (a card with no steps offers no guidance)`,
      );
    } else {
      let nonEmptyCount = 0;
      for (const child of node.children) {
        const text = nodeText(child);
        if (text == null || text.trim() === '') {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `card "${cardId}" :recipe entries must be non-empty strings`,
          );
          continue;
        }
        nonEmptyCount += 1;
      }
      if (nonEmptyCount === 0) {
        addError(
          diagnostics,
          file,
          node.loc ?? form.loc,
          `card "${cardId}" :recipe must contain at least one non-empty step string`,
        );
      }
    }
  }

  // ---------------------------------------------------------------------
  // Recommended-field warnings. Driven by RECOMMENDED_CARD_FIELDS so
  // adding a new recommended field auto-warns without touching this loop.
  // ---------------------------------------------------------------------
  for (const field of RECOMMENDED_CARD_FIELDS) {
    if (props[field]) continue;
    diagnostics.push({
      severity: 'warning',
      file,
      line: form.loc?.line ?? 1,
      column: form.loc?.column ?? 1,
      message: `card "${cardId}" missing recommended field ${field} (no exemplar yet — acceptable for newly-seeded cards but flagged so we notice when an exemplar exists and should be linked)`,
    });
  }

  // ---------------------------------------------------------------------
  // :known-good vector — repo-relative path strings; empty -> warning.
  // ---------------------------------------------------------------------
  if (props[':known-good']) {
    const node = props[':known-good'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? form.loc,
        `card "${cardId}" :known-good must be a vector/list of repo-relative path strings`,
      );
    } else if (node.children.length === 0) {
      diagnostics.push({
        severity: 'warning',
        file,
        line: node.loc?.line ?? 1,
        column: node.loc?.column ?? 1,
        message: `card "${cardId}" :known-good is empty (no exemplar yet — acceptable for newly-seeded cards)`,
      });
    } else {
      const seenPaths = new Set();
      for (const child of node.children) {
        const text = nodeText(child);
        if (text == null || text.trim() === '') {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `card "${cardId}" :known-good entries must be non-empty path strings`,
          );
          continue;
        }
        if (path.isAbsolute(text) || text.startsWith('~')) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `card "${cardId}" :known-good entry must be repo-relative (no leading '/' or '~'), got "${text}"`,
          );
          continue;
        }
        if (text.split(/[\\/]/).some((seg) => seg === '..')) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `card "${cardId}" :known-good entry must not contain ".." traversal, got "${text}"`,
          );
          continue;
        }
        if (seenPaths.has(text)) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `card "${cardId}" :known-good contains duplicate path "${text}"`,
          );
          continue;
        }
        seenPaths.add(text);
        // Disk-existence check: only when reading from a real file (not
        // stdin / fixtures), and only as a WARNING — the schema does not
        // own the exemplar files.
        if (opts.allowDiskCheck && file !== '<stdin>') {
          const cwd = process.cwd();
          const resolved = path.isAbsolute(text) ? text : path.resolve(cwd, text);
          if (!fs.existsSync(resolved)) {
            diagnostics.push({
              severity: 'warning',
              file,
              line: child.loc?.line ?? 1,
              column: child.loc?.column ?? 1,
              message: `card "${cardId}" :known-good "${text}" not present on disk (warning only — schema does not own the exemplar)`,
            });
          }
        }
      }
    }
  }

  // ---------------------------------------------------------------------
  // Optional vector-of-non-empty-strings fields.
  // ---------------------------------------------------------------------
  for (const field of [':anti-pattern', ':non-goals']) {
    if (!props[field]) continue;
    const node = props[field].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? form.loc,
        `card "${cardId}" ${field} must be a vector/list of non-empty strings`,
      );
      continue;
    }
    for (const child of node.children) {
      const text = nodeText(child);
      if (text == null || text.trim() === '') {
        addError(
          diagnostics,
          file,
          child.loc ?? node.loc,
          `card "${cardId}" ${field} entries must be non-empty strings`,
        );
      }
    }
  }

  // Optional non-empty single-string fields (also valid on multi-container
  // cards).
  for (const field of [':purpose', ':summary', ':notes']) {
    if (!props[field]) continue;
    const text = keywordPropText(props, field);
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[field].value?.loc ?? form.loc,
        `card "${cardId}" ${field} must be a non-empty string when present`,
      );
    }
  }

  return true;
}

// Normalize underscore aliases into the kebab canonical keyword form.
// Returns a NEW props object so callers do not mutate the parser output.
function normalizeKeywordProps(props) {
  const out = {};
  for (const [key, entry] of Object.entries(props)) {
    const canonical = ALIAS_TABLE[key] ?? key;
    if (out[canonical]) {
      // Two keys (alias + canonical) were both supplied. Keep the first
      // and emit a downstream-visible signal by stashing the second under
      // the alias key so the unknown-field pass flags the duplicate.
      out[key] = entry;
    } else {
      out[canonical] = entry;
    }
  }
  return out;
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
// Named exports for downstream tooling. wave29-03 / wave29-06 / wave29-07
// will consume these to project / validate cards without re-implementing the
// schema rules. The functions are READ-ONLY: parse, project, return — they
// do not mutate input nodes and never write files.
// ---------------------------------------------------------------------------

// Project a single (card ...) or (pattern-card ...) form into a structured
// object. Returns null when the form is not a card. Returned shape:
//   { id, use_for: string[], recipe: string[], known_good: string[],
//     purpose, summary, anti_pattern: string[], non_goals: string[],
//     notes, loc }
export function projectCard(form) {
  if (!isList(form)) return null;
  const h = head(form);
  if (h !== CARD_HEAD && h !== SINGLE_HEAD) return null;
  const props = normalizeKeywordProps(readKeywordProps(form, { start: 2 }));
  const secondFormText = nodeText(form.children[1]);
  const id = secondFormText ?? keywordPropText(props, ':id') ?? null;
  return {
    id,
    use_for: vectorToStrings(props[':use-for']?.value),
    recipe: vectorToStrings(props[':recipe']?.value),
    known_good: vectorToStrings(props[':known-good']?.value),
    purpose: keywordPropText(props, ':purpose') ?? null,
    summary: keywordPropText(props, ':summary') ?? null,
    anti_pattern: vectorToStrings(props[':anti-pattern']?.value),
    non_goals: vectorToStrings(props[':non-goals']?.value),
    notes: keywordPropText(props, ':notes') ?? null,
    loc: form.loc ?? null,
  };
}

// Read a pattern-card Lisp file from disk and return all projected card
// objects in source order (flattening (pattern-cards ...) containers).
// Throws (with file context) when the reader fails.
export function readPatternCardFile(file) {
  const forms = readLispFile(file);
  const out = [];
  for (const form of forms) {
    if (!isList(form)) continue;
    const h = head(form);
    if (h === SET_HEAD) {
      for (const child of form.children.slice(2)) {
        if (!isList(child) || head(child) !== CARD_HEAD) continue;
        const projected = projectCard(child);
        if (projected) out.push(projected);
      }
    } else if (h === SINGLE_HEAD) {
      const projected = projectCard(form);
      if (projected) out.push(projected);
    }
  }
  return out;
}

// Validate an already-projected card object against the schema. Returns
// an array of error message strings; empty array means valid. Mirrors
// the on-disk validator's required-field / shape / path rules.
export function validateCardObject(obj) {
  const errors = [];
  if (!obj || typeof obj !== 'object') {
    errors.push('card must be an object');
    return errors;
  }
  if (typeof obj.id !== 'string' || !CARD_ID_RE.test(obj.id)) {
    errors.push(`card :id "${obj.id}" must match ${CARD_ID_RE}`);
  }
  if (!Array.isArray(obj.use_for)) {
    errors.push(`card "${obj.id}" :use-for must be an array`);
  } else {
    for (const entry of obj.use_for) {
      if (typeof entry !== 'string' || !TASK_ID_PATTERN_RE.test(entry)) {
        errors.push(`card "${obj.id}" :use-for entry "${entry}" must match ${TASK_ID_PATTERN_RE}`);
      }
    }
  }
  if (!Array.isArray(obj.recipe) || obj.recipe.length === 0) {
    errors.push(`card "${obj.id}" :recipe must be a non-empty array of step strings`);
  } else {
    for (const entry of obj.recipe) {
      if (typeof entry !== 'string' || entry.trim() === '') {
        errors.push(`card "${obj.id}" :recipe entries must be non-empty strings`);
        break;
      }
    }
  }
  if (!Array.isArray(obj.known_good)) {
    errors.push(`card "${obj.id}" :known-good must be an array`);
  } else {
    for (const entry of obj.known_good) {
      if (typeof entry !== 'string' || entry.trim() === '') {
        errors.push(`card "${obj.id}" :known-good entries must be non-empty strings`);
        break;
      }
      if (path.isAbsolute(entry) || entry.startsWith('~')) {
        errors.push(`card "${obj.id}" :known-good entry must be repo-relative, got "${entry}"`);
        break;
      }
      if (entry.split(/[\\/]/).some((seg) => seg === '..')) {
        errors.push(`card "${obj.id}" :known-good entry must not contain "..", got "${entry}"`);
        break;
      }
    }
  }
  return errors;
}

function vectorToStrings(node) {
  if (!isList(node)) return [];
  const out = [];
  for (const child of node.children) {
    const text = nodeText(child);
    if (text != null && text !== '') out.push(text);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Self-contained pass/fail fixtures.
// ---------------------------------------------------------------------------

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'pass-single-card-minimal',
      category: 'pass',
      source: `(pattern-card schema-checker
        :schema "${SCHEMA}"
        :version "v1"
        :purpose "Recurring schema/checker pattern."
        :use-for [wave29-01-context-atlas-schema-v0]
        :recipe ["Reuse scripts/lib/missiond_lisp.mjs."
                 "Add named exports for downstream consumers."
                 "Ship --json --stdin --dry-fixture flags."]
        :known-good ["scripts/check-task-runner-manifest.mjs"])`,
      ok: true,
    },
    {
      name: 'pass-multi-card-container',
      category: 'pass-multi',
      source: `(pattern-cards demo-set
        :schema "${SCHEMA}"
        :version "v1"
        (card schema-checker
          :use-for [wave29-01 wave29-02]
          :recipe ["Step one." "Step two."]
          :known-good ["scripts/check-task-runner-manifest.mjs"])
        (card node-cli-readonly
          :use-for [wave29-03]
          :recipe ["Step one."]
          :known-good ["scripts/plan-task-runner.mjs"]))`,
      ok: true,
    },
    {
      name: 'pass-legacy-dispatch-schema-accepted',
      category: 'pass-legacy-schema',
      source: `(pattern-cards wave29-runner-efficiency
        :schema "${LEGACY_SCHEMA_DISPATCH}"
        :wave wave29
        (card schema-checker
          :use-for [wave29-01]
          :recipe ["Step one."]
          :known-good ["scripts/check-task-runner-manifest.mjs"]))`,
      ok: true,
    },
    {
      name: 'pass-empty-known-good-warns-not-fails',
      category: 'pass-warn-only',
      source: `(pattern-card new-pattern
        :schema "${SCHEMA}"
        :use-for [wave99-01-stub]
        :recipe ["Single step is enough."]
        :known-good [])`,
      ok: true,
    },
    {
      name: 'pass-underscore-aliases-accepted',
      category: 'pass-aliases',
      source: `(pattern-card aliased-card
        :schema "${SCHEMA}"
        :use_for [wave99-01-foo]
        :recipe ["One."]
        :known_good ["scripts/check-task-runner-manifest.mjs"])`,
      ok: true,
    },
    {
      name: 'fail-duplicate-card-id-in-container',
      category: 'fail-duplicate-id',
      source: `(pattern-cards dup-set
        :schema "${SCHEMA}"
        (card schema-checker
          :use-for [wave99-01]
          :recipe ["Step."]
          :known-good ["scripts/x.mjs"])
        (card schema-checker
          :use-for [wave99-02]
          :recipe ["Step."]
          :known-good ["scripts/y.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-empty-recipe-vector',
      category: 'fail-empty-recipe',
      source: `(pattern-card empty-recipe
        :schema "${SCHEMA}"
        :use-for [wave99-01]
        :recipe []
        :known-good ["scripts/x.mjs"])`,
      ok: false,
    },
    {
      name: 'fail-recipe-entry-empty-string',
      category: 'fail-empty-recipe-entry',
      source: `(pattern-card empty-recipe-entry
        :schema "${SCHEMA}"
        :use-for [wave99-01]
        :recipe ["" "real step"]
        :known-good ["scripts/x.mjs"])`,
      ok: false,
    },
    {
      name: 'fail-absolute-known-good-path',
      category: 'fail-bad-path',
      source: `(pattern-card abs-path
        :schema "${SCHEMA}"
        :use-for [wave99-01]
        :recipe ["Step."]
        :known-good ["/etc/passwd"])`,
      ok: false,
    },
    {
      name: 'fail-traversal-known-good-path',
      category: 'fail-bad-path',
      source: `(pattern-card traversal-path
        :schema "${SCHEMA}"
        :use-for [wave99-01]
        :recipe ["Step."]
        :known-good ["../../etc/passwd"])`,
      ok: false,
    },
    {
      name: 'fail-invalid-use-for-id',
      category: 'fail-bad-use-for',
      source: `(pattern-card bad-use-for
        :schema "${SCHEMA}"
        :use-for ["NOT-a-task-id!"]
        :recipe ["Step."]
        :known-good ["scripts/x.mjs"])`,
      ok: false,
    },
    {
      name: 'fail-missing-required-recipe-field',
      category: 'fail-missing-required',
      source: `(pattern-card missing-recipe
        :schema "${SCHEMA}"
        :use-for [wave99-01]
        :known-good ["scripts/x.mjs"])`,
      ok: false,
    },
    {
      name: 'fail-unknown-entry-head-at-top',
      category: 'fail-unknown-head',
      source: `(my-cards demo
        :schema "${SCHEMA}"
        (card foo
          :use-for [wave99-01]
          :recipe ["Step."]
          :known-good ["scripts/x.mjs"]))`,
      ok: false,
    },
    {
      name: 'fail-second-form-id-vs-keyword-id-mismatch',
      category: 'fail-id-mismatch',
      source: `(pattern-card outer-id
        :schema "${SCHEMA}"
        :id different-id
        :use-for [wave99-01]
        :recipe ["Step."]
        :known-good ["scripts/x.mjs"])`,
      ok: false,
    },
    // wave29-07 cross-layer smoke: all 5 wave29-02 seed pattern files on
    // disk MUST validate clean through the in-process readPatternCardFile +
    // validateCardObject pair. This is the layer-B pin for the wave29-07
    // cross-layer smoke — a regression where the durable schema drifts from
    // any seed card surfaces here BEFORE downstream tooling notices.
    {
      name: 'wave29-07-loop-smoke-5-seed-cards-validate',
      category: 'wave29-07-loop-smoke',
      source: null,
      ok: true,
      run: () => {
        const seedRel = [
          '.missiond/patterns/schema-checker.pattern.lisp',
          '.missiond/patterns/node-cli-readonly.pattern.lisp',
          '.missiond/patterns/report-lineage.pattern.lisp',
          '.missiond/patterns/cross-layer-smoke.pattern.lisp',
          '.missiond/patterns/large-file-navigation.pattern.lisp',
        ];
        for (const rel of seedRel) {
          const abs = path.resolve(process.cwd(), rel);
          if (!fs.existsSync(abs)) {
            throw new Error(`seed pattern file missing at ${abs}`);
          }
          const cards = readPatternCardFile(abs);
          if (cards.length === 0) {
            throw new Error(`seed pattern file ${rel} contained no cards`);
          }
          for (const card of cards) {
            const errors = validateCardObject(card);
            if (errors.length !== 0) {
              throw new Error(
                `seed card ${rel}#${card.id} failed validation:\n  ${errors.join('\n  ')}`,
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
    const file = `<fixture:${fixture.name}>`;
    // wave29-07 hook: a fixture with `run` runs an out-of-band check
    // (e.g. against the 5 wave29-02 seed pattern files on disk via the
    // named exports). It throws on failure; otherwise it counts as ok.
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
    validateForms(file, forms, diagnostics, { allowDiskCheck: false });
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
      `pattern-card fixtures FAILED — ${failed} of ${fixtures.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `pattern-card fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

// Only run the CLI when invoked directly. Keeps named exports clean for
// future tooling (wave29-03 brief renderer / wave29-06 ready-queue planner /
// wave29-07 cross-layer smoke).
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
