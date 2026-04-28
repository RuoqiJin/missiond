#!/usr/bin/env node

// MissionD verification-receipt v1 checker (read-only).
//
// Validates Lisp records of the form:
//   (verification-receipt-set <set-id> :schema "missiond.verification-receipt.v1"
//                                       [:version v1] [:wave <wave>]
//                                       [:generated_at <iso-8601>] [:notes ...]
//      (receipt <receipt-id> :wave <wave> :task_id <task-id>
//                            :commit_hash <hex> :command <string>
//                            :exit_code <int> :tier local|smoke|full
//                            (:started_at + :finished_at) | :duration_ms
//                            [:files [<repo-rel-path> ...]] [:notes ...])
//      ...)
// OR
//   (verification-receipt <receipt-id> :schema "missiond.verification-receipt.v1"
//                                      ...same body fields as above...)
//
// IMPORTANT (load-bearing): receipts are ADVISORY ACCELERATION ONLY. They
// are NEVER a substitute for source facts and NEVER a substitute for
// commit verification. The reuse helper exported below
// (`isReceiptReusable`) returns true ONLY when ALL FOUR conservative
// reuse rules hold:
//   1. receipt.exit_code === 0           (non-zero exit = invalid evidence)
//   2. receipt.commit_hash matches query (hex-prefix-agree, shorter >= 7)
//   3. receipt.command.trim() === query  (no fuzzy / argv-reorder matching)
//   4. receipt.tier covers query tier    (full > smoke > local; asymmetric)
// Any failure → reuse=false. When in doubt, RE-RUN the command. The schema
// header in .missiond/tasks/schema/verification-receipt-v1.lisp documents
// the same rules for human readers.

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
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
  node scripts/check-verification-receipt.mjs [--json] [--stdin] [--dry-fixture] [<file.lisp> ...]

Checks MissionD verification-receipt v1 Lisp records (read-only):
  - validates reader syntax through the shared MissionD Lisp parser
  - accepts two equivalent top-level shapes:
      (verification-receipt-set <set-id> :schema "missiond.verification-receipt.v1"
                                          [:version v1] [:wave <wave>] [:generated_at ...]
                                          (receipt <receipt-id> ...) ...)
      (verification-receipt     <receipt-id> :schema "missiond.verification-receipt.v1" ...)
  - rejects: schema mismatch; missing required receipt field
    (:wave :task_id :commit_hash :command :exit_code :tier); missing
    BOTH timing forms (need :duration_ms OR :started_at+:finished_at);
    asymmetric :started_at without :finished_at (or vice versa);
    malformed :wave / :task_id / :commit_hash; :task_id not starting
    with :wave + '-' (stale wave/task mismatch); :command empty /
    non-string; :exit_code non-integer; :tier not in {local|smoke|full};
    :duration_ms negative / non-integer; :started_at / :finished_at
    not a valid ISO-8601 timestamp; absolute / ~ / .. paths in :files;
    duplicate :receipt_id within a file (and across the input set);
    second-form id and :receipt_id keyword disagreement; unknown
    receipt field; unknown entry head.

Receipts are ADVISORY ACCELERATION only. The orchestrator MUST still
verify task contract, report, memory completion, and git commit even
when receipts are present. The exported \`isReceiptReusable\` helper
encodes the conservative reuse rules (commit + command + zero exit +
tier-cover all required) — call it instead of re-implementing the
rules in downstream planners.

Use --stdin to read a verification-receipt record from stdin.
Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.verification-receipt.v1';
export const SET_HEAD = 'verification-receipt-set';
export const SINGLE_HEAD = 'verification-receipt';
export const RECEIPT_HEAD = 'receipt';
export const TIERS = Object.freeze(['local', 'smoke', 'full']);

const TIER_SET = new Set(TIERS);
// Tier covering matrix: full > smoke > local. Asymmetric — a `full`
// receipt can satisfy a `smoke` reuse query, but a `local` receipt
// cannot satisfy a `smoke` query. Anchored in the schema header.
const TIER_COVERS = Object.freeze({
  full: new Set(['full', 'smoke', 'local']),
  smoke: new Set(['smoke', 'local']),
  local: new Set(['local']),
});

const RECEIPT_ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
const WAVE_ID_RE = /^[a-z][a-z0-9-]*$/;
const TASK_ID_RE = /^[a-z0-9][a-z0-9._-]*$/;
const COMMIT_HASH_RE = /^[0-9a-f]{7,64}$/i;

// ISO-8601 — accepts the YYYY-MM-DDTHH:MM:SS(.fff)? followed by either Z
// or a +HH:MM / -HH:MM offset. The wave29 timestamps in shared-memory /
// session-trace use the +08:00 form; the wave28 fixtures used Z. Both
// are accepted. We additionally feed the value to Date.parse to reject
// e.g. February 31.
const ISO_8601_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/;

const REQUIRED_RECEIPT_FIELDS = [
  ':wave',
  ':task_id',
  ':commit_hash',
  ':command',
  ':exit_code',
  ':tier',
];
const OPTIONAL_RECEIPT_FIELDS = [
  ':receipt_id',
  ':started_at',
  ':finished_at',
  ':duration_ms',
  ':files',
  ':notes',
];
const ALLOWED_RECEIPT_FIELDS = new Set([
  ...REQUIRED_RECEIPT_FIELDS,
  ...OPTIONAL_RECEIPT_FIELDS,
]);

const HEADER_OPTIONAL_FIELDS = [
  ':schema',
  ':version',
  ':wave',
  ':generated_at',
  ':notes',
];
const HEADER_ALLOWED_SET = new Set([
  ...HEADER_OPTIONAL_FIELDS,
  ...REQUIRED_RECEIPT_FIELDS,
  ...OPTIONAL_RECEIPT_FIELDS,
]);

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
  // Cross-file id uniqueness — the schema rule is "duplicate :receipt_id
  // within a file (or across input set)". A single CLI invocation may
  // batch several receipt files; we surface a duplicate across them.
  const seenReceiptIds = new Map(); // id -> { file, loc }
  let receiptsValidated = 0;

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
    receiptsValidated += validateForms(src.file, forms, diagnostics, seenReceiptIds);
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
          receipts_validated: receiptsValidated,
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
      `verification-receipt check OK (${receiptsValidated} receipt${receiptsValidated === 1 ? '' : 's'}${wText})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `verification-receipt check FAILED — ${errors.length} error(s), ${warnings.length} warning(s), ${receiptsValidated} receipt(s)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

// --------------------------------------------------------------------------
// Validation pipeline.
// --------------------------------------------------------------------------

function validateForms(file, forms, diagnostics, seenReceiptIds) {
  let receiptsValidated = 0;
  for (const form of forms) {
    if (!isList(form)) continue;
    const h = head(form);
    if (h === SET_HEAD) {
      receiptsValidated += validateReceiptContainer(file, form, diagnostics, seenReceiptIds);
    } else if (h === SINGLE_HEAD) {
      const ok = validateReceiptForm(file, form, diagnostics, seenReceiptIds);
      if (ok) receiptsValidated += 1;
    } else {
      addError(
        diagnostics,
        file,
        form.loc,
        `unknown entry head "${h}"; expected "${SET_HEAD}" or "${SINGLE_HEAD}"`,
      );
    }
  }
  return receiptsValidated;
}

function validateReceiptContainer(file, form, diagnostics, seenReceiptIds) {
  const setId = nodeText(form.children[1]);
  if (!setId) {
    addError(
      diagnostics,
      file,
      form.loc,
      `(${SET_HEAD} ...) missing set id as second form`,
    );
  } else if (!RECEIPT_ID_RE.test(setId)) {
    addError(
      diagnostics,
      file,
      form.children[1].loc,
      `(${SET_HEAD} ...) set id "${setId}" must match ${RECEIPT_ID_RE}`,
    );
  }

  const props = readKeywordProps(form, { start: 2 });

  if (!props[':schema']) {
    addError(
      diagnostics,
      file,
      form.loc,
      `(${SET_HEAD} ${setId ?? '<missing>'} ...) missing required header field :schema`,
    );
  } else {
    const schemaText = keywordPropText(props, ':schema');
    if (schemaText !== SCHEMA) {
      addError(
        diagnostics,
        file,
        props[':schema'].value?.loc ?? form.loc,
        `:schema must be ${JSON.stringify(SCHEMA)}, got ${JSON.stringify(schemaText)}`,
      );
    }
  }

  // Optional :wave shape.
  if (props[':wave']) {
    const wave = keywordPropText(props, ':wave');
    if (wave == null || !WAVE_ID_RE.test(wave)) {
      addError(
        diagnostics,
        file,
        props[':wave'].value?.loc ?? form.loc,
        `:wave "${wave}" must match ${WAVE_ID_RE}`,
      );
    }
  }

  // Optional :generated_at — must be a valid ISO-8601 if present.
  if (props[':generated_at']) {
    const ts = keywordPropText(props, ':generated_at');
    if (!isValidIso8601(ts)) {
      addError(
        diagnostics,
        file,
        props[':generated_at'].value?.loc ?? form.loc,
        `:generated_at "${ts}" must be a valid ISO-8601 timestamp`,
      );
    }
  }

  // Optional non-empty header strings.
  for (const field of [':version', ':notes']) {
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

  // Iterate (receipt ...) children — anything else under
  // (verification-receipt-set ...) is an unknown entry head.
  let receiptsValidated = 0;
  for (const child of form.children.slice(2)) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== RECEIPT_HEAD) {
      addError(
        diagnostics,
        file,
        child.loc,
        `unknown entry head "${h}" inside (${SET_HEAD} ...); expected "${RECEIPT_HEAD}"`,
      );
      continue;
    }
    const ok = validateReceiptForm(file, child, diagnostics, seenReceiptIds);
    if (ok) receiptsValidated += 1;
  }
  return receiptsValidated;
}

function validateReceiptForm(file, form, diagnostics, seenReceiptIds) {
  const headValue = head(form);
  const isSingle = headValue === SINGLE_HEAD;
  const secondFormText = nodeText(form.children[1]);

  const props = readKeywordProps(form, { start: 2 });

  // Single-receipt form ALSO carries header fields (:schema / :version);
  // multi-container receipts do not (header lives on the container).
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
          `:schema must be ${JSON.stringify(SCHEMA)} on single-receipt form, got ${JSON.stringify(schemaText)}`,
        );
      }
    }
    for (const field of [':version', ':notes', ':generated_at']) {
      if (!props[field]) continue;
      if (field === ':generated_at') {
        const ts = keywordPropText(props, field);
        if (!isValidIso8601(ts)) {
          addError(
            diagnostics,
            file,
            props[field].value?.loc ?? form.loc,
            `:generated_at "${ts}" must be a valid ISO-8601 timestamp`,
          );
        }
      } else {
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
  }

  // -----------------------------------------------------------------------
  // :receipt_id resolution. The id comes from the second form (canonical)
  // and may also be supplied via the :receipt_id keyword. If both present
  // they MUST agree. If neither, the checker derives a deterministic id
  // (see below) so the duplicate-id pass still works.
  // -----------------------------------------------------------------------
  let receiptId = null;
  let idLoc = null;
  if (
    secondFormText &&
    (form.children[1].type === 'atom' || form.children[1].type === 'string')
  ) {
    receiptId = secondFormText;
    idLoc = form.children[1].loc;
  }
  if (props[':receipt_id']) {
    const keyId = keywordPropText(props, ':receipt_id');
    if (receiptId == null) {
      receiptId = keyId;
      idLoc = props[':receipt_id'].value?.loc ?? form.loc;
    } else if (keyId != null && keyId !== receiptId) {
      addError(
        diagnostics,
        file,
        props[':receipt_id'].value?.loc ?? form.loc,
        `receipt :receipt_id keyword "${keyId}" disagrees with second-form id "${receiptId}"`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // Required-field presence + unknown-field rejection.
  // -----------------------------------------------------------------------
  let hasMissingRequired = false;
  for (const field of REQUIRED_RECEIPT_FIELDS) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        form.loc,
        `receipt "${receiptId ?? '<unknown>'}" missing required field ${field}`,
      );
      hasMissingRequired = true;
    }
  }
  const allowedKeys = isSingle ? HEADER_ALLOWED_SET : ALLOWED_RECEIPT_FIELDS;
  for (const key of Object.keys(props)) {
    if (allowedKeys.has(key)) continue;
    addError(
      diagnostics,
      file,
      props[key].keyNode.loc,
      `receipt "${receiptId ?? '<unknown>'}" has unknown field ${key}`,
    );
  }

  if (hasMissingRequired) {
    // Continue validating fields we DO have so the user sees all the
    // structural issues at once, not just the first.
  }

  // -----------------------------------------------------------------------
  // :wave / :task_id shape + alignment.
  // -----------------------------------------------------------------------
  const waveText = keywordPropText(props, ':wave');
  if (props[':wave']) {
    if (waveText == null || waveText.trim() === '' || !WAVE_ID_RE.test(waveText)) {
      addError(
        diagnostics,
        file,
        props[':wave'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :wave "${waveText}" must match ${WAVE_ID_RE}`,
      );
    }
  }
  const taskIdText = keywordPropText(props, ':task_id');
  if (props[':task_id']) {
    if (taskIdText == null || taskIdText.trim() === '' || !TASK_ID_RE.test(taskIdText)) {
      addError(
        diagnostics,
        file,
        props[':task_id'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :task_id "${taskIdText}" must match ${TASK_ID_RE}`,
      );
    } else if (waveText && WAVE_ID_RE.test(waveText) && !taskIdText.startsWith(`${waveText}-`)) {
      // Stale wave/task mismatch — this is the load-bearing defence
      // against accidentally-mixed receipt files (e.g. wave28 receipts
      // pasted into a wave29 receipts file).
      addError(
        diagnostics,
        file,
        props[':task_id'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :task_id "${taskIdText}" must start with :wave prefix "${waveText}-" (stale wave/task mismatch)`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // :commit_hash shape.
  // -----------------------------------------------------------------------
  const commitText = keywordPropText(props, ':commit_hash');
  if (props[':commit_hash']) {
    if (commitText == null || !COMMIT_HASH_RE.test(commitText)) {
      addError(
        diagnostics,
        file,
        props[':commit_hash'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :commit_hash "${commitText}" must match ${COMMIT_HASH_RE}`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // :command — non-empty string (whitespace preserved as-recorded).
  // -----------------------------------------------------------------------
  const commandText = keywordPropText(props, ':command');
  if (props[':command']) {
    if (commandText == null || commandText.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':command'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :command must be a non-empty string`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // :exit_code — integer (Number.isInteger semantics in the helper).
  // -----------------------------------------------------------------------
  const exitNode = props[':exit_code']?.value;
  if (props[':exit_code']) {
    const exitText = nodeText(exitNode);
    if (exitText == null || !/^-?\d+$/.test(exitText)) {
      addError(
        diagnostics,
        file,
        exitNode?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :exit_code must be an integer (got ${JSON.stringify(exitText)})`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // :tier — enum {local|smoke|full}.
  // -----------------------------------------------------------------------
  const tierText = keywordPropText(props, ':tier');
  if (props[':tier']) {
    if (tierText == null || !TIER_SET.has(tierText)) {
      addError(
        diagnostics,
        file,
        props[':tier'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :tier "${tierText}" not in enum {${TIERS.join('|')}}`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // Timing — need (:started_at + :finished_at) OR :duration_ms (or both).
  // Both forms are independently advisory; the checker does NOT
  // arithmetic-cross-check them.
  // -----------------------------------------------------------------------
  const hasStarted = !!props[':started_at'];
  const hasFinished = !!props[':finished_at'];
  const hasDuration = !!props[':duration_ms'];

  if (!hasStarted && !hasFinished && !hasDuration) {
    addError(
      diagnostics,
      file,
      form.loc,
      `receipt "${receiptId ?? '<unknown>'}" missing timing — need :duration_ms OR (:started_at + :finished_at)`,
    );
  }
  if (hasStarted !== hasFinished) {
    addError(
      diagnostics,
      file,
      (hasStarted ? props[':started_at'] : props[':finished_at']).value?.loc ?? form.loc,
      `receipt "${receiptId ?? '<unknown>'}" :started_at and :finished_at must be supplied together (got only :${hasStarted ? 'started_at' : 'finished_at'})`,
    );
  }
  if (hasStarted) {
    const ts = keywordPropText(props, ':started_at');
    if (!isValidIso8601(ts)) {
      addError(
        diagnostics,
        file,
        props[':started_at'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :started_at "${ts}" must be a valid ISO-8601 timestamp`,
      );
    }
  }
  if (hasFinished) {
    const ts = keywordPropText(props, ':finished_at');
    if (!isValidIso8601(ts)) {
      addError(
        diagnostics,
        file,
        props[':finished_at'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :finished_at "${ts}" must be a valid ISO-8601 timestamp`,
      );
    }
  }
  if (hasDuration) {
    const dNode = props[':duration_ms'].value;
    const dText = nodeText(dNode);
    if (dText == null || !/^-?\d+$/.test(dText)) {
      addError(
        diagnostics,
        file,
        dNode?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :duration_ms must be a non-negative integer (got ${JSON.stringify(dText)})`,
      );
    } else {
      const d = Number(dText);
      if (!Number.isInteger(d) || d < 0) {
        addError(
          diagnostics,
          file,
          dNode?.loc ?? form.loc,
          `receipt "${receiptId ?? '<unknown>'}" :duration_ms must be a non-negative integer (got ${d})`,
        );
      }
    }
  }

  // -----------------------------------------------------------------------
  // :files — optional vector of repo-relative path strings.
  // -----------------------------------------------------------------------
  if (props[':files']) {
    const node = props[':files'].value;
    if (!isList(node)) {
      addError(
        diagnostics,
        file,
        node?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :files must be a vector/list of repo-relative path strings`,
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
            `receipt "${receiptId ?? '<unknown>'}" :files entries must be non-empty path strings`,
          );
          continue;
        }
        if (path.isAbsolute(text) || text.startsWith('~')) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `receipt "${receiptId ?? '<unknown>'}" :files entry must be repo-relative (no leading '/' or '~'), got "${text}"`,
          );
          continue;
        }
        if (text.split(/[\\/]/).some((seg) => seg === '..')) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `receipt "${receiptId ?? '<unknown>'}" :files entry must not contain ".." traversal, got "${text}"`,
          );
          continue;
        }
        if (seenPaths.has(text)) {
          addError(
            diagnostics,
            file,
            child.loc ?? node.loc,
            `receipt "${receiptId ?? '<unknown>'}" :files contains duplicate path "${text}"`,
          );
          continue;
        }
        seenPaths.add(text);
      }
    }
  }

  // -----------------------------------------------------------------------
  // Optional non-empty :notes.
  // -----------------------------------------------------------------------
  if (props[':notes']) {
    const text = keywordPropText(props, ':notes');
    if (text == null || text.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':notes'].value?.loc ?? form.loc,
        `receipt "${receiptId ?? '<unknown>'}" :notes must be a non-empty string when present`,
      );
    }
  }

  // -----------------------------------------------------------------------
  // Receipt-id derivation when neither second-form nor :receipt_id was
  // supplied — derive deterministically from {wave}-{task_id}-{commit[:7]}-
  // {tier} so the duplicate-id pass below still works. This derivation is
  // ONLY used for de-duplication — the receipt-id-shape rule still applies.
  // -----------------------------------------------------------------------
  if (!receiptId) {
    if (waveText && taskIdText && commitText && tierText) {
      receiptId = `${waveText}-${taskIdText}-${commitText.slice(0, 7).toLowerCase()}-${tierText}`;
      idLoc = form.loc;
    } else {
      // Cannot derive — leave id null; downstream uniqueness check skipped.
    }
  }
  if (receiptId != null && !RECEIPT_ID_RE.test(receiptId)) {
    addError(
      diagnostics,
      file,
      idLoc ?? form.loc,
      `receipt id "${receiptId}" must match ${RECEIPT_ID_RE}`,
    );
  }
  if (receiptId != null) {
    if (seenReceiptIds.has(receiptId)) {
      const prior = seenReceiptIds.get(receiptId);
      addError(
        diagnostics,
        file,
        idLoc ?? form.loc,
        `duplicate receipt id "${receiptId}" (also defined at ${prior.file}:${prior.loc?.line ?? 1})`,
      );
      return false;
    }
    seenReceiptIds.set(receiptId, { file, loc: idLoc ?? form.loc });
  }

  // Treat the receipt as "validated" for counting purposes whenever the
  // structural required-field set is satisfied. Diagnostic count is the
  // ground truth for ok/fail; this counter is purely informational.
  return !hasMissingRequired;
}

// --------------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------------

function addError(diagnostics, file, loc, message) {
  diagnostics.push({
    severity: 'error',
    file,
    line: loc?.line ?? 1,
    column: loc?.column ?? 1,
    message,
  });
}

function isValidIso8601(text) {
  if (typeof text !== 'string' || text.length === 0) return false;
  if (!ISO_8601_RE.test(text)) return false;
  const t = Date.parse(text);
  return Number.isFinite(t);
}

// --------------------------------------------------------------------------
// Named exports for downstream tooling. wave29-06 / wave29-07 / future
// planner tasks consume these instead of re-implementing schema rules.
// READ-ONLY: parse, project, return — no mutation, no I/O writes.
// --------------------------------------------------------------------------

// Project a single (receipt ...) or (verification-receipt ...) form into a
// structured object. Returns null when the form is not a receipt.
// Returned shape:
//   { id, wave, task_id, commit_hash, command, exit_code, tier,
//     started_at, finished_at, duration_ms, files: string[], notes, loc }
export function projectReceipt(form) {
  if (!isList(form)) return null;
  const h = head(form);
  if (h !== RECEIPT_HEAD && h !== SINGLE_HEAD) return null;
  const props = readKeywordProps(form, { start: 2 });
  const secondFormText = nodeText(form.children[1]);
  const idFromKeyword = keywordPropText(props, ':receipt_id');
  const id = secondFormText ?? idFromKeyword ?? null;
  const exitText = nodeText(props[':exit_code']?.value);
  const dText = nodeText(props[':duration_ms']?.value);
  return {
    id,
    wave: keywordPropText(props, ':wave') ?? null,
    task_id: keywordPropText(props, ':task_id') ?? null,
    commit_hash: keywordPropText(props, ':commit_hash') ?? null,
    command: keywordPropText(props, ':command') ?? null,
    exit_code: exitText != null && /^-?\d+$/.test(exitText) ? Number(exitText) : null,
    tier: keywordPropText(props, ':tier') ?? null,
    started_at: keywordPropText(props, ':started_at') ?? null,
    finished_at: keywordPropText(props, ':finished_at') ?? null,
    duration_ms: dText != null && /^-?\d+$/.test(dText) ? Number(dText) : null,
    files: vectorToStrings(props[':files']?.value),
    notes: keywordPropText(props, ':notes') ?? null,
    loc: form.loc ?? null,
  };
}

// Read a verification-receipt Lisp file from disk and return all projected
// receipt objects in source order (flattening (verification-receipt-set ...)
// containers). Throws (with file context) when the reader fails.
export function readVerificationReceiptFile(file) {
  const forms = readLispFile(file);
  const out = [];
  for (const form of forms) {
    if (!isList(form)) continue;
    const h = head(form);
    if (h === SET_HEAD) {
      for (const child of form.children.slice(2)) {
        if (!isList(child) || head(child) !== RECEIPT_HEAD) continue;
        const projected = projectReceipt(child);
        if (projected) out.push(projected);
      }
    } else if (h === SINGLE_HEAD) {
      const projected = projectReceipt(form);
      if (projected) out.push(projected);
    }
  }
  return out;
}

// Validate an already-projected receipt object against the schema. Returns
// an array of error message strings; empty array means valid. Mirrors the
// on-disk validator's required-field / shape / path / timing rules. Used
// by verify-task-runner-batch when it loads --receipts via the helper.
export function validateReceiptObject(obj) {
  const errors = [];
  if (!obj || typeof obj !== 'object') {
    errors.push('receipt must be an object');
    return errors;
  }
  if (typeof obj.wave !== 'string' || !WAVE_ID_RE.test(obj.wave)) {
    errors.push(`receipt :wave "${obj.wave}" must match ${WAVE_ID_RE}`);
  }
  if (typeof obj.task_id !== 'string' || !TASK_ID_RE.test(obj.task_id)) {
    errors.push(`receipt :task_id "${obj.task_id}" must match ${TASK_ID_RE}`);
  } else if (typeof obj.wave === 'string' && WAVE_ID_RE.test(obj.wave) && !obj.task_id.startsWith(`${obj.wave}-`)) {
    errors.push(`receipt :task_id "${obj.task_id}" must start with :wave prefix "${obj.wave}-" (stale wave/task mismatch)`);
  }
  if (typeof obj.commit_hash !== 'string' || !COMMIT_HASH_RE.test(obj.commit_hash)) {
    errors.push(`receipt :commit_hash "${obj.commit_hash}" must match ${COMMIT_HASH_RE}`);
  }
  if (typeof obj.command !== 'string' || obj.command.trim() === '') {
    errors.push('receipt :command must be a non-empty string');
  }
  if (!Number.isInteger(obj.exit_code)) {
    errors.push('receipt :exit_code must be an integer');
  }
  if (typeof obj.tier !== 'string' || !TIER_SET.has(obj.tier)) {
    errors.push(`receipt :tier "${obj.tier}" not in enum {${TIERS.join('|')}}`);
  }
  const hasStarted = typeof obj.started_at === 'string';
  const hasFinished = typeof obj.finished_at === 'string';
  const hasDuration = obj.duration_ms != null;
  if (!hasStarted && !hasFinished && !hasDuration) {
    errors.push('receipt missing timing — need :duration_ms OR (:started_at + :finished_at)');
  }
  if (hasStarted !== hasFinished) {
    errors.push('receipt :started_at and :finished_at must be supplied together');
  }
  if (hasStarted && !isValidIso8601(obj.started_at)) {
    errors.push(`receipt :started_at "${obj.started_at}" must be a valid ISO-8601 timestamp`);
  }
  if (hasFinished && !isValidIso8601(obj.finished_at)) {
    errors.push(`receipt :finished_at "${obj.finished_at}" must be a valid ISO-8601 timestamp`);
  }
  if (hasDuration && (!Number.isInteger(obj.duration_ms) || obj.duration_ms < 0)) {
    errors.push('receipt :duration_ms must be a non-negative integer');
  }
  if (Array.isArray(obj.files)) {
    for (const entry of obj.files) {
      if (typeof entry !== 'string' || entry.trim() === '') {
        errors.push('receipt :files entries must be non-empty strings');
        break;
      }
      if (path.isAbsolute(entry) || entry.startsWith('~')) {
        errors.push(`receipt :files entry must be repo-relative, got "${entry}"`);
        break;
      }
      if (entry.split(/[\\/]/).some((seg) => seg === '..')) {
        errors.push(`receipt :files entry must not contain "..", got "${entry}"`);
        break;
      }
    }
  } else if (obj.files != null) {
    errors.push('receipt :files must be an array (or omitted)');
  }
  return errors;
}

// Hex-prefix-agree comparator — mirrors the wave29-04 / verify-task-runner-
// batch local helper so reuse rules use the same logic the batch verifier
// already runs against the lineage hashes.
function commitHashesAgreeHex(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  const ax = a.trim().toLowerCase();
  const bx = b.trim().toLowerCase();
  if (ax.length === 0 || bx.length === 0) return false;
  if (!/^[0-9a-f]+$/.test(ax) || !/^[0-9a-f]+$/.test(bx)) return false;
  if (ax === bx) return true;
  const longer = ax.length >= bx.length ? ax : bx;
  const shorter = ax.length >= bx.length ? bx : ax;
  if (shorter.length >= 7 && longer.startsWith(shorter)) return true;
  return false;
}

// Canonical conservative reuse rules — exported so wave29-06 / wave29-07
// and future planners NEVER re-implement these rules. ALL FOUR conditions
// MUST hold for a receipt to be reusable. Any failure → false.
//
// IMPORTANT: this function caches command-evidence reuse — it does NOT
// substitute for source facts and does NOT substitute for commit
// verification. Callers that decide to skip a re-run based on this
// helper MUST still verify the underlying task contract / report /
// memory completion / git commit through the orchestrator's normal
// path.
export function isReceiptReusable(receipt, query) {
  if (!receipt || typeof receipt !== 'object') return false;
  if (!query || typeof query !== 'object') return false;

  // Rule 3: zero exit code.
  if (!Number.isInteger(receipt.exit_code) || receipt.exit_code !== 0) {
    return false;
  }
  // Rule 1: commit hash agreement (exact OR hex-prefix-agree).
  if (!commitHashesAgreeHex(receipt.commit_hash, query.commit_hash)) {
    return false;
  }
  // Rule 2: command exact match (after .trim()). No fuzzy / argv-reorder
  // matching — receipts cache a SPECIFIC command line.
  const a = typeof receipt.command === 'string' ? receipt.command.trim() : null;
  const b = typeof query.command === 'string' ? query.command.trim() : null;
  if (a == null || b == null || a !== b) {
    return false;
  }
  // Rule 4: tier covering. full > smoke > local; asymmetric.
  const cover = TIER_COVERS[receipt.tier];
  if (!cover || !cover.has(query.tier)) {
    return false;
  }
  return true;
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

// --------------------------------------------------------------------------
// Self-contained pass/fail fixtures.
// --------------------------------------------------------------------------

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'pass-single-receipt-minimal-with-duration',
      category: 'pass',
      source: `(verification-receipt wave99-01-foo-aaaaaaa-smoke
        :schema "${SCHEMA}"
        :version "v1"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "aaaaaaa1234"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :duration_ms 1500
        :files ["scripts/foo.mjs"])`,
      ok: true,
    },
    {
      name: 'pass-single-receipt-with-start-finish-no-duration',
      category: 'pass-start-finish',
      source: `(verification-receipt wave99-02-bar-bbbbbbb-full
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-02-bar
        :commit_hash "bbbbbbb"
        :command "node scripts/bar.mjs --dry-fixture"
        :exit_code 0
        :tier full
        :started_at "2026-04-28T15:00:00Z"
        :finished_at "2026-04-28T15:00:02Z")`,
      ok: true,
    },
    {
      name: 'pass-multi-receipt-container',
      category: 'pass-multi',
      source: `(verification-receipt-set wave99-batch
        :schema "${SCHEMA}"
        :version "v1"
        :wave wave99
        :generated_at "2026-04-28T15:10:00+08:00"
        (receipt wave99-01-foo-cccccccc-local
          :wave wave99
          :task_id wave99-01-foo
          :commit_hash "cccccccc"
          :command "node scripts/check-x.mjs --dry-fixture"
          :exit_code 0
          :tier local
          :duration_ms 800)
        (receipt wave99-02-bar-cccccccc-smoke
          :wave wave99
          :task_id wave99-02-bar
          :commit_hash "cccccccc"
          :command "node scripts/check-y.mjs --dry-fixture"
          :exit_code 0
          :tier smoke
          :duration_ms 1200))`,
      ok: true,
    },
    {
      name: 'pass-non-zero-exit-receipt-still-validates-structurally',
      category: 'pass-non-zero-exit',
      // Schema-validation note: a non-zero exit_code is STRUCTURALLY valid
      // (the schema accepts negative / zero / positive integers). The
      // reuse helper returns false for non-zero exit; that semantic
      // check happens in the planner, not the structural checker.
      source: `(verification-receipt wave99-01-foo-ddddddd-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "ddddddd"
        :command "node scripts/check-z.mjs --dry-fixture"
        :exit_code 1
        :tier smoke
        :duration_ms 200)`,
      ok: true,
    },
    {
      name: 'fail-empty-command',
      category: 'fail-empty-command',
      source: `(verification-receipt wave99-01-foo-eeeeeee-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "eeeeeee"
        :command ""
        :exit_code 0
        :tier smoke
        :duration_ms 1)`,
      ok: false,
    },
    {
      name: 'fail-negative-duration-ms',
      category: 'fail-bad-duration',
      source: `(verification-receipt wave99-01-foo-fffffff-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "fffffff"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :duration_ms -10)`,
      ok: false,
    },
    {
      name: 'fail-non-integer-exit-code',
      category: 'fail-bad-exit-code',
      source: `(verification-receipt wave99-01-foo-1111111-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "1111111"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code "zero"
        :tier smoke
        :duration_ms 5)`,
      ok: false,
    },
    {
      name: 'fail-tier-out-of-enum',
      category: 'fail-bad-tier',
      source: `(verification-receipt wave99-01-foo-2222222-cosmic
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "2222222"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier cosmic
        :duration_ms 5)`,
      ok: false,
    },
    {
      name: 'fail-malformed-commit-hash',
      category: 'fail-bad-commit',
      source: `(verification-receipt wave99-01-foo-shortz-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "zzz"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :duration_ms 5)`,
      ok: false,
    },
    {
      name: 'fail-absolute-files-path',
      category: 'fail-bad-path',
      source: `(verification-receipt wave99-01-foo-3333333-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "3333333"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :duration_ms 5
        :files ["/etc/passwd"])`,
      ok: false,
    },
    {
      name: 'fail-traversal-files-path',
      category: 'fail-bad-path',
      source: `(verification-receipt wave99-01-foo-4444444-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "4444444"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :duration_ms 5
        :files ["../../etc/passwd"])`,
      ok: false,
    },
    {
      name: 'fail-stale-wave-task-mismatch',
      category: 'fail-stale-wave-task',
      source: `(verification-receipt wave99-foo-5555555-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave28-01-pasted-here
        :commit_hash "5555555"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :duration_ms 5)`,
      ok: false,
    },
    {
      name: 'fail-duplicate-receipt-id-in-container',
      category: 'fail-duplicate-id',
      source: `(verification-receipt-set dup-set
        :schema "${SCHEMA}"
        (receipt wave99-01-foo-6666666-smoke
          :wave wave99
          :task_id wave99-01-foo
          :commit_hash "6666666"
          :command "node scripts/foo.mjs --dry-fixture"
          :exit_code 0
          :tier smoke
          :duration_ms 5)
        (receipt wave99-01-foo-6666666-smoke
          :wave wave99
          :task_id wave99-01-foo
          :commit_hash "6666666"
          :command "node scripts/foo.mjs --dry-fixture"
          :exit_code 0
          :tier smoke
          :duration_ms 5))`,
      ok: false,
    },
    {
      name: 'fail-missing-required-field-task-id',
      category: 'fail-missing-required',
      source: `(verification-receipt wave99-01-foo-7777777-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :commit_hash "7777777"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :duration_ms 5)`,
      ok: false,
    },
    {
      name: 'fail-missing-both-timing-forms',
      category: 'fail-missing-timing',
      source: `(verification-receipt wave99-01-foo-8888888-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "8888888"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke)`,
      ok: false,
    },
    {
      name: 'fail-bad-iso-8601-started-at',
      category: 'fail-bad-iso-8601',
      source: `(verification-receipt wave99-01-foo-9999999-smoke
        :schema "${SCHEMA}"
        :wave wave99
        :task_id wave99-01-foo
        :commit_hash "9999999"
        :command "node scripts/foo.mjs --dry-fixture"
        :exit_code 0
        :tier smoke
        :started_at "April 28 2026 noon"
        :finished_at "2026-04-28T15:00:02Z")`,
      ok: false,
    },
  ];

  let failed = 0;
  const categories = new Set();
  const reuseHelperResults = [];

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
    validateForms(file, forms, diagnostics, new Map());
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

  // Reuse-helper fixtures — pin all four conservative rules. These exercise
  // isReceiptReusable explicitly so the contract requirement #4 (conservative
  // reuse) is mechanically tested, not just documented.
  const baseReceipt = {
    id: 'r-1',
    wave: 'wave99',
    task_id: 'wave99-01-foo',
    commit_hash: 'abc1234567',
    command: 'node scripts/foo.mjs --dry-fixture',
    exit_code: 0,
    tier: 'smoke',
    started_at: null,
    finished_at: null,
    duration_ms: 100,
    files: [],
    notes: null,
    loc: null,
  };
  const reuseChecks = [
    {
      name: 'reuse: exact commit + command + tier ⇒ true',
      receipt: baseReceipt,
      query: { commit_hash: 'abc1234567', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
      expect: true,
    },
    {
      name: 'reuse: hex-prefix commit (longer query) ⇒ true',
      receipt: baseReceipt,
      query: { commit_hash: 'abc12345678901234567890', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
      expect: true,
    },
    {
      name: 'reuse: full receipt covers smoke query ⇒ true',
      receipt: { ...baseReceipt, tier: 'full' },
      query: { commit_hash: 'abc1234567', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
      expect: true,
    },
    {
      name: 'reuse rule 4 fail: local receipt CANNOT cover smoke query ⇒ false',
      receipt: { ...baseReceipt, tier: 'local' },
      query: { commit_hash: 'abc1234567', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
      expect: false,
    },
    {
      name: 'reuse rule 3 fail: non-zero exit ⇒ false',
      receipt: { ...baseReceipt, exit_code: 1 },
      query: { commit_hash: 'abc1234567', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
      expect: false,
    },
    {
      name: 'reuse rule 2 fail: command differs ⇒ false',
      receipt: baseReceipt,
      query: { commit_hash: 'abc1234567', command: 'node scripts/foo.mjs --different', tier: 'smoke' },
      expect: false,
    },
    {
      name: 'reuse rule 1 fail: commit unrelated ⇒ false',
      receipt: baseReceipt,
      query: { commit_hash: 'cafebabe', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
      expect: false,
    },
  ];
  for (const r of reuseChecks) {
    categories.add('reuse-helper');
    const got = isReceiptReusable(r.receipt, r.query);
    const ok = got === r.expect;
    reuseHelperResults.push({ name: r.name, expect: r.expect, got, ok });
    if (!ok) {
      failed += 1;
      console.error(
        `reuse-helper fixture failed: ${r.name} (expected ${r.expect}, got ${got})`,
      );
    }
  }

  // wave29-07 cross-layer smoke (layer E): pin the conservative-reuse
  // contract as a single composite check. One synthetic receipt + 5
  // queries: matching-all → reusable=true; wrong commit → false; wrong
  // command → false; wrong tier → false (local CANNOT cover smoke); and
  // non-zero exit → false. ANY single failed sub-check fails the whole
  // smoke fixture. This is the layer-E pin: a regression that loosens any
  // one of the four conservative rules surfaces here, near the receipt
  // checker, BEFORE the batch verifier or planner is misled.
  categories.add('wave29-07-loop-smoke');
  {
    const wave2907Receipt = {
      id: 'wave29-07-foo-1234567-smoke',
      wave: 'wave99',
      task_id: 'wave99-01-foo',
      commit_hash: '1234567abc',
      command: 'node scripts/foo.mjs --dry-fixture',
      exit_code: 0,
      tier: 'smoke',
      started_at: null,
      finished_at: null,
      duration_ms: 250,
      files: [],
      notes: null,
      loc: null,
    };
    const wave2907Subchecks = [
      {
        sub: 'matching-all-4-rules ⇒ true',
        receipt: wave2907Receipt,
        query: { commit_hash: '1234567abc', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
        expect: true,
      },
      {
        sub: 'wrong commit ⇒ false',
        receipt: wave2907Receipt,
        query: { commit_hash: 'cafebabe', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
        expect: false,
      },
      {
        sub: 'wrong command ⇒ false',
        receipt: wave2907Receipt,
        query: { commit_hash: '1234567abc', command: 'node scripts/foo.mjs --different-flag', tier: 'smoke' },
        expect: false,
      },
      {
        sub: 'wrong tier (local CANNOT cover smoke) ⇒ false',
        receipt: { ...wave2907Receipt, tier: 'local' },
        query: { commit_hash: '1234567abc', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
        expect: false,
      },
      {
        sub: 'non-zero exit ⇒ false',
        receipt: { ...wave2907Receipt, exit_code: 1 },
        query: { commit_hash: '1234567abc', command: 'node scripts/foo.mjs --dry-fixture', tier: 'smoke' },
        expect: false,
      },
    ];
    let smokeOk = true;
    const subFailures = [];
    for (const c of wave2907Subchecks) {
      const got = isReceiptReusable(c.receipt, c.query);
      if (got !== c.expect) {
        smokeOk = false;
        subFailures.push(`${c.sub} (expected ${c.expect}, got ${got})`);
      }
    }
    reuseHelperResults.push({
      name: 'wave29-07-loop-smoke-receipt-reuse-conservative',
      expect: true,
      got: smokeOk,
      ok: smokeOk,
    });
    if (!smokeOk) {
      failed += 1;
      console.error(
        `wave29-07-loop-smoke-receipt-reuse-conservative FAILED: ${subFailures.join(' | ')}`,
      );
    }
  }

  if (json) {
    console.log(
      JSON.stringify(
        {
          ok: failed === 0,
          fixtures: fixtures.length + reuseHelperResults.length,
          failed,
          categories: [...categories].sort(),
          reuse_helper: reuseHelperResults,
        },
        null,
        2,
      ),
    );
  }
  if (failed > 0) {
    console.error(
      `verification-receipt fixtures FAILED — ${failed} of ${fixtures.length + reuseHelperResults.length}`,
    );
    process.exit(1);
  }
  if (!json) {
    console.log(
      `verification-receipt fixtures OK (${fixtures.length} structural + ${reuseHelperResults.length} reuse-helper, ${categories.size} categories)`,
    );
  }
}

// Only run the CLI when invoked directly. Keeps named exports clean for
// downstream tooling (wave29-06 ready-queue planner / wave29-07 cross-layer
// smoke / verify-task-runner-batch --receipts).
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
