#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-unified-entry-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 unified-entry-runtime Lisp/code isomorphism contract.
This surface is intentionally split from mission_request: mission_request is
the user-facing review-packet/respond adapter, and unified-entry-runtime is
the in-daemon pipeline substrate the adapter (and mission_directive /
mission_plan / mission_workflow bridges) compose to drive
F-intent-alignment-plan-execution-loop end to end.

  - V3 blueprint declares (surface unified-entry-runtime ...) with
    :status "code-aligned" and :code naming unified_entry.rs.
  - The surface body pins the substrate contract names that callers and
    other surfaces rely on by string match:
      run_pipeline / run_directive_compile_stage / run_plan_compile_stage /
      run_plan_execute_stage stage runners.
      stages::{MESSAGE_INTAKE, DIRECTIVE_REVIEW_GATE, PLAN_AUTHORING,
               PLAN_REVIEW_GATE, EXECUTION_RUNNER} — the s1/s3/s4/s5/s6
      stage label vocabulary actually emitted on the wire.
      ArtifactScope::{Directive, Plan, Execution} — projection scope
      surfaced into artifact_refs.
      Response shape fields: pipeline_stage, flow_ref, artifact_refs,
      next_step (and next_call when meaningful).
      FLOW_REF anchor F-intent-alignment-plan-execution-loop.
      Bridge surfaces: mission_request / mission_directive / mission_plan /
      mission_workflow (the four runtime bridges that compose this
      substrate or are composed by it).
  - compression-contract :checks pins this checker.
  - unified_entry.rs exposes the stable substrate API:
      pub(crate) async fn run_pipeline
      async fn run_directive_compile_stage
      async fn run_plan_compile_stage
      async fn run_plan_execute_stage
      pub(crate) mod stages with the s1/s3/s4/s5/s6 label constants
      pub(crate) enum ArtifactScope { Directive, Plan, Execution }
      fn build_artifact_refs / fn decorate
      const FLOW_REF: &str = "F-intent-alignment-plan-execution-loop"
    plus the wire field name string literals "pipeline_stage", "flow_ref",
    "artifact_refs", "next_step" that the decorator stamps into every
    response.
  - request.rs bridges via super::unified_entry::run_pipeline so the
    user-facing mission_request adapter never reimplements stage routing.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  unifiedEntry: 'crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs',
  requestHandler: 'crates/missiond-daemon/src/handlers/knowledge/request.rs',
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
    console.log('v3 unified-entry-runtime Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 unified-entry-runtime Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

// Whole-file blueprint anchors: the surface must exist, be code-aligned,
// name unified_entry.rs as its code home, and the compression-contract
// :checks block must pin this checker.
const BLUEPRINT_NEEDLES = [
  '(surface unified-entry-runtime',
  ':status "code-aligned"',
  'crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs',
  'node scripts/check-v3-unified-entry-isomorphism.mjs',
];

// Anchors that MUST live inside the (surface unified-entry-runtime ...) form
// itself, not just somewhere in the file. The substrate's stability
// invariants belong inside the surface declaration so reviewers see them
// next to the code paths.
const BLUEPRINT_SURFACE_BODY_ANCHORS = [
  // staged request/directive/plan/workflow runtime bridging (s1-s6)
  's1_message_intake',
  's3_alignment_review_gate',
  's4_plan_authoring',
  's5_plan_review_gate',
  's6_execution_runner',
  // async stage runners actually present in unified_entry.rs
  'run_pipeline',
  'run_directive_compile_stage',
  'run_plan_compile_stage',
  'run_plan_execute_stage',
  // ArtifactScope projection vocabulary used by the decorator
  'ArtifactScope',
  // response shape fields stamped onto every response by decorate(...)
  'pipeline_stage',
  'flow_ref',
  'artifact_refs',
  'next_step',
  // FLOW_REF stable anchor — the Lisp flow id this substrate implements
  'F-intent-alignment-plan-execution-loop',
  // bridge surfaces composed by / composing this substrate
  'mission_request',
  'mission_directive',
  'mission_plan',
  'mission_workflow',
];

// Tokens that MUST be present in the actual unified_entry.rs source. Robust
// substring checks that pin the public substrate API + the wire field name
// string literals the decorator stamps onto every response.
const UNIFIED_ENTRY_RS_NEEDLES = [
  // public stage runner entry point
  'pub(crate) async fn run_pipeline',
  // per-stage async runners
  'async fn run_directive_compile_stage',
  'async fn run_plan_compile_stage',
  'async fn run_plan_execute_stage',
  // stage label module + constants (s1/s3/s4/s5/s6 vocabulary)
  'pub(crate) mod stages',
  'MESSAGE_INTAKE',
  'DIRECTIVE_REVIEW_GATE',
  'PLAN_AUTHORING',
  'PLAN_REVIEW_GATE',
  'EXECUTION_RUNNER',
  '"s1_message_intake"',
  '"s3_alignment_review_gate"',
  '"s4_plan_authoring"',
  '"s5_plan_review_gate"',
  '"s6_execution_runner"',
  // ArtifactScope enum + its three projection variants
  'pub(crate) enum ArtifactScope',
  'ArtifactScope::Directive',
  'ArtifactScope::Plan',
  'ArtifactScope::Execution',
  // decorator + artifact_refs builder
  'fn build_artifact_refs',
  'fn decorate',
  // FLOW_REF constant binding the Lisp flow id into the Rust response shape
  'const FLOW_REF: &str = "F-intent-alignment-plan-execution-loop"',
  // wire field names stamped onto every response by decorate
  '"pipeline_stage"',
  '"flow_ref"',
  '"artifact_refs"',
  '"next_step"',
];

// mission_request must bridge to unified_entry::run_pipeline rather than
// reimplementing stage routing. This is the runtime-bridging anchor for the
// user-facing surface.
const REQUEST_HANDLER_NEEDLES = [
  'super::unified_entry::run_pipeline',
];

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, BLUEPRINT_NEEDLES);
  requireSurfaceBodyContains(
    diagnostics,
    files.blueprint,
    sources.blueprint,
    'unified-entry-runtime',
    BLUEPRINT_SURFACE_BODY_ANCHORS,
  );

  requireAll(
    diagnostics,
    files.unifiedEntry,
    sources.unifiedEntry,
    UNIFIED_ENTRY_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.requestHandler,
    sources.requestHandler,
    REQUEST_HANDLER_NEEDLES,
  );

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
// every needle appears inside its body. This is intentionally narrower than a
// whole-file `requireAll`: the V3 contract is that the named surface itself
// carries the semantics, not some unrelated section that happens to mention
// the same words.
function requireSurfaceBodyContains(diagnostics, file, source, surfaceName, needles) {
  const opener = `(surface ${surfaceName}`;
  const start = source.indexOf(opener);
  if (start < 0) {
    diagnostics.push({
      file,
      message: `(surface ${surfaceName} ...) form not found; cannot verify surface-body anchors: ${needles.join(', ')}`,
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
      message: `(surface ${surfaceName} ...) form is not balanced; cannot verify surface-body anchors`,
    });
    return;
  }
  const body = source.slice(start, end);
  for (const needle of needles) {
    if (!body.includes(needle)) {
      diagnostics.push({
        file,
        message: `(surface ${surfaceName} ...) body missing required anchor: ${needle}`,
      });
    }
  }
}

function runFixtures(json) {
  const cases = [];

  // ── Pass: every required key present in every file. ────────────────
  const goodFiles = {
    [DEFAULT_FILES.blueprint]: buildGoodBlueprint(),
    [DEFAULT_FILES.unifiedEntry]: buildGoodUnifiedEntry(),
    [DEFAULT_FILES.requestHandler]: buildGoodRequestHandler(),
  };
  cases.push({
    name: 'pass: blueprint surface + unified_entry.rs + request.rs bridge aligned',
    expectOk: true,
    files: goodFiles,
  });

  // ── Fail: blueprint missing the (surface unified-entry-runtime ...) form. ─
  const missingSurface = { ...goodFiles };
  missingSurface[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    '(surface unified-entry-runtime',
    '(surface unified-entry-GHOST',
  );
  cases.push({
    name: 'fail: blueprint missing (surface unified-entry-runtime ...)',
    expectOk: false,
    expectMessage: /\(surface unified-entry-runtime/,
    files: missingSurface,
  });

  // ── Fail: blueprint surface body drops the s4 plan-authoring stage anchor. ─
  const missingStage = { ...goodFiles };
  missingStage[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replaceAll(
    's4_plan_authoring',
    's4_plan_GHOST',
  );
  cases.push({
    name: 'fail: blueprint unified-entry-runtime surface drops s4_plan_authoring anchor',
    expectOk: false,
    expectMessage: /s4_plan_authoring/,
    files: missingStage,
  });

  // ── Fail: blueprint surface body drops mission_workflow runtime bridge anchor. ─
  const missingBridge = { ...goodFiles };
  missingBridge[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replaceAll(
    'mission_workflow',
    'mission_GHOSTflow',
  );
  cases.push({
    name: 'fail: blueprint unified-entry-runtime surface drops mission_workflow bridge anchor',
    expectOk: false,
    expectMessage: /mission_workflow/,
    files: missingBridge,
  });

  // ── Fail: blueprint surface body drops the FLOW_REF anchor. ─
  const missingFlowRef = { ...goodFiles };
  missingFlowRef[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replaceAll(
    'F-intent-alignment-plan-execution-loop',
    'F-intent-alignment-plan-GHOST',
  );
  cases.push({
    name: 'fail: blueprint unified-entry-runtime surface loses FLOW_REF anchor',
    expectOk: false,
    expectMessage: /F-intent-alignment-plan-execution-loop/,
    files: missingFlowRef,
  });

  // ── Fail: unified_entry.rs renames run_pipeline. ─
  const missingRunPipeline = { ...goodFiles };
  missingRunPipeline[DEFAULT_FILES.unifiedEntry] = goodFiles[
    DEFAULT_FILES.unifiedEntry
  ].replace(
    'pub(crate) async fn run_pipeline',
    'pub(crate) async fn run_GHOST_pipeline',
  );
  cases.push({
    name: 'fail: unified_entry.rs renames run_pipeline',
    expectOk: false,
    expectMessage: /pub\(crate\) async fn run_pipeline/,
    files: missingRunPipeline,
  });

  // ── Fail: unified_entry.rs drops the ArtifactScope enum. ─
  const missingArtifactScope = { ...goodFiles };
  missingArtifactScope[DEFAULT_FILES.unifiedEntry] = goodFiles[
    DEFAULT_FILES.unifiedEntry
  ].replace(
    'pub(crate) enum ArtifactScope',
    'pub(crate) enum ArtifactGHOST',
  );
  cases.push({
    name: 'fail: unified_entry.rs drops ArtifactScope enum',
    expectOk: false,
    expectMessage: /pub\(crate\) enum ArtifactScope/,
    files: missingArtifactScope,
  });

  // ── Fail: unified_entry.rs drops the FLOW_REF binding. ─
  const missingFlowRefRs = { ...goodFiles };
  missingFlowRefRs[DEFAULT_FILES.unifiedEntry] = goodFiles[
    DEFAULT_FILES.unifiedEntry
  ].replace(
    'const FLOW_REF: &str = "F-intent-alignment-plan-execution-loop"',
    'const FLOW_GHOST: &str = "F-intent-alignment-plan-execution-loop"',
  );
  cases.push({
    name: 'fail: unified_entry.rs drops FLOW_REF binding',
    expectOk: false,
    expectMessage: /const FLOW_REF: &str = "F-intent-alignment-plan-execution-loop"/,
    files: missingFlowRefRs,
  });

  // ── Fail: request.rs no longer bridges via unified_entry::run_pipeline. ─
  const missingBridgeRs = { ...goodFiles };
  missingBridgeRs[DEFAULT_FILES.requestHandler] = goodFiles[
    DEFAULT_FILES.requestHandler
  ].replaceAll(
    'super::unified_entry::run_pipeline',
    'super::unified_entry::run_GHOST_pipeline',
  );
  cases.push({
    name: 'fail: request.rs no longer bridges via unified_entry::run_pipeline',
    expectOk: false,
    expectMessage: /super::unified_entry::run_pipeline/,
    files: missingBridgeRs,
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
    console.error(`v3 unified-entry-runtime fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 unified-entry-runtime fixtures OK (${cases.length} cases)`);
  }
}

function materializeFixture(filesByPath) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-unified-entry-iso-'));
  for (const [rel, body] of Object.entries(filesByPath)) {
    const abs = path.join(root, rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, body);
  }
  return root;
}

function buildGoodBlueprint() {
  // Minimal but realistic V3 blueprint snippet that satisfies every
  // BLUEPRINT_NEEDLES entry AND the surface-body anchors. Any non-fixture
  // declaration that names the same anchors inside the surface form would
  // also pass.
  return `;; fixture
(missiond-blueprint
  (implementation-map
    (surface unified-entry-runtime
      :status "code-aligned"
      :implements [unified-entry F-intent-alignment-plan-execution-loop]
      :code ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
      :note "unified-entry-runtime is the in-daemon stage substrate that mission_request, mission_directive, mission_plan, and mission_workflow compose to drive F-intent-alignment-plan-execution-loop. run_pipeline is the single dispatcher; per-stage runners are run_directive_compile_stage, run_plan_compile_stage, and run_plan_execute_stage. The wire stage label vocabulary stamped into pipeline_stage is s1_message_intake, s3_alignment_review_gate, s4_plan_authoring, s5_plan_review_gate, s6_execution_runner. ArtifactScope (Directive | Plan | Execution) is the projection scope the decorator surfaces into artifact_refs. Every response carries the same shape: pipeline_stage + flow_ref (FLOW_REF=F-intent-alignment-plan-execution-loop) + artifact_refs + next_step (and next_call when meaningful). mission_request bridges via super::unified_entry::run_pipeline; mission_directive / mission_plan / mission_workflow are the inner handlers each stage forwards to."))
  (compression-contract
    :checks ["node scripts/check-v3-unified-entry-isomorphism.mjs"]))
`;
}

function buildGoodUnifiedEntry() {
  return `// fixture
pub(crate) mod stages {
    pub(crate) const MESSAGE_INTAKE: &str = "s1_message_intake";
    pub(crate) const DIRECTIVE_REVIEW_GATE: &str = "s3_alignment_review_gate";
    pub(crate) const PLAN_AUTHORING: &str = "s4_plan_authoring";
    pub(crate) const PLAN_REVIEW_GATE: &str = "s5_plan_review_gate";
    pub(crate) const EXECUTION_RUNNER: &str = "s6_execution_runner";
}

const FLOW_REF: &str = "F-intent-alignment-plan-execution-loop";

pub(crate) enum ArtifactScope { Directive, Plan, Execution }
const ARTIFACT_SCOPE_REFS: &[&str] = &[
    "ArtifactScope::Directive",
    "ArtifactScope::Plan",
    "ArtifactScope::Execution",
];

pub(crate) async fn run_pipeline() {}
async fn run_directive_compile_stage() {}
async fn run_plan_compile_stage() {}
async fn run_plan_execute_stage() {}

fn build_artifact_refs() {}
fn decorate() {
    // wire field names stamped onto every response
    let _names = ["pipeline_stage", "flow_ref", "artifact_refs", "next_step"];
}
`;
}

function buildGoodRequestHandler() {
  return `// fixture
async fn handle_start() {
    let inner = super::unified_entry::run_pipeline(state, args).await?;
}
`;
}

main();
