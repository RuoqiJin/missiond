#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-workstation-dispatch-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 workstation-dispatch substrate Lisp/code isomorphism contract.
This surface is intentionally split from workstation-config: workstation-config
owns slot/model/prompt dispatch, workstation-dispatch owns the
WorkstationDispatchOutcome substrate that mission_plan execute_internal drives
for both --execute-dry-run and --execute-real-dispatch smokes.

  - V3 blueprint declares (surface workstation-dispatch ...) with
    :status "code-aligned" and :code naming workstation_dispatch.rs.
  - The surface note pins the substrate contract names that other surfaces
    rely on by string match:
      WorkstationDispatchOutcome / DryRun / Dispatched
      run_workstation_dispatch_with_contract_and_trace
      evaluate_dispatch_decision
      extract_inner_board_task_id
      runner_status="workstation_dispatch_v0"
      workstation_dispatch_status="dry_run_no_dispatch" / "dispatched"
      target_tool, task_brief_preview, delegated_board_task_id
  - compression-contract :checks pins this checker.
  - workstation_dispatch.rs exposes the stable substrate API:
      pub(crate) enum WorkstationDispatchOutcome
        { Dispatched, DryRun, InnerError, SafeDescriptor }
      pub(crate) fn evaluate_dispatch_decision
      pub(crate) async fn run_workstation_dispatch_with_contract_and_trace
      pub(crate) fn outcome_to_response_fields
      fn extract_inner_board_task_id
    plus the deterministic status strings ("dispatched",
    "dry_run_no_dispatch", "inner_returned_error") that
    outcome_to_response_fields surfaces under "workstation_dispatch_status".
  - scripts/check-v3-request-flow-smoke.mjs asserts the substrate-level
    invariants (workstation_dispatch_v0 / workstation_dispatch_status /
    dry_run_no_dispatch / dispatched / delegated_board_task_id) so the
    --execute-dry-run and --execute-real-dispatch audits prove MissionD
    reached this substrate rather than bridge mode.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workstationDispatch:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs',
  requestFlowSmoke: 'scripts/check-v3-request-flow-smoke.mjs',
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
    console.log('v3 workstation-dispatch Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 workstation-dispatch Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

const BLUEPRINT_NEEDLES = [
  '(surface workstation-dispatch',
  ':status "code-aligned"',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs',
  'WorkstationDispatchOutcome',
  'DryRun',
  'Dispatched',
  'run_workstation_dispatch_with_contract_and_trace',
  'evaluate_dispatch_decision',
  'extract_inner_board_task_id',
  'workstation_dispatch_v0',
  'workstation_dispatch_status',
  'dry_run_no_dispatch',
  'dispatched',
  'target_tool',
  'task_brief_preview',
  'delegated_board_task_id',
  'mission_task_delegate',
  'node scripts/check-v3-workstation-dispatch-isomorphism.mjs',
];

// Anchors that MUST live inside the (surface workstation-dispatch ...) form
// itself, not just somewhere in the file. The substrate's stability
// invariants belong inside the surface declaration so reviewers see them
// next to the code paths.
const BLUEPRINT_SURFACE_BODY_ANCHORS = [
  'WorkstationDispatchOutcome',
  'DryRun',
  'Dispatched',
  'run_workstation_dispatch_with_contract_and_trace',
  'evaluate_dispatch_decision',
  'extract_inner_board_task_id',
  'dry_run_no_dispatch',
];

const WORKSTATION_DISPATCH_RS_NEEDLES = [
  'pub(crate) enum WorkstationDispatchOutcome',
  'WorkstationDispatchOutcome::Dispatched',
  'WorkstationDispatchOutcome::DryRun',
  'WorkstationDispatchOutcome::InnerError',
  'WorkstationDispatchOutcome::SafeDescriptor',
  'pub(crate) enum SafeDescriptorReason',
  'pub(crate) fn evaluate_dispatch_decision',
  'pub(crate) async fn run_workstation_dispatch_with_contract_and_trace',
  'pub(crate) fn outcome_to_response_fields',
  'fn extract_inner_board_task_id',
  'pub(crate) fn classify_task_kind',
  '"workstation_dispatch_status"',
  '"task_brief_preview"',
  '"delegated_board_task_id"',
  '"inner_result"',
  '"dispatched"',
  '"dry_run_no_dispatch"',
  '"inner_returned_error"',
  'mission_task_delegate',
];

const REQUEST_FLOW_SMOKE_NEEDLES = [
  'workstation_dispatch_v0',
  'workstation_dispatch_status',
  'dry_run_no_dispatch',
  'dispatched',
  'target_tool',
  'task_brief_preview',
  'delegated_board_task_id',
  'mission_task_delegate',
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
    'workstation-dispatch',
    BLUEPRINT_SURFACE_BODY_ANCHORS,
  );

  requireAll(
    diagnostics,
    files.workstationDispatch,
    sources.workstationDispatch,
    WORKSTATION_DISPATCH_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.requestFlowSmoke,
    sources.requestFlowSmoke,
    REQUEST_FLOW_SMOKE_NEEDLES,
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
    [DEFAULT_FILES.workstationDispatch]: buildGoodWorkstationDispatch(),
    [DEFAULT_FILES.requestFlowSmoke]: buildGoodRequestFlowSmoke(),
  };
  cases.push({
    name: 'pass: blueprint surface + workstation_dispatch.rs + request-flow smoke aligned',
    expectOk: true,
    files: goodFiles,
  });

  // ── Fail: blueprint missing the (surface workstation-dispatch ...) form. ─
  const missingSurface = { ...goodFiles };
  missingSurface[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    '(surface workstation-dispatch',
    '(surface workstation-GHOST',
  );
  cases.push({
    name: 'fail: blueprint missing (surface workstation-dispatch ...)',
    expectOk: false,
    expectMessage: /\(surface workstation-dispatch/,
    files: missingSurface,
  });

  // ── Fail: blueprint surface body missing extract_inner_board_task_id. ─
  const missingAnchor = { ...goodFiles };
  missingAnchor[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replaceAll(
    'extract_inner_board_task_id',
    'extract_inner_board_task_GHOST',
  );
  cases.push({
    name: 'fail: blueprint workstation-dispatch surface drops extract_inner_board_task_id',
    expectOk: false,
    expectMessage: /extract_inner_board_task_id/,
    files: missingAnchor,
  });

  // ── Fail: blueprint surface body missing dry_run_no_dispatch anchor. ─
  const missingDryRunStatus = { ...goodFiles };
  missingDryRunStatus[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint]
    .replaceAll('dry_run_no_dispatch', 'dry_run_no_GHOST');
  cases.push({
    name: 'fail: blueprint workstation-dispatch surface loses dry_run_no_dispatch anchor',
    expectOk: false,
    expectMessage: /dry_run_no_dispatch/,
    files: missingDryRunStatus,
  });

  // ── Fail: workstation_dispatch.rs missing WorkstationDispatchOutcome enum. ─
  const missingOutcome = { ...goodFiles };
  missingOutcome[DEFAULT_FILES.workstationDispatch] = goodFiles[
    DEFAULT_FILES.workstationDispatch
  ].replace(
    'pub(crate) enum WorkstationDispatchOutcome',
    'pub(crate) enum WorkstationDispatchGHOST',
  );
  cases.push({
    name: 'fail: workstation_dispatch.rs lost the WorkstationDispatchOutcome enum',
    expectOk: false,
    expectMessage: /pub\(crate\) enum WorkstationDispatchOutcome/,
    files: missingOutcome,
  });

  // ── Fail: workstation_dispatch.rs renames extract_inner_board_task_id. ─
  const missingExtract = { ...goodFiles };
  missingExtract[DEFAULT_FILES.workstationDispatch] = goodFiles[
    DEFAULT_FILES.workstationDispatch
  ].replace('fn extract_inner_board_task_id', 'fn extract_inner_board_task_GHOST');
  cases.push({
    name: 'fail: workstation_dispatch.rs renames extract_inner_board_task_id',
    expectOk: false,
    expectMessage: /extract_inner_board_task_id/,
    files: missingExtract,
  });

  // ── Fail: request-flow smoke drops workstation_dispatch_v0 anchor. ─
  const missingSmoke = { ...goodFiles };
  missingSmoke[DEFAULT_FILES.requestFlowSmoke] = goodFiles[
    DEFAULT_FILES.requestFlowSmoke
  ].replaceAll('workstation_dispatch_v0', 'workstation_dispatch_GHOST');
  cases.push({
    name: 'fail: request-flow smoke loses workstation_dispatch_v0 invariant',
    expectOk: false,
    expectMessage: /workstation_dispatch_v0/,
    files: missingSmoke,
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
    console.error(`v3 workstation-dispatch fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 workstation-dispatch fixtures OK (${cases.length} cases)`);
  }
}

function materializeFixture(filesByPath) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-workstation-dispatch-iso-'));
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
    (surface workstation-dispatch
      :status "code-aligned"
      :implements [workstation-dispatch-substrate execute-dry-run-smoke real-dispatch-smoke]
      :code ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
      :note "workstation-dispatch is the substrate that mission_plan execute_internal drives when target=mission_task_delegate is inferred or explicit. WorkstationDispatchOutcome is the closed enum (Dispatched | DryRun | InnerError | SafeDescriptor) returned by run_workstation_dispatch_with_contract_and_trace; outcome_to_response_fields projects each variant onto a stable wire shape the outer plan-execute response wraps. evaluate_dispatch_decision is the single auto-inference + opt-in gate (target=mission_task_delegate + INFERABLE strategy + non-empty objective + scoping signal). extract_inner_board_task_id projects the delegated BoardTask UUID from the inner mission_task_delegate payload onto a top-level delegated_board_task_id field. The substrate emits a fixed status vocabulary readable from the response: workstation_dispatch_status='dispatched' | 'dry_run_no_dispatch' | 'inner_returned_error' | safe-descriptor reasons. Wire fields callers and smokes pin: runner_status='workstation_dispatch_v0' (set by mission_plan), execute_mode='internal', target_tool=mission_task_delegate, dispatch_strategy, task_brief_preview, delegated_board_task_id (Dispatched only), inner_result. Bridge mode is no longer accepted as a no-dispatch proof; both --execute-dry-run and --execute-real-dispatch route through this substrate."))
  (compression-contract
    :checks ["node scripts/check-v3-workstation-dispatch-isomorphism.mjs"]))
`;
}

function buildGoodWorkstationDispatch() {
  return `// fixture
pub(crate) enum WorkstationDispatchOutcome {
    Dispatched { /* ... */ },
    InnerError { /* ... */ },
    DryRun { /* ... */ },
    SafeDescriptor { /* ... */ },
}
const REFS: &[&str] = &[
    "WorkstationDispatchOutcome::Dispatched",
    "WorkstationDispatchOutcome::DryRun",
    "WorkstationDispatchOutcome::InnerError",
    "WorkstationDispatchOutcome::SafeDescriptor",
];
pub(crate) enum SafeDescriptorReason { UnsupportedTarget(String) }
pub(crate) fn evaluate_dispatch_decision() {}
pub(crate) async fn run_workstation_dispatch_with_contract_and_trace() {}
pub(crate) fn outcome_to_response_fields() {}
fn extract_inner_board_task_id() {}
pub(crate) fn classify_task_kind() {}
// Stable wire field names projected by outcome_to_response_fields:
//   "workstation_dispatch_status" "task_brief_preview"
//   "delegated_board_task_id"     "inner_result"
// Stable status values returned by WorkstationDispatchOutcome::status:
//   "dispatched" "dry_run_no_dispatch" "inner_returned_error"
// substrate only ever wraps mission_task_delegate
`;
}

function buildGoodRequestFlowSmoke() {
  // Realistic-ish smoke fixture. Required tokens are flat substring hits
  // matching the live wave-46/47 invariants.
  return `// fixture
const ASSERTIONS = [
  'pipeline_result.runner_status === "workstation_dispatch_v0"',
  'pipeline_result.workstation_dispatch_status === "dispatched"',
  'pipeline_result.workstation_dispatch_status === "dry_run_no_dispatch"',
  'pipeline_result.target_tool === "mission_task_delegate"',
  'pipeline_result.task_brief_preview is non-empty',
  'pipeline_result.delegated_board_task_id is a UUID',
];
`;
}

main();
