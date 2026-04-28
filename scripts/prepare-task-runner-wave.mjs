#!/usr/bin/env node

// wave29-03 runner-wave preparation CLI.
//
// Purpose: a read-only-plus-file-generation harness that future task-runner
// waves run ONCE before dispatch to:
//   1. validate the manifest (wave28-01 schema/checker; defence-in-depth on
//      productive-only nodes via wave28-03 substring/kind reuse);
//   2. render thin briefs + the shared preamble in-process by delegating to
//      scripts/render-wave-briefs.mjs renderManifest (wave29-03 surgical
//      named export — CLI behavior unchanged);
//   3. create the .missiond/tasks/<wave>/reports/ directory plus a
//      deterministic report skeleton stub per productive node so the worker
//      starts with a valid (report ...) draft to fill in;
//   4. emit bootstrap shared-memory + session-trace entries (an
//      observation-kind shared-memory entry + a `start` and `read` trace
//      pair) so the preamble-read audit trail is created BEFORE the first
//      worker boots, not retroactively.
//
// The CLI NEVER shells out — manifest reader, manifest validator, and brief
// renderer all reach the prep script via direct named imports. The only
// side effects are filesystem writes scoped to the manifest's wave (briefs,
// preamble, report skeletons, ledger appends).
//
// CLI:
//   node scripts/prepare-task-runner-wave.mjs --manifest <manifest.lisp>
//     [--out-dir <repo>] [--dry-run] [--force] [--json] [--dry-fixture]
//
// Flags:
//   --manifest      : path to a wave28-01 task-runner-manifest v1 file.
//   --out-dir       : repo root (defaults to process.cwd()). All generated
//                     paths resolve under this directory.
//   --dry-run       : validate + plan but write NOTHING. Useful for CI-style
//                     preview; --json still prints the planned summary.
//   --force         : overwrite existing brief/preamble/report-skeleton
//                     files (default: skip when present, byte-identical to
//                     wave28-03 renderer behavior).
//   --json          : emit a structured summary on stdout instead of human
//                     prose. Schema documented in printJsonResult below.
//   --dry-fixture   : self-contained pass/fail fixtures inside a tmpdir; the
//                     real repo's .missiond/ tree is NEVER touched.
//
// Productive-only enforcement: archive / backfill / index / lisp-backfill
// pseudo-nodes (matched by :kind OR id substring) are REJECTED before any
// brief / skeleton is written. This is defence-in-depth on top of the
// wave28-01 productive_only flag and the wave28-03 renderer guard.
//
// Cross-wave invariants:
//   - Manifests are advisory orchestration metadata; the renderer / prep
//     CLI write Markdown + Lisp ledger entries only. NEVER a runtime
//     backend switch.
//   - Bootstrap session-trace events use :kind start + :kind read so the
//     wave29-shared-preamble-read expectation is auditable per the
//     wave29 navigation protocol.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  readManifestFile,
  validateManifestObject,
  FORBIDDEN_PRODUCTIVE_KINDS,
} from './check-task-runner-manifest.mjs';

import {
  renderManifest,
  FORBIDDEN_ID_SUBSTRINGS,
} from './render-wave-briefs.mjs';

import { buildLifecycleEventRecord } from './task-runner-append-event.mjs';

import {
  projectLifecycleEvents,
  renderSharedMemoryEntry,
  renderSessionTraceEvent,
} from './project-task-lifecycle-ledger.mjs';

// wave29-07 cross-layer smoke (layer C): the wave29-07 fixture parses the
// appended session-trace via the shared Lisp reader so the structured
// `:kind read` / `:files [...]` shape is asserted mechanically (not via a
// fragile substring check). These helpers are NOT used by the production
// path; they only appear inside the dry-fixture loop body.
import {
  parseLisp,
  isList,
  head,
  readKeywordProps,
  nodeText,
  nodeToStringArray,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/prepare-task-runner-wave.mjs --manifest <manifest.lisp>
    [--out-dir <repo>] [--dry-run] [--force] [--json] [--dry-fixture]

Read-only-plus-file-generation preparation CLI for task-runner waves.

Validates the manifest, renders thin briefs + shared preamble (delegating to
scripts/render-wave-briefs.mjs renderManifest in-process — never shells out),
prepares the reports/ directory + per-task report skeletons, and appends
bootstrap shared-memory + session-trace entries (observation + start + read)
so the wave29-shared-preamble-read audit expectation is recorded BEFORE the
first worker boots.

--dry-run validates + plans but writes NOTHING. --force overwrites existing
brief / preamble / skeleton files (default: skip when present). --json emits
a structured summary on stdout. --dry-fixture runs self-contained fixtures
inside a tmp dir; the real repo .missiond/ tree is NEVER touched.

Productive-only enforcement: archive/backfill/index/lisp-backfill nodes
(matched by :kind OR id substring) are REJECTED before any artifact is
written. Defence-in-depth on top of wave28-01 + wave28-03 guards.
`;

// Bootstrap entry ID convention. Centralized so fixtures and downstream
// consumers can derive the same ids without re-deriving the format.
//   shared-memory entry: <wave>-bootstrap-<NNN>
//   session-trace start: <wave>-trace-bootstrap-start-<NNN>
//   session-trace read : <wave>-trace-bootstrap-read-<NNN>
function bootstrapMemoryId(wave, ordinal) {
  return `${wave}-bootstrap-${String(ordinal).padStart(3, '0')}`;
}
function bootstrapTraceStartId(wave, ordinal) {
  return `${wave}-trace-bootstrap-start-${String(ordinal).padStart(3, '0')}`;
}
function bootstrapTraceReadId(wave, ordinal) {
  return `${wave}-trace-bootstrap-read-${String(ordinal).padStart(3, '0')}`;
}

function main() {
  const args = process.argv.slice(2);
  let manifestPath = null;
  let outDir = null;
  let dryRun = false;
  let force = false;
  let json = false;
  let dryFixture = false;

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--manifest') {
      manifestPath = args[++i];
      if (!manifestPath) fail('--manifest requires a path');
    } else if (arg === '--out-dir') {
      outDir = args[++i];
      if (!outDir) fail('--out-dir requires a path');
    } else if (arg === '--dry-run') {
      dryRun = true;
    } else if (arg === '--force') {
      force = true;
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      fail(`unknown argument: ${arg}\n\n${usage}`);
    }
  }

  if (dryFixture) {
    runFixturesPatched().then(
      () => {},
      (err) => {
        console.error(err.stack || err.message);
        process.exit(1);
      },
    );
    return;
  }

  if (!manifestPath) fail(usage);

  const cwd = outDir ? path.resolve(process.cwd(), outDir) : process.cwd();
  const resolvedManifest = path.resolve(process.cwd(), manifestPath);
  const result = prepareWave({
    manifestPath: resolvedManifest,
    cwd,
    dryRun,
    force,
    nowIso: isoNow(),
  });
  if (json) {
    printJsonResult(result, { manifestPath: resolvedManifest, dryRun });
    return;
  }
  console.log(
    `prepare-task-runner-wave OK (wave ${result.wave}, manifest ${path.relative(cwd, resolvedManifest)}): ` +
      `briefs ${result.briefsWritten} written, ${result.briefsSkipped} skipped; ` +
      `skeletons ${result.skeletonsWritten} written, ${result.skeletonsSkipped} skipped; ` +
      `bootstrap ${result.bootstrapEmitted ? 'emitted' : 'planned'}` +
      (dryRun ? ' (dry-run)' : ''),
  );
}

// Top-level orchestration: validate + render + skeletons + bootstrap.
//
// Returns a structured summary object so both the CLI and the dry-fixture
// suite can assert post-conditions. Behavior:
//   - dryRun=true performs every validation step but writes NOTHING. The
//     summary still includes the planned arrays so callers can inspect what
//     WOULD have been written.
//   - force=true overwrites existing briefs / preamble / skeletons.
//     Otherwise existing files are preserved byte-identically (skip).
//
// This function is deliberately pure on the manifest object (mutation-free)
// so the dry-fixture suite can call it repeatedly inside fresh tmp dirs.
export function prepareWave({ manifestPath, cwd, dryRun, force, nowIso }) {
  if (!fs.existsSync(manifestPath)) {
    fail(`manifest file does not exist: ${manifestPath}`);
  }

  const manifests = readManifestFile(manifestPath);
  if (manifests.length === 0) {
    fail(`no (task-runner-manifest ...) form found in ${manifestPath}`);
  }
  if (manifests.length > 1) {
    fail(
      `${manifestPath} contains ${manifests.length} (task-runner-manifest ...) forms; expected exactly 1`,
    );
  }
  const manifest = manifests[0];

  const errors = validateManifestObject(manifest);
  if (errors.length > 0) {
    const joined = errors.map((e) => `  - ${e}`).join('\n');
    fail(`manifest ${manifestPath} failed schema validation:\n${joined}`);
  }

  // Productive-only defence-in-depth. We walk every node BEFORE writing
  // anything so a single bad node aborts the whole prep run cleanly.
  for (const node of manifest.nodes) {
    if (node.kind && FORBIDDEN_PRODUCTIVE_KINDS.has(node.kind)) {
      fail(
        `node "${node.task_id}" has :kind "${node.kind}" — archive/backfill/index/lisp-backfill nodes MUST NOT receive worker briefs or report skeletons`,
      );
    }
    for (const sub of FORBIDDEN_ID_SUBSTRINGS) {
      if (node.task_id && node.task_id.includes(sub)) {
        fail(
          `node "${node.task_id}" id contains "${sub}" — archive/backfill/index/lisp-backfill nodes MUST NOT receive worker briefs or report skeletons`,
        );
      }
    }
  }

  const wave = manifest.wave;

  // Plan deterministic output paths up front. Skeletons land at
  // .missiond/tasks/<wave>/reports/<task-id>.report.lisp; briefs land where
  // render-wave-briefs decides (delegates to render-claudecode-task).
  const reportsDir = path.resolve(cwd, '.missiond', 'tasks', wave, 'reports');
  const skeletonPlan = manifest.nodes
    .map((node) => ({
      task_id: node.task_id,
      path: path.join(reportsDir, `${node.task_id}.report.lisp`),
    }))
    .sort((a, b) => a.task_id.localeCompare(b.task_id));

  const sharedMemoryPath = path.resolve(
    cwd,
    '.missiond',
    'tasks',
    wave,
    'shared-memory.lisp',
  );
  const sessionTracePath = path.resolve(
    cwd,
    '.missiond',
    'tasks',
    wave,
    'session-trace.lisp',
  );
  const sharedPreambleRel = manifest.shared_preamble_path;

  // Plan-only path: validate + plan; no fs side effects.
  if (dryRun) {
    return {
      wave,
      manifestPath,
      dryRun: true,
      briefs: { written: 0, skipped: 0, overwritten: 0, plan: skeletonPlan.map((s) => s.task_id) },
      briefsWritten: 0,
      briefsSkipped: 0,
      skeletonsWritten: 0,
      skeletonsSkipped: 0,
      skeletonsPlan: skeletonPlan.map((s) => path.relative(cwd, s.path)),
      sharedMemoryPath: path.relative(cwd, sharedMemoryPath),
      sessionTracePath: path.relative(cwd, sessionTracePath),
      sharedPreamblePath: sharedPreambleRel,
      bootstrapEmitted: false,
      bootstrapEntryIds: [],
    };
  }

  // Step 1: render preamble + thin briefs via the wave28-03 renderer.
  // The renderer is pure on the manifest path (no global state) so we just
  // forward through. Skip-when-present + --force semantics are inherited.
  const renderResult = renderManifest({
    manifestPath,
    cwd,
    force,
  });
  const briefsWritten = renderResult.briefs.filter((b) => b.action === 'written').length;
  const briefsOverwritten = renderResult.briefs.filter((b) => b.action === 'overwritten').length;
  const briefsSkipped = renderResult.briefs.filter((b) => b.action === 'skipped').length;

  // Step 2: ensure reports/ directory exists, then materialize a draft
  // report skeleton per node. The skeleton text is deterministic byte-for-byte
  // so two prep runs against the same manifest produce identical files.
  fs.mkdirSync(reportsDir, { recursive: true });
  let skeletonsWritten = 0;
  let skeletonsOverwritten = 0;
  let skeletonsSkipped = 0;
  for (const entry of skeletonPlan) {
    const action = writeSkeletonIfAllowed(entry.task_id, entry.path, force);
    if (action === 'written') skeletonsWritten += 1;
    else if (action === 'overwritten') skeletonsOverwritten += 1;
    else if (action === 'skipped') skeletonsSkipped += 1;
  }

  // Step 3: bootstrap shared-memory + session-trace entries. Both files MUST
  // exist already (the wave's ledgers are seeded before dispatch); we
  // append-only to preserve the canonical append-only invariant.
  const ordinal = nextOrdinalFromTracePath(sessionTracePath);
  const bootstrapMemId = bootstrapMemoryId(wave, ordinal);
  const bootstrapStartId = bootstrapTraceStartId(wave, ordinal);
  const bootstrapReadId = bootstrapTraceReadId(wave, ordinal);
  const memSeq = nextMemorySeq(sharedMemoryPath);
  const traceSeq = nextTraceSeq(sessionTracePath);

  const bootstrapProjection = projectBootstrapLifecycleEvents({
    wave,
    ordinal,
    bootstrapMemId,
    bootstrapStartId,
    bootstrapReadId,
    nowIso,
    sharedPreambleRel,
  });
  appendSharedMemoryBootstrap({
    sharedMemoryPath,
    entry: bootstrapProjection.sharedMemoryEntries[0],
    seq: memSeq,
    wave,
  });
  appendSessionTraceBootstrap({
    sessionTracePath,
    entries: bootstrapProjection.sessionTraceEvents,
    startSeq: traceSeq,
    wave,
  });

  return {
    wave,
    manifestPath,
    dryRun: false,
    briefs: {
      written: briefsWritten,
      skipped: briefsSkipped,
      overwritten: briefsOverwritten,
      plan: renderResult.briefs.map((b) => b.task_id).sort(),
    },
    briefsWritten,
    briefsSkipped,
    briefsOverwritten,
    skeletonsWritten,
    skeletonsSkipped,
    skeletonsOverwritten,
    skeletonsPlan: skeletonPlan.map((s) => path.relative(cwd, s.path)),
    sharedMemoryPath: path.relative(cwd, sharedMemoryPath),
    sessionTracePath: path.relative(cwd, sessionTracePath),
    sharedPreamblePath: sharedPreambleRel,
    bootstrapEmitted: true,
    bootstrapEntryIds: [bootstrapMemId, bootstrapStartId, bootstrapReadId],
  };
}

// Render a single deterministic report skeleton stub. Shape MUST match
// missiond.report-contract.v1: required keys :schema :task_id :status
// :commit_hash :files_changed :acceptance_results — :status=draft so
// :commit_hash empty string and :files_changed [] / :acceptance_results []
// are all permitted by check-task-report.mjs.
//
// Two skeletons for the same task id are byte-identical; this is the
// determinism guarantee asserted by the dry-fixture suite.
export function renderReportSkeleton(taskId) {
  return [
    `;; Draft report skeleton scaffolded by scripts/prepare-task-runner-wave.mjs.`,
    `;; Schema: missiond.report-contract.v1`,
    `;; Replace :status with done (and fill :commit_hash + :files_changed`,
    `;; + :acceptance_results) once the task completes.`,
    ``,
    `(report ${taskId}`,
    `  :schema "missiond.report-contract.v1"`,
    `  :task_id "${taskId}"`,
    `  :status draft`,
    `  :commit_hash ""`,
    `  :files_changed []`,
    `  :acceptance_results [])`,
    ``,
  ].join('\n');
}

function writeSkeletonIfAllowed(taskId, outputAbs, force) {
  fs.mkdirSync(path.dirname(outputAbs), { recursive: true });
  const exists = fs.existsSync(outputAbs);
  if (exists && !force) return 'skipped';
  fs.writeFileSync(outputAbs, renderReportSkeleton(taskId));
  return exists ? 'overwritten' : 'written';
}

// Build lifecycle facts for the bootstrap operation and project them back to
// the legacy ledgers. The lifecycle event objects are in-memory here so the
// CLI preserves its historical side effects: it still writes only briefs,
// report skeletons, shared-memory, and session-trace unless a future caller
// explicitly invokes task-runner-append-event.mjs for a lifecycle ledger.
function projectBootstrapLifecycleEvents({
  wave,
  ordinal,
  bootstrapMemId,
  bootstrapStartId,
  bootstrapReadId,
  nowIso,
  sharedPreambleRel,
}) {
  const startEvent = buildLifecycleEventRecord({
    id: `${wave}-lifecycle-bootstrap-start-${String(ordinal).padStart(3, '0')}`,
    task: `${wave}-bootstrap`,
    actorRole: 'prepare-task-runner-wave',
    eventKind: 'trace_start',
    commitRole: 'none',
    seq: 1,
    at: nowIso,
    touched: [sharedPreambleRel],
    summary: 'Bootstrap lifecycle start.',
    legacyMemoryId: bootstrapMemId,
    legacyTraceId: bootstrapStartId,
    legacyTraceFiles: [],
    legacyMemorySummary:
      'wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.',
    legacyTraceSummary:
      'Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.',
  });
  const readEvent = buildLifecycleEventRecord({
    id: `${wave}-lifecycle-bootstrap-read-${String(ordinal).padStart(3, '0')}`,
    task: `${wave}-bootstrap`,
    actorRole: 'prepare-task-runner-wave',
    eventKind: 'read',
    commitRole: 'none',
    seq: 2,
    at: nowIso,
    touched: [sharedPreambleRel],
    summary:
      'Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.',
    legacyTraceId: bootstrapReadId,
  });
  return projectLifecycleEvents([startEvent, readEvent], { wave });
}

// Append the projected bootstrap observation entry to the wave's shared-memory
// ledger. Insertion is done by replacing the trailing close-paren of the
// outer (shared-memory ...) form so the file remains a single well-formed
// S-expression.
function appendSharedMemoryBootstrap({ sharedMemoryPath, entry, seq, wave }) {
  if (!fs.existsSync(sharedMemoryPath)) {
    fail(`shared-memory ledger missing for wave ${wave}: ${sharedMemoryPath}`);
  }
  const body = fs.readFileSync(sharedMemoryPath, 'utf8');
  const block = `\n\n${renderSharedMemoryEntry(entry, seq)}`;
  fs.writeFileSync(sharedMemoryPath, spliceBeforeFinalParen(body, block));
}

// Append the projected bootstrap session-trace pair (start + read) to the
// wave's session-trace ledger.
function appendSessionTraceBootstrap({ sessionTracePath, entries, startSeq, wave }) {
  if (!fs.existsSync(sessionTracePath)) {
    fail(`session-trace ledger missing for wave ${wave}: ${sessionTracePath}`);
  }
  const body = fs.readFileSync(sessionTracePath, 'utf8');
  const block = `\n\n${entries
    .map((entry, index) => renderSessionTraceEvent(entry, startSeq + index))
    .join('\n\n')}`;
  fs.writeFileSync(sessionTracePath, spliceBeforeFinalParen(body, block));
}

// Replace the trailing `)` of the outer ledger form with `<block>)` plus a
// newline so the file remains a single well-formed (shared-memory ...) /
// (session-trace ...) S-expression. The implementation walks the file body
// from the end, skips trailing whitespace, asserts the very last
// non-whitespace character is `)`, and splices the block between the
// existing payload and that closing paren.
function spliceBeforeFinalParen(body, block) {
  let i = body.length - 1;
  while (i >= 0 && /\s/.test(body[i])) i--;
  if (i < 0 || body[i] !== ')') {
    fail(
      `ledger file is not terminated by a close-paren — refusing to splice bootstrap block ` +
        `(last non-whitespace char at offset ${i}: ${JSON.stringify(body[i] ?? '<eof>')})`,
    );
  }
  const before = body.slice(0, i);
  const after = body.slice(i); // starts with `)`
  return `${before}${block}${after}\n`.replace(/\n+$/, '\n');
}

// Compute the next bootstrap ordinal by counting existing bootstrap-* trace
// ids. Idempotent re-runs would otherwise collide on duplicate :id; the
// wave checker rejects duplicates so we increment past the highest ordinal
// already on disk. Returns 1 when no prior bootstrap is present.
function nextOrdinalFromTracePath(sessionTracePath) {
  if (!fs.existsSync(sessionTracePath)) return 1;
  const body = fs.readFileSync(sessionTracePath, 'utf8');
  const re = /-bootstrap(?:-(?:start|read))?-(\d+)\b/g;
  let max = 0;
  let m;
  while ((m = re.exec(body))) {
    const n = Number.parseInt(m[1], 10);
    if (Number.isFinite(n) && n > max) max = n;
  }
  return max + 1;
}

// Compute the next :seq for a shared-memory append. The checker enforces
// strictly-increasing seq across entries; we scan all `:seq <int>` values
// and return max+1. Returns 1 for an empty ledger.
function nextMemorySeq(sharedMemoryPath) {
  if (!fs.existsSync(sharedMemoryPath)) return 1;
  return scanMaxSeq(fs.readFileSync(sharedMemoryPath, 'utf8'));
}
function nextTraceSeq(sessionTracePath) {
  if (!fs.existsSync(sessionTracePath)) return 1;
  return scanMaxSeq(fs.readFileSync(sessionTracePath, 'utf8'));
}
function scanMaxSeq(body) {
  const re = /:seq\s+(-?\d+)\b/g;
  let max = 0;
  let m;
  while ((m = re.exec(body))) {
    const n = Number.parseInt(m[1], 10);
    if (Number.isFinite(n) && n > max) max = n;
  }
  return max + 1;
}

function isoNow() {
  // Stable wall-clock timestamp; fixtures override via nowIso so the
  // determinism assertion can compare byte-for-byte.
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function printJsonResult(result, { manifestPath, dryRun }) {
  const payload = {
    ok: true,
    manifest_path: manifestPath,
    wave: result.wave,
    briefs_written: result.briefsWritten,
    skeletons_written: result.skeletonsWritten,
    bootstrap_emitted: result.bootstrapEmitted,
    dry_run: dryRun,
  };
  console.log(JSON.stringify(payload));
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Dry fixtures.
//
// All fixtures run inside a fresh tmpdir so the real .missiond/ tree is
// NEVER touched. Each fixture seeds a minimal wave skeleton (manifest +
// task contracts + ledgers) and exercises one prepareWave behavior.
//
// Coverage (target 8-12):
//   1.  pass: dry-run no-write (no files materialize in tmp)
//   2.  pass: full run writes briefs + skeletons + bootstrap entries
//   3.  pass: skeleton contains :status draft + :task_id + empty commit
//   4.  pass: preamble-read trace event emitted in session-trace
//   5.  pass: --force overwrites existing skeleton (sentinel preserved
//             without --force; replaced under --force)
//   6.  pass: deterministic skeleton bytes (re-run --force produces the
//             same skeleton bytes)
//   7.  pass: idempotent re-run is a no-op for skeleton + brief files
//             (skip-when-present)
//   8.  fail: archive id rejected
//   9.  fail: backfill kind rejected
//   10. pass: bootstrap entries are append-only (existing entries preserved
//             byte-identically)
// ---------------------------------------------------------------------------

async function runFixtures() {
  const fixtures = [];

  fixtures.push({
    name: 'dry-run-writes-nothing',
    category: 'pass-dry-run-no-write',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        const before = listAllFiles(env.cwd);
        const result = prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: true,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        const after = listAllFiles(env.cwd);
        if (JSON.stringify(before) !== JSON.stringify(after)) {
          throw new Error(
            'dry-run modified the tmp tree:\n  before=' +
              JSON.stringify(before) +
              '\n  after=' +
              JSON.stringify(after),
          );
        }
        if (result.dryRun !== true) throw new Error('dryRun flag should round-trip on result');
        if (result.briefsWritten !== 0) throw new Error('dry-run briefsWritten should be 0');
        if (result.skeletonsWritten !== 0) throw new Error('dry-run skeletonsWritten should be 0');
        if (result.bootstrapEmitted !== false) throw new Error('dry-run bootstrap should not emit');
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'full-run-writes-briefs-skeletons-and-bootstrap-entries',
    category: 'pass-full-run',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        const result = prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        if (result.briefsWritten !== 2) {
          throw new Error(`expected 2 briefs written, got ${result.briefsWritten}`);
        }
        if (result.skeletonsWritten !== 2) {
          throw new Error(`expected 2 skeletons written, got ${result.skeletonsWritten}`);
        }
        if (!result.bootstrapEmitted) throw new Error('bootstrap should emit on full run');
        if (result.bootstrapEntryIds.length !== 3) {
          throw new Error(
            `expected 3 bootstrap entry ids (memory + start + read), got ${result.bootstrapEntryIds.length}`,
          );
        }
        // The shared-preamble file MUST exist on disk after the run.
        const preambleAbs = path.resolve(
          env.cwd,
          '.missiond/claudecode/wave99-shared-preamble.md',
        );
        if (!fs.existsSync(preambleAbs)) throw new Error('preamble missing post-run');
        // Both skeletons MUST exist on disk.
        for (const id of ['wave99-01-foo', 'wave99-02-bar']) {
          const skel = path.resolve(env.cwd, `.missiond/tasks/wave99/reports/${id}.report.lisp`);
          if (!fs.existsSync(skel)) throw new Error(`skeleton missing for ${id}`);
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'skeleton-shape-status-draft-empty-commit',
    category: 'pass-skeleton-shape',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        const skelPath = path.resolve(
          env.cwd,
          '.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp',
        );
        const body = fs.readFileSync(skelPath, 'utf8');
        // Critical fields the schema requires for a draft:
        const required = [
          ':schema "missiond.report-contract.v1"',
          ':task_id "wave99-01-foo"',
          ':status draft',
          ':commit_hash ""',
          ':files_changed []',
          ':acceptance_results []',
        ];
        for (const literal of required) {
          if (!body.includes(literal)) {
            throw new Error(`skeleton missing required literal: ${literal}`);
          }
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'preamble-read-trace-event-emitted',
    category: 'pass-preamble-read-trace',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        const result = prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        const tracePath = path.resolve(env.cwd, '.missiond/tasks/wave99/session-trace.lisp');
        const body = fs.readFileSync(tracePath, 'utf8');
        // The wave29 audit expectation: a `kind read` trace event whose
        // :files vector references the manifest's shared_preamble_path.
        if (!body.includes(':kind read')) {
          throw new Error('session-trace should contain a :kind read bootstrap event');
        }
        if (!body.includes('.missiond/claudecode/wave99-shared-preamble.md')) {
          throw new Error('preamble-read trace event must reference the shared preamble path');
        }
        // The bootstrap entry ids MUST appear in the trace file body.
        const readId = result.bootstrapEntryIds.find((id) => id.includes('-bootstrap-read-'));
        if (!readId || !body.includes(readId)) {
          throw new Error(`bootstrap read trace id missing from session-trace body`);
        }
        const startId = result.bootstrapEntryIds.find((id) => id.includes('-bootstrap-start-'));
        if (!startId || !body.includes(startId)) {
          throw new Error(`bootstrap start trace id missing from session-trace body`);
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'force-overwrites-existing-skeleton',
    category: 'pass-force-overwrites',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        const skelPath = path.resolve(
          env.cwd,
          '.missiond/tasks/wave99/reports/wave99-01-foo.report.lisp',
        );
        fs.mkdirSync(path.dirname(skelPath), { recursive: true });
        const sentinel = ';; sentinel — preserved without --force\n';
        fs.writeFileSync(skelPath, sentinel);
        // First pass without --force: sentinel preserved.
        prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        if (fs.readFileSync(skelPath, 'utf8') !== sentinel) {
          throw new Error('skip-when-present should preserve existing skeleton bytes');
        }
        // Second pass with --force: sentinel replaced.
        prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: true,
          nowIso: '2026-04-28T14:31:00Z',
        });
        const after = fs.readFileSync(skelPath, 'utf8');
        if (after === sentinel) throw new Error('--force should overwrite the sentinel');
        if (!after.includes(':status draft')) {
          throw new Error('--force overwrite should write the canonical skeleton body');
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'deterministic-skeleton-bytes-across-reruns',
    category: 'pass-deterministic-output',
    run: () => {
      const envA = setupTmpRepo();
      const envB = setupTmpRepo();
      try {
        seedTwoNodeWave(envA);
        seedTwoNodeWave(envB);
        prepareWave({
          manifestPath: envA.manifestPath,
          cwd: envA.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        prepareWave({
          manifestPath: envB.manifestPath,
          cwd: envB.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        for (const id of ['wave99-01-foo', 'wave99-02-bar']) {
          const a = fs.readFileSync(
            path.resolve(envA.cwd, `.missiond/tasks/wave99/reports/${id}.report.lisp`),
            'utf8',
          );
          const b = fs.readFileSync(
            path.resolve(envB.cwd, `.missiond/tasks/wave99/reports/${id}.report.lisp`),
            'utf8',
          );
          if (a !== b) throw new Error(`skeleton bytes drift across tmp dirs for ${id}`);
        }
      } finally {
        cleanupTmpRepo(envA);
        cleanupTmpRepo(envB);
      }
    },
  });

  fixtures.push({
    name: 'idempotent-rerun-no-op-when-files-present',
    category: 'pass-idempotent-rerun',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        const r1 = prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        if (r1.briefsWritten !== 2) throw new Error('first run should write 2 briefs');
        if (r1.skeletonsWritten !== 2) throw new Error('first run should write 2 skeletons');
        const r2 = prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:31:00Z',
        });
        if (r2.briefsWritten !== 0) throw new Error('rerun should NOT rewrite briefs');
        if (r2.skeletonsWritten !== 0) throw new Error('rerun should NOT rewrite skeletons');
        if (r2.briefsSkipped !== 2) throw new Error('rerun should skip 2 briefs');
        if (r2.skeletonsSkipped !== 2) throw new Error('rerun should skip 2 skeletons');
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'archive-id-rejected-defence-in-depth',
    category: 'fail-archive-id',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedCustomNodeWave(env, [taskContract('wave99-00-archive-foo')]);
        let threw = false;
        try {
          prepareWave({
            manifestPath: env.manifestPath,
            cwd: env.cwd,
            dryRun: false,
            force: false,
            nowIso: '2026-04-28T14:30:00Z',
          });
        } catch (err) {
          threw = true;
          if (!String(err.message).includes('-archive-')) {
            throw new Error(`expected error to mention -archive-, got: ${err.message}`);
          }
        }
        if (!threw) throw new Error('archive-id node should have been rejected');
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'backfill-kind-rejected-defence-in-depth',
    category: 'fail-backfill-kind',
    run: () => {
      const env = setupTmpRepo();
      try {
        // Use a productive id and stamp :kind backfill so we exercise the
        // kind-based rejection path independent of the substring path.
        seedCustomNodeWave(env, [taskContract('wave99-10-parallel-dispatcher')], {
          kind: 'backfill',
        });
        let threw = false;
        try {
          prepareWave({
            manifestPath: env.manifestPath,
            cwd: env.cwd,
            dryRun: false,
            force: false,
            nowIso: '2026-04-28T14:30:00Z',
          });
        } catch (err) {
          threw = true;
          if (!String(err.message).match(/backfill/)) {
            throw new Error(`expected error to mention backfill, got: ${err.message}`);
          }
        }
        if (!threw) throw new Error('backfill-kind node should have been rejected');
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  // wave29-07 cross-layer smoke (layer C): prove the trace-event audit
  // expectation is mechanically pinned. The prep CLI MUST emit a
  // `:kind read` trace-event whose `:files` vector references the
  // manifest's shared_preamble_path; this is the auditability guarantee
  // shared with wave29-shared-preamble-read invariants. The existing
  // `preamble-read-trace-event-emitted` fixture asserts the substring;
  // this wave29-07 fixture parses the appended trace via the shared Lisp
  // reader and asserts the structured event shape so a regression that
  // breaks the keyword form (but happens to leave the path string intact)
  // surfaces here, near the prep-CLI layer.
  fixtures.push({
    name: 'wave29-07-loop-smoke-preamble-read-trace-emitted',
    category: 'wave29-07-loop-smoke',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        const result = prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T15:45:00Z',
        });
        const tracePath = path.resolve(env.cwd, '.missiond/tasks/wave99/session-trace.lisp');
        const body = fs.readFileSync(tracePath, 'utf8');
        const forms = parseLisp(body, tracePath);
        // Walk the parsed forms and find a (trace-event ...) child whose
        // :kind is `read` AND whose :files vector contains the manifest's
        // shared_preamble_path. Mechanical assertion (no substring shortcut).
        const expectedPreamble = '.missiond/claudecode/wave99-shared-preamble.md';
        let foundReadEvent = false;
        for (const form of forms) {
          if (!isList(form)) continue;
          if (head(form) !== 'session-trace') continue;
          for (const child of form.children.slice(2)) {
            if (!isList(child) || head(child) !== 'trace-event') continue;
            const props = readKeywordProps(child, { start: 1 });
            const kind = nodeText(props[':kind']?.value);
            if (kind !== 'read') continue;
            const filesNode = props[':files']?.value;
            const files = nodeToStringArray(filesNode) ?? [];
            if (files.includes(expectedPreamble)) {
              foundReadEvent = true;
              break;
            }
          }
        }
        if (!foundReadEvent) {
          throw new Error(
            'wave29-07 layer C: prep CLI MUST emit a (trace-event :kind read :files [...]) referencing the shared_preamble_path',
          );
        }
        // Cross-check: the bootstrap-emitted `bootstrapEntryIds` must
        // include exactly one read-style id so the structured-vs-substring
        // count agrees with the API surface.
        const readIds = result.bootstrapEntryIds.filter((id) => id.includes('-bootstrap-read-'));
        if (readIds.length !== 1) {
          throw new Error(
            `wave29-07 layer C: expected 1 bootstrap-read id, got ${readIds.length}`,
          );
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'bootstrap-append-preserves-existing-entries',
    category: 'pass-bootstrap-append-only',
    run: () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeWave(env);
        const sharedPath = path.resolve(env.cwd, '.missiond/tasks/wave99/shared-memory.lisp');
        const tracePath = path.resolve(env.cwd, '.missiond/tasks/wave99/session-trace.lisp');
        const beforeShared = fs.readFileSync(sharedPath, 'utf8');
        const beforeTrace = fs.readFileSync(tracePath, 'utf8');
        prepareWave({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          dryRun: false,
          force: false,
          nowIso: '2026-04-28T14:30:00Z',
        });
        const afterShared = fs.readFileSync(sharedPath, 'utf8');
        const afterTrace = fs.readFileSync(tracePath, 'utf8');
        // The pre-existing seed entries MUST be preserved verbatim. We
        // assert the seed-summary literal appears unchanged in the appended
        // file.
        if (!afterShared.includes('seed-shared-memory-entry')) {
          throw new Error('append-only: seed shared-memory entry must remain in body');
        }
        if (!afterTrace.includes('seed-trace-event')) {
          throw new Error('append-only: seed trace event must remain in body');
        }
        // And the pre-existing prefix MUST remain unchanged; we only
        // appended after the original closing paren.
        const prefixSharedLen = beforeShared.lastIndexOf(')');
        if (afterShared.slice(0, prefixSharedLen) !== beforeShared.slice(0, prefixSharedLen)) {
          throw new Error('append-only: shared-memory prefix mutated');
        }
        const prefixTraceLen = beforeTrace.lastIndexOf(')');
        if (afterTrace.slice(0, prefixTraceLen) !== beforeTrace.slice(0, prefixTraceLen)) {
          throw new Error('append-only: session-trace prefix mutated');
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

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
  if (failed > 0) {
    console.error(`prepare-task-runner-wave fixtures FAILED — ${failed} of ${fixtures.length}`);
    process.exit(1);
  }
  console.log(
    `prepare-task-runner-wave fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
  );
}

// Build a fresh tmp repo skeleton with the directory layout the prep CLI
// expects: .missiond/tasks/wave99/{,reports/} + .missiond/claudecode/.
// Returns the cwd + key paths the fixture body uses; cleanup is the
// caller's job (cleanupTmpRepo) so a panic mid-fixture does not leak.
function setupTmpRepo() {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'wave29-03-prep-'));
  const tasksDir = path.join(cwd, '.missiond', 'tasks', 'wave99');
  const briefDir = path.join(cwd, '.missiond', 'claudecode');
  fs.mkdirSync(tasksDir, { recursive: true });
  fs.mkdirSync(briefDir, { recursive: true });
  const manifestPath = path.join(tasksDir, 'manifest.lisp');
  return { cwd, tasksDir, briefDir, manifestPath };
}
function cleanupTmpRepo(env) {
  fs.rmSync(env.cwd, { recursive: true, force: true });
}

// Minimal task contract body used by every fixture seed. Mirrors the one
// in render-wave-briefs.mjs so the renderer accepts the synthetic file.
function taskContract(taskId, opts = {}) {
  const kindLine = opts.taskKind ? `  :kind ${opts.taskKind}` : '  :kind code-alignment';
  return `(task ${taskId}\n  :schema "missiond.task-contract.v1"\n  :title "Synthetic ${taskId}"\n${kindLine}\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "fresh-code-alignment"\n  :verification-tier local\n  :dispatch-group "A"\n  :estimated-minutes 25\n  :heartbeat-minutes 10\n  :goal "Synthetic ${taskId} goal."\n  :write-scope ["scripts/${taskId}.mjs"]\n  :must-not-touch []\n  :requirements ["Run prep CLI."]\n  :acceptance ["true"]\n  :commit (:required true :message "test: ${taskId}" :scope-check write-scope-only)\n  :report ["Commit hash."])\n`;
}

// Seed a 2-node productive manifest plus pre-populated shared-memory and
// session-trace ledgers. The seed entries carry recognizable :id values so
// the append-only fixture can assert they survive verbatim.
function seedTwoNodeWave(env) {
  const fooContract = taskContract('wave99-01-foo');
  const barContract = taskContract('wave99-02-bar');
  fs.writeFileSync(path.join(env.tasksDir, 'wave99-01-foo.lisp'), fooContract);
  fs.writeFileSync(path.join(env.tasksDir, 'wave99-02-bar.lisp'), barContract);
  const manifest = `(task-runner-manifest m-wave99\n  :schema "missiond.task-runner-manifest.v1"\n  :wave wave99\n  :brief_mode thin\n  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"\n  :productive_only true\n  (node :task_id wave99-01-foo\n        :depends_on []\n        :verification_tier local\n        :dispatch_group A\n        :estimated_minutes 25\n        :heartbeat_minutes 10\n        :write_scope ["scripts/wave99-01-foo.mjs"])\n  (node :task_id wave99-02-bar\n        :depends_on [wave99-01-foo]\n        :verification_tier local\n        :dispatch_group B\n        :estimated_minutes 25\n        :heartbeat_minutes 10\n        :write_scope ["scripts/wave99-02-bar.mjs"]))\n`;
  fs.writeFileSync(env.manifestPath, manifest);
  // Seed shared-memory ledger with one observation so the bootstrap append
  // exercises a non-empty file path.
  const sharedMem = `(shared-memory wave99\n  :schema "missiond.shared-memory.v1"\n  :wave wave99\n  :created-at "2026-04-28T00:00:00+08:00"\n  :sequence 1\n\n  (observation\n    :id seed-shared-memory-entry\n    :task wave99-bootstrap\n    :agent fixture\n    :seq 1\n    :at "2026-04-28T00:00:00+08:00"\n    :touched [".missiond/tasks/wave99/manifest.lisp"]\n    :summary "fixture seed"))\n`;
  fs.writeFileSync(path.join(env.tasksDir, 'shared-memory.lisp'), sharedMem);
  const trace = `(session-trace wave99\n  :schema "missiond.session-trace.v1"\n  :wave wave99\n  :created-at "2026-04-28T00:00:00+08:00"\n  :sequence 1\n\n  (trace-event\n    :id seed-trace-event\n    :seq 1\n    :at "2026-04-28T00:00:00+08:00"\n    :task wave99-bootstrap\n    :backend fixture\n    :kind dispatch\n    :summary "fixture seed"))\n`;
  fs.writeFileSync(path.join(env.tasksDir, 'session-trace.lisp'), trace);
}

// Seed a manifest that smuggles an archive/backfill/index node past the
// wave28-01 checker (productive_only=false), so we can prove the prep CLI's
// own defence-in-depth still rejects it.
function seedCustomNodeWave(env, contracts, opts = {}) {
  const ids = [];
  for (const contract of contracts) {
    const idMatch = contract.match(/^\(task\s+([a-z0-9][a-z0-9._-]*)/);
    if (!idMatch) throw new Error('synthetic contract missing task id');
    const id = idMatch[1];
    ids.push(id);
    fs.writeFileSync(path.join(env.tasksDir, `${id}.lisp`), contract);
  }
  const kindAttr = opts.kind ? `\n        :kind ${opts.kind}` : '';
  const nodes = ids
    .map(
      (id) =>
        `  (node :task_id ${id}\n        :depends_on []\n        :verification_tier local\n        :dispatch_group A\n        :estimated_minutes 25\n        :heartbeat_minutes 10\n        :write_scope ["scripts/${id}.mjs"]${kindAttr})`,
    )
    .join('\n');
  const manifest = `(task-runner-manifest m-wave99-custom\n  :schema "missiond.task-runner-manifest.v1"\n  :wave wave99\n  :brief_mode thin\n  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"\n  :productive_only false\n${nodes})\n`;
  fs.writeFileSync(env.manifestPath, manifest);
  const sharedMem = `(shared-memory wave99\n  :schema "missiond.shared-memory.v1"\n  :wave wave99\n  :created-at "2026-04-28T00:00:00+08:00"\n  :sequence 1\n\n  (observation\n    :id seed-shared-memory-entry\n    :task wave99-bootstrap\n    :agent fixture\n    :seq 1\n    :at "2026-04-28T00:00:00+08:00"\n    :touched [".missiond/tasks/wave99/manifest.lisp"]\n    :summary "fixture seed"))\n`;
  fs.writeFileSync(path.join(env.tasksDir, 'shared-memory.lisp'), sharedMem);
  const trace = `(session-trace wave99\n  :schema "missiond.session-trace.v1"\n  :wave wave99\n  :created-at "2026-04-28T00:00:00+08:00"\n  :sequence 1\n\n  (trace-event\n    :id seed-trace-event\n    :seq 1\n    :at "2026-04-28T00:00:00+08:00"\n    :task wave99-bootstrap\n    :backend fixture\n    :kind dispatch\n    :summary "fixture seed"))\n`;
  fs.writeFileSync(path.join(env.tasksDir, 'session-trace.lisp'), trace);
}

// Recursively list every file under `dir` so dry-run fixtures can prove no
// new file was created. Output is a sorted relative-path array.
function listAllFiles(dir) {
  const out = [];
  function walk(rel) {
    const abs = path.join(dir, rel);
    const stat = fs.statSync(abs);
    if (stat.isDirectory()) {
      for (const entry of fs.readdirSync(abs).sort()) {
        walk(path.join(rel, entry));
      }
    } else {
      out.push(rel);
    }
  }
  for (const entry of fs.readdirSync(dir).sort()) walk(entry);
  return out.sort();
}

// fail() in the production path calls process.exit(2). Fixtures need that
// to throw so per-fixture try/catch can record the failure without bringing
// down the whole suite. Mirrors the patch used in render-wave-briefs.mjs.
const originalExit = process.exit.bind(process);
const originalConsoleError = console.error.bind(console);
let _lastFailMessage = null;
function patchProcessExitForFixtures() {
  process.exit = function patchedExit(code) {
    if (code === 2) {
      const err = new Error(_lastFailMessage || 'process.exit(2)');
      throw err;
    }
    return originalExit(code);
  };
  console.error = function patchedErr(msg) {
    _lastFailMessage = typeof msg === 'string' ? msg : String(msg);
    return originalConsoleError(msg);
  };
}
function unpatchProcessExitForFixtures() {
  process.exit = originalExit;
  console.error = originalConsoleError;
}

async function runFixturesPatched() {
  patchProcessExitForFixtures();
  try {
    await runFixtures();
  } finally {
    unpatchProcessExitForFixtures();
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main();
  } catch (err) {
    console.error(err.stack || err.message);
    process.exit(1);
  }
}
