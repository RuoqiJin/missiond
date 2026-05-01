#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-file-artifacts-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 file-artifacts Lisp/code isomorphism contract:
  - .missiond/v3/missiond-blueprint.lisp declares (surface file-artifacts ...)
    with :status "code-aligned", :code naming the file_artifacts facade plus
    its attempt/kind/write submodules, and a :note that anchors ArtifactKind,
    atomic_write_artifact, the request-local artifact projection contract, the
    compat path / stable artifact paths (.missiond/alignment, .missiond/plans,
    .missiond/workflows), and the "no partial writes" atomicity invariant.
  - compression-contract :checks pins this checker so drift surfaces in CI.
  - file_artifacts.rs is a thin facade over kind.rs, write.rs, and attempt.rs.
    The combined writer surface exposes the ArtifactKind enum (IntentAlignment
    | Plan | Workflow), atomic_write_artifact, unique_temp_path_in_dir,
    attempt_artifact_write + WriterContext + AttemptOutcome, and the "partial
    writes do not leak" invariant comment that pairs the temp+fsync+rename
    sequence with the file-vs-db contract.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  fileArtifacts: 'crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs',
  fileArtifactsAttempt: 'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/attempt.rs',
  fileArtifactsKind: 'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/kind.rs',
  fileArtifactsWrite: 'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/write.rs',
  fileArtifactsTests: 'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/tests.rs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  if (dryFixture) {
    runFixtures(json);
    return;
  }

  const repoRoot = process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 file-artifacts Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 file-artifacts Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

// Whole-file needles asserted against the blueprint. The (surface file-artifacts
// ...) form must exist with status code-aligned and pin the file_artifacts.rs
// path; the compression-contract must pin THIS checker so the contract is
// self-enforcing.
const BLUEPRINT_NEEDLES = [
  '(surface file-artifacts',
  ':status "code-aligned"',
  'crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs',
  'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/attempt.rs',
  'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/kind.rs',
  'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/write.rs',
  'crates/missiond-daemon/src/handlers/knowledge/file_artifacts/tests.rs',
  'node scripts/check-v3-file-artifacts-isomorphism.mjs',
];

// Anchors that MUST appear inside the (surface file-artifacts ...) body. Using
// a surface-scoped check (instead of whole-file substring search) keeps the
// V3 promise localized — unrelated prose mentioning the same words elsewhere
// in the blueprint must not satisfy the contract.
const SURFACE_NOTE_ANCHORS = [
  'ArtifactKind',
  'atomic_write_artifact',
  'request-local artifact projection',
  'compat path',
  '.missiond/alignment',
  '.missiond/plans',
  '.missiond/workflows',
  'no partial writes',
];

// Whole-file needles asserted against file_artifacts.rs. These pin the
// stable writer surface that mission_request, compile_directive, compile_plan,
// and compile_workflow compose on top of. The "partial writes do not leak"
// phrase pairs with the temp+fsync+rename sequence — losing it would mean
// silent loss of the atomicity invariant the surface note advertises.
const FILE_ARTIFACTS_NEEDLES = [
  'pub(crate) enum ArtifactKind',
  'ArtifactKind::IntentAlignment',
  'ArtifactKind::Plan',
  'ArtifactKind::Workflow',
  'pub(crate) fn artifact_path',
  'pub(crate) fn unique_temp_path_in_dir',
  'pub(crate) fn atomic_write_artifact',
  'pub(crate) async fn attempt_artifact_write',
  'pub(crate) struct WriterContext',
  '#[cfg(test)]',
  'mod tests;',
  'AttemptOutcome::Written',
  'AttemptOutcome::ResolveFailed',
  'AttemptOutcome::WriteFailed',
  // Wrapped doc-comment fragment: "...so partial writes do not\n/// leak across crashes."
  // Substring search must accommodate the line wrap, so we pin the
  // discriminating prefix only — losing it removes the atomicity promise
  // even if the trailing "leak across crashes" survives independently.
  'partial writes do not',
  '.missiond/alignment',
  '.missiond/plans',
  '.missiond/workflows',
];

const FILE_ARTIFACTS_FACADE_NEEDLES = [
  'mod attempt;',
  'mod kind;',
  'mod write;',
  'pub(crate) use attempt::{',
  'pub(crate) use kind::{',
  'pub(crate) use write::{',
  '#[cfg(test)]',
  'mod tests;',
];

const FILE_ARTIFACTS_KIND_NEEDLES = [
  'pub(crate) enum ArtifactKind',
  'ArtifactKind::IntentAlignment',
  'ArtifactKind::Plan',
  'ArtifactKind::Workflow',
  'pub(crate) struct ArtifactSpec',
  'pub(crate) struct WriteOutcome',
  'pub(crate) struct ArtifactMetadata',
  'pub(crate) fn sanitize_topic_segment',
  'pub(crate) fn artifact_path',
  'pub(crate) fn artifact_path_from_spec',
  '.missiond/alignment',
  '.missiond/plans',
  '.missiond/workflows',
];

const FILE_ARTIFACTS_WRITE_NEEDLES = [
  'static TEMP_FILE_COUNTER',
  'pub(crate) fn unique_temp_path_in_dir',
  'pub(crate) fn atomic_write_artifact',
  'pub(crate) fn read_existing_metadata',
  'flushes + fsyncs the temp file before rename so partial writes do not',
];

const FILE_ARTIFACTS_ATTEMPT_NEEDLES = [
  'pub(crate) struct WriterContext',
  'pub(crate) enum AttemptOutcome',
  'AttemptOutcome::Written',
  'AttemptOutcome::ResolveFailed',
  'AttemptOutcome::WriteFailed',
  'pub(crate) async fn attempt_artifact_write',
  'pub(crate) async fn resolve_writer_project_root',
  'file-first writer refuses process-cwd fallback',
];

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, BLUEPRINT_NEEDLES);
  // The (surface file-artifacts ...) note must explicitly anchor the
  // V3 promise (kind enum + atomic write + request-local projection +
  // compat-path stable paths + atomicity) so the contract lives next to
  // the surface declaration, not somewhere unrelated in the blueprint.
  requireSurfaceNoteContains(
    diagnostics,
    files.blueprint,
    sources.blueprint,
    'file-artifacts',
    SURFACE_NOTE_ANCHORS,
  );

  requireAll(diagnostics, files.fileArtifacts, sources.fileArtifacts, FILE_ARTIFACTS_FACADE_NEEDLES);
  requireAll(diagnostics, files.fileArtifactsKind, sources.fileArtifactsKind, FILE_ARTIFACTS_KIND_NEEDLES);
  requireAll(diagnostics, files.fileArtifactsWrite, sources.fileArtifactsWrite, FILE_ARTIFACTS_WRITE_NEEDLES);
  requireAll(
    diagnostics,
    files.fileArtifactsAttempt,
    sources.fileArtifactsAttempt,
    FILE_ARTIFACTS_ATTEMPT_NEEDLES,
  );
  const combinedWriterSurface = [
    sources.fileArtifacts,
    sources.fileArtifactsKind,
    sources.fileArtifactsWrite,
    sources.fileArtifactsAttempt,
  ].join('\n');
  requireAll(diagnostics, files.fileArtifacts, combinedWriterSurface, FILE_ARTIFACTS_NEEDLES);
  requireAll(diagnostics, files.fileArtifactsTests, sources.fileArtifactsTests, [
    'use super::*;',
    'fn sanitize_topic_keeps_safe_chars',
    'fn artifact_path_alignment_lives_under_alignment_topic_dir',
    'fn atomic_write_returns_correct_sha256_and_bytes',
    'fn splice_writes_emits_canonical_keys',
    'fn splice_resolve_failed_downgrades_status_and_includes_error',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

// Find the (surface <name> ...) form by simple paren matching and assert that
// every needle appears inside its body. Identical strategy to
// check-v3-review-gate-isomorphism.mjs — the V3 contract is that the named
// surface itself carries the semantics, not some unrelated section that
// happens to mention the same words.
function requireSurfaceNoteContains(diagnostics, file, source, surfaceName, needles) {
  const opener = `(surface ${surfaceName}`;
  const start = source.indexOf(opener);
  if (start < 0) {
    diagnostics.push({
      file,
      message: `(surface ${surfaceName} ...) form not found; cannot verify surface-local notes: ${needles.join(', ')}`,
    });
    return;
  }
  let depth = 0;
  let end = -1;
  for (let i = start; i < source.length; i += 1) {
    const ch = source[i];
    if (ch === '(') depth += 1;
    else if (ch === ')') {
      depth -= 1;
      if (depth === 0) {
        end = i + 1;
        break;
      }
    }
  }
  if (end < 0) {
    diagnostics.push({
      file,
      message: `(surface ${surfaceName} ...) form is not balanced; cannot verify surface-local notes`,
    });
    return;
  }
  const body = source.slice(start, end);
  for (const needle of needles) {
    if (!body.includes(needle)) {
      diagnostics.push({
        file,
        message: `(surface ${surfaceName} ...) note missing required anchor: ${needle}`,
      });
    }
  }
}

function runFixtures(json) {
  const cases = [];

  // ── Pass: every required key present in every file. ──────────────────
  const goodFiles = {
    [DEFAULT_FILES.blueprint]: buildGoodBlueprint(),
    [DEFAULT_FILES.fileArtifacts]: buildGoodFileArtifacts(),
    [DEFAULT_FILES.fileArtifactsAttempt]: buildGoodFileArtifactsAttempt(),
    [DEFAULT_FILES.fileArtifactsKind]: buildGoodFileArtifactsKind(),
    [DEFAULT_FILES.fileArtifactsWrite]: buildGoodFileArtifactsWrite(),
    [DEFAULT_FILES.fileArtifactsTests]: buildGoodFileArtifactsTests(),
  };
  cases.push({
    name: 'pass: blueprint surface + file_artifacts facade/modules aligned',
    expectOk: true,
    files: goodFiles,
  });

  // ── Fail: blueprint missing the (surface file-artifacts ...) form. ──
  const missingSurface = { ...goodFiles };
  missingSurface[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    '(surface file-artifacts',
    '(surface file-GHOST',
  );
  cases.push({
    name: 'fail: blueprint missing (surface file-artifacts ...)',
    expectOk: false,
    expectMessage: /\(surface file-artifacts/,
    files: missingSurface,
  });

  // ── Fail: blueprint surface body missing the no-partial-writes anchor. ─
  const missingPartialAnchor = { ...goodFiles };
  missingPartialAnchor[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    'no partial writes',
    'no partial GHOST',
  );
  cases.push({
    name: 'fail: blueprint file-artifacts surface note loses no-partial-writes anchor',
    expectOk: false,
    expectMessage: /no partial writes/,
    files: missingPartialAnchor,
  });

  // ── Fail: blueprint surface body missing the request-local anchor. ──
  const missingRequestLocal = { ...goodFiles };
  missingRequestLocal[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    'request-local artifact projection',
    'request-GHOST artifact projection',
  );
  cases.push({
    name: 'fail: blueprint file-artifacts surface note loses request-local-artifact-projection anchor',
    expectOk: false,
    expectMessage: /request-local artifact projection/,
    files: missingRequestLocal,
  });

  // ── Fail: blueprint :status downgraded from code-aligned. ─────────────
  const downgradedStatus = { ...goodFiles };
  downgradedStatus[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    ':status "code-aligned"',
    ':status "draft"',
  );
  cases.push({
    name: 'fail: blueprint file-artifacts surface is no longer code-aligned',
    expectOk: false,
    expectMessage: /code-aligned/,
    files: downgradedStatus,
  });

  // ── Fail: file_artifacts/kind.rs lost ArtifactKind enum. ─────────────
  const missingEnum = { ...goodFiles };
  missingEnum[DEFAULT_FILES.fileArtifactsKind] = goodFiles[DEFAULT_FILES.fileArtifactsKind].replace(
    'pub(crate) enum ArtifactKind',
    'pub(crate) enum ArtifactGHOST',
  );
  cases.push({
    name: 'fail: file_artifacts/kind.rs lost the ArtifactKind enum',
    expectOk: false,
    expectMessage: /pub\(crate\) enum ArtifactKind/,
    files: missingEnum,
  });

  // ── Fail: file_artifacts/write.rs lost atomic_write_artifact. ───────
  const missingAtomic = { ...goodFiles };
  missingAtomic[DEFAULT_FILES.fileArtifactsWrite] = goodFiles[DEFAULT_FILES.fileArtifactsWrite].replace(
    'pub(crate) fn atomic_write_artifact',
    'pub(crate) fn atomic_write_GHOST',
  );
  cases.push({
    name: 'fail: file_artifacts/write.rs lost atomic_write_artifact',
    expectOk: false,
    expectMessage: /atomic_write_artifact/,
    files: missingAtomic,
  });

  // ── Fail: file_artifacts/write.rs lost the no-partial-writes invariant.
  const missingPartialInvariant = { ...goodFiles };
  missingPartialInvariant[DEFAULT_FILES.fileArtifactsWrite] = goodFiles[DEFAULT_FILES.fileArtifactsWrite]
    .replace('partial writes do not', 'partial writes are GHOST');
  cases.push({
    name: 'fail: file_artifacts/write.rs lost the partial-writes-do-not invariant comment',
    expectOk: false,
    expectMessage: /partial writes do not/,
    files: missingPartialInvariant,
  });

  // ── Fail: compression-contract no longer pins this checker. ──────────
  const missingChecker = { ...goodFiles };
  missingChecker[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    'node scripts/check-v3-file-artifacts-isomorphism.mjs',
    'node scripts/check-v3-file-artifacts-GHOST.mjs',
  );
  cases.push({
    name: 'fail: compression-contract :checks dropped this checker',
    expectOk: false,
    expectMessage: /check-v3-file-artifacts-isomorphism/,
    files: missingChecker,
  });

  let failed = 0;
  for (const c of cases) {
    const root = materializeFixture(c.files);
    try {
      const diagnostics = checkFiles(root, DEFAULT_FILES);
      const ok = diagnostics.length === 0;
      if (ok !== c.expectOk) {
        failed += 1;
        console.error(`fixture FAILED: ${c.name}: expected ok=${c.expectOk}, got ok=${ok}`);
        for (const d of diagnostics) console.error(`  ${d.file}: ${d.message}`);
        continue;
      }
      if (c.expectMessage) {
        const messages = diagnostics.map((d) => d.message).join(' | ');
        if (!c.expectMessage.test(messages)) {
          failed += 1;
          console.error(
            `fixture FAILED: ${c.name}: expected diagnostic matching ${c.expectMessage}, got: ${messages || '(none)'}`,
          );
        }
      }
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  }

  if (failed > 0) {
    console.error(`v3 file-artifacts fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 file-artifacts fixtures OK (${cases.length} cases)`);
  }
}

function materializeFixture(filesByPath) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-file-artifacts-iso-'));
  for (const [rel, body] of Object.entries(filesByPath)) {
    const abs = path.join(root, rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, body);
  }
  return root;
}

function buildGoodBlueprint() {
  // Minimal-but-realistic V3 blueprint snippet that satisfies BLUEPRINT_NEEDLES
  // and every SURFACE_NOTE_ANCHORS entry inside the (surface file-artifacts ...)
  // body. The note prose mirrors the shape used by other code-aligned surfaces
  // (review-gate, mission_board) so a real declaration would also pass.
  return `;; fixture
(missiond-blueprint
  (axioms
    (artifact-first
      :rule "Reviewable truth is in Lisp artifacts; DB rows and markdown are projections."))
  (implementation-map
    (surface file-artifacts
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/attempt.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/kind.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/write.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/tests.rs"]
      :note "file-artifacts is the file-first SSOT writer underneath compile_directive / compile_plan / compile_workflow and the request-local artifact projection that mission_request materializes for review. file_artifacts.rs is the facade; kind.rs owns ArtifactKind plus artifact_path and the stable artifact paths (.missiond/alignment/<topic>/intent-alignment.lisp, .missiond/plans/<topic>/PLAN.lisp, .missiond/workflows/<topic>.lisp) which are the V3 compat path served when mission_request opt-in compat_write_file=true; write.rs owns atomic_write_artifact and unique_temp_path_in_dir so callers see no partial writes on crash; attempt.rs owns attempt_artifact_write, WriterContext, AttemptOutcome, and project-root resolution so the file-vs-db contract never silently rolls back a committed row when the file write fails. file_artifacts/tests.rs holds the writer regression suite outside the runtime facade."))
  (compression-contract
    :checks ["node scripts/check-v3-file-artifacts-isomorphism.mjs"]))
`;
}

function buildGoodFileArtifacts() {
  // Minimal facade skeleton; behavior needles live in the split modules.
  return `// fixture
mod attempt;
mod kind;
mod write;

pub(crate) use attempt::{attempt_artifact_write, AttemptOutcome, WriterContext};
pub(crate) use kind::{artifact_path, ArtifactKind};
pub(crate) use write::{atomic_write_artifact, unique_temp_path_in_dir};

#[cfg(test)]
mod tests;
`;
}

function buildGoodFileArtifactsKind() {
  return `// fixture
//! Path convention:
//!   - .missiond/alignment/<topic>/intent-alignment.lisp
//!   - .missiond/plans/<topic>/PLAN.lisp
//!   - .missiond/workflows/<topic>.lisp

pub(crate) enum ArtifactKind { IntentAlignment, Plan, Workflow }
const _: &[&str] = &[
    "ArtifactKind::IntentAlignment",
    "ArtifactKind::Plan",
    "ArtifactKind::Workflow",
];

pub(crate) struct ArtifactSpec;
pub(crate) struct WriteOutcome;
pub(crate) struct ArtifactMetadata;
pub(crate) fn sanitize_topic_segment() {}
pub(crate) fn artifact_path() {}
pub(crate) fn artifact_path_from_spec() {}
`;
}

function buildGoodFileArtifactsWrite() {
  return `// fixture
static TEMP_FILE_COUNTER: usize = 0;
pub(crate) fn unique_temp_path_in_dir() {}
/// flushes + fsyncs the temp file before rename so partial writes do not leak across crashes
pub(crate) fn atomic_write_artifact() {}
pub(crate) fn read_existing_metadata() {}
`;
}

function buildGoodFileArtifactsAttempt() {
  return `// fixture
pub(crate) struct WriterContext;
pub(crate) enum AttemptOutcome { Written, ResolveFailed, WriteFailed }
const _: &[&str] = &[
    "AttemptOutcome::Written",
    "AttemptOutcome::ResolveFailed",
    "AttemptOutcome::WriteFailed",
];
pub(crate) async fn attempt_artifact_write() {}
pub(crate) async fn resolve_writer_project_root() {}
const _: &str = "file-first writer refuses process-cwd fallback";
`;
}

function buildGoodFileArtifactsTests() {
  return `// fixture
use super::*;

fn sanitize_topic_keeps_safe_chars() {}
fn artifact_path_alignment_lives_under_alignment_topic_dir() {}
fn atomic_write_returns_correct_sha256_and_bytes() {}
fn splice_writes_emits_canonical_keys() {}
fn splice_resolve_failed_downgrades_status_and_includes_error() {}
`;
}

main();
