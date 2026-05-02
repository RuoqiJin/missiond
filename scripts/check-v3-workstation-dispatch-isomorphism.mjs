#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-workstation-dispatch-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 workstation-dispatch substrate Lisp/code isomorphism contract.
This surface is intentionally split from workstation-config: workstation-config
owns slot/model/prompt dispatch, workstation-dispatch owns the
WorkstationDispatchOutcome substrate that mission_plan execute_internal drives
for both --execute-dry-run and --execute-real-dispatch smokes.

  - V3 blueprint declares (surface workstation-dispatch ...) with
    :status "code-aligned" and :code naming workstation_dispatch.rs plus
    the physical descriptor / decision / outcome / runner / brief splits under workstation_dispatch/.
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
    while workstation_dispatch/descriptor.rs owns the task-contract v1
    Lisp projection parser:
      ParsedTaskContract / TaskContractParseError
      parse_task_contract / load_task_contract
      resolve_contract_path_public
    and workstation_dispatch/decision.rs owns the pure dispatch gate:
      WorkstationDispatchSource / DispatchDecision / InferenceContext
      INFERABLE_DISPATCH_STRATEGIES / evaluate_dispatch_decision
    and workstation_dispatch/outcome.rs owns the response projection:
      WorkstationDispatchOutcome / SafeDescriptorReason
      outcome_to_response_fields / extract_inner_board_task_id
    and workstation_dispatch/runner.rs owns the inner dispatch adapter:
      run_workstation_dispatch* / mission_task_delegate / evidence sidecar
    and workstation_dispatch/brief.rs owns the scoped handoff renderer:
      BriefTaskKind / classify_task_kind / build_task_brief*
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
  workstationDescriptor:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/descriptor.rs',
  workstationDecision:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/decision.rs',
  workstationOutcome:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs',
  workstationRunner:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/runner.rs',
  workstationBrief:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/brief.rs',
  workstationProposal:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/proposal.rs',
  workstationAutoSpawn:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn.rs',
  workstationAutoSpawnGate:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/gate.rs',
  workstationAutoSpawnHash:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/hash.rs',
  workstationAutoSpawnInput:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/input.rs',
  workstationAutoSpawnOutcome:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/outcome.rs',
  workstationTests:
    'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/tests.rs',
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
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/descriptor.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/decision.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/runner.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/brief.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/proposal.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/gate.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/hash.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/input.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/outcome.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/tests.rs',
  'WorkstationDispatchOutcome',
  'DryRun',
  'Dispatched',
  'ParsedTaskContract',
  'InferenceContext',
  'SafeDescriptorReason',
  'BriefTaskKind',
  'parse_task_contract',
  'run_workstation_dispatch_with_contract_and_trace',
  'evaluate_dispatch_decision',
  'outcome_to_response_fields',
  'run_workstation_dispatch_with_contract_and_trace',
  'classify_task_kind',
  'build_task_brief',
  'WorkstationProposalBundle',
  'request_workstation_proposals',
  'proposal model label projects from router-runtime-policy queued_sonnet_model',
  'WorkstationAutoSpawnGateOutcome',
  'evaluate_workstation_auto_spawn_gate',
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
  'forbidden git state mutations',
];

// Anchors that MUST live inside the (surface workstation-dispatch ...) form
// itself, not just somewhere in the file. The substrate's stability
// invariants belong inside the surface declaration so reviewers see them
// next to the code paths.
const BLUEPRINT_SURFACE_BODY_ANCHORS = [
  'WorkstationDispatchOutcome',
  'DryRun',
  'Dispatched',
  'ParsedTaskContract',
  'InferenceContext',
  'SafeDescriptorReason',
  'BriefTaskKind',
  'parse_task_contract',
  'run_workstation_dispatch_with_contract_and_trace',
  'evaluate_dispatch_decision',
  'outcome_to_response_fields',
  'run_workstation_dispatch_with_contract_and_trace',
  'classify_task_kind',
  'build_task_brief',
  'proposal model label projects from router-runtime-policy queued_sonnet_model',
  'extract_inner_board_task_id',
  'dry_run_no_dispatch',
  'forbidden git state mutations',
];

const WORKSTATION_DISPATCH_RS_NEEDLES = [
  'mod descriptor;',
  'mod decision;',
  'mod outcome;',
  'mod runner;',
  'mod brief;',
  'mod proposal;',
  'mod auto_spawn;',
  'mod tests;',
  'pub(crate) use descriptor::{',
  'pub(crate) use decision::{',
  'pub(crate) use outcome::{',
  'pub(crate) use runner::{',
  'pub(crate) use brief::{',
  'pub(crate) use proposal::{',
  'pub(crate) use auto_spawn::{',
  'evaluate_dispatch_decision',
  'run_workstation_dispatch_with_contract_and_trace',
  'outcome_to_response_fields',
  'classify_task_kind',
  'WorkstationDispatchOutcome',
  'SafeDescriptorReason',
  'mission_task_delegate',
];

const WORKSTATION_DESCRIPTOR_RS_NEEDLES = [
  'pub(crate) struct ParsedTaskContract',
  'pub(crate) enum TaskContractParseError',
  'pub(crate) fn parse_task_contract',
  'pub(crate) fn load_task_contract',
  'pub(super) fn resolve_contract_path',
  'pub(crate) fn resolve_contract_path_public',
  'missiond.task-contract.v1',
  'session-trace-path',
];

const WORKSTATION_DECISION_RS_NEEDLES = [
  'pub(crate) fn opt_in_requested',
  'pub(crate) enum WorkstationDispatchSource',
  'pub(crate) struct DispatchDecision',
  'pub(crate) const INFERABLE_DISPATCH_STRATEGIES',
  'pub(crate) struct InferenceContext',
  'pub(crate) fn explicit_workstation_dispatch_flag',
  'pub(crate) fn evaluate_dispatch_decision',
  'explicit_arg',
  'inferred',
  'mission_task_delegate',
];

const WORKSTATION_OUTCOME_RS_NEEDLES = [
  'pub(crate) enum WorkstationDispatchOutcome',
  'WorkstationDispatchOutcome::Dispatched',
  'WorkstationDispatchOutcome::DryRun',
  'WorkstationDispatchOutcome::InnerError',
  'WorkstationDispatchOutcome::SafeDescriptor',
  'pub(crate) enum SafeDescriptorReason',
  'pub(crate) fn outcome_to_response_fields',
  'pub(crate) fn extract_inner_board_task_id',
  'pub(crate) fn truncate_brief_preview',
  'SCOPED_COMMIT_REQUIRED',
  'SCOPED_COMMIT_POLICY',
  'CompletionLogUnavailable',
  '"workstation_dispatch_status"',
  '"task_brief_preview"',
  '"delegated_board_task_id"',
  '"inner_result"',
  '"dispatched"',
  '"dry_run_no_dispatch"',
  '"inner_returned_error"',
  '"skipped_completion_log_unavailable"',
];

const WORKSTATION_RUNNER_RS_NEEDLES = [
  'pub(crate) async fn run_workstation_dispatch',
  'pub(crate) async fn run_workstation_dispatch_with_contract',
  'pub(crate) async fn run_workstation_dispatch_with_contract_and_trace',
  'resolve_target_project_root',
  'resolve_contract_path',
  'build_task_brief_with_source_and_trace',
  'workstation_execution_id',
  'agent_execution::handle',
  '"action": "open"',
  'mission_task_delegate',
  'tool_result_payload',
  'EvidenceEntry::new',
  'evidence_collector::append',
  'WorkstationDispatchOutcome::Dispatched',
  'WorkstationDispatchOutcome::DryRun',
  'WorkstationDispatchOutcome::InnerError',
  'WorkstationDispatchOutcome::SafeDescriptor',
  'SafeDescriptorReason::MalformedTaskContract',
  'SafeDescriptorReason::CompletionLogUnavailable',
  'task_contract_source_path',
  'session_trace_path',
];

const WORKSTATION_BRIEF_RS_NEEDLES = [
  'pub(crate) enum BriefTaskKind',
  'pub(crate) fn classify_task_kind',
  'pub(crate) fn workstation_execution_id',
  'pub(crate) fn build_task_brief',
  'pub(crate) fn build_task_brief_with_source',
  'pub(crate) fn build_task_brief_with_source_and_trace',
  'AGENT_TEAM_OBJECTIVE_HINT',
  'Execution log:',
  'execution_id=',
  'dispatcher pre-opened this MissionD audit log',
  'Completion handoff (scoped commit)',
  'Session trace',
  'COMMIT_POLICY_SCOPED',
  // Wave-N / "harden workstation brief against hidden git state mutations" — the
  // rendered brief must carry a single visible bullet that names every git
  // command we have seen workers reach for to "clean up" before staging
  // (`git stash`, `git reset`, `git checkout`, `git restore`) and instructs
  // them to stop and add a BoardTask note instead unless the task contract
  // explicitly owns the operation.
  'forbidden git state mutations',
  '`git stash`',
  '`git reset`',
  '`git checkout`',
  '`git restore`',
];

const WORKSTATION_PROPOSAL_RS_NEEDLES = [
  'RouterRuntimeConfig::load_for_current_dir',
  'V3_BLUEPRINT_CONFIG_ERROR',
  'queued_sonnet_model',
  'pub(crate) const WORKSTATION_PROPOSAL_FIELDS',
  'pub(crate) const WORKSTATION_PROPOSAL_CAP',
  'pub(crate) const SONNET_WORKSTATION_PROPOSAL_CALLER',
  'pub(crate) const PROPOSAL_VALID_TARGETS',
  'pub(crate) const PROPOSAL_VALID_STRATEGIES',
  'pub(crate) enum WorkstationProposalStatus',
  'pub(crate) enum WorkstationProposalSafetyStatus',
  'pub(crate) enum WorkstationProposalConfidence',
  'pub(crate) struct WorkstationProposal',
  'pub(crate) struct WorkstationProposalBundle',
  'pub(crate) struct WorkstationProposalGate',
  'pub(crate) fn build_workstation_proposal_prompt',
  'pub(crate) fn parse_workstation_proposals',
  'pub(crate) fn classify_proposal_safety',
  'pub(crate) async fn request_workstation_proposals',
  '"workstation_dispatch_proposal"',
  '"applied": false',
  '"auto_spawn": false',
  'no fallback to claude -p',
];

const WORKSTATION_PROPOSAL_RS_FORBIDDEN = [
  'SONNET_WORKSTATION_PROPOSAL_MODEL',
  'const SONNET_WORKSTATION_PROPOSAL_MODEL',
  'Some("claude-sonnet".to_string())',
];

const WORKSTATION_AUTO_SPAWN_RS_NEEDLES = [
  'mod gate;',
  'mod hash;',
  'mod input;',
  'mod outcome;',
  'pub(crate) use self::gate::{enforce_auto_spawn_preflight, evaluate_workstation_auto_spawn_gate};',
  'pub(crate) use self::hash::{compute_workstation_proposal_hash, WorkstationProposalHashStatus};',
  'pub(crate) use self::input::{',
  'pub(crate) use self::outcome::{WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus};',
  'autonomous workstation true spawn v1',
];

const WORKSTATION_AUTO_SPAWN_INPUT_RS_NEEDLES = [
  'use super::*;',
  'pub(crate) const AUTO_SPAWN_MISSING_PROPOSAL_HASH',
  'pub(crate) const AUTO_SPAWN_PROPOSAL_HASH_MISMATCH',
  'pub(crate) const AUTO_SPAWN_INVALID_PARAM',
  'pub(crate) struct WorkstationAutoSpawnInput',
  'pub(crate) fn parse_workstation_auto_spawn_input',
  'proposal_json_kind',
];

const WORKSTATION_AUTO_SPAWN_HASH_RS_NEEDLES = [
  'use super::*;',
  'pub(crate) enum WorkstationProposalHashStatus',
  'pub(crate) fn compute_workstation_proposal_hash',
  'Sha256',
];

const WORKSTATION_AUTO_SPAWN_OUTCOME_RS_NEEDLES = [
  'use super::*;',
  'pub(crate) enum WorkstationAutoSpawnStatus',
  'pub(crate) struct WorkstationAutoSpawnGateOutcome',
  'WorkstationAutoSpawnStatus::Spawned',
  'WorkstationAutoSpawnStatus::SkippedUnavailable',
  'WorkstationAutoSpawnStatus::SkippedSubstrateRefused',
  'json!({',
];

const WORKSTATION_AUTO_SPAWN_GATE_RS_NEEDLES = [
  'use super::*;',
  'pub(crate) fn enforce_auto_spawn_preflight',
  'pub(crate) fn evaluate_workstation_auto_spawn_gate',
  'WorkstationAutoSpawnStatus::Spawned',
  'WorkstationAutoSpawnStatus::SkippedUnavailable',
  'WorkstationProposalHashStatus::Matches',
  'mission_task_delegate',
  'task_contract_path',
  'preflight_status_acceptable',
];

const WORKSTATION_TESTS_RS_NEEDLES = [
  'fn fixture_plan',
  'fn opt_in_requires_explicit_arg_or_plan_hint',
  'fn build_task_brief_includes_canonical_sections_and_scoped_commit_reminder',
  'fn parse_task_contract_extracts_every_consumed_field',
  'fn parse_workstation_proposals_accepts_canonical_envelope',
  'fn wave22_05_gate_happy_path_returns_spawned',
  'fn smoke_wave22_07_workstation_auto_spawn_gate_rejects_missing_hash_accepts_fixture_hash',
  'fn build_task_brief_with_source_and_trace_renders_session_trace_block_when_path_supplied',
  'fn brief_forbids_hidden_git_state_mutations_unless_owned',
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
      sources[key] = key === 'blueprint' ? readBlueprintWithEvidenceSidecars(root, rel) : fs.readFileSync(abs, 'utf8');
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
    files.workstationDescriptor,
    sources.workstationDescriptor,
    WORKSTATION_DESCRIPTOR_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationDecision,
    sources.workstationDecision,
    WORKSTATION_DECISION_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationOutcome,
    sources.workstationOutcome,
    WORKSTATION_OUTCOME_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationRunner,
    sources.workstationRunner,
    WORKSTATION_RUNNER_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationBrief,
    sources.workstationBrief,
    WORKSTATION_BRIEF_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationProposal,
    sources.workstationProposal,
    WORKSTATION_PROPOSAL_RS_NEEDLES,
  );
  forbidAll(
    diagnostics,
    files.workstationProposal,
    sources.workstationProposal,
    WORKSTATION_PROPOSAL_RS_FORBIDDEN,
  );
  requireAll(
    diagnostics,
    files.workstationAutoSpawn,
    sources.workstationAutoSpawn,
    WORKSTATION_AUTO_SPAWN_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationAutoSpawnInput,
    sources.workstationAutoSpawnInput,
    WORKSTATION_AUTO_SPAWN_INPUT_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationAutoSpawnHash,
    sources.workstationAutoSpawnHash,
    WORKSTATION_AUTO_SPAWN_HASH_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationAutoSpawnOutcome,
    sources.workstationAutoSpawnOutcome,
    WORKSTATION_AUTO_SPAWN_OUTCOME_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationAutoSpawnGate,
    sources.workstationAutoSpawnGate,
    WORKSTATION_AUTO_SPAWN_GATE_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.workstationTests,
    sources.workstationTests,
    WORKSTATION_TESTS_RS_NEEDLES,
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

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push({ file, message: `forbidden contract text present: ${needle}` });
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
    [DEFAULT_FILES.workstationDescriptor]: buildGoodWorkstationDescriptor(),
    [DEFAULT_FILES.workstationDecision]: buildGoodWorkstationDecision(),
    [DEFAULT_FILES.workstationOutcome]: buildGoodWorkstationOutcome(),
    [DEFAULT_FILES.workstationRunner]: buildGoodWorkstationRunner(),
    [DEFAULT_FILES.workstationBrief]: buildGoodWorkstationBrief(),
    [DEFAULT_FILES.workstationProposal]: buildGoodWorkstationProposal(),
    [DEFAULT_FILES.workstationAutoSpawn]: buildGoodWorkstationAutoSpawn(),
    [DEFAULT_FILES.workstationAutoSpawnGate]: buildGoodWorkstationAutoSpawnGate(),
    [DEFAULT_FILES.workstationAutoSpawnHash]: buildGoodWorkstationAutoSpawnHash(),
    [DEFAULT_FILES.workstationAutoSpawnInput]: buildGoodWorkstationAutoSpawnInput(),
    [DEFAULT_FILES.workstationAutoSpawnOutcome]: buildGoodWorkstationAutoSpawnOutcome(),
    [DEFAULT_FILES.workstationTests]: buildGoodWorkstationTests(),
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
  missingOutcome[DEFAULT_FILES.workstationOutcome] = goodFiles[
    DEFAULT_FILES.workstationOutcome
  ].replace(
    'pub(crate) enum WorkstationDispatchOutcome',
    'pub(crate) enum WorkstationDispatchGHOST',
  );
  cases.push({
    name: 'fail: workstation_dispatch/outcome.rs lost the WorkstationDispatchOutcome enum',
    expectOk: false,
    expectMessage: /pub\(crate\) enum WorkstationDispatchOutcome/,
    files: missingOutcome,
  });

  // ── Fail: workstation_dispatch/outcome.rs renames extract_inner_board_task_id. ─
  const missingExtract = { ...goodFiles };
  missingExtract[DEFAULT_FILES.workstationOutcome] = goodFiles[
    DEFAULT_FILES.workstationOutcome
  ].replace('pub(crate) fn extract_inner_board_task_id', 'pub(crate) fn extract_inner_board_task_GHOST');
  cases.push({
    name: 'fail: workstation_dispatch/outcome.rs renames extract_inner_board_task_id',
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
      :code ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/decision.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/runner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/brief.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/proposal.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/gate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/hash.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/input.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/tests.rs"]
      :note "workstation-dispatch is the substrate that mission_plan execute_internal drives when target=mission_task_delegate is inferred or explicit. workstation_dispatch/outcome.rs owns WorkstationDispatchOutcome, SafeDescriptorReason, and outcome_to_response_fields; WorkstationDispatchOutcome is the closed enum (Dispatched | DryRun | InnerError | SafeDescriptor) returned by run_workstation_dispatch_with_contract_and_trace; outcome_to_response_fields projects each variant onto a stable wire shape the outer plan-execute response wraps. workstation_dispatch/runner.rs owns run_workstation_dispatch / run_workstation_dispatch_with_contract / run_workstation_dispatch_with_contract_and_trace, the mission_task_delegate inner adapter, task_contract_source_path/session_trace_path threading, and the evidence sidecar append. workstation_dispatch/descriptor.rs owns ParsedTaskContract and parse_task_contract so task-contract v1 Lisp projection is physically split from the substrate facade. workstation_dispatch/decision.rs owns InferenceContext and evaluate_dispatch_decision, the single auto-inference + opt-in gate (target=mission_task_delegate + INFERABLE strategy + non-empty objective + scoping signal). workstation_dispatch/brief.rs owns BriefTaskKind, classify_task_kind, and build_task_brief / build_task_brief_with_source / build_task_brief_with_source_and_trace, including the Completion handoff (scoped commit), Session trace block, AGENT_TEAM_OBJECTIVE_HINT projection, and the forbidden git state mutations bullet (no \`git stash\`, \`git reset\`, \`git checkout\`, or \`git restore\` unless the task contract explicitly owns that operation). workstation_dispatch/proposal.rs owns WorkstationProposalBundle, request_workstation_proposals, parse_workstation_proposals, proposal safety/confidence enums, applied=false, auto_spawn=false, proposal model label projects from router-runtime-policy queued_sonnet_model, and the no fallback to claude -p invariant. workstation_dispatch/auto_spawn.rs owns the auto-spawn facade; auto_spawn/input.rs owns strict gate inputs; auto_spawn/hash.rs owns proposal hash projection; auto_spawn/outcome.rs owns WorkstationAutoSpawnGateOutcome; auto_spawn/gate.rs owns enforce_auto_spawn_preflight and evaluate_workstation_auto_spawn_gate for the mission_task_delegate true-spawn gate. extract_inner_board_task_id projects the delegated BoardTask UUID from the inner mission_task_delegate payload onto a top-level delegated_board_task_id field. The substrate emits a fixed status vocabulary readable from the response: workstation_dispatch_status='dispatched' | 'dry_run_no_dispatch' | 'inner_returned_error' | safe-descriptor reasons. Wire fields callers and smokes pin: runner_status='workstation_dispatch_v0' (set by mission_plan), execute_mode='internal', target_tool=mission_task_delegate, dispatch_strategy, task_brief_preview, delegated_board_task_id (Dispatched only), inner_result. Bridge mode is no longer accepted as a no-dispatch proof; both --execute-dry-run and --execute-real-dispatch route through this substrate."))
  (compression-contract
    :checks ["node scripts/check-v3-workstation-dispatch-isomorphism.mjs"]))
`;
}

function buildGoodWorkstationDispatch() {
  return `// fixture
mod descriptor;
mod decision;
mod outcome;
mod runner;
mod brief;
mod proposal;
mod auto_spawn;
#[cfg(test)]
mod tests;
pub(crate) use descriptor::{
    load_task_contract, resolve_contract_path_public, ParsedTaskContract, TaskContractParseError,
};
pub(crate) use decision::{
    evaluate_dispatch_decision, explicit_workstation_dispatch_flag, opt_in_requested,
    DispatchDecision, InferenceContext, WorkstationDispatchSource, INFERABLE_DISPATCH_STRATEGIES,
};
pub(crate) use outcome::{
    outcome_to_response_fields, truncate_brief_preview, SafeDescriptorReason, WorkstationDispatchOutcome,
    SCOPED_COMMIT_POLICY, SCOPED_COMMIT_REQUIRED,
};
pub(crate) use runner::{
    run_workstation_dispatch, run_workstation_dispatch_with_contract,
    run_workstation_dispatch_with_contract_and_trace,
};
pub(crate) use brief::{
    build_task_brief, build_task_brief_with_source, build_task_brief_with_source_and_trace,
    classify_task_kind, BriefTaskKind,
};
pub(crate) use proposal::{
    request_workstation_proposals, WorkstationProposalBundle, WorkstationProposalGate,
};
pub(crate) use auto_spawn::{
    enforce_auto_spawn_preflight, evaluate_workstation_auto_spawn_gate,
    parse_workstation_auto_spawn_input, WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnInput,
    WorkstationAutoSpawnStatus,
};
// Stable wire field names projected by outcome_to_response_fields:
//   "workstation_dispatch_status" "task_brief_preview"
//   "delegated_board_task_id"     "inner_result"
// Stable status values returned by WorkstationDispatchOutcome::status:
//   "dispatched" "dry_run_no_dispatch" "inner_returned_error" "skipped_completion_log_unavailable"
// substrate only ever wraps mission_task_delegate
`;
}

function buildGoodWorkstationRunner() {
  return `// fixture
use crate::slot_orchestrator::project_root::resolve_target_project_root;
use super::descriptor::resolve_contract_path;
use super::{
    build_task_brief_with_source_and_trace, workstation_execution_id, SafeDescriptorReason,
    WorkstationDispatchOutcome,
};
pub(crate) async fn run_workstation_dispatch() {}
pub(crate) async fn run_workstation_dispatch_with_contract() {}
pub(crate) async fn run_workstation_dispatch_with_contract_and_trace() {
    let _ = resolve_target_project_root;
    let _ = resolve_contract_path;
    let _ = build_task_brief_with_source_and_trace;
    let _ = workstation_execution_id;
    let _ = "agent_execution::handle";
    let _ = "\"action\": \"open\"";
    let _ = "mission_task_delegate";
    let _ = "tool_result_payload";
    let _ = "EvidenceEntry::new";
    let _ = "evidence_collector::append";
    let _ = "task_contract_source_path";
    let _ = "session_trace_path";
    let _ = "WorkstationDispatchOutcome::Dispatched";
    let _ = "WorkstationDispatchOutcome::DryRun";
    let _ = "WorkstationDispatchOutcome::InnerError";
    let _ = "WorkstationDispatchOutcome::SafeDescriptor";
    let _ = "SafeDescriptorReason::MalformedTaskContract";
    let _ = "SafeDescriptorReason::CompletionLogUnavailable";
}
`;
}

function buildGoodWorkstationOutcome() {
  return `// fixture
use serde_json::{json, Value};
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
pub(crate) enum SafeDescriptorReason { UnsupportedTarget(String), CompletionLogUnavailable(String) }
pub(crate) fn outcome_to_response_fields() {}
pub(crate) fn extract_inner_board_task_id() {}
pub(crate) fn truncate_brief_preview() {}
pub(crate) const SCOPED_COMMIT_REQUIRED: bool = true;
pub(crate) const SCOPED_COMMIT_POLICY: &str = "enforced-on-complete";
// Stable wire field names projected by outcome_to_response_fields:
//   "workstation_dispatch_status" "task_brief_preview"
//   "delegated_board_task_id"     "inner_result"
// Stable status values returned by WorkstationDispatchOutcome::status:
//   "dispatched" "dry_run_no_dispatch" "inner_returned_error" "skipped_completion_log_unavailable"
`;
}

function buildGoodWorkstationBrief() {
  return `// fixture
use super::{WorkstationDispatchHints, COMMIT_POLICY_SCOPED};
use super::super::plan::AGENT_TEAM_OBJECTIVE_HINT;
pub(crate) enum BriefTaskKind {
    Code,
    ReadOnly,
}
pub(crate) fn classify_task_kind(_: &WorkstationDispatchHints) -> BriefTaskKind {
    BriefTaskKind::ReadOnly
}
pub(crate) fn workstation_execution_id() {}
pub(crate) fn build_task_brief() {
    let _ = COMMIT_POLICY_SCOPED;
    let _ = AGENT_TEAM_OBJECTIVE_HINT;
    let _ = "Execution log:";
    let _ = "execution_id=";
    let _ = "dispatcher pre-opened this MissionD audit log";
    let _ = "Completion handoff (scoped commit)";
    let _ = "forbidden git state mutations: do NOT run \`git stash\`, \`git reset\`, \`git checkout\`, or \`git restore\`";
}
pub(crate) fn build_task_brief_with_source() {}
pub(crate) fn build_task_brief_with_source_and_trace() {
    let _ = "Session trace";
}
`;
}

function buildGoodWorkstationProposal() {
  return `// fixture
use serde_json::{json, Value};
use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
pub(crate) const WORKSTATION_PROPOSAL_FIELDS: &[&str] = &["target", "dispatch_strategy", "objective", "scope"];
pub(crate) const WORKSTATION_PROPOSAL_CAP: usize = 6;
pub(crate) const SONNET_WORKSTATION_PROPOSAL_CALLER: &str = "workstation_dispatch_proposal";
pub(crate) const PROPOSAL_VALID_TARGETS: &[&str] = &["mission_execution", "mission_task_delegate", "mission_flow_run"];
pub(crate) const PROPOSAL_VALID_STRATEGIES: &[&str] = &["resident-lisp", "fresh-code-alignment", "agent-team", "mixed"];
pub(crate) enum WorkstationProposalStatus { NotInvoked, Unavailable, Suggested, NoSuggestions, PlanHintsPresent }
pub(crate) enum WorkstationProposalSafetyStatus { Safe, AmbiguousValue, UnsupportedTarget, InvalidStrategy }
pub(crate) enum WorkstationProposalConfidence { High, Medium, Low }
pub(crate) struct WorkstationProposal;
pub(crate) struct WorkstationProposalBundle;
pub(crate) struct WorkstationProposalGate<'a> { _x: &'a () }
pub(crate) fn build_workstation_proposal_prompt() {}
pub(crate) fn parse_workstation_proposals() {}
pub(crate) fn classify_proposal_safety() {}
pub(crate) async fn request_workstation_proposals() {
    let router_config = RouterRuntimeConfig::load_for_current_dir().unwrap();
    let _ = "V3_BLUEPRINT_CONFIG_ERROR";
    let _ = router_config.queued_sonnet_model;
    let _ = "applied\": false";
    let _ = "auto_spawn\": false";
    let _ = "no fallback to claude -p";
}
`;
}

function buildGoodWorkstationAutoSpawn() {
  return `// fixture
// autonomous workstation true spawn v1
mod gate;
mod hash;
mod input;
mod outcome;
pub(crate) use self::gate::{enforce_auto_spawn_preflight, evaluate_workstation_auto_spawn_gate};
pub(crate) use self::hash::{compute_workstation_proposal_hash, WorkstationProposalHashStatus};
pub(crate) use self::input::{
    parse_workstation_auto_spawn_input, WorkstationAutoSpawnInput,
    AUTO_SPAWN_INVALID_PARAM, AUTO_SPAWN_MISSING_PROPOSAL_HASH,
    AUTO_SPAWN_PROPOSAL_HASH_MISMATCH,
};
pub(crate) use self::outcome::{WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus};
`;
}

function buildGoodWorkstationAutoSpawnInput() {
  return `// fixture
use super::*;
pub(crate) const AUTO_SPAWN_MISSING_PROPOSAL_HASH: &str = "AUTO_SPAWN_MISSING_PROPOSAL_HASH";
pub(crate) const AUTO_SPAWN_PROPOSAL_HASH_MISMATCH: &str = "AUTO_SPAWN_PROPOSAL_HASH_MISMATCH";
pub(crate) const AUTO_SPAWN_INVALID_PARAM: &str = "AUTO_SPAWN_INVALID_PARAM";
pub(crate) struct WorkstationAutoSpawnInput;
pub(crate) fn parse_workstation_auto_spawn_input() {}
fn parser_error() { proposal_json_kind; }
`;
}

function buildGoodWorkstationAutoSpawnHash() {
  return `// fixture
use super::*;
pub(crate) enum WorkstationProposalHashStatus { Matches }
pub(crate) fn compute_workstation_proposal_hash() {}
fn hash_impl() { Sha256; }
`;
}

function buildGoodWorkstationAutoSpawnOutcome() {
  return `// fixture
use super::*;
pub(crate) enum WorkstationAutoSpawnStatus { Spawned, SkippedUnavailable, SkippedSubstrateRefused }
pub(crate) struct WorkstationAutoSpawnGateOutcome;
pub(crate) fn to_response_json() {
    json!({});
    let _ = "WorkstationAutoSpawnStatus::Spawned";
    let _ = "WorkstationAutoSpawnStatus::SkippedUnavailable";
    let _ = "WorkstationAutoSpawnStatus::SkippedSubstrateRefused";
}
`;
}

function buildGoodWorkstationAutoSpawnGate() {
  return `// fixture
use super::*;
pub(crate) fn enforce_auto_spawn_preflight() {}
pub(crate) fn evaluate_workstation_auto_spawn_gate() {
    let _ = "WorkstationAutoSpawnStatus::Spawned";
    let _ = "WorkstationAutoSpawnStatus::SkippedUnavailable";
    let _ = "WorkstationAutoSpawnStatus::SkippedSubstrateRefused";
    let _ = "WorkstationProposalHashStatus::Matches";
    let _ = "mission_task_delegate";
    let _ = "task_contract_path";
    let _ = "preflight_status_acceptable";
}
`;
}

function buildGoodWorkstationTests() {
  return `// fixture
fn fixture_plan() {}
fn opt_in_requires_explicit_arg_or_plan_hint() {}
fn build_task_brief_includes_canonical_sections_and_scoped_commit_reminder() {}
fn parse_task_contract_extracts_every_consumed_field() {}
fn parse_workstation_proposals_accepts_canonical_envelope() {}
fn wave22_05_gate_happy_path_returns_spawned() {}
fn smoke_wave22_07_workstation_auto_spawn_gate_rejects_missing_hash_accepts_fixture_hash() {}
fn build_task_brief_with_source_and_trace_renders_session_trace_block_when_path_supplied() {}
fn brief_forbids_hidden_git_state_mutations_unless_owned() {}
`;
}

function buildGoodWorkstationDescriptor() {
  return `// fixture
pub(crate) struct ParsedTaskContract {
    session_trace_path: Option<String>,
}
pub(crate) enum TaskContractParseError { MissingRequired(&'static str) }
pub(crate) fn parse_task_contract(src: &str) -> Result<ParsedTaskContract, TaskContractParseError> {
    let _schema = "missiond.task-contract.v1";
    let _field = "session-trace-path";
    let _ = src;
    Ok(ParsedTaskContract { session_trace_path: None })
}
pub(crate) fn load_task_contract() {}
pub(super) fn resolve_contract_path() {}
pub(crate) fn resolve_contract_path_public() {}
`;
}

function buildGoodWorkstationDecision() {
  return `// fixture
use serde_json::Value;
pub(crate) fn opt_in_requested(_: &Value, _: bool) -> bool { false }
pub(crate) enum WorkstationDispatchSource { ExplicitArg, PlanHint, Inferred, Disabled, NotApplicable }
impl WorkstationDispatchSource {
    fn as_str(self) -> &'static str {
        match self {
            WorkstationDispatchSource::ExplicitArg => "explicit_arg",
            WorkstationDispatchSource::Inferred => "inferred",
            _ => "not_applicable",
        }
    }
}
pub(crate) struct DispatchDecision { source: WorkstationDispatchSource }
pub(crate) const INFERABLE_DISPATCH_STRATEGIES: &[&str] = &["agent-team"];
pub(crate) struct InferenceContext<'a> { target: &'a str }
pub(crate) fn explicit_workstation_dispatch_flag(_: &Value) -> Option<bool> { None }
pub(crate) fn evaluate_dispatch_decision(_: &Value, _: bool, ctx: &InferenceContext<'_>) -> DispatchDecision {
    let _ = ctx.target == "mission_task_delegate";
    DispatchDecision { source: WorkstationDispatchSource::Inferred }
}
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
